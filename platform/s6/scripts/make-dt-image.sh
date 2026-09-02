#!/bin/sh
set -eu

ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)"
OUT="${AURORA_OUT:-$ROOT/out/s6-aarch64}"
TOOL="$OUT/tools/dtbhtoolExynos"
DTB_DIR="$OUT/kernel/dtbs"
DTIMG="$OUT/kernel/aurora-s6-dt.img"
PAGE_SIZE=2048

fail() { echo "make-dt-image: $*" >&2; exit 1; }
[ -x "$TOOL" ] || fail "dtbhtoolExynos missing; run build-samsung-boot-tools.sh"
[ -d "$DTB_DIR" ] || fail "kernel DTB directory missing; run build-kernel.sh"
COUNT="$(find "$DTB_DIR" -maxdepth 1 -type f -name '*zeroflte*.dtb' | wc -l | tr -d ' ')"
[ "$COUNT" -gt 0 ] || fail "no zeroflte DTBs found"

rm -f "$DTIMG"
"$TOOL" --dt_dir "$DTB_DIR" -s "$PAGE_SIZE" -o "$DTIMG"
[ -s "$DTIMG" ] || fail "empty DT image produced"

# Samsung DTBH images begin with the literal DTBH magic.
MAGIC="$(dd if="$DTIMG" bs=1 count=4 2>/dev/null || true)"
[ "$MAGIC" = "DTBH" ] || fail "unexpected DT image magic: ${MAGIC:-none}"

sha256sum "$DTIMG" > "$DTIMG.sha256"
{
  echo "format=samsung-dtbh-v2"
  echo "page_size=$PAGE_SIZE"
  echo "dtb_count=$COUNT"
  echo "source_dir=$DTB_DIR"
} > "$OUT/kernel/DT-IMAGE-MANIFEST.txt"

echo "Samsung DT image built: $DTIMG ($COUNT zeroflte DTBs)"
