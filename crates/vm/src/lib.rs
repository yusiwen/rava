//! Runtime model: class identity, class sources, heap handles and shared state.
//!
//! M0 fixes the seams here; the interpreter and the class library consume them.

#![forbid(unsafe_code)]

pub mod error;
pub mod heap;
pub mod hooks;
pub mod loader;
pub mod memory;
pub mod shared;
pub mod source;
pub mod thread;
pub mod value;

pub use error::VmError;
pub use heap::{
    Array, ArrayKind, FieldLayout, FieldSlot, Heap, HeapError, HeapObject, Instance, ObjRef,
};
pub use hooks::{ClassUnloadHook, Hooks, ReferenceHook};
pub use loader::{ClassEntry, ClassId, ClassState, LoaderId};
pub use memory::{MemoryError, MemoryRegion};
pub use shared::{LockHeld, NativeFn, NativeKey, Shared, SharedLock};
pub use source::{ClassSource, DirectorySource, JarSource};
pub use thread::{FrameRootsGuard, ThreadState};
pub use value::Value;
