//! The target lock: platform pins, the pinned jar set and the `compat target` check.
//!
//! `compat/targets.toml` is the single source of truth for the pinned platform
//! (ADR-0002). `compat/jars.lock` pins the sha256 of every jar in `compat/jars/`.
//! This module reads both and fails when either is missing or they disagree. It does
//! not fetch jars and does not re-hash them; `compat/fetch-jars.sh --check` owns that.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;

use toml::Table;

/// Path of the target lock, relative to the repository root.
pub const TARGET_LOCK: &str = "compat/targets.toml";

/// Only schema 1 is understood; a new layout must bump the number.
const SUPPORTED_SCHEMA: i64 = 1;

/// Default tree for the M2.2 JDK 17 signature stubs when `[c1]` is absent.
pub const DEFAULT_C1_PATH: &str = "stdlib/c1/classes";

/// Default list of manifest owners JDK 17 does not define when `[c1]` is absent.
pub const DEFAULT_C1_UNREFERENCED: &str = "compat/surface/c1-unreferenced.txt";

/// A missing or inconsistent lock, reported to the caller as a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockError(String);

impl LockError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for LockError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// The parsed contents of `compat/targets.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetLock {
    pub schema: i64,
    pub java: String,
    pub class_file: String,
    pub spring_boot: String,
    pub spring_framework: String,
    pub tomcat: String,
    pub app_path: String,
    pub app_version: String,
    pub app_main_class: String,
    pub app_source_sha256: String,
    pub jars_dir: String,
    pub jars_lock: String,
    pub jar_count: usize,
    pub manifest_path: String,
    pub stubs_path: String,
    pub c1_path: String,
    pub c1_unreferenced: String,
}

impl TargetLock {
    /// Parses and validates the target lock.
    pub fn parse(text: &str) -> Result<Self, LockError> {
        let document = text
            .parse::<Table>()
            .map_err(|error| LockError::new(format!("{TARGET_LOCK}: invalid TOML: {error}")))?;

        let schema = integer(&document, "", "schema")?;
        if schema != SUPPORTED_SCHEMA {
            return Err(LockError::new(format!(
                "{TARGET_LOCK}: unsupported schema {schema}, expected {SUPPORTED_SCHEMA}"
            )));
        }

        let platform = table(&document, "platform")?;
        let reference_app = table(&document, "reference_app")?;
        let jars = table(&document, "jars")?;
        let manifest = table(&document, "manifest")?;
        let stubs = table(&document, "stubs")?;
        let c1 = document.get("c1").and_then(|value| value.as_table());
        let (c1_path, c1_unreferenced) = match c1 {
            Some(c1) => (string(c1, "c1", "path")?, string(c1, "c1", "unreferenced")?),
            None => (
                DEFAULT_C1_PATH.to_owned(),
                DEFAULT_C1_UNREFERENCED.to_owned(),
            ),
        };

        let lock = Self {
            schema,
            java: string(platform, "platform", "java")?,
            class_file: string(platform, "platform", "class_file")?,
            spring_boot: string(platform, "platform", "spring_boot")?,
            spring_framework: string(platform, "platform", "spring_framework")?,
            tomcat: string(platform, "platform", "tomcat")?,
            app_path: string(reference_app, "reference_app", "path")?,
            app_version: string(reference_app, "reference_app", "version")?,
            app_main_class: string(reference_app, "reference_app", "main_class")?,
            app_source_sha256: string(reference_app, "reference_app", "source_sha256")?,
            jars_dir: string(jars, "jars", "directory")?,
            jars_lock: string(jars, "jars", "lock")?,
            jar_count: count(jars, "jars", "count")?,
            manifest_path: string(manifest, "manifest", "path")?,
            stubs_path: string(stubs, "stubs", "path")?,
            c1_path,
            c1_unreferenced,
        };
        lock.validate()?;
        Ok(lock)
    }

    fn validate(&self) -> Result<(), LockError> {
        let java_major = self.java.parse::<u32>().map_err(|_| {
            LockError::new(format!(
                "{TARGET_LOCK}: platform.java must be a major version number, got `{}`",
                self.java
            ))
        })?;
        let expected_class_file = format!("{}.0", java_major + 44);
        if self.class_file != expected_class_file {
            return Err(LockError::new(format!(
                "{TARGET_LOCK}: platform.class_file is `{}` but Java {java_major} is class file {expected_class_file}",
                self.class_file
            )));
        }

        for (field, value) in [
            ("platform.spring_boot", &self.spring_boot),
            ("platform.spring_framework", &self.spring_framework),
            ("platform.tomcat", &self.tomcat),
            ("reference_app.version", &self.app_version),
        ] {
            if !is_release(value) {
                return Err(LockError::new(format!(
                    "{TARGET_LOCK}: {field} must be a dotted release number, got `{value}`"
                )));
            }
        }

        if !is_sha256(&self.app_source_sha256) {
            return Err(LockError::new(format!(
                "{TARGET_LOCK}: reference_app.source_sha256 must be 64 lowercase hex digits"
            )));
        }

        for (field, value) in [
            ("reference_app.path", &self.app_path),
            ("jars.directory", &self.jars_dir),
            ("jars.lock", &self.jars_lock),
            ("manifest.path", &self.manifest_path),
            ("stubs.path", &self.stubs_path),
            ("c1.path", &self.c1_path),
            ("c1.unreferenced", &self.c1_unreferenced),
        ] {
            if !is_safe_relative(value) {
                return Err(LockError::new(format!(
                    "{TARGET_LOCK}: {field} must be a relative path inside the repository, got `{value}`"
                )));
            }
        }

        Ok(())
    }
}

/// The parsed contents of `compat/jars.lock`: one sha256 per jar plus the count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JarLock {
    pub count: usize,
    pub entries: BTreeMap<String, String>,
}

impl JarLock {
    /// Parses and validates the jar lock.
    pub fn parse(text: &str) -> Result<Self, LockError> {
        let mut count = None;
        let mut entries = BTreeMap::new();

        for (index, raw) in text.lines().enumerate() {
            let number = index + 1;
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            if let Some((key, value)) = line.split_once('=') {
                if key.trim() != "count" {
                    return Err(LockError::new(format!(
                        "compat/jars.lock:{number}: unknown key `{}`",
                        key.trim()
                    )));
                }
                let parsed = value.trim().parse::<usize>().map_err(|_| {
                    LockError::new(format!(
                        "compat/jars.lock:{number}: count must be a non-negative integer"
                    ))
                })?;
                if count.replace(parsed).is_some() {
                    return Err(LockError::new(format!(
                        "compat/jars.lock:{number}: duplicate count"
                    )));
                }
                continue;
            }

            let mut fields = line.split_whitespace();
            let hash = fields.next().unwrap_or_default();
            let name = fields.next().unwrap_or_default();
            if fields.next().is_some() || !is_sha256(hash) || !is_jar_name(name) {
                return Err(LockError::new(format!(
                    "compat/jars.lock:{number}: expected `<sha256>  <name>.jar`, got `{line}`"
                )));
            }
            if entries
                .insert(name.to_owned(), hash.to_ascii_lowercase())
                .is_some()
            {
                return Err(LockError::new(format!(
                    "compat/jars.lock:{number}: duplicate entry for `{name}`"
                )));
            }
        }

        let count =
            count.ok_or_else(|| LockError::new("compat/jars.lock: missing `count` line"))?;
        if count != entries.len() {
            return Err(LockError::new(format!(
                "compat/jars.lock: count = {count} but the lock has {} entries",
                entries.len()
            )));
        }
        Ok(Self { count, entries })
    }
}

/// Runs `rava compat target [--root <dir>]`.
pub fn command(args: &[String]) -> ExitCode {
    let mut root = None;
    let mut index = 0;
    while index < args.len() {
        let argument = args[index].as_str();
        if argument == "-h" || argument == "--help" {
            print_usage();
            return ExitCode::SUCCESS;
        } else if argument == "--root" {
            let Some(value) = args.get(index + 1) else {
                eprintln!("error: --root needs a directory");
                return ExitCode::from(2);
            };
            root = Some(PathBuf::from(value));
            index += 2;
        } else if let Some(value) = argument.strip_prefix("--root=") {
            root = Some(PathBuf::from(value));
            index += 1;
        } else {
            eprintln!("error: unknown argument `{argument}`");
            print_usage();
            return ExitCode::from(2);
        }
    }

    let root = match root {
        Some(root) => root,
        None => match find_root() {
            Ok(root) => root,
            Err(error) => {
                eprintln!("error: {error}");
                return ExitCode::from(1);
            }
        },
    };

    match report(&root) {
        Ok(output) => {
            print!("{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(1)
        }
    }
}

/// Checks the lock and the jar set below `root`, returning the stable report text.
fn report(root: &Path) -> Result<String, LockError> {
    let (lock, jar_lock) = load(root)?;

    let mut output = String::from("rava compat target\n");
    line(&mut output, "schema", &lock.schema.to_string());
    line(&mut output, "java", &lock.java);
    line(&mut output, "class file", &lock.class_file);
    line(&mut output, "spring boot", &lock.spring_boot);
    line(&mut output, "spring framework", &lock.spring_framework);
    line(&mut output, "tomcat", &lock.tomcat);
    line(
        &mut output,
        "reference app",
        &format!("{} {}", lock.app_path, lock.app_version),
    );
    line(&mut output, "main class", &lock.app_main_class);
    line(&mut output, "source sha256", &lock.app_source_sha256);
    line(&mut output, "jar directory", &lock.jars_dir);
    line(&mut output, "jar lock", &lock.jars_lock);
    line(&mut output, "jar count", &jar_lock.count.to_string());
    line(&mut output, "manifest", &lock.manifest_path);
    line(&mut output, "stubs", &lock.stubs_path);
    line(&mut output, "c1 stubs", &lock.c1_path);
    line(&mut output, "c1 unreferenced", &lock.c1_unreferenced);
    output.push_str("target lock: ok\n");
    Ok(output)
}

/// Loads and validates only `compat/targets.toml`.
///
/// `compat link-check` and `compat stubs` use this: they need the manifest and stub
/// paths, not the downloaded jar set. `compat manifest` uses [`load`], which additionally
/// requires the pinned jars and their lock.
pub fn load_target(root: &Path) -> Result<TargetLock, LockError> {
    let target_path = root.join(TARGET_LOCK);
    let target_text = fs::read_to_string(&target_path).map_err(|error| {
        LockError::new(format!(
            "missing target lock {}: {error}; run from the repository root or pass --root <dir>",
            target_path.display()
        ))
    })?;
    TargetLock::parse(&target_text)
}

/// Loads the target lock and the jar lock and checks that the jar directory matches.
///
/// `compat manifest` reuses this so both commands reject the same broken target.
pub fn load(root: &Path) -> Result<(TargetLock, JarLock), LockError> {
    let lock = load_target(root)?;

    let jars_dir = root.join(&lock.jars_dir);
    if !jars_dir.is_dir() {
        return Err(LockError::new(format!(
            "missing jar directory {}: run `bash compat/fetch-jars.sh`",
            jars_dir.display()
        )));
    }

    let jar_lock_path = root.join(&lock.jars_lock);
    let jar_lock_text = fs::read_to_string(&jar_lock_path).map_err(|error| {
        LockError::new(format!(
            "missing jar lock {}: {error}",
            jar_lock_path.display()
        ))
    })?;
    let jar_lock = JarLock::parse(&jar_lock_text)?;
    if jar_lock.count != lock.jar_count {
        return Err(LockError::new(format!(
            "{TARGET_LOCK} pins {} jars but {} declares {}",
            lock.jar_count, lock.jars_lock, jar_lock.count
        )));
    }

    let present = count_jars(&jars_dir)?;
    if present != jar_lock.count {
        return Err(LockError::new(format!(
            "{} contains {present} jar files but {} declares {}",
            lock.jars_dir, lock.jars_lock, jar_lock.count
        )));
    }

    Ok((lock, jar_lock))
}

/// Counts the top-level `*.jar` files in the pinned jar directory.
fn count_jars(directory: &Path) -> Result<usize, LockError> {
    let entries = fs::read_dir(directory)
        .map_err(|error| LockError::new(format!("cannot read {}: {error}", directory.display())))?;
    let mut count = 0;
    for entry in entries {
        let entry = entry.map_err(|error| {
            LockError::new(format!("cannot read {}: {error}", directory.display()))
        })?;
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("jar") {
            count += 1;
        }
    }
    Ok(count)
}

/// Prints one aligned label/value row of the target report.
fn line(output: &mut String, label: &str, value: &str) {
    output.push_str("  ");
    output.push_str(label);
    output.push_str(&" ".repeat(18usize.saturating_sub(label.len())));
    output.push_str(value);
    output.push('\n');
}

/// Finds the repository root by walking up from the current directory.
pub fn find_root() -> Result<PathBuf, LockError> {
    let current = std::env::current_dir()
        .map_err(|error| LockError::new(format!("cannot read the current directory: {error}")))?;
    for ancestor in current.ancestors() {
        if ancestor.join(TARGET_LOCK).is_file() {
            return Ok(ancestor.to_path_buf());
        }
    }
    Err(LockError::new(format!(
        "no {TARGET_LOCK} found above {}; run from the repository root or pass --root <dir>",
        current.display()
    )))
}

fn print_usage() {
    println!("usage: rava compat target [--root <dir>]");
    println!();
    println!("Prints the pinned target and validates compat/targets.toml and");
    println!("compat/jars.lock against the jar directory. It never fetches jars.");
}

fn table<'a>(document: &'a Table, key: &str) -> Result<&'a Table, LockError> {
    document
        .get(key)
        .and_then(toml::Value::as_table)
        .ok_or_else(|| LockError::new(format!("{TARGET_LOCK}: missing table [{key}]")))
}

fn string(table: &Table, section: &str, key: &str) -> Result<String, LockError> {
    table
        .get(key)
        .and_then(toml::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| LockError::new(format!("{TARGET_LOCK}: missing string {section}.{key}")))
}

fn integer(table: &Table, section: &str, key: &str) -> Result<i64, LockError> {
    table
        .get(key)
        .and_then(toml::Value::as_integer)
        .ok_or_else(|| {
            let prefix = if section.is_empty() {
                String::new()
            } else {
                format!("{section}.")
            };
            LockError::new(format!("{TARGET_LOCK}: missing integer {prefix}{key}"))
        })
}

fn count(table: &Table, section: &str, key: &str) -> Result<usize, LockError> {
    let value = integer(table, section, key)?;
    usize::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| {
            LockError::new(format!(
                "{TARGET_LOCK}: {section}.{key} must be a positive integer"
            ))
        })
}

fn is_release(value: &str) -> bool {
    let parts: Vec<&str> = value.split('.').collect();
    parts.len() >= 2
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_jar_name(value: &str) -> bool {
    value.ends_with(".jar")
        && Path::new(value)
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == value)
}

fn is_safe_relative(value: &str) -> bool {
    let path = Path::new(value);
    !value.is_empty()
        && path.is_relative()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source_hash() -> String {
        "a".repeat(64)
    }

    fn valid_lock() -> String {
        format!(
            r#"
schema = 1

[platform]
java = "17"
class_file = "61.0"
spring_boot = "3.5.16"
spring_framework = "6.2.19"
tomcat = "10.1.55"

[reference_app]
path = "reference-app"
version = "0.1.0"
main_class = "dev.rava.reference.ReferenceApplication"
source_sha256 = "{}"

[jars]
directory = "compat/jars"
lock = "compat/jars.lock"
count = 2

[manifest]
path = "compat/surface/test.txt"

[stubs]
path = "stdlib/classes"
"#,
            source_hash()
        )
    }

    fn valid_jar_lock() -> String {
        format!(
            "count = 2\n\n{}  first.jar\n{}  second.jar\n",
            "1".repeat(64),
            "2".repeat(64)
        )
    }

    #[test]
    fn parses_the_pinned_target() {
        let lock = TargetLock::parse(&valid_lock()).expect("valid lock");
        assert_eq!(lock.schema, 1);
        assert_eq!(lock.java, "17");
        assert_eq!(lock.class_file, "61.0");
        assert_eq!(lock.spring_boot, "3.5.16");
        assert_eq!(lock.spring_framework, "6.2.19");
        assert_eq!(lock.tomcat, "10.1.55");
        assert_eq!(lock.app_path, "reference-app");
        assert_eq!(lock.app_version, "0.1.0");
        assert_eq!(lock.app_source_sha256, source_hash());
        assert_eq!(lock.jars_dir, "compat/jars");
        assert_eq!(lock.jars_lock, "compat/jars.lock");
        assert_eq!(lock.jar_count, 2);
        assert_eq!(lock.manifest_path, "compat/surface/test.txt");
        assert_eq!(lock.stubs_path, "stdlib/classes");
        assert_eq!(lock.c1_path, "stdlib/c1/classes");
        assert_eq!(lock.c1_unreferenced, "compat/surface/c1-unreferenced.txt");
    }

    #[test]
    fn rejects_a_missing_platform_pin() {
        let text = valid_lock().replace("tomcat = \"10.1.55\"\n", "");
        let error = TargetLock::parse(&text).expect_err("missing tomcat");
        assert!(error.to_string().contains("platform.tomcat"), "{error}");
    }

    #[test]
    fn rejects_a_class_file_that_contradicts_java() {
        let text = valid_lock().replace("class_file = \"61.0\"", "class_file = \"65.0\"");
        let error = TargetLock::parse(&text).expect_err("wrong class file");
        assert!(error.to_string().contains("Java 17"), "{error}");
    }

    #[test]
    fn rejects_a_source_hash_that_is_not_sha256() {
        let text = valid_lock().replace(&source_hash(), "not-a-hash");
        let error = TargetLock::parse(&text).expect_err("bad hash");
        assert!(error.to_string().contains("source_sha256"), "{error}");
    }

    #[test]
    fn rejects_a_path_outside_the_repository() {
        let text = valid_lock().replace("directory = \"compat/jars\"", "directory = \"../jars\"");
        let error = TargetLock::parse(&text).expect_err("unsafe path");
        assert!(error.to_string().contains("jars.directory"), "{error}");
    }

    #[test]
    fn parses_the_jar_lock() {
        let lock = JarLock::parse(&valid_jar_lock()).expect("valid jar lock");
        assert_eq!(lock.count, 2);
        assert_eq!(lock.entries.len(), 2);
        assert_eq!(lock.entries["first.jar"], "1".repeat(64));
    }

    #[test]
    fn rejects_a_jar_lock_whose_count_disagrees_with_its_entries() {
        let text = valid_jar_lock().replace("count = 2", "count = 3");
        let error = JarLock::parse(&text).expect_err("count mismatch");
        assert!(error.to_string().contains("count = 3"), "{error}");
        assert!(error.to_string().contains("2 entries"), "{error}");
    }

    #[test]
    fn rejects_a_duplicate_jar_entry() {
        let text = valid_jar_lock().replace("second.jar", "first.jar");
        let error = JarLock::parse(&text).expect_err("duplicate");
        assert!(error.to_string().contains("duplicate entry"), "{error}");
    }

    #[test]
    fn rejects_a_malformed_jar_entry() {
        let text = valid_jar_lock().replace("first.jar", "first.zip");
        let error = JarLock::parse(&text).expect_err("not a jar");
        assert!(error.to_string().contains("expected"), "{error}");
    }
}
