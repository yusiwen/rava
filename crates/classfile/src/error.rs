use std::fmt;

/// Every way reading or writing a class file can fail.
///
/// Deliberately granular: a malformed or future file must produce a diagnosis, never
/// a panic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The input ended before an item could be read.
    UnexpectedEof { offset: usize, needed: usize },
    /// The first four bytes were not `0xCAFEBABE`.
    BadMagic { found: u32 },
    /// An index points outside the pool, at slot 0, or at the unusable half of a
    /// `long`/`double` entry.
    BadConstantPoolIndex { index: u16, pool_len: u16 },
    /// The entry exists but is not the kind this accessor requires.
    WrongConstantKind { index: u16, expected: &'static str },
    /// The declared `constant_pool_count` does not match what was parsed.
    PoolSizeMismatch { declared: u16, parsed: usize },
    /// A `CONSTANT_Utf8` payload is not valid modified UTF-8.
    BadModifiedUtf8 { index: u16, offset: usize },
    /// A typed attribute decoder ([`crate::metadata`]) rejected an attribute payload.
    ///
    /// `cause` keeps the underlying diagnosis structured, so a caller can tell a
    /// truncated payload from a constant-pool reference of the wrong kind or a
    /// semantically invalid payload.
    BadAttribute { name: String, cause: Box<Error> },
    /// A payload whose length is consistent but whose contents are invalid for the
    /// attribute kind: an unknown `type_annotation` target, a trailing byte, a value out
    /// of range for its tag. Produced inside a typed decoder and surfaced wrapped in
    /// [`Error::BadAttribute`].
    MalformedAttribute { detail: String },
    /// Assembling bytecode failed: an unbound label, or a branch beyond a 16-bit offset.
    InvalidAssembly { detail: String },
    /// A structure this VM does not implement yet.
    Unsupported {
        feature: &'static str,
        detail: String,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEof { offset, needed } => write!(
                f,
                "unexpected end of file: needed {needed} byte(s) at offset {offset}"
            ),
            Self::BadMagic { found } => {
                write!(f, "bad magic 0x{found:08X}, expected 0xCAFEBABE")
            }
            Self::BadConstantPoolIndex { index, pool_len } => write!(
                f,
                "constant pool index {index} is invalid for a pool of {pool_len} slot(s)"
            ),
            Self::WrongConstantKind { index, expected } => {
                write!(f, "constant pool entry {index} is not a {expected}")
            }
            Self::PoolSizeMismatch { declared, parsed } => write!(
                f,
                "constant pool declared {declared} slot(s) but {parsed} were parsed"
            ),
            Self::BadModifiedUtf8 { index, offset } => write!(
                f,
                "entry {index} is not valid modified UTF-8 at byte {offset}"
            ),
            Self::BadAttribute { name, cause } => {
                write!(f, "attribute {name}: {cause}")
            }
            Self::MalformedAttribute { detail } => write!(f, "malformed attribute: {detail}"),
            Self::InvalidAssembly { detail } => write!(f, "invalid assembly: {detail}"),
            Self::Unsupported { feature, detail } => {
                write!(f, "unsupported {feature}: {detail}")
            }
        }
    }
}

impl std::error::Error for Error {}
