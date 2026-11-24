//! Class identity and loader delegation.
//!
//! Identity is `(defining loader, binary name)`, never a bare name. The class table
//! is keyed by that pair so two loaders can define the same name independently.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use rava_classfile::ClassFile;

use crate::error::VmError;
use crate::source::ClassSource;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LoaderId(pub u32);

impl LoaderId {
    pub const BOOTSTRAP: Self = Self(0);

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ClassId(pub u32);

impl ClassId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ClassKey {
    loader: LoaderId,
    name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassState {
    Uninitialized,
    Initializing,
    Initialized,
    Erroneous,
}

#[derive(Debug)]
pub struct ClassEntry {
    pub id: ClassId,
    pub loader: LoaderId,
    pub name: String,
    pub file: Arc<ClassFile>,
    pub statics: Mutex<std::collections::HashMap<(String, String), crate::value::Value>>,
    pub state: Mutex<ClassState>,
}

#[derive(Debug)]
struct Loader {
    parent: Option<LoaderId>,
    source: Arc<dyn ClassSource>,
}

/// The class table. [`crate::Shared`] owns it behind an `RwLock`; nothing outside
/// this module mutates it directly.
#[derive(Debug)]
pub(crate) struct ClassTable {
    loaders: Vec<Loader>,
    classes: HashMap<ClassKey, ClassId>,
    entries: Vec<Arc<ClassEntry>>,
}

impl ClassTable {
    pub(crate) fn with_bootstrap(source: Arc<dyn ClassSource>) -> Self {
        Self {
            loaders: vec![Loader {
                parent: None,
                source,
            }],
            classes: HashMap::new(),
            entries: Vec::new(),
        }
    }

    pub(crate) fn add_loader(
        &mut self,
        parent: LoaderId,
        source: Arc<dyn ClassSource>,
    ) -> Result<LoaderId, VmError> {
        if parent.index() >= self.loaders.len() {
            return Err(VmError::Loader(format!(
                "parent loader {} does not exist",
                parent.0
            )));
        }
        let id = LoaderId(self.loaders.len() as u32);
        self.loaders.push(Loader {
            parent: Some(parent),
            source,
        });
        Ok(id)
    }

    pub(crate) fn lookup(&self, loader: LoaderId, name: &str) -> Option<ClassId> {
        self.classes
            .get(&ClassKey {
                loader,
                name: name.to_owned(),
            })
            .copied()
    }

    pub(crate) fn entry(&self, id: ClassId) -> Option<&Arc<ClassEntry>> {
        self.entries.get(id.index())
    }

    pub(crate) fn class_count(&self) -> usize {
        self.entries.len()
    }

    pub(crate) fn load_class(&mut self, loader: LoaderId, name: &str) -> Result<ClassId, VmError> {
        self.require_loader(loader)?;
        let chain = self.chain(loader);

        for candidate in &chain {
            if let Some(id) = self.lookup(*candidate, name) {
                return Ok(id);
            }
        }

        // Parent-first delegation: read the parent's source before the child's.
        for candidate in chain.iter().rev() {
            let source = self.loaders[candidate.index()].source.clone();
            if let Some(bytes) = source.read(name)? {
                return self.define_class(*candidate, name, &bytes);
            }
        }

        Err(VmError::ClassNotFound {
            loader,
            name: name.to_owned(),
        })
    }

    pub(crate) fn define_class(
        &mut self,
        loader: LoaderId,
        name: &str,
        bytes: &[u8],
    ) -> Result<ClassId, VmError> {
        self.require_loader(loader)?;
        let key = ClassKey {
            loader,
            name: name.to_owned(),
        };
        if self.classes.contains_key(&key) {
            return Err(VmError::DuplicateClass {
                loader,
                name: name.to_owned(),
            });
        }

        let file = Arc::new(ClassFile::parse(bytes)?);
        let id = ClassId(self.entries.len() as u32);
        self.classes.insert(key, id);
        self.entries.push(Arc::new(ClassEntry {
            id,
            loader,
            name: name.to_owned(),
            file,
            statics: Mutex::new(std::collections::HashMap::new()),
            state: Mutex::new(ClassState::Uninitialized),
        }));
        Ok(id)
    }

    fn require_loader(&self, loader: LoaderId) -> Result<(), VmError> {
        if loader.index() >= self.loaders.len() {
            return Err(VmError::Loader(format!(
                "loader {} does not exist",
                loader.0
            )));
        }
        Ok(())
    }

    fn chain(&self, loader: LoaderId) -> Vec<LoaderId> {
        let mut chain = Vec::new();
        let mut current = Some(loader);
        while let Some(id) = current {
            chain.push(id);
            current = self.loaders[id.index()].parent;
        }
        chain
    }
}
