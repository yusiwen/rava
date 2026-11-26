//! `java.beans.Introspector` helpers (M4.5).
//!
//! Spring's bean-property code calls `Introspector.decapitalize` to turn a property name
//! into an accessor suffix. The full `Introspector`/`BeanInfo`/`PropertyDescriptor`
//! machinery is a larger subsystem; this slice supplies the one static method the pinned
//! startup path uses and reports anything else by name.
//!
//! The JDK rule: an empty name is unchanged, a name whose first two characters are both
//! upper case is unchanged (`URL` stays `URL`), and otherwise the first character is
//! lower-cased.

use std::collections::HashMap;

use crate::VmError;
use crate::heap::ObjRef;
use crate::loader::LoaderId;
use crate::shared::Shared;
use crate::thread::ThreadState;
use crate::value::Value;

const INTROSPECTOR_CLASS: &str = "java/beans/Introspector";
const PROPERTY_DESCRIPTOR_CLASS: &str = "java/beans/PropertyDescriptor";
const FEATURE_DESCRIPTOR_CLASS: &str = "java/beans/FeatureDescriptor";
const SIMPLE_BEAN_INFO_CLASS: &str = "java/beans/SimpleBeanInfo";

/// One `PropertyDescriptor`: its name and accessor methods.
#[derive(Debug, Clone)]
pub struct DescriptorData {
    pub name: String,
    pub read: ObjRef,
    pub write: ObjRef,
    pub display_name: Option<String>,
    pub expert: bool,
    pub hidden: bool,
    pub bound: bool,
    pub constrained: bool,
}

/// The `java.beans` state owned by [`Shared`].
#[derive(Debug, Default)]
pub struct BeanState {
    pub descriptors: HashMap<ObjRef, DescriptorData>,
}

impl BeanState {
    /// Every descriptor object, for root enumeration.
    pub fn references(&self) -> Vec<ObjRef> {
        self.descriptors.keys().copied().collect()
    }
}

pub fn register_natives(shared: &Shared) {
    shared.register_native(
        INTROSPECTOR_CLASS,
        "decapitalize",
        "(Ljava/lang/String;)Ljava/lang/String;",
        introspector_decapitalize,
    );
    shared.register_native(
        PROPERTY_DESCRIPTOR_CLASS,
        "<init>",
        "(Ljava/lang/String;Ljava/lang/reflect/Method;Ljava/lang/reflect/Method;)V",
        property_descriptor_init,
    );
    shared.register_native(SIMPLE_BEAN_INFO_CLASS, "<init>", "()V", noop);
    for class in [PROPERTY_DESCRIPTOR_CLASS, FEATURE_DESCRIPTOR_CLASS] {
        shared.register_native(class, "getName", "()Ljava/lang/String;", descriptor_name);
        shared.register_native(
            class,
            "getDisplayName",
            "()Ljava/lang/String;",
            descriptor_display_name,
        );
        shared.register_native(
            class,
            "setDisplayName",
            "(Ljava/lang/String;)V",
            descriptor_set_display_name,
        );
        shared.register_native(class, "isExpert", "()Z", descriptor_is_expert);
        shared.register_native(class, "setExpert", "(Z)V", descriptor_set_expert);
        shared.register_native(class, "isHidden", "()Z", descriptor_is_hidden);
        shared.register_native(class, "setHidden", "(Z)V", descriptor_set_hidden);
        shared.register_native(
            class,
            "toString",
            "()Ljava/lang/String;",
            descriptor_to_string,
        );
        shared.register_native(class, "equals", "(Ljava/lang/Object;)Z", descriptor_equals);
        shared.register_native(class, "hashCode", "()I", descriptor_hash_code);
    }
    shared.register_native(
        PROPERTY_DESCRIPTOR_CLASS,
        "getReadMethod",
        "()Ljava/lang/reflect/Method;",
        descriptor_get_read,
    );
    shared.register_native(
        PROPERTY_DESCRIPTOR_CLASS,
        "getWriteMethod",
        "()Ljava/lang/reflect/Method;",
        descriptor_get_write,
    );
    shared.register_native(
        PROPERTY_DESCRIPTOR_CLASS,
        "setReadMethod",
        "(Ljava/lang/reflect/Method;)V",
        descriptor_set_read,
    );
    shared.register_native(
        PROPERTY_DESCRIPTOR_CLASS,
        "setWriteMethod",
        "(Ljava/lang/reflect/Method;)V",
        descriptor_set_write,
    );
    shared.register_native(
        PROPERTY_DESCRIPTOR_CLASS,
        "getPropertyType",
        "()Ljava/lang/Class;",
        descriptor_property_type,
    );
    shared.register_native(
        PROPERTY_DESCRIPTOR_CLASS,
        "getPropertyEditorClass",
        "()Ljava/lang/Class;",
        descriptor_get_editor_class,
    );
    shared.register_native(
        PROPERTY_DESCRIPTOR_CLASS,
        "isBound",
        "()Z",
        descriptor_is_bound,
    );
    shared.register_native(
        PROPERTY_DESCRIPTOR_CLASS,
        "setBound",
        "(Z)V",
        descriptor_set_bound,
    );
    shared.register_native(
        PROPERTY_DESCRIPTOR_CLASS,
        "isConstrained",
        "()Z",
        descriptor_is_constrained,
    );
    shared.register_native(
        PROPERTY_DESCRIPTOR_CLASS,
        "setConstrained",
        "(Z)V",
        descriptor_set_constrained,
    );
}

fn descriptor_state(
    shared: &Shared,
    receiver: ObjRef,
    method: &str,
) -> Result<DescriptorData, VmError> {
    shared
        .bean_state()
        .descriptors
        .get(&receiver)
        .cloned()
        .ok_or_else(|| VmError::Native(format!("{method}: descriptor has no state")))
}

fn update_descriptor(
    shared: &Shared,
    receiver: ObjRef,
    method: &str,
    update: impl FnOnce(&mut DescriptorData),
) -> Result<(), VmError> {
    let mut state = shared.bean_state();
    let descriptor = state
        .descriptors
        .get_mut(&receiver)
        .ok_or_else(|| VmError::Native(format!("{method}: descriptor has no state")))?;
    update(descriptor);
    Ok(())
}

fn optional_reference(reference: ObjRef) -> Value {
    if reference.is_null() {
        Value::Ref(ObjRef::NULL)
    } else {
        Value::Ref(reference)
    }
}

fn property_descriptor_init(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let name = match args.first().copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => {
            let heap = shared.heap();
            heap.string(reference)
                .map_err(|error| VmError::Native(error.to_string()))?
                .to_owned()
        }
        _ => {
            return Err(VmError::Native(
                "PropertyDescriptor.<init> expects a name".to_owned(),
            ));
        }
    };
    let read = match args.get(1).copied() {
        Some(Value::Ref(reference)) => reference,
        _ => ObjRef::NULL,
    };
    let write = match args.get(2).copied() {
        Some(Value::Ref(reference)) => reference,
        _ => ObjRef::NULL,
    };
    shared.bean_state().descriptors.insert(
        receiver,
        DescriptorData {
            name,
            read,
            write,
            display_name: None,
            expert: false,
            hidden: false,
            bound: false,
            constrained: false,
        },
    );
    Ok(None)
}

fn descriptor_name(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let name = descriptor_state(shared, receiver, "PropertyDescriptor.getName")?.name;
    Ok(Some(Value::Ref(shared.heap().allocate_string(name))))
}

fn descriptor_display_name(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let descriptor = descriptor_state(shared, receiver, "PropertyDescriptor.getDisplayName")?;
    let name = descriptor.display_name.unwrap_or(descriptor.name);
    Ok(Some(Value::Ref(shared.heap().allocate_string(name))))
}

fn descriptor_set_display_name(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let name = match args.first().copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => {
            let heap = shared.heap();
            heap.string(reference)
                .map_err(|error| VmError::Native(error.to_string()))?
                .to_owned()
        }
        _ => "null".to_owned(),
    };
    update_descriptor(
        shared,
        receiver,
        "PropertyDescriptor.setDisplayName",
        |descriptor| {
            descriptor.display_name = Some(name);
        },
    )?;
    Ok(None)
}

fn descriptor_is_expert(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let expert = descriptor_state(shared, receiver, "PropertyDescriptor.isExpert")?.expert;
    Ok(Some(Value::Int(i32::from(expert))))
}

fn descriptor_set_expert(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let expert = matches!(args.first().copied(), Some(Value::Int(value)) if value != 0);
    update_descriptor(
        shared,
        receiver,
        "PropertyDescriptor.setExpert",
        |descriptor| {
            descriptor.expert = expert;
        },
    )?;
    Ok(None)
}

fn descriptor_is_hidden(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let hidden = descriptor_state(shared, receiver, "PropertyDescriptor.isHidden")?.hidden;
    Ok(Some(Value::Int(i32::from(hidden))))
}

fn descriptor_set_hidden(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let hidden = matches!(args.first().copied(), Some(Value::Int(value)) if value != 0);
    update_descriptor(
        shared,
        receiver,
        "PropertyDescriptor.setHidden",
        |descriptor| {
            descriptor.hidden = hidden;
        },
    )?;
    Ok(None)
}

fn descriptor_get_read(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let read = descriptor_state(shared, receiver, "PropertyDescriptor.getReadMethod")?.read;
    Ok(Some(optional_reference(read)))
}

fn descriptor_get_write(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let write = descriptor_state(shared, receiver, "PropertyDescriptor.getWriteMethod")?.write;
    Ok(Some(optional_reference(write)))
}

fn descriptor_set_read(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let read = match args.first().copied() {
        Some(Value::Ref(reference)) => reference,
        _ => ObjRef::NULL,
    };
    update_descriptor(
        shared,
        receiver,
        "PropertyDescriptor.setReadMethod",
        |descriptor| {
            descriptor.read = read;
        },
    )?;
    Ok(None)
}

fn descriptor_set_write(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let write = match args.first().copied() {
        Some(Value::Ref(reference)) => reference,
        _ => ObjRef::NULL,
    };
    update_descriptor(
        shared,
        receiver,
        "PropertyDescriptor.setWriteMethod",
        |descriptor| {
            descriptor.write = write;
        },
    )?;
    Ok(None)
}

fn descriptor_property_type(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let descriptor = descriptor_state(shared, receiver, "PropertyDescriptor.getPropertyType")?;
    if !descriptor.read.is_null() {
        return Ok(Some(Value::Ref(crate::reflection::accessor_property_type(
            shared,
            descriptor.read,
            false,
        )?)));
    }
    if !descriptor.write.is_null() {
        return Ok(Some(Value::Ref(crate::reflection::accessor_property_type(
            shared,
            descriptor.write,
            true,
        )?)));
    }
    let object = shared.load_library_class("java/lang/Object")?;
    Ok(Some(Value::Ref(shared.class_object(object)?)))
}

fn descriptor_to_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let name = descriptor_state(shared, receiver, "PropertyDescriptor.toString")?.name;
    Ok(Some(Value::Ref(
        shared
            .heap()
            .allocate_string(format!("PropertyDescriptor[{name}]")),
    )))
}

fn descriptor_equals(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let equals = match args.first().copied() {
        Some(Value::Ref(other)) if !other.is_null() => {
            let left = descriptor_state(shared, receiver, "PropertyDescriptor.equals")?;
            let right = shared.bean_state().descriptors.get(&other).cloned();
            match right {
                Some(right) => {
                    left.name == right.name && left.read == right.read && left.write == right.write
                }
                None => false,
            }
        }
        _ => false,
    };
    Ok(Some(Value::Int(i32::from(equals))))
}

fn descriptor_hash_code(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let name = descriptor_state(shared, receiver, "PropertyDescriptor.hashCode")?.name;
    let mut hash = 0i32;
    for unit in name.encode_utf16() {
        hash = hash.wrapping_mul(31).wrapping_add(i32::from(unit));
    }
    Ok(Some(Value::Int(hash)))
}

fn introspector_decapitalize(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let Some(Value::Ref(reference)) = args.first().copied() else {
        return Err(VmError::Native(
            "Introspector.decapitalize expects a String".to_owned(),
        ));
    };
    if reference.is_null() {
        return Err(VmError::Native(
            "Introspector.decapitalize: null String".to_owned(),
        ));
    }
    let name = {
        let heap = shared.heap();
        heap.string(reference)
            .map_err(|error| VmError::Native(error.to_string()))?
            .to_owned()
    };
    let mut characters = name.chars();
    let Some(first) = characters.next() else {
        return Ok(Some(Value::Ref(
            shared.heap().allocate_string(String::new()),
        )));
    };
    let second_is_upper = characters.next().is_some_and(char::is_uppercase);
    let text = if first.is_uppercase() && second_is_upper {
        name
    } else {
        let lower: String = first.to_lowercase().collect();
        format!("{lower}{}", characters.as_str())
    };
    Ok(Some(Value::Ref(shared.heap().allocate_string(text))))
}

fn noop(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(None)
}

fn descriptor_get_editor_class(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    // No `PropertyEditor` support yet; the JDK returns null when none is registered.
    Ok(Some(Value::Ref(ObjRef::NULL)))
}

fn descriptor_is_bound(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let bound = descriptor_state(shared, receiver, "PropertyDescriptor.isBound")?.bound;
    Ok(Some(Value::Int(i32::from(bound))))
}

fn descriptor_set_bound(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let bound = matches!(args.first().copied(), Some(Value::Int(value)) if value != 0);
    update_descriptor(
        shared,
        receiver,
        "PropertyDescriptor.setBound",
        |descriptor| {
            descriptor.bound = bound;
        },
    )?;
    Ok(None)
}

fn descriptor_is_constrained(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let constrained =
        descriptor_state(shared, receiver, "PropertyDescriptor.isConstrained")?.constrained;
    Ok(Some(Value::Int(i32::from(constrained))))
}

fn descriptor_set_constrained(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let constrained = matches!(args.first().copied(), Some(Value::Int(value)) if value != 0);
    update_descriptor(
        shared,
        receiver,
        "PropertyDescriptor.setConstrained",
        |descriptor| {
            descriptor.constrained = constrained;
        },
    )?;
    Ok(None)
}
