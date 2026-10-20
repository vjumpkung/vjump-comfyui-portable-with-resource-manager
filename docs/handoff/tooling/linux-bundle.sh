#!/usr/bin/env bash
# Linux x64 release bundle (deb, rpm, AppImage) of the desktop app inside a Debian container.
# /src = repository (read-only), /out = artifacts, /target + cargo registry = cache volumes.
set -u
export DEBIAN_FRONTEND=noninteractive
export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-1.99.0}"
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export CI=true
# No FUSE inside the container: linuxdeploy/appimagetool must extract themselves.
export APPIMAGE_EXTRACT_AND_RUN=1
NODE_VERSION=24.13.0
log() { echo "== $(date -u +%H:%M:%S) $*"; }

log "apt dependencies"
apt-get update -qq >/dev/null
apt-get install -y -qq --no-install-recommends \
  libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev \
  libssl-dev pkg-config build-essential file git ca-certificates curl xz-utils xdg-utils desktop-file-utils >/dev/null

log "node $NODE_VERSION + pnpm"
cd /tmp
curl -fsSLO "https://nodejs.org/dist/v$NODE_VERSION/node-v$NODE_VERSION-linux-x64.tar.xz"
curl -fsSL "https://nodejs.org/dist/v$NODE_VERSION/SHASUMS256.txt" | grep " node-v$NODE_VERSION-linux-x64.tar.xz\$" | sha256sum -c -
tar -xJf "node-v$NODE_VERSION-linux-x64.tar.xz" -C /usr/local --strip-components=1
node --version
npm install -g --silent pnpm@10.24.0 >/dev/null
pnpm --version

log "copy source"
rm -rf /work && mkdir -p /work
(cd /src && tar --exclude=./target --exclude=./target-gnu --exclude=./ui/node_modules --exclude=./ui/dist \
  --exclude=./docs/handoff/patches --exclude=./.git -cf - .) | (cd /work && tar xf -)
cd /work
rm -f rust-toolchain.toml
rustup toolchain install "$RUSTUP_TOOLCHAIN" --profile minimal >/dev/null 2>&1 || true
rustc --version

log "pnpm install"
pnpm -C ui install --frozen-lockfile 2>&1 | tail -5

log "tauri build (release bundles)"
pnpm -C ui tauri build 2>&1 | tail -60
echo "BUNDLE_EXIT=${PIPESTATUS[0]}"

log "artifacts"
mkdir -p /out/bundle
find /target/release/bundle -maxdepth 2 -type f \( -name '*.deb' -o -name '*.rpm' -o -name '*.AppImage' \) -print -exec cp -f {} /out/bundle/ \;
cp -f /target/release/vjumpkung-comfyui-desktop /out/bundle/ 2>/dev/null || true
(cd /out/bundle && ls -la && sha256sum *)
log "done"
