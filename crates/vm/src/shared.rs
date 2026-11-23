//! The shared runtime state.
//!
//! `Shared` is split into independently lockable parts: the class table and the
//! heap. Per-thread state lives in [`crate::ThreadState`]; a Java call never holds
//! one of these locks.

use std::sync::{Mutex, MutexGuard, RwLock};

use crate::VmError;
use crate::heap::Heap;
use crate::loader::{ClassEntry, ClassId, ClassTable, LoaderId};
use crate::source::ClassSource;
use crate::thread::ThreadState;
use std::sync::Arc;

#[derive(Debug)]
pub struct Shared {
    classes: RwLock<ClassTable>,
    heap: Mutex<Heap>,
}

impl Shared {
    pub fn with_bootstrap(source: Arc<dyn ClassSource>) -> Self {
        Self {
            classes: RwLock::new(ClassTable::with_bootstrap(source)),
            heap: Mutex::new(Heap::new()),
        }
    }

    pub fn add_loader(
        &self,
        parent: LoaderId,
        source: Arc<dyn ClassSource>,
    ) -> Result<LoaderId, VmError> {
        self.classes
            .write()
            .expect("class table lock poisoned")
            .add_loader(parent, source)
    }

    pub fn load_class(&self, loader: LoaderId, name: &str) -> Result<ClassId, VmError> {
        self.classes
            .write()
            .expect("class table lock poisoned")
            .load_class(loader, name)
    }

    pub fn define_class(
        &self,
        loader: LoaderId,
        name: &str,
        bytes: &[u8],
    ) -> Result<ClassId, VmError> {
        self.classes
            .write()
            .expect("class table lock poisoned")
            .define_class(loader, name, bytes)
    }

    pub fn lookup(&self, loader: LoaderId, name: &str) -> Option<ClassId> {
        self.classes
            .read()
            .expect("class table lock poisoned")
            .lookup(loader, name)
    }

    pub fn entry(&self, id: ClassId) -> Option<Arc<ClassEntry>> {
        self.classes
            .read()
            .expect("class table lock poisoned")
            .entry(id)
            .cloned()
    }

    pub fn class_count(&self) -> usize {
        self.classes
            .read()
            .expect("class table lock poisoned")
            .class_count()
    }

    /// Lock the heap for a short, scoped operation. Never hold this guard across a
    /// Java call.
    pub fn heap(&self) -> MutexGuard<'_, Heap> {
        self.heap.lock().expect("heap lock poisoned")
    }

    pub fn thread_state(&self, id: u64) -> ThreadState {
        ThreadState::new(id)
    }
}
