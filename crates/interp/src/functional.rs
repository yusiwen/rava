//! Functional-interface default methods (M4.5).
//!
//! Spring's configuration-property and conversion code composes predicates with
//! `BiPredicate.or`/`and` and `Predicate.or`/`and`/`negate`. Those are interface default
//! methods in the JDK, so the C1 stub declares them without `Code`; a call site resolves to
//! the interface declaration and needs a native.
//!
//! A composition returns another lambda object: its interface is the same functional
//! interface, its SAM is `test`, and it captures the operands. The target is a native
//! registry entry (`LambdaTargetKind::Native`) that evaluates the operands in order with
//! the JDK's short-circuit rule. The same mechanism the VM factories use for
//! `Comparator.comparingInt` carries it, so there is no second lambda representation.
//!
//! Boundaries: `Function.andThen`/`compose` and `Consumer.andThen` are not installed yet;
//! a call to one still fails loudly by name. `Predicate.not` and `BiPredicate` variants
//! not listed above are later slices.

use rava_vm::{
    HeapObject, LambdaObject, LambdaTarget, LambdaTargetKind, LoaderId, ObjRef, Shared,
    ThreadState, Value, VmError,
};

use crate::{InterpError, lambda, resolve_virtual, run_instance_method, runtime_class};

const BI_PREDICATE_CLASS: &str = "java/util/function/BiPredicate";
const PREDICATE_CLASS: &str = "java/util/function/Predicate";
const BI_PREDICATE_TEST: &str = "(Ljava/lang/Object;Ljava/lang/Object;)Z";
const PREDICATE_TEST: &str = "(Ljava/lang/Object;)Z";

/// Register the predicate composition natives.
pub fn register_natives(shared: &Shared) {
    shared.register_native(
        BI_PREDICATE_CLASS,
        "or",
        "(Ljava/util/function/BiPredicate;)Ljava/util/function/BiPredicate;",
        bi_predicate_or,
    );
    shared.register_native(
        BI_PREDICATE_CLASS,
        "and",
        "(Ljava/util/function/BiPredicate;)Ljava/util/function/BiPredicate;",
        bi_predicate_and,
    );
    shared.register_native(
        BI_PREDICATE_CLASS,
        "negate",
        "()Ljava/util/function/BiPredicate;",
        bi_predicate_negate,
    );
    shared.register_native(
        PREDICATE_CLASS,
        "or",
        "(Ljava/util/function/Predicate;)Ljava/util/function/Predicate;",
        predicate_or,
    );
    shared.register_native(
        PREDICATE_CLASS,
        "and",
        "(Ljava/util/function/Predicate;)Ljava/util/function/Predicate;",
        predicate_and,
    );
    shared.register_native(
        PREDICATE_CLASS,
        "negate",
        "()Ljava/util/function/Predicate;",
        predicate_negate,
    );

    // The native targets of the composed lambdas. The extra parameter is the SAM arguments:
    // captured operands come first, exactly as `invoke_sam` orders call values.
    shared.register_native(
        BI_PREDICATE_CLASS,
        "$or",
        "(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;)Z",
        bi_predicate_or_native,
    );
    shared.register_native(
        BI_PREDICATE_CLASS,
        "$and",
        "(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;)Z",
        bi_predicate_and_native,
    );
    shared.register_native(
        BI_PREDICATE_CLASS,
        "$negate",
        "(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;)Z",
        bi_predicate_negate_native,
    );
    shared.register_native(
        PREDICATE_CLASS,
        "$or",
        "(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;)Z",
        predicate_or_native,
    );
    shared.register_native(
        PREDICATE_CLASS,
        "$and",
        "(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;)Z",
        predicate_and_native,
    );
    shared.register_native(
        PREDICATE_CLASS,
        "$negate",
        "(Ljava/lang/Object;Ljava/lang/Object;)Z",
        predicate_negate_native,
    );
    // `Function.identity()`/`UnaryOperator.identity()`: a lambda that returns its argument.
    //
    // The two static methods erase differently: `Function.identity()` returns `Function`,
    // `UnaryOperator.identity()` returns the subtype `UnaryOperator`. The lambda's
    // interface must be the class the static method belongs to, or an `invokeinterface`
    // through `UnaryOperator.apply` fails the interface check.
    shared.register_native(
        "java/util/function/Function",
        "identity",
        "()Ljava/util/function/Function;",
        function_identity,
    );
    shared.register_native(
        "java/util/function/UnaryOperator",
        "identity",
        "()Ljava/util/function/UnaryOperator;",
        unary_operator_identity,
    );
    for class in [
        "java/util/function/Function",
        "java/util/function/UnaryOperator",
    ] {
        shared.register_native(
            class,
            "$identity",
            "(Ljava/lang/Object;)Ljava/lang/Object;",
            function_identity_apply,
        );
    }
}

const FUNCTION_APPLY: &str = "(Ljava/lang/Object;)Ljava/lang/Object;";

fn function_identity(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    identity_lambda(shared, "java/util/function/Function")
}

fn unary_operator_identity(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    identity_lambda(shared, "java/util/function/UnaryOperator")
}

/// A lambda of `interface_name` whose `apply` returns its argument.
fn identity_lambda(shared: &Shared, interface_name: &str) -> Result<Option<Value>, VmError> {
    let interface = shared.load_library_class(interface_name)?;
    let lambda = shared.allocate_lambda(LambdaObject {
        interface,
        sam_name: "apply".to_owned(),
        sam_descriptor: FUNCTION_APPLY.to_owned(),
        instantiated_descriptor: FUNCTION_APPLY.to_owned(),
        target: LambdaTarget {
            class: interface,
            name: "$identity".to_owned(),
            descriptor: FUNCTION_APPLY.to_owned(),
            kind: LambdaTargetKind::Native,
        },
        captured: Vec::new(),
    });
    Ok(Some(Value::Ref(lambda)))
}

fn function_identity_apply(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(
        args.first().copied().unwrap_or(Value::Ref(ObjRef::NULL)),
    ))
}

/// Allocate a composed lambda whose target is one of the `$...` natives above.
fn compose(
    shared: &Shared,
    interface_name: &str,
    sam_descriptor: &str,
    target_name: &str,
    target_descriptor: &str,
    captured: Vec<Value>,
) -> Result<ObjRef, VmError> {
    let interface = shared.load_library_class(interface_name)?;
    Ok(shared.allocate_lambda(LambdaObject {
        interface,
        sam_name: "test".to_owned(),
        sam_descriptor: sam_descriptor.to_owned(),
        instantiated_descriptor: sam_descriptor.to_owned(),
        target: LambdaTarget {
            class: interface,
            name: target_name.to_owned(),
            descriptor: target_descriptor.to_owned(),
            kind: LambdaTargetKind::Native,
        },
        captured,
    }))
}

fn reference(args: &[Value], position: usize, method: &str) -> Result<ObjRef, VmError> {
    match args.get(position).copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => Ok(reference),
        _ => Err(VmError::Native(format!(
            "{method}: missing or null operand at position {position}"
        ))),
    }
}

/// Call a predicate's `test` SAM and return its boolean value.
fn call_test(
    shared: &Shared,
    thread: &ThreadState,
    predicate: ObjRef,
    args: Vec<Value>,
    sam_descriptor: &str,
) -> Result<bool, VmError> {
    let lambda_object = match shared.heap().get(predicate) {
        Ok(HeapObject::Lambda(object)) => Some(object.clone()),
        Ok(_) => None,
        Err(error) => return Err(VmError::Native(error.to_string())),
    };
    let value = if let Some(object) = lambda_object {
        match lambda::invoke_sam(shared, thread, &object, args) {
            Ok(value) => value,
            Err(InterpError::Thrown(throwable)) => return Err(VmError::Thrown(throwable)),
            Err(error) => return Err(VmError::Native(error.to_string())),
        }
    } else {
        let class = match runtime_class(shared, predicate) {
            Ok(class) => class,
            Err(InterpError::Thrown(throwable)) => return Err(VmError::Thrown(throwable)),
            Err(error) => return Err(VmError::Native(error.to_string())),
        };
        let (entry, _) = match resolve_virtual(shared, class, "test", sam_descriptor) {
            Ok(found) => found,
            Err(InterpError::Thrown(throwable)) => return Err(VmError::Thrown(throwable)),
            Err(error) => return Err(VmError::Native(error.to_string())),
        };
        match run_instance_method(
            shared,
            thread,
            entry.id,
            "test",
            sam_descriptor,
            predicate,
            args,
        ) {
            Ok(value) => value,
            Err(InterpError::Thrown(throwable)) => return Err(VmError::Thrown(throwable)),
            Err(error) => return Err(VmError::Native(error.to_string())),
        }
    };
    Ok(matches!(value, Some(Value::Int(value)) if value != 0))
}

// --- BiPredicate -----------------------------------------------------------

fn bi_predicate_or(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let other = reference(args, 0, "BiPredicate.or")?;
    Ok(Some(Value::Ref(compose(
        shared,
        BI_PREDICATE_CLASS,
        BI_PREDICATE_TEST,
        "$or",
        "(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;)Z",
        vec![Value::Ref(receiver), Value::Ref(other)],
    )?)))
}

fn bi_predicate_and(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let other = reference(args, 0, "BiPredicate.and")?;
    Ok(Some(Value::Ref(compose(
        shared,
        BI_PREDICATE_CLASS,
        BI_PREDICATE_TEST,
        "$and",
        "(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;)Z",
        vec![Value::Ref(receiver), Value::Ref(other)],
    )?)))
}

fn bi_predicate_negate(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Ref(compose(
        shared,
        BI_PREDICATE_CLASS,
        BI_PREDICATE_TEST,
        "$negate",
        "(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;)Z",
        vec![Value::Ref(receiver)],
    )?)))
}

fn bi_predicate_or_native(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let first = reference(args, 0, "BiPredicate.or")?;
    let second = reference(args, 1, "BiPredicate.or")?;
    let a = args.get(2).copied().unwrap_or(Value::Ref(ObjRef::NULL));
    let b = args.get(3).copied().unwrap_or(Value::Ref(ObjRef::NULL));
    let left = call_test(shared, thread, first, vec![a, b], BI_PREDICATE_TEST)?;
    if left {
        return Ok(Some(Value::Int(1)));
    }
    let right = call_test(shared, thread, second, vec![a, b], BI_PREDICATE_TEST)?;
    Ok(Some(Value::Int(i32::from(right))))
}

fn bi_predicate_and_native(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let first = reference(args, 0, "BiPredicate.and")?;
    let second = reference(args, 1, "BiPredicate.and")?;
    let a = args.get(2).copied().unwrap_or(Value::Ref(ObjRef::NULL));
    let b = args.get(3).copied().unwrap_or(Value::Ref(ObjRef::NULL));
    let left = call_test(shared, thread, first, vec![a, b], BI_PREDICATE_TEST)?;
    if !left {
        return Ok(Some(Value::Int(0)));
    }
    let right = call_test(shared, thread, second, vec![a, b], BI_PREDICATE_TEST)?;
    Ok(Some(Value::Int(i32::from(right))))
}

fn bi_predicate_negate_native(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let operand = reference(args, 0, "BiPredicate.negate")?;
    let a = args.get(1).copied().unwrap_or(Value::Ref(ObjRef::NULL));
    let b = args.get(2).copied().unwrap_or(Value::Ref(ObjRef::NULL));
    let value = call_test(shared, thread, operand, vec![a, b], BI_PREDICATE_TEST)?;
    Ok(Some(Value::Int(i32::from(!value))))
}

// --- Predicate -------------------------------------------------------------

fn predicate_or(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let other = reference(args, 0, "Predicate.or")?;
    Ok(Some(Value::Ref(compose(
        shared,
        PREDICATE_CLASS,
        PREDICATE_TEST,
        "$or",
        "(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;)Z",
        vec![Value::Ref(receiver), Value::Ref(other)],
    )?)))
}

fn predicate_and(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let other = reference(args, 0, "Predicate.and")?;
    Ok(Some(Value::Ref(compose(
        shared,
        PREDICATE_CLASS,
        PREDICATE_TEST,
        "$and",
        "(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;)Z",
        vec![Value::Ref(receiver), Value::Ref(other)],
    )?)))
}

fn predicate_negate(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Ref(compose(
        shared,
        PREDICATE_CLASS,
        PREDICATE_TEST,
        "$negate",
        "(Ljava/lang/Object;Ljava/lang/Object;)Z",
        vec![Value::Ref(receiver)],
    )?)))
}

fn predicate_or_native(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let first = reference(args, 0, "Predicate.or")?;
    let second = reference(args, 1, "Predicate.or")?;
    let value = args.get(2).copied().unwrap_or(Value::Ref(ObjRef::NULL));
    let left = call_test(shared, thread, first, vec![value], PREDICATE_TEST)?;
    if left {
        return Ok(Some(Value::Int(1)));
    }
    let right = call_test(shared, thread, second, vec![value], PREDICATE_TEST)?;
    Ok(Some(Value::Int(i32::from(right))))
}

fn predicate_and_native(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let first = reference(args, 0, "Predicate.and")?;
    let second = reference(args, 1, "Predicate.and")?;
    let value = args.get(2).copied().unwrap_or(Value::Ref(ObjRef::NULL));
    let left = call_test(shared, thread, first, vec![value], PREDICATE_TEST)?;
    if !left {
        return Ok(Some(Value::Int(0)));
    }
    let right = call_test(shared, thread, second, vec![value], PREDICATE_TEST)?;
    Ok(Some(Value::Int(i32::from(right))))
}

fn predicate_negate_native(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let operand = reference(args, 0, "Predicate.negate")?;
    let value = args.get(1).copied().unwrap_or(Value::Ref(ObjRef::NULL));
    let result = call_test(shared, thread, operand, vec![value], PREDICATE_TEST)?;
    Ok(Some(Value::Int(i32::from(!result))))
}
