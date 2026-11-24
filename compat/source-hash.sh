#!/usr/bin/env bash
#
# Print the deterministic sha256 of the reference application source tree.
#
# The hash covers reference-app/pom.xml (parent and dependency pins) and every file
# under reference-app/src/. Build output is excluded, so a rebuild does not move it.
# The value is pinned as [reference_app].source_sha256 in compat/targets.toml.
#
#   bash compat/source-hash.sh
#
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT"

find reference-app/pom.xml reference-app/src -type f -print0 \
  | LC_ALL=C sort -z \
  | xargs -0 sha256sum \
  | sha256sum \
  | cut -d' ' -f1
