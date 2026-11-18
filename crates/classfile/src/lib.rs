//! Class file parsing for the rava Java runtime.
//!
//! Scope is fixed by ADR-0002 (this crate parses, and depends on nothing internal) and
//! ADR-0003 (bytecode decoding belongs to `rava-vm`, not here).
//!
//! Two properties are deliberate:
//!
//! - **Forward compatible.** Unknown attributes are kept as raw bytes and an unknown
//!   constant-pool tag is reported as [`Error::Unsupported`], never as a failure for
//!   merely being new. The class file version is recorded, never enforced.
//! - **No execution knowledge.** There is no instruction decoding, no verification and
//!   no class path concept here.

pub mod assembler;
pub mod builder;
pub mod constant_pool;
pub mod error;
pub mod model;
pub mod mutf8;
pub mod reader;

pub use assembler::{Assembler, Label};
pub use builder::ClassBuilder;
pub use constant_pool::{Constant, ConstantPool, ReferenceKind};
pub use error::Error;
pub use model::{
    Attribute, BootstrapMethod, ClassFile, Field, Method, ModuleExport, ModuleInfo, ModuleProvides,
    ModuleRequires,
};
