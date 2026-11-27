#!/usr/bin/env bash
# Report the Java/Maven toolchain that builds the pinned reference-app jar.
#
# The jar is reproducible across Java 17 and 25: reference-app/pom.xml stores entries
# uncompressed, fixes their timestamps and omits build-environment manifest entries. This
# check records the active versions and rejects a Java older than 17; the lock checksum is
# the byte-level gate.
set -euo pipefail

[[ -n "${JAVA_HOME:-}" ]] || {
  echo "error: JAVA_HOME must point at the build JDK" >&2
  exit 2
}
JAVA="$JAVA_HOME/bin/java"
[[ -x "$JAVA" ]] || {
  echo "error: $JAVA not found" >&2
  exit 2
}
version=$("$JAVA" -version 2>&1 | awk -F'"' '/version/ { print $2; exit }')
[[ -n "$version" ]] || {
  echo "error: cannot read the Java version from $JAVA" >&2
  exit 2
}
major=${version%%.*}
case "$major" in
  '' | *[!0-9]*)
    echo "error: unexpected Java version: $version" >&2
    exit 2
    ;;
esac
if ((major < 17)); then
  echo "error: Java $version is too old; the reference app targets 17+" >&2
  exit 1
fi
MVN=${MVN:-mvn}
command -v "$MVN" >/dev/null 2>&1 || {
  echo "error: mvn not found; set MVN=/path/to/mvn" >&2
  exit 2
}
echo "java:  $version ($JAVA_HOME)"
"$MVN" -v | sed -n '1p' | sed 's/^/maven: /'
echo "note: the app jar is byte-reproducible across Java 17 and 25; the lock checksum is the gate"
