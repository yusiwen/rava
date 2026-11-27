#!/usr/bin/env bash
# Rebuild the reference-app jar from clean and compare it byte-for-byte with the locked
# copy fetch-jars.sh installed. This is the CI determinism proof: the lock checksum is only
# meaningful if a clean rebuild produces the same bytes.
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
TARGETS="$ROOT/compat/targets.toml"
JARS="$ROOT/compat/jars"

[[ -n "${JAVA_HOME:-}" ]] || {
  echo "error: JAVA_HOME must point at the build JDK" >&2
  exit 2
}
[[ -f "$TARGETS" ]] || {
  echo "error: missing compat/targets.toml" >&2
  exit 2
}
version=$(awk '
  /^\[reference_app\]/ { in_section = 1; next }
  /^\[/ { in_section = 0 }
  in_section && /^version[[:space:]]*=/ {
    sub(/^version[[:space:]]*=[[:space:]]*"/, "")
    sub(/".*$/, "")
    print
    exit
  }
' "$TARGETS")
[[ -n "$version" ]] || {
  echo "error: compat/targets.toml has no reference_app.version" >&2
  exit 2
}
jar="reference-app-$version.jar"
[[ -f "$JARS/$jar" ]] || {
  echo "error: $JARS/$jar is missing; run: bash compat/fetch-jars.sh" >&2
  exit 1
}
MVN=${MVN:-mvn}
command -v "$MVN" >/dev/null 2>&1 || {
  echo "error: mvn not found; set MVN=/path/to/mvn" >&2
  exit 2
}

"$MVN" -B -ntp -f "$ROOT/reference-app/pom.xml" clean package -DskipTests
cmp "$ROOT/reference-app/target/$jar" "$JARS/$jar"
echo "ok: a clean rebuild reproduces the locked reference-app jar ($jar)"
