//! `java.util.Random` and `java.security.SecureRandom` natives (M4.5).
//!
//! Spring Boot's `RandomValuePropertySource` constructs a `SecureRandom` while it adds the
//! `random.*` property source to the environment. The object is only read when a
//! `random.*` property is requested, but the constructor must exist for the environment
//! to initialize.
//!
//! The C1 stubs declare no fields, so each generator's state is a 48-bit word in the
//! [`Shared::random_state`] side table keyed by [`ObjRef`]. `Random` runs the JDK's linear
//! congruential generator, so a fixed seed reproduces the JDK's sequence. `SecureRandom`
//! shares that stream: there is no entropy source and no algorithm name, which is a
//! documented boundary (not a cryptographic generator).

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

/// `java.util.Random`'s 48-bit linear congruential generator. The constants and the seed
/// scramble are the JDK's, so a fixed seed produces the JDK's sequence.
const MULTIPLIER: u64 = 0x0005_DEEC_E66D;
const ADDEND: u64 = 0xB;
const MASK: u64 = (1 << 48) - 1;

fn set_seed(state: &mut u64, seed: i64) {
    *state = ((seed as u64) ^ MULTIPLIER) & MASK;
}

/// `Random.next(bits)`: one LCG step and the high `bits` bits of the new state.
fn next_bits(state: &mut u64, bits: u32) -> u32 {
    *state = state.wrapping_mul(MULTIPLIER).wrapping_add(ADDEND) & MASK;
    (*state >> (48 - bits)) as u32
}

fn state_mut<'a>(
    state: &'a mut RandomState,
    receiver: ObjRef,
    method: &str,
) -> Result<&'a mut u64, VmError> {
    state
        .states
        .get_mut(&receiver)
        .ok_or_else(|| VmError::Native(format!("{method}: Random has no state")))
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
    let mut state = shared.random_state();
    let slot = state.states.entry(receiver).or_insert(0);
    // The clock and the receiver identity uniquify two unseeded generators.
    set_seed(slot, seed as i64);
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
    let mut state = shared.random_state();
    let slot = state.states.entry(receiver).or_insert(0);
    set_seed(slot, seed as i64);
    Ok(None)
}

fn next_u32(shared: &Shared, receiver: ObjRef, method: &str) -> Result<u32, VmError> {
    let mut state = shared.random_state();
    let slot = state_mut(&mut state, receiver, method)?;
    Ok(next_bits(slot, 32))
}

fn next_u64(shared: &Shared, receiver: ObjRef, method: &str) -> Result<u64, VmError> {
    let mut state = shared.random_state();
    let slot = state_mut(&mut state, receiver, method)?;
    // `Random.nextLong`: `((long) next(32) << 32) + next(32)`.
    let high = u64::from(next_bits(slot, 32));
    let low = u64::from(next_bits(slot, 32));
    Ok((high << 32) + low)
}

fn random_next_int(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Int(
        next_u32(shared, receiver, "Random.nextInt")? as i32,
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
        Some(Value::Int(value)) if value > 0 => value as u32,
        _ => {
            return Err(VmError::Native(
                "Random.nextInt(bound) expects a positive bound".to_owned(),
            ));
        }
    };
    let mut state = shared.random_state();
    let slot = state_mut(&mut state, receiver, "Random.nextInt")?;
    // `Random.nextInt(int)`: reject the first value when it falls in the incomplete bucket,
    // and scale a power-of-two bound. The `i32` arithmetic deliberately wraps like Java's.
    let mut r = next_bits(slot, 31) as i32;
    let m = bound as i32 - 1;
    if bound & (bound - 1) == 0 {
        r = (((bound as u64) * (r as u32 as u64)) >> 31) as i32;
    } else {
        loop {
            let u = r;
            r = (u as u32 % bound) as i32;
            if u.wrapping_sub(r).wrapping_add(m) >= 0 {
                break;
            }
            r = next_bits(slot, 31) as i32;
        }
    }
    Ok(Some(Value::Int(r)))
}

fn random_next_long(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Long(
        next_u64(shared, receiver, "Random.nextLong")? as i64,
    )))
}

fn random_next_boolean(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let mut state = shared.random_state();
    let slot = state_mut(&mut state, receiver, "Random.nextBoolean")?;
    Ok(Some(Value::Int(i32::from(next_bits(slot, 1) != 0))))
}

fn random_next_float(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let mut state = shared.random_state();
    let slot = state_mut(&mut state, receiver, "Random.nextFloat")?;
    let bits = next_bits(slot, 24);
    Ok(Some(Value::Float(bits as f32 / (1u32 << 24) as f32)))
}

fn random_next_double(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let mut state = shared.random_state();
    let slot = state_mut(&mut state, receiver, "Random.nextDouble")?;
    // `(((long) next(26) << 27) + next(27)) * 0x1.0p-53`.
    let high = u64::from(next_bits(slot, 26));
    let low = u64::from(next_bits(slot, 27));
    Ok(Some(Value::Double(
        (((high << 27) + low) as f64) * (1.0 / (1u64 << 53) as f64),
    )))
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
        for _ in 0..length {
            bytes.push(next_bits(word, 8) as u8);
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
    for _ in 0..count {
        bytes.push(next_bits(word, 8) as u8);
    }
    bytes.truncate(count);
    Ok(Some(Value::Ref(crate::resources::allocate_bytes(
        shared, &bytes,
    )?)))
}
