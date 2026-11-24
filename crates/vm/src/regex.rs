//! A `java.util.regex` surface over the Rust `regex` crate (M2.23).
//!
//! Spring compiles a few patterns in static initializers (`AntPathMatcher`'s
//! `\{[^/]+?\}`) and uses `Matcher.find`/`group`/`replaceAll` when a path contains a URI
//! template. The Rust engine covers those constructs. The two engines are not identical:
//! backreferences and look-around are rejected by Rust's `regex`, Java's `\Q...\E` quoting
//! is not translated, and a replacement's backslash escapes follow Rust's rules. Those
//! differences are the documented M2.23 boundary; a malformed pattern is a typed
//! [`crate::VmError::Native`] error rather than a synthesized `PatternSyntaxException`.
//!
//! The pattern and matcher are heap objects ([`crate::HeapObject::Pattern`] and
//! [`crate::HeapObject::Matcher`]); the matcher holds its input and the groups of the most
//! recent match, so `group`, `start` and `end` are plain reads.

use regex::Regex;

use crate::VmError;
use crate::heap::{MatcherObject, ObjRef};
use crate::loader::LoaderId;
use crate::shared::Shared;
use crate::thread::ThreadState;
use crate::value::Value;

pub const PATTERN_CLASS: &str = "java/util/regex/Pattern";
pub const MATCHER_CLASS: &str = "java/util/regex/Matcher";

pub fn register_natives(shared: &Shared) {
    shared.register_native(
        PATTERN_CLASS,
        "compile",
        "(Ljava/lang/String;)Ljava/util/regex/Pattern;",
        pattern_compile,
    );
    shared.register_native(
        PATTERN_CLASS,
        "compile",
        "(Ljava/lang/String;I)Ljava/util/regex/Pattern;",
        pattern_compile,
    );
    shared.register_native(
        PATTERN_CLASS,
        "matcher",
        "(Ljava/lang/CharSequence;)Ljava/util/regex/Matcher;",
        pattern_matcher,
    );
    shared.register_native(
        PATTERN_CLASS,
        "pattern",
        "()Ljava/lang/String;",
        pattern_source,
    );
    shared.register_native(
        PATTERN_CLASS,
        "toString",
        "()Ljava/lang/String;",
        pattern_source,
    );
    shared.register_native(
        PATTERN_CLASS,
        "split",
        "(Ljava/lang/CharSequence;)[Ljava/lang/String;",
        pattern_split,
    );
    shared.register_native(
        PATTERN_CLASS,
        "quote",
        "(Ljava/lang/String;)Ljava/lang/String;",
        pattern_quote,
    );

    shared.register_native(MATCHER_CLASS, "find", "()Z", matcher_find);
    shared.register_native(MATCHER_CLASS, "matches", "()Z", matcher_matches);
    shared.register_native(MATCHER_CLASS, "lookingAt", "()Z", matcher_looking_at);
    shared.register_native(
        MATCHER_CLASS,
        "group",
        "()Ljava/lang/String;",
        matcher_group,
    );
    shared.register_native(
        MATCHER_CLASS,
        "group",
        "(I)Ljava/lang/String;",
        matcher_group_at,
    );
    shared.register_native(MATCHER_CLASS, "start", "()I", matcher_start);
    shared.register_native(MATCHER_CLASS, "end", "()I", matcher_end);
    shared.register_native(
        MATCHER_CLASS,
        "reset",
        "()Ljava/util/regex/Matcher;",
        matcher_reset,
    );
    shared.register_native(
        MATCHER_CLASS,
        "reset",
        "(Ljava/lang/CharSequence;)Ljava/util/regex/Matcher;",
        matcher_reset_input,
    );
    shared.register_native(
        MATCHER_CLASS,
        "replaceAll",
        "(Ljava/lang/String;)Ljava/lang/String;",
        matcher_replace_all,
    );
    shared.register_native(
        MATCHER_CLASS,
        "replaceFirst",
        "(Ljava/lang/String;)Ljava/lang/String;",
        matcher_replace_first,
    );
    shared.register_native(
        MATCHER_CLASS,
        "quoteReplacement",
        "(Ljava/lang/String;)Ljava/lang/String;",
        matcher_quote_replacement,
    );
}

fn heap_error(error: crate::HeapError) -> VmError {
    VmError::Native(error.to_string())
}

fn string_argument(
    shared: &Shared,
    args: &[Value],
    position: usize,
    method: &'static str,
) -> Result<String, VmError> {
    let Some(Value::Ref(reference)) = args.get(position).copied() else {
        return Err(VmError::Native(format!(
            "{method} expects a String argument"
        )));
    };
    if reference.is_null() {
        return Err(VmError::Native(format!("{method} received null")));
    }
    let text = shared
        .heap()
        .string(reference)
        .map_err(heap_error)?
        .to_owned();
    Ok(text)
}

fn compile(source: &str) -> Result<Regex, VmError> {
    Regex::new(source).map_err(|error| {
        VmError::Native(format!(
            "java.util.regex cannot compile {source:?}: {error}"
        ))
    })
}

fn pattern_compile(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let source = string_argument(shared, args, 0, "Pattern.compile")?;
    compile(&source)?;
    let pattern = shared.heap().allocate_pattern(source);
    Ok(Some(Value::Ref(pattern)))
}

fn pattern_source(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let source = {
        let heap = shared.heap();
        heap.pattern(receiver).map_err(heap_error)?.to_owned()
    };
    let text = shared.intern_string(&source);
    Ok(Some(Value::Ref(text)))
}

fn pattern_matcher(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let source = {
        let heap = shared.heap();
        heap.pattern(receiver).map_err(heap_error)?.to_owned()
    };
    let input = string_argument(shared, args, 0, "Pattern.matcher")?;
    let matcher = shared.heap().allocate_matcher(source, input);
    Ok(Some(Value::Ref(matcher)))
}

fn pattern_split(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let source = {
        let heap = shared.heap();
        heap.pattern(receiver).map_err(heap_error)?.to_owned()
    };
    let input = string_argument(shared, args, 0, "Pattern.split")?;
    let regex = compile(&source)?;
    let parts: Vec<String> = regex.split(&input).map(str::to_owned).collect();
    let strings: Vec<ObjRef> = parts
        .iter()
        .map(|part| shared.heap().allocate_string(part.clone()))
        .collect();
    let class = shared.load_library_class("java/lang/String")?;
    let array = shared
        .heap()
        .allocate_array(crate::heap::ClassRef::Named(class), strings.len());
    {
        let mut heap = shared.heap();
        let elements = heap.array_mut(array).expect("a freshly allocated array");
        for (slot, string) in elements.elements.iter_mut().zip(strings) {
            *slot = Value::Ref(string);
        }
    }
    Ok(Some(Value::Ref(array)))
}

fn pattern_quote(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let source = string_argument(shared, args, 0, "Pattern.quote")?;
    let text = shared.intern_string(&regex::escape(&source));
    Ok(Some(Value::Ref(text)))
}

fn compile_matcher(matcher: &MatcherObject) -> Result<Regex, VmError> {
    compile(&matcher.pattern)
}

fn record_match(matcher: &mut MatcherObject, groups: &regex::Captures<'_>) {
    matcher.matched = true;
    let whole = groups.get(0).expect("group 0 is the whole match");
    matcher.start = whole.start();
    matcher.end = whole.end();
    matcher.groups = (0..groups.len())
        .map(|index| groups.get(index).map(|group| (group.start(), group.end())))
        .collect();
}

fn matcher_find(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let mut heap = shared.heap();
    let matcher = heap.matcher_mut(receiver).map_err(heap_error)?;
    let regex = compile_matcher(matcher)?;
    let input = matcher.input.clone();
    let from = matcher.search_from.min(input.len());
    let found = regex
        .captures(&input[from..])
        .map(|groups| record_match_offset(matcher, &groups, from));
    match found {
        Some(()) => {
            matcher.search_from = matcher.end;
            Ok(Some(Value::Int(1)))
        }
        None => {
            matcher.matched = false;
            matcher.search_from = input.len();
            Ok(Some(Value::Int(0)))
        }
    }
}

/// Record captures found in a suffix of the input, shifting their positions by `offset`.
fn record_match_offset(matcher: &mut MatcherObject, groups: &regex::Captures<'_>, offset: usize) {
    matcher.matched = true;
    let whole = groups.get(0).expect("group 0 is the whole match");
    matcher.start = whole.start() + offset;
    matcher.end = whole.end() + offset;
    matcher.groups = (0..groups.len())
        .map(|index| {
            groups
                .get(index)
                .map(|group| (group.start() + offset, group.end() + offset))
        })
        .collect();
}

fn matcher_matches(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let mut heap = shared.heap();
    let matcher = heap.matcher_mut(receiver).map_err(heap_error)?;
    let regex = compile_matcher(matcher)?;
    let input = matcher.input.clone();
    let matched = regex.captures(&input).is_some_and(|groups| {
        let whole = groups.get(0).expect("group 0");
        whole.start() == 0 && whole.end() == input.len()
    });
    if matched {
        if let Some(groups) = regex.captures(&input) {
            record_match(matcher, &groups);
        }
    } else {
        matcher.matched = false;
    }
    Ok(Some(Value::Int(i32::from(matched))))
}

fn matcher_looking_at(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let mut heap = shared.heap();
    let matcher = heap.matcher_mut(receiver).map_err(heap_error)?;
    let regex = compile_matcher(matcher)?;
    let input = matcher.input.clone();
    match regex.captures(&input) {
        Some(groups) if groups.get(0).map(|group| group.start()) == Some(0) => {
            record_match(matcher, &groups);
            Ok(Some(Value::Int(1)))
        }
        _ => {
            matcher.matched = false;
            Ok(Some(Value::Int(0)))
        }
    }
}

fn matcher_group(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    matcher_group_at(shared, _thread, _loader, receiver, &[Value::Int(0)])
}

fn matcher_group_at(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let index = match args.first().copied() {
        Some(Value::Int(value)) if value >= 0 => value as usize,
        _ => return Err(VmError::Native("Matcher.group expects an int".to_owned())),
    };
    let (input, range) = {
        let mut heap = shared.heap();
        let matcher = heap.matcher_mut(receiver).map_err(heap_error)?;
        if !matcher.matched {
            return Ok(Some(Value::Ref(ObjRef::NULL)));
        }
        let range = matcher.groups.get(index).copied().flatten();
        (matcher.input.clone(), range)
    };
    match range {
        Some((start, end)) => {
            let text = shared.intern_string(&input[start..end]);
            Ok(Some(Value::Ref(text)))
        }
        None => Ok(Some(Value::Ref(ObjRef::NULL))),
    }
}

fn matcher_start(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let start = {
        let heap = shared.heap();
        let matcher = heap.matcher(receiver).map_err(heap_error)?;
        if matcher.matched { matcher.start } else { 0 }
    };
    Ok(Some(Value::Int(start as i32)))
}

fn matcher_end(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let end = {
        let heap = shared.heap();
        let matcher = heap.matcher(receiver).map_err(heap_error)?;
        if matcher.matched { matcher.end } else { 0 }
    };
    Ok(Some(Value::Int(end as i32)))
}

fn matcher_reset(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let mut heap = shared.heap();
    let matcher = heap.matcher_mut(receiver).map_err(heap_error)?;
    matcher.search_from = 0;
    matcher.matched = false;
    matcher.groups.clear();
    Ok(Some(Value::Ref(receiver)))
}

fn matcher_reset_input(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let input = string_argument(shared, args, 0, "Matcher.reset")?;
    let mut heap = shared.heap();
    let matcher = heap.matcher_mut(receiver).map_err(heap_error)?;
    matcher.input = input;
    matcher.search_from = 0;
    matcher.matched = false;
    matcher.groups.clear();
    Ok(Some(Value::Ref(receiver)))
}

fn matcher_replace_all(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let replacement = string_argument(shared, args, 0, "Matcher.replaceAll")?;
    let (regex, input) = matcher_parts(shared, receiver)?;
    let text = regex.replace_all(&input, replacement.as_str()).into_owned();
    let text = shared.intern_string(&text);
    Ok(Some(Value::Ref(text)))
}

fn matcher_replace_first(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let replacement = string_argument(shared, args, 0, "Matcher.replaceFirst")?;
    let (regex, input) = matcher_parts(shared, receiver)?;
    let text = regex.replace(&input, replacement.as_str()).into_owned();
    let text = shared.intern_string(&text);
    Ok(Some(Value::Ref(text)))
}

fn matcher_parts(shared: &Shared, receiver: ObjRef) -> Result<(Regex, String), VmError> {
    let heap = shared.heap();
    let matcher = heap.matcher(receiver).map_err(heap_error)?;
    Ok((compile(&matcher.pattern)?, matcher.input.clone()))
}

fn matcher_quote_replacement(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let replacement = string_argument(shared, args, 0, "Matcher.quoteReplacement")?;
    let text = shared.intern_string(&replacement.replace('$', "$$"));
    Ok(Some(Value::Ref(text)))
}
