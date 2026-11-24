//! Reference and class-unloading hooks.
//!
//! M0 registers the hooks only: there is no collector, so no weak, soft or phantom
//! reference is cleared and no class is unloaded. M5 (ADR-0008) invokes these
//! callbacks from the collector; the registration point is fixed now so the
//! collector does not have to change the heap or object header to add one. The
//! event methods are public so the seam is testable and M5 has a stable entry point.

use std::sync::{Arc, Mutex};

use crate::heap::ObjRef;
use crate::loader::ClassId;

/// A callback invoked when a reference's referent is cleared and the reference is
/// ready to be enqueued on its `ReferenceQueue`.
pub type ReferenceHook = Arc<dyn Fn(ObjRef) + Send + Sync>;

/// A callback invoked when a class is unloaded and its metadata and statics are no
/// longer reachable.
pub type ClassUnloadHook = Arc<dyn Fn(ClassId) + Send + Sync>;

#[derive(Default)]
pub struct Hooks {
    references: Mutex<Vec<ReferenceHook>>,
    classes: Mutex<Vec<ClassUnloadHook>>,
}

impl Hooks {
    pub fn register_reference(&self, hook: impl Fn(ObjRef) + Send + Sync + 'static) {
        self.references
            .lock()
            .expect("reference hook lock poisoned")
            .push(Arc::new(hook));
    }

    pub fn register_class_unload(&self, hook: impl Fn(ClassId) + Send + Sync + 'static) {
        self.classes
            .lock()
            .expect("class unload hook lock poisoned")
            .push(Arc::new(hook));
    }

    /// Report that the collector cleared `reference`. The hook list is cloned before
    /// invocation, so no VM lock is held while a hook runs.
    pub fn reference_cleared(&self, reference: ObjRef) {
        let hooks = self
            .references
            .lock()
            .expect("reference hook lock poisoned")
            .clone();
        for hook in hooks {
            hook(reference);
        }
    }

    /// Report that the collector unloaded `class`. The hook list is cloned before
    /// invocation, so no VM lock is held while a hook runs.
    pub fn class_unloaded(&self, class: ClassId) {
        let hooks = self
            .classes
            .lock()
            .expect("class unload hook lock poisoned")
            .clone();
        for hook in hooks {
            hook(class);
        }
    }

    /// Number of registered reference hooks. Used by tests and diagnostics.
    pub fn reference_hook_count(&self) -> usize {
        self.references
            .lock()
            .expect("reference hook lock poisoned")
            .len()
    }

    /// Number of registered class-unload hooks. Used by tests and diagnostics.
    pub fn class_unload_hook_count(&self) -> usize {
        self.classes
            .lock()
            .expect("class unload hook lock poisoned")
            .len()
    }
}
