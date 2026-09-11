#!/usr/bin/env bash
set -euo pipefail

fail() {
  printf 'ERROR: %s\n' "$*" >&2
  exit 1
}

require_tool() {
  command -v "$1" >/dev/null 2>&1 || fail "required tool '$1' is not installed"
}

for tool in git cargo rustc pkg-config; do
  require_tool "$tool"
done

[[ "$(uname -s)" == "Linux" ]] || fail "layout-runtime validation requires Linux"
pkg-config --exists alsa || fail "ALSA development files are missing"

RUSTC_VERSION="$(rustc --version | awk '{print $2}')"
CARGO_VERSION="$(cargo --version | awk '{print $2}')"
[[ "$RUSTC_VERSION" == 1.85.* ]] || fail "requires rustc 1.85.x; found $RUSTC_VERSION"
[[ "$CARGO_VERSION" == 1.85.* ]] || fail "requires cargo 1.85.x; found $CARGO_VERSION"

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"
HEAD_SHA="$(git rev-parse HEAD)"

printf 'Aurora wider-layout runtime validation\n'
printf 'head=%s\n' "$HEAD_SHA"

cargo generate-lockfile
cargo metadata --format-version 1 --locked >/dev/null
cargo fmt --all -- --check

cargo check --locked -p aurora-layout-playback-runtime --all-targets
cargo test --locked -p aurora-layout-playback-runtime
cargo clippy --locked -p aurora-layout-playback-runtime --all-targets -- -D warnings

cargo check --locked -p aurora-speaker-output --all-targets
cargo test --locked -p aurora-speaker-output
cargo clippy --locked -p aurora-speaker-output --all-targets -- -D warnings

cargo check --locked -p aurora-cli --no-default-features --features layout-runtime --bin aurora-layout-runtime
cargo test --locked -p aurora-cli --no-default-features --features layout-runtime --bin aurora-layout-runtime
cargo clippy --locked -p aurora-cli --no-default-features --features layout-runtime --bin aurora-layout-runtime -- -D warnings

if ! git diff --quiet -- Cargo.lock; then
  git diff --stat -- Cargo.lock >&2
  printf '\nCargo.lock is stale. Review and commit the resolver-generated Rust 1.85 lockfile, then rerun.\n' >&2
  exit 2
fi

printf '\nPASS Aurora wider-layout software validation head=%s\n' "$HEAD_SHA"
printf 'NOTE this does not prove physical eARC capture, TDM speaker order, XRUN behavior, acoustic latency, or commercial streaming interoperability.\n'
