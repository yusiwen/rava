//! Minimal `java.util.jar.Manifest`/`Attributes` and `java.lang.Package` natives (#73).
//!
//! Rendering Boot's startup `LogMessage` calls `ApplicationHome`, which opens each
//! `META-INF/MANIFEST.MF` URL and parses it with `new Manifest(InputStream)`, then asks its
//! main attributes for `Start-Class`. The class library has no `Code` for those classes, so
//! the constructor and the two lookups live here. Only the main section is parsed
//! (continuation lines included); per-entry sections are ignored, which is all this VM
//! needs. `Package.getImplementationVersion` answers null rather than inventing a version.
//!
//! The parsed attributes are side-table state keyed by the `Manifest`/`Attributes` object;
//! no Java field holds them, matching the other side tables in this crate.

use std::collections::BTreeMap;

use crate::VmError;
use crate::heap::ObjRef;
use crate::loader::LoaderId;
use crate::shared::Shared;
use crate::thread::ThreadState;
use crate::value::Value;

/// Install the manifest/attributes/package natives.
pub fn register_natives(shared: &Shared) {
    shared.register_native(
        "java/util/jar/Manifest",
        "<init>",
        "(Ljava/io/InputStream;)V",
        manifest_init,
    );
    shared.register_native(
        "java/util/jar/Manifest",
        "getMainAttributes",
        "()Ljava/util/jar/Attributes;",
        manifest_get_main_attributes,
    );
    shared.register_native("java/util/jar/Attributes", "<init>", "()V", attributes_init);
    shared.register_native(
        "java/util/jar/Attributes",
        "getValue",
        "(Ljava/lang/String;)Ljava/lang/String;",
        attributes_get_value,
    );
    for (name, function) in [
        ("getName", package_get_name as crate::NativeFn),
        ("getImplementationVersion", package_null_string),
        ("getSpecificationVersion", package_null_string),
        ("getImplementationTitle", package_null_string),
        ("getImplementationVendor", package_null_string),
        ("getSpecificationTitle", package_null_string),
        ("getSpecificationVendor", package_null_string),
    ] {
        shared.register_native("java/lang/Package", name, "()Ljava/lang/String;", function);
    }
}

fn reference_argument(args: &[Value], position: usize, method: &str) -> Result<ObjRef, VmError> {
    match args.get(position).copied() {
        Some(Value::Ref(reference)) => Ok(reference),
        Some(_) => Err(VmError::Native(format!(
            "{method} expects a reference argument"
        ))),
        None => Err(VmError::Native(format!("{method} is missing its argument"))),
    }
}

/// `Manifest(InputStream)`: read the stream and keep its main-section attributes.
fn manifest_init(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let stream = reference_argument(args, 0, "Manifest.<init>")?;
    if stream.is_null() {
        return Err(VmError::Native(
            "Manifest.<init> received a null InputStream".to_owned(),
        ));
    }
    let read_all = shared
        .native("java/io/InputStream", "readAllBytes", "()[B")
        .ok_or_else(|| VmError::Native("InputStream.readAllBytes is not registered".to_owned()))?;
    let Some(Value::Ref(bytes)) = read_all(shared, thread, LoaderId::BOOTSTRAP, stream, &[])?
    else {
        return Err(VmError::Native(
            "Manifest.<init>: readAllBytes returned no byte[]".to_owned(),
        ));
    };
    let bytes = {
        let heap = shared.heap();
        heap.array(bytes)
            .map_err(|error| VmError::Native(format!("Manifest.<init>: {error}")))?
            .elements
            .iter()
            .map(|value| match value {
                Value::Int(byte) => *byte as u8,
                _ => 0,
            })
            .collect::<Vec<u8>>()
    };
    shared.set_manifest_attributes(receiver, parse_main_attributes(&bytes));
    Ok(None)
}

/// Parse a manifest's main section: `Name: value`, with continuation lines starting with a
/// space. Parsing stops at the first blank line (the per-entry sections are unused here).
fn parse_main_attributes(bytes: &[u8]) -> BTreeMap<String, String> {
    let text = String::from_utf8_lossy(bytes);
    let mut attributes: BTreeMap<String, String> = BTreeMap::new();
    let mut current: Option<String> = None;
    for raw in text.split_inclusive('\n') {
        let line = raw.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        if let Some(continuation) = line.strip_prefix(' ') {
            if let Some(key) = &current {
                if let Some(value) = attributes.get_mut(key) {
                    value.push_str(continuation);
                }
            }
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            let value = value.strip_prefix(' ').unwrap_or(value);
            attributes.insert(key.to_owned(), value.to_owned());
            current = Some(key.to_owned());
        }
    }
    attributes
}

/// `Manifest.getMainAttributes()`: an `Attributes` object over the parsed main section.
fn manifest_get_main_attributes(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let attributes = shared.manifest_attributes(receiver).unwrap_or_default();
    let class = shared.load_library_class("java/util/jar/Attributes")?;
    let object = shared.allocate_instance(class)?;
    shared.set_manifest_attributes(object, attributes);
    Ok(Some(Value::Ref(object)))
}

/// `Attributes.<init>()`: an empty attribute map.
fn attributes_init(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    shared.set_manifest_attributes(receiver, BTreeMap::new());
    Ok(None)
}

/// `Attributes.getValue(String)`: the value, or null. Manifest names are case-insensitive.
fn attributes_get_value(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let Some(Value::Ref(name)) = args.first().copied() else {
        return Err(VmError::Native(
            "Attributes.getValue expects a String".to_owned(),
        ));
    };
    if name.is_null() {
        return Ok(Some(Value::Ref(ObjRef::NULL)));
    }
    let name = {
        let heap = shared.heap();
        heap.string(name)
            .map_err(|error| VmError::Native(format!("Attributes.getValue: {error}")))?
            .to_owned()
    };
    let attributes = shared.manifest_attributes(receiver).unwrap_or_default();
    let found = attributes.get(&name).cloned().or_else(|| {
        attributes
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(&name))
            .map(|(_, value)| value.clone())
    });
    Ok(Some(match found {
        Some(value) => Value::Ref(shared.intern_string(&value)),
        None => Value::Ref(ObjRef::NULL),
    }))
}

/// `Package.getName()`: the dotted package name recorded by `Class.getPackage`.
fn package_get_name(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    match shared.package_name(receiver) {
        Some(name) => Ok(Some(Value::Ref(shared.intern_string(&name)))),
        None => Ok(Some(Value::Ref(ObjRef::NULL))),
    }
}

fn package_null_string(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Ref(ObjRef::NULL)))
}
