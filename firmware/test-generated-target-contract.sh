#!/bin/sh
set -eu

ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
TMPDIR_ROOT="$(mktemp -d)"
trap 'rm -rf "$TMPDIR_ROOT"' EXIT HUP INT TERM

HEADER="$TMPDIR_ROOT/aurora_hw_target_generated.h"
OBJECT="$TMPDIR_ROOT/aurora_target_contract.o"

sh "$ROOT/firmware/generate-hardware-target-header.sh" "$HEADER"

cc -std=c11 -O2 -Wall -Wextra -Werror \
    -I"$TMPDIR_ROOT" \
    -c "$ROOT/firmware/hal/aurora_target_contract.c" \
    -o "$OBJECT"

test -s "$OBJECT"
echo "generated hardware target contract compile passed"
