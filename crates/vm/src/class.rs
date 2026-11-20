//! Loaded class metadata: what the loader keeps after the class file has been parsed.

use std::collections::HashMap;
use std::sync::Arc;

use crate::code::Code;
use crate::value::Value;
use rava_classfile::ClassFile;

/// `ACC_STATIC` (JVMS Table 4.6-A).
pub const ACC_STATIC: u16 = 0x0008;
/// `ACC_SYNCHRONIZED`: the monitor is held for the whole method body.
pub const ACC_SYNCHRONIZED: u16 = 0x0020;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ClassId(pub u32);

impl ClassId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Initialization state (JVMS §5.5). Single-threaded form: `Initializing` marks a class
/// whose `<clinit>` is running, so a re-entrant initialization skips instead of looping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassState {
    Uninitialized,
    Initializing,
    Initialized,
    /// `<clinit>` threw (JVMS §5.5): later use raises `NoClassDefFoundError`.
    Erroneous,
}

#[derive(Debug, Clone)]
pub struct MethodInfo {
    pub name: String,
    pub descriptor: String,
    pub access_flags: u16,
    pub is_native: bool,
    /// Decoded body; `None` for `native` and `abstract` methods.
    pub code: Option<Arc<Code>>,
    /// `(instruction index, source line)` from the `LineNumberTable`, sorted by index. Empty
    /// when the class was compiled without debug information.
    pub lines: Vec<(u32, u16)>,
    /// The native or intrinsic implementation, resolved at load time from the registry.
    /// `None` for an ordinary Java method, or for a native the registry does not know (which
    /// `invoke` reports by name). An intrinsic is a non-`native` method the VM implements
    /// itself; see [`crate::Shared::intrinsics`].
    pub native: Option<crate::native::NativeFn>,
    /// `Class.name(descriptor)`, built once at load. Every error message and every stack trace
    /// needs it, and building it per call was most of the per-call cost the ceiling experiment
    /// found (S37).
    pub qualified: Arc<str>,
}

impl MethodInfo {
    /// The source line of the instruction at `index`, or the nearest line at or before it.
    pub fn line_at(&self, index: u32) -> u16 {
        match self.lines.binary_search_by_key(&index, |(entry, _)| *entry) {
            Ok(found) => self.lines[found].1,
            Err(0) => self.lines.first().map(|(_, line)| *line).unwrap_or(0),
            Err(position) => self.lines[position - 1].1,
        }
    }
}

/// A resolved `invokevirtual`/`invokeinterface` call site, keyed by receiver class.
///
/// Sound without invalidation: resolution for a class is fixed when the class is linked, and
/// loading a new class can only add a *subclass*, which changes the resolution of the subclass —
/// never of a class already resolved. So a `(call site, receiver class)` entry cannot go stale.
#[derive(Debug, Clone, Default)]
pub struct VirtualCall {
    pub params: usize,
    pub returns_value: bool,
    /// Receiver class -> the method that class resolves the call to.
    pub targets: HashMap<ClassId, (ClassId, usize)>,
}

/// A resolved `invokestatic` call site.
#[derive(Debug, Clone, Copy)]
pub struct StaticCall {
    pub class: ClassId,
    pub method: usize,
    /// Arguments the call site pops, including none for a receiver (it is static).
    pub params: usize,
    /// Whether the method's descriptor has a non-void return.
    pub returns_value: bool,
}

/// One instance field's position in the class's layout.
///
/// The layout is the superclass layout followed by the class's own non-static fields, so a
/// field's index in a superclass is also its index in every subclass. That prefix property
/// is what lets `putfield Rect.w` address the slot of a `Square` instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldSlot {
    pub name: String,
    pub descriptor: String,
}

#[derive(Debug, Clone)]
pub struct FieldInfo {
    pub name: String,
    pub descriptor: String,
    pub access_flags: u16,
}

#[derive(Debug)]
pub struct Class {
    pub name: String,
    /// The parsed class file, kept because constant-pool resolution is lazy: a
    /// `Fieldref` is only turned into a name when the instruction executes.
    ///
    /// The loader fills this for every class it builds, including `java/lang/Object`, which
    /// is the host's own (S2 removed the synthesized one). `None` is only reachable for a
    /// class model constructed without a file.
    pub file: Option<ClassFile>,
    /// The `SourceFile` attribute, used by stack traces.
    pub source_file: Option<String>,
    pub super_name: Option<String>,
    pub super_class: Option<ClassId>,
    /// Resolved interfaces, in declaration order. Needed for default-method resolution and,
    /// later, for `instanceof` and `checkcast`.
    pub interfaces: Vec<ClassId>,
    /// Interface binary names as written in the class file, resolved into `interfaces` once
    /// the class is published.
    pub interface_names: Vec<String>,
    pub methods: Vec<MethodInfo>,
    pub fields: Vec<FieldInfo>,
    /// Every instance field, inherited first; see [`FieldSlot`].
    pub layout: Vec<FieldSlot>,
    pub statics: HashMap<String, Value>,
    /// Resolved `invokestatic` call sites, keyed by constant-pool index. A static call names its
    /// target directly, so it resolves once — the ceiling experiment (S37) found the per-call
    /// resolution allocated three `String`s and scanned the method table every time.
    pub invoke_cache: HashMap<u16, StaticCall>,
    /// Resolved virtual and interface call sites, one entry per call site with a target per
    /// receiver class seen so far (S40).
    pub virtual_cache: HashMap<u16, VirtualCall>,
    /// How many times each method has been invoked, indexed like `methods`. The raw material for
    /// a JIT's trigger policy (G3): a method is a candidate when this crosses a threshold.
    pub invocations: Vec<u64>,
    /// Resolved `invokedynamic` call sites, keyed by constant-pool index, so a call site is
    /// linked once (JVMS §5.4.3.6).
    pub indy_sites: HashMap<u16, crate::indy::CallSiteInfo>,
    /// Resolved `CONSTANT_Dynamic` values, keyed by constant-pool index.
    pub condy: HashMap<u16, Value>,
    pub state: ClassState,
}

impl Class {
    pub fn method_index(&self, name: &str, descriptor: &str) -> Option<usize> {
        self.methods
            .iter()
            .position(|method| method.name == name && method.descriptor == descriptor)
    }

    /// The slot of an instance field declared by this class (or inherited by it).
    pub fn field_slot(&self, name: &str, descriptor: &str) -> Option<usize> {
        self.layout
            .iter()
            .position(|slot| slot.name == name && slot.descriptor == descriptor)
    }

    /// The slot of an instance field by name alone. `Unsafe.objectFieldOffset(Class, String)` has
    /// no descriptor, and Java forbids two fields with the same name in one class.
    pub fn field_slot_by_name(&self, name: &str) -> Option<usize> {
        self.layout.iter().position(|slot| slot.name == name)
    }

    pub fn method(&self, name: &str, descriptor: &str) -> Option<&MethodInfo> {
        self.methods
            .iter()
            .find(|method| method.name == name && method.descriptor == descriptor)
    }
}
