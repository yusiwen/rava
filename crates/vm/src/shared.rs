//! The shared runtime state.
//!
//! `Shared` is split into independently lockable parts: the class table, the heap,
//! the native registry and stdout. Per-thread state lives in [`crate::ThreadState`];
//! a Java call never holds one of these locks. [`Shared::check_no_locks_held`] is the
//! probe the interpreter uses to enforce that invariant at call boundaries.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;
use std::sync::{Mutex, MutexGuard, RwLock};

use crate::VmError;
use crate::heap::{Heap, ObjRef};
use crate::hooks::Hooks;
use crate::loader::{ClassEntry, ClassId, ClassTable, LoaderId};
use crate::source::ClassSource;
use crate::thread::ThreadState;
use crate::value::Value;

pub type NativeFn = fn(&Shared, &ThreadState, LoaderId, &[Value]) -> Result<Option<Value>, VmError>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NativeKey {
    pub owner: String,
    pub name: String,
    pub descriptor: String,
}

/// The independently lockable parts of [`Shared`] the no-lock probe covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SharedLock {
    ClassTable,
    Heap,
    NativeRegistry,
    Stdout,
}

impl fmt::Display for SharedLock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ClassTable => write!(f, "class table"),
            Self::Heap => write!(f, "heap"),
            Self::NativeRegistry => write!(f, "native registry"),
            Self::Stdout => write!(f, "stdout"),
        }
    }
}

/// A [`SharedLock`] that was held when a Java call boundary was probed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LockHeld {
    pub lock: SharedLock,
}

impl fmt::Display for LockHeld {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the {} lock is held across a Java call", self.lock)
    }
}

impl std::error::Error for LockHeld {}

pub struct Shared {
    classes: RwLock<ClassTable>,
    heap: Mutex<Heap>,
    natives: RwLock<HashMap<NativeKey, NativeFn>>,
    stdout: Mutex<Vec<u8>>,
    hooks: Hooks,
}

impl Shared {
    pub fn with_bootstrap(source: Arc<dyn ClassSource>) -> Self {
        Self {
            classes: RwLock::new(ClassTable::with_bootstrap(source)),
            heap: Mutex::new(Heap::new()),
            natives: RwLock::new(HashMap::new()),
            stdout: Mutex::new(Vec::new()),
            hooks: Hooks::default(),
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

    /// The reference and class-unloading hook registry. M0 registers callbacks only;
    /// there is no collector and no unloading yet.
    pub fn hooks(&self) -> &Hooks {
        &self.hooks
    }

    /// Collect the VM roots: every non-null class static field plus the roots the
    /// given threads' active frames published.
    ///
    /// The class table is snapshotted before any per-class statics lock is taken, and
    /// each statics lock is released before the next class is visited, so this never
    /// holds one shared lock while another is acquired. This returns the roots only,
    /// not their transitive closure; M0 has no collector.
    pub fn roots(&self, threads: &[&ThreadState]) -> Vec<ObjRef> {
        let entries: Vec<Arc<ClassEntry>> = {
            let classes = self.classes.read().expect("class table lock poisoned");
            classes.entries()
        };

        let mut roots = Vec::new();
        let mut seen = HashSet::new();
        for entry in entries {
            let statics = entry.statics.lock().expect("static field lock poisoned");
            for value in statics.values() {
                let Value::Ref(reference) = *value else {
                    continue;
                };
                if !reference.is_null() && seen.insert(reference) {
                    roots.push(reference);
                }
            }
        }

        for thread in threads {
            for reference in thread.frame_roots() {
                if seen.insert(reference) {
                    roots.push(reference);
                }
            }
        }
        roots
    }

    /// Probe the shared locks before entering a Java or native call. Returns the first
    /// lock that is already held, meaning a VM lock would be held across the call.
    ///
    /// M0 runs one thread, so a held lock is reported as held; the probe cannot yet
    /// distinguish this thread's guard from another thread's, which M5 will need.
    pub fn check_no_locks_held(&self) -> Result<(), LockHeld> {
        // Each probe guard is scoped to its own block: the probe checks one lock at a
        // time and never holds a lock while checking the next.
        {
            let _guard = self.classes.try_write().map_err(|_| LockHeld {
                lock: SharedLock::ClassTable,
            })?;
        }
        {
            let _guard = self.heap.try_lock().map_err(|_| LockHeld {
                lock: SharedLock::Heap,
            })?;
        }
        {
            let _guard = self.natives.try_write().map_err(|_| LockHeld {
                lock: SharedLock::NativeRegistry,
            })?;
        }
        {
            let _guard = self.stdout.try_lock().map_err(|_| LockHeld {
                lock: SharedLock::Stdout,
            })?;
        }
        Ok(())
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
