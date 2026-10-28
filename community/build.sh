#!/usr/bin/env bash
# Rebuilds the packages and the signed index of this source from `skills/*.md`.
#
#   community/build.sh ~/.config/warden/curator.key [path/to/warden-server]
#
# The key is the private half of `PUBLIC_KEY`; it never goes in this repository. The CI cannot run this (it has no key): it only checks
# that what is committed here agrees with itself (`crates/warden-server/tests/community.rs`). Run this after changing any skill, then
# commit `packages/` and `index.json` together with the skill.
#
# Taking a package back: add `--revoke HASH=reason` to the `index build` line below (keep the old ones, the list is rebuilt each time).
set -euo pipefail

key="${1:?usage: community/build.sh KEY_FILE [warden-server]}"
server="${2:-warden-server}"
here="$(cd "$(dirname "$0")" && pwd)"

AUTHOR="Fabio Anjos Junior"
LICENSE="MIT"
VERSION="1.0.0"
NAME="Fabio"
BASE_URL="https://raw.githubusercontent.com/Arven-Labs/warden/main/community/packages"

public="$(tr -d '[:space:]' < "$here/PUBLIC_KEY")"
rm -f "$here"/packages/*.warden-skill.json
for skill in "$here"/skills/*.md; do
  name="$(basename "$skill" .md)"
  package="$here/packages/$name.warden-skill.json"
  "$server" package export "$skill" --version "$VERSION" --author "$AUTHOR" --license "$LICENSE" --out "$package"
  "$server" package sign "$package" --key "$key"
done

"$server" package index build "$here"/packages/*.warden-skill.json \
  --name "$NAME" --key "$key" --base-url "$BASE_URL" --out "$here/index.json"
"$server" package index verify "$here/index.json" --key "$public"
