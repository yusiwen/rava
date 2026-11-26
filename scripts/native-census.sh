#!/usr/bin/env bash
# native-census.sh — measure which registered natives the Boot gate actually calls.
#
# Usage: scripts/native-census.sh
#
# Runs the M4.5 non-web Boot fixture on the pinned jars with the debug-only
# `RAVA_NATIVE_CENSUS` hook enabled and summarises the native table: how many natives are
# registered, how many are called, and which owners contribute the uncalled ones. The
# numbers are the baseline `docs/explanation/class-loading-and-libraries.md` §5e records.
#
# Needs `compat/jars/` (`bash compat/fetch-jars.sh`) and a debug build of the CLI; release
# builds do not carry the census hook.
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
JARS="$ROOT/compat/jars"

[[ -d "$JARS" ]] || {
  echo "error: compat/jars is absent; run: bash compat/fetch-jars.sh" >&2
  exit 1
}

LOG=$(mktemp "${TMPDIR:-/tmp}/rava-native-census.XXXXXX.tsv")
OWNERS="$LOG.owners"
trap 'rm -f "$LOG" "$OWNERS"' EXIT

CLASSPATH="$ROOT/crates/cli/tests/fixtures/boot_nonweb/classes"
for jar in "$JARS"/*.jar; do
  CLASSPATH="$CLASSPATH:$jar"
done

cargo build --quiet --manifest-path "$ROOT/Cargo.toml" -p rava-cli
(
  cd "$ROOT"
  RAVA_NATIVE_CENSUS="$LOG" target/debug/rava-cli run demo.BootNonWeb \
    --stdlib stdlib/c1/classes --cp "$CLASSPATH" >/dev/null
)

awk -F'\t' '
  $1 == "REG" {
    key = $2 "\t" $3 "\t" $4
    if (!(key in registered)) {
      registered[key] = 1
      registered_count++
    }
  }
  $1 == "CALL" && $5 == "hit" {
    key = $2 "\t" $3 "\t" $4
    if (!(key in called)) {
      called[key] = 1
      called_count++
    }
    calls++
  }
  END {
    for (key in registered) {
      if (!(key in called)) {
        uncalled_count++
        split(key, parts, "\t")
        uncalled_owner[parts[1]]++
      }
    }
    printf "registered natives: %d\n", registered_count
    printf "distinct natives called: %d\n", called_count
    printf "registered but not called: %d\n", uncalled_count
    printf "native calls: %d\n", calls
    for (owner in uncalled_owner) {
      printf "%d\t%s\n", uncalled_owner[owner], owner > "'"$OWNERS"'"
    }
  }
' "$LOG"

echo "top owners of uncalled natives:"
sort -rn "$OWNERS" | head -10 | while read -r count owner; do
  printf '  %6s %s\n' "$count" "$owner"
done
