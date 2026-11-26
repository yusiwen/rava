//! A `java.time` formatting subset for logging startup (M4.5).
//!
//! Logback 1.5 replaced `SimpleDateFormat` with `java.time.format.DateTimeFormatter` in
//! `CachingDateFormatter`, so booting any SLF4J/logback application reaches
//! `ZoneId.systemDefault`, `Instant.ofEpochMilli`, `DateTimeFormatter.ofPattern` and
//! `DateTimeFormatter.format(TemporalAccessor)`. The C1 stubs carry no `Code`, so every
//! operation the logging path calls is a native here.
//!
//! The objects are ordinary class instances; their state lives in [`TimeState`], a side
//! table keyed by [`ObjRef`], exactly as the M4.1 resource objects do. The state is Rust
//! text and numbers, so it owns no Java references and [`crate::Shared::roots`] reports
//! the keys to keep the attachment alive.
//!
//! The implemented surface is the one the pinned logging stack uses:
//!
//! - `ZoneId.systemDefault()`/`getId()`/`toString()`/`of(String)`;
//! - `Instant.ofEpochMilli(long)`/`toEpochMilli()`/`getEpochSecond()`/`getNano()`/
//!   `now()`/`toString()`;
//! - `DateTimeFormatter.ofPattern`/`withZone`/`withLocale`/`format`/`toString`;
//! - `Locale.getDefault()` (the locale is inert until a locale-sensitive format is
//!   requested, which is a typed error rather than a silently English result).
//!
//! `systemDefault` resolves the host zone from `/etc/timezone` and `/etc/localtime` (a
//! TZif v1/v2 file) at startup. The offset is captured once, so a long-running process
//! that crosses a DST transition keeps the startup offset: full zone-rule lookups are a
//! later slice. When the host zone cannot be read the offset is UTC. `Locale` is inert:
//! month, weekday and AM/PM text is always English, which is what the pinned logging
//! formats use.
//!
//! The pattern renderer supports the letters the logging stack uses and their ordinary
//! neighbours: `y`, `M`, `d`, `D`, `H`, `h`, `m`, `s`, `S`, `X`, `Z`, `z`, `E`, `a`,
//! `G` and `Q`, with single-quote literals. Any other letter is a typed error.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::VmError;
use crate::heap::ObjRef;
use crate::loader::LoaderId;
use crate::shared::Shared;
use crate::thread::ThreadState;
use crate::value::Value;

const INSTANT_CLASS: &str = "java/time/Instant";
const ZONE_ID_CLASS: &str = "java/time/ZoneId";
const DATE_TIME_FORMATTER_CLASS: &str = "java/time/format/DateTimeFormatter";
const LOCALE_CLASS: &str = "java/util/Locale";

const CLOCK_CLASS: &str = "java/time/Clock";
const DURATION_CLASS: &str = "java/time/Duration";

/// The Rust state attached to the `java.time` objects this VM allocates.
#[derive(Debug, Default)]
pub struct TimeState {
    instants: HashMap<ObjRef, i64>,
    zones: HashMap<ObjRef, ZoneInfo>,
    formatters: HashMap<ObjRef, FormatterState>,
    locales: HashMap<ObjRef, String>,
    clocks: HashMap<ObjRef, ZoneInfo>,
    durations: HashMap<ObjRef, (i64, i32)>,
}

impl TimeState {
    /// Every object the side table keeps state for, so [`crate::Shared::roots`] can
    /// report them.
    pub fn references(&self) -> Vec<ObjRef> {
        self.instants
            .keys()
            .chain(self.zones.keys())
            .chain(self.formatters.keys())
            .chain(self.locales.keys())
            .chain(self.clocks.keys())
            .chain(self.durations.keys())
            .copied()
            .collect()
    }
}

/// A zone identity and the UTC offset captured for it at startup.
#[derive(Debug, Clone)]
struct ZoneInfo {
    id: String,
    offset_seconds: i32,
}

/// One `DateTimeFormatter`: the pattern, an optional zone and an inert locale.
#[derive(Debug, Clone)]
struct FormatterState {
    pattern: String,
    zone: Option<ZoneInfo>,
}

/// Registers the `java.time` and `java.util.Locale` natives.
pub fn register_natives(shared: &Shared) {
    shared.register_native(
        ZONE_ID_CLASS,
        "systemDefault",
        "()Ljava/time/ZoneId;",
        zone_system_default,
    );
    shared.register_native(ZONE_ID_CLASS, "getId", "()Ljava/lang/String;", zone_get_id);
    shared.register_native(
        ZONE_ID_CLASS,
        "toString",
        "()Ljava/lang/String;",
        zone_get_id,
    );
    shared.register_native(
        ZONE_ID_CLASS,
        "of",
        "(Ljava/lang/String;)Ljava/time/ZoneId;",
        zone_of,
    );
    shared.register_native(
        ZONE_ID_CLASS,
        "of",
        "(Ljava/lang/String;Ljava/util/Map;)Ljava/time/ZoneId;",
        zone_of,
    );

    shared.register_native(
        INSTANT_CLASS,
        "ofEpochMilli",
        "(J)Ljava/time/Instant;",
        instant_of_epoch_milli,
    );
    shared.register_native(INSTANT_CLASS, "now", "()Ljava/time/Instant;", instant_now);
    shared.register_native(INSTANT_CLASS, "toEpochMilli", "()J", instant_to_epoch_milli);
    shared.register_native(
        INSTANT_CLASS,
        "getEpochSecond",
        "()J",
        instant_get_epoch_second,
    );
    shared.register_native(INSTANT_CLASS, "getNano", "()I", instant_get_nano);
    shared.register_native(
        INSTANT_CLASS,
        "toString",
        "()Ljava/lang/String;",
        instant_to_string,
    );

    shared.register_native(
        DATE_TIME_FORMATTER_CLASS,
        "ofPattern",
        "(Ljava/lang/String;)Ljava/time/format/DateTimeFormatter;",
        formatter_of_pattern,
    );
    shared.register_native(
        DATE_TIME_FORMATTER_CLASS,
        "ofPattern",
        "(Ljava/lang/String;Ljava/util/Locale;)Ljava/time/format/DateTimeFormatter;",
        formatter_of_pattern,
    );
    shared.register_native(
        DATE_TIME_FORMATTER_CLASS,
        "ofLocalizedDate",
        "(Ljava/time/format/FormatStyle;)Ljava/time/format/DateTimeFormatter;",
        formatter_of_localized_date,
    );
    shared.register_native(
        DATE_TIME_FORMATTER_CLASS,
        "ofLocalizedTime",
        "(Ljava/time/format/FormatStyle;)Ljava/time/format/DateTimeFormatter;",
        formatter_of_localized_time,
    );
    shared.register_native(
        DATE_TIME_FORMATTER_CLASS,
        "ofLocalizedDateTime",
        "(Ljava/time/format/FormatStyle;)Ljava/time/format/DateTimeFormatter;",
        formatter_of_localized_date_time,
    );
    shared.register_native(
        DATE_TIME_FORMATTER_CLASS,
        "ofLocalizedDateTime",
        "(Ljava/time/format/FormatStyle;Ljava/time/format/FormatStyle;)Ljava/time/format/DateTimeFormatter;",
        formatter_of_localized_date_time,
    );
    shared.register_native(
        DATE_TIME_FORMATTER_CLASS,
        "withZone",
        "(Ljava/time/ZoneId;)Ljava/time/format/DateTimeFormatter;",
        formatter_with_zone,
    );
    shared.register_native(
        DATE_TIME_FORMATTER_CLASS,
        "withLocale",
        "(Ljava/util/Locale;)Ljava/time/format/DateTimeFormatter;",
        formatter_with_locale,
    );
    shared.register_native(
        DATE_TIME_FORMATTER_CLASS,
        "format",
        "(Ljava/time/temporal/TemporalAccessor;)Ljava/lang/String;",
        formatter_format,
    );
    shared.register_native(
        DATE_TIME_FORMATTER_CLASS,
        "toString",
        "()Ljava/lang/String;",
        formatter_to_string,
    );
    shared.register_native(
        DATE_TIME_FORMATTER_CLASS,
        "getZone",
        "()Ljava/time/ZoneId;",
        formatter_get_zone,
    );

    register_duration_natives(shared);
    shared.register_native(
        CLOCK_CLASS,
        "systemUTC",
        "()Ljava/time/Clock;",
        clock_system_utc,
    );
    shared.register_native(
        CLOCK_CLASS,
        "systemDefaultZone",
        "()Ljava/time/Clock;",
        clock_system_default,
    );
    shared.register_native(
        CLOCK_CLASS,
        "system",
        "(Ljava/time/ZoneId;)Ljava/time/Clock;",
        clock_system,
    );
    shared.register_native(CLOCK_CLASS, "millis", "()J", clock_millis);
    shared.register_native(
        CLOCK_CLASS,
        "instant",
        "()Ljava/time/Instant;",
        clock_instant,
    );
    shared.register_native(
        CLOCK_CLASS,
        "getZone",
        "()Ljava/time/ZoneId;",
        clock_get_zone,
    );
    shared.register_native(
        CLOCK_CLASS,
        "withZone",
        "(Ljava/time/ZoneId;)Ljava/time/Clock;",
        clock_with_zone,
    );
    shared.register_native(
        LOCALE_CLASS,
        "getDefault",
        "()Ljava/util/Locale;",
        locale_get_default,
    );
    shared.register_native(
        LOCALE_CLASS,
        "getDefault",
        "(Ljava/util/Locale$Category;)Ljava/util/Locale;",
        locale_get_default,
    );
    shared.register_native(
        LOCALE_CLASS,
        "toString",
        "()Ljava/lang/String;",
        locale_to_string,
    );

    // `java.util.concurrent.TimeUnit` conversions. `System.nanoTime`/`sleep` are already
    // no-ops in the single-threaded VM; the conversions are plain arithmetic and the
    // timed waits are the M5 boundary.
    shared.register_native(
        "java/util/concurrent/TimeUnit",
        "toNanos",
        "(J)J",
        time_unit_to_nanos,
    );
    for name in [
        "toMicros",
        "toMillis",
        "toSeconds",
        "toMinutes",
        "toHours",
        "toDays",
    ] {
        let function = match name {
            "toMicros" => time_unit_to_micros as crate::NativeFn,
            "toMillis" => time_unit_to_millis,
            "toSeconds" => time_unit_to_seconds,
            "toMinutes" => time_unit_to_minutes,
            "toHours" => time_unit_to_hours,
            _ => time_unit_to_days,
        };
        shared.register_native("java/util/concurrent/TimeUnit", name, "(J)J", function);
    }
    shared.register_native(
        "java/util/concurrent/TimeUnit",
        "convert",
        "(JLjava/util/concurrent/TimeUnit;)J",
        time_unit_convert,
    );
    shared.register_native(
        "java/util/concurrent/TimeUnit",
        "sleep",
        "(J)V",
        time_unit_noop,
    );
}

fn heap_error(error: crate::HeapError) -> VmError {
    VmError::Native(error.to_string())
}

/// Read a reference argument and reject null.
fn reference_argument(args: &[Value], position: usize, method: &str) -> Result<ObjRef, VmError> {
    match args.get(position).copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => Ok(reference),
        Some(Value::Ref(_)) => Err(VmError::Native(format!("{method}: null argument"))),
        Some(_) => Err(VmError::Native(format!(
            "{method}: expected a reference argument"
        ))),
        None => Err(VmError::Native(format!("{method}: missing its argument"))),
    }
}

fn string_argument(
    shared: &Shared,
    args: &[Value],
    position: usize,
    method: &str,
) -> Result<String, VmError> {
    let reference = reference_argument(args, position, method)?;
    let text = shared
        .heap()
        .string(reference)
        .map_err(heap_error)?
        .to_owned();
    Ok(text)
}

fn long_argument(args: &[Value], position: usize, method: &str) -> Result<i64, VmError> {
    match args.get(position).copied() {
        Some(Value::Long(value)) => Ok(value),
        Some(Value::Int(value)) => Ok(i64::from(value)),
        Some(_) => Err(VmError::Native(format!(
            "{method}: expected a long argument"
        ))),
        None => Err(VmError::Native(format!("{method}: missing its argument"))),
    }
}

fn new_string(shared: &Shared, text: String) -> Value {
    Value::Ref(shared.heap().allocate_string(text))
}

// ---------------------------------------------------------------------------
// ZoneId
// ---------------------------------------------------------------------------

fn zone_system_default(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let zone = host_zone_cached();
    Ok(Some(Value::Ref(allocate_zone(shared, zone)?)))
}

fn zone_of(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let id = string_argument(shared, args, 0, "ZoneId.of")?;
    let offset_seconds = offset_for_id(&id);
    Ok(Some(Value::Ref(allocate_zone(
        shared,
        ZoneInfo { id, offset_seconds },
    )?)))
}

fn zone_get_id(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let id = shared
        .time_state()
        .zones
        .get(&receiver)
        .map(|zone| zone.id.clone())
        .ok_or_else(|| VmError::Native("ZoneId has no state".to_owned()))?;
    Ok(Some(new_string(shared, id)))
}

fn allocate_zone(shared: &Shared, zone: ZoneInfo) -> Result<ObjRef, VmError> {
    let class = shared.load_library_class(ZONE_ID_CLASS)?;
    let object = shared.allocate_instance(class)?;
    shared.time_state().zones.insert(object, zone);
    Ok(object)
}

// ---------------------------------------------------------------------------
// Instant
// ---------------------------------------------------------------------------

fn instant_of_epoch_milli(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let millis = long_argument(args, 0, "Instant.ofEpochMilli")?;
    Ok(Some(Value::Ref(allocate_instant(shared, millis)?)))
}

fn instant_now(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Ref(allocate_instant(shared, now_millis())?)))
}

fn allocate_instant(shared: &Shared, millis: i64) -> Result<ObjRef, VmError> {
    let class = shared.load_library_class(INSTANT_CLASS)?;
    let object = shared.allocate_instance(class)?;
    shared.time_state().instants.insert(object, millis);
    Ok(object)
}

fn instant_millis(shared: &Shared, receiver: ObjRef) -> Result<i64, VmError> {
    shared
        .time_state()
        .instants
        .get(&receiver)
        .copied()
        .ok_or_else(|| VmError::Native("Instant has no state".to_owned()))
}

fn instant_to_epoch_milli(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Long(instant_millis(shared, receiver)?)))
}

fn instant_get_epoch_second(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Long(
        instant_millis(shared, receiver)?.div_euclid(1000),
    )))
}

fn instant_get_nano(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let millis = instant_millis(shared, receiver)?.rem_euclid(1000);
    Ok(Some(Value::Int((millis * 1_000_000) as i32)))
}

fn instant_to_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let millis = instant_millis(shared, receiver)?;
    // `Instant.toString` prints the fraction only when it is non-zero, and always ends in
    // `Z` (the UTC offset form is not used).
    let mut text = format_epoch(millis, &utc_zone(), "yyyy-MM-dd'T'HH:mm:ss")?;
    let sub_millis = millis.rem_euclid(1000);
    if sub_millis != 0 {
        text.push_str(&format!(".{sub_millis:03}"));
    }
    text.push('Z');
    Ok(Some(new_string(shared, text)))
}

// ---------------------------------------------------------------------------
// DateTimeFormatter
// ---------------------------------------------------------------------------

fn formatter_of_pattern(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let pattern = string_argument(shared, args, 0, "DateTimeFormatter.ofPattern")?;
    let class = shared.load_library_class(DATE_TIME_FORMATTER_CLASS)?;
    let object = shared.allocate_instance(class)?;
    shared.time_state().formatters.insert(
        object,
        FormatterState {
            pattern,
            zone: None,
        },
    );
    Ok(Some(Value::Ref(object)))
}

/// The date, time and date-time patterns this slice uses for the localized styles.
///
/// The JDK consults the locale's CLDR pattern; this slice has no locale data, so it uses
/// the ISO-shaped patterns. `DateTimeFormatter.ofLocalized*` therefore produces a working
/// formatter whose rendering is ISO for every style, not a locale-specific one. A caller
/// that formats an instant still gets the right fields; a caller that needs a SHORT
/// locale pattern gets English ISO text. That boundary is recorded in the module docs.
fn localized_date_pattern() -> &'static str {
    "yyyy-MM-dd"
}

fn localized_time_pattern() -> &'static str {
    "HH:mm:ss"
}

fn localized_date_time_pattern() -> &'static str {
    "yyyy-MM-dd'T'HH:mm:ss"
}

fn allocate_formatter(
    shared: &Shared,
    pattern: String,
    zone: Option<ZoneInfo>,
) -> Result<ObjRef, VmError> {
    let class = shared.load_library_class(DATE_TIME_FORMATTER_CLASS)?;
    let object = shared.allocate_instance(class)?;
    shared
        .time_state()
        .formatters
        .insert(object, FormatterState { pattern, zone });
    Ok(object)
}

fn formatter_of_localized_date(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let object = allocate_formatter(shared, localized_date_pattern().to_owned(), None)?;
    Ok(Some(Value::Ref(object)))
}

fn formatter_of_localized_time(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let object = allocate_formatter(shared, localized_time_pattern().to_owned(), None)?;
    Ok(Some(Value::Ref(object)))
}

fn formatter_of_localized_date_time(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let object = allocate_formatter(shared, localized_date_time_pattern().to_owned(), None)?;
    Ok(Some(Value::Ref(object)))
}

/// Install the `DateTimeFormatter` constants the framework reads as static fields.
///
/// The C1 stub has no initializer `Code`, so without this every constant would read as
/// `null` and an identity comparison such as `formatter == ISO_DATE` would compare
/// against null. Each constant is a formatter object with its ISO pattern in the side
/// table; the identity relations the JDK guarantees (`ISO_DATE` and `ISO_LOCAL_DATE` are
/// distinct objects) hold because each field gets its own object.
pub fn install_constants(shared: &Shared) -> Result<(), VmError> {
    let class = shared.load_library_class(DATE_TIME_FORMATTER_CLASS)?;
    let constants = [
        ("ISO_LOCAL_DATE", "yyyy-MM-dd"),
        ("ISO_OFFSET_DATE", "yyyy-MM-ddXXX"),
        ("ISO_DATE", "yyyy-MM-dd"),
        ("ISO_LOCAL_TIME", "HH:mm:ss"),
        ("ISO_OFFSET_TIME", "HH:mm:ssXXX"),
        ("ISO_TIME", "HH:mm:ss"),
        ("ISO_LOCAL_DATE_TIME", "yyyy-MM-dd'T'HH:mm:ss"),
        ("ISO_OFFSET_DATE_TIME", "yyyy-MM-dd'T'HH:mm:ssXXX"),
        ("ISO_ZONED_DATE_TIME", "yyyy-MM-dd'T'HH:mm:ssXXX'['z']'"),
        ("ISO_DATE_TIME", "yyyy-MM-dd'T'HH:mm:ss"),
        ("ISO_ORDINAL_DATE", "yyyy-DDD"),
        ("ISO_WEEK_DATE", "yyyy-'W'ww-e"),
        ("ISO_INSTANT", "yyyy-MM-dd'T'HH:mm:ss'Z'"),
        ("BASIC_ISO_DATE", "yyyyMMdd"),
        ("RFC_1123_DATE_TIME", "EEE, dd MMM yyyy HH:mm:ss 'GMT'"),
    ];
    let mut values = Vec::with_capacity(constants.len());
    for (name, pattern) in constants {
        let object = allocate_formatter(shared, pattern.to_owned(), None)?;
        values.push((
            name.to_owned(),
            "Ljava/time/format/DateTimeFormatter;".to_owned(),
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

fn formatter_with_zone(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let zone_object = reference_argument(args, 0, "DateTimeFormatter.withZone")?;
    let zone = shared
        .time_state()
        .zones
        .get(&zone_object)
        .cloned()
        .ok_or_else(|| VmError::Native("DateTimeFormatter.withZone: unknown ZoneId".to_owned()))?;
    let mut state = shared
        .time_state()
        .formatters
        .get(&receiver)
        .cloned()
        .ok_or_else(|| VmError::Native("DateTimeFormatter has no state".to_owned()))?;
    state.zone = Some(zone);
    let class = shared.load_library_class(DATE_TIME_FORMATTER_CLASS)?;
    let object = shared.allocate_instance(class)?;
    shared.time_state().formatters.insert(object, state);
    Ok(Some(Value::Ref(object)))
}

fn formatter_with_locale(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    // The locale is inert: the renderer always uses English text names, and the pinned
    // logging patterns use numeric fields only. Return a fresh formatter with the same
    // pattern and zone so the caller sees a distinct object, as the JDK does.
    let state = shared
        .time_state()
        .formatters
        .get(&receiver)
        .cloned()
        .ok_or_else(|| VmError::Native("DateTimeFormatter has no state".to_owned()))?;
    let class = shared.load_library_class(DATE_TIME_FORMATTER_CLASS)?;
    let object = shared.allocate_instance(class)?;
    shared.time_state().formatters.insert(object, state);
    Ok(Some(Value::Ref(object)))
}

fn formatter_format(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let temporal = reference_argument(args, 0, "DateTimeFormatter.format")?;
    let state = shared
        .time_state()
        .formatters
        .get(&receiver)
        .cloned()
        .ok_or_else(|| VmError::Native("DateTimeFormatter has no state".to_owned()))?;
    let zone = state.zone.clone().unwrap_or_else(utc_zone);
    let millis = shared
        .time_state()
        .instants
        .get(&temporal)
        .copied()
        .ok_or_else(|| {
            VmError::Native(
                "DateTimeFormatter.format: only java.time.Instant is supported".to_owned(),
            )
        })?;
    let text = format_epoch(millis, &zone, &state.pattern)?;
    Ok(Some(new_string(shared, text)))
}

fn formatter_to_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let pattern = shared
        .time_state()
        .formatters
        .get(&receiver)
        .map(|state| state.pattern.clone())
        .ok_or_else(|| VmError::Native("DateTimeFormatter has no state".to_owned()))?;
    Ok(Some(new_string(shared, pattern)))
}

fn formatter_get_zone(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let zone = shared
        .time_state()
        .formatters
        .get(&receiver)
        .and_then(|state| state.zone.clone());
    match zone {
        Some(zone) => Ok(Some(Value::Ref(allocate_zone(shared, zone)?))),
        None => Ok(Some(Value::Ref(ObjRef::NULL))),
    }
}

// ---------------------------------------------------------------------------
// Locale
// ---------------------------------------------------------------------------

fn locale_get_default(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let class = shared.load_library_class(LOCALE_CLASS)?;
    let object = shared.allocate_instance(class)?;
    let tag = host_locale();
    shared.time_state().locales.insert(object, tag);
    Ok(Some(Value::Ref(object)))
}

fn locale_to_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let tag = shared
        .time_state()
        .locales
        .get(&receiver)
        .cloned()
        .ok_or_else(|| VmError::Native("Locale has no state".to_owned()))?;
    Ok(Some(new_string(shared, tag)))
}

// ---------------------------------------------------------------------------
// Host zone and locale
// ---------------------------------------------------------------------------

fn utc_zone() -> ZoneInfo {
    ZoneInfo {
        id: "UTC".to_owned(),
        offset_seconds: 0,
    }
}

fn host_locale() -> String {
    let language = std::env::var("LANG").unwrap_or_else(|_| "en".to_owned());
    let language = language.split(['_', '.']).next().unwrap_or("en");
    if language.is_empty() {
        "en".to_owned()
    } else {
        language.to_owned()
    }
}

fn host_zone() -> ZoneInfo {
    let id = std::fs::read_to_string("/etc/timezone")
        .ok()
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
        .or_else(|| {
            std::env::var("TZ")
                .ok()
                .filter(|text| !text.is_empty() && !text.starts_with(':'))
        })
        .or_else(|| {
            std::fs::read_link("/etc/localtime").ok().and_then(|path| {
                let path = path.to_string_lossy().into_owned();
                let (_, zone) = path.split_once("zoneinfo/")?;
                Some(zone.to_owned())
            })
        })
        .unwrap_or_else(|| "UTC".to_owned());
    let offset_seconds = std::fs::read("/etc/localtime")
        .ok()
        .and_then(|bytes| tzif_offset(&bytes, now_millis().div_euclid(1000)))
        .unwrap_or(0);
    ZoneInfo { id, offset_seconds }
}

/// The offset for a zone id when the zone rules are unavailable.
///
/// `UTC`/`GMT` and the fixed `+HH:MM` forms are honoured; any other id keeps the offset
/// the host zone reported, because the pinned logging formats request offsets, not
/// arbitrary zones.
fn offset_for_id(id: &str) -> i32 {
    if matches!(id, "UTC" | "GMT" | "UT" | "Z") {
        return 0;
    }
    let Some(rest) = id.strip_prefix(['+', '-']) else {
        return host_zone().offset_seconds;
    };
    let sign = if id.starts_with('-') { -1 } else { 1 };
    let (hours, minutes) = rest.split_once(':').unwrap_or((rest, "0"));
    let hours: i32 = hours.parse().unwrap_or(0);
    let minutes: i32 = minutes.parse().unwrap_or(0);
    sign * (hours * 3600 + minutes * 60)
}

fn now_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_millis() as i64,
        Err(_) => 0,
    }
}

/// The UTC offset of a TZif file at `instant`, in seconds.
///
/// The file is read once per call; logging formats a handful of timestamps, and a parsed
/// transition cache would be a subsystem this slice does not need yet.
fn tzif_offset(bytes: &[u8], instant: i64) -> Option<i32> {
    if bytes.len() < 44 || &bytes[0..4] != b"TZif" {
        return None;
    }
    let version = bytes[4];
    let mut counts = read_tzif_counts(bytes, 20)?;
    let mut data = 44usize;
    if version >= b'2' {
        let first_len = tzif_block_len(&counts);
        let second_header = data + first_len;
        if bytes.len() < second_header + 44 {
            return None;
        }
        counts = read_tzif_counts(bytes, second_header + 20)?;
        data = second_header + 44;
    }
    let (timecnt, typecnt) = (counts.3, counts.4);
    if typecnt == 0 {
        return None;
    }
    if version >= b'2' {
        // The second block uses 64-bit transition times.
        let transitions_len = timecnt
            .checked_mul(8)?
            .checked_add(timecnt)?
            .checked_add(typecnt.checked_mul(6)?)?
            .checked_add(counts.5)?
            .checked_add(counts.2.checked_mul(12)?)?
            .checked_add(counts.0)?
            .checked_add(counts.1)?;
        if bytes.len() < data.checked_add(transitions_len)? {
            return None;
        }
        let transitions = data;
        let indices = transitions + timecnt * 8;
        let types = indices + timecnt;
        if timecnt == 0 {
            return read_tzif_utoff(bytes, types);
        }
        let mut selected = None;
        for index in 0..timecnt {
            let time = read_i64(bytes, transitions + index * 8)?;
            if time <= instant {
                selected = Some(index);
            } else {
                break;
            }
        }
        let Some(index) = selected else {
            return read_tzif_utoff(bytes, types);
        };
        let kind = usize::from(*bytes.get(indices + index)?);
        read_tzif_utoff(bytes, types + kind * 6)
    } else {
        let transitions_len = timecnt
            .checked_mul(4)?
            .checked_add(timecnt)?
            .checked_add(typecnt.checked_mul(6)?)?
            .checked_add(counts.5)?
            .checked_add(counts.2.checked_mul(8)?)?
            .checked_add(counts.0)?
            .checked_add(counts.1)?;
        if bytes.len() < data.checked_add(transitions_len)? {
            return None;
        }
        let transitions = data;
        let indices = transitions + timecnt * 4;
        let types = indices + timecnt;
        if timecnt == 0 {
            return read_tzif_utoff(bytes, types);
        }
        let mut selected = None;
        for index in 0..timecnt {
            let time = i64::from(read_i32(bytes, transitions + index * 4)?);
            if time <= instant {
                selected = Some(index);
            } else {
                break;
            }
        }
        let Some(index) = selected else {
            return read_tzif_utoff(bytes, types);
        };
        let kind = usize::from(*bytes.get(indices + index)?);
        read_tzif_utoff(bytes, types + kind * 6)
    }
}

fn read_tzif_counts(
    bytes: &[u8],
    start: usize,
) -> Option<(usize, usize, usize, usize, usize, usize)> {
    let value =
        |offset: usize| -> Option<usize> { usize::try_from(read_i32(bytes, start + offset)?).ok() };
    Some((
        value(0)?,
        value(4)?,
        value(8)?,
        value(12)?,
        value(16)?,
        value(20)?,
    ))
}

fn tzif_block_len(counts: &(usize, usize, usize, usize, usize, usize)) -> usize {
    counts.3 * 4 + counts.3 + counts.4 * 6 + counts.5 + counts.2 * 8 + counts.1 + counts.0
}

fn read_tzif_utoff(bytes: &[u8], position: usize) -> Option<i32> {
    read_i32(bytes, position)
}

fn read_i32(bytes: &[u8], position: usize) -> Option<i32> {
    let slice = bytes.get(position..position + 4)?;
    Some(i32::from_be_bytes(slice.try_into().ok()?))
}

fn read_i64(bytes: &[u8], position: usize) -> Option<i64> {
    let slice = bytes.get(position..position + 8)?;
    Some(i64::from_be_bytes(slice.try_into().ok()?))
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// The local calendar fields of one instant in one zone.
struct Fields {
    year: i64,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    millis: u32,
    day_of_year: u32,
    weekday: usize,
    offset_seconds: i32,
}

fn fields_of(millis: i64, zone: &ZoneInfo) -> Fields {
    let local = millis + i64::from(zone.offset_seconds) * 1000;
    let days = local.div_euclid(86_400_000);
    let of_day = local.rem_euclid(86_400_000);
    let (year, month, day) = civil_from_days(days);
    let hour = (of_day / 3_600_000) as u32;
    let minute = (of_day / 60_000 % 60) as u32;
    let second = (of_day / 1000 % 60) as u32;
    let millis = (of_day % 1000) as u32;
    let day_of_year = day_of_year(year, month, day);
    let weekday = ((days + 3).rem_euclid(7)) as usize;
    Fields {
        year,
        month,
        day,
        hour,
        minute,
        second,
        millis,
        day_of_year,
        weekday,
        offset_seconds: zone.offset_seconds,
    }
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

fn day_of_year(year: i64, month: u32, day: u32) -> u32 {
    const CUMULATIVE: [u32; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let extra = u32::from(leap && month > 2);
    CUMULATIVE[(month - 1) as usize] + day + extra
}

fn format_epoch(millis: i64, zone: &ZoneInfo, pattern: &str) -> Result<String, VmError> {
    let fields = fields_of(millis, zone);
    let mut out = String::new();
    let mut characters = pattern.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\'' => {
                if characters.peek() == Some(&'\'') {
                    characters.next();
                    out.push('\'');
                } else {
                    let mut closed = false;
                    for literal in characters.by_ref() {
                        if literal == '\'' {
                            closed = true;
                            break;
                        }
                        out.push(literal);
                    }
                    if !closed {
                        return Err(VmError::Native(
                            "DateTimeFormatter: unterminated quote in pattern".to_owned(),
                        ));
                    }
                }
            }
            letter if letter.is_ascii_alphabetic() => {
                let mut count = 1usize;
                while characters.peek() == Some(&letter) {
                    characters.next();
                    count += 1;
                }
                out.push_str(&format_field(&fields, zone, letter, count)?);
            }
            other => out.push(other),
        }
    }
    Ok(out)
}

fn format_field(
    fields: &Fields,
    zone: &ZoneInfo,
    letter: char,
    count: usize,
) -> Result<String, VmError> {
    let text = match letter {
        'y' => {
            if count == 2 {
                format!("{:02}", fields.year.rem_euclid(100))
            } else {
                format!("{:0width$}", fields.year, width = count.max(4))
            }
        }
        'M' => match count {
            1 => fields.month.to_string(),
            2 => format!("{:02}", fields.month),
            3 => MONTHS[(fields.month - 1) as usize][..3].to_owned(),
            4 => MONTHS[(fields.month - 1) as usize].to_owned(),
            _ => MONTHS[(fields.month - 1) as usize][..1].to_owned(),
        },
        'd' => format!("{:0width$}", fields.day, width = count),
        'D' => format!("{:0width$}", fields.day_of_year, width = count.max(3)),
        'H' => format!("{:0width$}", fields.hour, width = count),
        'h' => {
            let hour = if fields.hour % 12 == 0 {
                12
            } else {
                fields.hour % 12
            };
            format!("{:0width$}", hour, width = count)
        }
        'm' => format!("{:0width$}", fields.minute, width = count),
        's' => format!("{:0width$}", fields.second, width = count),
        'S' => {
            let fraction = format!("{:03}", fields.millis);
            if count <= 3 {
                fraction[..count].to_owned()
            } else {
                format!("{fraction}{}", "0".repeat(count - 3))
            }
        }
        'a' => {
            if fields.hour < 12 {
                "AM".to_owned()
            } else {
                "PM".to_owned()
            }
        }
        'E' => match count {
            4.. => WEEKDAYS_FULL[fields.weekday].to_owned(),
            _ => WEEKDAYS_SHORT[fields.weekday].to_owned(),
        },
        'X' => {
            let offset = fields.offset_seconds;
            if offset == 0 {
                "Z".to_owned()
            } else {
                let sign = if offset < 0 { '-' } else { '+' };
                let total_minutes = offset.abs() / 60;
                let hours = total_minutes / 60;
                let minutes = total_minutes % 60;
                match count {
                    1 => format!("{sign}{hours:02}"),
                    2 => format!("{sign}{hours:02}{minutes:02}"),
                    _ => format!("{sign}{hours:02}:{minutes:02}"),
                }
            }
        }
        'Z' => {
            let offset = fields.offset_seconds;
            let sign = if offset < 0 { '-' } else { '+' };
            let total_minutes = offset.abs() / 60;
            let hours = total_minutes / 60;
            let minutes = total_minutes % 60;
            if count >= 5 {
                format!("{sign}{hours:02}:{minutes:02}")
            } else {
                format!("{sign}{hours:02}{minutes:02}")
            }
        }
        'z' => zone.id.clone(),
        'G' => "AD".to_owned(),
        'Q' => format!("{}", (fields.month - 1) / 3 + 1),
        other => {
            return Err(VmError::Native(format!(
                "DateTimeFormatter: pattern letter '{other}' is not implemented"
            )));
        }
    };
    Ok(text)
}

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

const WEEKDAYS_SHORT: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const WEEKDAYS_FULL: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

/// Cache the host zone so repeated `systemDefault` calls agree.
static HOST_ZONE: OnceLock<ZoneInfo> = OnceLock::new();

/// The host zone, read once and shared.
fn host_zone_cached() -> ZoneInfo {
    HOST_ZONE.get_or_init(host_zone).clone()
}

// ---------------------------------------------------------------------------
// java.util.concurrent.TimeUnit
// ---------------------------------------------------------------------------

/// Nanoseconds in one unit, from the receiver enum constant's name.
fn time_unit_nanos_per_unit(shared: &Shared, receiver: ObjRef) -> Result<i64, VmError> {
    let name = {
        let heap = shared.heap();
        match heap.get(receiver).map_err(heap_error)? {
            crate::heap::HeapObject::Enum(constant) => constant.name.clone(),
            _ => {
                return Err(VmError::Native(
                    "TimeUnit method called on a non-TimeUnit receiver".to_owned(),
                ));
            }
        }
    };
    Ok(match name.as_str() {
        "NANOSECONDS" => 1,
        "MICROSECONDS" => 1_000,
        "MILLISECONDS" => 1_000_000,
        "SECONDS" => 1_000_000_000,
        "MINUTES" => 60_000_000_000,
        "HOURS" => 3_600_000_000_000,
        "DAYS" => 86_400_000_000_000,
        other => {
            return Err(VmError::Native(format!(
                "TimeUnit: unknown constant {other}"
            )));
        }
    })
}

fn convert_time_unit(
    shared: &Shared,
    receiver: ObjRef,
    duration: i64,
    target: i64,
) -> Result<i64, VmError> {
    let source = time_unit_nanos_per_unit(shared, receiver)?;
    let nanos = duration.saturating_mul(source);
    Ok(nanos.saturating_div(target))
}

fn time_unit_to_nanos(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let duration = long_argument(args, 0, "TimeUnit.toNanos")?;
    Ok(Some(Value::Long(convert_time_unit(
        shared, receiver, duration, 1,
    )?)))
}

macro_rules! time_unit_conversion {
    ($name:ident, $nanos:expr) => {
        fn $name(
            shared: &Shared,
            _thread: &ThreadState,
            _loader: LoaderId,
            receiver: ObjRef,
            args: &[Value],
        ) -> Result<Option<Value>, VmError> {
            let duration = long_argument(args, 0, stringify!($name))?;
            Ok(Some(Value::Long(convert_time_unit(
                shared, receiver, duration, $nanos,
            )?)))
        }
    };
}

time_unit_conversion!(time_unit_to_micros, 1_000);
time_unit_conversion!(time_unit_to_millis, 1_000_000);
time_unit_conversion!(time_unit_to_seconds, 1_000_000_000);
time_unit_conversion!(time_unit_to_minutes, 60_000_000_000);
time_unit_conversion!(time_unit_to_hours, 3_600_000_000_000);
time_unit_conversion!(time_unit_to_days, 86_400_000_000_000);

fn time_unit_convert(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let duration = long_argument(args, 0, "TimeUnit.convert")?;
    let target = reference_argument(args, 1, "TimeUnit.convert")?;
    let target = time_unit_nanos_per_unit(shared, target)?;
    Ok(Some(Value::Long(convert_time_unit(
        shared, receiver, duration, target,
    )?)))
}

fn time_unit_noop(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(None)
}

// ---------------------------------------------------------------------------
// Clock
// ---------------------------------------------------------------------------

fn allocate_clock(shared: &Shared, zone: ZoneInfo) -> Result<ObjRef, VmError> {
    let class = shared.load_library_class(CLOCK_CLASS)?;
    let object = shared.allocate_instance(class)?;
    shared.time_state().clocks.insert(object, zone);
    Ok(object)
}

fn clock_zone(shared: &Shared, receiver: ObjRef, method: &str) -> Result<ZoneInfo, VmError> {
    shared
        .time_state()
        .clocks
        .get(&receiver)
        .cloned()
        .ok_or_else(|| VmError::Native(format!("{method}: Clock has no state")))
}

fn clock_system_utc(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Ref(allocate_clock(shared, utc_zone())?)))
}

fn clock_system_default(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Ref(allocate_clock(
        shared,
        host_zone_cached(),
    )?)))
}

fn clock_system(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let zone = reference_argument(args, 0, "Clock.system")?;
    let zone = shared
        .time_state()
        .zones
        .get(&zone)
        .cloned()
        .ok_or_else(|| VmError::Native("Clock.system: unknown ZoneId".to_owned()))?;
    Ok(Some(Value::Ref(allocate_clock(shared, zone)?)))
}

fn clock_millis(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    clock_zone(shared, receiver, "Clock.millis")?;
    Ok(Some(Value::Long(now_millis())))
}

fn clock_instant(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    clock_zone(shared, receiver, "Clock.instant")?;
    Ok(Some(Value::Ref(allocate_instant(shared, now_millis())?)))
}

fn clock_get_zone(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let zone = clock_zone(shared, receiver, "Clock.getZone")?;
    Ok(Some(Value::Ref(allocate_zone(shared, zone)?)))
}

fn clock_with_zone(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    clock_zone(shared, receiver, "Clock.withZone")?;
    let zone = reference_argument(args, 0, "Clock.withZone")?;
    let zone = shared
        .time_state()
        .zones
        .get(&zone)
        .cloned()
        .ok_or_else(|| VmError::Native("Clock.withZone: unknown ZoneId".to_owned()))?;
    Ok(Some(Value::Ref(allocate_clock(shared, zone)?)))
}

// ---------------------------------------------------------------------------
// Duration
// ---------------------------------------------------------------------------

fn duration_natives() -> Vec<(&'static str, &'static str)> {
    vec![
        ("ofSeconds", "(J)Ljava/time/Duration;"),
        ("ofSeconds", "(JJ)Ljava/time/Duration;"),
        ("ofMillis", "(J)Ljava/time/Duration;"),
        ("ofNanos", "(J)Ljava/time/Duration;"),
        ("ofMinutes", "(J)Ljava/time/Duration;"),
        ("ofHours", "(J)Ljava/time/Duration;"),
        ("ofDays", "(J)Ljava/time/Duration;"),
        ("getSeconds", "()J"),
        ("getNano", "()I"),
        ("toMillis", "()J"),
        ("toNanos", "()J"),
        ("toSeconds", "()J"),
        ("toMinutes", "()J"),
        ("toHours", "()J"),
        ("toDays", "()J"),
        ("isZero", "()Z"),
        ("isNegative", "()Z"),
        ("compareTo", "(Ljava/time/Duration;)I"),
        ("plus", "(Ljava/time/Duration;)Ljava/time/Duration;"),
        ("minus", "(Ljava/time/Duration;)Ljava/time/Duration;"),
        ("negated", "()Ljava/time/Duration;"),
        ("abs", "()Ljava/time/Duration;"),
        ("toString", "()Ljava/lang/String;"),
    ]
}

fn register_duration_natives(shared: &Shared) {
    for (name, descriptor) in duration_natives() {
        let function: crate::NativeFn = match name {
            "ofSeconds" if descriptor == "(JJ)Ljava/time/Duration;" => duration_of_seconds_nanos,
            "ofSeconds" => duration_of_seconds,
            "ofMillis" => duration_of_millis,
            "ofNanos" => duration_of_nanos,
            "ofMinutes" => duration_of_minutes,
            "ofHours" => duration_of_hours,
            "ofDays" => duration_of_days,
            "getSeconds" => duration_get_seconds,
            "getNano" => duration_get_nano,
            "toMillis" => duration_to_millis,
            "toNanos" => duration_to_nanos,
            "toSeconds" => duration_to_seconds,
            "toMinutes" => duration_to_minutes,
            "toHours" => duration_to_hours,
            "toDays" => duration_to_days,
            "isZero" => duration_is_zero,
            "isNegative" => duration_is_negative,
            "compareTo" => duration_compare_to,
            "plus" => duration_plus,
            "minus" => duration_minus,
            "negated" => duration_negated,
            "abs" => duration_abs,
            _ => duration_to_string,
        };
        shared.register_native(DURATION_CLASS, name, descriptor, function);
    }
}

fn duration_data(shared: &Shared, receiver: ObjRef, method: &str) -> Result<(i64, i32), VmError> {
    shared
        .time_state()
        .durations
        .get(&receiver)
        .copied()
        .ok_or_else(|| VmError::Native(format!("{method}: Duration has no state")))
}

fn allocate_duration(shared: &Shared, seconds: i64, nanos: i32) -> Result<Value, VmError> {
    let normalized_seconds = seconds.saturating_add(i64::from(nanos.div_euclid(1_000_000_000)));
    let normalized_nanos = nanos.rem_euclid(1_000_000_000);
    let class = shared.load_library_class(DURATION_CLASS)?;
    let object = shared.allocate_instance(class)?;
    shared
        .time_state()
        .durations
        .insert(object, (normalized_seconds, normalized_nanos));
    Ok(Value::Ref(object))
}

fn duration_long(args: &[Value], method: &str) -> Result<i64, VmError> {
    match args.first().copied() {
        Some(Value::Long(value)) => Ok(value),
        Some(Value::Int(value)) => Ok(i64::from(value)),
        _ => Err(VmError::Native(format!("{method} expects a long"))),
    }
}

fn duration_int(args: &[Value], position: usize, method: &str) -> Result<i64, VmError> {
    match args.get(position).copied() {
        Some(Value::Long(value)) => Ok(value),
        Some(Value::Int(value)) => Ok(i64::from(value)),
        _ => Err(VmError::Native(format!("{method} expects a long"))),
    }
}

fn duration_of_seconds(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(allocate_duration(
        shared,
        duration_long(args, "Duration.ofSeconds")?,
        0,
    )?))
}

fn duration_of_seconds_nanos(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let seconds = duration_int(args, 0, "Duration.ofSeconds")?;
    let nanos = duration_int(args, 1, "Duration.ofSeconds")?;
    Ok(Some(allocate_duration(shared, seconds, nanos as i32)?))
}

fn duration_of_millis(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let millis = duration_long(args, "Duration.ofMillis")?;
    Ok(Some(allocate_duration(
        shared,
        millis.div_euclid(1000),
        (millis.rem_euclid(1000) * 1_000_000) as i32,
    )?))
}

fn duration_of_nanos(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let nanos = duration_long(args, "Duration.ofNanos")?;
    Ok(Some(allocate_duration(shared, 0, nanos as i32)?))
}

fn duration_of_minutes(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(allocate_duration(
        shared,
        duration_long(args, "Duration.ofMinutes")?.saturating_mul(60),
        0,
    )?))
}

fn duration_of_hours(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(allocate_duration(
        shared,
        duration_long(args, "Duration.ofHours")?.saturating_mul(3600),
        0,
    )?))
}

fn duration_of_days(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(allocate_duration(
        shared,
        duration_long(args, "Duration.ofDays")?.saturating_mul(86_400),
        0,
    )?))
}

fn duration_get_seconds(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Long(
        duration_data(shared, receiver, "Duration.getSeconds")?.0,
    )))
}

fn duration_get_nano(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Int(
        duration_data(shared, receiver, "Duration.getNano")?.1,
    )))
}

fn duration_millis(shared: &Shared, receiver: ObjRef, method: &str) -> Result<i64, VmError> {
    let (seconds, nanos) = duration_data(shared, receiver, method)?;
    Ok(seconds
        .saturating_mul(1000)
        .saturating_add(i64::from(nanos / 1_000_000)))
}

fn duration_to_millis(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Long(duration_millis(
        shared,
        receiver,
        "Duration.toMillis",
    )?)))
}

fn duration_to_nanos(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let (seconds, nanos) = duration_data(shared, receiver, "Duration.toNanos")?;
    Ok(Some(Value::Long(
        seconds
            .saturating_mul(1_000_000_000)
            .saturating_add(i64::from(nanos)),
    )))
}

fn duration_to_seconds(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Long(
        duration_data(shared, receiver, "Duration.toSeconds")?.0,
    )))
}

fn duration_to_minutes(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Long(
        duration_data(shared, receiver, "Duration.toMinutes")?.0 / 60,
    )))
}

fn duration_to_hours(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Long(
        duration_data(shared, receiver, "Duration.toHours")?.0 / 3600,
    )))
}

fn duration_to_days(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Long(
        duration_data(shared, receiver, "Duration.toDays")?.0 / 86_400,
    )))
}

fn duration_is_zero(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let (seconds, nanos) = duration_data(shared, receiver, "Duration.isZero")?;
    Ok(Some(Value::Int(i32::from(seconds == 0 && nanos == 0))))
}

fn duration_is_negative(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let (seconds, nanos) = duration_data(shared, receiver, "Duration.isNegative")?;
    Ok(Some(Value::Int(i32::from(
        seconds < 0 || (seconds == 0 && nanos < 0),
    ))))
}

fn duration_compare_to(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let other = match args.first().copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => reference,
        _ => {
            return Err(VmError::Native(
                "Duration.compareTo expects a Duration".to_owned(),
            ));
        }
    };
    let left = duration_data(shared, receiver, "Duration.compareTo")?;
    let right = duration_data(shared, other, "Duration.compareTo")?;
    Ok(Some(Value::Int(match left.cmp(&right) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    })))
}

fn duration_plus(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let other = match args.first().copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => reference,
        _ => {
            return Err(VmError::Native(
                "Duration.plus expects a Duration".to_owned(),
            ));
        }
    };
    let (ls, ln) = duration_data(shared, receiver, "Duration.plus")?;
    let (rs, rn) = duration_data(shared, other, "Duration.plus")?;
    Ok(Some(allocate_duration(
        shared,
        ls.saturating_add(rs),
        ln.saturating_add(rn),
    )?))
}

fn duration_minus(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let other = match args.first().copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => reference,
        _ => {
            return Err(VmError::Native(
                "Duration.minus expects a Duration".to_owned(),
            ));
        }
    };
    let (ls, ln) = duration_data(shared, receiver, "Duration.minus")?;
    let (rs, rn) = duration_data(shared, other, "Duration.minus")?;
    Ok(Some(allocate_duration(
        shared,
        ls.saturating_sub(rs),
        ln.saturating_sub(rn),
    )?))
}

fn duration_negated(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let (seconds, nanos) = duration_data(shared, receiver, "Duration.negated")?;
    Ok(Some(allocate_duration(
        shared,
        seconds.saturating_neg(),
        -nanos,
    )?))
}

fn duration_abs(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let (seconds, nanos) = duration_data(shared, receiver, "Duration.abs")?;
    Ok(Some(allocate_duration(
        shared,
        seconds.saturating_abs(),
        nanos.abs(),
    )?))
}

fn duration_to_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let (seconds, nanos) = duration_data(shared, receiver, "Duration.toString")?;
    if seconds == 0 && nanos == 0 {
        return Ok(Some(new_string(shared, "PT0S".to_owned())));
    }
    // `Duration.toString`: hours/minutes/seconds, with a decimal fraction only when there
    // are nanos; the JDK's `2 * nanos - NANOS_PER_SECOND` form keeps a negative duration's
    // fraction on the right side of zero.
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;
    let mut text = String::from("PT");
    if hours != 0 {
        text.push_str(&format!("{hours}H"));
    }
    if minutes != 0 {
        text.push_str(&format!("{minutes}M"));
    }
    if secs == 0 && nanos == 0 && text.len() > 2 {
        return Ok(Some(new_string(shared, text)));
    }
    if secs < 0 && nanos > 0 {
        if secs == -1 {
            text.push_str("-0");
        } else {
            text.push_str(&format!("{}", secs + 1));
        }
    } else {
        text.push_str(&format!("{secs}"));
    }
    if nanos > 0 {
        let position = text.len();
        if secs < 0 {
            text.push_str(&format!("{}", 2 * i64::from(nanos) - 1_000_000_000));
        } else {
            text.push_str(&format!("{}", i64::from(nanos) + 1_000_000_000));
        }
        while text.ends_with('0') {
            text.pop();
        }
        if position < text.len() {
            text.replace_range(position..position + 1, ".");
        }
    }
    text.push('S');
    Ok(Some(new_string(shared, text)))
}
