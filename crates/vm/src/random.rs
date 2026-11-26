//! `java.util.Random` and `java.security.SecureRandom` natives (M4.5).
//!
//! Spring Boot's `RandomValuePropertySource` constructs a `SecureRandom` while it adds the
//! `random.*` property source to the environment. The object is only read when a
//! `random.*` property is requested, but the constructor must exist for the environment
//! to initialize.
//!
//! The C1 stubs declare no fields, so each generator's state is a `u64` in the
//! [`Shared::random_state`] side table keyed by [`ObjRef`]. The generator is an
//! xorshift64* sequence seeded from the clock and the object's identity hash. It is not a
//! cryptographic generator: a `SecureRandom` here has the same observable stream shape as
//! `Random`, which is a documented M4 boundary (no entropy source, no algorithm name).

use std::collections::HashMap;

use crate::VmError;
use crate::heap::ObjRef;
use crate::loader::LoaderId;
use crate::shared::Shared;
use crate::thread::ThreadState;
use crate::value::Value;

const RANDOM_CLASS: &str = "java/util/Random";
const SECURE_RANDOM_CLASS: &str = "java/security/SecureRandom";

/// The state held for each generator object.
#[derive(Debug, Default)]
pub struct RandomState {
    states: HashMap<ObjRef, u64>,
}

impl RandomState {
    /// The objects whose generator state the side table holds.
    pub fn references(&self) -> Vec<ObjRef> {
        self.states.keys().copied().collect()
    }
}

pub fn register_natives(shared: &Shared) {
    for class in [RANDOM_CLASS, SECURE_RANDOM_CLASS] {
        shared.register_native(class, "<init>", "()V", random_init);
        shared.register_native(class, "<init>", "(J)V", random_init_seed);
        shared.register_native(class, "nextInt", "()I", random_next_int);
        shared.register_native(class, "nextInt", "(I)I", random_next_int_bound);
        shared.register_native(class, "nextLong", "()J", random_next_long);
        shared.register_native(class, "nextBoolean", "()Z", random_next_boolean);
        shared.register_native(class, "nextFloat", "()F", random_next_float);
        shared.register_native(class, "nextDouble", "()D", random_next_double);
        shared.register_native(class, "nextBytes", "([B)V", random_next_bytes);
        shared.register_native(class, "setSeed", "(J)V", random_init_seed);
        shared.register_native(class, "generateSeed", "(I)[B", random_generate_seed);
    }
}

fn heap_error(error: crate::HeapError) -> VmError {
    VmError::Native(error.to_string())
}

/// One xorshift64* step.
fn next_word(state: &mut u64) -> u64 {
    let mut value = *state;
    value ^= value >> 12;
    value ^= value << 25;
    value ^= value >> 27;
    *state = value;
    value.wrapping_mul(0x2545_F491_4F6C_DD1D)
}

fn seeded(receiver: ObjRef, seed: u64) -> u64 {
    if seed == 0 {
        // xorshift cannot leave the all-zero state.
        (receiver.identity_hash() as u64) | 1
    } else {
        seed
    }
}

fn random_init(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0x9E37_79B9_7F4A_7C15)
        ^ (receiver.identity_hash() as u64);
    shared
        .random_state()
        .states
        .insert(receiver, seeded(receiver, seed));
    Ok(None)
}

fn random_init_seed(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let seed = match args.first().copied() {
        Some(Value::Long(value)) => value as u64,
        Some(Value::Int(value)) => value as u64,
        _ => {
            return Err(VmError::Native("Random.setSeed expects a long".to_owned()));
        }
    };
    shared
        .random_state()
        .states
        .insert(receiver, seeded(receiver, seed));
    Ok(None)
}

fn step(shared: &Shared, receiver: ObjRef, method: &str) -> Result<u64, VmError> {
    let mut state = shared.random_state();
    let value = state
        .states
        .get_mut(&receiver)
        .ok_or_else(|| VmError::Native(format!("{method}: Random has no state")))?;
    Ok(next_word(value))
}

fn random_next_int(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Int(
        step(shared, receiver, "Random.nextInt")? as i32
    )))
}

fn random_next_int_bound(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let bound = match args.first().copied() {
        Some(Value::Int(value)) if value > 0 => value as u64,
        _ => {
            return Err(VmError::Native(
                "Random.nextInt(bound) expects a positive bound".to_owned(),
            ));
        }
    };
    Ok(Some(Value::Int(
        (step(shared, receiver, "Random.nextInt")? % bound) as i32,
    )))
}

fn random_next_long(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Long(
        step(shared, receiver, "Random.nextLong")? as i64,
    )))
}

fn random_next_boolean(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Int(i32::from(
        step(shared, receiver, "Random.nextBoolean")? & 1 == 1,
    ))))
}

fn random_next_float(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let bits = (step(shared, receiver, "Random.nextFloat")? >> 40) as u32;
    Ok(Some(Value::Float(bits as f32 / (1u32 << 24) as f32)))
}

fn random_next_double(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let bits = step(shared, receiver, "Random.nextDouble")? >> 11;
    Ok(Some(Value::Double(bits as f64 / (1u64 << 53) as f64)))
}

fn random_next_bytes(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let Some(Value::Ref(array)) = args.first().copied() else {
        return Err(VmError::Native(
            "Random.nextBytes expects a byte[]".to_owned(),
        ));
    };
    let length = {
        let heap = shared.heap();
        heap.array(array).map_err(heap_error)?.elements.len()
    };
    let mut bytes = Vec::with_capacity(length);
    {
        let mut state = shared.random_state();
        let word = state
            .states
            .get_mut(&receiver)
            .ok_or_else(|| VmError::Native("Random.nextBytes: Random has no state".to_owned()))?;
        while bytes.len() < length {
            bytes.extend_from_slice(&next_word(word).to_le_bytes());
        }
    }
    bytes.truncate(length);
    let mut heap = shared.heap();
    let slots = heap.array_mut(array).map_err(heap_error)?;
    for (slot, byte) in slots.elements.iter_mut().zip(bytes) {
        *slot = Value::Int(i32::from(byte as i8));
    }
    Ok(None)
}

fn random_generate_seed(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let count = match args.first().copied() {
        Some(Value::Int(value)) if value >= 0 => value as usize,
        _ => {
            return Err(VmError::Native(
                "SecureRandom.generateSeed expects a non-negative count".to_owned(),
            ));
        }
    };
    let mut state = shared.random_state();
    let word = state
        .states
        .get_mut(&receiver)
        .ok_or_else(|| VmError::Native("SecureRandom has no state".to_owned()))?;
    let mut bytes = Vec::with_capacity(count);
    while bytes.len() < count {
        bytes.extend_from_slice(&next_word(word).to_le_bytes());
    }
    bytes.truncate(count);
    Ok(Some(Value::Ref(crate::resources::allocate_bytes(
        shared, &bytes,
    )?)))
}
