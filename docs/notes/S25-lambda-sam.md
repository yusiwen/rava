# S25 — Lambda dispatch is SAM-only, and three enablers the regex path needed

- **Slice:** S25
- **Requirements:** R1 (execute a class file end to end: dispatch semantics)
- **Acceptance:** `cargo test -p rava-cli --test lambdas` exited 0 on 2026-11-20 — a
  `javac`-compiled program with a lambda, an interface default, a `synchronized static` method,
  `String.intern` and `Class.forName` is compared with `java` byte for byte (7 lines).

## What surprised me

**A lambda was dispatched for *every* call on the object.** The interception ran the captured
implementation whenever the receiver was a `HeapObject::Lambda`, regardless of which interface
method was invoked. `Pattern`'s parser calls `CharPredicate.union(...)` — a **default** method —
on a `Range` lambda whose implementation takes three ints; the VM fed it the union argument (a
reference) and `inRange` failed with `expected int, found Ref(ObjRef(...))`. This was #37's
symptom, and it was a lambda dispatch bug, not a `Formatter` bug.

The fix is the standard model: a lambda implements one interface method (its SAM). The lambda
object now carries:

- the functional **interface** (the call site's return type, `()L...CharPredicate;`) — which is
  also its runtime class, so `class_of` answers it and interface default methods resolve;
- the **SAM name and descriptor** (the `invokedynamic`'s name and the bootstrap's
  `samMethodType`).

Interception happens only when the invoked method matches the SAM; everything else dispatches
normally. The default method's body then calls the SAM on `this` (the lambda), which does
intercept — the intended path.

**Three enablers came out of the same probe**: `Pattern` → `Locale` reached
`synchronized static` (`Locale.getFormatLocale`), `String.intern`, and `Class.forName`. The
first locks the class object's monitor (JVMS §2.11.10 — the VM has had class objects since S9);
`String.intern` is the Java face of the VM's intern table; `Class.forName0` loads from the host
image and runs `<clinit>` through the interpreter's call seam.

## Where `printf` stands

With the regex wall gone, `printf` reaches `Formatter` → `DecimalFormatSymbols` →
`LocaleProviderAdapter` → `ServiceLoader` → `Class.getDeclaredConstructors0`. Reflection is not
modelled (M2/M4), so the locale providers cannot be instantiated. A temporary empty-array stub
made the library throw `ServiceConfigurationError`, which is worse than the missing native, so it
was removed. `printf`/`String.format` remain blocked on **reflection** (#37, #43), not on
`Formatter` logic.

## Measurements

| Fact | Value |
| :--- | ---: |
| Differential test | 1 test, 7 output lines (lambda SAM, default method, synchronized static, intern, forName, clinit side effect) |
| Workspace tests | 118 |
| Before / after | `Pattern.compile` of the Formatter pattern: `expected int, found Ref` → compiles |

## Open threads

- **Reflection** (`getDeclaredConstructors0` and the `Constructor` surface) is the next wall for
  `printf`/`String.format` and for locale providers; #43.
- `Class.forName` ignores loaders (bootstrap only); array names are accepted in descriptor form.
- The lambda's captures are copied into the `Lambda` object; a capture that is itself a lambda
  works, but `altMetafactory`'s markers are not interpreted.
