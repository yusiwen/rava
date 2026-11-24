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
    /// A `String` native received a null reference where the JDK raises
    /// `NullPointerException`.
    ///
    /// M2.9 has no Java exception-object synthesis, so the condition is a typed VM error
    /// that the caller reports instead of a `Throwable`.
    NullStringArgument {
        /// The Java method that rejected the argument, such as `String.concat`.
        method: &'static str,
    },
    /// A `String` index or range lies outside the string's UTF-16 length, where the JDK
    /// raises `StringIndexOutOfBoundsException`.
    ///
    /// `index` is the offending argument and `length` the receiver's UTF-16 length. Like
    /// [`VmError::NullStringArgument`], this is a typed error rather than a synthesized
    /// exception object.
    StringIndexOutOfBounds { index: i32, length: usize },
    /// A `String` operation would produce a lone UTF-16 surrogate, which the heap's
    /// `String` storage (valid UTF-8) cannot represent.
    ///
    /// A supplementary character is a surrogate pair and works normally; the error is
    /// limited to an operation such as `substring(0, 1)` that splits a pair.
    UnpairedSurrogate {
        /// The Java method whose result would hold the lone surrogate.
        method: &'static str,
    },
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
            Self::NullStringArgument { method } => {
                write!(f, "{method} received a null String argument")
            }
            Self::StringIndexOutOfBounds { index, length } => write!(
                f,
                "String index {index} is outside a UTF-16 length of {length}"
            ),
            Self::UnpairedSurrogate { method } => write!(
                f,
                "{method} would produce an unpaired UTF-16 surrogate, which the heap's String storage cannot represent"
            ),
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
