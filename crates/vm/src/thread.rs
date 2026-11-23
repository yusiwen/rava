//! Per-thread state and the safepoint handshake.
//!
//! `Shared` holds no thread-local state. The collector asks a thread to stop by
//! setting its safepoint flag; the interpreter polls it at instruction boundaries.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

#[derive(Debug)]
pub struct ThreadState {
    id: u64,
    safepoint_requested: AtomicBool,
    polls: AtomicU64,
}

impl ThreadState {
    pub fn new(id: u64) -> Self {
        Self {
            id,
            safepoint_requested: AtomicBool::new(false),
            polls: AtomicU64::new(0),
        }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn request_safepoint(&self) {
        self.safepoint_requested.store(true, Ordering::SeqCst);
    }

    /// Poll at an instruction boundary. Returns true once per request.
    pub fn poll_safepoint(&self) -> bool {
        self.polls.fetch_add(1, Ordering::Relaxed);
        self.safepoint_requested.swap(false, Ordering::SeqCst)
    }

    pub fn polls(&self) -> u64 {
        self.polls.load(Ordering::Relaxed)
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
}
