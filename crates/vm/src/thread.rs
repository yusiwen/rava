//! Per-thread state, the safepoint handshake and the frame-root stack.
//!
//! `Shared` holds no thread-local state. The collector asks a thread to stop by
//! setting its safepoint flag; the interpreter polls it at instruction boundaries.
//! Every active Java frame owns a [`FrameRootsGuard`], so a thread can report the
//! references held by all of its frames, including nested `invokestatic` frames.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::heap::ObjRef;

#[derive(Debug)]
struct FrameRoots {
    id: u64,
    roots: Vec<ObjRef>,
}

#[derive(Debug)]
pub struct ThreadState {
    id: u64,
    requests: AtomicU64,
    handled: AtomicU64,
    polls: AtomicU64,
    next_frame: AtomicU64,
    frames: Mutex<Vec<FrameRoots>>,
}

impl ThreadState {
    pub fn new(id: u64) -> Self {
        Self {
            id,
            requests: AtomicU64::new(0),
            handled: AtomicU64::new(0),
            polls: AtomicU64::new(0),
            next_frame: AtomicU64::new(0),
            frames: Mutex::new(Vec::new()),
        }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    /// Request a safepoint. Requests are counted rather than latched in a bool, so a
    /// request that arrives while the previous one is being published is not lost.
    pub fn request_safepoint(&self) {
        self.requests.fetch_add(1, Ordering::SeqCst);
    }

    /// Is a safepoint requested but not yet acknowledged?
    pub fn safepoint_requested(&self) -> bool {
        self.requests.load(Ordering::SeqCst) != self.handled.load(Ordering::SeqCst)
    }

    /// Count one instruction-boundary poll.
    pub fn note_poll(&self) {
        self.polls.fetch_add(1, Ordering::Relaxed);
    }

    /// Poll one instruction boundary. When a request is pending, `publish` runs
    /// before the request is acknowledged, so a collector can never observe the
    /// acknowledgement with the previous epoch's root snapshot. Returns true when a
    /// request was handled.
    pub fn poll_with(&self, publish: impl FnOnce()) -> bool {
        self.note_poll();
        let request = self.requests.load(Ordering::SeqCst);
        if request == self.handled.load(Ordering::SeqCst) {
            return false;
        }
        publish();
        // Acknowledge only the request observed before publishing. A newer request
        // (requests > handled) stays pending for the next poll.
        self.handled.fetch_max(request, Ordering::SeqCst);
        true
    }

    /// Poll at an instruction boundary without publishing a root snapshot. Returns
    /// true once per request. The interpreter uses [`Self::poll_with`] instead, so
    /// the snapshot is published before the request is acknowledged.
    pub fn poll_safepoint(&self) -> bool {
        self.poll_with(|| {})
    }

    pub fn polls(&self) -> u64 {
        self.polls.load(Ordering::Relaxed)
    }

    /// Register a new frame's root slot. The interpreter calls this on method entry
    /// and drops the returned guard when the frame returns, including on an error
    /// path (RAII), so a nested frame never leaves its roots behind.
    pub fn enter_frame(&self) -> FrameRootsGuard<'_> {
        let frame = self.next_frame.fetch_add(1, Ordering::Relaxed);
        self.frames
            .lock()
            .expect("frame root lock poisoned")
            .push(FrameRoots {
                id: frame,
                roots: Vec::new(),
            });
        FrameRootsGuard {
            thread: self,
            frame,
        }
    }

    /// The roots published by every active frame, outermost first. Null handles are
    /// never registered; duplicates across frames are preserved.
    pub fn frame_roots(&self) -> Vec<ObjRef> {
        self.frames
            .lock()
            .expect("frame root lock poisoned")
            .iter()
            .flat_map(|frame| frame.roots.iter().copied())
            .collect()
    }

    /// Number of active frames: one per registered [`FrameRootsGuard`].
    pub fn frame_depth(&self) -> usize {
        self.frames.lock().expect("frame root lock poisoned").len()
    }
}

/// The root slot of one active frame.
///
/// The guard removes the slot when dropped. A frame publishes a fresh snapshot of
/// its roots at each safepoint request and before every Java or native call.
#[derive(Debug)]
pub struct FrameRootsGuard<'a> {
    thread: &'a ThreadState,
    frame: u64,
}

impl FrameRootsGuard<'_> {
    /// Replace this frame's published roots with `roots`, skipping null handles.
    pub fn publish(&self, roots: &[ObjRef]) {
        let mut frames = self.thread.frames.lock().expect("frame root lock poisoned");
        let Some(slot) = frames.iter_mut().find(|slot| slot.id == self.frame) else {
            return;
        };
        slot.roots.clear();
        slot.roots
            .extend(roots.iter().copied().filter(|root| !root.is_null()));
    }
}

impl Drop for FrameRootsGuard<'_> {
    fn drop(&mut self) {
        let mut frames = self.thread.frames.lock().expect("frame root lock poisoned");
        if let Some(position) = frames.iter().position(|slot| slot.id == self.frame) {
            frames.remove(position);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safepoint_is_reported_once_per_request() {
        let thread = ThreadState::new(1);
        assert!(!thread.poll_safepoint());
        thread.request_safepoint();
        assert!(thread.poll_safepoint());
        assert!(!thread.poll_safepoint());
        assert_eq!(thread.polls(), 3);
    }

    #[test]
    fn frame_guards_form_a_nested_root_stack() {
        let thread = ThreadState::new(2);
        let outer = thread.enter_frame();
        outer.publish(&[ObjRef::new(1, 0), ObjRef::NULL]);
        let inner = thread.enter_frame();
        inner.publish(&[ObjRef::new(2, 0)]);

        assert_eq!(thread.frame_depth(), 2);
        assert_eq!(
            thread.frame_roots(),
            vec![ObjRef::new(1, 0), ObjRef::new(2, 0)]
        );

        drop(inner);
        assert_eq!(thread.frame_roots(), vec![ObjRef::new(1, 0)]);
        drop(outer);
        assert!(thread.frame_roots().is_empty());
    }
}
