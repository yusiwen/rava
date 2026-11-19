# S4 — The image, and its two endiannesses

- **Slice:** S4
- **Requirements:** R3
- **Acceptance:** `cargo test -p rava-vm --test image` exited 0 on 2026-11-19 (9 tests, run
  inside `nix develop`; the image is `$JAVA21_HOME/lib/modules`).

## What surprised me

**The image is not a zip and not a jar.** It is a perfect-hash index plus four blobs: a
redirect table, an offsets table, a locations blob and a string table. A resource name is never
stored whole — it is split into module, parent, base and extension, each an offset into the
string table, and `getFullName` glues them back together. The redirect table is the interesting
part: a negative value is the index directly, a positive value is the seed of a second, final
hash. That is why a lookup is two multiplies, not a scan.

**The header and the attributes disagree about endianness.** The seven header words are
host-endian (`jlink` writes for the machine it targets), but the values inside a location record
are big-endian. A reader that picks one and applies it to both reads nonsense in exactly one
place; this one did, and the acceptance test caught it.

**The devShell exported a path that was not a JDK home.** `JAVA21_HOME` pointed at the nix
package root, where `java` is a wrapper on `PATH` and `lib/modules` does not exist; the runtime
image lives at `lib/openjdk/lib/modules`. The flake now exports `${jdk21.home}`, so the
variable means what its name says and CI and the tests agree.

## What the specification says

None of this is in the JVMS: the class-file format is specified, the container is not. The
format is an implementation detail of the JDK, described by `jdk.internal.jimage`
(`ImageHeader`, `ImageLocation`, `BasicImageReader`, `ImageStringsReader`), which ships inside
the image itself — the reader was written from those sources, not from a blog post. ADR-0008
records the choice: the image is the class source, and a reader we own keeps the dependency
graph flat.

The measured image:

| Property | Value |
| :--- | :--- |
| File size | 141,940,452 bytes (nix `openjdk-21.0.9+10`, the devShell's) |
| Resources | 30,722 (all index slots non-empty, all names distinct) |
| `java.base` resources | 7,572 |
| `java.base` classes | 7,559 |
| Class-file version inside | 65 (`0x41`), the ADR-0008 target |

`jimage list` agrees on both counts; that comparison was the independent check. A second
image on the same machine (Zulu `21.0.13`) carries 30,819 resources and 7,647 in `java.base`:
patch releases add classes, which is why the test asserts the shape (every name distinct, the
whole library present) and the note records the numbers, not the other way round.

## The bug I hit

**Content offsets are relative to the end of the index.** The first version of `resource` used
the stored offset as a file offset, so `java/lang/Object.class` came back as the bytes at
579,664 — inside the index, not the resource. The header, the enumeration and the hash lookup
were all correct —
which is why six tests passed and only the three that read a resource failed. The fix is one
addition, `index_size() + offset`; removing it again fails those same three tests (the mutation
check for this slice).

Two smaller ones: the negative redirect is stored as two's complement, so the index is
`-value - 1`, not `-value`; and the redirect table is indexed by `hash % table_length` where
the table length is 30,819 — not a power of two, so masking would have been wrong.

## Open threads

- **Compressed images are refused** (`ImageError::Compressed`). The reference images are
  uncompressed; `jlink --compress` images would need the JDK's deflate format, which is not on
  the critical path.
- **The whole file is held in memory** (140 MB). That is a deliberate S4 simplification; the
  seam for a lazy or mapped reader is `Image::resource`, not its callers.
- **The module is hardcoded to `java.base`** in `Image::class`. Every boot class is there, and
  the `(loader, module, name)` split is M2's problem.
- **The CLI still takes a directory.** Wiring `--home` to `lib/modules` belongs to the boot
  slice, where there is something to run; the test drives `Shared::load_class` directly.
- **The test needs `JAVA21_HOME`.** Outside `nix develop` it fails with a message rather than
  skipping, because a skipped acceptance is a passing one in CI.
