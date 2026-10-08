#!/usr/bin/env bash
# Runs a command in the Linux dev container, building the image first if it is missing or stale.
#
#   scripts/dev-container.sh cargo clippy --all-targets --locked --workspace -- -D warnings
#   scripts/dev-container.sh cargo test --locked --workspace
#   scripts/dev-container.sh scripts/update-translations.sh
#   scripts/dev-container.sh            # an interactive shell
#
# Needs Docker and nothing else on the host.
set -euo pipefail

repo=$(cd "$(dirname "$0")/.." && pwd)

slint_version=$(sed -n 's/^slint = { version = "\([^"]*\)".*/\1/p' "$repo/Cargo.toml")
[ -n "$slint_version" ] || { echo "error: can't read slint's version from Cargo.toml" >&2; exit 1; }
toolchain=$(sed -n 's/^channel = "\([^"]*\)"/\1/p' "$repo/rust-toolchain.toml")
# Tagged by both pins, so a bump of either builds a fresh image rather than reusing a stale one.
image="melodia-dev:rust-$toolchain-slint-$slint_version"

if ! docker image inspect "$image" >/dev/null 2>&1; then
    docker build \
        --file "$repo/docker/dev.Dockerfile" \
        --build-arg "SLINT_VERSION=$slint_version" \
        --tag "$image" \
        "$repo"
fi

tty_flags=()
[ -t 0 ] && [ -t 1 ] && tty_flags=(--interactive --tty)

[ "$#" -gt 0 ] || set -- bash

# Spelled so bash 3.2, macOS's, doesn't read an empty array as unbound under `set -u`.
exec docker run --rm ${tty_flags[@]+"${tty_flags[@]}"} \
    --volume "$repo:/workspace" \
    --volume melodia-cargo-registry:/usr/local/cargo/registry \
    --volume melodia-cargo-git:/usr/local/cargo/git \
    --volume melodia-target:/cargo-target \
    "$image" "$@"
