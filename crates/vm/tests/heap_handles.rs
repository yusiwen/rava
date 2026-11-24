//! M0.4 acceptance: heap handles, field layout and per-thread state.

use std::sync::Arc;

use rava_vm::{
    ArrayKind, ClassId, ClassRef, DirectorySource, FieldLayout, Heap, HeapError, ObjRef,
    PrimitiveKind, Shared, Value,
};

fn empty_shared() -> Shared {
    let dir = std::env::temp_dir().join(format!("rava-m04-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    Shared::with_bootstrap(Arc::new(DirectorySource::new(dir)))
}

#[test]
fn instance_fields_start_with_jvm_defaults() {
    let mut layout = FieldLayout::new();
    layout.push("count", "I");
    layout.push("next", "Ljava/lang/Object;");
    layout.push("total", "J");

    let mut heap = Heap::new();
    let reference = heap.allocate_instance(ClassId(7), &layout);
    let instance = heap.instance(reference).unwrap();

    assert_eq!(instance.class, ClassId(7));
    assert_eq!(instance.fields[0], Value::Int(0));
    assert_eq!(instance.fields[1], Value::Ref(ObjRef::NULL));
    assert_eq!(instance.fields[2], Value::Long(0));
}

#[test]
fn field_layout_keeps_the_superclass_prefix() {
    let mut parent = FieldLayout::new();
    parent.push("id", "I");
    let mut child = FieldLayout::with_parent(&parent);
    child.push("name", "Ljava/lang/String;");

    assert_eq!(child.len(), 2);
    assert_eq!(child.slot("id", "I"), Some(0));
    assert_eq!(child.slot("name", "Ljava/lang/String;"), Some(1));
    assert_eq!(child.slots()[0].name, "id");
}

#[test]
fn freeing_a_slot_invalidates_the_old_handle() {
    let layout = FieldLayout::new();
    let mut heap = Heap::new();
    let first = heap.allocate_instance(ClassId(1), &layout);
    heap.free(first).unwrap();
    assert!(matches!(
        heap.get(first),
        Err(HeapError::StaleReference { .. })
    ));

    let second = heap.allocate_instance(ClassId(2), &layout);
    assert_ne!(first, second);
    assert_eq!(heap.instance(second).unwrap().class, ClassId(2));
    assert!(heap.get(first).is_err());
}

#[test]
fn arrays_use_their_kind_defaults_and_are_distinct_from_instances() {
    let mut heap = Heap::new();
    let reference = heap.allocate_array(ClassRef::Primitive(PrimitiveKind::Int), 3);
    let array = heap.array(reference).unwrap();
    assert_eq!(array.kind, ArrayKind::Int);
    assert_eq!(array.elements, vec![Value::Int(0); 3]);
    assert!(matches!(
        heap.instance(reference),
        Err(HeapError::WrongObjectKind { .. })
    ));

    let array = heap.array_mut(reference).unwrap();
    array.elements[1] = Value::Int(42);
    assert_eq!(heap.array(reference).unwrap().elements[1], Value::Int(42));
}

#[test]
fn null_and_out_of_range_handles_are_rejected() {
    let heap = Heap::new();
    assert!(matches!(
        heap.get(ObjRef::NULL),
        Err(HeapError::NullReference)
    ));
    let bogus = ObjRef::new(99, 0);
    assert!(matches!(
        heap.get(bogus),
        Err(HeapError::IndexOutOfBounds { .. })
    ));
}

#[test]
fn shared_owns_a_lockable_heap_and_separate_thread_state() {
    let shared = empty_shared();
    let layout = FieldLayout::new();
    let reference = {
        let mut heap = shared.heap();
        heap.allocate_instance(ClassId(1), &layout)
    };
    assert_eq!(shared.heap().live_count(), 1);
    assert_eq!(shared.heap().instance(reference).unwrap().class, ClassId(1));

    let thread = shared.thread_state(42);
    assert_eq!(thread.id(), 42);
    assert!(!thread.poll_safepoint());
    thread.request_safepoint();
    assert!(thread.poll_safepoint());
    assert!(!thread.poll_safepoint());
}
