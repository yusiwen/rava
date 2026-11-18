#!/usr/bin/env bash
# check-invariants.sh — the architecture rules that must not rot.
#
# Run from the repository root. Exit 0 means every invariant holds; exit 1 means a claim in
# the documentation has stopped being true. It never runs the test suite: the gate for that
# is `cargo xtask ci`.
set -uo pipefail

FAILS=0
pass() { printf 'PASS  %s\n' "$1"; }
fail() { FAILS=$((FAILS + 1)); printf 'FAIL  %s\n' "$1"; }

cd "$(dirname "$0")/.." || exit 2

# The five crates of ADR-0002, by name.
for crate in classfile vm interp native cli; do
    if [ -d "crates/$crate/src" ]; then
        pass "crate rava-$crate exists"
    else
        fail "crate rava-$crate is missing"
    fi
done

# Dependency direction: a crate may depend on the crates to its left, never to its right.
# The table is the ADR, so the check is the ADR's executable form.
check_deps() {
    local crate=$1 allowed=$2
    local bad=""
    local line
    while IFS= read -r line; do
        case "$line" in
            rava-classfile) dep=classfile ;;
            rava-vm) dep=vm ;;
            rava-interp) dep=interp ;;
            rava-native) dep=native ;;
            rava-cli) dep=cli ;;
            *) continue ;;
        esac
        case " $allowed " in
            *" $dep "*) ;;
            *) bad="$bad $dep" ;;
        esac
    done < <(sed -n '/^\[dependencies\]/,/^\[/p' "crates/$crate/Cargo.toml" | grep -o 'rava-[a-z]*' || true)
    if [ -z "$bad" ]; then
        pass "rava-$crate depends only on [$allowed]"
    else
        fail "rava-$crate depends on$bad, which the direction forbids"
    fi
}

check_deps classfile ""
check_deps vm "classfile"
check_deps interp "classfile vm"
check_deps native "classfile vm"
check_deps cli "classfile vm interp native"

if grep -rn 'unsafe' crates/*/src --include=*.rs | grep -v '^\S*:[0-9]*: *//' >/dev/null 2>&1; then
    fail "unsafe appears in crates/*/src (the VM is safe Rust throughout)"
else
    pass "no unsafe code in crates/"
fi

# The interpreter may not know class-file versions: policy lives in the loader (ADR-0008).
if grep -rni 'major_version\|minor_version\|class.file.version' crates/interp/src --include=*.rs >/dev/null 2>&1; then
    fail "crates/interp/src mentions class-file versions"
else
    pass "crates/interp/src is free of class-file version logic"
fi

if grep -q 'pub struct Shared' crates/vm/src/lib.rs && grep -q 'pub struct ThreadState' crates/vm/src/lib.rs; then
    pass "rava-vm defines struct Shared and struct ThreadState"
else
    fail "rava-vm no longer defines Shared and ThreadState"
fi

if grep -q 'pub fn register_jcl_natives' crates/native/src/lib.rs; then
    pass "rava-native exposes register_jcl_natives"
else
    fail "rava-native does not expose register_jcl_natives"
fi

# The project hosts the reference class library. A committed copy of some class library, or a
# corpus compiled against one, would mean the model has quietly changed (ADR-0001).
if [ -d stdlib ] || [ -d samples ]; then
    fail "a class library or corpus is committed at the root (the host supplies both)"
else
    pass "no owned class library or corpus is committed"
fi

printf '\n%d failure(s)\n' "$FAILS"
[ "$FAILS" -eq 0 ]
