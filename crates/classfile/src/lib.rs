//! Class-file parsing for the rava Java runtime.
//!
//! This crate parses and depends on no other rava crate; bytecode decoding and
//! execution belong to the interpreter/runtime crates.
//!
//! Two properties are deliberate:
//!
//! - **Forward compatible.** Unknown attributes are kept as raw bytes and an unknown
//!   constant-pool tag is reported as [`Error::Unsupported`], never as a failure for
//!   merely being new. The class-file version is recorded, never enforced.
//! - **No execution knowledge.** There is no instruction decoding, no verification and
//!   no class path concept here.
//!
//! The parser keeps attributes as raw bytes; [`metadata`] is the typed C1 view on top of
//! it (ADR-0006), and [`metadata::dump`] renders that view as stable text.

#![forbid(unsafe_code)]

pub mod assembler;
pub mod builder;
pub mod constant_pool;
pub mod error;
pub mod metadata;
pub mod model;
pub mod mutf8;
pub mod reader;

pub use assembler::{Assembler, Label};
pub use builder::ClassBuilder;
pub use constant_pool::{Constant, ConstantPool, ReferenceKind};
pub use error::Error;
pub use metadata::{
    Annotation, AnnotationValue, ClassMetadata, ConstantValue, ElementValue, EnclosingMethod,
    FieldMetadata, InnerClass, MethodMetadata, MethodNameAndDescriptor, MethodParameter,
    RawAttribute, RecordComponent, TypeAnnotation, TypeAnnotationTarget, TypePathEntry,
    TypePathKind,
};
pub use model::{
    Attribute, BootstrapMethod, ClassFile, Field, Method, ModuleExport, ModuleInfo, ModuleProvides,
    ModuleRequires,
};
