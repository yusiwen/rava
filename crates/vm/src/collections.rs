//! `java.lang.StringBuilder` and `java.util.ArrayList` natives (M2.12).
//!
//! Both classes are VM-level heap objects rather than `HeapObject::Instance`s:
//! [`crate::HeapObject::StringBuilder`] holds the builder's text and
//! [`crate::HeapObject::List`] holds the list's elements. [`crate::Shared::allocate_instance`]
//! recognizes the two class identities, so `new` and reflective construction allocate the
//! right representation without a second class table.
//!
//! The builder stores valid UTF-8 (the same representation `java.lang.String` uses), and
//! every Java-visible operation works on UTF-16 code units through
//! [`crate::string::utf16_units`]. `length`, `charAt`, `setLength`, `deleteCharAt`,
//! `insert`, `reverse` and `append(String)` therefore count and index units, not Rust
//! `char`s, and `reverse` keeps a surrogate pair intact the way the JDK does. An
//! operation that would leave a lone surrogate (for example `setLength(1)` over a
//! surrogate pair) is a typed [`crate::VmError::UnpairedSurrogate`], because the stored
//! text cannot represent one.
//!
//! `contains`, `indexOf` and `remove(Object)` use reference identity, not `Object.equals`:
//! invoking a Java `equals` is not available to a VM native, and a silent value-equality
//! comparison would be wrong for every class that does not override `equals`. The
//! limitation is documented in the slice boundary; `null` matches a null element, as on
//! the JDK.
//!
//! There is no Java exception-object synthesis yet, so a null receiver and a negative
//! capacity are typed [`crate::VmError::Native`] errors, and an out-of-range index is
//! `StringIndexOutOfBounds` for the builder or
//! [`crate::VmError::CollectionIndexOutOfBounds`] for a list.

use crate::VmError;
use crate::heap::{ClassRef, Heap, HeapError, HeapObject, ObjRef};
use crate::loader::LoaderId;
use crate::maps::values_equal;
use crate::shared::Shared;
use crate::string::{double_text, float_text, object_text, string_from_utf16, utf16_units};
use crate::thread::ThreadState;
use crate::value::Value;

/// The internal name of the builder's runtime class.
pub const STRING_BUILDER_CLASS: &str = "java/lang/StringBuilder";

/// The internal name of the list's runtime class.
pub const ARRAY_LIST_CLASS: &str = "java/util/ArrayList";

/// The internal name of the linked list's runtime class.
pub const LINKED_LIST_CLASS: &str = "java/util/LinkedList";

/// The internal name of the copy-on-write list's runtime class.
pub const COPY_ON_WRITE_ARRAY_LIST_CLASS: &str = "java/util/concurrent/CopyOnWriteArrayList";

/// The internal name of the insertion-ordered set's runtime class.
pub const LINKED_HASH_SET_CLASS: &str = "java/util/LinkedHashSet";

/// The internal name of the hash set's runtime class.
pub const HASH_SET_CLASS: &str = "java/util/HashSet";

/// The internal name of the tree set's runtime class.
pub const TREE_SET_CLASS: &str = "java/util/TreeSet";

/// The internal name of the copy-on-write set's runtime class.
pub const COPY_ON_WRITE_ARRAY_SET_CLASS: &str = "java/util/concurrent/CopyOnWriteArraySet";

/// The internal name of the array deque's runtime class.
pub const ARRAY_DEQUE_CLASS: &str = "java/util/ArrayDeque";

/// The internal name of the blocking queue's runtime class.
///
/// It shares the list shape (a FIFO sequence); only its capacity and the blocking
/// operations that M5 owns are new.
pub const LINKED_BLOCKING_QUEUE_CLASS: &str = "java/util/concurrent/LinkedBlockingQueue";

/// The internal name of the legacy stack's runtime class.
///
/// It extends `java.util.Vector`, which the C1 tree does not project, so the VM models the
/// stack as the list shape and supplies the stack's own operations.
pub const STACK_CLASS: &str = "java/util/Stack";

/// The internal name of the concurrent deque's runtime class.
pub const CONCURRENT_LINKED_DEQUE_CLASS: &str = "java/util/concurrent/ConcurrentLinkedDeque";

/// The internal name of the concurrent queue's runtime class.
pub const CONCURRENT_LINKED_QUEUE_CLASS: &str = "java/util/concurrent/ConcurrentLinkedQueue";

/// The internal name of the iterator interface.
pub const ITERATOR_CLASS: &str = "java/util/Iterator";

/// Registers the M2.12 `java.lang.StringBuilder` and `java.util.ArrayList` natives.
///
/// Every entry is keyed by the class that declares the Java method. The C1 stubs carry no
/// `Code` attribute, so the registry is the implementation for `invokevirtual`,
/// `invokespecial` and `invokestatic` alike.
pub fn register_natives(shared: &Shared) {
    // java.lang.StringBuilder
    shared.register_native(STRING_BUILDER_CLASS, "<init>", "()V", string_builder_init);
    shared.register_native(
        STRING_BUILDER_CLASS,
        "<init>",
        "(I)V",
        string_builder_init_capacity,
    );
    shared.register_native(
        STRING_BUILDER_CLASS,
        "<init>",
        "(Ljava/lang/String;)V",
        string_builder_init_string,
    );
    shared.register_native(
        STRING_BUILDER_CLASS,
        "append",
        "(Ljava/lang/String;)Ljava/lang/StringBuilder;",
        string_builder_append_string,
    );
    shared.register_native(
        STRING_BUILDER_CLASS,
        "append",
        "(Ljava/lang/Object;)Ljava/lang/StringBuilder;",
        string_builder_append_object,
    );
    shared.register_native(
        STRING_BUILDER_CLASS,
        "append",
        "(I)Ljava/lang/StringBuilder;",
        string_builder_append_int,
    );
    shared.register_native(
        STRING_BUILDER_CLASS,
        "append",
        "(J)Ljava/lang/StringBuilder;",
        string_builder_append_long,
    );
    shared.register_native(
        STRING_BUILDER_CLASS,
        "append",
        "(C)Ljava/lang/StringBuilder;",
        string_builder_append_char,
    );
    shared.register_native(
        STRING_BUILDER_CLASS,
        "append",
        "(Z)Ljava/lang/StringBuilder;",
        string_builder_append_boolean,
    );
    shared.register_native(
        STRING_BUILDER_CLASS,
        "append",
        "(F)Ljava/lang/StringBuilder;",
        string_builder_append_float,
    );
    shared.register_native(
        STRING_BUILDER_CLASS,
        "append",
        "(D)Ljava/lang/StringBuilder;",
        string_builder_append_double,
    );
    shared.register_native(
        STRING_BUILDER_CLASS,
        "append",
        "(Ljava/lang/CharSequence;)Ljava/lang/StringBuilder;",
        string_builder_append_char_sequence,
    );
    shared.register_native(
        STRING_BUILDER_CLASS,
        "append",
        "(Ljava/lang/CharSequence;II)Ljava/lang/StringBuilder;",
        string_builder_append_char_sequence_range,
    );
    shared.register_native(
        STRING_BUILDER_CLASS,
        "toString",
        "()Ljava/lang/String;",
        string_builder_to_string,
    );
    shared.register_native(STRING_BUILDER_CLASS, "length", "()I", string_builder_length);
    shared.register_native(
        STRING_BUILDER_CLASS,
        "charAt",
        "(I)C",
        string_builder_char_at,
    );
    shared.register_native(
        STRING_BUILDER_CLASS,
        "setLength",
        "(I)V",
        string_builder_set_length,
    );
    shared.register_native(
        STRING_BUILDER_CLASS,
        "reverse",
        "()Ljava/lang/StringBuilder;",
        string_builder_reverse,
    );
    shared.register_native(
        STRING_BUILDER_CLASS,
        "deleteCharAt",
        "(I)Ljava/lang/StringBuilder;",
        string_builder_delete_char_at,
    );
    shared.register_native(
        STRING_BUILDER_CLASS,
        "insert",
        "(ILjava/lang/String;)Ljava/lang/StringBuilder;",
        string_builder_insert_string,
    );

    // java.util.ArrayList
    shared.register_native(ARRAY_LIST_CLASS, "<init>", "()V", array_list_init);
    shared.register_native(ARRAY_LIST_CLASS, "<init>", "(I)V", array_list_init_capacity);
    shared.register_native(ARRAY_LIST_CLASS, "size", "()I", array_list_size);
    shared.register_native(ARRAY_LIST_CLASS, "isEmpty", "()Z", array_list_is_empty);
    shared.register_native(
        ARRAY_LIST_CLASS,
        "add",
        "(Ljava/lang/Object;)Z",
        array_list_add,
    );
    shared.register_native(
        ARRAY_LIST_CLASS,
        "add",
        "(ILjava/lang/Object;)V",
        array_list_add_at,
    );
    shared.register_native(
        ARRAY_LIST_CLASS,
        "get",
        "(I)Ljava/lang/Object;",
        array_list_get,
    );
    shared.register_native(
        ARRAY_LIST_CLASS,
        "set",
        "(ILjava/lang/Object;)Ljava/lang/Object;",
        array_list_set,
    );
    shared.register_native(ARRAY_LIST_CLASS, "clear", "()V", array_list_clear);
    shared.register_native(
        ARRAY_LIST_CLASS,
        "remove",
        "(I)Ljava/lang/Object;",
        array_list_remove_at,
    );
    shared.register_native(
        ARRAY_LIST_CLASS,
        "remove",
        "(Ljava/lang/Object;)Z",
        array_list_remove_object,
    );
    shared.register_native(
        ARRAY_LIST_CLASS,
        "contains",
        "(Ljava/lang/Object;)Z",
        array_list_contains,
    );
    shared.register_native(
        ARRAY_LIST_CLASS,
        "indexOf",
        "(Ljava/lang/Object;)I",
        array_list_index_of,
    );
    shared.register_native(
        ARRAY_LIST_CLASS,
        "addAll",
        "(Ljava/util/Collection;)Z",
        abstract_collection_add_all,
    );
    for class in [
        ARRAY_LIST_CLASS,
        LINKED_LIST_CLASS,
        COPY_ON_WRITE_ARRAY_LIST_CLASS,
        ARRAY_DEQUE_CLASS,
    ] {
        shared.register_native(
            class,
            "<init>",
            "(Ljava/util/Collection;)V",
            list_init_collection,
        );
    }
    register_list_iterator(shared, ARRAY_LIST_CLASS);
    register_list_iterator(shared, LINKED_LIST_CLASS);
    register_list_iterator(shared, COPY_ON_WRITE_ARRAY_LIST_CLASS);
    for class in [LINKED_LIST_CLASS, COPY_ON_WRITE_ARRAY_LIST_CLASS] {
        register_list_surface(shared, class);
    }
    register_deque_natives(shared, ARRAY_DEQUE_CLASS);
    register_stack_natives(shared);
    register_deque_natives(shared, CONCURRENT_LINKED_DEQUE_CLASS);
    register_concurrent_queue_natives(shared);
    register_queue_natives(shared);
    // `CopyOnWriteArrayList` declares the absent-insertion pair itself; logback's
    // `COWArrayList` subclass reaches `addIfAbsent` while it attaches an appender.
    shared.register_native(
        COPY_ON_WRITE_ARRAY_LIST_CLASS,
        "addIfAbsent",
        "(Ljava/lang/Object;)Z",
        list_add_if_absent,
    );
    shared.register_native(
        COPY_ON_WRITE_ARRAY_LIST_CLASS,
        "addAllAbsent",
        "(Ljava/util/Collection;)I",
        list_add_all_absent,
    );
    register_set_natives(shared);
    register_iterator_natives(shared);
    register_collections_natives(shared);
    shared.register_native(
        "java/util/Enumeration",
        "hasMoreElements",
        "()Z",
        enumeration_has_more,
    );
    shared.register_native(
        "java/util/Enumeration",
        "nextElement",
        "()Ljava/lang/Object;",
        enumeration_next,
    );
    // `List.copyOf`/`Set.copyOf`/`Map.copyOf`: the immutable copies Spring's listener
    // list and the factory loader build. They copy the elements; immutability itself is
    // outside this slice, exactly as for `Collections.unmodifiableList`.
    shared.register_native(
        "java/util/List",
        "copyOf",
        "(Ljava/util/Collection;)Ljava/util/List;",
        list_copy_of,
    );
    shared.register_native(
        "java/util/Set",
        "copyOf",
        "(Ljava/util/Collection;)Ljava/util/Set;",
        set_copy_of,
    );
    shared.register_native(
        "java/util/Map",
        "copyOf",
        "(Ljava/util/Map;)Ljava/util/Map;",
        map_copy_of,
    );
    // The abstract collection superclasses carry no initializer `Code` in the C1 stubs.
    for class in [
        "java/util/AbstractCollection",
        "java/util/AbstractList",
        "java/util/AbstractSet",
        "java/util/AbstractMap",
        "java/util/AbstractSequentialList",
    ] {
        shared.register_native(class, "<init>", "()V", collections_noop_init);
    }
    // The C1 `HashSet`/`TreeSet` stubs inherit `addAll`, `contains`, `remove`, `clear`
    // and `toArray` from `AbstractCollection`, and the JDK implementations there are
    // concrete methods with no `Code` in the stub. These natives implement the same
    // operations over the VM's list and set shapes.
    let abstract_collection = "java/util/AbstractCollection";
    shared.register_native(
        abstract_collection,
        "addAll",
        "(Ljava/util/Collection;)Z",
        abstract_collection_add_all,
    );
    // `addAll(int, Collection)` is declared on `ArrayList`, `LinkedList`,
    // `AbstractList` and the `List` interface, so every declaration site a virtual call
    // can reach gets the entry.
    for class in [
        ARRAY_LIST_CLASS,
        LINKED_LIST_CLASS,
        "java/util/AbstractList",
        "java/util/List",
    ] {
        shared.register_native(
            class,
            "addAll",
            "(ILjava/util/Collection;)Z",
            list_add_all_at,
        );
    }
    shared.register_native(
        abstract_collection,
        "add",
        "(Ljava/lang/Object;)Z",
        abstract_collection_add,
    );
    shared.register_native(
        abstract_collection,
        "iterator",
        "()Ljava/util/Iterator;",
        abstract_collection_iterator,
    );
    shared.register_native(
        abstract_collection,
        "contains",
        "(Ljava/lang/Object;)Z",
        abstract_collection_contains,
    );
    shared.register_native(
        abstract_collection,
        "remove",
        "(Ljava/lang/Object;)Z",
        abstract_collection_remove,
    );
    shared.register_native(
        abstract_collection,
        "clear",
        "()V",
        abstract_collection_clear,
    );
    // The JDK implements `removeAll`/`retainAll`/`containsAll` in `AbstractCollection`,
    // overrides `removeAll` in `AbstractSet`, and redeclares the bulk operations on the
    // interfaces and on the concurrent collections whose stubs carry their own copy. Each
    // declaration site a virtual call can resolve to needs its own registry entry.
    for class in [
        "java/util/AbstractCollection",
        "java/util/AbstractSet",
        "java/util/Collection",
        "java/util/List",
        "java/util/Set",
        ARRAY_LIST_CLASS,
        LINKED_LIST_CLASS,
        COPY_ON_WRITE_ARRAY_LIST_CLASS,
        ARRAY_DEQUE_CLASS,
        HASH_SET_CLASS,
        "java/util/concurrent/CopyOnWriteArraySet",
        "java/util/concurrent/ConcurrentHashMap$KeySetView",
    ] {
        shared.register_native(
            class,
            "removeAll",
            "(Ljava/util/Collection;)Z",
            abstract_collection_remove_all,
        );
        shared.register_native(
            class,
            "retainAll",
            "(Ljava/util/Collection;)Z",
            abstract_collection_retain_all,
        );
        shared.register_native(
            class,
            "containsAll",
            "(Ljava/util/Collection;)Z",
            abstract_collection_contains_all,
        );
    }
    shared.register_native(
        abstract_collection,
        "toArray",
        "()[Ljava/lang/Object;",
        abstract_collection_to_array,
    );
    shared.register_native(
        abstract_collection,
        "toArray",
        "([Ljava/lang/Object;)[Ljava/lang/Object;",
        collection_to_array_into,
    );
    for class in [
        HASH_SET_CLASS,
        TREE_SET_CLASS,
        "java/util/EnumSet",
        "java/util/Set",
    ] {
        shared.register_native(
            class,
            "toArray",
            "([Ljava/lang/Object;)[Ljava/lang/Object;",
            collection_to_array_into,
        );
    }
    shared.register_native(
        "java/util/List",
        "toArray",
        "([Ljava/lang/Object;)[Ljava/lang/Object;",
        collection_to_array_into,
    );
    // Every concrete list and set declares its own `toArray` override in the C1 tree, so
    // the interface entries alone are not reached through virtual dispatch.
    for class in [
        ARRAY_LIST_CLASS,
        LINKED_LIST_CLASS,
        COPY_ON_WRITE_ARRAY_LIST_CLASS,
        ARRAY_DEQUE_CLASS,
        HASH_SET_CLASS,
        LINKED_HASH_SET_CLASS,
        TREE_SET_CLASS,
        COPY_ON_WRITE_ARRAY_SET_CLASS,
    ] {
        shared.register_native(
            class,
            "toArray",
            "()[Ljava/lang/Object;",
            abstract_collection_to_array,
        );
        shared.register_native(
            class,
            "toArray",
            "([Ljava/lang/Object;)[Ljava/lang/Object;",
            collection_to_array_into,
        );
    }
    shared.register_native(
        "java/util/List",
        "iterator",
        "()Ljava/util/Iterator;",
        abstract_collection_iterator,
    );
}

/// Register the `java.util.Collections` helpers Spring's static initializers use.
fn register_collections_natives(shared: &Shared) {
    let class = "java/util/Collections";
    shared.register_native(class, "reverse", "(Ljava/util/List;)V", collections_reverse);
    shared.register_native(
        class,
        "emptySet",
        "()Ljava/util/Set;",
        collections_empty_set,
    );
    shared.register_native(
        class,
        "emptyList",
        "()Ljava/util/List;",
        collections_empty_list,
    );
    shared.register_native(
        class,
        "emptyMap",
        "()Ljava/util/Map;",
        collections_empty_map,
    );
    shared.register_native(
        class,
        "singleton",
        "(Ljava/lang/Object;)Ljava/util/Set;",
        collections_singleton,
    );
    shared.register_native(
        class,
        "singletonList",
        "(Ljava/lang/Object;)Ljava/util/List;",
        collections_singleton_list,
    );
    shared.register_native(
        class,
        "singletonMap",
        "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/util/Map;",
        collections_singleton_map,
    );
    shared.register_native(
        class,
        "unmodifiableSet",
        "(Ljava/util/Set;)Ljava/util/Set;",
        collections_identity,
    );
    shared.register_native(
        class,
        "unmodifiableList",
        "(Ljava/util/List;)Ljava/util/List;",
        collections_identity,
    );
    shared.register_native(
        class,
        "unmodifiableCollection",
        "(Ljava/util/Collection;)Ljava/util/Collection;",
        collections_identity,
    );
    shared.register_native(
        class,
        "unmodifiableMap",
        "(Ljava/util/Map;)Ljava/util/Map;",
        collections_identity,
    );
    shared.register_native(
        class,
        "synchronizedSet",
        "(Ljava/util/Set;)Ljava/util/Set;",
        collections_identity,
    );
    shared.register_native(
        class,
        "synchronizedList",
        "(Ljava/util/List;)Ljava/util/List;",
        collections_identity,
    );
    shared.register_native(
        class,
        "synchronizedMap",
        "(Ljava/util/Map;)Ljava/util/Map;",
        collections_identity,
    );
    shared.register_native(
        class,
        "unmodifiableSortedSet",
        "(Ljava/util/SortedSet;)Ljava/util/SortedSet;",
        collections_identity,
    );
    shared.register_native(
        class,
        "newSetFromMap",
        "(Ljava/util/Map;)Ljava/util/Set;",
        collections_new_set_from_map,
    );
    shared.register_native(
        class,
        "emptyIterator",
        "()Ljava/util/Iterator;",
        collections_empty_iterator,
    );
    shared.register_native(
        class,
        "addAll",
        "(Ljava/util/Collection;[Ljava/lang/Object;)Z",
        collections_add_all,
    );
    register_collection_factories(shared);
}

/// Register `Set.of`/`List.of`/`Map.of` for their fixed arities and varargs forms.
///
/// `javac` selects a fixed-arity overload for up to ten arguments, so both the fixed
/// descriptors and the trailing `Object[]` form are registered.
fn register_collection_factories(shared: &Shared) {
    for class in ["java/util/Set", "java/util/List"] {
        shared.register_native(class, "of", "()Ljava/util/Set;", fixed_set_empty);
        shared.register_native(class, "of", "()Ljava/util/List;", fixed_list_empty);
        for arity in 1..=10usize {
            let parameters = "Ljava/lang/Object;".repeat(arity);
            shared.register_native(
                class,
                "of",
                format!("({parameters})Ljava/util/Set;"),
                fixed_set,
            );
            shared.register_native(
                class,
                "of",
                format!("({parameters})Ljava/util/List;"),
                fixed_list,
            );
        }
        shared.register_native(
            class,
            "of",
            "([Ljava/lang/Object;)Ljava/util/Set;",
            fixed_set_varargs,
        );
        shared.register_native(
            class,
            "of",
            "([Ljava/lang/Object;)Ljava/util/List;",
            fixed_list_varargs,
        );
    }
    // `Map.of` has one overload per pair count up to ten, exactly as `Set.of`/`List.of`
    // do up to ten elements; Spring's `AnnotationUtils` builds an eighteen-argument
    // attributes map with the nine-pair form.
    shared.register_native("java/util/Map", "of", "()Ljava/util/Map;", fixed_map);
    for pairs in 1..=10usize {
        let parameters = "Ljava/lang/Object;".repeat(pairs * 2);
        shared.register_native(
            "java/util/Map",
            "of",
            format!("({parameters})Ljava/util/Map;"),
            fixed_map,
        );
    }
}

/// Register the list surface for a list class other than `ArrayList`.
///
/// `ArrayList` has the same entries registered individually above; these two classes
/// declare the same methods, so they share the implementations.
fn register_list_surface(shared: &Shared, class: &'static str) {
    shared.register_native(class, "<init>", "()V", array_list_init);
    shared.register_native_if_declared(class, "<init>", "(I)V", array_list_init_capacity);
    shared.register_native(class, "size", "()I", array_list_size);
    shared.register_native(class, "isEmpty", "()Z", array_list_is_empty);
    shared.register_native(class, "add", "(Ljava/lang/Object;)Z", array_list_add);
    shared.register_native(class, "add", "(ILjava/lang/Object;)V", array_list_add_at);
    shared.register_native(class, "get", "(I)Ljava/lang/Object;", array_list_get);
    shared.register_native(
        class,
        "set",
        "(ILjava/lang/Object;)Ljava/lang/Object;",
        array_list_set,
    );
    shared.register_native(class, "clear", "()V", array_list_clear);
    shared.register_native(
        class,
        "remove",
        "(I)Ljava/lang/Object;",
        array_list_remove_at,
    );
    shared.register_native(
        class,
        "remove",
        "(Ljava/lang/Object;)Z",
        array_list_remove_object,
    );
    shared.register_native(
        class,
        "contains",
        "(Ljava/lang/Object;)Z",
        array_list_contains,
    );
    shared.register_native(
        class,
        "indexOf",
        "(Ljava/lang/Object;)I",
        array_list_index_of,
    );
    shared.register_native(
        class,
        "addAll",
        "(Ljava/util/Collection;)Z",
        abstract_collection_add_all,
    );
}

/// Register `iterator()` for one list class.
fn register_list_iterator(shared: &Shared, class: &'static str) {
    shared.register_native(class, "iterator", "()Ljava/util/Iterator;", list_iterator);
}

/// Register the set surface on the classes the C1 tree declares it on.
///
/// `LinkedHashSet` inherits `add`/`size`/`iterator` from `HashSet`, so the `HashSet`
/// entries also serve a `LinkedHashSet` receiver through the ordinary superclass walk.
fn register_set_natives(shared: &Shared) {
    for class in [
        LINKED_HASH_SET_CLASS,
        HASH_SET_CLASS,
        TREE_SET_CLASS,
        COPY_ON_WRITE_ARRAY_SET_CLASS,
    ] {
        for (name, descriptor) in [
            ("<init>", "()V"),
            ("<init>", "(I)V"),
            ("<init>", "(IF)V"),
            ("<init>", "(Ljava/util/Collection;)V"),
        ] {
            shared.register_native_if_declared(class, name, descriptor, set_init);
        }
    }
    // `TreeSet(Comparator)` keeps the comparator out of the set shape; this VM's sets are
    // insertion-ordered, so the constructor only starts empty.
    shared.register_native(
        TREE_SET_CLASS,
        "<init>",
        "(Ljava/util/Comparator;)V",
        set_init_comparator,
    );
    shared.register_native(
        TREE_SET_CLASS,
        "<init>",
        "(Ljava/util/SortedSet;)V",
        set_init,
    );
    for class in [
        HASH_SET_CLASS,
        TREE_SET_CLASS,
        COPY_ON_WRITE_ARRAY_SET_CLASS,
        "java/util/EnumSet",
    ] {
        shared.register_native(class, "size", "()I", set_size);
        shared.register_native(class, "isEmpty", "()Z", set_is_empty);
        shared.register_native(class, "add", "(Ljava/lang/Object;)Z", set_add);
        shared.register_native(class, "remove", "(Ljava/lang/Object;)Z", set_remove);
        shared.register_native(class, "contains", "(Ljava/lang/Object;)Z", set_contains);
        shared.register_native(class, "clear", "()V", set_clear);
        shared.register_native(class, "iterator", "()Ljava/util/Iterator;", set_iterator);
        shared.register_native(class, "addAll", "(Ljava/util/Collection;)Z", set_add_all);
        shared.register_native(class, "toArray", "()[Ljava/lang/Object;", set_to_array);
    }
    for (name, descriptor, function) in [
        ("add", "(Ljava/lang/Object;)Z", set_add as crate::NativeFn),
        ("contains", "(Ljava/lang/Object;)Z", set_contains),
        ("remove", "(Ljava/lang/Object;)Z", set_remove),
        ("size", "()I", set_size),
        ("isEmpty", "()Z", set_is_empty),
        ("iterator", "()Ljava/util/Iterator;", set_iterator),
        ("clear", "()V", set_clear),
        ("addAll", "(Ljava/util/Collection;)Z", set_add_all),
        ("toArray", "()[Ljava/lang/Object;", set_to_array),
    ] {
        shared.register_native("java/util/Set", name, descriptor, function);
    }
    // The C1 tree omits `AbstractCollection`'s concrete subclasses
    // (`AbstractList`/`AbstractSequentialList`), so a `Collection` call site on a
    // `LinkedList` has no class-file declaration to resolve to and falls back to the
    // `java/util/Collection` interface entries. Every entry there must therefore accept
    // both collection shapes; the `Set`-only natives above would reject the list.
    for (name, descriptor, function) in [
        (
            "add",
            "(Ljava/lang/Object;)Z",
            abstract_collection_add as crate::NativeFn,
        ),
        (
            "contains",
            "(Ljava/lang/Object;)Z",
            abstract_collection_contains,
        ),
        (
            "remove",
            "(Ljava/lang/Object;)Z",
            abstract_collection_remove,
        ),
        ("size", "()I", collection_size),
        ("isEmpty", "()Z", collection_is_empty),
        (
            "iterator",
            "()Ljava/util/Iterator;",
            abstract_collection_iterator,
        ),
        ("clear", "()V", abstract_collection_clear),
        (
            "addAll",
            "(Ljava/util/Collection;)Z",
            abstract_collection_add_all,
        ),
        (
            "toArray",
            "()[Ljava/lang/Object;",
            abstract_collection_to_array,
        ),
        (
            "toArray",
            "([Ljava/lang/Object;)[Ljava/lang/Object;",
            collection_to_array_into,
        ),
    ] {
        shared.register_native("java/util/Collection", name, descriptor, function);
    }
    shared.register_native(
        "java/util/EnumSet",
        "noneOf",
        "(Ljava/lang/Class;)Ljava/util/EnumSet;",
        enum_set_none_of,
    );
    for arity in 1..=5usize {
        let parameters = "Ljava/lang/Enum;".repeat(arity);
        shared.register_native(
            "java/util/EnumSet",
            "of",
            format!("({parameters})Ljava/util/EnumSet;"),
            enum_set_of,
        );
    }
    shared.register_native(
        "java/util/EnumSet",
        "of",
        "(Ljava/lang/Enum;[Ljava/lang/Enum;)Ljava/util/EnumSet;",
        enum_set_of_varargs,
    );
    shared.register_native(
        "java/util/EnumSet",
        "allOf",
        "(Ljava/lang/Class;)Ljava/util/EnumSet;",
        enum_set_none_of,
    );
    shared.register_native(
        "java/util/EnumSet",
        "copyOf",
        "(Ljava/util/Collection;)Ljava/util/EnumSet;",
        enum_set_copy_of,
    );
    shared.register_native(
        "java/util/EnumSet",
        "copyOf",
        "(Ljava/util/EnumSet;)Ljava/util/EnumSet;",
        enum_set_clone,
    );
}

/// Register the iterator surface the M2.23 collections need.
fn register_iterator_natives(shared: &Shared) {
    shared.register_native(ITERATOR_CLASS, "hasNext", "()Z", iterator_has_next);
    shared.register_native(
        ITERATOR_CLASS,
        "next",
        "()Ljava/lang/Object;",
        iterator_next,
    );
    shared.register_native(ITERATOR_CLASS, "remove", "()V", iterator_remove);
}

fn heap_error(error: HeapError) -> VmError {
    VmError::Native(error.to_string())
}

fn null_receiver(method: &'static str) -> VmError {
    VmError::Native(format!("{method} was called on a null receiver"))
}

fn int_argument(args: &[Value], position: usize, method: &'static str) -> Result<i32, VmError> {
    match args.get(position).copied() {
        Some(Value::Int(value)) => Ok(value),
        Some(_) => Err(VmError::Native(format!("{method} expects an int argument"))),
        None => Err(VmError::Native(format!("{method} is missing its argument"))),
    }
}

fn long_argument(args: &[Value], position: usize, method: &'static str) -> Result<i64, VmError> {
    match args.get(position).copied() {
        Some(Value::Long(value)) => Ok(value),
        Some(_) => Err(VmError::Native(format!("{method} expects a long argument"))),
        None => Err(VmError::Native(format!("{method} is missing its argument"))),
    }
}

fn float_argument(args: &[Value], position: usize, method: &'static str) -> Result<f32, VmError> {
    match args.get(position).copied() {
        Some(Value::Float(value)) => Ok(value),
        Some(_) => Err(VmError::Native(format!(
            "{method} expects a float argument"
        ))),
        None => Err(VmError::Native(format!("{method} is missing its argument"))),
    }
}

fn double_argument(args: &[Value], position: usize, method: &'static str) -> Result<f64, VmError> {
    match args.get(position).copied() {
        Some(Value::Double(value)) => Ok(value),
        Some(_) => Err(VmError::Native(format!(
            "{method} expects a double argument"
        ))),
        None => Err(VmError::Native(format!("{method} is missing its argument"))),
    }
}

/// A `char` argument: a UTF-16 code unit that must be a valid scalar value.
///
/// A lone surrogate is a legal Java `char` but cannot be stored in the heap's UTF-8 text,
/// so it is a typed [`VmError::UnpairedSurrogate`] rather than a corrupted builder.
fn char_argument(args: &[Value], position: usize, method: &'static str) -> Result<char, VmError> {
    let value = int_argument(args, position, method)?;
    let unit = u16::try_from(value)
        .map_err(|_| VmError::Native(format!("{method} expects a char argument")))?;
    char::from_u32(u32::from(unit)).ok_or(VmError::UnpairedSurrogate { method })
}

/// A reference argument, including null.
fn reference_argument(
    args: &[Value],
    position: usize,
    method: &'static str,
) -> Result<ObjRef, VmError> {
    match args.get(position).copied() {
        Some(Value::Ref(reference)) => Ok(reference),
        Some(_) => Err(VmError::Native(format!(
            "{method} expects a reference argument"
        ))),
        None => Err(VmError::Native(format!("{method} is missing its argument"))),
    }
}

/// A `String` argument. A null reference becomes the JDK's `"null"` text.
fn nullable_string_argument(
    shared: &Shared,
    args: &[Value],
    position: usize,
    method: &'static str,
) -> Result<String, VmError> {
    let reference = reference_argument(args, position, method)?;
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

/// The builder's current text, copied out of the heap before another lock is taken.
fn builder_text(
    shared: &Shared,
    receiver: ObjRef,
    method: &'static str,
) -> Result<String, VmError> {
    if receiver.is_null() {
        return Err(null_receiver(method));
    }
    let text = shared
        .heap()
        .string_builder(receiver)
        .map_err(heap_error)?
        .to_owned();
    Ok(text)
}

/// Replace the builder's text and return the receiver, as the JDK's builder methods do.
fn set_builder_text(
    shared: &Shared,
    receiver: ObjRef,
    text: String,
    method: &'static str,
) -> Result<Option<Value>, VmError> {
    if receiver.is_null() {
        return Err(null_receiver(method));
    }
    *shared
        .heap()
        .string_builder_mut(receiver)
        .map_err(heap_error)? = text;
    Ok(Some(Value::Ref(receiver)))
}

/// Append `text` to the builder and return the receiver.
fn append_text(
    shared: &Shared,
    receiver: ObjRef,
    text: &str,
    method: &'static str,
) -> Result<Option<Value>, VmError> {
    if receiver.is_null() {
        return Err(null_receiver(method));
    }
    shared
        .heap()
        .string_builder_mut(receiver)
        .map_err(heap_error)?
        .push_str(text);
    Ok(Some(Value::Ref(receiver)))
}

/// A UTF-16 unit index in `0..length`, or a typed `StringIndexOutOfBounds`.
fn unit_index(index: i32, length: usize) -> Result<usize, VmError> {
    match usize::try_from(index) {
        Ok(position) if position < length => Ok(position),
        _ => Err(VmError::StringIndexOutOfBounds { index, length }),
    }
}

/// An insertion point in `0..=length`, or a typed `StringIndexOutOfBounds`.
fn insertion_index(index: i32, length: usize) -> Result<usize, VmError> {
    match usize::try_from(index) {
        Ok(position) if position <= length => Ok(position),
        _ => Err(VmError::StringIndexOutOfBounds { index, length }),
    }
}

/// Reverse UTF-16 units the way `AbstractStringBuilder.reverse` does.
///
/// The units are reversed first; when that turns a surrogate pair into a low/high pair,
/// the two halves are swapped back so a supplementary character stays one character.
fn reversed_units(units: &[u16]) -> Vec<u16> {
    let mut reversed: Vec<u16> = units.iter().copied().rev().collect();
    let mut index = 0;
    while index + 1 < reversed.len() {
        let low = (0xDC00..=0xDFFF).contains(&reversed[index]);
        let high = (0xD800..=0xDBFF).contains(&reversed[index + 1]);
        if low && high {
            reversed.swap(index, index + 1);
            index += 2;
        } else {
            index += 1;
        }
    }
    reversed
}

// ---------------------------------------------------------------------------
// java.lang.StringBuilder
// ---------------------------------------------------------------------------

fn string_builder_init(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    set_builder_text(shared, receiver, String::new(), "StringBuilder.<init>")?;
    Ok(None)
}

fn string_builder_init_capacity(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let capacity = int_argument(args, 0, "StringBuilder.<init>")?;
    if capacity < 0 {
        return Err(VmError::Native(format!(
            "StringBuilder.<init>: negative capacity {capacity}"
        )));
    }
    // The capacity is a performance hint; the UTF-16 text has no separate buffer size.
    set_builder_text(shared, receiver, String::new(), "StringBuilder.<init>")?;
    Ok(None)
}

fn string_builder_init_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = nullable_string_argument(shared, args, 0, "StringBuilder.<init>")?;
    set_builder_text(shared, receiver, text, "StringBuilder.<init>")?;
    Ok(None)
}

fn string_builder_append_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = nullable_string_argument(shared, args, 0, "StringBuilder.append")?;
    append_text(shared, receiver, &text, "StringBuilder.append")
}

fn string_builder_append_object(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let reference = reference_argument(args, 0, "StringBuilder.append")?;
    let text = object_text(shared, thread, reference, "StringBuilder.append")?;
    append_text(shared, receiver, &text, "StringBuilder.append")
}

fn string_builder_append_int(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = int_argument(args, 0, "StringBuilder.append")?.to_string();
    append_text(shared, receiver, &text, "StringBuilder.append")
}

fn string_builder_append_long(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = long_argument(args, 0, "StringBuilder.append")?.to_string();
    append_text(shared, receiver, &text, "StringBuilder.append")
}

fn string_builder_append_char(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let mut text = String::new();
    text.push(char_argument(args, 0, "StringBuilder.append")?);
    append_text(shared, receiver, &text, "StringBuilder.append")
}

fn string_builder_append_boolean(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = int_argument(args, 0, "StringBuilder.append")?;
    let text = if value == 0 { "false" } else { "true" };
    append_text(shared, receiver, text, "StringBuilder.append")
}

fn string_builder_append_float(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = float_text(float_argument(args, 0, "StringBuilder.append")?);
    append_text(shared, receiver, &text, "StringBuilder.append")
}

fn string_builder_append_double(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = double_text(double_argument(args, 0, "StringBuilder.append")?);
    append_text(shared, receiver, &text, "StringBuilder.append")
}

/// `StringBuilder.append(CharSequence)`: the text of a String or another builder.
fn string_builder_append_char_sequence(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let reference = reference_argument(args, 0, "StringBuilder.append")?;
    let text = char_sequence_text(shared, reference, "StringBuilder.append")?;
    append_text(shared, receiver, &text, "StringBuilder.append")
}

/// `StringBuilder.append(CharSequence, int, int)`: the `[start, end)` slice of a
/// character sequence, with the JDK's `AbstractStringBuilder` semantics.
///
/// A null sequence is the text `"null"` before the range is checked, and the range is
/// measured in UTF-16 units, like every other builder operation. The JDK's single
/// `start < 0 || start > end || end > length` test is reported as a typed
/// [`VmError::StringIndexOutOfBounds`] naming the argument that failed.
fn string_builder_append_char_sequence_range(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let method = "StringBuilder.append";
    let reference = reference_argument(args, 0, method)?;
    let text = char_sequence_text(shared, reference, method)?;
    let units = utf16_units(&text);
    let start = int_argument(args, 1, method)?;
    let end = int_argument(args, 2, method)?;
    let start_position = usize::try_from(start)
        .ok()
        .filter(|position| *position <= units.len())
        .ok_or(VmError::StringIndexOutOfBounds {
            index: start,
            length: units.len(),
        })?;
    let end_position = usize::try_from(end)
        .ok()
        .filter(|position| *position <= units.len() && *position >= start_position)
        .ok_or(VmError::StringIndexOutOfBounds {
            index: end,
            length: units.len(),
        })?;
    let slice = string_from_utf16(&units[start_position..end_position], method)?;
    append_text(shared, receiver, &slice, method)
}

/// The text behind a `CharSequence` argument the class library models.
///
/// Implementing `CharSequence` through `length`/`charAt` would require re-entering the
/// interpreter, so the three character sequences the class library models are accepted (a
/// `String`, a `StringBuilder` and a `StringBuffer`) and any other reference is a typed
/// error rather than a guessed text. A null reference is the JDK's `"null"` text.
fn char_sequence_text(
    shared: &Shared,
    reference: ObjRef,
    method: &'static str,
) -> Result<String, VmError> {
    if reference.is_null() {
        return Ok("null".to_owned());
    }
    let heap = shared.heap();
    match heap.get(reference).map_err(heap_error)? {
        HeapObject::Str(text)
        | HeapObject::StringBuilder(text)
        | HeapObject::StringBuffer(text) => Ok(text.clone()),
        _ => Err(VmError::Native(format!(
            "{method} received {reference}, which is not a String, StringBuilder or \
             StringBuffer CharSequence"
        ))),
    }
}

fn string_builder_to_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = builder_text(shared, receiver, "StringBuilder.toString")?;
    let reference = shared.heap().allocate_string(text);
    Ok(Some(Value::Ref(reference)))
}

fn string_builder_length(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = builder_text(shared, receiver, "StringBuilder.length")?;
    let length = i32::try_from(text.encode_utf16().count())
        .map_err(|_| VmError::Native("StringBuilder is longer than an int can hold".to_owned()))?;
    Ok(Some(Value::Int(length)))
}

fn string_builder_char_at(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = builder_text(shared, receiver, "StringBuilder.charAt")?;
    let units = utf16_units(&text);
    let index = int_argument(args, 0, "StringBuilder.charAt")?;
    let position = unit_index(index, units.len())?;
    Ok(Some(Value::Int(i32::from(units[position]))))
}

/// `StringBuilder.setLength(int)`: shrink, or grow with NUL code units, as on the JDK.
///
/// A negative length is `StringIndexOutOfBounds`, and a shrink that would split a
/// surrogate pair is `UnpairedSurrogate`, because the stored UTF-8 text cannot hold the
/// lone half.
fn string_builder_set_length(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = builder_text(shared, receiver, "StringBuilder.setLength")?;
    let units = utf16_units(&text);
    let new_length = int_argument(args, 0, "StringBuilder.setLength")?;
    let position = match usize::try_from(new_length) {
        Ok(position) => position,
        _ => {
            return Err(VmError::StringIndexOutOfBounds {
                index: new_length,
                length: units.len(),
            });
        }
    };
    // `resize` truncates a shrink and grows with NUL code units, which is what
    // `AbstractStringBuilder.setLength` does.
    let mut resized = units;
    resized.resize(position, 0u16);
    let text = string_from_utf16(&resized, "StringBuilder.setLength")?;
    set_builder_text(shared, receiver, text, "StringBuilder.setLength")?;
    Ok(None)
}

/// `StringBuilder.reverse()`: reverse the UTF-16 units and keep surrogate pairs intact.
fn string_builder_reverse(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = builder_text(shared, receiver, "StringBuilder.reverse")?;
    let units = utf16_units(&text);
    let reversed = reversed_units(&units);
    let text = string_from_utf16(&reversed, "StringBuilder.reverse")?;
    set_builder_text(shared, receiver, text, "StringBuilder.reverse")
}

fn string_builder_delete_char_at(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = builder_text(shared, receiver, "StringBuilder.deleteCharAt")?;
    let mut units = utf16_units(&text);
    let index = int_argument(args, 0, "StringBuilder.deleteCharAt")?;
    let position = unit_index(index, units.len())?;
    units.remove(position);
    let text = string_from_utf16(&units, "StringBuilder.deleteCharAt")?;
    set_builder_text(shared, receiver, text, "StringBuilder.deleteCharAt")
}

fn string_builder_insert_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = builder_text(shared, receiver, "StringBuilder.insert")?;
    let mut units = utf16_units(&text);
    let index = int_argument(args, 0, "StringBuilder.insert")?;
    let position = insertion_index(index, units.len())?;
    let inserted = nullable_string_argument(shared, args, 1, "StringBuilder.insert")?;
    units.splice(position..position, inserted.encode_utf16());
    let text = string_from_utf16(&units, "StringBuilder.insert")?;
    set_builder_text(shared, receiver, text, "StringBuilder.insert")
}

// ---------------------------------------------------------------------------
// java.util.ArrayList
// ---------------------------------------------------------------------------

fn array_list_init(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    if receiver.is_null() {
        return Err(null_receiver("ArrayList.<init>"));
    }
    shared
        .heap()
        .list_mut(receiver)
        .map_err(heap_error)?
        .clear();
    Ok(None)
}

/// `ArrayList(Collection)`/`LinkedList(Collection)`/`ArrayDeque(Collection)`/
/// `CopyOnWriteArrayList(Collection)`: copy the source in iteration order.
///
/// A list keeps duplicates, so the copy is the source sequence unchanged; a null source
/// is the JDK's `NullPointerException` and is reported as a typed native error.
fn list_init_collection(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let collection = reference_argument(args, 0, "List.<init>")?;
    let source = collection_elements(shared, collection, "List.<init>")?;
    if receiver.is_null() {
        return Err(null_receiver("List.<init>"));
    }
    let mut heap = shared.heap();
    let list = heap.list_mut(receiver).map_err(heap_error)?;
    list.clear();
    list.extend(source);
    Ok(None)
}

fn array_list_init_capacity(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let capacity = int_argument(args, 0, "ArrayList.<init>")?;
    if capacity < 0 {
        return Err(VmError::Native(format!(
            "ArrayList.<init>: negative capacity {capacity}"
        )));
    }
    if receiver.is_null() {
        return Err(null_receiver("ArrayList.<init>"));
    }
    // The capacity is a performance hint; a Vec has no separate capacity contract here.
    shared
        .heap()
        .list_mut(receiver)
        .map_err(heap_error)?
        .clear();
    Ok(None)
}

fn array_list_size(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let length = {
        let heap = shared.heap();
        let list = heap.list(receiver).map_err(heap_error)?;
        list.len()
    };
    let size = i32::try_from(length)
        .map_err(|_| VmError::Native("ArrayList is longer than an int can hold".to_owned()))?;
    Ok(Some(Value::Int(size)))
}

fn array_list_is_empty(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let empty = {
        let heap = shared.heap();
        heap.list(receiver).map_err(heap_error)?.is_empty()
    };
    Ok(Some(Value::Int(i32::from(empty))))
}

fn array_list_add(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = reference_argument(args, 0, "ArrayList.add")?;
    shared
        .heap()
        .list_mut(receiver)
        .map_err(heap_error)?
        .push(Value::Ref(value));
    Ok(Some(Value::Int(1)))
}

/// An insertion point in `0..=length`, or a typed `CollectionIndexOutOfBounds`.
fn list_insertion_index(
    operation: &'static str,
    index: i32,
    length: usize,
) -> Result<usize, VmError> {
    match usize::try_from(index) {
        Ok(position) if position <= length => Ok(position),
        _ => Err(VmError::CollectionIndexOutOfBounds {
            operation,
            index,
            length,
        }),
    }
}

/// An element index in `0..length`, or a typed `CollectionIndexOutOfBounds`.
fn list_element_index(
    operation: &'static str,
    index: i32,
    length: usize,
) -> Result<usize, VmError> {
    match usize::try_from(index) {
        Ok(position) if position < length => Ok(position),
        _ => Err(VmError::CollectionIndexOutOfBounds {
            operation,
            index,
            length,
        }),
    }
}

fn array_list_add_at(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let index = int_argument(args, 0, "ArrayList.add")?;
    let value = reference_argument(args, 1, "ArrayList.add")?;
    let mut heap = shared.heap();
    let list = heap.list_mut(receiver).map_err(heap_error)?;
    let position = list_insertion_index("ArrayList.add", index, list.len())?;
    list.insert(position, Value::Ref(value));
    Ok(None)
}

/// `List.addAll(int, Collection)`: insert every source element at `index`, in order.
///
/// The JDK returns `true` when the collection was non-empty; an empty source inserts
/// nothing and reports `false`.
fn list_add_all_at(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let index = int_argument(args, 0, "List.addAll")?;
    let collection = reference_argument(args, 1, "List.addAll")?;
    let values = collection_elements(shared, collection, "List.addAll")?;
    let changed = !values.is_empty();
    let mut heap = shared.heap();
    let list = heap.list_mut(receiver).map_err(heap_error)?;
    let start = list_insertion_index("List.addAll", index, list.len())?;
    for (offset, value) in values.into_iter().enumerate() {
        list.insert(start + offset, value);
    }
    Ok(Some(Value::Int(i32::from(changed))))
}

fn array_list_get(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let index = int_argument(args, 0, "ArrayList.get")?;
    let value = {
        let heap = shared.heap();
        let list = heap.list(receiver).map_err(heap_error)?;
        let position = list_element_index("ArrayList.get", index, list.len())?;
        list[position]
    };
    Ok(Some(value))
}

fn array_list_set(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let index = int_argument(args, 0, "ArrayList.set")?;
    let value = reference_argument(args, 1, "ArrayList.set")?;
    let mut heap = shared.heap();
    let list = heap.list_mut(receiver).map_err(heap_error)?;
    let position = list_element_index("ArrayList.set", index, list.len())?;
    let previous = std::mem::replace(&mut list[position], Value::Ref(value));
    Ok(Some(previous))
}

fn array_list_clear(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    shared
        .heap()
        .list_mut(receiver)
        .map_err(heap_error)?
        .clear();
    Ok(None)
}

fn array_list_remove_at(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let index = int_argument(args, 0, "ArrayList.remove")?;
    let removed = {
        let mut heap = shared.heap();
        let list = heap.list_mut(receiver).map_err(heap_error)?;
        let position = list_element_index("ArrayList.remove", index, list.len())?;
        list.remove(position)
    };
    Ok(Some(removed))
}

/// `ArrayList.remove(Object)`: remove the first element with the same reference.
///
/// A null argument removes the first null element, as on the JDK; any other argument is
/// matched by reference identity, not by `Object.equals`.
fn array_list_remove_object(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let target = reference_argument(args, 0, "ArrayList.remove")?;
    let mut heap = shared.heap();
    let list = heap.list_mut(receiver).map_err(heap_error)?;
    let Some(position) = list
        .iter()
        .position(|element| *element == Value::Ref(target))
    else {
        return Ok(Some(Value::Int(0)));
    };
    list.remove(position);
    Ok(Some(Value::Int(1)))
}

fn list_add_if_absent(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = reference_argument(args, 0, "CopyOnWriteArrayList.addIfAbsent")?;
    let mut heap = shared.heap();
    let list = heap.list_mut(receiver).map_err(heap_error)?;
    if list.contains(&Value::Ref(value)) {
        return Ok(Some(Value::Int(0)));
    }
    list.push(Value::Ref(value));
    Ok(Some(Value::Int(1)))
}

fn list_add_all_absent(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let source = reference_argument(args, 0, "CopyOnWriteArrayList.addAllAbsent")?;
    let values = collection_elements(shared, source, "CopyOnWriteArrayList.addAllAbsent")?;
    let mut heap = shared.heap();
    let list = heap.list_mut(receiver).map_err(heap_error)?;
    let mut added = 0i32;
    for value in values {
        if !list.contains(&value) {
            list.push(value);
            added += 1;
        }
    }
    Ok(Some(Value::Int(added)))
}

fn array_list_contains(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let target = reference_argument(args, 0, "ArrayList.contains")?;
    let found = {
        let heap = shared.heap();
        let list = heap.list(receiver).map_err(heap_error)?;
        list.contains(&Value::Ref(target))
    };
    Ok(Some(Value::Int(i32::from(found))))
}

fn array_list_index_of(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let target = reference_argument(args, 0, "ArrayList.indexOf")?;
    let position = {
        let heap = shared.heap();
        let list = heap.list(receiver).map_err(heap_error)?;
        list.iter()
            .position(|element| *element == Value::Ref(target))
    };
    let index = match position {
        Some(position) => i32::try_from(position)
            .map_err(|_| VmError::Native("ArrayList is longer than an int can hold".to_owned()))?,
        None => -1,
    };
    Ok(Some(Value::Int(index)))
}

/// A non-null set receiver's elements.
fn set_elements<'a>(
    heap: &'a Heap,
    receiver: ObjRef,
    method: &'static str,
) -> Result<&'a [Value], VmError> {
    if receiver.is_null() {
        return Err(null_receiver(method));
    }
    heap.set(receiver).map_err(|error| match error {
        HeapError::WrongObjectKind { .. } => {
            VmError::Native(format!("{method} receiver is not a java.util.Set"))
        }
        other => heap_error(other),
    })
}

/// `Set.<init>()V`/`<init>(I)V`/`<init>(Collection)V`: the empty set `new` allocated,
/// optionally filled from a collection.
fn set_init(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let elements = {
        let heap = shared.heap();
        set_elements(&heap, receiver, "Set.<init>")?.to_vec()
    };
    // The initial capacity, when present, is a hint and not stored.
    if let Some(Value::Int(capacity)) = args.first().copied() {
        if capacity < 0 {
            return Err(VmError::Native(format!(
                "Set.<init>: negative capacity {capacity}"
            )));
        }
    }
    if let Some(Value::Ref(collection)) = args.first().copied() {
        if !collection.is_null() {
            let source = collection_elements(shared, collection, "Set.<init>")?;
            // A `Set` constructor copies the source as a set, so a repeated element
            // (including `null`) contributes once, in first-occurrence order. The JDK's
            // `LinkedHashSet(Collection)` does not sort; insertion order is observable
            // through iteration here just as it is there.
            let mut unique = Vec::with_capacity(source.len());
            {
                let heap = shared.heap();
                for value in source {
                    if !contains_value(&heap, &unique, value)? {
                        unique.push(value);
                    }
                }
            }
            let mut heap = shared.heap();
            let set = heap.set_mut(receiver).map_err(heap_error)?;
            set.clear();
            set.extend(unique);
        }
    }
    let _ = elements;
    Ok(None)
}

/// `Set.size()`.
fn set_size(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let length = {
        let heap = shared.heap();
        set_elements(&heap, receiver, "Set.size")?.len()
    };
    let size = i32::try_from(length)
        .map_err(|_| VmError::Native("Set is longer than an int can hold".to_owned()))?;
    Ok(Some(Value::Int(size)))
}

/// `Set.isEmpty()`.
fn set_is_empty(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let empty = {
        let heap = shared.heap();
        set_elements(&heap, receiver, "Set.isEmpty")?.is_empty()
    };
    Ok(Some(Value::Int(i32::from(empty))))
}

/// `Set.add(Object)`: insert once, reporting whether the set changed.
fn set_add(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = reference_argument(args, 0, "Set.add")?;
    let mut heap = shared.heap();
    let present = {
        let set = set_elements(&heap, receiver, "Set.add")?;
        contains_value(&heap, set, Value::Ref(value))?
    };
    if !present {
        heap.set_mut(receiver)
            .map_err(heap_error)?
            .push(Value::Ref(value));
    }
    Ok(Some(Value::Int(i32::from(!present))))
}

/// `Set.remove(Object)`: remove the matching element, reporting whether it was present.
fn set_remove(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = reference_argument(args, 0, "Set.remove")?;
    let mut heap = shared.heap();
    let position = {
        let set = set_elements(&heap, receiver, "Set.remove")?;
        find_value(&heap, set, Value::Ref(value))?
    };
    let removed = match position {
        Some(position) => {
            heap.set_mut(receiver).map_err(heap_error)?.remove(position);
            true
        }
        None => false,
    };
    Ok(Some(Value::Int(i32::from(removed))))
}

/// `Set.contains(Object)`.
fn set_contains(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = reference_argument(args, 0, "Set.contains")?;
    let found = {
        let heap = shared.heap();
        let set = set_elements(&heap, receiver, "Set.contains")?;
        contains_value(&heap, set, Value::Ref(value))?
    };
    Ok(Some(Value::Int(i32::from(found))))
}

/// `Set.clear()`.
fn set_clear(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let mut heap = shared.heap();
    heap.set_mut(receiver).map_err(heap_error)?.clear();
    Ok(None)
}

/// `Set.iterator()`: a snapshot iterator over the current elements.
fn set_iterator(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let items = {
        let heap = shared.heap();
        set_elements(&heap, receiver, "Set.iterator")?.to_vec()
    };
    let iterator = new_iterator(shared, items, receiver)?;
    Ok(Some(Value::Ref(iterator)))
}

/// `Set.addAll(Collection)`: add every source element, reporting whether the set changed.
fn set_add_all(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let collection = reference_argument(args, 0, "Set.addAll")?;
    let source = collection_elements(shared, collection, "Set.addAll")?;
    let mut heap = shared.heap();
    let mut changed = false;
    for value in source {
        let present = {
            let set = set_elements(&heap, receiver, "Set.addAll")?;
            contains_value(&heap, set, value)?
        };
        if !present {
            heap.set_mut(receiver).map_err(heap_error)?.push(value);
            changed = true;
        }
    }
    Ok(Some(Value::Int(i32::from(changed))))
}

/// `Set.toArray()`: a new `Object[]` holding the current elements.
fn set_to_array(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let values = {
        let heap = shared.heap();
        set_elements(&heap, receiver, "Set.toArray")?.to_vec()
    };
    let object = shared.load_library_class("java/lang/Object")?;
    Ok(Some(Value::Ref(allocate_reference_array(
        shared, object, values,
    ))))
}

/// `List.iterator()`: a snapshot iterator over the current elements.
fn list_iterator(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let items = {
        let heap = shared.heap();
        if receiver.is_null() {
            return Err(null_receiver("List.iterator"));
        }
        heap.list(receiver).map_err(heap_error)?.to_vec()
    };
    let iterator = new_iterator(shared, items, receiver)?;
    Ok(Some(Value::Ref(iterator)))
}

/// Allocate an iterator over `items`, remembering `source` for `Iterator.remove`.
fn new_iterator(shared: &Shared, items: Vec<Value>, source: ObjRef) -> Result<ObjRef, VmError> {
    let class = shared.load_library_class(ITERATOR_CLASS)?;
    Ok(shared
        .heap()
        .allocate_iterator_with_source(class, items, source))
}

/// The elements of a collection argument, which may be a list or a set.
fn collection_elements(
    shared: &Shared,
    reference: ObjRef,
    method: &'static str,
) -> Result<Vec<Value>, VmError> {
    if reference.is_null() {
        return Err(VmError::Native(format!(
            "{method} received a null collection"
        )));
    }
    let heap = shared.heap();
    match heap.get(reference).map_err(heap_error)? {
        HeapObject::List(list) => Ok(list.elements.clone()),
        HeapObject::Set(set) => Ok(set.elements.clone()),
        _ => Err(VmError::Native(format!(
            "{method} expects a java.util.Collection argument"
        ))),
    }
}

/// Membership by the equality the map uses for keys: strings by content, boxed
/// `Integer`s by value, everything else by reference identity.
fn contains_value(heap: &Heap, values: &[Value], probe: Value) -> Result<bool, VmError> {
    for value in values {
        if values_equal(heap, *value, probe).map_err(heap_error)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The position of `probe` by the same equality.
fn find_value(heap: &Heap, values: &[Value], probe: Value) -> Result<Option<usize>, VmError> {
    for (position, value) in values.iter().enumerate() {
        if values_equal(heap, *value, probe).map_err(heap_error)? {
            return Ok(Some(position));
        }
    }
    Ok(None)
}

/// Allocate an array of references with `component` and fill it with `values`.
fn allocate_reference_array(
    shared: &Shared,
    component: crate::loader::ClassId,
    values: Vec<Value>,
) -> ObjRef {
    let reference = shared
        .heap()
        .allocate_array(ClassRef::Named(component), values.len());
    {
        let mut heap = shared.heap();
        let array = heap
            .array_mut(reference)
            .expect("a freshly allocated array is an array");
        for (slot, value) in array.elements.iter_mut().zip(values) {
            *slot = value;
        }
    }
    reference
}

/// `Iterator.hasNext()` for the VM's snapshot iterator.
///
/// `rava_interp::service_loader` overrides the `java/util/Iterator` registry entry to
/// serve its lazy iterator and delegates every other receiver here, so this stays the one
/// snapshot implementation.
pub fn iterator_has_next(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let has_next = {
        let heap = shared.heap();
        let iterator = heap
            .get(receiver)
            .map_err(heap_error)
            .and_then(|object| match object {
                HeapObject::Iterator(iterator) => Ok(iterator),
                _ => Err(VmError::Native(
                    "Iterator.hasNext receiver is not an iterator".to_owned(),
                )),
            })?;
        iterator.position < iterator.items.len()
    };
    Ok(Some(Value::Int(i32::from(has_next))))
}

/// `Iterator.next()`: the element at the current position and advance.
///
/// This is the snapshot fallback for receivers without service state; see
/// [`iterator_has_next`].
pub fn iterator_next(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let mut heap = shared.heap();
    let iterator = heap.iterator_mut(receiver).map_err(heap_error)?;
    let Some(value) = iterator.items.get(iterator.position).copied() else {
        return Err(VmError::Native(
            "Iterator.next past the end of the iterator".to_owned(),
        ));
    };
    iterator.position += 1;
    Ok(Some(value))
}

/// `Iterator.remove()`: remove the last element returned from its list or set source.
///
/// The iterator is still a snapshot, so the last-returned value is resolved by position in
/// the snapshot and then removed from both the source and the snapshot. A stream iterator
/// (or the empty iterator) has no mutable source and reports a typed error.
fn iterator_remove(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let (value, source, index) = {
        let heap = shared.heap();
        let iterator = match heap.get(receiver).map_err(heap_error)? {
            HeapObject::Iterator(iterator) => iterator,
            _ => {
                return Err(VmError::Native(format!("{receiver} is not a vm iterator")));
            }
        };
        if iterator.position == 0 {
            return Err(VmError::Native(
                "Iterator.remove called before next()".to_owned(),
            ));
        }
        let index = iterator.position - 1;
        let value = *iterator
            .items
            .get(index)
            .ok_or_else(|| VmError::Native("iterator snapshot is inconsistent".to_owned()))?;
        (value, iterator.source, index)
    };
    if source.is_null() {
        return Err(VmError::Native(
            "Iterator.remove is not supported by this iterator".to_owned(),
        ));
    }
    let mut heap = shared.heap();
    match heap.get(source).map_err(heap_error)? {
        HeapObject::List(list) => {
            // The snapshot index is the list index unless the list changed after the
            // iterator was created; falling back to equality keeps the common case exact.
            let matches = list
                .elements
                .get(index)
                .is_some_and(|candidate| values_equal(&heap, *candidate, value).unwrap_or(false));
            let index = if matches {
                index
            } else {
                let mut found = None;
                for (position, candidate) in list.elements.iter().enumerate() {
                    if values_equal(&heap, *candidate, value).unwrap_or(false) {
                        found = Some(position);
                        break;
                    }
                }
                found.ok_or_else(|| {
                    VmError::Native("Iterator.remove value is no longer in the list".to_owned())
                })?
            };
            heap.list_mut(source).map_err(heap_error)?.remove(index);
        }
        HeapObject::Set(set) => {
            let mut found = None;
            for (position, candidate) in set.elements.iter().enumerate() {
                if values_equal(&heap, *candidate, value).unwrap_or(false) {
                    found = Some(position);
                    break;
                }
            }
            let index = found.ok_or_else(|| {
                VmError::Native("Iterator.remove value is no longer in the set".to_owned())
            })?;
            heap.set_mut(source).map_err(heap_error)?.remove(index);
        }
        _ => {
            return Err(VmError::Native(
                "Iterator.remove source is not a list or set".to_owned(),
            ));
        }
    }
    let iterator = heap.iterator_mut(receiver).map_err(heap_error)?;
    if index < iterator.position {
        iterator.items.remove(index);
        iterator.position -= 1;
    }
    Ok(None)
}

/// Install `Collections.EMPTY_SET`, `EMPTY_LIST` and `EMPTY_MAP`.
///
/// The C1 stubs carry no initializer `Code` for `Collections`, so the real `<clinit>` never
/// runs and these `static final` fields would read as their type's default (`null`).
/// CGLIB's `EmitUtils.wrap_undeclared_throwable` reads `Collections.EMPTY_SET` whenever the
/// method it is emitting has no checked exceptions, so the empty collection objects must
/// exist before any application code runs. They are ordinary heap shapes, so the same
/// `Collection.add`/`iterator`/`contains` natives serve them as `Collections.emptySet()`
/// returns.
pub fn install_empty_constants(shared: &Shared) -> Result<(), VmError> {
    let collections = shared.load_library_class("java/util/Collections")?;
    let set = {
        let class = shared.load_library_class(LINKED_HASH_SET_CLASS)?;
        shared.heap().allocate_set(class)
    };
    let list = {
        let class = shared.load_library_class(ARRAY_LIST_CLASS)?;
        shared.heap().allocate_list(class)
    };
    let map = {
        let class = shared.load_library_class(crate::maps::HASH_MAP_CLASS)?;
        shared.heap().allocate_map(class)
    };
    let entry = shared
        .entry(collections)
        .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", collections.0)))?;
    let mut statics = entry.statics.lock().expect("static field lock poisoned");
    statics.insert(
        ("EMPTY_SET".to_owned(), "Ljava/util/Set;".to_owned()),
        Value::Ref(set),
    );
    statics.insert(
        ("EMPTY_LIST".to_owned(), "Ljava/util/List;".to_owned()),
        Value::Ref(list),
    );
    statics.insert(
        ("EMPTY_MAP".to_owned(), "Ljava/util/Map;".to_owned()),
        Value::Ref(map),
    );
    Ok(())
}

fn collections_empty_set(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let class = shared.load_library_class(LINKED_HASH_SET_CLASS)?;
    let set = shared.heap().allocate_set(class);
    Ok(Some(Value::Ref(set)))
}

fn collections_empty_list(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let class = shared.load_library_class(ARRAY_LIST_CLASS)?;
    let list = shared.heap().allocate_list(class);
    Ok(Some(Value::Ref(list)))
}

fn collections_empty_map(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let class = shared.load_library_class(crate::maps::HASH_MAP_CLASS)?;
    let map = shared.heap().allocate_map(class);
    Ok(Some(Value::Ref(map)))
}

fn collections_singleton(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = reference_argument(args, 0, "Collections.singleton")?;
    let class = shared.load_library_class(LINKED_HASH_SET_CLASS)?;
    let set = shared.heap().allocate_set(class);
    {
        let mut heap = shared.heap();
        heap.set_mut(set)
            .map_err(heap_error)?
            .push(Value::Ref(value));
    }
    Ok(Some(Value::Ref(set)))
}

fn collections_singleton_list(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = reference_argument(args, 0, "Collections.singletonList")?;
    let class = shared.load_library_class(ARRAY_LIST_CLASS)?;
    let list = shared.heap().allocate_list(class);
    {
        let mut heap = shared.heap();
        heap.list_mut(list)
            .map_err(heap_error)?
            .push(Value::Ref(value));
    }
    Ok(Some(Value::Ref(list)))
}

fn collections_singleton_map(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let key = reference_argument(args, 0, "Collections.singletonMap")?;
    let value = reference_argument(args, 1, "Collections.singletonMap")?;
    let class = shared.load_library_class(crate::maps::HASH_MAP_CLASS)?;
    let map = shared.heap().allocate_map(class);
    {
        let mut heap = shared.heap();
        heap.map_mut(map)
            .map_err(heap_error)?
            .push((Value::Ref(key), Value::Ref(value)));
    }
    Ok(Some(Value::Ref(map)))
}

/// The `unmodifiable*`/`synchronized*` wrappers return the argument unchanged.
///
/// M2.23 has one thread and no mutation through a wrapper in the startup path; enforcing
/// immutability needs a view object, which is deferred.
/// `Collections.newSetFromMap(Map)`: an empty VM set.
///
/// The JDK's result is backed by the map; this VM's set is the ordinary heap set, so the
/// two do not share storage. Spring builds identity/thread-safe sets with this factory and
/// uses only the set interface, which the ordinary set implements.
fn collections_new_set_from_map(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let map = reference_argument(args, 0, "Collections.newSetFromMap")?;
    if map.is_null() {
        return Err(VmError::Native(
            "Collections.newSetFromMap received a null map".to_owned(),
        ));
    }
    let class = shared.load_library_class(LINKED_HASH_SET_CLASS)?;
    Ok(Some(Value::Ref(shared.heap().allocate_set(class))))
}

/// `Collections.emptyIterator()`.
fn collections_empty_iterator(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let class = shared.load_library_class(ITERATOR_CLASS)?;
    Ok(Some(Value::Ref(
        shared.heap().allocate_iterator(class, Vec::new()),
    )))
}

fn collections_identity(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = reference_argument(args, 0, "Collections.wrapper")?;
    Ok(Some(Value::Ref(value)))
}

fn collections_noop_init(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(None)
}

/// A receiver that is either a list or a set, for the `AbstractCollection` natives.
enum AnyCollection {
    List(Vec<Value>),
    Set(Vec<Value>),
}

fn any_collection(
    shared: &Shared,
    receiver: ObjRef,
    method: &'static str,
) -> Result<AnyCollection, VmError> {
    if receiver.is_null() {
        return Err(null_receiver(method));
    }
    let heap = shared.heap();
    match heap.get(receiver).map_err(heap_error)? {
        HeapObject::List(list) => Ok(AnyCollection::List(list.elements.clone())),
        HeapObject::Set(set) => Ok(AnyCollection::Set(set.elements.clone())),
        _ => Err(VmError::Native(format!(
            "{method} receiver is not a java.util.Collection"
        ))),
    }
}

/// `Collection.size()`: the element count of either container shape.
fn collection_size(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    if receiver.is_null() {
        return Err(null_receiver("Collection.size"));
    }
    let length = {
        let heap = shared.heap();
        match heap.get(receiver).map_err(heap_error)? {
            HeapObject::List(list) => list.elements.len(),
            HeapObject::Set(set) => set.elements.len(),
            _ => {
                return Err(VmError::Native(
                    "Collection.size receiver is not a java.util.Collection".to_owned(),
                ));
            }
        }
    };
    let size = i32::try_from(length)
        .map_err(|_| VmError::Native("collection is longer than an int can hold".to_owned()))?;
    Ok(Some(Value::Int(size)))
}

/// `Collection.isEmpty()`: whether either container shape holds no elements.
fn collection_is_empty(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    if receiver.is_null() {
        return Err(null_receiver("Collection.isEmpty"));
    }
    let empty = {
        let heap = shared.heap();
        match heap.get(receiver).map_err(heap_error)? {
            HeapObject::List(list) => list.elements.is_empty(),
            HeapObject::Set(set) => set.elements.is_empty(),
            _ => {
                return Err(VmError::Native(
                    "Collection.isEmpty receiver is not a java.util.Collection".to_owned(),
                ));
            }
        }
    };
    Ok(Some(Value::Int(i32::from(empty))))
}

fn abstract_collection_add_all(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let source = reference_argument(args, 0, "Collection.addAll")?;
    let values = collection_elements(shared, source, "Collection.addAll")?;
    match any_collection(shared, receiver, "Collection.addAll")? {
        AnyCollection::List(existing) => {
            let mut heap = shared.heap();
            let list = heap.list_mut(receiver).map_err(heap_error)?;
            let mut changed = false;
            for value in values {
                if !existing.contains(&value) {
                    changed = true;
                }
                list.push(value);
            }
            Ok(Some(Value::Int(i32::from(changed))))
        }
        AnyCollection::Set(_) => {
            let mut heap = shared.heap();
            let mut changed = false;
            for value in values {
                let present = {
                    let set = heap.set(receiver).map_err(heap_error)?;
                    contains_value(&heap, set, value)?
                };
                if !present {
                    heap.set_mut(receiver).map_err(heap_error)?.push(value);
                    changed = true;
                }
            }
            Ok(Some(Value::Int(i32::from(changed))))
        }
    }
}

fn abstract_collection_contains(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = reference_argument(args, 0, "Collection.contains")?;
    let found = {
        let heap = shared.heap();
        let elements = match heap.get(receiver).map_err(heap_error)? {
            HeapObject::List(list) => &list.elements,
            HeapObject::Set(set) => &set.elements,
            _ => {
                return Err(VmError::Native(
                    "Collection.contains receiver is not a java.util.Collection".to_owned(),
                ));
            }
        };
        contains_value(&heap, elements, Value::Ref(value))?
    };
    Ok(Some(Value::Int(i32::from(found))))
}

fn abstract_collection_remove(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = reference_argument(args, 0, "Collection.remove")?;
    let position = {
        let heap = shared.heap();
        let elements = match heap.get(receiver).map_err(heap_error)? {
            HeapObject::List(list) => &list.elements,
            HeapObject::Set(set) => &set.elements,
            _ => {
                return Err(VmError::Native(
                    "Collection.remove receiver is not a java.util.Collection".to_owned(),
                ));
            }
        };
        find_value(&heap, elements, Value::Ref(value))?
    };
    let removed = match position {
        Some(position) => {
            let mut heap = shared.heap();
            match heap.get(receiver).map_err(heap_error)? {
                HeapObject::List(_) => {
                    heap.list_mut(receiver)
                        .map_err(heap_error)?
                        .remove(position);
                }
                HeapObject::Set(_) => {
                    heap.set_mut(receiver).map_err(heap_error)?.remove(position);
                }
                _ => unreachable!(),
            }
            true
        }
        None => false,
    };
    Ok(Some(Value::Int(i32::from(removed))))
}

/// A snapshot of a list's or set's elements, rejecting anything else.
fn collection_values(
    shared: &Shared,
    reference: ObjRef,
    method: &'static str,
) -> Result<Vec<Value>, VmError> {
    if reference.is_null() {
        return Err(null_receiver(method));
    }
    let heap = shared.heap();
    match heap.get(reference).map_err(heap_error)? {
        HeapObject::List(list) => Ok(list.elements.clone()),
        HeapObject::Set(set) => Ok(set.elements.clone()),
        _ => Err(VmError::Native(format!(
            "{method} receiver is not a java.util.Collection"
        ))),
    }
}

/// Replace a list's or set's elements with `values`, keeping the receiver's identity.
fn replace_collection(
    shared: &Shared,
    receiver: ObjRef,
    values: Vec<Value>,
    method: &'static str,
) -> Result<(), VmError> {
    let mut heap = shared.heap();
    match heap.get(receiver).map_err(heap_error)? {
        HeapObject::List(_) => {
            let list = heap.list_mut(receiver).map_err(heap_error)?;
            list.clear();
            list.extend(values);
        }
        HeapObject::Set(_) => {
            let set = heap.set_mut(receiver).map_err(heap_error)?;
            set.clear();
            set.extend(values);
        }
        _ => {
            return Err(VmError::Native(format!(
                "{method} receiver is not a java.util.Collection"
            )));
        }
    }
    Ok(())
}

/// Is `probe` equal to any element of `values` by the collection equality rule?
fn contains_any(heap: &Heap, values: &[Value], probe: Value) -> Result<bool, VmError> {
    for value in values {
        if values_equal(heap, *value, probe).map_err(heap_error)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// `Collection.removeAll(Collection)`: drop every element the argument contains,
/// reporting whether the collection changed.
///
/// The argument is read first, so the receiver's heap lock is never held while the
/// argument is inspected. A receiver that is the argument itself loses every element,
/// which is what the JDK's `AbstractSet.removeAll` does.
fn abstract_collection_remove_all(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let argument = reference_argument(args, 0, "Collection.removeAll")?;
    if argument == receiver {
        let elements = collection_values(shared, receiver, "Collection.removeAll")?;
        let removed = !elements.is_empty();
        replace_collection(shared, receiver, Vec::new(), "Collection.removeAll")?;
        return Ok(Some(Value::Int(i32::from(removed))));
    }
    let source = collection_values(shared, argument, "Collection.removeAll")?;
    let elements = collection_values(shared, receiver, "Collection.removeAll")?;
    let original_len = elements.len();
    let kept = {
        let heap = shared.heap();
        let mut kept = Vec::with_capacity(original_len);
        for value in elements {
            if !contains_any(&heap, &source, value)? {
                kept.push(value);
            }
        }
        kept
    };
    let changed = kept.len() != original_len;
    if changed {
        replace_collection(shared, receiver, kept, "Collection.removeAll")?;
    }
    Ok(Some(Value::Int(i32::from(changed))))
}

/// `Collection.retainAll(Collection)`: keep only the elements the argument contains,
/// reporting whether the collection changed.
fn abstract_collection_retain_all(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let argument = reference_argument(args, 0, "Collection.retainAll")?;
    let source = collection_values(shared, argument, "Collection.retainAll")?;
    let elements = collection_values(shared, receiver, "Collection.retainAll")?;
    let original_len = elements.len();
    let kept = {
        let heap = shared.heap();
        let mut kept = Vec::with_capacity(original_len);
        for value in elements {
            if contains_any(&heap, &source, value)? {
                kept.push(value);
            }
        }
        kept
    };
    let changed = kept.len() != original_len;
    if changed {
        replace_collection(shared, receiver, kept, "Collection.retainAll")?;
    }
    Ok(Some(Value::Int(i32::from(changed))))
}

/// `Collection.containsAll(Collection)`: whether every argument element is present.
fn abstract_collection_contains_all(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let argument = reference_argument(args, 0, "Collection.containsAll")?;
    let source = collection_values(shared, argument, "Collection.containsAll")?;
    let elements = collection_values(shared, receiver, "Collection.containsAll")?;
    let mut all = true;
    {
        let heap = shared.heap();
        for value in source {
            if !contains_any(&heap, &elements, value)? {
                all = false;
                break;
            }
        }
    }
    Ok(Some(Value::Int(i32::from(all))))
}

fn abstract_collection_clear(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let mut heap = shared.heap();
    match heap.get(receiver).map_err(heap_error)? {
        HeapObject::List(_) => heap.list_mut(receiver).map_err(heap_error)?.clear(),
        HeapObject::Set(_) => heap.set_mut(receiver).map_err(heap_error)?.clear(),
        _ => {
            return Err(VmError::Native(
                "Collection.clear receiver is not a java.util.Collection".to_owned(),
            ));
        }
    }
    Ok(None)
}

fn abstract_collection_to_array(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let values = match any_collection(shared, receiver, "Collection.toArray")? {
        AnyCollection::List(values) | AnyCollection::Set(values) => values,
    };
    let object = shared.load_library_class("java/lang/Object")?;
    Ok(Some(Value::Ref(allocate_reference_array(
        shared, object, values,
    ))))
}

/// `Collections.addAll(Collection, Object...)`: add every array element.
fn collections_add_all(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let collection = reference_argument(args, 0, "Collections.addAll")?;
    let array = reference_argument(args, 1, "Collections.addAll")?;
    let values: Vec<Value> = {
        let heap = shared.heap();
        heap.array(array).map_err(heap_error)?.elements.to_vec()
    };
    let mut heap = shared.heap();
    let mut changed = false;
    match heap.get(collection).map_err(heap_error)? {
        HeapObject::List(_) => {
            for value in values {
                heap.list_mut(collection).map_err(heap_error)?.push(value);
                changed = true;
            }
        }
        HeapObject::Set(_) => {
            for value in values {
                let present = {
                    let set = heap.set(collection).map_err(heap_error)?;
                    contains_value(&heap, set, value)?
                };
                if !present {
                    heap.set_mut(collection).map_err(heap_error)?.push(value);
                    changed = true;
                }
            }
        }
        _ => {
            return Err(VmError::Native(
                "Collections.addAll expects a java.util.Collection".to_owned(),
            ));
        }
    }
    Ok(Some(Value::Int(i32::from(changed))))
}

fn fixed_set_empty(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let class = shared.load_library_class(LINKED_HASH_SET_CLASS)?;
    Ok(Some(Value::Ref(shared.heap().allocate_set(class))))
}

fn fixed_list_empty(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let class = shared.load_library_class(ARRAY_LIST_CLASS)?;
    Ok(Some(Value::Ref(shared.heap().allocate_list(class))))
}

fn fixed_set(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    build_fixed_set(shared, args.to_vec())
}

fn fixed_set_varargs(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let array = reference_argument(args, 0, "Set.of")?;
    let values = shared
        .heap()
        .array(array)
        .map_err(heap_error)?
        .elements
        .to_vec();
    build_fixed_set(shared, values)
}

fn build_fixed_set(shared: &Shared, values: Vec<Value>) -> Result<Option<Value>, VmError> {
    let mut unique = Vec::with_capacity(values.len());
    {
        let heap = shared.heap();
        for value in values {
            if contains_value(&heap, &unique, value)? {
                return Err(VmError::Native(
                    "Set.of received a duplicate element".to_owned(),
                ));
            }
            unique.push(value);
        }
    }
    let class = shared.load_library_class(LINKED_HASH_SET_CLASS)?;
    let set = shared.heap().allocate_set(class);
    {
        let mut heap = shared.heap();
        *heap.set_mut(set).expect("a freshly allocated set is a set") = unique;
    }
    Ok(Some(Value::Ref(set)))
}

fn fixed_list(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    build_fixed_list(shared, args.to_vec())
}

fn fixed_list_varargs(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let array = reference_argument(args, 0, "List.of")?;
    let values = shared
        .heap()
        .array(array)
        .map_err(heap_error)?
        .elements
        .to_vec();
    build_fixed_list(shared, values)
}

fn build_fixed_list(shared: &Shared, values: Vec<Value>) -> Result<Option<Value>, VmError> {
    let class = shared.load_library_class(ARRAY_LIST_CLASS)?;
    let list = shared.heap().allocate_list(class);
    {
        let mut heap = shared.heap();
        *heap
            .list_mut(list)
            .expect("a freshly allocated list is a list") = values;
    }
    Ok(Some(Value::Ref(list)))
}

fn fixed_map(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let class = shared.load_library_class(crate::maps::HASH_MAP_CLASS)?;
    let map = shared.heap().allocate_map(class);
    let mut entries = Vec::with_capacity(args.len() / 2);
    for pair in args.chunks(2) {
        if pair.len() == 2 {
            entries.push((pair[0], pair[1]));
        }
    }
    {
        let mut heap = shared.heap();
        *heap.map_mut(map).expect("a freshly allocated map is a map") = entries;
    }
    Ok(Some(Value::Ref(map)))
}

/// `EnumSet.copyOf(Collection)`: the collection's elements as an `EnumSet`.
///
/// The VM's enum set is the ordinary set shape with the `java.util.EnumSet` class; the
/// element's enum type is carried by the elements themselves, so no separate enum-type
/// field is needed. The JDK rejects an empty collection because it cannot infer the
/// element type; this slice reports that as a typed error for the same reason.
fn enum_set_copy_of(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let source = reference_argument(args, 0, "EnumSet.copyOf")?;
    let values = collection_elements(shared, source, "EnumSet.copyOf")?;
    if values.is_empty() {
        // The JDK throws `IllegalArgumentException` here (the element type is unknown);
        // a caller that guards the call catches it.
        let throwable = crate::exceptions::new_throwable(
            shared,
            "java/lang/IllegalArgumentException",
            "Collection is empty".to_owned(),
        )?;
        return Err(VmError::Thrown(throwable));
    }
    let class = shared.load_library_class("java/util/EnumSet")?;
    let set = shared.heap().allocate_set(class);
    for value in values {
        let present = {
            let heap = shared.heap();
            contains_value(&heap, heap.set(set).map_err(heap_error)?, value)?
        };
        if !present {
            shared.heap().set_mut(set).map_err(heap_error)?.push(value);
        }
    }
    Ok(Some(Value::Ref(set)))
}

/// `EnumSet.noneOf(Class)`: an empty set of the enum type.
///
/// The C1 tree has only the abstract `EnumSet`, so the set's runtime class is
/// `java.util.EnumSet`; its `add`/`contains`/`iterator` entries are registered above and
/// `instanceof Set` walks the stub's interface list.
fn enum_set_none_of(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let class = shared.load_library_class("java/util/EnumSet")?;
    let set = shared.heap().allocate_set(class);
    Ok(Some(Value::Ref(set)))
}

/// `AbstractCollection.add(Object)`: apply list or set insertion semantics.
fn abstract_collection_add(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = reference_argument(args, 0, "Collection.add")?;
    let mut heap = shared.heap();
    match heap.get(receiver).map_err(heap_error)? {
        HeapObject::List(_) => {
            heap.list_mut(receiver)
                .map_err(heap_error)?
                .push(Value::Ref(value));
            Ok(Some(Value::Int(1)))
        }
        HeapObject::Set(_) => {
            let present = {
                let set = heap.set(receiver).map_err(heap_error)?;
                contains_value(&heap, set, Value::Ref(value))?
            };
            if present {
                Ok(Some(Value::Int(0)))
            } else {
                heap.set_mut(receiver)
                    .map_err(heap_error)?
                    .push(Value::Ref(value));
                Ok(Some(Value::Int(1)))
            }
        }
        _ => Err(VmError::Native(
            "Collection.add receiver is not a java.util.Collection".to_owned(),
        )),
    }
}

/// `AbstractCollection.iterator()`: a snapshot iterator over either container shape.
fn abstract_collection_iterator(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let items = match any_collection(shared, receiver, "Collection.iterator")? {
        AnyCollection::List(values) | AnyCollection::Set(values) => values,
    };
    let iterator = new_iterator(shared, items, receiver)?;
    Ok(Some(Value::Ref(iterator)))
}

/// `Collection.toArray(Object[])`: fill the supplied array, or allocate one when it is too
/// small, and append a null terminator when there is room.
fn collection_to_array_into(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let target = reference_argument(args, 0, "Collection.toArray")?;
    let values = match any_collection(shared, receiver, "Collection.toArray")? {
        AnyCollection::List(values) | AnyCollection::Set(values) => values,
    };
    let object = shared.load_library_class("java/lang/Object")?;
    let (target_length, target_component) = {
        let heap = shared.heap();
        let array = heap.array(target).map_err(heap_error)?;
        (array.elements.len(), array.component.clone())
    };
    let array = if target_length >= values.len() {
        target
    } else {
        // The caller's array names the component type it expects (`Adapt[]`), so a
        // too-small argument array is replaced by an array of that same component rather
        // than of `Object`.
        let component = if target.is_null() {
            ClassRef::Named(object)
        } else {
            target_component
        };
        shared.heap().allocate_array(component, values.len())
    };
    {
        let mut heap = shared.heap();
        let elements = heap.array_mut(array).map_err(heap_error)?;
        for (slot, value) in elements.elements.iter_mut().zip(values.iter().copied()) {
            *slot = value;
        }
        if elements.elements.len() > values.len() {
            elements.elements[values.len()] = Value::Ref(ObjRef::NULL);
        }
    }
    Ok(Some(Value::Ref(array)))
}

fn enum_set_of(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    enum_set_from_values(shared, args.to_vec())
}

fn enum_set_of_varargs(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let mut values = Vec::new();
    if let Some(first) = args.first().copied() {
        values.push(first);
    }
    if let Some(Value::Ref(array)) = args.get(1).copied() {
        if !array.is_null() {
            values.extend(
                shared
                    .heap()
                    .array(array)
                    .map_err(heap_error)?
                    .elements
                    .iter()
                    .copied(),
            );
        }
    }
    enum_set_from_values(shared, values)
}

fn enum_set_from_values(shared: &Shared, values: Vec<Value>) -> Result<Option<Value>, VmError> {
    let class = shared.load_library_class("java/util/EnumSet")?;
    let set = shared.heap().allocate_set(class);
    {
        let mut heap = shared.heap();
        *heap.set_mut(set).expect("a freshly allocated set is a set") = values;
    }
    Ok(Some(Value::Ref(set)))
}

fn enumeration_has_more(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let has_more = {
        let mut heap = shared.heap();
        let enumeration = heap.enumeration_mut(receiver).map_err(heap_error)?;
        enumeration.position < enumeration.items.len()
    };
    Ok(Some(Value::Int(i32::from(has_more))))
}

fn enumeration_next(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let mut heap = shared.heap();
    let enumeration = heap.enumeration_mut(receiver).map_err(heap_error)?;
    let Some(value) = enumeration.items.get(enumeration.position).copied() else {
        return Err(VmError::Native(
            "Enumeration.nextElement past the end".to_owned(),
        ));
    };
    enumeration.position += 1;
    Ok(Some(value))
}

/// Register a `Deque` surface over the list shape.
fn register_deque_natives(shared: &Shared, class: &'static str) {
    shared.register_native(class, "<init>", "()V", array_list_init);
    shared.register_native_if_declared(class, "<init>", "(I)V", array_list_init_capacity);
    shared.register_native(
        class,
        "<init>",
        "(Ljava/util/Collection;)V",
        deque_init_collection,
    );
    shared.register_native(class, "size", "()I", array_list_size);
    shared.register_native(class, "isEmpty", "()Z", array_list_is_empty);
    shared.register_native(class, "add", "(Ljava/lang/Object;)Z", deque_push_back);
    shared.register_native(
        class,
        "addLast",
        "(Ljava/lang/Object;)V",
        deque_push_back_void,
    );
    shared.register_native(
        class,
        "addFirst",
        "(Ljava/lang/Object;)V",
        deque_push_front_void,
    );
    shared.register_native(
        class,
        "push",
        "(Ljava/lang/Object;)V",
        deque_push_front_void,
    );
    shared.register_native(class, "offer", "(Ljava/lang/Object;)Z", deque_push_back);
    shared.register_native(
        class,
        "offerFirst",
        "(Ljava/lang/Object;)Z",
        deque_push_front,
    );
    shared.register_native(class, "offerLast", "(Ljava/lang/Object;)Z", deque_push_back);
    shared.register_native(class, "poll", "()Ljava/lang/Object;", deque_poll_front);
    shared.register_native(class, "pollFirst", "()Ljava/lang/Object;", deque_poll_front);
    shared.register_native(class, "pollLast", "()Ljava/lang/Object;", deque_poll_back);
    shared.register_native(class, "remove", "()Ljava/lang/Object;", deque_poll_front);
    shared.register_native(
        class,
        "removeFirst",
        "()Ljava/lang/Object;",
        deque_poll_front,
    );
    shared.register_native(class, "removeLast", "()Ljava/lang/Object;", deque_poll_back);
    shared.register_native(class, "pop", "()Ljava/lang/Object;", deque_poll_front);
    shared.register_native(class, "peek", "()Ljava/lang/Object;", deque_peek_front);
    shared.register_native(class, "peekFirst", "()Ljava/lang/Object;", deque_peek_front);
    shared.register_native(class, "peekLast", "()Ljava/lang/Object;", deque_peek_back);
    shared.register_native(class, "getFirst", "()Ljava/lang/Object;", deque_peek_front);
    shared.register_native(class, "getLast", "()Ljava/lang/Object;", deque_peek_back);
    shared.register_native(class, "iterator", "()Ljava/util/Iterator;", list_iterator);
    shared.register_native(class, "toArray", "()[Ljava/lang/Object;", set_to_array);
}

/// Register the `java.util.concurrent.ConcurrentLinkedQueue` surface over the list shape.
///
/// The queue is a FIFO sequence; its operations never block, so the ordinary list
/// functions are the whole implementation.
fn register_concurrent_queue_natives(shared: &Shared) {
    let class = CONCURRENT_LINKED_QUEUE_CLASS;
    shared.register_native(class, "<init>", "()V", array_list_init);
    shared.register_native(
        class,
        "<init>",
        "(Ljava/util/Collection;)V",
        deque_init_collection,
    );
    shared.register_native(class, "size", "()I", array_list_size);
    shared.register_native(class, "isEmpty", "()Z", array_list_is_empty);
    shared.register_native(class, "offer", "(Ljava/lang/Object;)Z", deque_push_back);
    shared.register_native(class, "add", "(Ljava/lang/Object;)Z", deque_push_back);
    shared.register_native(class, "poll", "()Ljava/lang/Object;", deque_poll_front);
    shared.register_native(class, "remove", "()Ljava/lang/Object;", deque_poll_front);
    shared.register_native(class, "peek", "()Ljava/lang/Object;", deque_peek_front);
    shared.register_native(class, "element", "()Ljava/lang/Object;", deque_peek_front);
    shared.register_native(
        class,
        "remove",
        "(Ljava/lang/Object;)Z",
        array_list_remove_object,
    );
    shared.register_native(
        class,
        "contains",
        "(Ljava/lang/Object;)Z",
        array_list_contains,
    );
    shared.register_native(class, "iterator", "()Ljava/util/Iterator;", list_iterator);
    shared.register_native(
        class,
        "toArray",
        "()[Ljava/lang/Object;",
        abstract_collection_to_array,
    );
}

fn deque_init_collection(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let collection = reference_argument(args, 0, "ArrayDeque.<init>")?;
    let values = collection_elements(shared, collection, "ArrayDeque.<init>")?;
    let mut heap = shared.heap();
    *heap.list_mut(receiver).map_err(heap_error)? = values;
    Ok(None)
}

fn deque_push_back(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = reference_argument(args, 0, "Deque.add")?;
    shared
        .heap()
        .list_mut(receiver)
        .map_err(heap_error)?
        .push(Value::Ref(value));
    Ok(Some(Value::Int(1)))
}

fn deque_push_back_void(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    deque_push_back(shared, _thread, _loader, receiver, args)?;
    Ok(None)
}

fn deque_push_front(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = reference_argument(args, 0, "Deque.addFirst")?;
    shared
        .heap()
        .list_mut(receiver)
        .map_err(heap_error)?
        .insert(0, Value::Ref(value));
    Ok(Some(Value::Int(1)))
}

fn deque_push_front_void(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    deque_push_front(shared, _thread, _loader, receiver, args)?;
    Ok(None)
}

fn deque_poll_front(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let mut heap = shared.heap();
    let list = heap.list_mut(receiver).map_err(heap_error)?;
    if list.is_empty() {
        Ok(Some(Value::Ref(ObjRef::NULL)))
    } else {
        Ok(Some(list.remove(0)))
    }
}

fn deque_poll_back(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let mut heap = shared.heap();
    let list = heap.list_mut(receiver).map_err(heap_error)?;
    Ok(Some(list.pop().unwrap_or(Value::Ref(ObjRef::NULL))))
}

fn deque_peek_front(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let mut heap = shared.heap();
    let list = heap.list_mut(receiver).map_err(heap_error)?;
    Ok(Some(
        list.first().copied().unwrap_or(Value::Ref(ObjRef::NULL)),
    ))
}

fn deque_peek_back(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let mut heap = shared.heap();
    let list = heap.list_mut(receiver).map_err(heap_error)?;
    Ok(Some(
        list.last().copied().unwrap_or(Value::Ref(ObjRef::NULL)),
    ))
}

// ---------------------------------------------------------------------------
// `java.util.concurrent.LinkedBlockingQueue`
// ---------------------------------------------------------------------------

/// Register the `LinkedBlockingQueue` surface over the list shape.
///
/// A blocking queue is a FIFO sequence, so the elements live in the same list shape as
/// the other containers. SLF4J 2 constructs one unconditionally while `LoggerFactory`
/// initializes its substitute provider, which is the startup path M4.5 has to run.
///
/// The VM is single-threaded until M5. `offer`, `poll`, `peek`, `clear` and the query
/// methods never wait, and they are the whole surface the startup path touches. `put`
/// and `take` perform their operation when it cannot block and report a typed error when
/// it would have to wait; waking a producer or consumer is the M5 boundary, not a
/// behaviour this slice can honestly provide.
fn register_queue_natives(shared: &Shared) {
    let class = LINKED_BLOCKING_QUEUE_CLASS;
    shared.register_native(class, "<init>", "()V", queue_init);
    shared.register_native(class, "<init>", "(I)V", queue_init_capacity);
    shared.register_native(
        class,
        "<init>",
        "(Ljava/util/Collection;)V",
        queue_init_collection,
    );
    shared.register_native(class, "size", "()I", array_list_size);
    shared.register_native(class, "isEmpty", "()Z", array_list_is_empty);
    shared.register_native(class, "remainingCapacity", "()I", queue_remaining_capacity);
    shared.register_native(class, "offer", "(Ljava/lang/Object;)Z", queue_offer);
    shared.register_native(class, "add", "(Ljava/lang/Object;)Z", queue_add);
    shared.register_native(class, "put", "(Ljava/lang/Object;)V", queue_put);
    shared.register_native(class, "poll", "()Ljava/lang/Object;", deque_poll_front);
    shared.register_native(class, "take", "()Ljava/lang/Object;", queue_take);
    shared.register_native(class, "peek", "()Ljava/lang/Object;", deque_peek_front);
    shared.register_native(class, "clear", "()V", array_list_clear);
    shared.register_native(
        class,
        "contains",
        "(Ljava/lang/Object;)Z",
        array_list_contains,
    );
    shared.register_native(
        class,
        "remove",
        "(Ljava/lang/Object;)Z",
        array_list_remove_object,
    );
    shared.register_native(
        class,
        "drainTo",
        "(Ljava/util/Collection;)I",
        queue_drain_to,
    );
    shared.register_native(
        class,
        "drainTo",
        "(Ljava/util/Collection;I)I",
        queue_drain_to_max,
    );
    shared.register_native(
        class,
        "toArray",
        "()[Ljava/lang/Object;",
        abstract_collection_to_array,
    );
    shared.register_native(
        class,
        "toArray",
        "([Ljava/lang/Object;)[Ljava/lang/Object;",
        collection_to_array_into,
    );
    shared.register_native(class, "iterator", "()Ljava/util/Iterator;", list_iterator);
}

fn queue_init(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let mut heap = shared.heap();
    heap.list_mut(receiver).map_err(heap_error)?.clear();
    heap.set_list_capacity(receiver, usize::MAX)
        .map_err(heap_error)?;
    Ok(None)
}

fn queue_init_capacity(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let capacity = int_argument(args, 0, "LinkedBlockingQueue.<init>")?;
    let capacity = usize::try_from(capacity).ok().filter(|value| *value > 0);
    let Some(capacity) = capacity else {
        return Err(VmError::Native(
            "LinkedBlockingQueue.<init>: capacity must be positive".to_owned(),
        ));
    };
    let mut heap = shared.heap();
    heap.list_mut(receiver).map_err(heap_error)?.clear();
    heap.set_list_capacity(receiver, capacity)
        .map_err(heap_error)?;
    Ok(None)
}

fn queue_init_collection(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let source = reference_argument(args, 0, "LinkedBlockingQueue.<init>")?;
    let values = collection_elements(shared, source, "LinkedBlockingQueue.<init>")?;
    if values
        .iter()
        .any(|value| matches!(value, Value::Ref(reference) if reference.is_null()))
    {
        return Err(VmError::Native(
            "LinkedBlockingQueue.<init>: null elements are not permitted".to_owned(),
        ));
    }
    let mut heap = shared.heap();
    let list = heap.list_mut(receiver).map_err(heap_error)?;
    list.clear();
    list.extend(values);
    heap.set_list_capacity(receiver, usize::MAX)
        .map_err(heap_error)?;
    Ok(None)
}

fn queue_remaining_capacity(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let heap = shared.heap();
    let capacity = heap.list_capacity(receiver).map_err(heap_error)?;
    let length = heap.list(receiver).map_err(heap_error)?.len();
    let remaining = capacity.saturating_sub(length).min(i32::MAX as usize);
    Ok(Some(Value::Int(remaining as i32)))
}

fn queue_offer(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = reference_argument(args, 0, "LinkedBlockingQueue.offer")?;
    if value.is_null() {
        return Err(VmError::Native(
            "LinkedBlockingQueue.offer: null elements are not permitted".to_owned(),
        ));
    }
    let mut heap = shared.heap();
    let capacity = heap.list_capacity(receiver).map_err(heap_error)?;
    let list = heap.list_mut(receiver).map_err(heap_error)?;
    if list.len() >= capacity {
        return Ok(Some(Value::Int(0)));
    }
    list.push(Value::Ref(value));
    Ok(Some(Value::Int(1)))
}

fn queue_add(
    shared: &Shared,
    thread: &ThreadState,
    loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let offered = queue_offer(shared, thread, loader, receiver, args)?;
    if offered == Some(Value::Int(0)) {
        return Err(VmError::Native(
            "LinkedBlockingQueue.add: the queue is full".to_owned(),
        ));
    }
    Ok(Some(Value::Int(1)))
}

fn queue_put(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = reference_argument(args, 0, "LinkedBlockingQueue.put")?;
    if value.is_null() {
        return Err(VmError::Native(
            "LinkedBlockingQueue.put: null elements are not permitted".to_owned(),
        ));
    }
    let mut heap = shared.heap();
    let capacity = heap.list_capacity(receiver).map_err(heap_error)?;
    let list = heap.list_mut(receiver).map_err(heap_error)?;
    if list.len() >= capacity {
        return Err(VmError::Native(
            "LinkedBlockingQueue.put would block on a full queue; the VM is single-threaded until M5"
                .to_owned(),
        ));
    }
    list.push(Value::Ref(value));
    Ok(None)
}

fn queue_take(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let mut heap = shared.heap();
    let list = heap.list_mut(receiver).map_err(heap_error)?;
    if list.is_empty() {
        return Err(VmError::Native(
            "LinkedBlockingQueue.take would block on an empty queue; the VM is single-threaded until M5"
                .to_owned(),
        ));
    }
    Ok(Some(list.remove(0)))
}

fn queue_drain_to(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let target = reference_argument(args, 0, "LinkedBlockingQueue.drainTo")?;
    let drained = drain_queue(shared, receiver, target, usize::MAX)?;
    Ok(Some(Value::Int(drained)))
}

fn queue_drain_to_max(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let target = reference_argument(args, 0, "LinkedBlockingQueue.drainTo")?;
    let max = int_argument(args, 1, "LinkedBlockingQueue.drainTo")?;
    let limit = usize::try_from(max).unwrap_or(0);
    if limit == 0 {
        return Ok(Some(Value::Int(0)));
    }
    let drained = drain_queue(shared, receiver, target, limit)?;
    Ok(Some(Value::Int(drained)))
}

/// Move up to `limit` front elements from `receiver` to the collection `target`.
fn drain_queue(
    shared: &Shared,
    receiver: ObjRef,
    target: ObjRef,
    limit: usize,
) -> Result<i32, VmError> {
    let mut heap = shared.heap();
    let (values, count): (Vec<Value>, usize) = {
        let list = heap.list_mut(receiver).map_err(heap_error)?;
        let count = limit.min(list.len());
        (list.drain(..count).collect(), count)
    };
    let is_list = matches!(heap.get(target).map_err(heap_error)?, HeapObject::List(_));
    if is_list {
        heap.list_mut(target).map_err(heap_error)?.extend(values);
    } else if matches!(heap.get(target).map_err(heap_error)?, HeapObject::Set(_)) {
        for value in values {
            let present = {
                let set = heap.set(target).map_err(heap_error)?;
                contains_value(&heap, set, value)?
            };
            if !present {
                heap.set_mut(target).map_err(heap_error)?.push(value);
            }
        }
    } else {
        return Err(VmError::Native(
            "LinkedBlockingQueue.drainTo expects a java.util.Collection argument".to_owned(),
        ));
    }
    i32::try_from(count).map_err(|_| {
        VmError::Native("LinkedBlockingQueue.drainTo drained too many elements".to_owned())
    })
}

/// `List.copyOf(Collection)`: a list holding the source elements.
fn list_copy_of(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let source = reference_argument(args, 0, "List.copyOf")?;
    let values = collection_elements(shared, source, "List.copyOf")?;
    let class = shared.load_library_class(ARRAY_LIST_CLASS)?;
    let list = shared.heap().allocate_list(class);
    shared
        .heap()
        .list_mut(list)
        .map_err(heap_error)?
        .extend(values);
    Ok(Some(Value::Ref(list)))
}

/// `Set.copyOf(Collection)`: a set holding the distinct source elements.
fn set_copy_of(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let source = reference_argument(args, 0, "Set.copyOf")?;
    let values = collection_elements(shared, source, "Set.copyOf")?;
    let class = shared.load_library_class(HASH_SET_CLASS)?;
    let set = shared.heap().allocate_set(class);
    for value in values {
        let present = {
            let heap = shared.heap();
            contains_value(&heap, heap.set(set).map_err(heap_error)?, value)?
        };
        if !present {
            shared.heap().set_mut(set).map_err(heap_error)?.push(value);
        }
    }
    Ok(Some(Value::Ref(set)))
}

/// `Map.copyOf(Map)`: a map holding the source entries.
fn map_copy_of(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let source = reference_argument(args, 0, "Map.copyOf")?;
    let entries = {
        let heap = shared.heap();
        match heap.get(source).map_err(heap_error)? {
            HeapObject::Map(map) => map.entries.clone(),
            _ => {
                return Err(VmError::Native(
                    "Map.copyOf expects a java.util.Map argument".to_owned(),
                ));
            }
        }
    };
    let class = shared.load_library_class(crate::maps::HASH_MAP_CLASS)?;
    let map = shared.heap().allocate_map(class);
    shared
        .heap()
        .map_mut(map)
        .map_err(heap_error)?
        .extend(entries);
    Ok(Some(Value::Ref(map)))
}

/// `Collections.reverse(List)`: reverse the list in place.
fn collections_reverse(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let list = reference_argument(args, 0, "Collections.reverse")?;
    if list.is_null() {
        return Err(VmError::Native(
            "Collections.reverse received a null list".to_owned(),
        ));
    }
    shared.heap().list_mut(list).map_err(heap_error)?.reverse();
    Ok(None)
}

/// Register the `java.util.Stack` surface over the list shape.
///
/// `Stack` is the legacy `Vector` subclass logback's `NodeToStringTransformer` uses for its
/// recursion guard. The C1 tree projects `Stack` but not its `Vector` superclass, so the VM
/// models it as the list shape (the same decision as `ArrayDeque`) and implements the stack
/// operations directly.
fn register_stack_natives(shared: &Shared) {
    let class = STACK_CLASS;
    shared.register_native(class, "<init>", "()V", array_list_init);
    shared.register_native(class, "size", "()I", array_list_size);
    shared.register_native(class, "isEmpty", "()Z", array_list_is_empty);
    shared.register_native(
        class,
        "push",
        "(Ljava/lang/Object;)Ljava/lang/Object;",
        stack_push,
    );
    shared.register_native(class, "add", "(Ljava/lang/Object;)Z", deque_push_back);
    shared.register_native(
        class,
        "addElement",
        "(Ljava/lang/Object;)V",
        deque_push_back_void,
    );
    shared.register_native(class, "pop", "()Ljava/lang/Object;", stack_pop);
    shared.register_native(class, "peek", "()Ljava/lang/Object;", deque_peek_front);
    shared.register_native(class, "empty", "()Z", array_list_is_empty);
    shared.register_native(
        class,
        "lastElement",
        "()Ljava/lang/Object;",
        deque_peek_back,
    );
    shared.register_native(
        class,
        "firstElement",
        "()Ljava/lang/Object;",
        deque_peek_front,
    );
    shared.register_native(class, "get", "(I)Ljava/lang/Object;", array_list_get);
    shared.register_native(class, "elementAt", "(I)Ljava/lang/Object;", array_list_get);
    shared.register_native(
        class,
        "set",
        "(ILjava/lang/Object;)Ljava/lang/Object;",
        array_list_set,
    );
    shared.register_native(
        class,
        "remove",
        "(I)Ljava/lang/Object;",
        array_list_remove_at,
    );
    shared.register_native(class, "iterator", "()Ljava/util/Iterator;", list_iterator);
    shared.register_native(class, "search", "(Ljava/lang/Object;)I", stack_search);
    shared.register_native(
        class,
        "toArray",
        "()[Ljava/lang/Object;",
        abstract_collection_to_array,
    );
}

fn stack_push(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = reference_argument(args, 0, "Stack.push")?;
    shared
        .heap()
        .list_mut(receiver)
        .map_err(heap_error)?
        .push(Value::Ref(value));
    Ok(Some(Value::Ref(value)))
}

fn stack_pop(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let mut heap = shared.heap();
    let list = heap.list_mut(receiver).map_err(heap_error)?;
    match list.pop() {
        Some(value) => Ok(Some(value)),
        None => Err(VmError::Native(
            "Stack.pop on an empty stack (EmptyStackException is not synthesized)".to_owned(),
        )),
    }
}

fn stack_search(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = reference_argument(args, 0, "Stack.search")?;
    let position = {
        let heap = shared.heap();
        heap.list(receiver)
            .map_err(heap_error)?
            .iter()
            .rev()
            .position(|element| *element == Value::Ref(value))
    };
    Ok(Some(Value::Int(match position {
        Some(position) => (position + 1) as i32,
        None => -1,
    })))
}

/// `TreeSet(Comparator)`: an empty set; the comparator is accepted and not stored.
fn set_init_comparator(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    shared.heap().set_mut(receiver).map_err(heap_error)?.clear();
    Ok(None)
}

/// `EnumSet.copyOf(EnumSet)`: a copy that, unlike the `Collection` overload, may be empty.
fn enum_set_clone(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let source = reference_argument(args, 0, "EnumSet.copyOf")?;
    let values = collection_elements(shared, source, "EnumSet.copyOf")?;
    let class = shared.load_library_class("java/util/EnumSet")?;
    let set = shared.heap().allocate_set(class);
    shared
        .heap()
        .set_mut(set)
        .map_err(heap_error)?
        .extend(values);
    Ok(Some(Value::Ref(set)))
}
