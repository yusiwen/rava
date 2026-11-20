//! Runtime model for the rava Java runtime.
//!
//! The seams are fixed by the ADRs:
//!
//! - [`ObjRef`] is an index into a VM-owned heap, never a pointer (ADR-0004).
//! - [`Shared`] and [`ThreadState`] are separate types, so true parallelism later means
//!   deleting one lock rather than rewriting signatures (ADR-0006).
//! - [`JavaCall`] lets `rava-native` invoke Java code without depending on
//!   `rava-interp` (ADR-0002).
//! - Bytecode is decoded into [`Instr`] at load time (ADR-0003).

pub mod class;
pub mod code;
pub mod error;
pub mod format;
pub mod heap;
pub mod image;
pub mod indy;
pub mod loader;
pub mod native;
pub mod resolve;
pub mod value;
pub mod verify;

use std::collections::HashMap;

pub use class::{Class, ClassId, ClassState, FieldInfo, FieldSlot, MethodInfo};
pub use code::{BinOp, Code, Compare, Convert, Instr, UnOp};
pub use error::VmError;
pub use heap::{Array, ArrayKind, HeapObject, Instance, Lambda, ObjRef};
pub use image::{Entry, Image, ImageError};
pub use indy::{CallSite, CallSiteInfo, Component, RecipePart, RecordMethodKind};
pub use loader::ClassPath;
pub use native::{NativeCtx, NativeFn};
pub use value::Value;

/// One frame of a throwable's captured stack trace.
#[derive(Debug, Clone)]
pub struct TraceFrame {
    pub class: String,
    pub method: String,
    pub line: u16,
}

/// One object's monitor.
#[derive(Debug, Default)]
pub struct Monitor {
    /// The thread holding it, or `None` when free.
    pub owner: Option<u64>,
    pub depth: u32,
}

/// What one collection did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CollectionStats {
    /// Heap slots before the sweep.
    pub slots: usize,
    /// Slots reachable from the roots.
    pub live: usize,
    /// Slots available for reuse after the sweep.
    pub freed: usize,
}

/// What a `java.lang.Class` object names: a loaded class or a primitive/array type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClassTarget {
    Loaded(ClassId),
    /// A primitive or array type by descriptor (`I`, `[Ljava/lang/String;`).
    Type(String),
}

/// What a call to [`Shared::begin_initialization`] should do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Initialization {
    /// Run the `<clinit>` at this method index, then finish initialization.
    Run(usize),
    /// Nothing to run: already initialized, or already being initialized on this thread.
    AlreadyDone,
    /// A previous `<clinit>` threw; use must raise `NoClassDefFoundError`.
    Erroneous,
}

/// How a native invokes Java code: the interpreter registers one of these on [`Shared`], which
/// keeps `rava-native` unable to name `rava-interp` (ADR-0002).
pub type Invoker = fn(
    &mut Shared,
    &mut ThreadState,
    &ClassPath,
    ClassId,
    usize,
    Vec<Value>,
) -> Result<Value, VmError>;

/// Builds the initial heap object for a class, or `None` to get a plain instance.
pub type ObjectFactory = fn(class_name: &str, class: ClassId) -> Option<HeapObject>;

/// State shared by every thread: heap, class table, interned strings, natives, and the
/// output sink (ADR-0006).
pub struct Shared {
    pub heap: Vec<HeapObject>,
    /// Slots freed by the last collection, reused by the next allocation.
    pub free: Vec<ObjRef>,
    /// Set by the allocator when the heap reaches its budget. The interpreter collects at
    /// the next instruction boundary, where the frame stack is the complete root set.
    pub pending_gc: bool,
    /// Heap size that triggers the next collection; grows with the live set after each one.
    next_gc: usize,
    pub classes: Vec<Class>,
    pub class_ids: HashMap<String, ClassId>,
    pub natives: HashMap<String, NativeFn>,
    pub interned: HashMap<String, ObjRef>,
    /// Per-object monitors, created on first entry. `depth` is re-entrancy: a thread that
    /// already holds the monitor may enter again, which is what makes a `synchronized` block
    /// calling a `synchronized` method on the same object legal.
    ///
    /// Level 3 (true parallelism) adds an owner and a condition variable; with a single thread
    /// they would be constants (ADR-0012).
    pub monitors: HashMap<ObjRef, Monitor>,
    /// Installed by the interpreter at start-up; natives use it to call Java code.
    pub invoker: Option<Invoker>,
    /// One `java.lang.Class` object per class, so class objects compare by identity.
    pub class_objects: HashMap<ClassId, ObjRef>,
    /// What a class object names: a loaded class or a primitive/array descriptor. This is the
    /// reverse of `class_objects`/`type_classes`; it exists because the host's
    /// `java.lang.Class` has no spare field to hold the identity (OpenJ9 puts a `vmRef` long
    /// there, but its class library is its own).
    pub class_targets: HashMap<ObjRef, ClassTarget>,
    /// Class objects for primitive and array types, keyed by descriptor.
    pub type_classes: HashMap<String, ObjRef>,
    /// Identity hash codes, assigned on first use. `Object.hashCode` must be stable for the
    /// life of the object, and a counter is enough until the object header can hold it.
    pub identity_hashes: HashMap<ObjRef, i32>,
    /// The next identity hash to hand out.
    pub next_identity_hash: i32,
    /// One `Field`/`Method` object per declared member, for the same reason.
    pub field_objects: HashMap<(ClassId, usize), ObjRef>,
    pub method_objects: HashMap<(ClassId, usize), ObjRef>,
    pub constructor_objects: HashMap<(ClassId, usize), ObjRef>,
    /// Boxed `Integer` objects for the JDK-cached range, so `Integer.valueOf(5) ==
    /// Integer.valueOf(5)` holds.
    pub int_cache: HashMap<i32, ObjRef>,
    /// Boxed `Long` objects for the same JDK-cached range.
    pub long_cache: HashMap<i64, ObjRef>,
    /// Boxed `Double` objects. Only the common values are cached; the rest are fresh, which is
    /// what the JDK does for `Double` as well.
    pub double_cache: HashMap<u64, ObjRef>,
    /// Stack traces captured when each throwable was created. The frames are gone once the
    /// exception unwinds, so the trace has to be taken at the throw, not at the report.
    pub traces: HashMap<ObjRef, Vec<TraceFrame>>,
    /// Exceptions suppressed by a resource's `close()` during unwinding, keyed by the primary
    /// exception. Native storage, because `Throwable` has no field for it in this VM and an
    /// array field would need `System.arraycopy` to grow.
    pub suppressed: HashMap<ObjRef, Vec<ObjRef>>,
    /// Whether a class is verified when it loads. `true` by default; tests that build malformed
    /// class files on purpose turn it off (ADR-0011's follow-up).
    pub verify_on_load: bool,
    /// Instructions dispatched since the VM was created. A `u64` add in the loop costs
    /// nothing measurable and makes "how much work was this run" a number instead of an
    /// impression (G8).
    pub steps: u64,
    /// Everything the program wrote to stdout. Buffered because the only consumer is the
    /// end of the run; streaming output is a later concern.
    pub out: Vec<u8>,
    /// Everything the program wrote to stderr, kept separate so the CLI can flush it to the
    /// right stream at the end of the run.
    pub err: Vec<u8>,
    /// How the replacement class library gives its instances storage the VM does not
    /// model: a `StringBuilder` is a Rust `String`, an ordinary object is a field vector.
    /// `new` asks this first, so the VM stays free of library-specific knowledge.
    pub object_factory: Option<ObjectFactory>,
}

impl Shared {
    pub fn new() -> Self {
        Self {
            // Index 0 is the null object, so every ObjRef is a valid index.
            heap: vec![HeapObject::Null],
            free: Vec::new(),
            pending_gc: false,
            verify_on_load: true,
            next_gc: 256,
            classes: Vec::new(),
            class_ids: HashMap::new(),
            natives: HashMap::new(),
            interned: HashMap::new(),
            monitors: HashMap::new(),
            invoker: None,
            class_objects: HashMap::new(),
            class_targets: HashMap::new(),
            type_classes: HashMap::new(),
            field_objects: HashMap::new(),
            method_objects: HashMap::new(),
            constructor_objects: HashMap::new(),
            identity_hashes: HashMap::new(),
            next_identity_hash: 1,
            int_cache: HashMap::new(),
            long_cache: HashMap::new(),
            double_cache: HashMap::new(),
            traces: HashMap::new(),
            suppressed: HashMap::new(),
            steps: 0,
            out: Vec::new(),
            err: Vec::new(),
            object_factory: None,
        }
    }

    pub fn allocate(&mut self, object: HeapObject) -> ObjRef {
        let reference = match self.free.pop() {
            Some(reference) => {
                self.heap[reference.index()] = object;
                reference
            }
            None => {
                let reference = ObjRef(self.heap.len() as u32);
                self.heap.push(object);
                reference
            }
        };
        // A hint, not a collection: the allocator does not see the thread, and frames are
        // roots. The interpreter collects at its next instruction boundary.
        if self.heap.len() >= self.next_gc {
            self.pending_gc = true;
        }
        reference
    }

    /// Mark-sweep over the handle table (ADR-0004, M2).
    ///
    /// Roots are the threads' frames, the static fields of loaded classes, the interned
    /// strings and the suppressed-exception map. Everything reachable survives; everything
    /// else becomes [`HeapObject::Free`] and its slot joins the free list.
    ///
    /// Collection is only correct at a point where every live reference is reachable from
    /// those roots, which is why it is invoked explicitly (`System.gc()`) rather than from
    /// the allocator: allocation does not see the thread.
    pub fn collect(&mut self, thread: &ThreadState) -> CollectionStats {
        self.pending_gc = false;
        let mut marked = vec![false; self.heap.len()];

        let mut worklist: Vec<ObjRef> = Vec::new();
        worklist.push(ObjRef::NULL);
        for frame in &thread.frames {
            for value in frame.locals.iter().chain(frame.stack.iter()) {
                if let Some(reference) = value.as_ref() {
                    worklist.push(reference);
                }
            }
        }
        for class in &self.classes {
            for value in class.statics.values() {
                if let Some(reference) = value.as_ref() {
                    worklist.push(reference);
                }
            }
        }
        worklist.extend(self.interned.values().copied());
        worklist.extend(self.class_objects.values().copied());
        worklist.extend(self.type_classes.values().copied());
        worklist.extend(self.field_objects.values().copied());
        worklist.extend(self.method_objects.values().copied());
        worklist.extend(self.constructor_objects.values().copied());
        worklist.extend(self.int_cache.values().copied());
        worklist.extend(self.long_cache.values().copied());
        worklist.extend(self.double_cache.values().copied());
        for (primary, suppressed) in &self.suppressed {
            worklist.push(*primary);
            worklist.extend(suppressed.iter().copied());
        }

        while let Some(reference) = worklist.pop() {
            let index = reference.index();
            if index >= self.heap.len() || marked[index] {
                continue;
            }
            marked[index] = true;
            match &self.heap[index] {
                HeapObject::Instance(instance) => {
                    worklist.extend(instance.fields.iter().filter_map(|value| value.as_ref()));
                }
                HeapObject::Array(array) => {
                    worklist.extend(array.elements.iter().filter_map(|value| value.as_ref()));
                }
                // A lambda's captured values are references too; missing this arm would free
                // objects that a live lambda still needs.
                HeapObject::Lambda(lambda) => {
                    worklist.extend(lambda.captured.iter().filter_map(|value| value.as_ref()));
                }
                // A class object holds no references: the class is a root by being loaded,
                // and the cache keeps its object alive.
                HeapObject::Null
                | HeapObject::Free
                | HeapObject::Builder(_)
                | HeapObject::FieldRef(_, _)
                | HeapObject::MethodRef(_, _)
                | HeapObject::ConstructorRef(_, _) => {}
            }
        }

        let before = self.heap.len();
        for index in 1..self.heap.len() {
            if marked[index] || matches!(self.heap[index], HeapObject::Free) {
                continue;
            }
            self.heap[index] = HeapObject::Free;
            self.free.push(ObjRef(index as u32));
        }
        let live = marked.iter().filter(|marked| **marked).count();
        // Grow the budget with the live set so a program that keeps more does not collect
        // after every allocation.
        self.next_gc = (live * 2).max(256);
        CollectionStats {
            slots: before,
            live,
            freed: self.free.len(),
        }
    }

    /// Returns the canonical reference for a string constant (JVMS §5.3: `ldc` of the
    /// same string always yields the same object).
    pub fn intern(&mut self, path: &ClassPath, text: &str) -> Result<ObjRef, VmError> {
        if let Some(reference) = self.interned.get(text) {
            return Ok(*reference);
        }
        let reference = self.new_string(path, text.to_owned())?;
        self.interned.insert(text.to_owned(), reference);
        Ok(reference)
    }

    pub fn write_out(&mut self, bytes: &[u8]) {
        self.out.extend_from_slice(bytes);
    }

    /// The text of a `java.lang.String` instance: its `byte[] value` decoded through `coder`.
    ///
    /// Strings are ordinary instances now, so this reads the host class's fields rather than a
    /// VM-private payload.
    pub fn string_text(&self, reference: ObjRef) -> Result<String, VmError> {
        let class = self
            .class_of(reference)
            .ok_or_else(|| VmError::Native(format!("{reference:?} has no runtime class")))?;
        if self.class(class).name != "java/lang/String" {
            return Err(VmError::Native(format!(
                "expected a String, got {}",
                self.class(class).name
            )));
        }
        let value_slot = self
            .class(class)
            .field_slot("value", "[B")
            .ok_or_else(|| VmError::Native("java.lang.String has no `value` field".to_owned()))?;
        let coder_slot = self
            .class(class)
            .field_slot("coder", "B")
            .ok_or_else(|| VmError::Native("java.lang.String has no `coder` field".to_owned()))?;
        let value = self.instance_field(reference, value_slot)?;
        let coder = self
            .instance_field(reference, coder_slot)?
            .as_int()
            .ok_or_else(|| VmError::Native("java.lang.String.coder is not an int".to_owned()))?;
        let bytes = value
            .as_ref()
            .ok_or_else(|| VmError::Native("java.lang.String.value is null".to_owned()))?;
        let bytes: Vec<u8> = self
            .array(bytes)?
            .elements
            .iter()
            .map(|element| {
                element.as_int().map(|byte| byte as u8).ok_or_else(|| {
                    VmError::Native("java.lang.String.value holds a non-byte".to_owned())
                })
            })
            .collect::<Result<_, _>>()?;
        decode_string(&bytes, coder)
    }

    pub fn class(&self, id: ClassId) -> &Class {
        &self.classes[id.index()]
    }

    /// Resolves an exact `(name, descriptor)`: the class and its superclasses first, then the
    /// superinterfaces, which is where a `default` method lives (JVMS §5.4.3.3).
    pub fn resolve_method(
        &self,
        id: ClassId,
        name: &str,
        descriptor: &str,
    ) -> Option<(ClassId, usize)> {
        let mut current = Some(id);
        while let Some(class_id) = current {
            let class = self.class(class_id);
            if let Some(index) = class.method_index(name, descriptor) {
                return Some((class_id, index));
            }
            current = class.super_class;
        }
        let mut visited = Vec::new();
        self.resolve_in_interfaces(id, name, descriptor, &mut visited)
    }

    fn resolve_in_interfaces(
        &self,
        id: ClassId,
        name: &str,
        descriptor: &str,
        visited: &mut Vec<ClassId>,
    ) -> Option<(ClassId, usize)> {
        if visited.contains(&id) {
            return None;
        }
        visited.push(id);

        for interface in &self.class(id).interfaces {
            if let Some(index) = self.class(*interface).method_index(name, descriptor) {
                return Some((*interface, index));
            }
            if let Some(found) = self.resolve_in_interfaces(*interface, name, descriptor, visited) {
                return Some(found);
            }
        }
        None
    }

    /// The `java.lang.Class` object for a class, allocated once and cached.
    ///
    /// It is a real instance of the host's `java.lang.Class` (which is why `assert` and
    /// `getName` work on it), created on first use. The class object for `java.lang.Class`
    /// itself is created the same way, so no bootstrap ordering is special-cased.
    pub fn class_object(&mut self, path: &ClassPath, class: ClassId) -> Result<ObjRef, VmError> {
        if let Some(reference) = self.class_objects.get(&class) {
            return Ok(*reference);
        }
        let class_class = self.load_class(path, "java/lang/Class")?;
        let reference = self.new_instance(class_class);
        self.class_targets
            .insert(reference, ClassTarget::Loaded(class));
        self.class_objects.insert(class, reference);
        Ok(reference)
    }

    /// The `Field` object for a declared field, allocated once and cached.
    pub fn field_object(&mut self, class: ClassId, index: usize) -> ObjRef {
        if let Some(reference) = self.field_objects.get(&(class, index)) {
            return *reference;
        }
        let reference = self.allocate(HeapObject::FieldRef(class, index));
        self.field_objects.insert((class, index), reference);
        reference
    }

    /// The `Method` object for a declared method, allocated once and cached.
    pub fn method_object(&mut self, class: ClassId, index: usize) -> ObjRef {
        if let Some(reference) = self.method_objects.get(&(class, index)) {
            return *reference;
        }
        let reference = self.allocate(HeapObject::MethodRef(class, index));
        self.method_objects.insert((class, index), reference);
        reference
    }

    /// The `Constructor` object for a declared constructor, allocated once and cached.
    pub fn constructor_object(&mut self, class: ClassId, index: usize) -> ObjRef {
        if let Some(reference) = self.constructor_objects.get(&(class, index)) {
            return *reference;
        }
        let reference = self.allocate(HeapObject::ConstructorRef(class, index));
        self.constructor_objects.insert((class, index), reference);
        reference
    }

    /// What a heap reference names, if it is a field or method object.
    pub fn field_ref(&self, reference: ObjRef) -> Option<(ClassId, usize)> {
        match self.heap.get(reference.index()) {
            Some(HeapObject::FieldRef(class, index)) => Some((*class, *index)),
            _ => None,
        }
    }

    pub fn method_ref(&self, reference: ObjRef) -> Option<(ClassId, usize)> {
        match self.heap.get(reference.index()) {
            Some(HeapObject::MethodRef(class, index)) => Some((*class, *index)),
            _ => None,
        }
    }

    pub fn constructor_ref(&self, reference: ObjRef) -> Option<(ClassId, usize)> {
        match self.heap.get(reference.index()) {
            Some(HeapObject::ConstructorRef(class, index)) => Some((*class, *index)),
            _ => None,
        }
    }

    /// The class object for a primitive or array type, allocated once per descriptor.
    pub fn type_class(&mut self, path: &ClassPath, descriptor: &str) -> Result<ObjRef, VmError> {
        if let Some(reference) = self.type_classes.get(descriptor) {
            return Ok(*reference);
        }
        let class_class = self.load_class(path, "java/lang/Class")?;
        let reference = self.new_instance(class_class);
        self.class_targets
            .insert(reference, ClassTarget::Type(descriptor.to_owned()));
        self.type_classes.insert(descriptor.to_owned(), reference);
        Ok(reference)
    }

    /// The descriptor a heap reference names, if it is a primitive or array class object.
    pub fn type_class_ref(&self, reference: ObjRef) -> Option<&str> {
        match self.class_targets.get(&reference) {
            Some(ClassTarget::Type(descriptor)) => Some(descriptor),
            _ => None,
        }
    }

    /// The class id a heap reference names, if it is a class object.
    pub fn class_ref(&self, reference: ObjRef) -> Option<ClassId> {
        match self.class_targets.get(&reference) {
            Some(ClassTarget::Loaded(class)) => Some(*class),
            _ => None,
        }
    }

    /// The most-invoked methods, highest first, as `(Class.method(descriptor), count)`.
    pub fn hot_methods(&self, limit: usize) -> Vec<(String, u64)> {
        let mut entries: Vec<(String, u64)> = self
            .classes
            .iter()
            .flat_map(|class| {
                class
                    .methods
                    .iter()
                    .zip(class.invocations.iter())
                    .map(|(method, count)| {
                        (
                            format!("{}.{}{}", class.name, method.name, method.descriptor),
                            *count,
                        )
                    })
            })
            .filter(|(_, count)| *count > 0)
            .collect();
        entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        entries.truncate(limit);
        entries
    }

    /// Methods invoked at least `threshold` times: the candidates a JIT would compile.
    pub fn hot_candidates(&self, threshold: u64) -> Vec<(String, u64)> {
        self.hot_methods(usize::MAX)
            .into_iter()
            .filter(|(_, count)| *count >= threshold)
            .collect()
    }

    /// Captures the current stack as a trace, innermost frame first.
    pub fn capture_trace(&mut self, thread: &ThreadState) -> Vec<TraceFrame> {
        thread
            .frames
            .iter()
            .rev()
            .map(|frame| {
                let class = self.class(frame.class);
                let method = &class.methods[frame.method];
                TraceFrame {
                    class: class.name.clone(),
                    method: method.name.clone(),
                    // `pc` is the *next* instruction, so the line of the one that threw is the
                    // previous index.
                    line: method.line_at(frame.pc.saturating_sub(1) as u32),
                }
            })
            .collect()
    }

    /// The Java-style report for an uncaught throwable.
    pub fn describe_throwable(&self, object: ObjRef) -> String {
        let class_name = self
            .class_of(object)
            .map(|class| self.class(class).name.clone())
            .unwrap_or_else(|| "java/lang/Throwable".to_owned());
        let mut text = format!(
            "Exception in thread \"main\" {}",
            class_name.replace('/', ".")
        );
        if let Some(message) = self.throwable_message(object) {
            text.push_str(": ");
            text.push_str(&message);
        }
        for frame in self.traces.get(&object).into_iter().flatten() {
            let source = self
                .class_ids
                .get(&frame.class)
                .and_then(|class| self.class(*class).source_file.clone())
                .unwrap_or_else(|| "Unknown Source".to_owned());
            text.push_str(&format!(
                "\n\tat {}.{}({}:{})",
                frame.class.replace('/', "."),
                frame.method,
                source,
                frame.line
            ));
        }
        text
    }

    /// The `message` field of a throwable, if it survived to be read.
    fn throwable_message(&self, object: ObjRef) -> Option<String> {
        let class = self.class_of(object)?;
        let slot = self
            .class(class)
            .field_slot("message", "Ljava/lang/String;")?;
        let value = self.instance_field(object, slot).ok()?;
        match value.as_ref() {
            Some(reference) => self.string_text(reference).ok(),
            None => None,
        }
    }

    /// Acquires the monitor of `reference`, re-entering if this thread already holds it.
    pub fn monitor_enter(&mut self, reference: ObjRef, thread: u64) -> Result<(), VmError> {
        if reference.is_null() {
            return Err(VmError::Unsupported {
                feature: "monitorenter",
                detail: "a null monitor would be a NullPointerException".to_owned(),
            });
        }
        let monitor = self.monitors.entry(reference).or_default();
        match monitor.owner {
            None => {
                monitor.owner = Some(thread);
                monitor.depth = 1;
            }
            Some(owner) if owner == thread => monitor.depth += 1,
            Some(_) => {
                // A real VM blocks here. Level 1 has one thread, so this can only be reached
                // if a future level forgets to wait.
                return Err(VmError::Unsupported {
                    feature: "monitor contention",
                    detail: "blocking for another thread is level 3 (ADR-0012)".to_owned(),
                });
            }
        }
        Ok(())
    }

    /// Releases one level of the monitor.
    pub fn monitor_exit(&mut self, reference: ObjRef, thread: u64) {
        if let Some(monitor) = self.monitors.get_mut(&reference) {
            if monitor.owner == Some(thread) {
                monitor.depth = monitor.depth.saturating_sub(1);
                if monitor.depth == 0 {
                    monitor.owner = None;
                }
            }
        }
    }

    /// Whether `thread` currently holds `reference`'s monitor.
    pub fn holds_monitor(&self, reference: ObjRef, thread: u64) -> bool {
        self.monitors
            .get(&reference)
            .is_some_and(|monitor| monitor.owner == Some(thread) && monitor.depth > 0)
    }

    /// Resolves a constant-pool entry that `ldc`/`ldc2_w` can push.
    ///
    /// A `CONSTANT_Dynamic` is resolved through the same bootstrap machinery as `invokedynamic`,
    /// once, and cached on the class: the bootstrap of a dynamic constant is a pure function of
    /// its arguments, so the value cannot change between executions.
    pub fn resolve_constant(
        &mut self,
        path: &ClassPath,
        class: ClassId,
        index: u16,
    ) -> Result<Value, VmError> {
        if let Some(value) = self.classes[class.index()].condy.get(&index) {
            return Ok(*value);
        }
        let value = match self
            .class(class)
            .file
            .as_ref()
            .and_then(|file| file.constant_pool.get(index).ok())
        {
            Some(rava_classfile::Constant::Dynamic { .. }) => self.resolve_condy(class, index)?,
            Some(rava_classfile::Constant::Class { .. }) => {
                // `Foo.class`: the class object, allocated once so two reads are the same object.
                // A primitive or array type has no class file, so it becomes a type class object.
                let name = crate::resolve::class_constant(self.class(class), index)?;
                if is_type_descriptor(&name) {
                    Value::Ref(self.type_class(path, &name)?)
                } else {
                    let target = self.load_class(path, &name)?;
                    Value::Ref(self.class_object(path, target)?)
                }
            }
            _ => {
                let class_ref = self.class(class);
                match crate::resolve::constant(class_ref, index)? {
                    crate::resolve::ConstantValue::Int(value) => Value::Int(value),
                    crate::resolve::ConstantValue::Float(value) => Value::Float(value),
                    crate::resolve::ConstantValue::Long(value) => Value::Long(value),
                    crate::resolve::ConstantValue::Double(value) => Value::Double(value),
                    crate::resolve::ConstantValue::String(text) => {
                        Value::Ref(self.intern(path, &text)?)
                    }
                }
            }
        };
        self.classes[class.index()].condy.insert(index, value);
        Ok(value)
    }

    /// Evaluates one `CONSTANT_Dynamic`.
    fn resolve_condy(&mut self, class: ClassId, index: u16) -> Result<Value, VmError> {
        let (owner, name) = {
            let file = self.class(class).file.as_ref().ok_or_else(|| {
                VmError::BadConstantPool(format!("{} is synthetic", self.class(class).name))
            })?;
            let pool = &file.constant_pool;
            let bootstrap_index = match pool.get(index)? {
                rava_classfile::Constant::Dynamic {
                    bootstrap_method_attr_index,
                    ..
                } => *bootstrap_method_attr_index,
                other => {
                    return Err(VmError::BadConstantPool(format!(
                        "entry {index} is {other:?}, expected a Dynamic constant"
                    )));
                }
            };
            let bootstrap = file
                .bootstrap_methods
                .get(usize::from(bootstrap_index))
                .ok_or_else(|| {
                    VmError::BadConstantPool(format!(
                        "condy #{index} names bootstrap method {bootstrap_index}, which is absent"
                    ))
                })?;
            crate::indy::bootstrap_target_name(pool, bootstrap)?
        };

        match (owner.as_str(), name.as_str()) {
            ("java/lang/invoke/ConstantBootstraps", "nullConstant") => Ok(Value::NULL),
            _ => Err(VmError::Unsupported {
                feature: "condy bootstrap",
                detail: format!("{owner}.{name}"),
            }),
        }
    }

    /// Resolves an `invokedynamic` call site once and caches it on the class.
    pub fn indy_site(
        &mut self,
        path: &ClassPath,
        class: ClassId,
        index: u16,
    ) -> Result<indy::CallSiteInfo, VmError> {
        if let Some(site) = self.classes[class.index()].indy_sites.get(&index) {
            return Ok(site.clone());
        }
        let resolved = indy::resolve(self, path, class, index)?;
        self.classes[class.index()]
            .indy_sites
            .insert(index, resolved.clone());
        Ok(resolved)
    }

    /// Begins initialization and reports the `<clinit>` that must run first, if any.
    ///
    /// Single-threaded form of JVMS §5.5: a re-entrant initialization sees `Initializing`
    /// and returns `AlreadyDone` rather than looping, and a class whose `<clinit>` threw is
    /// `Erroneous` forever.
    pub fn begin_initialization(&mut self, id: ClassId) -> Initialization {
        match self.classes[id.index()].state {
            ClassState::Uninitialized => {
                match self.classes[id.index()].method_index("<clinit>", "()V") {
                    Some(index) => {
                        self.classes[id.index()].state = ClassState::Initializing;
                        Initialization::Run(index)
                    }
                    None => {
                        self.classes[id.index()].state = ClassState::Initialized;
                        Initialization::AlreadyDone
                    }
                }
            }
            ClassState::Initializing | ClassState::Initialized => Initialization::AlreadyDone,
            ClassState::Erroneous => Initialization::Erroneous,
        }
    }

    pub fn finish_initialization(&mut self, id: ClassId) {
        self.classes[id.index()].state = ClassState::Initialized;
    }

    /// Records that `<clinit>` threw: the class stays erroneous (JVMS §5.5).
    pub fn fail_initialization(&mut self, id: ClassId) {
        self.classes[id.index()].state = ClassState::Erroneous;
    }
}

impl Default for Shared {
    fn default() -> Self {
        Self::new()
    }
}

/// Per-thread execution state: call frames and their operand stacks (ADR-0006).
pub struct ThreadState {
    pub frames: Vec<Frame>,
    /// Frames returned when a call returned, ready to be reused. The ceiling experiment (S37)
    /// found two `Vec` allocations per call were a large part of the call cost; a pooled frame
    /// allocates only when the pool is empty.
    pub pool: Vec<Frame>,
    /// Stable identity, used as the owner of a monitor (ADR-0012).
    pub id: u64,
    /// The `java.lang.Thread` object this state is running, if any. `Thread.currentThread()`
    /// creates one lazily for the main thread.
    pub current: Option<ObjRef>,
}

static NEXT_THREAD_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

impl ThreadState {
    pub fn new() -> Self {
        Self {
            frames: Vec::new(),
            pool: Vec::new(),
            id: NEXT_THREAD_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            current: None,
        }
    }
}

impl Default for ThreadState {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Frame {
    pub class: ClassId,
    pub method: usize,
    pub pc: usize,
    pub locals: Vec<Value>,
    pub stack: Vec<Value>,
}

/// How many returned frames a thread keeps for reuse.
const FRAME_POOL_LIMIT: usize = 64;

impl ThreadState {
    /// A frame for a new call, reusing a pooled one when there is one.
    ///
    /// A reused frame's locals are reset to `null`, not left as they were: a stale reference in a
    /// local is a value the program could read, and (if the pool were ever scanned) a GC root that
    /// should not exist.
    pub fn take_frame(&mut self, class: ClassId, method: usize, max_locals: usize) -> Frame {
        match self.pool.pop() {
            Some(mut frame) => {
                frame.class = class;
                frame.method = method;
                frame.pc = 0;
                frame.locals.clear();
                frame.locals.resize(max_locals, Value::NULL);
                frame.stack.clear();
                frame
            }
            None => Frame {
                class,
                method,
                pc: 0,
                locals: vec![Value::NULL; max_locals],
                stack: Vec::new(),
            },
        }
    }

    /// Returns a finished frame to the pool, or drops it when the pool is full.
    pub fn recycle_frame(&mut self, mut frame: Frame) {
        if self.pool.len() >= FRAME_POOL_LIMIT {
            return;
        }
        // Keep the capacity, drop every value: the next call starts from `null` locals.
        frame.locals.clear();
        frame.stack.clear();
        self.pool.push(frame);
    }
}

/// Invokes Java code from a native method.
///
/// Defined here so `rava-native` depends on this crate alone (ADR-0002). S3's natives do
/// not call back into Java; the first one that does (any `toString` on an object) will
/// take this parameter.
pub trait JavaCall {
    fn invoke(
        &mut self,
        shared: &mut Shared,
        target: &str,
        name: &str,
        descriptor: &str,
        args: &[Value],
    ) -> Result<Value, VmError>;
}

impl Shared {
    pub fn register_native(
        &mut self,
        owner: &str,
        name: &str,
        descriptor: &str,
        function: NativeFn,
    ) {
        self.natives
            .insert(native::key(owner, name, descriptor), function);
    }

    /// A `String[]` holding fresh strings for `main`'s arguments.
    pub fn string_array(&mut self, path: &ClassPath, values: &[String]) -> Result<ObjRef, VmError> {
        let mut elements: Vec<Value> = Vec::with_capacity(values.len());
        for value in values {
            elements.push(Value::Ref(self.new_string(path, value.clone())?));
        }
        Ok(self.allocate(HeapObject::Array(Array {
            kind: ArrayKind::Reference,
            elements,
        })))
    }

    pub fn array(&self, reference: ObjRef) -> Result<&Array, VmError> {
        match self.heap.get(reference.index()) {
            Some(HeapObject::Array(array)) => Ok(array),
            Some(_) => Err(VmError::TypeMismatch {
                expected: "an array reference",
                found: Value::Ref(reference),
            }),
            None => Err(VmError::Native(format!("dangling reference {reference:?}"))),
        }
    }

    pub fn array_mut(&mut self, reference: ObjRef) -> Result<&mut Array, VmError> {
        match self.heap.get_mut(reference.index()) {
            Some(HeapObject::Array(array)) => Ok(array),
            Some(_) => Err(VmError::TypeMismatch {
                expected: "an array reference",
                found: Value::Ref(reference),
            }),
            None => Err(VmError::Native(format!("dangling reference {reference:?}"))),
        }
    }

    /// Whether `actual` is `target` or a subtype of it (JVMS §5.4.4): the class itself, one of
    /// its superclasses, or an interface it implements, transitively.
    ///
    /// The interpreter's `instanceof`/`checkcast` and the `Class` natives both ask this, which
    /// is why it lives on `Shared` rather than in the interpreter.
    pub fn is_assignable(&self, actual: ClassId, target: ClassId) -> bool {
        let mut visited = Vec::new();
        let mut current = Some(actual);
        while let Some(class) = current {
            if class == target {
                return true;
            }
            for interface in &self.class(class).interfaces {
                if self.interface_is_assignable(*interface, target, &mut visited) {
                    return true;
                }
            }
            current = self.class(class).super_class;
        }
        false
    }

    fn interface_is_assignable(
        &self,
        interface: ClassId,
        target: ClassId,
        visited: &mut Vec<ClassId>,
    ) -> bool {
        if interface == target {
            return true;
        }
        if visited.contains(&interface) {
            return false;
        }
        visited.push(interface);
        self.class(interface)
            .interfaces
            .iter()
            .any(|parent| self.interface_is_assignable(*parent, target, visited))
    }

    /// The runtime class of a reference: the instance's class for objects, the library
    /// class for the strings and builders the VM owns storage for.
    pub fn class_of(&self, reference: ObjRef) -> Option<ClassId> {
        match self.heap.get(reference.index())? {
            HeapObject::Instance(instance) => Some(instance.class),
            HeapObject::Builder(_) => self.class_ids.get("java/lang/StringBuilder").copied(),
            HeapObject::Array(_)
            | HeapObject::Lambda(_)
            | HeapObject::FieldRef(_, _)
            | HeapObject::MethodRef(_, _)
            | HeapObject::ConstructorRef(_, _)
            | HeapObject::Null
            | HeapObject::Free => None,
        }
    }

    /// Allocates an instance whose field vector matches the class layout.
    pub fn new_instance(&mut self, class: ClassId) -> ObjRef {
        let fields = self.classes[class.index()]
            .layout
            .iter()
            .map(|slot| Value::default_for(&slot.descriptor))
            .collect();
        self.allocate(HeapObject::Instance(Instance { class, fields }))
    }

    /// The object's identity hash, assigned on first use and stable afterwards.
    pub fn identity_hash(&mut self, reference: ObjRef) -> i32 {
        if let Some(hash) = self.identity_hashes.get(&reference) {
            return *hash;
        }
        let hash = self.next_identity_hash;
        self.next_identity_hash = self.next_identity_hash.wrapping_add(1);
        self.identity_hashes.insert(reference, hash);
        hash
    }

    pub fn instance_field(&self, reference: ObjRef, slot: usize) -> Result<Value, VmError> {
        match self.heap.get(reference.index()) {
            Some(HeapObject::Instance(instance)) => {
                instance
                    .fields
                    .get(slot)
                    .copied()
                    .ok_or_else(|| VmError::FieldNotFound {
                        class: "<instance>".to_owned(),
                        name: format!("slot {slot}"),
                    })
            }
            Some(_) => Err(VmError::TypeMismatch {
                expected: "an object reference",
                found: Value::Ref(reference),
            }),
            None => Err(VmError::Native(format!("dangling reference {reference:?}"))),
        }
    }

    pub fn set_instance_field(
        &mut self,
        reference: ObjRef,
        slot: usize,
        value: Value,
    ) -> Result<(), VmError> {
        match self.heap.get_mut(reference.index()) {
            Some(HeapObject::Instance(instance)) => {
                let cell = instance
                    .fields
                    .get_mut(slot)
                    .ok_or_else(|| VmError::FieldNotFound {
                        class: "<instance>".to_owned(),
                        name: format!("slot {slot}"),
                    })?;
                *cell = value;
                Ok(())
            }
            Some(_) => Err(VmError::TypeMismatch {
                expected: "an object reference",
                found: Value::Ref(reference),
            }),
            None => Err(VmError::Native(format!("dangling reference {reference:?}"))),
        }
    }

    /// A zero-length array of references, used for `main`'s argument array.
    pub fn empty_reference_array(&mut self) -> ObjRef {
        self.allocate(HeapObject::Array(Array {
            kind: ArrayKind::Reference,
            elements: Vec::new(),
        }))
    }

    /// The payload of a `StringBuilder` instance.
    pub fn builder(&self, reference: ObjRef) -> Result<&str, VmError> {
        match self.heap.get(reference.index()) {
            Some(HeapObject::Builder(text)) => Ok(text),
            Some(other) => Err(VmError::Native(format!(
                "expected a StringBuilder, got {other:?}"
            ))),
            None => Err(VmError::Native(format!("dangling reference {reference:?}"))),
        }
    }

    pub fn builder_mut(&mut self, reference: ObjRef) -> Result<&mut String, VmError> {
        match self.heap.get_mut(reference.index()) {
            Some(HeapObject::Builder(text)) => Ok(text),
            Some(other) => Err(VmError::Native(format!(
                "expected a StringBuilder, got {other:?}"
            ))),
            None => Err(VmError::Native(format!("dangling reference {reference:?}"))),
        }
    }

    /// Allocates a `java.lang.String` with the host class's fields: a `byte[] value` and the
    /// `coder` byte (0 = Latin-1, 1 = UTF-16). Strings are instances so the library's own
    /// methods can read them; a Rust string in the heap would make every `String` method fail
    /// on its first `getfield`.
    pub fn new_string(&mut self, path: &ClassPath, text: String) -> Result<ObjRef, VmError> {
        let class = self.load_class(path, "java/lang/String")?;
        let (bytes, coder) = encode_string(&text);
        let value = self.allocate(HeapObject::Array(Array {
            kind: ArrayKind::Byte,
            elements: bytes
                .iter()
                .map(|byte| Value::Int(i32::from(*byte)))
                .collect(),
        }));
        let reference = self.new_instance(class);
        let value_slot = self
            .class(class)
            .field_slot("value", "[B")
            .ok_or_else(|| VmError::Native("java.lang.String has no `value` field".to_owned()))?;
        let coder_slot = self
            .class(class)
            .field_slot("coder", "B")
            .ok_or_else(|| VmError::Native("java.lang.String has no `coder` field".to_owned()))?;
        self.set_instance_field(reference, value_slot, Value::Ref(value))?;
        self.set_instance_field(reference, coder_slot, Value::Int(i32::from(coder)))?;
        Ok(reference)
    }
}

/// Stores text the way `java.lang.String` does: Latin-1 when every character fits a byte, else
/// UTF-16 big-endian. The returned byte says which (`0` Latin-1, `1` UTF-16), matching
/// `String.coder`.
fn encode_string(text: &str) -> (Vec<u8>, u8) {
    if text.chars().all(|character| u32::from(character) <= 0xFF) {
        (text.chars().map(|character| character as u8).collect(), 0)
    } else {
        let mut bytes = Vec::with_capacity(text.len() * 2);
        for unit in text.encode_utf16() {
            bytes.extend_from_slice(&unit.to_be_bytes());
        }
        (bytes, 1)
    }
}

/// Decodes `String.value` through `String.coder`.
fn decode_string(bytes: &[u8], coder: i32) -> Result<String, VmError> {
    match coder {
        0 => Ok(bytes.iter().map(|byte| char::from(*byte)).collect()),
        1 => {
            if bytes.len() % 2 != 0 {
                return Err(VmError::Native(
                    "a UTF-16 String has an odd number of bytes".to_owned(),
                ));
            }
            let units: Vec<u16> = bytes
                .chunks_exact(2)
                .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
                .collect();
            String::from_utf16(&units)
                .map_err(|_| VmError::Native("a String holds an unpaired surrogate".to_owned()))
        }
        other => Err(VmError::Native(format!("unknown String coder {other}"))),
    }
}

/// Whether a `CONSTANT_Class` name is a primitive or array descriptor rather than a binary name.
pub fn is_type_descriptor(name: &str) -> bool {
    if name.starts_with('[') {
        return true;
    }
    matches!(name, "I" | "J" | "F" | "D" | "Z" | "B" | "C" | "S" | "V")
}

/// `Class.getName` for a type descriptor: `int`, `[Ljava.lang.String;`, `[[I`.
pub fn type_name(descriptor: &str) -> String {
    match descriptor {
        "I" => "int".to_owned(),
        "J" => "long".to_owned(),
        "F" => "float".to_owned(),
        "D" => "double".to_owned(),
        "Z" => "boolean".to_owned(),
        "B" => "byte".to_owned(),
        "C" => "char".to_owned(),
        "S" => "short".to_owned(),
        "V" => "void".to_owned(),
        array => array.replace('/', "."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_array(shared: &mut Shared) -> ObjRef {
        shared.allocate(HeapObject::Array(Array {
            kind: ArrayKind::Reference,
            elements: Vec::new(),
        }))
    }

    #[test]
    fn collection_keeps_roots_and_frees_the_rest() {
        let mut shared = Shared::new();
        let thread = ThreadState::new();

        // The intern table is a root set.
        let kept = empty_array(&mut shared);
        shared.interned.insert("kept".to_owned(), kept);
        let orphan = empty_array(&mut shared);

        let stats = shared.collect(&thread);
        assert_eq!(stats.freed, 1, "only the orphan is unreachable");
        assert!(matches!(shared.heap[orphan.index()], HeapObject::Free));
        assert!(matches!(shared.heap[kept.index()], HeapObject::Array(_)));

        // The freed slot is reused, which is why a stale reference is a bug the VM cannot
        // detect: the index stays valid and now names a different object.
        let reused = empty_array(&mut shared);
        assert_eq!(reused, orphan);
    }

    #[test]
    fn collection_follows_references_from_objects_and_arrays() {
        let mut shared = Shared::new();
        let thread = ThreadState::new();

        let inner = empty_array(&mut shared);
        let _orphan = shared.allocate(HeapObject::Array(Array {
            kind: ArrayKind::Reference,
            elements: vec![Value::Ref(inner)],
        }));
        // The array is not a root, so both it and `inner` go; then make one reachable and
        // prove the walk keeps its element alive.
        let stats = shared.collect(&thread);
        assert_eq!(stats.freed, 2);

        let inner = empty_array(&mut shared);
        let outer = shared.allocate(HeapObject::Array(Array {
            kind: ArrayKind::Reference,
            elements: vec![Value::Ref(inner)],
        }));
        shared.interned.insert("root".to_owned(), outer);

        let stats = shared.collect(&thread);
        assert_eq!(
            stats.freed, 0,
            "the array is a root and the element is reachable from it"
        );
        assert!(matches!(shared.heap[inner.index()], HeapObject::Array(_)));
    }
}
