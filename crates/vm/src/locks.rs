//! `java.util.concurrent.locks` natives (M2.23).
//!
//! Spring 6.2 replaced `synchronized` with explicit `ReentrantLock`/`Condition` in its
//! application-context lifecycle: `AbstractApplicationContext` holds a
//! `startupShutdownLock` and uses `Condition` for lifecycle waits. The VM is single-threaded
//! until M5, so a lock never blocks. It still tracks ownership and reentrancy, so the
//! observable query methods (`isLocked`, `isHeldByCurrentThread`, `getHoldCount`) answer
//! as the JDK does, and an unlock without a matching lock is a typed error.
//!
//! `Condition.await` returns immediately (a spurious wakeup is legal); `signal`/`signalAll`
//! are recorded as no-ops because there is no thread to wake. `tryLock` succeeds unless the
//! lock is already held by this thread, matching a reentrant acquisition.

use crate::VmError;
use crate::heap::{LockState, ObjRef};
use crate::loader::LoaderId;
use crate::shared::Shared;
use crate::thread::ThreadState;
use crate::value::Value;

pub const REENTRANT_LOCK_CLASS: &str = "java/util/concurrent/locks/ReentrantLock";
pub const CONDITION_CLASS: &str = "java/util/concurrent/locks/Condition";
pub const COUNT_DOWN_LATCH_CLASS: &str = "java/util/concurrent/CountDownLatch";

pub fn register_natives(shared: &Shared) {
    shared.register_native(REENTRANT_LOCK_CLASS, "<init>", "()V", lock_init);
    shared.register_native(REENTRANT_LOCK_CLASS, "<init>", "(Z)V", lock_init);
    shared.register_native(REENTRANT_LOCK_CLASS, "lock", "()V", lock_lock);
    shared.register_native(REENTRANT_LOCK_CLASS, "lockInterruptibly", "()V", lock_lock);
    shared.register_native(REENTRANT_LOCK_CLASS, "unlock", "()V", lock_unlock);
    shared.register_native(REENTRANT_LOCK_CLASS, "tryLock", "()Z", lock_try_lock);
    shared.register_native(
        REENTRANT_LOCK_CLASS,
        "tryLock",
        "(JLjava/util/concurrent/TimeUnit;)Z",
        lock_try_lock,
    );
    shared.register_native(REENTRANT_LOCK_CLASS, "isLocked", "()Z", lock_is_locked);
    shared.register_native(
        REENTRANT_LOCK_CLASS,
        "isHeldByCurrentThread",
        "()Z",
        lock_is_held_by_current_thread,
    );
    shared.register_native(
        REENTRANT_LOCK_CLASS,
        "getHoldCount",
        "()I",
        lock_get_hold_count,
    );
    shared.register_native(
        REENTRANT_LOCK_CLASS,
        "newCondition",
        "()Ljava/util/concurrent/locks/Condition;",
        lock_new_condition,
    );

    shared.register_native(CONDITION_CLASS, "await", "()V", condition_await);
    shared.register_native(
        CONDITION_CLASS,
        "await",
        "(JLjava/util/concurrent/TimeUnit;)Z",
        condition_await_timed,
    );
    shared.register_native(
        CONDITION_CLASS,
        "awaitUninterruptibly",
        "()V",
        condition_await,
    );
    shared.register_native(CONDITION_CLASS, "awaitNanos", "(J)J", condition_await_nanos);
    shared.register_native(CONDITION_CLASS, "signal", "()V", condition_signal);
    shared.register_native(CONDITION_CLASS, "signalAll", "()V", condition_signal);

    shared.register_native(COUNT_DOWN_LATCH_CLASS, "<init>", "(I)V", latch_init);
    shared.register_native(COUNT_DOWN_LATCH_CLASS, "countDown", "()V", latch_count_down);
    shared.register_native(COUNT_DOWN_LATCH_CLASS, "getCount", "()J", latch_get_count);
    shared.register_native(COUNT_DOWN_LATCH_CLASS, "await", "()V", latch_await);
    shared.register_native(
        COUNT_DOWN_LATCH_CLASS,
        "await",
        "(JLjava/util/concurrent/TimeUnit;)Z",
        latch_await_timed,
    );
}

fn lock_init(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    shared.set_lock_state(receiver, LockState::default());
    Ok(None)
}

fn lock_lock(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    shared.update_lock(receiver, |state| {
        state.held = true;
        state.count += 1;
    });
    Ok(None)
}

fn lock_unlock(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let released = shared.update_lock(receiver, |state| {
        if state.count == 0 {
            return false;
        }
        state.count -= 1;
        state.held = state.count > 0;
        true
    });
    if !released {
        return Err(VmError::Native(
            "ReentrantLock.unlock without a matching lock".to_owned(),
        ));
    }
    Ok(None)
}

fn lock_try_lock(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    shared.update_lock(receiver, |state| {
        state.held = true;
        state.count += 1;
    });
    Ok(Some(Value::Int(1)))
}

fn lock_is_locked(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let locked = shared.lock_state(receiver).ok_or_else(|| {
        VmError::Native("ReentrantLock.isLocked receiver is not a lock".to_owned())
    })?;
    Ok(Some(Value::Int(i32::from(locked.held))))
}

fn lock_is_held_by_current_thread(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let held = shared.lock_state(receiver).ok_or_else(|| {
        VmError::Native("ReentrantLock.isHeldByCurrentThread receiver is not a lock".to_owned())
    })?;
    Ok(Some(Value::Int(i32::from(held.held))))
}

fn lock_get_hold_count(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let count = shared
        .lock_state(receiver)
        .ok_or_else(|| {
            VmError::Native("ReentrantLock.getHoldCount receiver is not a lock".to_owned())
        })?
        .count;
    let count = i32::try_from(count).unwrap_or(i32::MAX);
    Ok(Some(Value::Int(count)))
}

fn lock_new_condition(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let condition = shared.heap().allocate_condition(receiver);
    Ok(Some(Value::Ref(condition)))
}

fn condition_await(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(None)
}

fn condition_await_timed(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Int(0)))
}

fn condition_await_nanos(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Long(0)))
}

fn condition_signal(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(None)
}

// ---------------------------------------------------------------------------
// CountDownLatch
// ---------------------------------------------------------------------------

fn latch_init(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let count = match args.first().copied() {
        Some(Value::Int(value)) if value >= 0 => value as usize,
        Some(Value::Int(value)) => {
            return Err(VmError::Native(format!(
                "CountDownLatch.<init>: negative count {value}"
            )));
        }
        _ => {
            return Err(VmError::Native(
                "CountDownLatch.<init> expects an int count".to_owned(),
            ));
        }
    };
    shared.latch_state().insert(receiver, count);
    Ok(None)
}

fn latch_count_down(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let mut latches = shared.latch_state();
    let count = latches
        .get_mut(&receiver)
        .ok_or_else(|| VmError::Native("CountDownLatch has no count".to_owned()))?;
    *count = count.saturating_sub(1);
    Ok(None)
}

fn latch_get_count(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let count = *shared
        .latch_state()
        .get(&receiver)
        .ok_or_else(|| VmError::Native("CountDownLatch has no count".to_owned()))?;
    let count = i64::try_from(count)
        .map_err(|_| VmError::Native("CountDownLatch count exceeds a long".to_owned()))?;
    Ok(Some(Value::Long(count)))
}

/// `CountDownLatch.await()`: return only when the count is already zero.
///
/// A positive count means the caller would block for another thread, which the VM cannot
/// provide until M5. Reporting that as a typed error is honest: silently returning would
/// let a caller proceed as if the awaited work had happened.
fn latch_await(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let count = *shared
        .latch_state()
        .get(&receiver)
        .ok_or_else(|| VmError::Native("CountDownLatch has no count".to_owned()))?;
    if count > 0 {
        return Err(VmError::Native(format!(
            "CountDownLatch.await would block with count {count}; the VM is single-threaded until M5"
        )));
    }
    Ok(None)
}

fn latch_await_timed(
    shared: &Shared,
    thread: &ThreadState,
    loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    latch_await(shared, thread, loader, receiver, &[])?;
    Ok(Some(Value::Int(1)))
}
