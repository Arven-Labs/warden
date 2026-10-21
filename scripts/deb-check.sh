#!/usr/bin/env bash
# Checks that the release .deb installs on a clean Ubuntu and that the app opens (GUIA-DE-USO.md, path A).
# Usage: scripts/deb-check.sh <file.deb>   (needs docker). Runs inside an ubuntu:24.04 with nothing of Warden's.
set -euo pipefail

DEB="$(realpath "${1:?usage: deb-check.sh <file.deb>}")"

docker run --rm -v "$DEB":/pkg/warden.deb:ro ubuntu:24.04 bash -euo pipefail -c '
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
# The guide says `sudo apt install ./Warden_*.deb`: if the declared dependencies were not enough, this would fail or the ldd below would say so.
apt-get install -y -qq ./pkg/warden.deb xvfb >/dev/null

pkg="$(dpkg-deb -f /pkg/warden.deb Package)"
bins="$(dpkg -L "$pkg" | grep -E "^/usr/bin/" || true)"
[ -n "$bins" ] || { echo "FAILED: package $pkg installs nothing in /usr/bin" >&2; exit 1; }
bin="$(echo "$bins" | head -n 1)"
echo "package: $pkg, executable: $bin"
# The v0.1.1 release still installs `desktop`; from the next tag on the name is `warden-desktop` (tauri.conf.json).
case "$bin" in /usr/bin/desktop|/usr/bin/warden-desktop) ;; *) echo "FAILED: unexpected executable: $bin" >&2; exit 1 ;; esac

if ldd "$bin" | grep "not found"; then
  echo "FAILED: $bin has libraries the package did not pull in (above)" >&2
  exit 1
fi
echo "ldd: ok"

# Opens the app on a virtual screen for 20 s. If it dies earlier, the exit code is not 124 (the one from timeout).
set +e
HOME=/tmp/guide-home timeout 20 xvfb-run -a "$bin" >/tmp/app.log 2>&1
code=$?
set -e
if [ "$code" != "124" ]; then
  echo "FAILED: the app exited with $code before the 20 s" >&2
  cat /tmp/app.log >&2
  exit 1
fi
echo "OK: the .deb installs on a clean Ubuntu and the app stayed up for 20 s."
'
