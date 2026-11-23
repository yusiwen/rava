//! Runtime model: class identity, class sources, heap seams and shared state.
//!
//! M0 fixes the seams here; the interpreter and the class library consume them.

#![forbid(unsafe_code)]

pub mod error;
pub mod loader;
pub mod source;

pub use error::VmError;
pub use loader::{ClassEntry, ClassId, LoaderId, Shared};
pub use source::{ClassSource, DirectorySource, JarSource};
