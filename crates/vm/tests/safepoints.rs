//! M0.6 acceptance: safepoint root publication, GC roots, the no-lock invariant,
//! the memory-region seam and the reference/class-unloading hooks.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use rava_classfile::ClassBuilder;
use rava_vm::{
    ClassId, DirectorySource, LoaderId, LockHeld, MemoryError, MemoryRegion, ObjRef, Shared,
    SharedLock, ThreadState, Value,
};

const ACC_PUBLIC_STATIC: u16 = 0x0009;

fn empty_shared() -> Shared {
    let dir = std::env::temp_dir().join(format!("rava-m06-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    Shared::with_bootstrap(Arc::new(DirectorySource::new(dir)))
}

fn allocate_string(shared: &Shared, text: &str) -> ObjRef {
    shared.heap().allocate_string(text)
}

#[test]
fn a_safepoint_publishes_roots_before_acknowledging_the_request() {
    let thread = ThreadState::new(9);
    thread.request_safepoint();

    let mut pending_while_publishing = false;
    let mut published = false;
    let handled = thread.poll_with(|| {
        pending_while_publishing = thread.safepoint_requested();
        published = true;
    });

    assert!(handled);
    assert!(published);
    assert!(
        pending_while_publishing,
        "the request must stay pending until the new root snapshot is published"
    );
    assert!(!thread.safepoint_requested());

    let mut idle_publish = false;
    assert!(!thread.poll_with(|| idle_publish = true));
    assert!(!idle_publish, "an idle poll must not publish");
    assert_eq!(thread.polls(), 2);
}

#[test]
fn a_request_that_arrives_during_publication_stays_pending() {
    let thread = ThreadState::new(10);
    thread.request_safepoint();

    let handled = thread.poll_with(|| {
        thread.request_safepoint();
    });

    assert!(handled);
    assert!(
        thread.safepoint_requested(),
        "a newer request must not be cleared by the older acknowledgement"
    );
    assert!(thread.poll_safepoint());
    assert!(!thread.safepoint_requested());
}

#[test]
fn nested_frames_register_and_remove_their_roots() {
    let shared = empty_shared();
    let first = allocate_string(&shared, "first");
    let second = allocate_string(&shared, "second");
    let thread = shared.thread_state(7);

    let outer = thread.enter_frame();
    outer.publish(&[first, ObjRef::NULL]);
    assert_eq!(thread.frame_depth(), 1);
    assert_eq!(thread.frame_roots(), vec![first]);

    let inner = thread.enter_frame();
    inner.publish(&[second]);
    assert_eq!(thread.frame_depth(), 2);
    assert_eq!(thread.frame_roots(), vec![first, second]);

    let roots = shared.roots(&[&thread]);
    assert_eq!(roots, vec![first, second]);

    drop(inner);
    assert_eq!(thread.frame_depth(), 1);
    assert_eq!(thread.frame_roots(), vec![first]);
    drop(outer);
    assert_eq!(thread.frame_depth(), 0);
    assert!(thread.frame_roots().is_empty());
    assert!(shared.roots(&[&thread]).is_empty());
}

#[test]
fn shared_roots_include_static_fields() {
    let shared = empty_shared();
    assert!(shared.roots(&[]).is_empty());

    let mut builder = ClassBuilder::new("Holder", "java/lang/Object", 52);
    builder.field(ACC_PUBLIC_STATIC, "value", "Ljava/lang/String;");
    builder.field(ACC_PUBLIC_STATIC, "alias", "Ljava/lang/String;");
    builder.field(ACC_PUBLIC_STATIC, "count", "I");
    let bytes = builder.build();
    let class = shared
        .define_class(LoaderId::BOOTSTRAP, "Holder", &bytes)
        .unwrap();

    let object = allocate_string(&shared, "static");
    {
        let entry = shared.entry(class).unwrap();
        let mut statics = entry.statics.lock().unwrap();
        statics.insert(
            ("value".to_owned(), "Ljava/lang/String;".to_owned()),
            Value::Ref(object),
        );
        statics.insert(
            ("alias".to_owned(), "Ljava/lang/String;".to_owned()),
            Value::Ref(object),
        );
        statics.insert(("count".to_owned(), "I".to_owned()), Value::Int(3));
    }

    assert_eq!(shared.roots(&[]), vec![object]);
}

#[test]
fn shared_roots_deduplicate_repeated_thread_roots() {
    let shared = empty_shared();
    let object = allocate_string(&shared, "shared");
    let thread = shared.thread_state(3);

    let guard = thread.enter_frame();
    guard.publish(&[object, object]);
    assert_eq!(shared.roots(&[&thread]), vec![object]);
    drop(guard);
    assert!(shared.roots(&[&thread]).is_empty());
}

#[test]
fn the_lock_probe_reports_a_held_heap_lock_until_it_is_dropped() {
    let shared = empty_shared();
    assert!(shared.check_no_locks_held().is_ok());

    {
        let _heap = shared.heap();
        assert_eq!(
            shared.check_no_locks_held(),
            Err(LockHeld {
                lock: SharedLock::Heap
            })
        );
    }

    // Dropping the guard makes the probe pass, and the probe itself leaves no lock
    // behind: the heap can be taken again immediately.
    assert!(shared.check_no_locks_held().is_ok());
    let mut heap = shared.heap();
    assert_eq!(heap.live_count(), 0);
    assert!(!heap.allocate_string("after-probe").is_null());
}

#[test]
fn a_probe_failure_names_the_held_lock() {
    let shared = empty_shared();
    let _heap = shared.heap();
    let error = shared.check_no_locks_held().unwrap_err();
    assert_eq!(error.lock.to_string(), "heap");
}

#[test]
fn memory_regions_are_vm_owned_and_bounds_checked() {
    let mut region = MemoryRegion::allocate(16);
    assert_eq!(region.len(), 16);
    assert!(!region.is_empty());
    assert_ne!(region.address(), 0);
    assert_eq!(region.address() % 8, 0);
    assert_eq!(region.as_slice(), &[0u8; 16][..]);

    region.write(4, &[1, 2, 3, 4]).unwrap();
    assert_eq!(region.read(4, 4).unwrap(), &[1u8, 2, 3, 4][..]);
    region.fill(0, 4, 0xaa).unwrap();
    assert_eq!(region.read(0, 4).unwrap(), &[0xaa; 4][..]);

    assert!(matches!(
        region.read(14, 4),
        Err(MemoryError::OutOfBounds { .. })
    ));
    assert!(matches!(
        region.write(16, &[0]),
        Err(MemoryError::OutOfBounds { .. })
    ));
    assert!(matches!(
        region.fill(usize::MAX, 2, 0),
        Err(MemoryError::OutOfBounds { .. })
    ));

    let other = MemoryRegion::allocate(8);
    assert_ne!(region.address(), other.address());

    let mapped = MemoryRegion::at(0x2000, vec![7u8, 8]);
    assert_eq!(mapped.address(), 0x2000);
    assert_eq!(mapped.read(0, 2).unwrap(), &[7u8, 8][..]);
}

#[test]
fn reference_and_class_unload_hooks_are_registration_points_only() {
    let shared = empty_shared();
    let cleared = Arc::new(Mutex::new(Vec::new()));
    let unloaded = Arc::new(Mutex::new(Vec::new()));
    let clears = Arc::new(AtomicUsize::new(0));
    let unloads = Arc::new(AtomicUsize::new(0));

    let seen_cleared = Arc::clone(&cleared);
    let clears_hook = Arc::clone(&clears);
    shared.hooks().register_reference(move |reference| {
        seen_cleared.lock().unwrap().push(reference);
        clears_hook.fetch_add(1, Ordering::SeqCst);
    });
    let seen_unloaded = Arc::clone(&unloaded);
    let unloads_hook = Arc::clone(&unloads);
    shared.hooks().register_class_unload(move |class| {
        seen_unloaded.lock().unwrap().push(class);
        unloads_hook.fetch_add(1, Ordering::SeqCst);
    });

    assert_eq!(shared.hooks().reference_hook_count(), 1);
    assert_eq!(shared.hooks().class_unload_hook_count(), 1);

    let reference = allocate_string(&shared, "weak");
    shared.hooks().reference_cleared(reference);
    shared.hooks().class_unloaded(ClassId(5));

    assert_eq!(*cleared.lock().unwrap(), vec![reference]);
    assert_eq!(*unloaded.lock().unwrap(), vec![ClassId(5)]);
    assert_eq!(clears.load(Ordering::SeqCst), 1);
    assert_eq!(unloads.load(Ordering::SeqCst), 1);
}
