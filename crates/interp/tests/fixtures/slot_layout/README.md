# Slot-layout fixture

`demo.SlotLayoutProbe` is the compiled probe for JVM local-slot layout. Its methods put a
wide value before a narrower parameter, so a frame that binds arguments compactly loads
the wrong slot:

| Method | Descriptor | Loaded from |
| :--- | :--- | :--- |
| `shift(long, int)` | `(JI)J` | `lload 0`, `iload 2` |
| `scale(long, double)` | `(JD)I` | `lload 0`, `dload 2` |
| `blend(long, float, int)` | `(JFI)J` | `lload 0`, `fload 2`, `iload 3` |
| `instanceShift(long, int)` | `(JI)J` (instance) | `lload 1`, `iload 3` |

The source and the compiled class are committed, so the test needs no JDK. Rebuild it with
a release-17 compiler:

```sh
javac --release 17 \
  -d crates/interp/tests/fixtures/slot_layout/classes \
  crates/interp/tests/fixtures/slot_layout/src/demo/SlotLayoutProbe.java
```

Running it on JDK 17 prints:

```text
shift(1, 40) = 1099511627776
shift(3, 2) = 12
scale(4, 2.5) = 10
scale(3, -1.5) = -4
blend(10, 2.5, 4) = 20
blend(1, 0.5, 3) = 2
instanceShift(1, 40) = 1099511627776
instanceShift(5, 3) = 40
```

`crates/interp/tests/slot_layout.rs` asserts those values through `run_static_method` and
`run_instance_method`.
