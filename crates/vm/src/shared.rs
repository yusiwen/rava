//! The shared runtime state.
//!
//! `Shared` is split into independently lockable parts: the class table and the
//! heap. Per-thread state lives in [`crate::ThreadState`]; a Java call never holds
//! one of these locks.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, RwLock};

use crate::VmError;
use crate::heap::Heap;
use crate::loader::{ClassEntry, ClassId, ClassTable, LoaderId};
use crate::source::ClassSource;
use crate::thread::ThreadState;
use crate::value::Value;
use std::sync::Arc;

pub type NativeFn = fn(&Shared, &ThreadState, LoaderId, &[Value]) -> Result<Option<Value>, VmError>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NativeKey {
    pub owner: String,
    pub name: String,
    pub descriptor: String,
}

pub struct Shared {
    classes: RwLock<ClassTable>,
    heap: Mutex<Heap>,
    natives: RwLock<HashMap<NativeKey, NativeFn>>,
    stdout: Mutex<Vec<u8>>,
}

impl Shared {
    pub fn with_bootstrap(source: Arc<dyn ClassSource>) -> Self {
        Self {
            classes: RwLock::new(ClassTable::with_bootstrap(source)),
            heap: Mutex::new(Heap::new()),
            natives: RwLock::new(HashMap::new()),
            stdout: Mutex::new(Vec::new()),
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

    pub fn register_native(
        &self,
        owner: impl Into<String>,
        name: impl Into<String>,
        descriptor: impl Into<String>,
        function: NativeFn,
    ) {
        self.natives
            .write()
            .expect("native table lock poisoned")
            .insert(
                NativeKey {
                    owner: owner.into(),
                    name: name.into(),
                    descriptor: descriptor.into(),
                },
                function,
            );
    }

    pub fn native(&self, owner: &str, name: &str, descriptor: &str) -> Option<NativeFn> {
        self.natives
            .read()
            .expect("native table lock poisoned")
            .get(&NativeKey {
                owner: owner.to_owned(),
                name: name.to_owned(),
                descriptor: descriptor.to_owned(),
            })
            .copied()
    }

    pub fn write_stdout(&self, bytes: &[u8]) {
        self.stdout
            .lock()
            .expect("stdout lock poisoned")
            .extend_from_slice(bytes);
    }

    pub fn stdout(&self) -> String {
        String::from_utf8_lossy(&self.stdout.lock().expect("stdout lock poisoned")).into_owned()
    }
}
