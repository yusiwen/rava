//! Minimal `java.io.File` natives (M4.5).
//!
//! Spring Boot's configuration-data path names file locations and asks whether they exist
//! and what their canonical path is. The C1 stub declares no fields, so each `File`'s path
//! lives in [`FileState`], a side table keyed by [`ObjRef`]; the objects stay ordinary
//! `java.io.File` instances.
//!
//! Paths are host `PathBuf`s. The operations are the ones the startup path uses:
//! construction from a parent and a name, `getPath`/`getAbsolutePath`/`getCanonicalPath`,
//! existence and kind queries, `getName`/`getParent`, `toURI` and `toString`. A path that
//! does not exist keeps its lexical form for `getCanonicalPath` (the parent, if it exists,
//! is canonicalized and the file name appended), which is the JDK's best effort short of
//! resolving every link.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::VmError;
use crate::heap::ObjRef;
use crate::loader::LoaderId;
use crate::shared::Shared;
use crate::thread::ThreadState;
use crate::value::Value;

const FILE_CLASS: &str = "java/io/File";

/// The path of every live `java.io.File`.
#[derive(Debug, Default)]
pub struct FileState {
    files: HashMap<ObjRef, PathBuf>,
}

impl FileState {
    /// Every object whose path the table holds, for root enumeration.
    pub fn references(&self) -> Vec<ObjRef> {
        self.files.keys().copied().collect()
    }
}

pub fn register_natives(shared: &Shared) {
    shared.register_native(FILE_CLASS, "<init>", "(Ljava/lang/String;)V", file_init);
    shared.register_native(
        FILE_CLASS,
        "<init>",
        "(Ljava/lang/String;Ljava/lang/String;)V",
        file_init_parent_name,
    );
    shared.register_native(
        FILE_CLASS,
        "<init>",
        "(Ljava/io/File;Ljava/lang/String;)V",
        file_init_file_name,
    );
    shared.register_native(FILE_CLASS, "getPath", "()Ljava/lang/String;", file_get_path);
    shared.register_native(
        FILE_CLASS,
        "getAbsolutePath",
        "()Ljava/lang/String;",
        file_get_absolute_path,
    );
    shared.register_native(
        FILE_CLASS,
        "getCanonicalPath",
        "()Ljava/lang/String;",
        file_get_canonical_path,
    );
    shared.register_native(
        FILE_CLASS,
        "getAbsoluteFile",
        "()Ljava/io/File;",
        file_get_absolute_file,
    );
    shared.register_native(
        FILE_CLASS,
        "getCanonicalFile",
        "()Ljava/io/File;",
        file_get_canonical_file,
    );
    shared.register_native(
        FILE_CLASS,
        "getParent",
        "()Ljava/lang/String;",
        file_get_parent,
    );
    shared.register_native(
        FILE_CLASS,
        "getParentFile",
        "()Ljava/io/File;",
        file_get_parent_file,
    );
    shared.register_native(FILE_CLASS, "getName", "()Ljava/lang/String;", file_get_name);
    shared.register_native(FILE_CLASS, "exists", "()Z", file_exists);
    shared.register_native(FILE_CLASS, "isFile", "()Z", file_is_file);
    shared.register_native(FILE_CLASS, "isDirectory", "()Z", file_is_directory);
    shared.register_native(FILE_CLASS, "isAbsolute", "()Z", file_is_absolute);
    shared.register_native(FILE_CLASS, "length", "()J", file_length);
    shared.register_native(FILE_CLASS, "lastModified", "()J", file_last_modified);
    shared.register_native(FILE_CLASS, "canRead", "()Z", file_can_read);
    shared.register_native(FILE_CLASS, "canWrite", "()Z", file_can_write);
    shared.register_native(FILE_CLASS, "mkdir", "()Z", file_mkdir);
    shared.register_native(FILE_CLASS, "mkdirs", "()Z", file_mkdirs);
    shared.register_native(FILE_CLASS, "delete", "()Z", file_delete);
    shared.register_native(
        FILE_CLASS,
        "toString",
        "()Ljava/lang/String;",
        file_get_path,
    );
    shared.register_native(FILE_CLASS, "toURI", "()Ljava/net/URI;", file_to_uri);
    shared.register_native(FILE_CLASS, "toURL", "()Ljava/net/URL;", file_to_url);
    shared.register_native(FILE_CLASS, "equals", "(Ljava/lang/Object;)Z", file_equals);
    shared.register_native(FILE_CLASS, "hashCode", "()I", file_hash_code);
    shared.register_native(
        FILE_CLASS,
        "compareTo",
        "(Ljava/io/File;)I",
        file_compare_to,
    );
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

fn new_string(shared: &Shared, text: impl Into<String>) -> Value {
    Value::Ref(shared.heap().allocate_string(text.into()))
}

fn file_path(shared: &Shared, receiver: ObjRef, method: &str) -> Result<PathBuf, VmError> {
    shared
        .file_state()
        .files
        .get(&receiver)
        .cloned()
        .ok_or_else(|| VmError::Native(format!("{method}: File has no path")))
}

fn allocate_file(shared: &Shared, path: PathBuf) -> Result<ObjRef, VmError> {
    let class = shared.load_library_class(FILE_CLASS)?;
    let object = shared.allocate_instance(class)?;
    shared.file_state().files.insert(object, path);
    Ok(object)
}

fn file_init(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let path = string_argument(shared, args, 0, "File.<init>")?;
    shared
        .file_state()
        .files
        .insert(receiver, PathBuf::from(path));
    Ok(None)
}

fn file_init_parent_name(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let parent = string_argument(shared, args, 0, "File.<init>")?;
    let name = string_argument(shared, args, 1, "File.<init>")?;
    shared
        .file_state()
        .files
        .insert(receiver, Path::new(&parent).join(name));
    Ok(None)
}

fn file_init_file_name(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let parent = match args.first().copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => {
            file_path(shared, reference, "File.<init>")?
        }
        _ => PathBuf::new(),
    };
    let name = string_argument(shared, args, 1, "File.<init>")?;
    let path = if parent.as_os_str().is_empty() {
        PathBuf::from(name)
    } else {
        parent.join(name)
    };
    shared.file_state().files.insert(receiver, path);
    Ok(None)
}

fn file_get_path(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let path = file_path(shared, receiver, "File.getPath")?;
    Ok(Some(new_string(shared, path.to_string_lossy())))
}

fn absolute(path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    cwd.join(path)
}

/// The canonical form, or the best lexical form when the path does not exist.
fn canonical(path: &Path) -> PathBuf {
    if let Ok(canonical) = std::fs::canonicalize(path) {
        return canonical;
    }
    let absolute = absolute(path);
    let parent = absolute.parent().map(Path::to_path_buf).unwrap_or_default();
    let name = absolute
        .file_name()
        .map(|name| name.to_owned())
        .unwrap_or_default();
    match std::fs::canonicalize(&parent) {
        Ok(parent) if !name.is_empty() => parent.join(name),
        _ => normalize(&absolute),
    }
}

/// Lexically remove `.` and resolve `..` without touching the filesystem.
fn normalize(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                result.pop();
            }
            other => result.push(other),
        }
    }
    result
}

fn file_get_absolute_path(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let path = absolute(&file_path(shared, receiver, "File.getAbsolutePath")?);
    Ok(Some(new_string(shared, path.to_string_lossy())))
}

fn file_get_canonical_path(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let path = canonical(&file_path(shared, receiver, "File.getCanonicalPath")?);
    Ok(Some(new_string(shared, path.to_string_lossy())))
}

fn file_get_absolute_file(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let path = absolute(&file_path(shared, receiver, "File.getAbsoluteFile")?);
    Ok(Some(Value::Ref(allocate_file(shared, path)?)))
}

fn file_get_canonical_file(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let path = canonical(&file_path(shared, receiver, "File.getCanonicalFile")?);
    Ok(Some(Value::Ref(allocate_file(shared, path)?)))
}

fn file_get_parent(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let path = file_path(shared, receiver, "File.getParent")?;
    Ok(Some(match path.parent() {
        Some(parent) => new_string(shared, parent.to_string_lossy()),
        None => Value::Ref(ObjRef::NULL),
    }))
}

fn file_get_parent_file(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let path = file_path(shared, receiver, "File.getParentFile")?;
    Ok(Some(match path.parent() {
        Some(parent) => Value::Ref(allocate_file(shared, parent.to_path_buf())?),
        None => Value::Ref(ObjRef::NULL),
    }))
}

fn file_get_name(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let path = file_path(shared, receiver, "File.getName")?;
    Ok(Some(match path.file_name() {
        Some(name) => new_string(shared, name.to_string_lossy()),
        None => new_string(shared, path.to_string_lossy()),
    }))
}

fn file_exists(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let path = file_path(shared, receiver, "File.exists")?;
    Ok(Some(Value::Int(i32::from(path.exists()))))
}

fn file_is_file(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let path = file_path(shared, receiver, "File.isFile")?;
    Ok(Some(Value::Int(i32::from(path.is_file()))))
}

fn file_is_directory(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let path = file_path(shared, receiver, "File.isDirectory")?;
    Ok(Some(Value::Int(i32::from(path.is_dir()))))
}

fn file_is_absolute(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let path = file_path(shared, receiver, "File.isAbsolute")?;
    Ok(Some(Value::Int(i32::from(path.is_absolute()))))
}

fn file_length(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let path = file_path(shared, receiver, "File.length")?;
    let length = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
    Ok(Some(Value::Long(length as i64)))
}

fn file_last_modified(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let path = file_path(shared, receiver, "File.lastModified")?;
    let millis = std::fs::metadata(&path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0);
    Ok(Some(Value::Long(millis)))
}

fn file_can_read(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let path = file_path(shared, receiver, "File.canRead")?;
    Ok(Some(Value::Int(i32::from(
        std::fs::File::open(&path).is_ok(),
    ))))
}

fn file_can_write(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let path = file_path(shared, receiver, "File.canWrite")?;
    Ok(Some(Value::Int(i32::from(
        std::fs::OpenOptions::new().write(true).open(&path).is_ok(),
    ))))
}

fn file_mkdir(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let path = file_path(shared, receiver, "File.mkdir")?;
    Ok(Some(Value::Int(i32::from(
        std::fs::create_dir(&path).is_ok(),
    ))))
}

fn file_mkdirs(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let path = file_path(shared, receiver, "File.mkdirs")?;
    Ok(Some(Value::Int(i32::from(
        std::fs::create_dir_all(&path).is_ok(),
    ))))
}

fn file_delete(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let path = file_path(shared, receiver, "File.delete")?;
    let deleted = if path.is_dir() {
        std::fs::remove_dir(&path).is_ok()
    } else {
        std::fs::remove_file(&path).is_ok()
    };
    Ok(Some(Value::Int(i32::from(deleted))))
}

fn file_to_uri(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let path = absolute(&file_path(shared, receiver, "File.toURI")?);
    let form = format!("file:{}", path.to_string_lossy());
    let class = shared.load_library_class("java/net/URI")?;
    let object = shared.allocate_instance(class)?;
    shared.uri_state().insert(object, form);
    Ok(Some(Value::Ref(object)))
}

fn file_to_url(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let path = absolute(&file_path(shared, receiver, "File.toURL")?);
    let form = format!("file:{}", path.to_string_lossy());
    Ok(Some(Value::Ref(crate::resources::url_object(
        shared, &form,
    )?)))
}

fn file_equals(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let equal = match args.first().copied() {
        Some(Value::Ref(other)) if !other.is_null() => {
            let left = file_path(shared, receiver, "File.equals")?;
            let right = shared.file_state().files.get(&other).cloned();
            right.as_ref() == Some(&left)
        }
        _ => false,
    };
    Ok(Some(Value::Int(i32::from(equal))))
}

fn file_hash_code(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let path = file_path(shared, receiver, "File.hashCode")?;
    let text = path.to_string_lossy();
    let mut hash = 0i32;
    for unit in text.encode_utf16() {
        hash = hash.wrapping_mul(31).wrapping_add(i32::from(unit));
    }
    Ok(Some(Value::Int(hash)))
}

fn file_compare_to(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let other = match args.first().copied() {
        Some(Value::Ref(other)) if !other.is_null() => other,
        _ => {
            return Err(VmError::Native(
                "File.compareTo expects a non-null File".to_owned(),
            ));
        }
    };
    let left = file_path(shared, receiver, "File.compareTo")?;
    let right = file_path(shared, other, "File.compareTo")?;
    Ok(Some(Value::Int(match left.cmp(&right) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    })))
}
