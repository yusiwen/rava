//! Runtime model: class identity, class sources, heap handles, shared state, verifier
//! dataflow and the metadata dump.
//!
//! M0 fixes the seams here; the interpreter and the class library consume them.

#![forbid(unsafe_code)]

pub mod annotation;
pub mod boxed;
pub mod collections;
pub mod error;
pub mod heap;
pub mod hooks;
pub mod instance;
pub mod library;
pub mod loader;
pub mod maps;
pub mod memory;
pub mod metadata;
pub mod object;
pub mod reflection;
pub mod shared;
pub mod source;
pub mod string;
pub mod string_concat;
pub mod thread;
pub mod value;
pub mod verify;

pub use error::VmError;
pub use heap::{
    AnnotationObject, Array, ArrayClass, ArrayKind, BoxedPrimitive, ClassRef, EnumConstant,
    FieldLayout, FieldSlot, Heap, HeapError, HeapObject, Instance, LambdaObject, LambdaTarget,
    LambdaTargetKind, MemberKind, ObjRef, PrimitiveKind, ReflectMember,
};
pub use hooks::{ClassUnloadHook, Hooks, ReferenceHook};
pub use loader::{ClassEntry, ClassId, ClassState, LoaderId};
pub use memory::{MemoryError, MemoryRegion};
pub use metadata::dump_class;
pub use rava_classfile::{ClassMetadata, ElementValue};
pub use shared::{LockHeld, NativeFn, NativeKey, Shared, SharedLock};
pub use source::{ClassPathSource, ClassSource, DirectorySource, JarSource};
pub use thread::{FrameRootsGuard, ThreadState};
pub use value::Value;
pub use verify::{
    CodeAttribute, ExceptionHandler, VerifiedMethod, VerifyError, VerifyFrame, parse_code,
    verify_method,
};
