//! Implementations of the native methods the hosted class library declares.
//!
//! The class library is the reference one (JDK 21): its class files contain `native` method
//! declarations and no bodies, exactly as a user class would. This crate is where the Rust
//! side lives, keyed by the `(owner, name, descriptor)` triple the class file stores at the
//! call site. The registry itself is in `rava-vm`; nothing here is JNI, and no object layout
//! is visible to C.
//!
//! The function is deliberately empty at the migration baseline. Entries land with the boot
//! milestone, one per slice, each admitted only when a program needs it.

use rava_vm::Shared;

/// Installs the JCL natives this VM implements.
pub fn register_jcl_natives(_shared: &mut Shared) {}
