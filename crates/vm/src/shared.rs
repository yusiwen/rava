//! The shared runtime state.
//!
//! `Shared` is split into independently lockable parts: the class table, the heap,
//! the native registry and stdout. Per-thread state lives in [`crate::ThreadState`];
//! a Java call never holds one of these locks. [`Shared::check_no_locks_held`] is the
//! probe the interpreter uses to enforce that invariant at call boundaries.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::{Mutex, MutexGuard, RwLock};

use crate::VmError;
use crate::heap::{
    AnnotationObject, ArrayClass, BoxedPrimitive, ClassRef, FieldLayout, Heap, HeapObject,
    MemberKind, ObjRef, PrimitiveKind, ReflectMember, TypeObject,
};
use crate::hooks::Hooks;
use crate::loader::{ClassEntry, ClassId, ClassState, ClassTable, LoaderId};
use crate::source::{ClassSource, DirectorySource};
use crate::thread::ThreadState;
use crate::value::Value;

use rava_classfile::ClassMetadata;

pub type NativeFn =
    fn(&Shared, &ThreadState, LoaderId, ObjRef, &[Value]) -> Result<Option<Value>, VmError>;

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
    ClassObjects,
    PrimitiveClasses,
    ArrayClasses,
    ReflectMembers,
    TypeObjects,
    EnumConstants,
    InternedStrings,
    ProxyClasses,
    LoaderObjects,
    ObjectLoaders,
    NativeRegistry,
    Stdout,
    Stderr,
    /// The `Unsafe` U0 call log. A native appends one entry per call and releases the
    /// lock before returning, so it is never held across a Java call.
    UnsafeCalls,
}

impl fmt::Display for SharedLock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ClassTable => write!(f, "class table"),
            Self::Heap => write!(f, "heap"),
            Self::ClassObjects => write!(f, "class-object cache"),
            Self::PrimitiveClasses => write!(f, "primitive-class cache"),
            Self::ArrayClasses => write!(f, "array-class cache"),
            Self::ReflectMembers => write!(f, "reflected-member cache"),
            Self::TypeObjects => write!(f, "generic-type cache"),
            Self::EnumConstants => write!(f, "enum-constant cache"),
            Self::InternedStrings => write!(f, "interned-string table"),
            Self::ProxyClasses => write!(f, "generated-proxy cache"),
            Self::LoaderObjects => write!(f, "loader-object cache"),
            Self::ObjectLoaders => write!(f, "object-to-loader map"),
            Self::NativeRegistry => write!(f, "native registry"),
            Self::Stdout => write!(f, "stdout"),
            Self::Stderr => write!(f, "stderr"),
            Self::UnsafeCalls => write!(f, "Unsafe call log"),
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
    /// One class object per `ClassId`, the `(loader, name)` identity. The cache is the
    /// owner of those objects: they are never freed while `Shared` lives, and
    /// [`Shared::roots`] reports them as roots.
    class_objects: Mutex<HashMap<ClassId, ObjRef>>,
    /// One primitive class object per [`PrimitiveKind`]. The cache owns the objects, which
    /// are never freed while `Shared` lives and are reported as roots by
    /// [`Shared::roots`].
    primitive_classes: Mutex<HashMap<PrimitiveKind, ObjRef>>,
    /// One array class object per component [`ClassRef`]. The cache owns the objects,
    /// which are never freed while `Shared` lives and are reported as roots by
    /// [`Shared::roots`]. The component is the whole identity of an array class.
    array_classes: Mutex<HashMap<ClassRef, ObjRef>>,
    /// One reflected-member object per `(class, kind, name, descriptor)`. The cache owns
    /// the objects, which are never freed while `Shared` lives and are reported as roots
    /// by [`Shared::roots`].
    reflect_members: Mutex<HashMap<(ClassId, MemberKind, String, String), ObjRef>>,
    /// One `java.lang.reflect.Type` object per structural type. The cache owns the
    /// objects, which are never freed while `Shared` lives and are reported as roots by
    /// [`Shared::roots`]. A raw class type is not here: it is the class object itself.
    type_objects: Mutex<HashMap<TypeObject, ObjRef>>,
    /// One enum-constant object per `(enum class, constant name)`. The cache owns the
    /// objects, which are never freed while `Shared` lives and are reported as roots by
    /// [`Shared::roots`].
    enum_constants: Mutex<HashMap<(ClassId, String), ObjRef>>,
    /// One canonical string object per interned text. The table owns the objects, which
    /// are never freed while `Shared` lives and are reported as roots by
    /// [`Shared::roots`].
    interned_strings: Mutex<HashMap<String, ObjRef>>,
    /// One generated `java.lang.reflect.Proxy` class per `(defining loader, interfaces)`
    /// tuple. The cache stores [`ClassId`]s rather than heap roots, because a generated
    /// proxy class is an ordinary class-table entry; its class object is owned by
    /// [`Shared::class_object`] and reported as a root there.
    proxy_classes: Mutex<HashMap<(LoaderId, Vec<ClassId>), ClassId>>,
    natives: RwLock<HashMap<NativeKey, NativeFn>>,
    stdout: Mutex<Vec<u8>>,
    stderr: Mutex<Vec<u8>>,
    /// One entry per `sun.misc.Unsafe` / `jdk.internal.misc.Unsafe` call the VM served.
    ///
    /// ADR-0005's acceptance requires a runtime log that contains only the open tier's
    /// methods. The log is diagnostic state, not roots: it holds text, never references.
    /// The lock is released before a native returns, so it is never held across a Java
    /// call.
    unsafe_calls: Mutex<Vec<String>>,
    /// The minimal `java.util.logging` state: the one `Logger` per name and the fields of
    /// every live `LogRecord`.
    jul: Mutex<crate::jul::JulState>,
    /// The system property table. `System.getProperty` reads it and
    /// `StandardEnvironment` copies it into its property sources.
    properties: Mutex<HashMap<String, String>>,
    /// The name and ordinal of every ordinary enum instance a constructor created.
    enums: Mutex<HashMap<ObjRef, (String, i32)>>,
    /// Lock state for a lock object that is an ordinary instance, such as Spring's
    /// `ConcurrentReferenceHashMap$Segment`, which extends `ReentrantLock` and adds its own
    /// fields. A plain `ReentrantLock` keeps its state in [`crate::HeapObject::Lock`].
    locks: Mutex<HashMap<ObjRef, crate::heap::LockState>>,
    /// The referent of every `java.lang.ref.Reference` instance. There is no collector yet
    /// (M5), so a reference never clears on its own; `get`/`clear`/`enqueue` still work.
    references: Mutex<HashMap<ObjRef, ObjRef>>,
    /// The one `java.lang.Thread` object `Thread.currentThread` returns.
    main_thread: Mutex<Option<ObjRef>>,
    /// One value per `java.lang.ThreadLocal` object. There is one thread until M5, so the
    /// map is the thread-local storage.
    thread_locals: Mutex<HashMap<ObjRef, Value>>,
    /// One `java.lang.ClassLoader` object per [`LoaderId`]. The cache owns the objects,
    /// which are never freed while `Shared` lives and are reported as roots by
    /// [`Shared::roots`]. A user `ClassLoader` subclass instance is the object for the
    /// child loader [`Shared::loader_id_for`] created for it.
    loader_objects: Mutex<HashMap<LoaderId, ObjRef>>,
    /// The inverse of [`Shared::loader_objects`]: the loader an object represents. Both
    /// maps are needed because a `Class.getClassLoader` lookup is object-to-loader and a
    /// `ClassLoader` native starts from its receiver object.
    object_loaders: Mutex<HashMap<ObjRef, LoaderId>>,
    /// Members whose `AccessibleObject.setAccessible(true)` was called.
    ///
    /// The VM performs no access-control check, so this flag only answers
    /// `isAccessible`; `setAccessible(false)` removes the member again. The members
    /// themselves are already permanent roots through `reflect_members`.
    accessible: Mutex<HashSet<ObjRef>>,
    /// The detail message of each throwable a constructor created.
    throwable_messages: Mutex<HashMap<ObjRef, String>>,
    hooks: Hooks,
}

/// The system properties the runtime starts with.
///
/// These are the values Spring reads by name (`java.version`, `java.home`,
/// `line.separator`, ...), not a copy of a host JVM's table.
fn default_properties() -> HashMap<String, String> {
    let mut properties = HashMap::new();
    for (name, value) in [
        ("java.version", "17"),
        ("java.specification.version", "17"),
        ("java.vendor", "rava"),
        ("java.vm.name", "rava"),
        ("java.home", ""),
        ("java.class.path", ""),
        ("file.encoding", "UTF-8"),
        ("line.separator", "\n"),
        ("os.name", std::env::consts::OS),
        ("os.arch", std::env::consts::ARCH),
        ("user.dir", ""),
    ] {
        properties.insert(name.to_owned(), value.to_owned());
    }
    properties
}

/// The VM-level container shape a class is or extends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VmContainerKind {
    List,
    Set,
    Map,
}

impl Shared {
    pub fn with_bootstrap(source: Arc<dyn ClassSource>) -> Self {
        Self {
            classes: RwLock::new(ClassTable::with_bootstrap(source)),
            heap: Mutex::new(Heap::new()),
            class_objects: Mutex::new(HashMap::new()),
            primitive_classes: Mutex::new(HashMap::new()),
            array_classes: Mutex::new(HashMap::new()),
            reflect_members: Mutex::new(HashMap::new()),
            type_objects: Mutex::new(HashMap::new()),
            enum_constants: Mutex::new(HashMap::new()),
            interned_strings: Mutex::new(HashMap::new()),
            proxy_classes: Mutex::new(HashMap::new()),
            natives: RwLock::new(HashMap::new()),
            stdout: Mutex::new(Vec::new()),
            stderr: Mutex::new(Vec::new()),
            unsafe_calls: Mutex::new(Vec::new()),
            jul: Mutex::new(crate::jul::JulState::default()),
            properties: Mutex::new(default_properties()),
            enums: Mutex::new(HashMap::new()),
            locks: Mutex::new(HashMap::new()),
            references: Mutex::new(HashMap::new()),
            main_thread: Mutex::new(None),
            thread_locals: Mutex::new(HashMap::new()),
            loader_objects: Mutex::new(HashMap::new()),
            object_loaders: Mutex::new(HashMap::new()),
            accessible: Mutex::new(HashSet::new()),
            throwable_messages: Mutex::new(HashMap::new()),
            hooks: Hooks::default(),
        }
    }

    /// Build a `Shared` whose bootstrap loader reads a class-library directory.
    ///
    /// This is the class-library entry point: `stdlib/c1/classes` is read through the
    /// ordinary [`DirectorySource`], so classes get the same `(loader, name)` identity as
    /// any other source and no second class table is introduced. The loader does not read
    /// the directory up front; a class is parsed when it is first loaded.
    pub fn with_library_dir(root: impl Into<PathBuf>) -> Self {
        Self::with_bootstrap(Arc::new(DirectorySource::new(root)))
    }

    /// Add a loader parented to `parent` and make it the application loader.
    ///
    /// A runner calls this once for its classpath. The VM's own child loaders (for a user
    /// `ClassLoader` instance) are added through [`Shared::add_child_loader`] and do not
    /// replace the application loader.
    pub fn add_loader(
        &self,
        parent: LoaderId,
        source: Arc<dyn ClassSource>,
    ) -> Result<LoaderId, VmError> {
        self.classes
            .write()
            .expect("class table lock poisoned")
            .add_application_loader(parent, source)
    }

    /// Add a child loader without making it the application loader.
    ///
    /// [`Shared::loader_id_for`] uses this for a `java.lang.ClassLoader` subclass instance
    /// that no native had seen yet. The child has an empty source: it defines only classes
    /// handed to its `defineClass`, and delegates every `loadClass` lookup to its parent.
    pub(crate) fn add_child_loader(
        &self,
        parent: LoaderId,
        source: Arc<dyn ClassSource>,
    ) -> Result<LoaderId, VmError> {
        self.classes
            .write()
            .expect("class table lock poisoned")
            .add_loader(parent, source)
    }

    /// The application loader: the loader a runner added last, or the bootstrap loader
    /// when none was added.
    ///
    /// `Class.forName(String)` needs a loader but its native ABI receives the *declaring*
    /// class's loader (`java.lang.Class` is a bootstrap class), not the caller's. In this
    /// VM there is one application loader, so resolving through it is the same parent-first
    /// lookup the caller's loader would perform.
    pub fn application_loader(&self) -> LoaderId {
        self.classes
            .read()
            .expect("class table lock poisoned")
            .application_loader()
    }

    /// The parent loader of `loader`, if it has one.
    ///
    /// The bootstrap loader has no parent. The application loader's parent is the
    /// bootstrap loader; [`crate::class_loader`] maps that to a null `ClassLoader.getParent`
    /// because this VM has no separate platform loader.
    pub fn loader_parent(&self, loader: LoaderId) -> Option<LoaderId> {
        self.classes
            .read()
            .expect("class table lock poisoned")
            .parent(loader)
    }

    /// A stable display name for a loader.
    ///
    /// The application loader and the bootstrap loader are both `"rava"`: this VM has no
    /// platform loader, and a `ClassLoader` object is only ever allocated for a non-bootstrap
    /// loader. A child loader created for a user `ClassLoader` instance is
    /// `"rava-child-<loader id>"`, which is stable for the life of the `Shared` because a
    /// loader id is never reused.
    pub fn loader_name(&self, loader: LoaderId) -> String {
        if loader == self.application_loader() || loader == LoaderId::BOOTSTRAP {
            "rava".to_owned()
        } else {
            format!("rava-child-{}", loader.0)
        }
    }

    pub fn loader_exists(&self, loader: LoaderId) -> bool {
        self.classes
            .read()
            .expect("class table lock poisoned")
            .has_loader(loader)
    }

    pub fn load_class(&self, loader: LoaderId, name: &str) -> Result<ClassId, VmError> {
        self.classes
            .write()
            .expect("class table lock poisoned")
            .load_class(loader, name)
    }

    /// Load a class from the bootstrap (class-library) loader.
    ///
    /// The class is located through the delegation chain, so a child loader's request for a
    /// library class still defines it in the bootstrap loader.
    pub fn load_library_class(&self, name: &str) -> Result<ClassId, VmError> {
        self.load_class(LoaderId::BOOTSTRAP, name)
    }

    /// Load a library class and decode its C1 metadata in one step.
    ///
    /// Decoding reads attributes only. A signature-only stub with no `Code` attribute
    /// loads and exposes its metadata; nothing is initialized or executed.
    pub fn library_metadata(&self, name: &str) -> Result<ClassMetadata, VmError> {
        let id = self.load_library_class(name)?;
        self.entry(id)
            .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", id.0)))?
            .metadata()
    }

    /// The class object for one class identity, allocated on first use.
    ///
    /// The cache is keyed by [`ClassId`], which is `(defining loader, binary name)`, so
    /// two loaders that define the same name get distinct class objects. The returned
    /// handle is stable for the life of the `Shared`: class objects are never freed.
    ///
    /// Lock order is cache, then heap. No path takes the heap lock before this cache,
    /// which is what keeps the pair deadlock-free at the native boundary.
    pub fn class_object(&self, class: ClassId) -> Result<ObjRef, VmError> {
        if self.entry(class).is_none() {
            return Err(VmError::Loader(format!(
                "class id {} is not loaded",
                class.0
            )));
        }
        let mut cache = self
            .class_objects
            .lock()
            .expect("class-object cache lock poisoned");
        if let Some(reference) = cache.get(&class) {
            return Ok(*reference);
        }
        let reference = self
            .heap
            .lock()
            .expect("heap lock poisoned")
            .allocate_class(class);
        cache.insert(class, reference);
        Ok(reference)
    }

    /// The class object for one primitive kind, allocated on first use.
    ///
    /// A primitive has no defining loader and no class file, so its class object is
    /// keyed by the kind alone: one object per primitive for the life of the `Shared`.
    /// `Class.forName("int")` and `Integer.TYPE` both resolve here, which is what makes
    /// them reference-identical. The returned handle is stable: primitive class objects
    /// are never freed and are reported as roots by [`Shared::roots`].
    ///
    /// Lock order is cache, then heap, matching [`Shared::class_object`].
    pub fn primitive_class(&self, kind: PrimitiveKind) -> ObjRef {
        let mut cache = self
            .primitive_classes
            .lock()
            .expect("primitive-class cache lock poisoned");
        if let Some(reference) = cache.get(&kind) {
            return *reference;
        }
        let reference = self
            .heap
            .lock()
            .expect("heap lock poisoned")
            .allocate_primitive_class(kind);
        cache.insert(kind, reference);
        reference
    }

    /// The array class object for one component, allocated on first use.
    ///
    /// The component is the whole identity of an array class, so `int[]` is keyed by
    /// [`ClassRef::Primitive`]`(Int)` and `int[][]` by the `int[]` ref. A second request
    /// for the same component returns the same handle, which is what makes
    /// `Class.forName("[I")`, `int[].class` and a `new int[...]` receiver's `getClass()`
    /// reference-identical. The returned handle is stable: array class objects are never
    /// freed and are reported as roots by [`Shared::roots`].
    ///
    /// A named component must already be loaded. Lock order is cache, then heap, matching
    /// [`Shared::class_object`].
    pub fn array_class(&self, component: ClassRef) -> Result<ObjRef, VmError> {
        self.validate_class_ref(&component)?;
        let mut cache = self
            .array_classes
            .lock()
            .expect("array-class cache lock poisoned");
        if let Some(reference) = cache.get(&component) {
            return Ok(*reference);
        }
        let reference = self
            .heap
            .lock()
            .expect("heap lock poisoned")
            .allocate_array_class(component.clone());
        cache.insert(component, reference);
        Ok(reference)
    }

    /// The class object a [`ClassRef`] denotes: a loaded class, a primitive or an array.
    pub fn class_object_for_ref(&self, class_ref: &ClassRef) -> Result<ObjRef, VmError> {
        match class_ref {
            ClassRef::Named(class) => self.class_object(*class),
            ClassRef::Primitive(kind) => Ok(self.primitive_class(*kind)),
            ClassRef::Array(array) => self.array_class(array.component.clone()),
        }
    }

    /// The JVM field descriptor of a [`ClassRef`].
    ///
    /// A named class contributes `Linternal/Name;`, a primitive its one-character
    /// descriptor and an array a `[` followed by its component's descriptor. A named
    /// component must be loaded.
    pub fn class_ref_descriptor(&self, class_ref: &ClassRef) -> Result<String, VmError> {
        match class_ref {
            ClassRef::Named(class) => {
                let entry = self.entry(*class).ok_or_else(|| {
                    VmError::Loader(format!("class id {} is not loaded", class.0))
                })?;
                Ok(format!("L{};", entry.name))
            }
            ClassRef::Primitive(kind) => Ok(kind.descriptor().to_owned()),
            ClassRef::Array(array) => self.array_descriptor(array),
        }
    }

    /// The JVM field descriptor of an array class (`[I`, `[Ljava/lang/String;`).
    pub fn array_descriptor(&self, array: &ArrayClass) -> Result<String, VmError> {
        Ok(format!("[{}", self.class_ref_descriptor(&array.component)?))
    }

    /// The name of a [`ClassRef`] as `Class.getName()` reports it.
    ///
    /// A named class reports its dotted binary name, a primitive its Java name and an
    /// array the JDK's descriptor form with dots (`[I`, `[[I`, `[Ljava.lang.String;`).
    pub fn class_ref_name(&self, class_ref: &ClassRef) -> Result<String, VmError> {
        match class_ref {
            ClassRef::Named(class) => {
                let entry = self.entry(*class).ok_or_else(|| {
                    VmError::Loader(format!("class id {} is not loaded", class.0))
                })?;
                Ok(entry.name.replace('/', "."))
            }
            ClassRef::Primitive(kind) => Ok(kind.name().to_owned()),
            ClassRef::Array(array) => self.array_name(array),
        }
    }

    /// The `Class.getName()` of an array class.
    pub fn array_name(&self, array: &ArrayClass) -> Result<String, VmError> {
        Ok(self.array_descriptor(array)?.replace('/', "."))
    }

    /// Parse a field descriptor into a [`ClassRef`].
    ///
    /// A named component is resolved through `loader`, which is the declaring class's
    /// loader for a member type and the bootstrap loader for `Class.forName`. Arrays may
    /// nest (`[[I`, `[[Ljava/lang/String;`); a primitive component is a one-character
    /// descriptor and `void` is not a legal component. A malformed descriptor is a typed
    /// error rather than a guessed class.
    pub fn class_ref_for_descriptor(
        &self,
        loader: LoaderId,
        descriptor: &str,
    ) -> Result<ClassRef, VmError> {
        let bytes = descriptor.as_bytes();
        if bytes.first() == Some(&b'[') {
            let mut dimensions = 0;
            while bytes.get(dimensions) == Some(&b'[') {
                dimensions += 1;
            }
            let element = &descriptor[dimensions..];
            let mut class_ref = match object_descriptor_name(element) {
                Some(internal) => {
                    if internal.is_empty() {
                        return Err(malformed_array_descriptor(descriptor));
                    }
                    ClassRef::Named(self.load_class(loader, internal)?)
                }
                None => match PrimitiveKind::from_descriptor(element) {
                    Some(PrimitiveKind::Void) | None => {
                        return Err(malformed_array_descriptor(descriptor));
                    }
                    Some(kind) => ClassRef::Primitive(kind),
                },
            };
            for _ in 0..dimensions {
                class_ref = ClassRef::array(class_ref);
            }
            return Ok(class_ref);
        }
        if let Some(internal) = object_descriptor_name(descriptor) {
            if internal.is_empty() {
                return Err(malformed_descriptor(descriptor));
            }
            return Ok(ClassRef::Named(self.load_class(loader, internal)?));
        }
        PrimitiveKind::from_descriptor(descriptor)
            .map(ClassRef::Primitive)
            .ok_or_else(|| malformed_descriptor(descriptor))
    }

    fn validate_class_ref(&self, class_ref: &ClassRef) -> Result<(), VmError> {
        match class_ref {
            ClassRef::Named(class) => {
                if self.entry(*class).is_none() {
                    return Err(VmError::Loader(format!(
                        "class id {} is not loaded",
                        class.0
                    )));
                }
                Ok(())
            }
            ClassRef::Primitive(_) => Ok(()),
            ClassRef::Array(array) => self.validate_class_ref(&array.component),
        }
    }

    /// The value of a static field, including the primitive wrapper `TYPE` fields.
    ///
    /// The statics table is the ordinary home of a static field's value, but the C1 stubs
    /// carry no initializer `Code`, so `Integer.TYPE` and the other eight wrapper `TYPE`
    /// fields would read as absent. This resolves them virtually: a missing
    /// `TYPE:Ljava/lang/Class;` on one of the nine wrapper classes returns the cached
    /// primitive class object, which is the same object `Class.forName("int")` returns.
    /// Nothing is written into the statics table, so the value is not duplicated.
    ///
    /// Returns `None` for any other missing field, so the interpreter reports it exactly
    /// as before.
    pub fn static_field_value(
        &self,
        class: ClassId,
        name: &str,
        descriptor: &str,
    ) -> Result<Option<Value>, VmError> {
        let entry = self
            .entry(class)
            .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", class.0)))?;
        let stored = entry
            .statics
            .lock()
            .expect("static field lock poisoned")
            .get(&(name.to_owned(), descriptor.to_owned()))
            .copied();
        if stored.is_some() {
            return Ok(stored);
        }
        if name == "TYPE" && descriptor == "Ljava/lang/Class;" {
            if let Some(kind) = PrimitiveKind::from_wrapper_class(&entry.name) {
                return Ok(Some(Value::Ref(self.primitive_class(kind))));
            }
        }
        // The VM does not run an enum class's `<clinit>`, which is where javac stores each
        // constant, so a static field of the enum's own type resolves to the canonical
        // enum-constant object. Compiled code that reads `TimeUnit.MILLISECONDS` (or any
        // other enum constant) therefore sees a real object, and two reads of the same
        // constant are reference-identical.
        if let Some(value) = self.enum_static_constant(&entry, name, descriptor)? {
            return Ok(Some(value));
        }
        // A declared static field that no initializer has written is readable all the
        // same: JVMS §5.4.2 preparation gives it its type's default, and a `static final`
        // takes its `ConstantValue`. The statics table starts empty, so a field such as
        // CGLIB's `ClassEmitter.hookCounter` (no initializer, so no `putstatic`) resolves
        // from its declaration here.
        self.declared_static_value(&entry, name, descriptor)
    }

    /// The value of a declared static field whose statics entry is absent.
    ///
    /// `ConstantValue` is the field's initial value (JVMS §4.7.2); every other type gets
    /// its default. Only a declaration in this class is considered, so a misspelt field
    /// still reports as absent.
    fn declared_static_value(
        &self,
        entry: &crate::loader::ClassEntry,
        name: &str,
        descriptor: &str,
    ) -> Result<Option<Value>, VmError> {
        let file = &entry.file;
        for field in &file.fields {
            if !field.is_static()
                || file.field_name(field)? != name
                || file.field_descriptor(field)? != descriptor
            {
                continue;
            }
            let constant = file.constant_value(field).map_err(VmError::ClassFile)?;
            let value = match constant {
                Some(rava_classfile::Constant::Integer(value)) => Value::Int(*value),
                Some(rava_classfile::Constant::Float(value)) => Value::Float(*value),
                Some(rava_classfile::Constant::Long(value)) => Value::Long(*value),
                Some(rava_classfile::Constant::Double(value)) => Value::Double(*value),
                Some(rava_classfile::Constant::String { string_index }) => {
                    let text = file
                        .constant_pool
                        .utf8(*string_index)
                        .map_err(VmError::ClassFile)?;
                    Value::Ref(self.intern_string(text))
                }
                _ => Value::default_for(descriptor),
            };
            return Ok(Some(value));
        }
        Ok(None)
    }

    /// The static-field keys the VM has written into `class`'s statics table.
    ///
    /// The C1 projection carries only public members, so a VM-installed private static
    /// such as `sun.misc.Unsafe.theUnsafe` has a value but no class-file declaration.
    /// Reflection uses this list to expose those entries as declared fields; a key the
    /// metadata already declares is filtered by the caller, so `System.out` and `err`
    /// appear once. The keys are sorted for a deterministic `getDeclaredFields` order.
    pub fn installed_static_fields(
        &self,
        class: ClassId,
    ) -> Result<Vec<(String, String)>, VmError> {
        let entry = self
            .entry(class)
            .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", class.0)))?;
        let mut keys: Vec<(String, String)> = entry
            .statics
            .lock()
            .expect("static field lock poisoned")
            .keys()
            .cloned()
            .collect();
        keys.sort();
        Ok(keys)
    }

    /// Whether `class`'s statics table holds exactly this field key.
    ///
    /// This answers the declaration question for a VM-installed field whose descriptor the
    /// C1 metadata omits, without inventing a value when the key is absent.
    pub fn has_installed_static(
        &self,
        class: ClassId,
        name: &str,
        descriptor: &str,
    ) -> Result<bool, VmError> {
        let entry = self
            .entry(class)
            .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", class.0)))?;
        Ok(entry
            .statics
            .lock()
            .expect("static field lock poisoned")
            .contains_key(&(name.to_owned(), descriptor.to_owned())))
    }

    /// Write one static field value into `class`'s statics table.
    ///
    /// This is the write half of [`Shared::static_field_value`] for
    /// `java.lang.reflect.Field.set(null, value)`. It does not run an initializer and does
    /// not validate the descriptor against the class file: the reflected `Field` already
    /// names the declaration or the installed static the caller read.
    pub fn set_static_field(
        &self,
        class: ClassId,
        name: &str,
        descriptor: &str,
        value: Value,
    ) -> Result<(), VmError> {
        let entry = self
            .entry(class)
            .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", class.0)))?;
        entry
            .statics
            .lock()
            .expect("static field lock poisoned")
            .insert((name.to_owned(), descriptor.to_owned()), value);
        Ok(())
    }

    /// The canonical enum object for a static constant field of an enum class.
    ///
    /// Only a static field whose descriptor is `L<this class>;` on an `ACC_ENUM` class is
    /// treated as a constant: that is exactly the shape javac emits for one, and any
    /// other absent static stays absent so the missing value is still reported.
    fn enum_static_constant(
        &self,
        entry: &crate::loader::ClassEntry,
        name: &str,
        descriptor: &str,
    ) -> Result<Option<Value>, VmError> {
        let metadata = entry.metadata()?;
        if !metadata.is_enum() {
            return Ok(None);
        }
        let own_descriptor = format!("L{};", entry.name);
        if descriptor != own_descriptor {
            return Ok(None);
        }
        let Some(field) = metadata.field(name) else {
            return Ok(None);
        };
        if !field.is_static() {
            return Ok(None);
        }
        let object = self.enum_constant(entry.id, name)?;
        Ok(Some(Value::Ref(object)))
    }

    /// The `java.lang.reflect.Type` object for one structural type, allocated on first
    /// use.
    ///
    /// The key is the whole structure, so two resolutions of the same signature — the
    /// return type and the parameter type of one generic method, or the same field queried
    /// twice — return the same handle. That is what makes `==` on two reflections of one
    /// declaration true, as it is on the JDK. A raw class type never reaches this cache:
    /// it is a `Class` object, because `java.lang.Class` implements `Type`.
    ///
    /// Lock order is cache, then heap, matching [`Shared::class_object`].
    pub fn type_object(&self, ty: TypeObject) -> ObjRef {
        let mut cache = self
            .type_objects
            .lock()
            .expect("generic-type cache lock poisoned");
        if let Some(reference) = cache.get(&ty) {
            return *reference;
        }
        let reference = self
            .heap
            .lock()
            .expect("heap lock poisoned")
            .allocate_type(ty.clone());
        cache.insert(ty, reference);
        reference
    }

    /// The reflected-member object for one `(class, kind, name, descriptor)`, allocated on
    /// first use.
    ///
    /// The key includes the declaring class identity, so two loaders that define the same
    /// name get distinct member objects, and a field and a method with the same name and
    /// descriptor are distinct. The returned handle is stable for the life of the
    /// `Shared`: member objects are never freed.
    ///
    /// Lock order is cache, then heap, matching [`Shared::class_object`]. No path takes
    /// the heap lock before a cache lock, which keeps the pair deadlock-free.
    pub fn reflect_member(
        &self,
        class: ClassId,
        kind: MemberKind,
        name: &str,
        descriptor: &str,
    ) -> Result<ObjRef, VmError> {
        if self.entry(class).is_none() {
            return Err(VmError::Loader(format!(
                "class id {} is not loaded",
                class.0
            )));
        }
        let mut cache = self
            .reflect_members
            .lock()
            .expect("reflected-member cache lock poisoned");
        let key = (class, kind, name.to_owned(), descriptor.to_owned());
        if let Some(reference) = cache.get(&key) {
            return Ok(*reference);
        }
        let reference = self
            .heap
            .lock()
            .expect("heap lock poisoned")
            .allocate_reflect(ReflectMember {
                kind,
                class,
                name: key.2.clone(),
                descriptor: key.3.clone(),
            });
        cache.insert(key, reference);
        Ok(reference)
    }

    /// The generated proxy class for a defining loader and interface list, if it was
    /// already created.
    ///
    /// The key is the defining loader plus the deduplicated interface identities, so
    /// `Proxy.getProxyClass` returns one class object per interface list and repeated
    /// calls observe the same `Class`. The value is a [`ClassId`], not a heap reference:
    /// the class table owns the entry and [`Shared::class_object`] owns its class object.
    ///
    /// Lock order is this cache alone; no heap lock is taken while it is held.
    pub fn proxy_class(&self, loader: LoaderId, interfaces: &[ClassId]) -> Option<ClassId> {
        self.proxy_classes
            .lock()
            .expect("generated-proxy cache lock poisoned")
            .get(&(loader, interfaces.to_vec()))
            .copied()
    }

    /// Record a generated proxy class for a defining loader and interface list.
    ///
    /// The caller has already defined the class through [`Shared::define_class`], so the
    /// verified class table entry exists before the cache can name it.
    pub fn record_proxy_class(&self, loader: LoaderId, interfaces: Vec<ClassId>, class: ClassId) {
        self.proxy_classes
            .lock()
            .expect("generated-proxy cache lock poisoned")
            .insert((loader, interfaces), class);
    }

    /// Record `AccessibleObject.setAccessible`'s flag for one member.
    ///
    /// There is no access-control check in this VM, so this is the whole meaning of the
    /// flag: `isAccessible` reports it and `setAccessible(false)` clears it.
    pub fn set_accessible(&self, member: ObjRef, accessible: bool) {
        let mut members = self
            .accessible
            .lock()
            .expect("accessible-set lock poisoned");
        if accessible {
            members.insert(member);
        } else {
            members.remove(&member);
        }
    }

    /// `AccessibleObject.isAccessible`: whether `setAccessible(true)` was called.
    pub fn is_accessible(&self, member: ObjRef) -> bool {
        self.accessible
            .lock()
            .expect("accessible-set lock poisoned")
            .contains(&member)
    }

    /// Allocate an annotation instance over its annotation type and encoded values.
    ///
    /// Annotation instances are ordinary heap objects: no cache owns them, so a second
    /// query returns a second object. Two instances of the same encoded annotation
    /// compare equal through `Annotation.equals` (the bytes of their canonical encoding),
    /// which is the observable contract this slice provides; identity is not shared.
    pub fn annotation_object(
        &self,
        class: ClassId,
        values: Vec<rava_classfile::AnnotationValue>,
    ) -> Result<ObjRef, VmError> {
        if self.entry(class).is_none() {
            return Err(VmError::Loader(format!(
                "class id {} is not loaded",
                class.0
            )));
        }
        Ok(self.heap().allocate_annotation(class, values))
    }

    /// The annotation instance behind a reference, if it is one.
    ///
    /// A null reference and every other heap object return `None`, so a caller can use
    /// this as a dispatch test without treating a non-annotation receiver as an error.
    pub fn annotation(&self, reference: ObjRef) -> Result<Option<AnnotationObject>, VmError> {
        if reference.is_null() {
            return Ok(None);
        }
        let heap = self.heap();
        match heap.get(reference) {
            Ok(HeapObject::Annotation(annotation)) => Ok(Some(annotation.clone())),
            Ok(_) => Ok(None),
            Err(error) => Err(VmError::Native(error.to_string())),
        }
    }

    /// The canonical enum-constant object for `(class, name)`, allocated on first use.
    ///
    /// The VM does not run the enum's `<clinit>`, so this object is not the enum's static
    /// field value; it names the constant instead. Caching it makes two reads of the same
    /// annotation element reference-identical, which is what Java code that compares
    /// enum constants with `==` expects.
    ///
    /// Lock order is cache, then heap, matching [`Shared::class_object`].
    pub fn enum_constant(&self, class: ClassId, name: &str) -> Result<ObjRef, VmError> {
        if self.entry(class).is_none() {
            return Err(VmError::Loader(format!(
                "class id {} is not loaded",
                class.0
            )));
        }
        let mut cache = self
            .enum_constants
            .lock()
            .expect("enum-constant cache lock poisoned");
        let key = (class, name.to_owned());
        if let Some(reference) = cache.get(&key) {
            return Ok(*reference);
        }
        let reference = self
            .heap
            .lock()
            .expect("heap lock poisoned")
            .allocate_enum_constant(class, name);
        cache.insert(key, reference);
        Ok(reference)
    }

    /// The canonical string object for `text`, allocated on first use.
    ///
    /// `String.intern` promises one object per text, so this is a side table keyed by the
    /// content and holding the canonical [`ObjRef`]. The table owns the objects: they are
    /// never freed while `Shared` lives and [`Shared::roots`] reports them, so an interned
    /// string is a permanent root instead of inferred retention.
    ///
    /// Lock order is table, then heap, matching [`Shared::class_object`]. No path takes
    /// the heap lock before this cache, which keeps the pair deadlock-free.
    pub fn intern_string(&self, text: &str) -> ObjRef {
        let mut table = self
            .interned_strings
            .lock()
            .expect("interned-string table lock poisoned");
        if let Some(reference) = table.get(text) {
            return *reference;
        }
        let reference = self
            .heap
            .lock()
            .expect("heap lock poisoned")
            .allocate_string(text);
        table.insert(text.to_owned(), reference);
        reference
    }

    /// Allocate a lambda object over its interface, target and captured arguments.
    ///
    /// Lambdas are ordinary heap objects, not cached: each `invokedynamic` execution
    /// creates one, as the JDK creates one instance per call site activation. The
    /// interpreter's metafactory path is the only caller.
    pub fn allocate_lambda(&self, lambda: crate::heap::LambdaObject) -> ObjRef {
        self.heap().allocate_lambda(lambda)
    }

    /// Box a primitive value as a fresh wrapper heap object.
    ///
    /// There is no small-value cache: two calls with the same value return two distinct
    /// objects, so `==` on boxes is not the JDK's small-value identity. The box is an
    /// ordinary heap object, not a permanent root.
    pub fn boxed_value(&self, boxed: BoxedPrimitive) -> ObjRef {
        self.heap().allocate_boxed(boxed)
    }

    /// Box an `int` as a fresh `java.lang.Integer` heap object.
    pub fn box_int(&self, value: i32) -> ObjRef {
        self.boxed_value(BoxedPrimitive::Integer(value))
    }

    /// Box a `long` as a fresh `java.lang.Long` heap object.
    pub fn box_long(&self, value: i64) -> ObjRef {
        self.boxed_value(BoxedPrimitive::Long(value))
    }

    /// Box a `double` as a fresh `java.lang.Double` heap object.
    pub fn box_double(&self, value: f64) -> ObjRef {
        self.boxed_value(BoxedPrimitive::Double(value))
    }

    /// The boxed primitive behind a reference.
    ///
    /// A null reference and a non-boxed object are typed errors, so a caller never
    /// silently treats an unrelated reference as a box.
    pub fn boxed(&self, reference: ObjRef) -> Result<BoxedPrimitive, VmError> {
        if reference.is_null() {
            return Err(VmError::Native("cannot unbox a null reference".to_owned()));
        }
        let heap = self.heap();
        match heap.get(reference) {
            Ok(HeapObject::Boxed(boxed)) => Ok(*boxed),
            Ok(_) => Err(VmError::Native(format!(
                "{reference} is not a boxed primitive"
            ))),
            Err(error) => Err(VmError::Native(error.to_string())),
        }
    }

    /// The `int` inside a boxed `Integer`.
    ///
    /// A null reference, a non-boxed object and a box of another primitive kind are typed
    /// errors, so a caller never silently treats an unrelated reference as an `int`.
    pub fn unbox_int(&self, reference: ObjRef) -> Result<i32, VmError> {
        let boxed = self.boxed(reference)?;
        boxed.as_int().ok_or_else(|| {
            VmError::Native(format!(
                "{reference} is a boxed {}, not a boxed Integer",
                boxed.binary_name()
            ))
        })
    }

    /// The class object of a heap object: `Object.getClass` without the dispatch.
    ///
    /// An instance reports its defining class, a string reports `java.lang.String`, a
    /// class object (a loaded class or a primitive) reports `java.lang.Class`, a
    /// reflected member reports `java.lang.reflect.Field`, `Method` or `Constructor`, a
    /// box reports its boxed class (`java.lang.Integer`), an annotation reports
    /// `java.lang.annotation.Annotation` (its `annotationType()` names the annotation
    /// type), an enum constant reports its enum class, a `StringBuilder` reports
    /// `java.lang.StringBuilder`, an `ArrayList` reports `java.util.ArrayList`, a
    /// `HashMap` reports `java.util.HashMap`, a lambda reports its functional interface, a
    /// `MethodHandles.Lookup`, a `MethodType` and a `MethodHandle` report their
    /// `java.lang.invoke` class, and an array reports its cached array class (`int[]`,
    /// `String[][]`); those VM-level objects use the bootstrap class library as their
    /// identity.
    pub fn get_class(&self, reference: ObjRef) -> Result<ObjRef, VmError> {
        enum Kind {
            Instance(ClassId),
            Str,
            Class,
            Reflect(MemberKind),
            Boxed(BoxedPrimitive),
            Annotation,
            Enum(ClassId),
            StringBuilder,
            StringBuffer,
            MessageFormat,
            Named(ClassId),
            Array(ClassRef),
            Lambda(ClassId),
            Type(&'static str),
            LogLevel,
            Atomic(&'static str),
            Lock,
            Condition,
            Pattern,
            Matcher,
            Enumeration,
            Stream,
            Spliterator,
            Collector,
            Optional,
            Sink,
            Lookup,
            MethodType,
            MethodHandle,
        }

        let kind = {
            let heap = self.heap();
            match heap
                .get(reference)
                .map_err(|error| VmError::Native(error.to_string()))?
            {
                HeapObject::Instance(instance) => Kind::Instance(instance.class),
                HeapObject::Str(_) => Kind::Str,
                HeapObject::Class(_) => Kind::Class,
                HeapObject::PrimitiveClass(_) => Kind::Class,
                HeapObject::ArrayClass(_) => Kind::Class,
                HeapObject::Reflect(member) => Kind::Reflect(member.kind),
                HeapObject::Boxed(boxed) => Kind::Boxed(*boxed),
                HeapObject::Annotation(_) => Kind::Annotation,
                HeapObject::Enum(constant) => Kind::Enum(constant.class),
                HeapObject::StringBuilder(_) => Kind::StringBuilder,
                HeapObject::StringBuffer(_) => Kind::StringBuffer,
                HeapObject::MessageFormat(_) => Kind::MessageFormat,
                HeapObject::List(list) => Kind::Named(list.class),
                HeapObject::Set(set) => Kind::Named(set.class),
                HeapObject::Map(map) => Kind::Named(map.class),
                HeapObject::Iterator(iterator) => Kind::Named(iterator.class),
                HeapObject::MapEntry(_) => {
                    Kind::Named(self.load_library_class(crate::maps::MAP_ENTRY_CLASS)?)
                }
                HeapObject::LogLevel(_) => Kind::LogLevel,
                HeapObject::Atomic(cell) => Kind::Atomic(cell.class_name()),
                HeapObject::Lock(_) => Kind::Lock,
                HeapObject::FutureTask(_) => {
                    Kind::Named(self.load_library_class(crate::futures::FUTURE_TASK_CLASS)?)
                }
                HeapObject::BitSet(_) => {
                    Kind::Named(self.load_library_class(crate::bitset::BIT_SET_CLASS)?)
                }
                HeapObject::Condition(_) => Kind::Condition,
                HeapObject::Pattern(_) => Kind::Pattern,
                HeapObject::Matcher(_) => Kind::Matcher,
                HeapObject::Enumeration(_) => Kind::Enumeration,
                HeapObject::Stream(_) => Kind::Stream,
                HeapObject::Spliterator(_) => Kind::Spliterator,
                HeapObject::Collector(_) => Kind::Collector,
                HeapObject::Optional(_) => Kind::Optional,
                HeapObject::Sink(_) => Kind::Sink,
                HeapObject::Array(array) => Kind::Array(array.component.clone()),
                HeapObject::Lambda(lambda) => Kind::Lambda(lambda.interface),
                HeapObject::Type(ty) => Kind::Type(ty.class_name()),
                HeapObject::Lookup(_) => Kind::Lookup,
                HeapObject::MethodType(_) => Kind::MethodType,
                HeapObject::MethodHandle(_) => Kind::MethodHandle,
            }
        };

        match kind {
            Kind::Instance(class) => self.class_object(class),
            Kind::Lambda(interface) => self.class_object(interface),
            Kind::Str => {
                let class = self.load_library_class("java/lang/String")?;
                self.class_object(class)
            }
            Kind::Class => {
                let class = self.load_library_class("java/lang/Class")?;
                self.class_object(class)
            }
            Kind::Reflect(kind) => {
                let class = self.load_library_class(kind.class_name())?;
                self.class_object(class)
            }
            Kind::Boxed(boxed) => {
                let class = self.load_library_class(boxed.class_name())?;
                self.class_object(class)
            }
            Kind::Annotation => {
                let class = self.load_library_class("java/lang/annotation/Annotation")?;
                self.class_object(class)
            }
            Kind::Enum(class) => self.class_object(class),
            Kind::StringBuilder => {
                let class = self.load_library_class(crate::collections::STRING_BUILDER_CLASS)?;
                self.class_object(class)
            }
            Kind::StringBuffer => {
                let class = self.load_library_class(crate::format::STRING_BUFFER_CLASS)?;
                self.class_object(class)
            }
            Kind::MessageFormat => {
                let class = self.load_library_class(crate::format::MESSAGE_FORMAT_CLASS)?;
                self.class_object(class)
            }
            Kind::Named(class) => self.class_object(class),
            Kind::Array(component) => self.array_class(component),
            Kind::Type(name) => {
                let class = self.load_library_class(name)?;
                self.class_object(class)
            }
            Kind::LogLevel => {
                let class = self.load_library_class("java/util/logging/Level")?;
                self.class_object(class)
            }
            Kind::Atomic(name) => {
                let class = self.load_library_class(name)?;
                self.class_object(class)
            }
            Kind::Lock => {
                let class = self.load_library_class(crate::locks::REENTRANT_LOCK_CLASS)?;
                self.class_object(class)
            }
            Kind::Condition => {
                let class = self.load_library_class(crate::locks::CONDITION_CLASS)?;
                self.class_object(class)
            }
            Kind::Pattern => {
                let class = self.load_library_class(crate::regex::PATTERN_CLASS)?;
                self.class_object(class)
            }
            Kind::Matcher => {
                let class = self.load_library_class(crate::regex::MATCHER_CLASS)?;
                self.class_object(class)
            }
            Kind::Enumeration => {
                let class = self.load_library_class("java/util/Enumeration")?;
                self.class_object(class)
            }
            Kind::Stream => {
                let class = self.load_library_class("java/util/stream/Stream")?;
                self.class_object(class)
            }
            Kind::Spliterator => {
                let class = self.load_library_class("java/util/Spliterator")?;
                self.class_object(class)
            }
            Kind::Collector => {
                let class = self.load_library_class("java/util/stream/Collector")?;
                self.class_object(class)
            }
            Kind::Optional => {
                let class = self.load_library_class("java/util/Optional")?;
                self.class_object(class)
            }
            Kind::Sink => {
                let class = self.load_library_class("java/util/function/Consumer")?;
                self.class_object(class)
            }
            Kind::Lookup => {
                let class = self.load_library_class("java/lang/invoke/MethodHandles$Lookup")?;
                self.class_object(class)
            }
            Kind::MethodType => {
                let class = self.load_library_class("java/lang/invoke/MethodType")?;
                self.class_object(class)
            }
            Kind::MethodHandle => {
                let class = self.load_library_class("java/lang/invoke/MethodHandle")?;
                self.class_object(class)
            }
        }
    }

    /// The instance-field layout of a class: superclass slots first, then this class's
    /// non-static fields in class-file order.
    ///
    /// This is the layout [`crate::Heap::allocate_instance`] fills with defaults and the
    /// one `getfield`/`putfield` index into, so a field resolved in a superclass keeps its
    /// index in every subclass. Static fields are excluded: the layout describes object
    /// state only. The superclass chain is walked through the defining loader, so the
    /// layout is built from the same class identities as the rest of the VM.
    pub fn field_layout(&self, class: ClassId) -> Result<FieldLayout, VmError> {
        let mut layout = FieldLayout::new();
        self.collect_instance_fields(class, &mut layout)?;
        Ok(layout)
    }

    /// The superclass a class-file walk should continue at.
    ///
    /// `java.lang.StringBuilder` extends the package-private
    /// `java.lang.AbstractStringBuilder`, which `ct.sym` omits, so the committed C1 stub
    /// tree has no class file for it. The VM's class library has no members there, so the
    /// walk continues at `java.lang.Object` instead of failing to load a class the library
    /// does not ship. Every other class resolves the superclass its class file names, and
    /// a genuinely missing superclass is still a loader error.
    pub fn resolution_superclass(&self, class: ClassId) -> Result<Option<ClassId>, VmError> {
        let entry = self
            .entry(class)
            .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", class.0)))?;
        let Some(super_name) = entry.file.super_class_name()? else {
            return Ok(None);
        };
        let super_name = super_name.to_owned();
        let super_name = if (entry.name == crate::collections::STRING_BUILDER_CLASS
            || entry.name == crate::format::STRING_BUFFER_CLASS)
            && super_name == "java/lang/AbstractStringBuilder"
        {
            "java/lang/Object".to_owned()
        } else if super_name == "java/util/Dictionary" {
            // `ct.sym`'s release-17 view omits `java.util.Dictionary` even though
            // `Hashtable` extends it. `Dictionary` declares no instance fields and only
            // abstract methods, so the walk continues at `Object`; `Hashtable` itself
            // carries the fields and concrete members the VM uses.
            "java/lang/Object".to_owned()
        } else {
            super_name
        };
        Ok(Some(self.load_class(entry.loader, &super_name)?))
    }

    fn collect_instance_fields(
        &self,
        class: ClassId,
        layout: &mut FieldLayout,
    ) -> Result<(), VmError> {
        let entry = self
            .entry(class)
            .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", class.0)))?;
        // Load the superclass before pushing this class's fields, so the parent prefix
        // stays at the front of the layout.
        if let Some(super_class) = self.resolution_superclass(class)? {
            self.collect_instance_fields(super_class, layout)?;
        }
        for field in &entry.file.fields {
            if !field.is_static() {
                layout.push(
                    entry.file.field_name(field)?,
                    entry.file.field_descriptor(field)?,
                );
            }
        }
        Ok(())
    }

    /// Allocate an instance of `class`, with defaults for every field in its layout.
    ///
    /// `java.lang.StringBuilder` and `java.util.ArrayList` are VM-level objects rather
    /// than `HeapObject::Instance`s, so their class identities select a dedicated heap
    /// representation. Their constructors are normally the first thing to run, and they
    /// fill the empty object. Every other class gets the ordinary instance layout.
    pub fn allocate_instance(&self, class: ClassId) -> Result<ObjRef, VmError> {
        let entry = self
            .entry(class)
            .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", class.0)))?;
        let class = entry.id;
        let name = entry.name.clone();
        match name.as_str() {
            crate::collections::STRING_BUILDER_CLASS => {
                return Ok(self.heap().allocate_string_builder());
            }
            crate::format::STRING_BUFFER_CLASS => {
                return Ok(self.heap().allocate_string_buffer());
            }
            crate::format::MESSAGE_FORMAT_CLASS => {
                return Ok(self.heap().allocate_message_format(String::new()));
            }
            "java/lang/String" => {
                return Ok(self.heap().allocate_string(String::new()));
            }
            crate::locks::REENTRANT_LOCK_CLASS => {
                return Ok(self.heap().allocate_lock());
            }
            crate::futures::FUTURE_TASK_CLASS => {
                return Ok(self.heap().allocate_future_task(ObjRef::NULL));
            }
            crate::bitset::BIT_SET_CLASS => {
                return Ok(self.heap().allocate_bit_set());
            }
            crate::atomic::ATOMIC_BOOLEAN_CLASS
            | crate::atomic::ATOMIC_INTEGER_CLASS
            | crate::atomic::ATOMIC_LONG_CLASS
            | crate::atomic::ATOMIC_REFERENCE_CLASS => {
                return Ok(self
                    .heap()
                    .allocate_atomic(crate::atomic::initial_cell(&name)));
            }
            _ => {}
        }
        // A Spring class may extend a VM container shape and add fields (for example
        // `AbstractBeanFactory$BeanPostProcessorCacheAwareList` extends
        // `CopyOnWriteArrayList` and adds `this$0`). The shape is still selected from the
        // superclass chain, and the subclass's own field layout starts at its defaults.
        if let Some(kind) = self.container_kind(class)? {
            // A container base class keeps its state in the heap shape, not in instance
            // fields, so its layout is empty and its superclass chain is not consulted.
            // `LinkedList`'s superclass is `AbstractSequentialList`, which the C1 tree
            // does not project (it is not a manifest owner); allocating a linked list must
            // not fail because of a field layout it never uses.
            let fields = if Self::is_container_base(&name) {
                Vec::new()
            } else {
                self.field_layout(class)?.defaults()
            };
            return Ok(match kind {
                VmContainerKind::List => self.heap().allocate_list_with_fields(class, fields),
                VmContainerKind::Set => self.heap().allocate_set_with_fields(class, fields),
                VmContainerKind::Map => self.heap().allocate_map_with_fields(class, fields),
            });
        }
        let layout = self.field_layout(class)?;
        Ok(self.heap().allocate_instance(class, &layout))
    }

    /// The VM container shape a class is or extends, if any.
    fn container_kind(&self, class: ClassId) -> Result<Option<VmContainerKind>, VmError> {
        let mut current = Some(class);
        while let Some(id) = current {
            let entry = self
                .entry(id)
                .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", id.0)))?;
            match entry.name.as_str() {
                crate::collections::ARRAY_LIST_CLASS
                | crate::collections::LINKED_LIST_CLASS
                | crate::collections::COPY_ON_WRITE_ARRAY_LIST_CLASS
                | crate::collections::ARRAY_DEQUE_CLASS => return Ok(Some(VmContainerKind::List)),
                crate::collections::LINKED_HASH_SET_CLASS
                | crate::collections::HASH_SET_CLASS
                | crate::collections::TREE_SET_CLASS
                | crate::collections::COPY_ON_WRITE_ARRAY_SET_CLASS => {
                    return Ok(Some(VmContainerKind::Set));
                }
                crate::maps::HASH_MAP_CLASS
                | crate::maps::LINKED_HASH_MAP_CLASS
                | crate::maps::HASHTABLE_CLASS
                | crate::maps::PROPERTIES_CLASS
                | crate::maps::TREE_MAP_CLASS
                | crate::maps::WEAK_HASH_MAP_CLASS
                | crate::maps::IDENTITY_HASH_MAP_CLASS
                | crate::maps::CONCURRENT_HASH_MAP_CLASS => return Ok(Some(VmContainerKind::Map)),
                _ => {}
            }
            current = self.resolution_superclass(id)?;
        }
        Ok(None)
    }

    /// Whether `name` is one of the VM container base classes.
    ///
    /// A container's state lives in its heap shape ([`crate::heap::HeapObject::List`], `Set`
    /// or `Map`), so its class-file fields are not part of an instance layout.
    fn is_container_base(name: &str) -> bool {
        matches!(
            name,
            crate::collections::ARRAY_LIST_CLASS
                | crate::collections::LINKED_LIST_CLASS
                | crate::collections::COPY_ON_WRITE_ARRAY_LIST_CLASS
                | crate::collections::ARRAY_DEQUE_CLASS
                | crate::collections::LINKED_HASH_SET_CLASS
                | crate::collections::HASH_SET_CLASS
                | crate::collections::TREE_SET_CLASS
                | crate::collections::COPY_ON_WRITE_ARRAY_SET_CLASS
                | crate::maps::HASH_MAP_CLASS
                | crate::maps::LINKED_HASH_MAP_CLASS
                | crate::maps::HASHTABLE_CLASS
                | crate::maps::PROPERTIES_CLASS
                | crate::maps::TREE_MAP_CLASS
                | crate::maps::WEAK_HASH_MAP_CLASS
                | crate::maps::IDENTITY_HASH_MAP_CLASS
                | crate::maps::CONCURRENT_HASH_MAP_CLASS
        )
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

    /// Every loaded class entry, in class-table order.
    ///
    /// A class generated while a program runs has no name to look up in advance, so a
    /// caller that wants to inspect it scans the table instead. The returned entries are
    /// cheap `Arc` clones; the table lock is released before the caller reads them.
    pub fn class_entries(&self) -> Vec<Arc<ClassEntry>> {
        self.classes
            .read()
            .expect("class table lock poisoned")
            .entries()
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

    /// Collect the VM roots: every non-null class static field, every cached class object
    /// (a loaded class, a primitive or an array class), every reflected member, the
    /// references held by a heap list or map, plus the roots the given threads' active
    /// frames published.
    ///
    /// The class table is snapshotted before any per-class statics lock is taken, and
    /// each statics lock is released before the next class is visited, so this never
    /// holds one shared lock while another is acquired. The class and member objects are
    /// permanent (their caches never free them) and are listed so a future collector sees
    /// the retention instead of inferring it. This returns the roots only, not their
    /// transitive closure; M0 has no collector.
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

        {
            let class_objects = self
                .class_objects
                .lock()
                .expect("class-object cache lock poisoned");
            for reference in class_objects.values() {
                if seen.insert(*reference) {
                    roots.push(*reference);
                }
            }
        }

        {
            let primitive_classes = self
                .primitive_classes
                .lock()
                .expect("primitive-class cache lock poisoned");
            for reference in primitive_classes.values() {
                if seen.insert(*reference) {
                    roots.push(*reference);
                }
            }
        }

        {
            let array_classes = self
                .array_classes
                .lock()
                .expect("array-class cache lock poisoned");
            for reference in array_classes.values() {
                if seen.insert(*reference) {
                    roots.push(*reference);
                }
            }
        }

        {
            let reflect_members = self
                .reflect_members
                .lock()
                .expect("reflected-member cache lock poisoned");
            for reference in reflect_members.values() {
                if seen.insert(*reference) {
                    roots.push(*reference);
                }
            }
        }

        {
            let type_objects = self
                .type_objects
                .lock()
                .expect("generic-type cache lock poisoned");
            for (ty, reference) in type_objects.iter() {
                if seen.insert(*reference) {
                    roots.push(*reference);
                }
                for nested in ty.references() {
                    if !nested.is_null() && seen.insert(nested) {
                        roots.push(nested);
                    }
                }
            }
        }

        {
            let enum_constants = self
                .enum_constants
                .lock()
                .expect("enum-constant cache lock poisoned");
            for reference in enum_constants.values() {
                if seen.insert(*reference) {
                    roots.push(*reference);
                }
            }
        }

        {
            let interned_strings = self
                .interned_strings
                .lock()
                .expect("interned-string table lock poisoned");
            for reference in interned_strings.values() {
                if seen.insert(*reference) {
                    roots.push(*reference);
                }
            }
        }

        {
            let loader_objects = self
                .loader_objects
                .lock()
                .expect("loader-object cache lock poisoned");
            for reference in loader_objects.values() {
                if seen.insert(*reference) {
                    roots.push(*reference);
                }
            }
        }

        {
            // Spring keeps one logger per class name and a `LogRecord` may carry a
            // throwable; both are permanent roots.
            let jul = self.jul.lock().expect("jul lock poisoned");
            for reference in jul.loggers.values() {
                if seen.insert(*reference) {
                    roots.push(*reference);
                }
            }
            if let Some(reference) = self.main_thread() {
                if seen.insert(reference) {
                    roots.push(reference);
                }
            }
            let thread_locals = self
                .thread_locals
                .lock()
                .expect("thread-local lock poisoned");
            for value in thread_locals.values() {
                push_container_root(&mut roots, &mut seen, *value);
            }
            let references = self.references.lock().expect("reference lock poisoned");
            for referent in references.values() {
                if !referent.is_null() && seen.insert(*referent) {
                    roots.push(*referent);
                }
            }
            for record in jul.records.values() {
                if let Some(reference) = record.thrown {
                    if !reference.is_null() && seen.insert(reference) {
                        roots.push(reference);
                    }
                }
            }
        }

        {
            // A container holds references that are not themselves roots of the class
            // table or a cache. A list's elements and a map's keys and values are
            // enumerated here so a future collector sees them; there is no collector yet,
            // so this over-retains an element whose container is otherwise unreachable,
            // which is the honest state until M5 traverses the object graph. A null
            // key or value is stored by the map but is not a root, exactly as a null
            // element is not one.
            let heap = self.heap.lock().expect("heap lock poisoned");
            for object in heap.live_objects() {
                match object {
                    HeapObject::List(list) => {
                        for value in &list.elements {
                            push_container_root(&mut roots, &mut seen, *value);
                        }
                    }
                    HeapObject::Set(set) => {
                        for value in &set.elements {
                            push_container_root(&mut roots, &mut seen, *value);
                        }
                    }
                    HeapObject::Map(map) => {
                        for (key, value) in &map.entries {
                            push_container_root(&mut roots, &mut seen, *key);
                            push_container_root(&mut roots, &mut seen, *value);
                        }
                    }
                    HeapObject::Iterator(iterator) => {
                        for value in &iterator.items {
                            push_container_root(&mut roots, &mut seen, *value);
                        }
                    }
                    HeapObject::MapEntry(entry) => {
                        push_container_root(&mut roots, &mut seen, entry.key);
                        push_container_root(&mut roots, &mut seen, entry.value);
                    }
                    HeapObject::Atomic(crate::heap::AtomicCell::Reference(value)) => {
                        push_container_root(&mut roots, &mut seen, *value);
                    }
                    HeapObject::FutureTask(task) => {
                        if !task.callable.is_null() && seen.insert(task.callable) {
                            roots.push(task.callable);
                        }
                        match &task.outcome {
                            Some(Ok(Some(value))) => {
                                push_container_root(&mut roots, &mut seen, *value)
                            }
                            Some(Err(throwable)) => {
                                push_container_root(&mut roots, &mut seen, Value::Ref(*throwable));
                            }
                            _ => {}
                        }
                    }
                    HeapObject::Condition(condition) => {
                        if !condition.lock.is_null() && seen.insert(condition.lock) {
                            roots.push(condition.lock);
                        }
                    }
                    HeapObject::Lambda(lambda) => {
                        for value in &lambda.captured {
                            push_container_root(&mut roots, &mut seen, *value);
                        }
                    }
                    HeapObject::Stream(stream) => {
                        for value in &stream.elements {
                            push_container_root(&mut roots, &mut seen, *value);
                        }
                    }
                    HeapObject::Spliterator(spliterator) => {
                        for value in &spliterator.elements {
                            push_container_root(&mut roots, &mut seen, *value);
                        }
                    }
                    HeapObject::Collector(collector) => {
                        for reference in collector.references() {
                            if !reference.is_null() && seen.insert(reference) {
                                roots.push(reference);
                            }
                        }
                    }
                    HeapObject::Optional(Some(value)) => {
                        push_container_root(&mut roots, &mut seen, *value);
                    }
                    HeapObject::Sink(sink)
                        if !sink.destination.is_null() && seen.insert(sink.destination) =>
                    {
                        roots.push(sink.destination);
                    }
                    _ => {}
                }
            }
        }

        for thread in threads {
            // Frames and held monitors are both roots: a `monitorenter` keeps its object
            // alive for the duration of the critical section.
            for reference in thread
                .frame_roots()
                .into_iter()
                .chain(thread.monitor_roots())
            {
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
            let _guard = self.class_objects.try_lock().map_err(|_| LockHeld {
                lock: SharedLock::ClassObjects,
            })?;
        }
        {
            let _guard = self.primitive_classes.try_lock().map_err(|_| LockHeld {
                lock: SharedLock::PrimitiveClasses,
            })?;
        }
        {
            let _guard = self.array_classes.try_lock().map_err(|_| LockHeld {
                lock: SharedLock::ArrayClasses,
            })?;
        }
        {
            let _guard = self.reflect_members.try_lock().map_err(|_| LockHeld {
                lock: SharedLock::ReflectMembers,
            })?;
        }
        {
            let _guard = self.type_objects.try_lock().map_err(|_| LockHeld {
                lock: SharedLock::TypeObjects,
            })?;
        }
        {
            let _guard = self.enum_constants.try_lock().map_err(|_| LockHeld {
                lock: SharedLock::EnumConstants,
            })?;
        }
        {
            let _guard = self.interned_strings.try_lock().map_err(|_| LockHeld {
                lock: SharedLock::InternedStrings,
            })?;
        }
        {
            let _guard = self.proxy_classes.try_lock().map_err(|_| LockHeld {
                lock: SharedLock::ProxyClasses,
            })?;
        }
        {
            let _guard = self.loader_objects.try_lock().map_err(|_| LockHeld {
                lock: SharedLock::LoaderObjects,
            })?;
        }
        {
            let _guard = self.object_loaders.try_lock().map_err(|_| LockHeld {
                lock: SharedLock::ObjectLoaders,
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
        {
            let _guard = self.stderr.try_lock().map_err(|_| LockHeld {
                lock: SharedLock::Stderr,
            })?;
        }
        {
            let _guard = self.unsafe_calls.try_lock().map_err(|_| LockHeld {
                lock: SharedLock::UnsafeCalls,
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

    /// Append one served `Unsafe` call to the U0 log (ADR-0005 acceptance).
    ///
    /// The format is `owner.name descriptor`, for example
    /// `sun/misc/Unsafe.getUnsafe()Lsun/misc/Unsafe;`. The lock is scoped to the push and
    /// is never held across a Java call.
    pub fn record_unsafe_call(&self, call: impl Into<String>) {
        self.unsafe_calls
            .lock()
            .expect("Unsafe call log lock poisoned")
            .push(call.into());
    }

    /// The served `Unsafe` calls, oldest first.
    ///
    /// ADR-0005 uses this as the acceptance probe: at M3 every entry names a U0 method,
    /// and a `U1`+ call leaves no entry because no native serves it.
    pub fn unsafe_calls(&self) -> Vec<String> {
        self.unsafe_calls
            .lock()
            .expect("Unsafe call log lock poisoned")
            .clone()
    }

    /// Declare `class` initialized without running its `<clinit>`.
    ///
    /// The VM's own installation steps use this when the C1 stub's `<clinit>` names
    /// members the open tier does not implement but the fields the VM needs are installed
    /// directly: `java.nio.ByteBuffer`'s initializer reads `Buffer.UNSAFE` and calls a U1
    /// array accessor, and `sun.misc.Unsafe`'s initializer calls
    /// `jdk/internal/reflect/Reflection` and U1/U3 members. This is the same statement
    /// [`crate::Shared::static_field_value`] already makes for an enum, whose constants
    /// are installed instead of running its initializer. A class that genuinely failed
    /// initialization is not overwritten.
    pub fn mark_initialized(&self, class: ClassId) -> Result<(), VmError> {
        let entry = self
            .entry(class)
            .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", class.0)))?;
        let mut state = entry.state.lock().expect("class state lock poisoned");
        if matches!(*state, ClassState::Erroneous) {
            return Err(VmError::Loader(format!(
                "class {} failed initialization",
                entry.name
            )));
        }
        *state = ClassState::Initialized;
        Ok(())
    }

    /// Record a throwable's detail message.
    pub fn set_throwable_message(&self, reference: ObjRef, message: String) {
        self.throwable_messages
            .lock()
            .expect("throwable lock poisoned")
            .insert(reference, message);
    }

    /// A throwable's detail message, if a constructor recorded one.
    pub fn throwable_message(&self, reference: ObjRef) -> Option<String> {
        self.throwable_messages
            .lock()
            .expect("throwable lock poisoned")
            .get(&reference)
            .cloned()
    }

    /// The `java.lang.ClassLoader` object that represents `loader`, allocated on first use.
    ///
    /// One opaque `java.lang.ClassLoader` instance stands for each non-bootstrap loader, so
    /// `Class.getClassLoader`, `getParent` and `getSystemClassLoader` can compare loader
    /// identity by object identity. The cache is the owner of the objects: they are never
    /// freed while `Shared` lives and [`Shared::roots`] reports them as roots.
    ///
    /// A user `ClassLoader` subclass instance is not replaced by an opaque stand-in:
    /// [`Shared::loader_id_for`] records that instance as the object for the child loader it
    /// creates, so `getClassLoader` returns the actual object the Java program made.
    ///
    /// Lock order is cache, then heap, matching [`Shared::class_object`]. No Java call is
    /// made while the cache lock is held.
    pub fn loader_object(&self, loader: LoaderId) -> Result<ObjRef, VmError> {
        if !self.loader_exists(loader) {
            return Err(VmError::Loader(format!(
                "loader {} does not exist",
                loader.0
            )));
        }
        {
            let cache = self
                .loader_objects
                .lock()
                .expect("loader-object cache lock poisoned");
            if let Some(reference) = cache.get(&loader) {
                return Ok(*reference);
            }
        }
        let class = self.load_library_class("java/lang/ClassLoader")?;
        let mut cache = self
            .loader_objects
            .lock()
            .expect("loader-object cache lock poisoned");
        // A concurrent caller may have allocated the object while this one loaded the
        // class. The VM is single-threaded until M5, but the recheck keeps the cache shape
        // independent of that assumption.
        if let Some(reference) = cache.get(&loader) {
            return Ok(*reference);
        }
        let reference = self.allocate_instance(class)?;
        cache.insert(loader, reference);
        drop(cache);
        self.object_loaders
            .lock()
            .expect("object-to-loader map lock poisoned")
            .insert(reference, loader);
        Ok(reference)
    }

    /// The `ClassLoader` object reported for the application loader.
    ///
    /// This is the object `getSystemClassLoader`/`getPlatformClassLoader` return and the one
    /// `Class.getClassLoader` reports for an application class.
    pub fn class_loader_object(&self) -> Result<ObjRef, VmError> {
        self.loader_object(self.application_loader())
    }

    /// The loader a `java.lang.ClassLoader` receiver denotes.
    ///
    /// A receiver the VM allocated as a loader stand-in is already mapped. A subclass
    /// instance a Java program created with `new` is not: this creates a child loader under
    /// the application loader with an empty class source, binds the instance to it, and
    /// returns it. The child delegates every lookup to its parent and only defines the
    /// classes its `defineClass` calls provide, which is exactly the `ClassLoader` contract
    /// `OverridingClassLoader` and CGLIB's fallback rely on.
    pub fn loader_id_for(&self, receiver: ObjRef) -> Result<LoaderId, VmError> {
        if let Some(loader) = self
            .object_loaders
            .lock()
            .expect("object-to-loader map lock poisoned")
            .get(&receiver)
            .copied()
        {
            return Ok(loader);
        }
        if receiver.is_null() {
            return Err(VmError::Native(
                "a null reference is not a java.lang.ClassLoader".to_owned(),
            ));
        }
        let class = {
            let heap = self.heap();
            heap.object_class(receiver).map_err(|_| {
                VmError::Native(format!("{receiver} is not a java.lang.ClassLoader"))
            })?
        };
        let class_loader = self.load_library_class("java/lang/ClassLoader")?;
        let mut current = Some(class);
        let mut is_class_loader = false;
        while let Some(id) = current {
            if id == class_loader {
                is_class_loader = true;
                break;
            }
            current = self.resolution_superclass(id)?;
        }
        if !is_class_loader {
            return Err(VmError::Native(format!(
                "{receiver} is not a java.lang.ClassLoader"
            )));
        }

        let loader = self.add_child_loader(
            self.application_loader(),
            Arc::new(crate::source::ClassPathSource::empty()),
        )?;
        self.loader_objects
            .lock()
            .expect("loader-object cache lock poisoned")
            .insert(loader, receiver);
        self.object_loaders
            .lock()
            .expect("object-to-loader map lock poisoned")
            .insert(receiver, loader);
        Ok(loader)
    }

    /// The value stored in a `ThreadLocal`, if it was set.
    pub fn thread_local(&self, reference: ObjRef) -> Option<Value> {
        self.thread_locals
            .lock()
            .expect("thread-local lock poisoned")
            .get(&reference)
            .copied()
    }

    /// Store a value in a `ThreadLocal`.
    pub fn set_thread_local(&self, reference: ObjRef, value: Value) {
        self.thread_locals
            .lock()
            .expect("thread-local lock poisoned")
            .insert(reference, value);
    }

    /// Remove a `ThreadLocal`'s value.
    pub fn remove_thread_local(&self, reference: ObjRef) {
        self.thread_locals
            .lock()
            .expect("thread-local lock poisoned")
            .remove(&reference);
    }

    /// The cached object `Thread.currentThread` returns, if one has been created.
    pub fn main_thread(&self) -> Option<ObjRef> {
        *self
            .main_thread
            .lock()
            .expect("thread-object lock poisoned")
    }

    /// Cache the object `Thread.currentThread` returns.
    pub fn set_main_thread(&self, reference: ObjRef) {
        *self
            .main_thread
            .lock()
            .expect("thread-object lock poisoned") = Some(reference);
    }

    /// Record a reference object's referent.
    pub fn set_referent(&self, reference: ObjRef, referent: ObjRef) {
        self.references
            .lock()
            .expect("reference lock poisoned")
            .insert(reference, referent);
    }

    /// The referent of a reference object, or null when it has been cleared.
    pub fn referent(&self, reference: ObjRef) -> Option<ObjRef> {
        self.references
            .lock()
            .expect("reference lock poisoned")
            .get(&reference)
            .copied()
    }

    /// Clear a reference object's referent.
    pub fn clear_referent(&self, reference: ObjRef) {
        self.references
            .lock()
            .expect("reference lock poisoned")
            .insert(reference, ObjRef::NULL);
    }

    /// The state of a lock object, whether it is the `ReentrantLock` heap shape or an
    /// ordinary instance (a subclass such as Spring's `Segment`).
    pub fn lock_state(&self, reference: ObjRef) -> Option<crate::heap::LockState> {
        if let Ok(crate::HeapObject::Lock(state)) = self.heap().get(reference) {
            return Some(state.clone());
        }
        self.locks
            .lock()
            .expect("lock-state lock poisoned")
            .get(&reference)
            .cloned()
    }

    /// Replace a lock object's state.
    pub fn set_lock_state(&self, reference: ObjRef, state: crate::heap::LockState) {
        self.update_lock(reference, |cell| *cell = state);
    }

    /// Read-modify-write a lock object's state.
    ///
    /// A plain `ReentrantLock` lives in the heap shape; a subclass instance (Spring's
    /// `Segment`) lives in the side table. The closure runs under the owning lock, so the
    /// read and the write cannot interleave.
    pub fn update_lock<T>(
        &self,
        reference: ObjRef,
        update: impl FnOnce(&mut crate::heap::LockState) -> T,
    ) -> T {
        {
            let mut heap = self.heap();
            if let Ok(crate::HeapObject::Lock(state)) = heap.get_mut(reference) {
                return update(state);
            }
        }
        let mut locks = self.locks.lock().expect("lock-state lock poisoned");
        let state = locks.entry(reference).or_default();
        update(state)
    }

    /// Record an enum instance's constant name and ordinal.
    pub fn set_enum(&self, reference: ObjRef, name: String, ordinal: i32) {
        self.enums
            .lock()
            .expect("enum lock poisoned")
            .insert(reference, (name, ordinal));
    }

    /// The constant name of an ordinary enum instance, if its constructor ran.
    pub fn enum_name(&self, reference: ObjRef) -> Option<String> {
        self.enums
            .lock()
            .expect("enum lock poisoned")
            .get(&reference)
            .map(|(name, _)| name.clone())
    }

    /// The ordinal of an ordinary enum instance, if its constructor ran.
    pub fn enum_ordinal(&self, reference: ObjRef) -> Option<i32> {
        self.enums
            .lock()
            .expect("enum lock poisoned")
            .get(&reference)
            .map(|(_, ordinal)| *ordinal)
    }

    /// The value of one system property, if it is set.
    pub fn property(&self, name: &str) -> Option<String> {
        self.properties
            .lock()
            .expect("property lock poisoned")
            .get(name)
            .cloned()
    }

    /// Set a system property and return the previous value.
    pub fn set_property(&self, name: &str, value: &str) -> Option<String> {
        self.properties
            .lock()
            .expect("property lock poisoned")
            .insert(name.to_owned(), value.to_owned())
    }

    /// Remove a system property and return its previous value.
    pub fn remove_property(&self, name: &str) -> Option<String> {
        self.properties
            .lock()
            .expect("property lock poisoned")
            .remove(name)
    }

    /// The whole system property table.
    pub fn properties(&self) -> Vec<(String, String)> {
        self.properties
            .lock()
            .expect("property lock poisoned")
            .iter()
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect()
    }

    /// The minimal `java.util.logging` state.
    pub(crate) fn jul(&self) -> std::sync::MutexGuard<'_, crate::jul::JulState> {
        self.jul.lock().expect("jul lock poisoned")
    }

    /// Append bytes to the captured error stream.
    ///
    /// `java.util.logging` writes here, so Spring's diagnostics never pollute `System.out`.
    /// The runner flushes the buffer to the process's stderr.
    pub fn write_stderr(&self, bytes: &[u8]) {
        self.stderr
            .lock()
            .expect("stderr lock poisoned")
            .extend_from_slice(bytes);
    }

    /// The captured error stream as text.
    pub fn stderr(&self) -> String {
        String::from_utf8_lossy(&self.stderr.lock().expect("stderr lock poisoned")).into_owned()
    }
}

/// Add a container element to the root set unless it is null or already present.
///
/// A list element and a map key or value use the same rule: only a non-null reference is a
/// root, and the first occurrence wins so the result is deterministic.
fn push_container_root(roots: &mut Vec<ObjRef>, seen: &mut HashSet<ObjRef>, value: Value) {
    let Value::Ref(reference) = value else {
        return;
    };
    if !reference.is_null() && seen.insert(reference) {
        roots.push(reference);
    }
}

/// The internal name of an object descriptor (`Lname;`), if `descriptor` has that shape.
fn object_descriptor_name(descriptor: &str) -> Option<&str> {
    descriptor
        .strip_prefix('L')
        .and_then(|rest| rest.strip_suffix(';'))
}

fn malformed_descriptor(descriptor: &str) -> VmError {
    VmError::Native(format!("malformed descriptor {descriptor}"))
}

fn malformed_array_descriptor(descriptor: &str) -> VmError {
    VmError::Native(format!("malformed array descriptor {descriptor}"))
}
