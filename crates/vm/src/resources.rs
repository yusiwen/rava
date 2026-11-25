//! Minimal `java.net.URL`, `java.net.URLConnection` and `java.io.InputStream` (M4.1).
//!
//! The class-library stubs declare the JDK 17 signatures but carry no `Code`, so every
//! method a resource program actually calls is a native here. The URL's components, a
//! connection's target and an input stream's bytes are Rust text and bytes, not Java
//! references, so they live in a side table keyed by [`ObjRef`] instead of in a new
//! [`crate::HeapObject`] variant. The side table is owned by [`crate::Shared`] and its
//! keys are reported as roots, so a URL or stream survives exactly as long as the VM
//! keeps its attachment.
//!
//! Two limits are deliberate for this slice: there is no network transport, so only
//! `file:` and `jar:file:...!/entry` URLs open, and no header or content-type surface,
//! because there is nothing that would produce one.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::VmError;
use crate::heap::{ArrayKind, ClassRef, ObjRef, PrimitiveKind};
use crate::loader::LoaderId;
use crate::shared::Shared;
use crate::thread::ThreadState;
use crate::value::Value;

const URL_CLASS: &str = "java/net/URL";
const URL_CONNECTION_CLASS: &str = "java/net/URLConnection";
const INPUT_STREAM_CLASS: &str = "java/io/InputStream";

/// The Rust state attached to the Java resource objects this VM allocates.
#[derive(Debug, Default)]
pub struct ResourceState {
    urls: HashMap<ObjRef, UrlData>,
    streams: HashMap<ObjRef, StreamData>,
    byte_output_streams: HashMap<ObjRef, Vec<u8>>,
    connections: HashMap<ObjRef, ConnectionData>,
}

impl ResourceState {
    /// Every object the side table keeps data for, so [`crate::Shared::roots`] can report
    /// them. A future collector can then treat the attachment as a strong owner.
    pub fn references(&self) -> Vec<ObjRef> {
        self.urls
            .keys()
            .chain(self.streams.keys())
            .chain(self.byte_output_streams.keys())
            .chain(self.connections.keys())
            .copied()
            .collect()
    }
}

/// A parsed `java.net.URL`.
#[derive(Debug, Clone)]
pub struct UrlData {
    external_form: String,
    protocol: String,
    host: String,
    port: i32,
    path: String,
    query: Option<String>,
    fragment: Option<String>,
    user_info: Option<String>,
}

impl UrlData {
    fn parse(form: &str) -> Result<Self, String> {
        let (scheme, rest) =
            split_scheme(form).ok_or_else(|| format!("missing protocol in URL: {form}"))?;
        let protocol = scheme.to_ascii_lowercase();
        let (rest, fragment) = split_once(rest, '#');
        let (rest, query) = split_once(rest, '?');
        let (host, port, user_info, path) = parse_authority(rest)?;
        Ok(Self {
            external_form: form.to_owned(),
            protocol,
            host,
            port,
            path,
            query,
            fragment,
            user_info,
        })
    }

    fn resolve(&self, spec: &str) -> Result<Self, String> {
        if spec.is_empty() {
            return Ok(self.clone());
        }
        if split_scheme(spec).is_some() {
            return Self::parse(spec);
        }
        let (spec, fragment) = split_once(spec, '#');
        let (path_part, query) = split_once(spec, '?');
        if path_part.is_empty() {
            let mut resolved = self.clone();
            if query.is_some() {
                resolved.query = query;
            }
            resolved.fragment = fragment;
            resolved.external_form = resolved.compose();
            return Ok(resolved);
        }
        if path_part.starts_with("//") {
            return Self::parse(&format!("{}:{spec}", self.protocol));
        }
        let path = if path_part.starts_with('/') {
            normalize_path(path_part)
        } else {
            let base = match self.path.rfind('/') {
                Some(index) => &self.path[..=index],
                None => "",
            };
            normalize_path(&format!("{base}{path_part}"))
        };
        let mut resolved = self.clone();
        resolved.path = path;
        resolved.query = query;
        resolved.fragment = fragment;
        resolved.external_form = resolved.compose();
        Ok(resolved)
    }

    fn compose(&self) -> String {
        let mut form = format!("{}:", self.protocol);
        if self.protocol == "jar" || self.host.is_empty() {
            form.push_str(&self.path);
        } else {
            form.push_str("//");
            if let Some(user_info) = &self.user_info {
                form.push_str(user_info);
                form.push('@');
            }
            form.push_str(&self.host);
            if self.port >= 0 {
                form.push(':');
                form.push_str(&self.port.to_string());
            }
            form.push_str(&self.path);
        }
        if let Some(query) = &self.query {
            form.push('?');
            form.push_str(query);
        }
        if let Some(fragment) = &self.fragment {
            form.push('#');
            form.push_str(fragment);
        }
        form
    }

    fn file(&self) -> String {
        match &self.query {
            Some(query) => format!("{}?{query}", self.path),
            None => self.path.clone(),
        }
    }
}

/// One `java.io.InputStream` over a byte snapshot.
#[derive(Debug, Clone)]
struct StreamData {
    bytes: Vec<u8>,
    position: usize,
}

/// One `java.net.URLConnection` and the URL it opens.
#[derive(Debug, Clone)]
struct ConnectionData {
    url: ObjRef,
    use_caches: bool,
}

/// Install the `java.net` and `java.io.InputStream` natives.
pub fn register_natives(shared: &Shared) {
    for (name, descriptor, function) in [
        (
            "<init>",
            "(Ljava/lang/String;)V",
            url_init_string as crate::NativeFn,
        ),
        (
            "<init>",
            "(Ljava/net/URL;Ljava/lang/String;)V",
            url_init_context,
        ),
        ("getProtocol", "()Ljava/lang/String;", url_get_protocol),
        ("getPath", "()Ljava/lang/String;", url_get_path),
        ("getFile", "()Ljava/lang/String;", url_get_file),
        ("getHost", "()Ljava/lang/String;", url_get_host),
        ("getPort", "()I", url_get_port),
        ("getUserInfo", "()Ljava/lang/String;", url_get_user_info),
        (
            "toExternalForm",
            "()Ljava/lang/String;",
            url_to_external_form,
        ),
        ("toString", "()Ljava/lang/String;", url_to_external_form),
        ("equals", "(Ljava/lang/Object;)Z", url_equals),
        ("hashCode", "()I", url_hash_code),
        ("openStream", "()Ljava/io/InputStream;", url_open_stream),
        (
            "openConnection",
            "()Ljava/net/URLConnection;",
            url_open_connection,
        ),
    ] {
        shared.register_native(URL_CLASS, name, descriptor, function);
    }

    // `UrlResource.getFilename()` percent-decodes the path with `StandardCharsets.UTF_8`;
    // the VM has no charset table yet, so the charset argument is accepted and the escape
    // is decoded as UTF-8, which is the runtime's one string encoding.
    for (name, descriptor, function) in [
        (
            "decode",
            "(Ljava/lang/String;)Ljava/lang/String;",
            url_decoder_decode as crate::NativeFn,
        ),
        (
            "decode",
            "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
            url_decoder_decode,
        ),
        (
            "decode",
            "(Ljava/lang/String;Ljava/nio/charset/Charset;)Ljava/lang/String;",
            url_decoder_decode,
        ),
    ] {
        shared.register_native("java/net/URLDecoder", name, descriptor, function);
    }

    for (name, descriptor, function) in [
        (
            "<init>",
            "(Ljava/net/URL;)V",
            connection_init as crate::NativeFn,
        ),
        (
            "getInputStream",
            "()Ljava/io/InputStream;",
            connection_get_input_stream,
        ),
        ("getContentLength", "()I", connection_get_content_length),
        (
            "getContentLengthLong",
            "()J",
            connection_get_content_length_long,
        ),
        ("getLastModified", "()J", connection_get_last_modified),
        ("getURL", "()Ljava/net/URL;", connection_get_url),
        ("connect", "()V", connection_connect),
        ("setUseCaches", "(Z)V", connection_set_use_caches),
        ("getUseCaches", "()Z", connection_get_use_caches),
    ] {
        shared.register_native(URL_CONNECTION_CLASS, name, descriptor, function);
    }

    for (name, descriptor, function) in [
        ("read", "()I", input_read as crate::NativeFn),
        ("read", "([B)I", input_read_array),
        ("read", "([BII)I", input_read_array_range),
        ("close", "()V", input_close),
        ("available", "()I", input_available),
        ("skip", "(J)J", input_skip),
        ("readAllBytes", "()[B", input_read_all_bytes),
        ("readNBytes", "(I)[B", input_read_n_bytes),
        ("markSupported", "()Z", input_mark_supported),
    ] {
        shared.register_native(INPUT_STREAM_CLASS, name, descriptor, function);
    }

    // Spring's `StreamUtils.copyToByteArray` wraps a resource stream in a
    // `ByteArrayOutputStream`; without this small writer the now-real resource streams
    // would take the M2 Spring fixtures into an unimplemented C1 stub. It is not a new
    // resource model: the buffer is Rust bytes in the same side table.
    for (name, descriptor, function) in [
        ("<init>", "()V", byte_output_init as crate::NativeFn),
        ("<init>", "(I)V", byte_output_init_capacity),
        ("write", "(I)V", byte_output_write),
        ("write", "([BII)V", byte_output_write_range),
        ("writeBytes", "([B)V", byte_output_write_bytes),
        ("reset", "()V", byte_output_reset),
        ("toByteArray", "()[B", byte_output_to_byte_array),
        ("size", "()I", byte_output_size),
        ("toString", "()Ljava/lang/String;", byte_output_to_string),
        (
            "toString",
            "(Ljava/lang/String;)Ljava/lang/String;",
            byte_output_to_string,
        ),
        ("close", "()V", byte_output_close),
        ("writeTo", "(Ljava/io/OutputStream;)V", byte_output_write_to),
    ] {
        shared.register_native("java/io/ByteArrayOutputStream", name, descriptor, function);
    }
    // `ByteArrayOutputStream` inherits `flush` from `OutputStream`.
    shared.register_native("java/io/OutputStream", "flush", "()V", output_stream_flush);
}

fn heap_error(error: crate::HeapError) -> VmError {
    VmError::Native(error.to_string())
}

// ---------------------------------------------------------------------------
// Helpers shared with `class_loader`, `reflection` and `library`.
// ---------------------------------------------------------------------------

/// Allocate a `java.net.URL` carrying `external`, parsing it first.
pub(crate) fn url_object(shared: &Shared, external: &str) -> Result<ObjRef, VmError> {
    let data = UrlData::parse(external).map_err(VmError::Native)?;
    let class = shared.load_library_class(URL_CLASS)?;
    let reference = shared.allocate_instance(class)?;
    shared.resource_state().urls.insert(reference, data);
    Ok(reference)
}

/// Allocate a `java.io.InputStream` over `bytes`.
pub(crate) fn stream_object(shared: &Shared, bytes: Vec<u8>) -> Result<ObjRef, VmError> {
    let class = shared.load_library_class(INPUT_STREAM_CLASS)?;
    let reference = shared.allocate_instance(class)?;
    shared
        .resource_state()
        .streams
        .insert(reference, StreamData { bytes, position: 0 });
    Ok(reference)
}

fn url_data(shared: &Shared, reference: ObjRef) -> Result<UrlData, VmError> {
    shared
        .resource_state()
        .urls
        .get(&reference)
        .cloned()
        .ok_or_else(|| VmError::Native(format!("{reference} is not a java.net.URL")))
}

fn connection_data(shared: &Shared, reference: ObjRef) -> Result<ConnectionData, VmError> {
    shared
        .resource_state()
        .connections
        .get(&reference)
        .cloned()
        .ok_or_else(|| VmError::Native(format!("{reference} is not a java.net.URLConnection")))
}

fn string_argument(
    shared: &Shared,
    args: &[Value],
    index: usize,
    method: &str,
) -> Result<String, VmError> {
    match args.get(index).copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => {
            let heap = shared.heap();
            Ok(heap.string(reference).map_err(heap_error)?.to_owned())
        }
        _ => Err(VmError::Native(format!(
            "{method} expects a non-null String argument"
        ))),
    }
}

fn int_argument(args: &[Value], index: usize, method: &str) -> Result<i32, VmError> {
    match args.get(index).copied() {
        Some(Value::Int(value)) => Ok(value),
        _ => Err(VmError::Native(format!(
            "{method} expects an int argument at position {index}"
        ))),
    }
}

fn long_argument(args: &[Value], index: usize, method: &str) -> Result<i64, VmError> {
    match args.get(index).copied() {
        Some(Value::Long(value)) => Ok(value),
        _ => Err(VmError::Native(format!(
            "{method} expects a long argument at position {index}"
        ))),
    }
}

fn array_argument(args: &[Value], index: usize, method: &str) -> Result<ObjRef, VmError> {
    match args.get(index).copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => Ok(reference),
        _ => Err(VmError::Native(format!(
            "{method} expects a non-null byte[] argument"
        ))),
    }
}

fn string_value(shared: &Shared, text: String) -> Value {
    Value::Ref(shared.intern_string(&text))
}

// ---------------------------------------------------------------------------
// java.net.URL
// ---------------------------------------------------------------------------

fn url_init_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let form = string_argument(shared, args, 0, "URL.<init>")?;
    let data = UrlData::parse(&form).map_err(VmError::Native)?;
    shared.resource_state().urls.insert(receiver, data);
    Ok(None)
}

fn url_init_context(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let context = match args.first().copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => reference,
        _ => {
            return Err(VmError::Native(
                "URL.<init>(URL, String) expects a non-null context URL".to_owned(),
            ));
        }
    };
    let spec = string_argument(shared, args, 1, "URL.<init>(URL, String)")?;
    let data = url_data(shared, context)?
        .resolve(&spec)
        .map_err(VmError::Native)?;
    shared.resource_state().urls.insert(receiver, data);
    Ok(None)
}

fn url_get_protocol(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let data = url_data(shared, receiver)?;
    Ok(Some(string_value(shared, data.protocol)))
}

fn url_get_path(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let data = url_data(shared, receiver)?;
    Ok(Some(string_value(shared, data.path)))
}

fn url_get_file(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let data = url_data(shared, receiver)?;
    Ok(Some(string_value(shared, data.file())))
}

fn url_get_host(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let data = url_data(shared, receiver)?;
    Ok(Some(string_value(shared, data.host)))
}

fn url_get_port(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let data = url_data(shared, receiver)?;
    Ok(Some(Value::Int(data.port)))
}

fn url_get_user_info(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let data = url_data(shared, receiver)?;
    Ok(Some(match data.user_info {
        Some(user_info) => string_value(shared, user_info),
        None => Value::Ref(ObjRef::NULL),
    }))
}

fn url_to_external_form(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let data = url_data(shared, receiver)?;
    Ok(Some(string_value(shared, data.external_form)))
}

fn url_equals(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let current = url_data(shared, receiver)?.external_form;
    let other = match args.first().copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => reference,
        _ => return Ok(Some(Value::Int(0))),
    };
    let equal = shared
        .resource_state()
        .urls
        .get(&other)
        .is_some_and(|data| data.external_form == current);
    Ok(Some(Value::Int(i32::from(equal))))
}

fn url_hash_code(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let form = url_data(shared, receiver)?.external_form;
    let mut hash: i32 = 0;
    for byte in form.bytes() {
        hash = hash.wrapping_mul(31).wrapping_add(i32::from(byte));
    }
    Ok(Some(Value::Int(hash)))
}

fn url_open_stream(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let bytes = read_url_bytes(&url_data(shared, receiver)?)?;
    Ok(Some(Value::Ref(stream_object(shared, bytes)?)))
}

fn url_open_connection(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    // Validate the receiver before allocating: an unparseable URL has no attachment.
    url_data(shared, receiver)?;
    Ok(Some(Value::Ref(new_connection(shared, receiver)?)))
}

// ---------------------------------------------------------------------------
// java.net.URLConnection
// ---------------------------------------------------------------------------

fn connection_init(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let url = match args.first().copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => reference,
        _ => {
            return Err(VmError::Native(
                "URLConnection.<init> expects a non-null URL".to_owned(),
            ));
        }
    };
    url_data(shared, url)?;
    shared.resource_state().connections.insert(
        receiver,
        ConnectionData {
            url,
            use_caches: true,
        },
    );
    Ok(None)
}

fn connection_get_input_stream(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let connection = connection_data(shared, receiver)?;
    let bytes = read_url_bytes(&url_data(shared, connection.url)?)?;
    Ok(Some(Value::Ref(stream_object(shared, bytes)?)))
}

fn connection_get_content_length(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let connection = connection_data(shared, receiver)?;
    let length = match read_url_bytes(&url_data(shared, connection.url)?) {
        Ok(bytes) => i32::try_from(bytes.len()).unwrap_or(i32::MAX),
        Err(_) => -1,
    };
    Ok(Some(Value::Int(length)))
}

fn connection_get_content_length_long(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let connection = connection_data(shared, receiver)?;
    let length = match read_url_bytes(&url_data(shared, connection.url)?) {
        Ok(bytes) => bytes.len() as i64,
        Err(_) => -1,
    };
    Ok(Some(Value::Long(length)))
}

fn connection_get_last_modified(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    // A resource stream has no transport metadata; "unknown" is zero, as the JDK reports.
    Ok(Some(Value::Long(0)))
}

fn connection_get_url(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let connection = connection_data(shared, receiver)?;
    Ok(Some(Value::Ref(connection.url)))
}

fn connection_connect(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(None)
}

fn connection_set_use_caches(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let use_caches = match args.first().copied() {
        Some(Value::Int(value)) => value != 0,
        _ => {
            return Err(VmError::Native(
                "URLConnection.setUseCaches expects a boolean".to_owned(),
            ));
        }
    };
    if let Some(connection) = shared.resource_state().connections.get_mut(&receiver) {
        connection.use_caches = use_caches;
    }
    Ok(None)
}

fn connection_get_use_caches(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let connection = connection_data(shared, receiver)?;
    Ok(Some(Value::Int(i32::from(connection.use_caches))))
}

/// Allocate a connection object attached to `url`.
fn new_connection(shared: &Shared, url: ObjRef) -> Result<ObjRef, VmError> {
    let class = shared.load_library_class(URL_CONNECTION_CLASS)?;
    let reference = shared.allocate_instance(class)?;
    shared.resource_state().connections.insert(
        reference,
        ConnectionData {
            url,
            use_caches: true,
        },
    );
    Ok(reference)
}

// ---------------------------------------------------------------------------
// Opening file: and jar: URLs
// ---------------------------------------------------------------------------

/// The bytes a URL names. No network transport exists in this slice.
fn read_url_bytes(url: &UrlData) -> Result<Vec<u8>, VmError> {
    match url.protocol.as_str() {
        "file" => {
            let path = percent_decode(&url.path)?;
            std::fs::read(&path)
                .map_err(|error| VmError::Native(format!("cannot read {path}: {error}")))
        }
        "jar" => read_jar_url(&url.path),
        protocol => Err(VmError::Native(format!(
            "URL protocol {protocol} is not supported"
        ))),
    }
}

/// `jar:file:<jar>!/<entry>` — the inner form is a `file:` URL and the rest an entry name.
fn read_jar_url(path: &str) -> Result<Vec<u8>, VmError> {
    let inner = path
        .strip_prefix("file:")
        .ok_or_else(|| VmError::Native(format!("jar URL does not wrap a file: URL: {path}")))?;
    let (jar, entry) = inner
        .split_once("!/")
        .ok_or_else(|| VmError::Native(format!("jar URL has no !/ entry separator: {path}")))?;
    if entry.is_empty() {
        return Err(VmError::Native(format!("jar URL names no entry: {path}")));
    }
    let jar = percent_decode(jar)?;
    let entry = percent_decode(entry)?;
    let source = crate::source::JarSource::open(PathBuf::from(jar))?;
    source
        .read_entry(&entry)?
        .ok_or_else(|| VmError::Native(format!("jar has no entry {entry}")))
}

// ---------------------------------------------------------------------------
// java.io.InputStream
// ---------------------------------------------------------------------------

/// Take at most `max` bytes from the receiver's stream, or `None` at EOF.
fn take_bytes(shared: &Shared, reference: ObjRef, max: usize) -> Result<Option<Vec<u8>>, VmError> {
    let mut state = shared.resource_state();
    let stream = state
        .streams
        .get_mut(&reference)
        .ok_or_else(|| VmError::Native(format!("{reference} is not a VM resource stream")))?;
    let remaining = stream.bytes.len() - stream.position;
    if remaining == 0 {
        return Ok(None);
    }
    let count = remaining.min(max);
    let start = stream.position;
    stream.position += count;
    Ok(Some(stream.bytes[start..start + count].to_vec()))
}

/// Consume a VM resource stream, returning its unread bytes.
///
/// `java.util.Properties.load` uses this; the stream itself is an ordinary
/// `java.io.InputStream` and the bytes stay in the resource side table.
pub(crate) fn read_stream_to_end(shared: &Shared, reference: ObjRef) -> Result<Vec<u8>, VmError> {
    let mut state = shared.resource_state();
    let stream = state
        .streams
        .get_mut(&reference)
        .ok_or_else(|| VmError::Native(format!("{reference} is not a VM resource stream")))?;
    let bytes = stream.bytes[stream.position..].to_vec();
    stream.position = stream.bytes.len();
    Ok(bytes)
}

fn stream_remaining(shared: &Shared, reference: ObjRef) -> Result<usize, VmError> {
    let state = shared.resource_state();
    let stream = state
        .streams
        .get(&reference)
        .ok_or_else(|| VmError::Native(format!("{reference} is not a VM resource stream")))?;
    Ok(stream.bytes.len() - stream.position)
}

fn allocate_bytes(shared: &Shared, bytes: &[u8]) -> Result<ObjRef, VmError> {
    let array = shared
        .heap()
        .allocate_array(ClassRef::Primitive(PrimitiveKind::Byte), bytes.len());
    {
        let mut heap = shared.heap();
        let array_slots = heap.array_mut(array).map_err(heap_error)?;
        for (slot, byte) in array_slots.elements.iter_mut().zip(bytes) {
            *slot = Value::Int(i32::from(*byte as i8));
        }
    }
    Ok(array)
}

fn write_bytes(
    shared: &Shared,
    reference: ObjRef,
    offset: usize,
    bytes: &[u8],
    method: &str,
) -> Result<(), VmError> {
    let mut heap = shared.heap();
    let array = heap.array_mut(reference).map_err(heap_error)?;
    if array.kind != ArrayKind::Byte {
        return Err(VmError::Native(format!(
            "{method} expects a byte[], found a {}[]",
            array.kind
        )));
    }
    let end = offset
        .checked_add(bytes.len())
        .ok_or_else(|| VmError::Native(format!("{method} range overflows")))?;
    if end > array.elements.len() {
        return Err(VmError::Native(format!(
            "{method} range {offset}..{end} is outside a byte[] of length {}",
            array.elements.len()
        )));
    }
    for (slot, byte) in array.elements[offset..end].iter_mut().zip(bytes) {
        *slot = Value::Int(i32::from(*byte as i8));
    }
    Ok(())
}

fn byte_array_length(shared: &Shared, reference: ObjRef, method: &str) -> Result<usize, VmError> {
    let heap = shared.heap();
    let array = heap.array(reference).map_err(heap_error)?;
    if array.kind != ArrayKind::Byte {
        return Err(VmError::Native(format!(
            "{method} expects a byte[], found a {}[]",
            array.kind
        )));
    }
    Ok(array.elements.len())
}

fn input_read(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Int(match take_bytes(shared, receiver, 1)? {
        // `InputStream.read()` returns an unsigned 0..=255 value, not a sign-extended byte.
        Some(bytes) => i32::from(bytes[0]),
        None => -1,
    })))
}

fn input_read_array(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let array = array_argument(args, 0, "InputStream.read")?;
    let length = byte_array_length(shared, array, "InputStream.read")?;
    if length == 0 {
        return Ok(Some(Value::Int(0)));
    }
    Ok(Some(Value::Int(
        match take_bytes(shared, receiver, length)? {
            Some(bytes) => {
                write_bytes(shared, array, 0, &bytes, "InputStream.read")?;
                bytes.len() as i32
            }
            None => -1,
        },
    )))
}

fn input_read_array_range(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let array = array_argument(args, 0, "InputStream.read")?;
    let offset = int_argument(args, 1, "InputStream.read")?;
    let length = int_argument(args, 2, "InputStream.read")?;
    let offset = usize::try_from(offset)
        .map_err(|_| VmError::Native("InputStream.read offset is negative".to_owned()))?;
    let length = usize::try_from(length)
        .map_err(|_| VmError::Native("InputStream.read length is negative".to_owned()))?;
    let array_length = byte_array_length(shared, array, "InputStream.read")?;
    let end = offset
        .checked_add(length)
        .ok_or_else(|| VmError::Native("InputStream.read range overflows".to_owned()))?;
    if end > array_length {
        return Err(VmError::Native(format!(
            "InputStream.read range {offset}..{end} is outside a byte[] of length {array_length}"
        )));
    }
    if length == 0 {
        return Ok(Some(Value::Int(0)));
    }
    Ok(Some(Value::Int(
        match take_bytes(shared, receiver, length)? {
            Some(bytes) => {
                write_bytes(shared, array, offset, &bytes, "InputStream.read")?;
                bytes.len() as i32
            }
            None => -1,
        },
    )))
}

fn input_close(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    // The stream is an in-memory snapshot with no descriptor; close is a no-op, matching
    // `ByteArrayInputStream` rather than a real file stream.
    Ok(None)
}

fn input_available(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let remaining = stream_remaining(shared, receiver)?;
    Ok(Some(Value::Int(
        i32::try_from(remaining).unwrap_or(i32::MAX),
    )))
}

fn input_skip(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let requested = long_argument(args, 0, "InputStream.skip")?;
    if requested <= 0 {
        return Ok(Some(Value::Long(0)));
    }
    let mut state = shared.resource_state();
    let stream = state
        .streams
        .get_mut(&receiver)
        .ok_or_else(|| VmError::Native(format!("{receiver} is not a VM resource stream")))?;
    let remaining = stream.bytes.len() - stream.position;
    let skipped = remaining.min(usize::try_from(requested).unwrap_or(usize::MAX));
    stream.position += skipped;
    Ok(Some(Value::Long(skipped as i64)))
}

fn input_read_all_bytes(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let bytes = take_bytes(shared, receiver, usize::MAX)?.unwrap_or_default();
    Ok(Some(Value::Ref(allocate_bytes(shared, &bytes)?)))
}

fn input_read_n_bytes(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let requested = int_argument(args, 0, "InputStream.readNBytes")?;
    let requested = usize::try_from(requested)
        .map_err(|_| VmError::Native("InputStream.readNBytes length is negative".to_owned()))?;
    let bytes = take_bytes(shared, receiver, requested)?.unwrap_or_default();
    Ok(Some(Value::Ref(allocate_bytes(shared, &bytes)?)))
}

fn input_mark_supported(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Int(0)))
}

// ---------------------------------------------------------------------------
// java.io.ByteArrayOutputStream and OutputStream.flush
// ---------------------------------------------------------------------------

fn byte_output_init(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    shared
        .resource_state()
        .byte_output_streams
        .insert(receiver, Vec::new());
    Ok(None)
}

fn byte_output_init_capacity(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let capacity = int_argument(args, 0, "ByteArrayOutputStream.<init>")?;
    let capacity = usize::try_from(capacity)
        .map_err(|_| VmError::Native("ByteArrayOutputStream capacity is negative".to_owned()))?;
    shared
        .resource_state()
        .byte_output_streams
        .insert(receiver, Vec::with_capacity(capacity));
    Ok(None)
}

fn byte_output_write(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = int_argument(args, 0, "ByteArrayOutputStream.write")?;
    let mut state = shared.resource_state();
    let buffer = state
        .byte_output_streams
        .get_mut(&receiver)
        .ok_or_else(|| VmError::Native(format!("{receiver} is not a VM ByteArrayOutputStream")))?;
    buffer.push((value & 0xff) as u8);
    Ok(None)
}

fn byte_output_write_range(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let array = array_argument(args, 0, "ByteArrayOutputStream.write")?;
    let offset = int_argument(args, 1, "ByteArrayOutputStream.write")?;
    let length = int_argument(args, 2, "ByteArrayOutputStream.write")?;
    let bytes = byte_array_range(shared, array, offset, length, "ByteArrayOutputStream.write")?;
    let mut state = shared.resource_state();
    let buffer = state
        .byte_output_streams
        .get_mut(&receiver)
        .ok_or_else(|| VmError::Native(format!("{receiver} is not a VM ByteArrayOutputStream")))?;
    buffer.extend_from_slice(&bytes);
    Ok(None)
}

fn byte_output_write_bytes(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let array = array_argument(args, 0, "ByteArrayOutputStream.writeBytes")?;
    let length = byte_array_length(shared, array, "ByteArrayOutputStream.writeBytes")?;
    let bytes = byte_array_range(
        shared,
        array,
        0,
        i32::try_from(length).unwrap_or(i32::MAX),
        "ByteArrayOutputStream.writeBytes",
    )?;
    let mut state = shared.resource_state();
    let buffer = state
        .byte_output_streams
        .get_mut(&receiver)
        .ok_or_else(|| VmError::Native(format!("{receiver} is not a VM ByteArrayOutputStream")))?;
    buffer.extend_from_slice(&bytes);
    Ok(None)
}

fn byte_output_reset(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    if let Some(buffer) = shared
        .resource_state()
        .byte_output_streams
        .get_mut(&receiver)
    {
        buffer.clear();
    }
    Ok(None)
}

fn byte_output_to_byte_array(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let bytes = {
        let state = shared.resource_state();
        state
            .byte_output_streams
            .get(&receiver)
            .cloned()
            .ok_or_else(|| {
                VmError::Native(format!("{receiver} is not a VM ByteArrayOutputStream"))
            })?
    };
    Ok(Some(Value::Ref(allocate_bytes(shared, &bytes)?)))
}

fn byte_output_size(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let state = shared.resource_state();
    let buffer = state
        .byte_output_streams
        .get(&receiver)
        .ok_or_else(|| VmError::Native(format!("{receiver} is not a VM ByteArrayOutputStream")))?;
    Ok(Some(Value::Int(
        i32::try_from(buffer.len()).unwrap_or(i32::MAX),
    )))
}

fn byte_output_to_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let bytes = {
        let state = shared.resource_state();
        state
            .byte_output_streams
            .get(&receiver)
            .cloned()
            .ok_or_else(|| {
                VmError::Native(format!("{receiver} is not a VM ByteArrayOutputStream"))
            })?
    };
    let text = String::from_utf8_lossy(&bytes).into_owned();
    Ok(Some(string_value(shared, text)))
}

fn byte_output_close(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(None)
}

fn byte_output_write_to(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let target = match args.first().copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => reference,
        _ => {
            return Err(VmError::Native(
                "ByteArrayOutputStream.writeTo expects a non-null OutputStream".to_owned(),
            ));
        }
    };
    let bytes = {
        let state = shared.resource_state();
        state
            .byte_output_streams
            .get(&receiver)
            .cloned()
            .ok_or_else(|| {
                VmError::Native(format!("{receiver} is not a VM ByteArrayOutputStream"))
            })?
    };
    let mut state = shared.resource_state();
    let buffer = state.byte_output_streams.get_mut(&target).ok_or_else(|| {
        VmError::Native("writeTo target is not a VM ByteArrayOutputStream".to_owned())
    })?;
    buffer.extend_from_slice(&bytes);
    Ok(None)
}

fn output_stream_flush(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(None)
}

/// Copy a range out of a heap `byte[]`.
fn byte_array_range(
    shared: &Shared,
    reference: ObjRef,
    offset: i32,
    length: i32,
    method: &str,
) -> Result<Vec<u8>, VmError> {
    let offset = usize::try_from(offset)
        .map_err(|_| VmError::Native(format!("{method} offset is negative")))?;
    let length = usize::try_from(length)
        .map_err(|_| VmError::Native(format!("{method} length is negative")))?;
    let heap = shared.heap();
    let array = heap.array(reference).map_err(heap_error)?;
    if array.kind != ArrayKind::Byte {
        return Err(VmError::Native(format!(
            "{method} expects a byte[], found a {}[]",
            array.kind
        )));
    }
    let end = offset
        .checked_add(length)
        .ok_or_else(|| VmError::Native(format!("{method} range overflows")))?;
    if end > array.elements.len() {
        return Err(VmError::Native(format!(
            "{method} range {offset}..{end} is outside a byte[] of length {}",
            array.elements.len()
        )));
    }
    let mut bytes = Vec::with_capacity(length);
    for element in &array.elements[offset..end] {
        let Value::Int(value) = *element else {
            return Err(VmError::Native(format!(
                "{method} found a non-byte element in the byte[]"
            )));
        };
        bytes.push((value & 0xff) as u8);
    }
    Ok(bytes)
}

// ---------------------------------------------------------------------------
// java.net.URLDecoder
// ---------------------------------------------------------------------------

fn url_decoder_decode(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let encoded = string_argument(shared, args, 0, "URLDecoder.decode")?;
    // `application/x-www-form-urlencoded` spells a space as `+`; `%2B` decodes later.
    let normalized = encoded.replace('+', " ");
    let decoded = percent_decode(&normalized)?;
    Ok(Some(string_value(shared, decoded)))
}

fn split_scheme(spec: &str) -> Option<(&str, &str)> {
    let colon = spec.find(':')?;
    let scheme = &spec[..colon];
    let mut characters = scheme.chars();
    let first = characters.next()?;
    if !first.is_ascii_alphabetic() {
        return None;
    }
    if !characters
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.'))
    {
        return None;
    }
    Some((scheme, &spec[colon + 1..]))
}

fn split_once(text: &str, separator: char) -> (&str, Option<String>) {
    match text.split_once(separator) {
        Some((head, tail)) => (head, Some(tail.to_owned())),
        None => (text, None),
    }
}

/// Split `rest` (scheme already removed) into authority-ish parts and a path.
fn parse_authority(rest: &str) -> Result<(String, i32, Option<String>, String), String> {
    let Some(after) = rest.strip_prefix("//") else {
        // An opaque or rootless path such as `file:/tmp/x`, `jar:file:...` or `mailto:a@b`.
        let path = if rest.is_empty() {
            "/".to_owned()
        } else {
            rest.to_owned()
        };
        return Ok((String::new(), -1, None, path));
    };
    let (authority, path) = match after.find('/') {
        Some(index) => (&after[..index], &after[index..]),
        None => (after, ""),
    };
    let (user_info, host_port) = match authority.rsplit_once('@') {
        Some((user_info, host_port)) => (Some(user_info.to_owned()), host_port),
        None => (None, authority),
    };
    let (host, port) = if host_port.starts_with('[') {
        let end = host_port
            .find(']')
            .ok_or_else(|| format!("unterminated IPv6 host in URL: {host_port}"))?;
        let host = host_port[..=end].to_owned();
        let rest = &host_port[end + 1..];
        let port = match rest.strip_prefix(':') {
            Some(port) => port
                .parse::<i32>()
                .map_err(|_| format!("invalid port in URL: {port}"))?,
            None => -1,
        };
        (host, port)
    } else {
        match host_port.rsplit_once(':') {
            Some((host, port)) if !port.is_empty() => (
                host.to_owned(),
                port.parse::<i32>()
                    .map_err(|_| format!("invalid port in URL: {port}"))?,
            ),
            Some((host, _)) => (host.to_owned(), -1),
            None => (host_port.to_owned(), -1),
        }
    };
    Ok((host, port, user_info, path.to_owned()))
}

/// Collapse `.` and `..` segments of a URL path, keeping a leading `/`.
fn normalize_path(path: &str) -> String {
    let absolute = path.starts_with('/');
    let mut segments: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            segment => segments.push(segment),
        }
    }
    let joined = segments.join("/");
    if absolute {
        format!("/{joined}")
    } else {
        joined
    }
}

fn percent_decode(text: &str) -> Result<String, VmError> {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = bytes
                .get(index + 1)
                .and_then(|byte| hex_value(*byte))
                .ok_or_else(|| VmError::Native(format!("invalid percent escape in {text}")))?;
            let low = bytes
                .get(index + 2)
                .and_then(|byte| hex_value(*byte))
                .ok_or_else(|| VmError::Native(format!("invalid percent escape in {text}")))?;
            decoded.push(high * 16 + low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded)
        .map_err(|_| VmError::Native(format!("URL path is not UTF-8: {text}")))
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_url_text_is_split_into_components() {
        let url = UrlData::parse("file:/tmp/demo/data.txt").unwrap();
        assert_eq!(url.protocol, "file");
        assert_eq!(url.path, "/tmp/demo/data.txt");
        assert_eq!(url.file(), "/tmp/demo/data.txt");
        assert_eq!(url.host, "");
        assert_eq!(url.port, -1);
        assert_eq!(url.external_form, "file:/tmp/demo/data.txt");
    }

    #[test]
    fn jar_url_keeps_the_inner_form_as_its_path() {
        let url = UrlData::parse("jar:file:/tmp/demo.jar!/demo/data.txt").unwrap();
        assert_eq!(url.protocol, "jar");
        assert_eq!(url.path, "file:/tmp/demo.jar!/demo/data.txt");
        assert_eq!(url.host, "");
        assert_eq!(url.external_form, "jar:file:/tmp/demo.jar!/demo/data.txt");
    }

    #[test]
    fn a_relative_reference_resolves_against_the_context_directory() {
        let base = UrlData::parse("file:/tmp/demo/data.txt").unwrap();
        let resolved = base.resolve("other.txt").unwrap();
        assert_eq!(resolved.path, "/tmp/demo/other.txt");
        let absolute = base.resolve("/root.txt").unwrap();
        assert_eq!(absolute.path, "/root.txt");
    }

    #[test]
    fn a_missing_protocol_is_an_error_not_a_panic() {
        assert!(UrlData::parse("demo/data.txt").is_err());
        assert!(UrlData::parse("1file:/tmp/x").is_err());
    }

    #[test]
    fn input_stream_read_returns_unsigned_bytes() {
        let shared = Shared::with_library_dir(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../stdlib/c1/classes"
        ));
        let thread = shared.thread_state(1);
        let stream = stream_object(&shared, vec![0x00, 0x80, 0xFF]).unwrap();
        for expected in [0, 128, 255] {
            let value = input_read(&shared, &thread, LoaderId::BOOTSTRAP, stream, &[])
                .expect("read succeeds")
                .expect("read returns a value");
            assert_eq!(value, Value::Int(expected));
        }
        let eof = input_read(&shared, &thread, LoaderId::BOOTSTRAP, stream, &[])
            .expect("EOF read succeeds")
            .expect("EOF read returns a value");
        assert_eq!(eof, Value::Int(-1));
    }
}
