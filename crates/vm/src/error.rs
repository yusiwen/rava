//! Errors produced while locating, reading or defining a class.

use std::fmt;
use std::io;
use std::path::PathBuf;

use crate::heap::ObjRef;
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
    /// `index` is the offending argument and `length` the receiver's UTF-16 length. The
    /// `java.lang.StringBuilder` natives reuse this variant for `charAt`, `setLength`,
    /// `deleteCharAt` and `insert`, because the builder is a UTF-16 character sequence
    /// too. Like [`VmError::NullStringArgument`], this is a typed error rather than a
    /// synthesized exception object.
    StringIndexOutOfBounds { index: i32, length: usize },
    /// A `java.util.ArrayList` index or insertion point lies outside the list, where the
    /// JDK raises `IndexOutOfBoundsException`.
    ///
    /// `operation` names the Java method, `index` is the offending argument and `length`
    /// the list's size. The JDK's exception-object synthesis does not exist yet, so this
    /// is a typed error.
    CollectionIndexOutOfBounds {
        operation: &'static str,
        index: i32,
        length: usize,
    },
    /// A `String` operation would produce a lone UTF-16 surrogate, which the heap's
    /// `String` storage (valid UTF-8) cannot represent.
    ///
    /// A supplementary character is a surrogate pair and works normally; the error is
    /// limited to an operation such as `substring(0, 1)` that splits a pair.
    UnpairedSurrogate {
        /// The Java method whose result would hold the lone surrogate.
        method: &'static str,
    },
    /// A reflective invocation (`Method.invoke` or `Constructor.newInstance`)
    /// propagated a Java throwable raised by the invoked method.
    ///
    /// The JDK would wrap the throwable in
    /// `java.lang.reflect.InvocationTargetException`; this slice does not synthesize
    /// exception objects (M2.7), so the original throwable crosses the native boundary
    /// unchanged and the interpreter turns it back into `InterpError::Thrown`. The
    /// caller's exception table can therefore catch it.
    Thrown(ObjRef),
    /// An `invokedynamic` call site named a bootstrap method this VM does not implement.
    ///
    /// M2.18 implements `java.lang.invoke.StringConcatFactory` only; every other bootstrap,
    /// including `LambdaMetafactory`, is a typed error rather than a silent no-op.
    UnsupportedBootstrapMethod {
        owner: String,
        name: String,
        descriptor: String,
    },
    /// The same `(loader, name)` was defined twice.
    DuplicateClass { loader: LoaderId, name: String },
    /// No loader in the delegation chain supplied the class.
    ClassNotFound { loader: LoaderId, name: String },
    /// A `Signature` attribute is not a well-formed JVMS generic signature.
    ///
    /// Reflection reports this instead of falling back to the raw type, because a class
    /// file that declares generic metadata and encodes it badly is a real defect.
    Signature(rava_classfile::metadata::SignatureError),
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
            Self::CollectionIndexOutOfBounds {
                operation,
                index,
                length,
            } => write!(
                f,
                "{operation} index {index} is outside a list of size {length}"
            ),
            Self::UnpairedSurrogate { method } => write!(
                f,
                "{method} would produce an unpaired UTF-16 surrogate, which the heap's String storage cannot represent"
            ),
            Self::Thrown(throwable) => write!(f, "reflective invocation threw {throwable}"),
            Self::UnsupportedBootstrapMethod {
                owner,
                name,
                descriptor,
            } => write!(
                f,
                "unsupported invokedynamic bootstrap method {owner}.{name}{descriptor}"
            ),
            Self::DuplicateClass { loader, name } => {
                write!(f, "class {name} is already defined in loader {}", loader.0)
            }
            Self::ClassNotFound { loader, name } => {
                write!(f, "class {name} not found from loader {}", loader.0)
            }
            Self::Signature(error) => write!(f, "invalid generic signature: {error}"),
        }
    }
}

impl std::error::Error for VmError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::ClassFile(error) => Some(error),
            Self::Signature(error) => Some(error),
            _ => None,
        }
    }
}

impl From<rava_classfile::metadata::SignatureError> for VmError {
    fn from(error: rava_classfile::metadata::SignatureError) -> Self {
        Self::Signature(error)
    }
}

impl From<rava_classfile::Error> for VmError {
    fn from(error: rava_classfile::Error) -> Self {
        Self::ClassFile(error)
    }
}
