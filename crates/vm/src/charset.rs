//! Minimal `java.nio.charset.Charset` natives (M4.5).
//!
//! Logback's encoder asks for `Charset.defaultCharset()` (and the `StandardCharsets`
//! constants) when it converts a log line to bytes. The VM reports UTF-8 as the platform
//! charset, matching the `file.encoding` system property, and supports the UTF-8,
//! US-ASCII and ISO-8859-1 names. The C1 stub declares no fields, so a charset's name lives
//! in [`CharsetState`], a side table keyed by [`ObjRef`].

use std::collections::HashMap;

use crate::VmError;
use crate::heap::ObjRef;
use crate::loader::LoaderId;
use crate::shared::Shared;
use crate::thread::ThreadState;
use crate::value::Value;

const CHARSET_CLASS: &str = "java/nio/charset/Charset";
const STANDARD_CHARSETS_CLASS: &str = "java/nio/charset/StandardCharsets";

/// The canonical name of every live charset object.
#[derive(Debug, Default)]
pub struct CharsetState {
    charsets: HashMap<ObjRef, String>,
}

impl CharsetState {
    /// Every object whose charset name the table holds, for root enumeration.
    pub fn references(&self) -> Vec<ObjRef> {
        self.charsets.keys().copied().collect()
    }

    /// Record a charset object's canonical name.
    pub fn insert(&mut self, object: ObjRef, name: String) {
        self.charsets.insert(object, name);
    }

    /// The charset's canonical name.
    pub fn name(&self, object: ObjRef) -> Option<String> {
        self.charsets.get(&object).cloned()
    }
}

pub fn register_natives(shared: &Shared) {
    shared.register_native(
        CHARSET_CLASS,
        "defaultCharset",
        "()Ljava/nio/charset/Charset;",
        charset_default,
    );
    shared.register_native(
        CHARSET_CLASS,
        "forName",
        "(Ljava/lang/String;)Ljava/nio/charset/Charset;",
        charset_for_name,
    );
    shared.register_native(CHARSET_CLASS, "name", "()Ljava/lang/String;", charset_name);
    shared.register_native(
        CHARSET_CLASS,
        "displayName",
        "()Ljava/lang/String;",
        charset_name,
    );
    shared.register_native(
        CHARSET_CLASS,
        "toString",
        "()Ljava/lang/String;",
        charset_name,
    );
    shared.register_native(CHARSET_CLASS, "canEncode", "()Z", charset_can_encode);
    shared.register_native(
        CHARSET_CLASS,
        "equals",
        "(Ljava/lang/Object;)Z",
        charset_equals,
    );
    shared.register_native(CHARSET_CLASS, "hashCode", "()I", charset_hash_code);
}

fn heap_error(error: crate::HeapError) -> VmError {
    VmError::Native(error.to_string())
}

fn string_argument(
    shared: &Shared,
    args: &[Value],
    position: usize,
    method: &str,
) -> Result<String, VmError> {
    let Some(Value::Ref(reference)) = args.get(position).copied() else {
        return Err(VmError::Native(format!(
            "{method} expects a String argument"
        )));
    };
    if reference.is_null() {
        return Err(VmError::Native(format!("{method}: null String argument")));
    }
    let heap = shared.heap();
    Ok(heap.string(reference).map_err(heap_error)?.to_owned())
}

fn allocate_charset(shared: &Shared, canonical: &str) -> Result<ObjRef, VmError> {
    let class = shared.load_library_class(CHARSET_CLASS)?;
    let object = shared.allocate_instance(class)?;
    shared.charset_state().insert(object, canonical.to_owned());
    Ok(object)
}

/// The canonical name for a supported charset name, or `None`.
fn canonical_name(name: &str) -> Option<&'static str> {
    let normalized = name.trim().to_ascii_uppercase();
    match normalized.as_str() {
        "UTF-8" | "UTF8" => Some("UTF-8"),
        "US-ASCII" | "ASCII" => Some("US-ASCII"),
        "ISO-8859-1" | "ISO8859-1" | "LATIN1" => Some("ISO-8859-1"),
        _ => None,
    }
}

fn charset_default(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Ref(allocate_charset(shared, "UTF-8")?)))
}

fn charset_for_name(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let requested = string_argument(shared, args, 0, "Charset.forName")?;
    let Some(canonical) = canonical_name(&requested) else {
        return Err(VmError::Native(format!(
            "Charset.forName: unsupported charset {requested:?}"
        )));
    };
    Ok(Some(Value::Ref(allocate_charset(shared, canonical)?)))
}

fn charset_name(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let name = shared
        .charset_state()
        .name(receiver)
        .ok_or_else(|| VmError::Native("Charset has no name".to_owned()))?;
    Ok(Some(Value::Ref(shared.heap().allocate_string(name))))
}

fn charset_can_encode(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let present = shared.charset_state().name(receiver).is_some();
    Ok(Some(Value::Int(i32::from(present))))
}

fn charset_equals(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let equal = match args.first().copied() {
        Some(Value::Ref(other)) if !other.is_null() => {
            let left = shared.charset_state().name(receiver);
            let right = shared.charset_state().name(other);
            left.is_some() && left == right
        }
        _ => false,
    };
    Ok(Some(Value::Int(i32::from(equal))))
}

fn charset_hash_code(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let name = shared
        .charset_state()
        .name(receiver)
        .ok_or_else(|| VmError::Native("Charset has no name".to_owned()))?;
    let mut hash = 0i32;
    for unit in name.encode_utf16() {
        hash = hash.wrapping_mul(31).wrapping_add(i32::from(unit));
    }
    Ok(Some(Value::Int(hash)))
}

/// Install the `StandardCharsets` constants the logback encoder reads.
pub fn install_constants(shared: &Shared) -> Result<(), VmError> {
    let class = shared.load_library_class(STANDARD_CHARSETS_CLASS)?;
    let constants = [
        ("UTF_8", "UTF-8"),
        ("US_ASCII", "US-ASCII"),
        ("ISO_8859_1", "ISO-8859-1"),
    ];
    let mut values = Vec::with_capacity(constants.len());
    for (field, canonical) in constants {
        let object = allocate_charset(shared, canonical)?;
        values.push((
            field.to_owned(),
            "Ljava/nio/charset/Charset;".to_owned(),
            Value::Ref(object),
        ));
    }
    let entry = shared
        .entry(class)
        .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", class.0)))?;
    let mut statics = entry.statics.lock().expect("static field lock poisoned");
    for (name, descriptor, value) in values {
        statics.insert((name, descriptor), value);
    }
    Ok(())
}
