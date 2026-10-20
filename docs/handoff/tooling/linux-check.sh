#!/usr/bin/env bash
# Linux x64 compile + test of the workspace inside a Debian container (session 6, release gate "Linux").
# /src is the repository mounted read-only; work happens in /work; target and cargo caches are volumes.
set -u
export DEBIAN_FRONTEND=noninteractive
export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-1.99.0}"
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
log() { echo "== $(date -u +%H:%M:%S) $*"; }

log "apt dependencies"
apt-get update -qq >/dev/null
apt-get install -y -qq --no-install-recommends \
  libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev \
  libssl-dev pkg-config build-essential file git python3 ca-certificates curl xvfb xauth dbus-x11 >/dev/null
apt-get install -y -qq --no-install-recommends python-is-python3 python3-venv >/dev/null
# The pinned uv (crates/infrastructure/src/environment/pins.rs UV_LINUX), checked against its SHA-256.
curl -fsSL -o /tmp/uv.tgz https://github.com/astral-sh/uv/releases/download/0.13.1/uv-x86_64-unknown-linux-gnu.tar.gz
echo "eadd9d27c134ab5ac1ffd4473b97a741407f3ea238288f03121fc4deddf90220  /tmp/uv.tgz" | sha256sum -c -
tar -xzf /tmp/uv.tgz -C /tmp && install -m 0755 /tmp/uv-x86_64-unknown-linux-gnu/uv /usr/local/bin/uv
git --version
python --version
uv --version
pkg-config --modversion webkit2gtk-4.1

log "copy source"
rm -rf /work && mkdir -p /work
(cd /src && tar --exclude=./target --exclude=./target-gnu --exclude=./ui/node_modules \
  --exclude=./docs/handoff/patches --exclude=./.git -cf - .) | (cd /work && tar xf -)
cd /work
# The checked-in toolchain file names the Windows MSVC host; use the same Rust version for Linux.
rm -f rust-toolchain.toml
rustup toolchain install "$RUSTUP_TOOLCHAIN" --profile minimal -c clippy -c rustfmt >/dev/null 2>&1 || true
rustc --version
cargo --version
git config --global user.email "linux-check@example.invalid"
git config --global user.name "linux-check"

log "cargo build --workspace --all-targets"
cargo build --workspace --all-targets --locked -j 10 2>&1 | tail -40
echo "BUILD_EXIT=${PIPESTATUS[0]}"

log "cargo clippy -D warnings"
cargo clippy --workspace --all-targets --locked -j 10 -- -D warnings 2>&1 | tail -60
echo "CLIPPY_EXIT=${PIPESTATUS[0]}"

log "cargo test --workspace"
xvfb-run -a cargo test --workspace --locked -j 10 --no-fail-fast 2>&1 | tee /tmp/test.log | grep -E "^test result|FAILED|panicked|^failures:|^    [a-z_:]+$" | tail -120
echo "TEST_EXIT=${PIPESTATUS[0]}"
awk '/^test result/ {split($0,a," "); p+=a[4]; f+=a[6]; i+=a[8]} END {print "TOTAL passed="p" failed="f" ignored="i}' /tmp/test.log
cp /tmp/test.log /out/linux-test.log 2>/dev/null || true
log "done"
