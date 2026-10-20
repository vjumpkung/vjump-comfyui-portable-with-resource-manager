#!/usr/bin/env bash
# Container entry: install the .deb under test, start Xvfb + D-Bus + tauri-driver, then stay alive for wd.py commands.
set -u
DEB="${DEB:-$(ls /bundle/*.deb | head -1)}"
echo "== installing $DEB"
apt-get update -qq >/dev/null && apt-get install -y -qq "$DEB" 2>&1 | tail -3
dpkg -s vjumpkung-comfyui-desktop | grep -E '^(Version|Depends):'
export DISPLAY=:99
Xvfb :99 -screen 0 1400x900x24 -nolisten tcp >/out/xvfb.log 2>&1 &
sleep 1
eval "$(dbus-launch --sh-syntax)"
echo "DBUS_SESSION_BUS_ADDRESS=$DBUS_SESSION_BUS_ADDRESS" > /work/env
echo "DISPLAY=:99" >> /work/env
nvidia-smi --query-gpu=name,driver_version --format=csv,noheader 2>&1 | head -2
tauri-driver --port 4444 >/out/tauri-driver.log 2>&1 &
echo "== ready"
sleep infinity
