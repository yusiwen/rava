//! `rava compat check` — the local M1 compatibility gate.
//!
//! The command runs four stages in order and stops at the first failure:
//!
//! 1. the target lock and the pinned jar set agree (`compat target`'s check);
//! 2. the committed surface manifest is byte-identical to a fresh generation
//!    (`compat manifest --check`);
//! 3. every manifest record resolves against the committed stub library
//!    (`compat link-check`);
//! 4. a fresh stub library generated into a temporary directory is byte-identical
//!    to the committed `stdlib/classes/`.
//!
//! Stage 4 never writes to the committed stub tree. Stage 2 reads the pinned jars in
//! `compat/jars/`, so the gate needs the already-fetched jar set but never the
//! network; it validates the jar *set*, not the jar *hashes* (that is
//! `bash compat/fetch-jars.sh --check`'s job).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{manifest, stubs, target};

/// Monotonic suffix for temporary directories; the process id is not enough on its own.
static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

/// Prefix of every temporary directory this command creates. `TempTree::drop` only
/// removes a path that carries it.
const TEMP_PREFIX: &str = "rava-compat-check";

/// A stage failed; the message names the stage and the first actionable error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckError(String);

impl CheckError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for CheckError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Runs `rava compat check [--root <dir>]`.
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
        None => match target::find_root() {
            Ok(root) => root,
            Err(error) => {
                eprintln!("error: {error}");
                return ExitCode::from(1);
            }
        },
    };

    match run(&root) {
        Ok(()) => {
            println!("compat check: ok");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(1)
        }
    }
}

/// Runs the four stages in order, printing one pass/fail line per stage.
fn run(root: &Path) -> Result<(), CheckError> {
    println!("rava compat check");

    stage("target lock + jars", || {
        target::load(root)
            .map(|(lock, jar_lock)| {
                format!(
                    "java {}, {}",
                    lock.java,
                    plural(jar_lock.count, "jar", "jars")
                )
            })
            .map_err(|error| error.to_string())
    })?;

    stage("manifest --check", || {
        manifest::check(root)
            .map(|counts| {
                format!(
                    "{}, {}, {}, {}",
                    plural(counts.classes, "class", "classes"),
                    plural(counts.fields, "field", "fields"),
                    plural(counts.methods, "method", "methods"),
                    plural(
                        counts.interface_methods,
                        "interface-method",
                        "interface-methods"
                    )
                )
            })
            .map_err(|error| error.to_string())
    })?;

    stage("link-check", || {
        let report = stubs::link_check(root).map_err(|error| error.to_string())?;
        let members =
            report.counts.fields + report.counts.methods + report.counts.interface_methods;
        if report.unresolved.is_empty() {
            Ok(format!(
                "{}, {}, unresolved 0",
                plural(report.counts.classes, "class", "classes"),
                plural(members, "member record", "member records")
            ))
        } else {
            let first = &report.unresolved[0];
            Err(format!(
                "{} unresolved record(s); first: {first}; run `rava compat stubs`",
                report.unresolved.len()
            ))
        }
    })?;

    stage("stub determinism", || {
        let (lock, surface) = stubs::load_surface(root).map_err(|error| error.to_string())?;
        let committed = root.join(&lock.stubs_path);
        if !committed.is_dir() {
            return Err(format!(
                "missing stub directory {}; run `rava compat stubs`",
                committed.display()
            ));
        }

        let temporary = TempTree::new()?;
        stubs::write_stub_tree(&temporary.path, &lock, &surface)
            .map_err(|error| error.to_string())?;
        let files = compare_trees(&committed, &temporary.path)?;
        Ok(format!(
            "{} byte-identical",
            plural(files, "class file", "class files")
        ))
    })?;

    Ok(())
}

/// Runs one stage, printing `ok (detail)` or `failed`.
fn stage(name: &str, check: impl FnOnce() -> Result<String, String>) -> Result<(), CheckError> {
    match check() {
        Ok(detail) => {
            println!("  {name:<20}ok ({detail})");
            Ok(())
        }
        Err(error) => {
            println!("  {name:<20}failed");
            Err(CheckError::new(format!("{name}: {error}")))
        }
    }
}

/// Renders `N <singular>` or `N <plural>`, so one-item fixtures read well.
fn plural(count: usize, singular: &str, plural: &str) -> String {
    if count == 1 {
        format!("1 {singular}")
    } else {
        format!("{count} {plural}")
    }
}

/// A stub tree entry: `None` for a directory, the file bytes for a regular file.
type Tree = BTreeMap<PathBuf, Option<Vec<u8>>>;

/// Collects the full tree below `root`, keyed by the path relative to `root`.
fn collect_tree(root: &Path) -> Result<Tree, String> {
    let mut entries = BTreeMap::new();
    collect_into(root, root, &mut entries)?;
    Ok(entries)
}

fn collect_into(root: &Path, directory: &Path, entries: &mut Tree) -> Result<(), String> {
    let read = fs::read_dir(directory)
        .map_err(|error| format!("cannot read {}: {error}", directory.display()))?;
    for entry in read {
        let entry =
            entry.map_err(|error| format!("cannot read {}: {error}", directory.display()))?;
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .map_err(|_| format!("{} escaped the stub tree", path.display()))?
            .to_path_buf();
        let kind = entry
            .file_type()
            .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
        if kind.is_dir() {
            entries.insert(relative, None);
            collect_into(root, &path, entries)?;
        } else if kind.is_file() {
            let bytes = fs::read(&path)
                .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
            entries.insert(relative, Some(bytes));
        } else {
            return Err(format!(
                "{} is not a regular file or directory; the stub tree holds class files only",
                path.display()
            ));
        }
    }
    Ok(())
}

/// Byte-compares the committed stub tree with a freshly generated one and returns the
/// number of files that matched.
fn compare_trees(committed: &Path, generated: &Path) -> Result<usize, String> {
    let committed = collect_tree(committed)?;
    let generated = collect_tree(generated)?;

    let keys: BTreeSet<&PathBuf> = committed.keys().chain(generated.keys()).collect();
    let mut files = 0;
    for key in keys {
        match (committed.get(key), generated.get(key)) {
            (Some(None), Some(None)) => {}
            (Some(Some(old)), Some(Some(new))) => {
                files += 1;
                if old != new {
                    return Err(describe_difference(key, old, new));
                }
            }
            (None, Some(_)) => {
                return Err(format!(
                    "committed stub tree is missing {}; run `rava compat stubs`",
                    key.display()
                ));
            }
            (Some(_), None) => {
                return Err(format!(
                    "committed stub tree has {}, which a fresh generation does not; run `rava compat stubs`",
                    key.display()
                ));
            }
            (Some(None), Some(Some(_))) => {
                return Err(format!(
                    "{} is a directory in the committed stub tree but a file when regenerated; run `rava compat stubs`",
                    key.display()
                ));
            }
            (Some(Some(_)), Some(None)) => {
                return Err(format!(
                    "{} is a file in the committed stub tree but a directory when regenerated; run `rava compat stubs`",
                    key.display()
                ));
            }
            (None, None) => {}
        }
    }
    Ok(files)
}

/// Explains the first byte that differs between a committed and a regenerated stub.
fn describe_difference(path: &Path, committed: &[u8], generated: &[u8]) -> String {
    let offset = committed
        .iter()
        .zip(generated)
        .position(|(old, new)| old != new)
        .unwrap_or_else(|| committed.len().min(generated.len()));
    format!(
        "committed stub {} differs from the regenerated stub at byte {offset} ({} vs {} bytes); run `rava compat stubs`",
        path.display(),
        committed.len(),
        generated.len()
    )
}

/// A temporary directory removed when dropped.
///
/// The path is created by [`TempTree::new`] under the OS temporary directory and its
/// name always starts with [`TEMP_PREFIX`]; `drop` refuses to remove anything else.
struct TempTree {
    path: PathBuf,
}

impl TempTree {
    fn new() -> Result<Self, String> {
        let base = std::env::temp_dir();
        for _ in 0..100 {
            let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
            let path = base.join(format!("{TEMP_PREFIX}-{}-{id}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(format!(
                        "cannot create temporary directory {}: {error}",
                        path.display()
                    ));
                }
            }
        }
        Err(format!(
            "cannot create a temporary directory for the stub check under {}",
            base.display()
        ))
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let is_ours = self
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(TEMP_PREFIX));
        if is_ours {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

fn print_usage() {
    println!("usage: rava compat check [--root <dir>]");
    println!();
    println!("Runs the local M1 compatibility gate: target lock and jar set, the");
    println!("byte-exact surface manifest, the zero-unresolved link-check, and stub");
    println!("determinism against a temporary regeneration of stdlib/classes.");
    println!("It reads compat/jars/ but never fetches jars and never rewrites stubs.");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_tree() -> TempTree {
        TempTree::new().expect("temporary tree")
    }

    #[test]
    fn identical_trees_match_and_are_counted() {
        let left = temp_tree();
        let right = temp_tree();
        for tree in [&left, &right] {
            fs::create_dir_all(tree.path.join("java/lang")).expect("create directory");
            fs::write(tree.path.join("java/lang/Object.class"), b"stub").expect("write stub");
        }
        assert_eq!(compare_trees(&left.path, &right.path).unwrap(), 1);
    }

    #[test]
    fn a_missing_file_names_the_path() {
        let committed = temp_tree();
        let generated = temp_tree();
        fs::create_dir_all(committed.path.join("java/lang")).expect("create directory");
        fs::create_dir_all(generated.path.join("java/lang")).expect("create directory");
        fs::write(generated.path.join("java/lang/Object.class"), b"stub").expect("write stub");
        let error = compare_trees(&committed.path, &generated.path).expect_err("missing stub");
        assert!(error.contains("java/lang/Object.class"), "{error}");
        assert!(error.contains("missing"), "{error}");
        assert!(error.contains("rava compat stubs"), "{error}");
    }

    #[test]
    fn an_extra_file_is_rejected() {
        let committed = temp_tree();
        let generated = temp_tree();
        fs::write(committed.path.join("stale.class"), b"stub").expect("write stub");
        let error = compare_trees(&committed.path, &generated.path).expect_err("extra stub");
        assert!(error.contains("stale.class"), "{error}");
        assert!(error.contains("fresh generation does not"), "{error}");
    }

    #[test]
    fn a_byte_difference_reports_the_offset() {
        let committed = temp_tree();
        let generated = temp_tree();
        fs::write(committed.path.join("A.class"), b"abc").expect("write stub");
        fs::write(generated.path.join("A.class"), b"abd").expect("write stub");
        let error = compare_trees(&committed.path, &generated.path).expect_err("byte difference");
        assert!(error.contains("byte 2"), "{error}");
        assert!(error.contains("3 vs 3 bytes"), "{error}");
    }

    #[test]
    fn a_length_difference_reports_the_shorter_side() {
        let committed = temp_tree();
        let generated = temp_tree();
        fs::write(committed.path.join("A.class"), b"abc").expect("write stub");
        fs::write(generated.path.join("A.class"), b"abcd").expect("write stub");
        let error = compare_trees(&committed.path, &generated.path).expect_err("length difference");
        assert!(error.contains("byte 3"), "{error}");
        assert!(error.contains("3 vs 4 bytes"), "{error}");
    }

    #[test]
    fn dropping_a_temporary_tree_removes_it() {
        let path = {
            let tree = temp_tree();
            fs::write(tree.path.join("A.class"), b"stub").expect("write stub");
            tree.path.clone()
        };
        assert!(!path.exists(), "{} must be removed", path.display());
    }

    #[test]
    fn drop_leaves_a_directory_it_does_not_own() {
        let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("rava-not-owned-{}-{id}", std::process::id()));
        assert!(!path.exists());
        fs::create_dir(&path).expect("create foreign directory");
        let tree = TempTree { path: path.clone() };
        drop(tree);
        assert!(
            path.exists(),
            "drop must not remove a directory without the prefix"
        );
        fs::remove_dir(&path).expect("remove foreign directory");
    }
}
