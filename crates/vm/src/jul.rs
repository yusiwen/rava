//! A minimal `java.util.logging` native surface (M2.23).
//!
//! Spring's `spring-jcl` bridge picks `java.util.logging` when no SLF4J or Log4j API is
//! on the classpath. Its `JavaUtilLog` wrapper calls `Logger.getLogger(name)`,
//! `Logger.isLoggable(Level)` and, when a level is enabled, `Logger.log(LogRecord)`; the
//! `LogRecord` it builds is its own `LocationResolvingLogRecord` subclass, whose
//! constructor calls `LogRecord.<init>(Level, String)`.
//!
//! This module implements exactly that surface. The C1 `Logger` and `LogRecord` stubs have
//! no initializer `Code` and their private fields are not part of the projected metadata,
//! so the mutable state lives in [`JulState`], keyed by the heap handle. A `Logger` is
//! still an ordinary `Instance` of `java.util.logging.Logger`; only its name is stored
//! here, and one object is cached per name so repeated `getLogger` calls agree by
//! reference. A `LogRecord` is allocated by the ordinary `new` path (possibly as a
//! subclass such as `LocationResolvingLogRecord`) and its data is recorded when its
//! constructor runs.
//!
//! The nine `Level` constants are [`crate::HeapObject::LogLevel`] values written into
//! `java.util.logging.Level`'s statics by [`install_levels`]. Formatted output goes to the
//! VM's captured stderr, never stdout: Spring's diagnostics must not disturb the program's
//! `System.out`.
//!
//! Boundaries: there is one level per logger (the root `INFO` default) and no
//! `logging.properties`, `Handler`, `Formatter`, `LogManager` or resource bundle. A
//! record's message is rendered by `String.valueOf`-style text when it is logged, and a
//! throwable is reported by its class name.

use std::collections::HashMap;

use crate::VmError;
use crate::heap::{HeapObject, LevelKind, ObjRef};
use crate::loader::LoaderId;
use crate::shared::Shared;
use crate::thread::ThreadState;
use crate::value::Value;

/// The `java.util.logging.Logger` class.
const LOGGER_CLASS: &str = "java/util/logging/Logger";

/// The `java.util.logging.LogRecord` class.
const LOG_RECORD_CLASS: &str = "java/util/logging/LogRecord";

/// The `java.util.logging.Level` class.
const LEVEL_CLASS: &str = "java/util/logging/Level";

/// The default level of a logger whose name has no configuration, matching JUL's root
/// logger default of `INFO`.
const DEFAULT_LEVEL: LevelKind = LevelKind::Info;

/// The state of one `LogRecord` whose constructor or setters have run.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LogRecordData {
    pub level: Option<LevelKind>,
    pub message: String,
    pub logger_name: String,
    pub thrown: Option<ObjRef>,
}

/// The `java.util.logging` side state owned by [`Shared`].
#[derive(Debug, Default)]
pub struct JulState {
    /// One `Logger` per requested name, so `getLogger` returns the same handle.
    pub loggers: HashMap<String, ObjRef>,
    /// The fields of every live `LogRecord`, keyed by its heap handle.
    pub records: HashMap<ObjRef, LogRecordData>,
}

/// Register the minimal `java.util.logging` natives.
pub fn register_natives(shared: &Shared) {
    shared.register_native(
        LOGGER_CLASS,
        "getLogger",
        "(Ljava/lang/String;)Ljava/util/logging/Logger;",
        logger_get_logger,
    );
    shared.register_native(
        LOGGER_CLASS,
        "getLogger",
        "(Ljava/lang/String;Ljava/lang/String;)Ljava/util/logging/Logger;",
        logger_get_logger,
    );
    shared.register_native(
        LOGGER_CLASS,
        "isLoggable",
        "(Ljava/util/logging/Level;)Z",
        logger_is_loggable,
    );
    shared.register_native(
        LOGGER_CLASS,
        "log",
        "(Ljava/util/logging/LogRecord;)V",
        logger_log_record,
    );
    shared.register_native(
        LOGGER_CLASS,
        "log",
        "(Ljava/util/logging/Level;Ljava/lang/String;)V",
        logger_log_text,
    );
    shared.register_native(
        LOGGER_CLASS,
        "getName",
        "()Ljava/lang/String;",
        logger_get_name,
    );
    shared.register_native(
        LOGGER_CLASS,
        "getResourceBundleName",
        "()Ljava/lang/String;",
        logger_get_resource_bundle_name,
    );
    shared.register_native(
        LOGGER_CLASS,
        "getResourceBundle",
        "()Ljava/util/ResourceBundle;",
        logger_get_resource_bundle,
    );

    shared.register_native(
        LOG_RECORD_CLASS,
        "<init>",
        "(Ljava/util/logging/Level;Ljava/lang/String;)V",
        log_record_init,
    );
    shared.register_native(
        LOG_RECORD_CLASS,
        "setLoggerName",
        "(Ljava/lang/String;)V",
        log_record_set_logger_name,
    );
    shared.register_native(
        LOG_RECORD_CLASS,
        "setResourceBundleName",
        "(Ljava/lang/String;)V",
        log_record_set_resource_bundle_name,
    );
    shared.register_native(
        LOG_RECORD_CLASS,
        "setResourceBundle",
        "(Ljava/util/ResourceBundle;)V",
        log_record_set_resource_bundle,
    );
    shared.register_native(
        LOG_RECORD_CLASS,
        "setThrown",
        "(Ljava/lang/Throwable;)V",
        log_record_set_thrown,
    );
    shared.register_native(
        LOG_RECORD_CLASS,
        "getMessage",
        "()Ljava/lang/String;",
        log_record_get_message,
    );
    shared.register_native(
        LOG_RECORD_CLASS,
        "getLoggerName",
        "()Ljava/lang/String;",
        log_record_get_logger_name,
    );
}

/// Write the nine `Level` constants into `java.util.logging.Level`'s statics.
///
/// The C1 `Level` stub has no initializer `Code`, so `Level.INFO` and friends would read
/// as absent. One [`LevelKind`] heap object per constant is allocated and stored under the
/// field's Java name.
pub fn install_levels(shared: &Shared) -> Result<(), VmError> {
    let level_class = shared.load_library_class(LEVEL_CLASS)?;
    let mut values = Vec::with_capacity(LevelKind::ALL.len());
    for kind in LevelKind::ALL {
        let object = shared.heap().allocate_log_level(kind);
        values.push((
            kind.name().to_owned(),
            format!("L{LEVEL_CLASS};"),
            Value::Ref(object),
        ));
    }
    let entry = shared
        .entry(level_class)
        .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", level_class.0)))?;
    let mut statics = entry.statics.lock().expect("static field lock poisoned");
    for (name, descriptor, value) in values {
        statics.insert((name, descriptor), value);
    }
    Ok(())
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
        return Ok("null".to_owned());
    }
    let text = shared
        .heap()
        .string(reference)
        .map_err(heap_error)?
        .to_owned();
    Ok(text)
}

fn reference_argument(
    args: &[Value],
    position: usize,
    method: &'static str,
) -> Result<ObjRef, VmError> {
    match args.get(position).copied() {
        Some(Value::Ref(reference)) => Ok(reference),
        _ => Err(VmError::Native(format!(
            "{method} expects a reference argument"
        ))),
    }
}

/// Read a `Level` argument, which must be one of the installed constants.
fn level_argument(
    shared: &Shared,
    args: &[Value],
    method: &'static str,
) -> Result<LevelKind, VmError> {
    let reference = reference_argument(args, 0, method)?;
    if reference.is_null() {
        return Ok(DEFAULT_LEVEL);
    }
    let heap = shared.heap();
    match heap.get(reference).map_err(heap_error)? {
        HeapObject::LogLevel(kind) => Ok(*kind),
        _ => Err(VmError::Native(format!(
            "{method} expects a java.util.logging.Level argument"
        ))),
    }
}

/// `Logger.getLogger(String)`: the cached logger for `name`, allocated on first use.
fn logger_get_logger(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let name = string_argument(shared, args, 0, "Logger.getLogger")?;
    if let Some(reference) = shared.jul().loggers.get(&name).copied() {
        return Ok(Some(Value::Ref(reference)));
    }
    let class = shared.load_library_class(LOGGER_CLASS)?;
    let reference = shared.allocate_instance(class)?;
    shared.jul().loggers.insert(name, reference);
    Ok(Some(Value::Ref(reference)))
}

/// `Logger.isLoggable(Level)`: the logger's default `INFO` level against the argument.
fn logger_is_loggable(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let level = level_argument(shared, args, "Logger.isLoggable")?;
    let enabled = level != LevelKind::Off && level.value() >= DEFAULT_LEVEL.value();
    Ok(Some(Value::Int(i32::from(enabled))))
}

/// `Logger.log(LogRecord)`: format the record to the captured stderr.
fn logger_log_record(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let record = reference_argument(args, 0, "Logger.log")?;
    let data = shared
        .jul()
        .records
        .get(&record)
        .cloned()
        .unwrap_or_default();
    let level = data.level.unwrap_or(DEFAULT_LEVEL);
    if level == LevelKind::Off || level.value() < DEFAULT_LEVEL.value() {
        return Ok(None);
    }
    let logger = if data.logger_name.is_empty() {
        "<unnamed>".to_owned()
    } else {
        data.logger_name.clone()
    };
    let mut line = format!("{} {}: {}\n", level.name(), logger, data.message);
    if let Some(thrown) = data.thrown {
        if !thrown.is_null() {
            let class = shared.get_class(thrown)?;
            let name = {
                let heap = shared.heap();
                match heap.class(class) {
                    Ok(id) => shared
                        .entry(id)
                        .map(|entry| entry.name.replace('/', "."))
                        .unwrap_or_else(|| "<throwable>".to_owned()),
                    Err(_) => "<throwable>".to_owned(),
                }
            };
            line.push_str(&format!("  thrown: {name}\n"));
        }
    }
    shared.write_stderr(line.as_bytes());
    Ok(None)
}

/// `Logger.log(Level, String)`: the convenience form Spring does not use, kept so a
/// direct JUL caller gets a line rather than a missing-method error.
fn logger_log_text(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let level = level_argument(shared, args, "Logger.log")?;
    let message = string_argument(shared, args, 1, "Logger.log")?;
    if level == LevelKind::Off || level.value() < DEFAULT_LEVEL.value() {
        return Ok(None);
    }
    shared.write_stderr(format!("{}: {message}\n", level.name()).as_bytes());
    Ok(None)
}

/// `Logger.getName()`: the name the logger was requested under.
fn logger_get_name(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let name = shared
        .jul()
        .loggers
        .iter()
        .find(|(_, logger)| **logger == receiver)
        .map(|(name, _)| name.clone())
        .unwrap_or_default();
    let text = shared.intern_string(&name);
    Ok(Some(Value::Ref(text)))
}

fn logger_get_resource_bundle_name(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Ref(ObjRef::NULL)))
}

fn logger_get_resource_bundle(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Ref(ObjRef::NULL)))
}

/// `LogRecord.<init>(Level, String)`.
fn log_record_init(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let level = level_argument(shared, args, "LogRecord.<init>")?;
    let message = string_argument(shared, args, 1, "LogRecord.<init>")?;
    let mut jul = shared.jul();
    jul.records.insert(
        receiver,
        LogRecordData {
            level: Some(level),
            message,
            logger_name: String::new(),
            thrown: None,
        },
    );
    Ok(None)
}

fn log_record_set_logger_name(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let name = string_argument(shared, args, 0, "LogRecord.setLoggerName")?;
    if let Some(record) = shared.jul().records.get_mut(&receiver) {
        record.logger_name = name;
    }
    Ok(None)
}

fn log_record_set_resource_bundle_name(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(None)
}

fn log_record_set_resource_bundle(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(None)
}

fn log_record_set_thrown(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let thrown = reference_argument(args, 0, "LogRecord.setThrown")?;
    let mut jul = shared.jul();
    let record = jul.records.entry(receiver).or_default();
    record.thrown = if thrown.is_null() { None } else { Some(thrown) };
    Ok(None)
}

fn log_record_get_message(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let message = shared
        .jul()
        .records
        .get(&receiver)
        .map(|record| record.message.clone())
        .unwrap_or_default();
    let text = shared.intern_string(&message);
    Ok(Some(Value::Ref(text)))
}

fn log_record_get_logger_name(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let name = shared
        .jul()
        .records
        .get(&receiver)
        .map(|record| record.logger_name.clone())
        .unwrap_or_default();
    let text = shared.intern_string(&name);
    Ok(Some(Value::Ref(text)))
}
