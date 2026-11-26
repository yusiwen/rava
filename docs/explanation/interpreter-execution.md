# Interpreter execution: from a method call to a frame

## Question

How does rava turn a loaded class and a method call into a running frame, dispatch to a
native or bytecode body, propagate a Java exception, and run a lambda or stream
callback?

## Entry points

`rava-interp` has one execution path in
[`crates/interp/src/lib.rs`](../../crates/interp/src/lib.rs):

- `run_static_method(shared, thread, class, name, descriptor, args)` — no receiver;
- `run_instance_method(shared, thread, class, name, descriptor, receiver, args)` —
  receiver becomes local 0;
- both call the private `run_method(..., receiver: Option<ObjRef>, ...)`.

`rava run` calls `run_static_method` for `main([Ljava/lang/String;)V`. Reflective
invocation, callbacks and streams re-enter these same functions; there is no second
frame model.

## 1. Resolve and choose an implementation

`run_method` performs, in order:

1. `Shared::check_no_locks_held()` — a Java call must not begin under a VM lock.
2. `ensure_initialized(class)` unless this is `<clinit>`; class state lives on
   `ClassEntry`.
3. Annotation interception: an annotation receiver answers `annotationType`, its
   element methods and its value-based `equals`/`hashCode`/`toString` before ordinary
   method lookup.
4. `ClassFile::find_method(name, descriptor)` — exact name and descriptor, no overload
   search.
5. **Native registry before `Code` or `ACC_NATIVE`.** A C1 stub method has no `Code`
   and is usually not marked native; if a `(owner, name, descriptor)` entry exists, it
   is the implementation.
6. A method marked native without a registry entry is a typed “no native
   implementation” error; an abstract method is `NativeOrAbstract`; otherwise the
   `Code` attribute must exist.
7. On the bytecode path, an instance receiver must be assignable to the declaring
   class; natives keep their own receiver rules.
8. Decode `Code` once into `DecodedMethod`, build the frame, enter the thread frame-root
   guard and execute.

The native ABI is receiver-aware:

```rust
type NativeFn = fn(
    &Shared,
    &ThreadState,
    LoaderId,
    ObjRef,      // receiver; NULL for a static call
    &[Value],
) -> Result<Option<Value>, VmError>;
```

This is what lets `String.length`, `Class.getName`, `Field.get`, `Method.invoke` and the
stream/callback natives see `this` while the class files remain metadata-only.

## 2. Decode: instructions and exception ranges

[`crates/interp/src/instr.rs`](../../crates/interp/src/instr.rs) decodes the `Code`
payload into `DecodedMethod { instructions, handlers, max_stack, max_locals }`.

- A **two-pass** decode records every instruction's byte offset, then rewrites branch
  and switch targets (and exception-table `start_pc`/`end_pc`/`handler_pc`) to
  instruction indices. The dispatch loop never adds a byte offset to `pc`.
- The decoded instruction set is explicit (`Iconst`, `GetStatic`, `InvokeVirtual`,
  `InvokeDynamic`, `Athrow`, `TableSwitch`, …); an unknown opcode is a typed decode
  error, not a no-op.
- The exception table is decoded after the code array; a `catch_type` of 0 becomes a
  catch-all, a non-`Class` constant or a target that is not an instruction boundary is
  a decode error.
- `max_stack` is part of the frame contract: `Frame::push` rejects a stack that exceeds
  it.

## 3. Frame and roots

[`crates/interp/src/frame.rs`](../../crates/interp/src/frame.rs) is the only frame:

```
Frame {
    class: ClassId,
    locals: Vec<Value>,     // this (instance only) + arguments + temporaries
    stack: Vec<Value>,
    instructions: Vec<Instr>,
    handlers: Vec<ExceptionHandler>,
    pc: usize,
    max_stack: usize,
}
```

For an instance method, `receiver` is local 0 and `args` fill the following slots. For a
static method, `args` start at 0. A frame owns no heap object directly; it reports the
non-null references in locals and stack through `Frame::roots`, and the thread publishes
that snapshot at safepoint requests and before calls (`FrameRootsGuard`).

## 4. Dispatch of calls

The call opcodes share the machinery but resolve differently:

| Opcode | Resolution | Receiver |
| :--- | :--- | :--- |
| `invokestatic` | Owner from the current loader; static entry. | none (`NULL` for natives) |
| `invokevirtual` | Receiver's runtime class, then its superclass chain. | instance |
| `invokespecial` | The instruction's owner, **not** the receiver's class, so constructors and superclass calls stay non-virtual. | instance |
| `invokeinterface` | Runtime class first, then a default method in the instruction's interface hierarchy; lambdas are handled before class resolution. | instance |

Before a callee runs, the caller publishes the pushed receiver/arguments as roots, so
the callee cannot lose them. A result is pushed unless the descriptor returns `V`.
`invokeinterface` also checks assignability and rejects a missing abstract method with a
typed error.

`invokedynamic` has two current call-site families:

- `StringConcatFactory` → [`crates/vm/src/string_concat.rs`](../../crates/vm/src/string_concat.rs)
  builds the string from the recipe and arguments;
- `LambdaMetafactory` → [`crates/interp/src/lambda.rs`](../../crates/interp/src/lambda.rs)
  creates a `LambdaObject` (interface, SAM, target, captured values); an interface call
  on it runs the target through the same static/instance machinery.

`invokeinterface` on a lambda that does not match the SAM still allows a concrete
interface method (for example `Comparator.reversed`); an abstract non-SAM method remains
a typed lambda error.

## 5. Exceptions

There are two distinct paths:

- A **VM error** (`InterpError::Vm`, `Decode`, `NotAssignable`, …) propagates as a Rust
  `Result` error; it is not a Java throwable and is never caught by a Java handler.
- A **Java throwable** is `InterpError::Thrown(ObjRef)`.

`athrow` pops the reference; null and non-reference values are typed errors in this
slice. For a real reference, `handle_thrown` searches the current frame's exception
table in class-file order for an entry whose `[start, end)` range contains the throwing
instruction and whose catch type is assignable from the throwable's runtime class. A
catch-all matches everything. On a match it clears the operand stack, pushes the same
throwable and jumps to the handler; otherwise `Thrown` leaves the frame.

Calls use `deliver_call`: a callee's `Thrown` is routed through the caller's table at the
call instruction, and re-raised unchanged if the caller has no handler. This is why a
throw can cross several frames while preserving the same object. Native functions return
`VmError::Thrown`, which `InterpError::from` converts to the same path.

The current exception support does not synthesize `NullPointerException` for a null
`athrow`, does not produce stack traces, and does not duplicate `finally` blocks. The
throwable side table does record the message, the cause and suppressed exceptions, so
`printStackTrace()` prints the header, a `Caused by:` chain and `Suppressed:` entries even
though every trace lacks `at …` frame lines; `addSuppressed`/`getSuppressed` round-trip.

## 6. Safepoints and re-entry

Each instruction begins with `thread.poll_with(|| guard.publish(&frame.roots()))`: a
pending request publishes a fresh snapshot before it is acknowledged. There is no
stop-the-world collector yet; the handshake is the contract a later collector uses.

Reflection and streams re-enter the interpreter through the same entry points rather
than looping in Rust alone:

- `Method.invoke`/`Constructor.newInstance` in
  [`crates/interp/src/reflect_invoke.rs`](../../crates/interp/src/reflect_invoke.rs)
  unbox arguments, call `run_static_method`/`run_instance_method`, and box the result.
- Streams and callbacks in [`crates/interp/src/streams.rs`](../../crates/interp/src/streams.rs)
  and [`callbacks.rs`](../../crates/interp/src/callbacks.rs) invoke a Java
  `Function`/`Predicate`/`Consumer`/`Comparator` through the lambda SAM path.
- A Java exception raised inside such a callback crosses the native ABI as
  `VmError::Thrown`, then becomes `InterpError::Thrown` for the caller's handler search.

This re-entry is why the runner explicitly registers the interpreter-owned natives in
addition to `library::register_all`.

## 7. Evidence

```console
$ cargo test -p rava-interp --test arithmetic
$ cargo test -p rava-interp --test instance_execution
$ cargo test -p rava-interp --test arrays_control
$ cargo test -p rava-interp --test exceptions
$ cargo test -p rava-interp --test class_literals
$ cargo test -p rava-interp --test invokedynamic_concat
$ cargo test -p rava-interp --test lambdas
$ cargo test -p rava-interp --test streams
$ RAVA_SPRING_ACCEPTANCE=1 cargo test -p rava-cli --test spring_context
```

The Spring smoke is the integration proof: the context runs through decoded IR, native
overrides, reflection, lambdas and streams and reaches the `hello` bean. Its path is
documented in [`spring-context.md`](spring-context.md).

## 8. Boundaries

- The interpreter decodes a broad but not complete JVM instruction set; `jsr`/`ret`,
  some `wide` forms, full `MethodHandle`/`VarHandle` and arbitrary `invokedynamic`
  bootstraps are not implemented.
- Locals are JVM slots: a `long`/`double` value stored in a local occupies two, and the
  second is a hole that no load may read. A load of a hole is a typed interpreter error;
  the verifier counts slots but does not type-check local loads.
- `athrow` does not synthesize NPE/ClassCastException/ArrayStoreException objects;
  those paths are typed interpreter errors.
- `finally`, stack traces and `try-with-resources` lowering are not special-cased;
  only the exception table and `athrow` are executed.
- There is no JIT and no class redefinition. A later JIT consumes the same decoded IR
  and frame contract, not a second representation.

## Owners

- [`crates/interp/src/lib.rs`](../../crates/interp/src/lib.rs) — execution, calls, exception routing, native dispatch.
- [`crates/interp/src/instr.rs`](../../crates/interp/src/instr.rs) — decode and branch/handler targets.
- [`crates/interp/src/frame.rs`](../../crates/interp/src/frame.rs) — frame and roots.
- [`crates/interp/src/lambda.rs`](../../crates/interp/src/lambda.rs) — metafactory/SAM execution.
- [`crates/interp/src/{callbacks,streams,reflect_invoke}.rs`](../../crates/interp/src/) — re-entry natives.
- [`crates/vm/src/string_concat.rs`](../../crates/vm/src/string_concat.rs) — `StringConcatFactory`.
- [ADR-0010](../adr/0010-execution-and-performance.md) — execution/IR boundaries.
