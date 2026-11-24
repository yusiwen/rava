#!/usr/bin/env bash
# Rebuilds the committed M2.1 metadata fixtures in ../demo.
#
# The class files are committed, so `cargo test` needs no JDK at test time. Re-run this
# only when a source file changes, then regenerate the golden dumps and read the diff:
#
#   ./build.sh
#   RAVA_UPDATE_GOLDEN=1 cargo test -p rava-classfile --test metadata_model
#
# `--release 17` pins class-file version 61 (and the Java 17 language level records and
# sealed types need); `-parameters` is what makes MethodParameters carry real names.
set -euo pipefail
cd "$(dirname "$0")"

rm -rf ../demo
javac --release 17 -parameters -Xlint:none -d .. demo/*.java
