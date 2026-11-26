//! `rava -jar` — launch the `Main-Class` of an executable jar.
//!
//! An executable jar is an ordinary jar whose `META-INF/MANIFEST.MF` names the main
//! class and, optionally, extra `Class-Path` entries. The manifest reader lives in the
//! CLI because the classpath is a launcher concern; M4.4 reuses it for Spring Boot's
//! nested layout. The classpath is the executable jar first and then every `Class-Path`
//! entry in manifest order, resolved relative to the executable jar's directory. A
//! `Class-Path` entry the JDK would ignore — a missing file, or one that is not a
//! readable directory/jar — is skipped; the executable jar itself is always fatal.
//!
//! This slice launches only a flat executable jar. Nested jars (`BOOT-INF/lib`) and
//! Spring Boot's launcher are M4.4, and there is no `URLClassLoader`.

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use rava_vm::{ClassPathSource, JarSource};

use crate::run;

const MANIFEST_PATH: &str = "META-INF/MANIFEST.MF";

/// The main-attributes section of a jar manifest.
///
/// Attribute names are case-insensitive (the manifest format says so) and stored in their
/// lower-case form; values are preserved as written, with the JDK's continuation rule
/// already applied. Only the first, blank-line-terminated section is read: later
/// `Name:` sections describe individual entries and carry no launcher attributes.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Manifest {
    attributes: HashMap<String, String>,
}

impl Manifest {
    /// Parse a manifest, including `\r\n` line endings and wrapped values.
    ///
    /// A continuation line starts with one space; that space is the marker and is not
    /// part of the value. The 72-byte line length is a writer rule — a reader only needs
    /// the leading-space rule — so a wrapped `Class-Path` joins back into one value,
    /// however the writer chose to split it.
    pub(crate) fn parse(bytes: &[u8]) -> Result<Self, ManifestError> {
        let text = match std::str::from_utf8(bytes) {
            Ok(text) => text,
            Err(error) => {
                let line = bytes[..error.valid_up_to()]
                    .iter()
                    .filter(|byte| **byte == b'\n')
                    .count()
                    + 1;
                return Err(ManifestError::Malformed {
                    line,
                    reason: "the manifest is not valid UTF-8",
                });
            }
        };
        let mut manifest = Self::default();
        let mut current: Option<(String, String)> = None;

        for (index, raw_line) in text.split('\n').enumerate() {
            let line_number = index + 1;
            let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
            if line.is_empty() {
                // The first blank line ends the main attributes. Everything after it is
                // a per-entry section.
                manifest.insert(current.take());
                return Ok(manifest);
            }
            if let Some(continuation) = line.strip_prefix(' ') {
                let Some((_, value)) = current.as_mut() else {
                    return Err(ManifestError::Malformed {
                        line: line_number,
                        reason: "a continuation line has no header to continue",
                    });
                };
                value.push_str(continuation);
                continue;
            }
            let Some((name, value)) = line.split_once(':') else {
                return Err(ManifestError::Malformed {
                    line: line_number,
                    reason: "expected `Name: value`",
                });
            };
            if name.is_empty() {
                return Err(ManifestError::Malformed {
                    line: line_number,
                    reason: "the header name is empty",
                });
            }
            // The manifest format writes one space after the colon. Strip exactly that
            // space so `Class-Path`'s own spaces stay in the value; a header without the
            // space is accepted (some writers omit it).
            let value = value.strip_prefix(' ').unwrap_or(value);
            manifest.insert(current.take());
            current = Some((name.to_ascii_lowercase(), value.to_owned()));
        }

        // A manifest whose last header is not followed by a blank line is still valid.
        manifest.insert(current.take());
        Ok(manifest)
    }

    /// Read and parse `META-INF/MANIFEST.MF` from an already-opened jar.
    pub(crate) fn from_jar(jar: &JarSource) -> Result<Self, ManifestError> {
        let bytes = jar
            .read_entry(MANIFEST_PATH)
            .map_err(|error| ManifestError::Jar(error.to_string()))?;
        let bytes = bytes.ok_or(ManifestError::Missing)?;
        Self::parse(&bytes)
    }

    /// The `Main-Class` attribute, or `None` when it is missing or empty.
    pub(crate) fn main_class(&self) -> Option<&str> {
        self.attribute("main-class")
            .filter(|value| !value.trim().is_empty())
    }

    /// The `Class-Path` entries in manifest order.
    ///
    /// The JDK treats the value as a whitespace-separated list of URLs; the caller
    /// resolves each entry relative to the executable jar.
    pub(crate) fn class_path(&self) -> Vec<&str> {
        self.attribute("class-path")
            .map(|value| value.split_whitespace().collect())
            .unwrap_or_default()
    }

    fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes.get(name).map(String::as_str)
    }

    fn insert(&mut self, entry: Option<(String, String)>) {
        if let Some((name, value)) = entry {
            self.attributes.insert(name, value);
        }
    }
}

/// Why an executable jar's manifest could not provide launcher attributes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ManifestError {
    /// The jar has no `META-INF/MANIFEST.MF` entry.
    Missing,
    /// The entry exists but the jar reader could not return its bytes.
    Jar(String),
    /// The manifest bytes violate the line format at `line` (1-based).
    Malformed { line: usize, reason: &'static str },
}

impl fmt::Display for ManifestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => write!(formatter, "the jar has no {MANIFEST_PATH}"),
            Self::Jar(detail) => write!(formatter, "cannot read {MANIFEST_PATH}: {detail}"),
            Self::Malformed { line, reason } => {
                write!(formatter, "malformed manifest at line {line}: {reason}")
            }
        }
    }
}

impl std::error::Error for ManifestError {}

/// Parsed `rava -jar` options.
struct Options {
    jar: PathBuf,
    stdlib: PathBuf,
    program_args: Vec<String>,
}

enum Parsed {
    Launch(Options),
    Help,
}

pub(crate) fn command(args: &[String]) -> ExitCode {
    let options = match parse(args) {
        Ok(Parsed::Launch(options)) => options,
        Ok(Parsed::Help) => {
            print_usage();
            return ExitCode::SUCCESS;
        }
        Err(message) => {
            eprintln!("error: {message}");
            print_usage();
            return ExitCode::from(2);
        }
    };
    match launch(&options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::from(1)
        }
    }
}

fn launch(options: &Options) -> Result<(), String> {
    let jar = JarSource::open(&options.jar).map_err(|error| {
        format!(
            "cannot open executable jar {}: {error}",
            options.jar.display()
        )
    })?;
    let manifest = Manifest::from_jar(&jar).map_err(|error| {
        format!(
            "cannot read executable jar {}: {error}",
            options.jar.display()
        )
    })?;
    let main_class = manifest.main_class().ok_or_else(|| {
        format!(
            "executable jar {} has no Main-Class in {MANIFEST_PATH}",
            options.jar.display()
        )
    })?;

    // The executable jar is entry zero. The JDK ignores a Class-Path entry it cannot
    // find or open; only the executable jar is fatal. Recording that choice here: a
    // missing helper jar changes which classes are reachable, never whether the
    // launcher starts.
    let mut classpath = ClassPathSource::empty();
    classpath.add_path(&options.jar).map_err(|error| {
        format!(
            "cannot open executable jar {}: {error}",
            options.jar.display()
        )
    })?;
    let base = options
        .jar
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    for entry in manifest.class_path() {
        let path = resolve_class_path_entry(base, entry);
        // A bad entry is skipped, not fatal, exactly like a missing one: this is the
        // launcher's classpath, not a user-supplied `rava run --cp`.
        let _ = classpath.add_path(path);
    }

    run::execute_with(
        &run::Options {
            main_class: main_class.to_owned(),
            stdlib: options.stdlib.clone(),
            // `execute_with` uses the source built above and does not read this field.
            classpath: Vec::new(),
            program_args: options.program_args.clone(),
        },
        classpath,
    )
}

fn parse(args: &[String]) -> Result<Parsed, String> {
    let mut jar = None;
    let mut stdlib = PathBuf::from(run::DEFAULT_STDLIB);
    let mut program_args = Vec::new();
    let mut literal = false;
    let mut index = 0;

    while index < args.len() {
        let argument = args[index].as_str();
        if literal {
            program_args.push(argument.to_owned());
            index += 1;
            continue;
        }
        match argument {
            "--" => {
                literal = true;
                index += 1;
            }
            "-h" | "--help" => return Ok(Parsed::Help),
            "--stdlib" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| "`--stdlib` needs a directory".to_owned())?;
                stdlib = PathBuf::from(value);
                index += 2;
            }
            other if other.starts_with("--stdlib=") => {
                stdlib = PathBuf::from(&other["--stdlib=".len()..]);
                index += 1;
            }
            other if other.starts_with('-') && other.len() > 1 => {
                return Err(format!("unknown option `{other}`"));
            }
            other => {
                if jar.is_some() {
                    program_args.push(other.to_owned());
                } else {
                    jar = Some(PathBuf::from(other));
                }
                index += 1;
            }
        }
    }

    let jar = jar.ok_or_else(|| "missing <jar>".to_owned())?;
    Ok(Parsed::Launch(Options {
        jar,
        stdlib,
        program_args,
    }))
}

/// Resolve one `Class-Path` entry against the executable jar's directory.
///
/// Plain relative and absolute paths are accepted alongside `file:` URLs, because a
/// hand-written manifest commonly uses the plain form. A relative `file:` URL is
/// resolved the same way a relative plain path is.
fn resolve_class_path_entry(base: &Path, entry: &str) -> PathBuf {
    let path = if let Some(url) = entry.strip_prefix("file:") {
        PathBuf::from(percent_decode(strip_file_authority(url)))
    } else {
        PathBuf::from(entry)
    };
    if path.is_absolute() {
        path
    } else {
        base.join(path)
    }
}

/// Drop a `file:` URL's authority, if it has one.
///
/// `file:/x` and `file:///x` both name `/x`; `file://host/x` names `/x` after the host
/// is discarded. A `file:relative` URL keeps its relative path.
fn strip_file_authority(url: &str) -> &str {
    let Some(rest) = url.strip_prefix("//") else {
        return url;
    };
    if rest.starts_with('/') {
        // `file:///x`: the empty authority leaves `//x`; the path starts at the next
        // slash, which `strip_prefix("//")` already produced.
        return rest;
    }
    match rest.find('/') {
        Some(index) => &rest[index..],
        None => "/",
    }
}

/// Decode `%XX` escapes in a `file:` URL path.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(high), Some(low)) =
                (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
            {
                decoded.push((high << 4) | low);
                index += 3;
                continue;
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn print_usage() {
    println!("usage: rava -jar <file> [options] [args...]");
    println!();
    println!("runs the Main-Class named by <file>'s META-INF/MANIFEST.MF.");
    println!();
    println!("options:");
    println!(
        "  --stdlib <dir>    bootstrap class-library directory (default: {})",
        run::DEFAULT_STDLIB
    );
    println!("  --                treat every following argument as a program argument");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_main_class_and_class_path() {
        let manifest = Manifest::parse(
            b"Manifest-Version: 1.0\nMain-Class: demo.JarMain\nClass-Path: lib/helper.jar lib/second.jar\n\n",
        )
        .expect("the manifest parses");
        assert_eq!(manifest.main_class(), Some("demo.JarMain"));
        assert_eq!(
            manifest.class_path(),
            vec!["lib/helper.jar", "lib/second.jar"]
        );
    }

    #[test]
    fn joins_a_wrapped_class_path_and_preserves_the_value() {
        // The writer wraps at 72 bytes; the continuation marker is one leading space.
        let manifest = Manifest::parse(
            b"Manifest-Version: 1.0\r\nMain-Class: demo.JarMain\r\nClass-Path: lib/helper.jar lib/second.jar lib/th\r\n ird.jar\r\n\r\n",
        )
        .expect("the wrapped manifest parses");
        assert_eq!(
            manifest.class_path(),
            vec!["lib/helper.jar", "lib/second.jar", "lib/third.jar"]
        );
    }

    #[test]
    fn accepts_crlf_and_lf_line_endings() {
        for text in [
            "Manifest-Version: 1.0\r\nMain-Class: demo.JarMain\r\n\r\n",
            "Manifest-Version: 1.0\nMain-Class: demo.JarMain\n\n",
        ] {
            let manifest = Manifest::parse(text.as_bytes()).expect("both endings parse");
            assert_eq!(manifest.main_class(), Some("demo.JarMain"));
        }
    }

    #[test]
    fn header_names_are_case_insensitive() {
        let manifest = Manifest::parse(
            b"manifest-version: 1.0\nMAIN-CLASS: demo.JarMain\nclass-path: lib/helper.jar\n\n",
        )
        .expect("case-insensitive names parse");
        assert_eq!(manifest.main_class(), Some("demo.JarMain"));
        assert_eq!(manifest.class_path(), vec!["lib/helper.jar"]);
    }

    #[test]
    fn a_missing_or_empty_main_class_is_none() {
        let missing = Manifest::parse(b"Manifest-Version: 1.0\nClass-Path: lib/helper.jar\n\n")
            .expect("a manifest without Main-Class parses");
        assert_eq!(missing.main_class(), None);
        assert_eq!(missing.class_path(), vec!["lib/helper.jar"]);

        let empty = Manifest::parse(b"Manifest-Version: 1.0\nMain-Class: \n\n")
            .expect("an empty Main-Class parses");
        assert_eq!(empty.main_class(), None);
    }

    #[test]
    fn a_malformed_header_names_its_line() {
        let error = Manifest::parse(b"Manifest-Version: 1.0\nMain-Class demo.JarMain\n")
            .expect_err("a header without a colon is malformed");
        assert_eq!(
            error,
            ManifestError::Malformed {
                line: 2,
                reason: "expected `Name: value`",
            }
        );
        let message = error.to_string();
        assert!(message.contains("line 2"), "message: {message}");
    }

    #[test]
    fn a_continuation_without_a_header_is_malformed() {
        let error = Manifest::parse(b" Manifest-Version: 1.0\n")
            .expect_err("a continuation cannot open the manifest");
        assert_eq!(
            error,
            ManifestError::Malformed {
                line: 1,
                reason: "a continuation line has no header to continue",
            }
        );
    }

    #[test]
    fn per_entry_sections_after_the_blank_line_are_ignored() {
        let manifest = Manifest::parse(
            b"Manifest-Version: 1.0\nMain-Class: demo.JarMain\n\nName: demo/JarMain.class\nSHA-256-Digest: abc\n",
        )
        .expect("the main section parses");
        assert_eq!(manifest.main_class(), Some("demo.JarMain"));
        assert_eq!(manifest.class_path(), Vec::<&str>::new());
    }

    #[test]
    fn a_manifest_without_a_final_blank_line_still_parses() {
        let manifest = Manifest::parse(b"Manifest-Version: 1.0\nMain-Class: demo.JarMain")
            .expect("the final header parses at EOF");
        assert_eq!(manifest.main_class(), Some("demo.JarMain"));
    }

    #[test]
    fn class_path_entries_resolve_relative_absolute_and_file_urls() {
        let base = Path::new("/fixture/lib");
        assert_eq!(
            resolve_class_path_entry(base, "helper.jar"),
            PathBuf::from("/fixture/lib/helper.jar")
        );
        assert_eq!(
            resolve_class_path_entry(base, "../shared/helper.jar"),
            PathBuf::from("/fixture/lib/../shared/helper.jar")
        );
        assert_eq!(
            resolve_class_path_entry(base, "/opt/helper.jar"),
            PathBuf::from("/opt/helper.jar")
        );
        assert_eq!(
            resolve_class_path_entry(base, "file:/opt/helper.jar"),
            PathBuf::from("/opt/helper.jar")
        );
        assert_eq!(
            resolve_class_path_entry(base, "file:///opt/helper.jar"),
            PathBuf::from("/opt/helper.jar")
        );
        assert_eq!(
            resolve_class_path_entry(base, "file://host/opt/helper.jar"),
            PathBuf::from("/opt/helper.jar")
        );
        assert_eq!(
            resolve_class_path_entry(base, "file:helper.jar"),
            PathBuf::from("/fixture/lib/helper.jar")
        );
        assert_eq!(
            resolve_class_path_entry(base, "file:/opt/with%20space.jar"),
            PathBuf::from("/opt/with space.jar")
        );
    }
}
