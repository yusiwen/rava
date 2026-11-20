//! Implementations of the native methods the hosted class library declares.
//!
//! The class library is the reference one (JDK 21): its class files contain `native` method
//! declarations and no bodies, exactly as a user class would. This crate is where the Rust
//! side lives, keyed by the `(owner, name, descriptor)` triple the class file stores at the
//! call site. The registry itself is in `rava-vm`; nothing here is JNI, and no object layout
//! is visible to C.
//!
//! One module per area, so a slice adds a file instead of editing the registry body:
//!
//! | Module | Owns |
//! | :--- | :--- |
//! | [`system`] | the standard streams, the clock, identity hashes |
//! | [`io`] | `FileDescriptor`, `FileOutputStream` |
//! | [`class`] | `Object.getClass`, the `Class` queries, `Reflection` |
//! | [`misc`] | CDS flags, bit conversions, processor counts |

pub mod class;
pub mod exceptions;
pub mod io;
pub mod misc;
pub mod system;

use rava_vm::{NativeCtx, Shared, Value, VmError};

/// Installs the JCL natives this VM implements.
pub fn register_jcl_natives(shared: &mut Shared) {
    system::register(shared);
    io::register(shared);
    class::register(shared);
    misc::register(shared);
    exceptions::register(shared);
}

/// The answer for a native that only exists because the library calls it at initialization.
pub(crate) fn no_op(_ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    Ok(Value::NULL)
}
