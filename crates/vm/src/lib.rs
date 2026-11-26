//! Runtime model: class identity, class sources, heap handles, shared state, verifier
//! dataflow and the metadata dump.
//!
//! M0 fixes the seams here; the interpreter and the class library consume them.

#![forbid(unsafe_code)]

pub mod annotation;
pub mod arrays;
pub mod atomic;
pub mod beans;
pub mod big_numbers;
pub mod bitset;
pub mod boot;
pub mod boxed;
pub mod charset;
pub mod class_loader;
pub mod collections;
pub mod direct_buffer;
pub mod error;
pub mod exceptions;
pub mod format;
pub mod futures;
pub mod generic;
pub mod heap;
pub mod hooks;
pub mod instance;
pub mod io_file;
pub mod java_time;
pub mod jul;
pub mod library;
pub mod loader;
pub mod locks;
pub mod manifest;
pub mod maps;
pub mod math;
pub mod memory;
pub mod metadata;
pub mod method_handles;
pub mod modules;
pub mod object;
pub mod proxy;
pub mod random;
pub mod references;
pub mod reflection;
pub mod regex;
pub mod resources;
pub mod services;
pub mod shared;
pub mod source;
pub mod string;
pub mod string_concat;
pub mod system;
pub mod thread;
pub mod thread_natives;
pub mod uri;
pub mod value;
pub mod verify;

pub use boot::BootJarSource;
pub use error::VmError;
pub use futures::FutureTaskObject;
pub use heap::{
    AnnotationObject, Array, ArrayClass, ArrayKind, AtomicCell, BitSetObject, BoxedPrimitive,
    ClassRef, CollectorKind, CollectorObject, ConditionState, EnumConstant, EnumerationObject,
    FieldLayout, FieldSlot, GenericArrayType, HandleKind, Heap, HeapError, HeapObject, Instance,
    IteratorObject, LambdaObject, LambdaTarget, LambdaTargetKind, ListObject, LookupObject,
    MapEntry, MapObject, MemberKind, MessageFormatObject, MethodHandleObject, MethodTypeObject,
    ObjRef, ParameterizedType, PrimitiveKind, ReflectMember, SetObject, SinkObject,
    SpliteratorObject, StreamObject, TypeDeclaration, TypeObject, TypeVariableType, WildcardType,
};
pub use hooks::{ClassUnloadHook, Hooks, ReferenceHook};
pub use loader::{ClassEntry, ClassId, ClassState, LoaderId};
pub use memory::{MemoryError, MemoryRegion};
pub use metadata::dump_class;
pub use rava_classfile::{ClassMetadata, ElementValue};
pub use services::{ServiceIteratorState, ServiceLoaderState, ServiceState};
pub use shared::{LockHeld, NativeFn, NativeKey, Shared, SharedLock};
pub use source::{ClassPathSource, ClassSource, DirectorySource, JarSource};
pub use thread::{FrameRootsGuard, MonitorError, ThreadState};
pub use value::Value;
pub use verify::{
    CodeAttribute, ExceptionHandler, VerifiedMethod, VerifyError, VerifyFrame, parse_code,
    verify_method,
};
