//! `java.util.ServiceLoader` state (M4.2).
//!
//! A `ServiceLoader` is an ordinary instance of the C1 `java.util.ServiceLoader` class:
//! its service type, defining loader and provider classes are Rust state attached by
//! [`ObjRef`], exactly as the M4.1 URL and stream attachments are. The state holds only
//! [`ClassId`] and [`LoaderId`] values, never heap handles, so it owns no Java references
//! and [`crate::Shared::roots`] does not report its keys. The loader object itself stays
//! reachable through the Java reference the program holds, and the provider classes stay
//! defined in the class table the loader names.
//!
//! The iterator state carries the same provider list plus a cursor. It is separate from
//! [`crate::IteratorObject`] because instantiating a provider re-enters the interpreter
//! and therefore lives in `rava-interp`; the VM-side iterator object remains the ordinary
//! snapshot shape. [`crate::collections`] still owns the snapshot `Iterator` natives;
//! `rava_interp::service_loader` overrides `Iterator.hasNext`/`next` and falls back to
//! those natives for every iterator without service state.

use std::collections::HashMap;

use crate::heap::ObjRef;
use crate::loader::{ClassId, LoaderId};

/// The service type, the loader its configuration was read through and the provider
/// classes in configuration order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceLoaderState {
    pub service: ClassId,
    pub loader: LoaderId,
    pub providers: Vec<ClassId>,
}

/// One lazy iteration: the providers and the next index `next` will instantiate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceIteratorState {
    pub service: ClassId,
    pub loader: LoaderId,
    pub providers: Vec<ClassId>,
    pub position: usize,
}

/// The `ObjRef`-keyed side table for `ServiceLoader` instances and their iterators.
///
/// Neither map owns a Java reference: the keys identify heap objects, and the values only
/// name classes and loaders. There is no collector yet, so an entry lives as long as the
/// `Shared`.
#[derive(Debug, Default)]
pub struct ServiceState {
    loaders: HashMap<ObjRef, ServiceLoaderState>,
    iterators: HashMap<ObjRef, ServiceIteratorState>,
}

impl ServiceState {
    /// Attach state to a `ServiceLoader` instance created by `ServiceLoader.load`.
    pub fn insert_loader(&mut self, reference: ObjRef, state: ServiceLoaderState) {
        self.loaders.insert(reference, state);
    }

    /// The loader state behind a receiver, if `load` created it.
    pub fn loader(&self, reference: ObjRef) -> Option<&ServiceLoaderState> {
        self.loaders.get(&reference)
    }

    /// Attach lazy iteration state to an iterator created by `ServiceLoader.iterator`.
    pub fn insert_iterator(&mut self, reference: ObjRef, state: ServiceIteratorState) {
        self.iterators.insert(reference, state);
    }

    /// The iteration state behind a receiver, if it came from a `ServiceLoader`.
    pub fn iterator(&self, reference: ObjRef) -> Option<&ServiceIteratorState> {
        self.iterators.get(&reference)
    }

    /// Advance a service iterator past the provider `next` just instantiated.
    ///
    /// Returns the new position, or `None` when the iterator was already past its last
    /// provider. The caller instantiates first and advances only on success, so a failing
    /// constructor leaves the cursor on the provider it attempted.
    pub fn advance_iterator(&mut self, reference: ObjRef) -> Option<usize> {
        let state = self.iterators.get_mut(&reference)?;
        if state.position >= state.providers.len() {
            return None;
        }
        state.position += 1;
        Some(state.position)
    }
}
