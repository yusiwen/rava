//! The `rava` command line, as a library.
//!
//! `main.rs` holds the argument parser and the subcommands; this crate also exposes the
//! pieces an integration test needs to drive the VM without a subprocess: the native report
//! (issue #2) and the class-path construction every subcommand shares.

pub mod natives;

use std::path::Path;

use rava_vm::{ClassPath, VmError};

/// The class path a subcommand runs against.
///
/// A JDK home carries its classes in `lib/modules` (ADR-0008), so a home with that file is
/// opened as an image; anything else is a directory pair, which keeps the extracted-image
/// fallback and the fixture directories working.
pub fn class_path(home: &Path, classpath: &Path) -> Result<ClassPath, VmError> {
    if home.join("lib/modules").is_file() {
        ClassPath::from_jdk_home(home.to_path_buf(), classpath.to_path_buf()).map_err(VmError::from)
    } else {
        Ok(ClassPath::new(home.to_path_buf(), classpath.to_path_buf()))
    }
}
