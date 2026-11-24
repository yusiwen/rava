//! Errors produced while locating, reading or defining a class.

use std::fmt;
use std::io;
use std::path::PathBuf;

use crate::loader::LoaderId;

#[derive(Debug)]
pub enum VmError {
    /// A class source could not be read.
    Io { path: PathBuf, source: io::Error },
    /// A class-file parse failed.
    ClassFile(rava_classfile::Error),
    /// A jar entry or structure is not usable.
    Jar(String),
    /// A loader or class source is inconsistent.
    Loader(String),
    /// A native implementation failed.
    Native(String),
    /// The same `(loader, name)` was defined twice.
    DuplicateClass { loader: LoaderId, name: String },
    /// No loader in the delegation chain supplied the class.
    ClassNotFound { loader: LoaderId, name: String },
}

impl fmt::Display for VmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "cannot read {}: {source}", path.display()),
            Self::ClassFile(error) => write!(f, "invalid class file: {error}"),
            Self::Jar(detail) => write!(f, "invalid jar: {detail}"),
            Self::Loader(detail) => write!(f, "loader error: {detail}"),
            Self::Native(detail) => write!(f, "native error: {detail}"),
            Self::DuplicateClass { loader, name } => {
                write!(f, "class {name} is already defined in loader {}", loader.0)
            }
            Self::ClassNotFound { loader, name } => {
                write!(f, "class {name} not found from loader {}", loader.0)
            }
        }
    }
}

impl std::error::Error for VmError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::ClassFile(error) => Some(error),
            _ => None,
        }
    }
}

impl From<rava_classfile::Error> for VmError {
    fn from(error: rava_classfile::Error) -> Self {
        Self::ClassFile(error)
    }
}
