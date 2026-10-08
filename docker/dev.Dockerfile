# The Linux toolchain the PR gate runs, for building, linting and testing without installing
# anything on the host. `scripts/dev-container.sh` is the way in; it mounts the checkout and keeps
# the cargo registry and `target/` in named volumes, so nothing it builds lands in the tree.
#
# Headless: there is no display, so the app itself doesn't run here. Everything the gate runs does.
FROM rust:1.97.0-bookworm

# `.github/actions/linux-system-deps`'s base set, plus gettext for
# `scripts/update-translations.sh` and fonttools for `scripts/subset-icon-fonts.sh`.
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        libasound2-dev libxkbcommon-dev libfontconfig1-dev libfreetype-dev \
        libwayland-dev libssl-dev pkg-config \
        gettext python3-fonttools \
    && rm -rf /var/lib/apt/lists/*

# `.github/actions/headless-audio`: the default output opens on alsa-lib's userspace null device,
# which `crates/melodia/tests/headless.rs` needs and a container has no card for.
RUN printf 'pcm.!default { type null }\nctl.!default { type null }\n' > /etc/asound.conf

# Baked in so the first run doesn't download the pinned toolchain and its components.
COPY rust-toolchain.toml /tmp/toolchain/rust-toolchain.toml
RUN cd /tmp/toolchain && rustup show active-toolchain && rm -rf /tmp/toolchain

# The extractor parses with the Slint compiler, so its version has to equal the workspace's.
ARG SLINT_VERSION
RUN test -n "$SLINT_VERSION" \
    && cargo install slint-tr-extractor --version "$SLINT_VERSION" --locked \
    && rm -rf "$CARGO_HOME/registry"

# The `test` job's memory cap (`.claude/rules/ci-packaging.md`), lowered again because Docker
# Desktop's VM is usually smaller than the runner's 16 GB.
ENV CARGO_BUILD_JOBS=2 \
    CARGO_PROFILE_DEV_DEBUG=line-tables-only \
    CARGO_PROFILE_TEST_DEBUG=line-tables-only \
    CARGO_TARGET_DIR=/cargo-target

WORKDIR /workspace
