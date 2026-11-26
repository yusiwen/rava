//! `java.net.URI` natives (M4.5).
//!
//! Spring Boot resolves configuration-data locations as `java.net.URI` values:
//! `ConfigDataLocation.of` parses them, the resolvers inspect scheme, authority and path,
//! and a `file:` URI is turned into a `URL`. The C1 stub declares no `Code`, so this module
//! parses RFC 3986-shaped references and answers the query methods the startup path uses.
//!
//! A URI's components are Rust text in [`UriState`], a side table keyed by [`ObjRef`],
//! exactly as the M4.1 URL objects are stored. The raw components and the percent-decoded
//! forms are both kept, so `getRawPath` and `getPath` differ where they should. Relative
//! references, `resolve` and dot-segment normalization follow RFC 3986 §5.2; the opaque
//! (scheme-specific-part) form is preserved.

use std::collections::HashMap;

use crate::VmError;
use crate::heap::ObjRef;
use crate::loader::LoaderId;
use crate::shared::Shared;
use crate::thread::ThreadState;
use crate::value::Value;

const URI_CLASS: &str = "java/net/URI";

/// One parsed URI.
#[derive(Debug, Clone, PartialEq, Eq)]
struct UriData {
    scheme: Option<String>,
    authority: Option<String>,
    path: String,
    query: Option<String>,
    fragment: Option<String>,
    opaque: bool,
}

impl UriData {
    fn parse(form: &str) -> UriData {
        let mut rest = form;
        let mut scheme = None;
        if let Some((candidate, tail)) = split_scheme(rest) {
            scheme = Some(candidate.to_owned());
            rest = tail;
        }
        let (rest, fragment) = split_once(rest, '#');
        let (rest, query) = split_once(rest, '?');
        let mut authority = None;
        let mut path = rest;
        if let Some(after) = rest.strip_prefix("//") {
            let split = after.find(['/', '?', '#']).unwrap_or(after.len());
            authority = Some(after[..split].to_owned());
            path = &after[split..];
        }
        let opaque =
            scheme.is_some() && authority.is_none() && !path.is_empty() && !path.starts_with('/');
        UriData {
            scheme,
            authority,
            path: path.to_owned(),
            query,
            fragment,
            opaque,
        }
    }

    fn compose(&self) -> String {
        let mut text = String::new();
        if let Some(scheme) = &self.scheme {
            text.push_str(scheme);
            text.push(':');
        }
        if let Some(authority) = &self.authority {
            text.push_str("//");
            text.push_str(authority);
        }
        text.push_str(&self.path);
        if let Some(query) = &self.query {
            text.push('?');
            text.push_str(query);
        }
        if let Some(fragment) = &self.fragment {
            text.push('#');
            text.push_str(fragment);
        }
        text
    }

    fn scheme_specific_part(&self) -> String {
        let mut text = String::new();
        if let Some(authority) = &self.authority {
            text.push_str("//");
            text.push_str(authority);
        }
        text.push_str(&self.path);
        if let Some(query) = &self.query {
            text.push('?');
            text.push_str(query);
        }
        text
    }

    fn resolve(&self, reference: &UriData) -> UriData {
        if reference.scheme.is_some() {
            return reference.clone();
        }
        let mut target = self.clone();
        if let Some(authority) = &reference.authority {
            target.authority = Some(authority.clone());
            target.path = normalize_path(&reference.path);
            target.query = reference.query.clone();
        } else if reference.path.is_empty() {
            if reference.query.is_some() {
                target.query = reference.query.clone();
            }
        } else {
            target.path = if reference.path.starts_with('/') {
                normalize_path(&reference.path)
            } else {
                merge_path(&self.path, &reference.path)
            };
            target.query = reference.query.clone();
        }
        target.fragment = reference.fragment.clone();
        target
    }
}

/// The Rust state attached to the `java.net.URI` objects this VM allocates.
#[derive(Debug, Default)]
pub struct UriState {
    uris: HashMap<ObjRef, UriData>,
}

impl UriState {
    /// Every object whose URI state the table holds, for root enumeration.
    pub fn references(&self) -> Vec<ObjRef> {
        self.uris.keys().copied().collect()
    }

    /// Record a URI object's external form, parsing it first.
    pub fn insert(&mut self, object: ObjRef, form: impl AsRef<str>) {
        self.uris.insert(object, UriData::parse(form.as_ref()));
    }

    /// The object's external form, if it has URI state.
    pub fn text(&self, object: ObjRef) -> Option<String> {
        self.uris.get(&object).map(UriData::compose)
    }
}

pub fn register_natives(shared: &Shared) {
    shared.register_native(URI_CLASS, "<init>", "(Ljava/lang/String;)V", uri_init);
    shared.register_native(
        URI_CLASS,
        "create",
        "(Ljava/lang/String;)Ljava/net/URI;",
        uri_create,
    );
    shared.register_native(
        URI_CLASS,
        "getScheme",
        "()Ljava/lang/String;",
        uri_get_scheme,
    );
    shared.register_native(
        URI_CLASS,
        "getSchemeSpecificPart",
        "()Ljava/lang/String;",
        uri_get_scheme_specific_part,
    );
    shared.register_native(
        URI_CLASS,
        "getRawSchemeSpecificPart",
        "()Ljava/lang/String;",
        uri_get_scheme_specific_part,
    );
    shared.register_native(URI_CLASS, "isAbsolute", "()Z", uri_is_absolute);
    shared.register_native(URI_CLASS, "isOpaque", "()Z", uri_is_opaque);
    for (name, raw) in [
        ("getAuthority", false),
        ("getRawAuthority", true),
        ("getUserInfo", false),
        ("getRawUserInfo", true),
        ("getHost", true),
    ] {
        let function = match name {
            "getAuthority" | "getRawAuthority" => uri_get_authority as crate::NativeFn,
            "getUserInfo" | "getRawUserInfo" => uri_get_user_info,
            _ => uri_get_host,
        };
        let _ = raw;
        shared.register_native(URI_CLASS, name, "()Ljava/lang/String;", function);
    }
    shared.register_native(URI_CLASS, "getPort", "()I", uri_get_port);
    for (name, decode) in [
        ("getPath", true),
        ("getRawPath", false),
        ("getQuery", true),
        ("getRawQuery", false),
        ("getFragment", true),
        ("getRawFragment", false),
    ] {
        let function = match name {
            "getPath" | "getRawPath" => uri_get_path as crate::NativeFn,
            "getQuery" | "getRawQuery" => uri_get_query,
            _ => uri_get_fragment,
        };
        let _ = decode;
        shared.register_native(URI_CLASS, name, "()Ljava/lang/String;", function);
    }
    shared.register_native(URI_CLASS, "toString", "()Ljava/lang/String;", uri_to_string);
    shared.register_native(
        URI_CLASS,
        "toASCIIString",
        "()Ljava/lang/String;",
        uri_to_string,
    );
    shared.register_native(URI_CLASS, "normalize", "()Ljava/net/URI;", uri_normalize);
    shared.register_native(
        URI_CLASS,
        "resolve",
        "(Ljava/net/URI;)Ljava/net/URI;",
        uri_resolve,
    );
    shared.register_native(
        URI_CLASS,
        "resolve",
        "(Ljava/lang/String;)Ljava/net/URI;",
        uri_resolve_string,
    );
    shared.register_native(URI_CLASS, "toURL", "()Ljava/net/URL;", uri_to_url);
    shared.register_native(URI_CLASS, "equals", "(Ljava/lang/Object;)Z", uri_equals);
    shared.register_native(URI_CLASS, "hashCode", "()I", uri_hash_code);
    shared.register_native(URI_CLASS, "compareTo", "(Ljava/net/URI;)I", uri_compare_to);
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

fn allocate_uri(shared: &Shared, data: UriData) -> Result<ObjRef, VmError> {
    let class = shared.load_library_class(URI_CLASS)?;
    let object = shared.allocate_instance(class)?;
    shared.uri_state().uris.insert(object, data);
    Ok(object)
}

fn uri_data(shared: &Shared, receiver: ObjRef, method: &str) -> Result<UriData, VmError> {
    shared
        .uri_state()
        .uris
        .get(&receiver)
        .cloned()
        .ok_or_else(|| VmError::Native(format!("{method}: URI has no state")))
}

fn uri_init(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let form = string_argument(shared, args, 0, "URI.<init>")?;
    shared
        .uri_state()
        .uris
        .insert(receiver, UriData::parse(&form));
    Ok(None)
}

fn uri_create(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let form = string_argument(shared, args, 0, "URI.create")?;
    Ok(Some(Value::Ref(allocate_uri(
        shared,
        UriData::parse(&form),
    )?)))
}

fn uri_get_scheme(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let scheme = uri_data(shared, receiver, "URI.getScheme")?.scheme;
    Ok(Some(match scheme {
        Some(scheme) => new_string(shared, scheme),
        None => Value::Ref(ObjRef::NULL),
    }))
}

fn uri_get_scheme_specific_part(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let data = uri_data(shared, receiver, "URI.getSchemeSpecificPart")?;
    Ok(Some(new_string(shared, data.scheme_specific_part())))
}

fn uri_is_absolute(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let absolute = uri_data(shared, receiver, "URI.isAbsolute")?
        .scheme
        .is_some();
    Ok(Some(Value::Int(i32::from(absolute))))
}

fn uri_is_opaque(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let opaque = uri_data(shared, receiver, "URI.isOpaque")?.opaque;
    Ok(Some(Value::Int(i32::from(opaque))))
}

fn uri_get_authority(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let authority = uri_data(shared, receiver, "URI.getAuthority")?.authority;
    Ok(Some(match authority {
        Some(authority) => new_string(shared, authority),
        None => Value::Ref(ObjRef::NULL),
    }))
}

/// The user-info part of the authority (`user@`), or null.
fn uri_get_user_info(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let authority = uri_data(shared, receiver, "URI.getUserInfo")?.authority;
    let user = authority
        .as_deref()
        .and_then(|authority| authority.rsplit_once('@'))
        .map(|(user, _)| user.to_owned());
    Ok(Some(match user {
        Some(user) => new_string(shared, user),
        None => Value::Ref(ObjRef::NULL),
    }))
}

/// The host part of the authority, without the port.
fn uri_get_host(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let authority = uri_data(shared, receiver, "URI.getHost")?.authority;
    let host = authority
        .as_deref()
        .map(|authority| {
            authority
                .rsplit_once('@')
                .map_or(authority, |(_, host)| host)
        })
        .map(|host| match host.rsplit_once(':') {
            Some((host, port)) if port.chars().all(|c| c.is_ascii_digit()) => host.to_owned(),
            _ => host.to_owned(),
        });
    Ok(Some(match host {
        Some(host) if !host.is_empty() => new_string(shared, host),
        _ => Value::Ref(ObjRef::NULL),
    }))
}

fn uri_get_port(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let authority = uri_data(shared, receiver, "URI.getPort")?.authority;
    let port = authority
        .as_deref()
        .and_then(|authority| authority.rsplit_once(':'))
        .and_then(|(_, port)| port.parse::<i32>().ok())
        .unwrap_or(-1);
    Ok(Some(Value::Int(port)))
}

fn uri_get_path(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let path = uri_data(shared, receiver, "URI.getPath")?.path;
    Ok(Some(new_string(shared, decode_path(&path))))
}

fn uri_get_query(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let query = uri_data(shared, receiver, "URI.getQuery")?.query;
    Ok(Some(match query {
        Some(query) => new_string(shared, decode(&query)),
        None => Value::Ref(ObjRef::NULL),
    }))
}

fn uri_get_fragment(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let fragment = uri_data(shared, receiver, "URI.getFragment")?.fragment;
    Ok(Some(match fragment {
        Some(fragment) => new_string(shared, decode(&fragment)),
        None => Value::Ref(ObjRef::NULL),
    }))
}

fn uri_to_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = uri_data(shared, receiver, "URI.toString")?.compose();
    Ok(Some(new_string(shared, text)))
}

fn uri_normalize(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let mut data = uri_data(shared, receiver, "URI.normalize")?;
    data.path = normalize_path(&data.path);
    Ok(Some(Value::Ref(allocate_uri(shared, data)?)))
}

fn uri_resolve(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let reference = match args.first().copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => reference,
        _ => {
            return Err(VmError::Native(
                "URI.resolve expects a non-null URI".to_owned(),
            ));
        }
    };
    let base = uri_data(shared, receiver, "URI.resolve")?;
    let reference = uri_data(shared, reference, "URI.resolve")?;
    Ok(Some(Value::Ref(allocate_uri(
        shared,
        base.resolve(&reference),
    )?)))
}

fn uri_resolve_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let form = string_argument(shared, args, 0, "URI.resolve")?;
    let base = uri_data(shared, receiver, "URI.resolve")?;
    Ok(Some(Value::Ref(allocate_uri(
        shared,
        base.resolve(&UriData::parse(&form)),
    )?)))
}

fn uri_to_url(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let data = uri_data(shared, receiver, "URI.toURL")?;
    if data.scheme.is_none() {
        // The JDK's `URI.toURL` throws `IllegalArgumentException` for a relative URI;
        // Spring's `ResourceUtils.toURL` catches it and treats the location as a path.
        let throwable = crate::exceptions::new_throwable(
            shared,
            "java/lang/IllegalArgumentException",
            "URI is not absolute".to_owned(),
        )?;
        return Err(VmError::Thrown(throwable));
    }
    let form = data.compose();
    match crate::resources::url_object(shared, &form) {
        Ok(url) => Ok(Some(Value::Ref(url))),
        Err(error) => {
            let throwable = crate::exceptions::new_throwable(
                shared,
                "java/net/MalformedURLException",
                format!("no URL handler for {form}: {error}"),
            )?;
            Err(VmError::Thrown(throwable))
        }
    }
}

fn uri_equals(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let equals = match args.first().copied() {
        Some(Value::Ref(other)) if !other.is_null() => {
            let left = uri_data(shared, receiver, "URI.equals")?;
            let right = shared.uri_state().uris.get(&other).cloned();
            right.as_ref() == Some(&left)
        }
        _ => false,
    };
    Ok(Some(Value::Int(i32::from(equals))))
}

fn uri_hash_code(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = uri_data(shared, receiver, "URI.hashCode")?.compose();
    let mut hash = 0i32;
    for unit in text.encode_utf16() {
        hash = hash.wrapping_mul(31).wrapping_add(i32::from(unit));
    }
    Ok(Some(Value::Int(hash)))
}

fn uri_compare_to(
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
                "URI.compareTo expects a non-null URI".to_owned(),
            ));
        }
    };
    let left = uri_data(shared, receiver, "URI.compareTo")?.compose();
    let right = uri_data(shared, other, "URI.compareTo")?.compose();
    Ok(Some(Value::Int(match left.cmp(&right) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    })))
}

// --- parsing helpers -------------------------------------------------------

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

fn merge_path(base: &str, reference: &str) -> String {
    let prefix = match base.rfind('/') {
        Some(index) => &base[..=index],
        None => "",
    };
    normalize_path(&format!("{prefix}{reference}"))
}

/// RFC 3986 §5.2.4 dot-segment removal.
fn normalize_path(path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }
    let absolute = path.starts_with('/');
    let trailing = path.ends_with('/');
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
    let mut normalized = String::new();
    if absolute {
        normalized.push('/');
    }
    normalized.push_str(&segments.join("/"));
    if trailing && !normalized.is_empty() {
        normalized.push('/');
    }
    normalized
}

/// Percent-decode a component. A malformed escape is left literal, which keeps the
/// original text rather than guessing.
fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let high = (bytes[index + 1] as char).to_digit(16);
            let low = (bytes[index + 2] as char).to_digit(16);
            if let (Some(high), Some(low)) = (high, low) {
                out.push((high * 16 + low) as u8);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_owned())
}

/// `getPath` decodes escapes but does not turn `+` into a space (that is a query rule).
fn decode_path(path: &str) -> String {
    decode(path)
}
