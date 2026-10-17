#!/usr/bin/env bash
# Confere que o .deb do release instala num Ubuntu limpo e que o app abre (GUIA-DE-USO.md, caminho A).
# Uso: scripts/deb-check.sh <arquivo.deb>   (precisa de docker). Roda dentro de um ubuntu:24.04 sem nada do Warden.
set -euo pipefail

DEB="$(realpath "${1:?uso: deb-check.sh <arquivo.deb>}")"

docker run --rm -v "$DEB":/pkg/warden.deb:ro ubuntu:24.04 bash -euo pipefail -c '
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
# O guia manda `sudo apt install ./Warden_*.deb`: se as dependências declaradas não bastassem, isto falharia ou o ldd abaixo diria.
apt-get install -y -qq ./pkg/warden.deb xvfb >/dev/null

pkg="$(dpkg-deb -f /pkg/warden.deb Package)"
bins="$(dpkg -L "$pkg" | grep -E "^/usr/bin/" || true)"
[ -n "$bins" ] || { echo "FALHOU: o pacote $pkg não instala nada em /usr/bin" >&2; exit 1; }
bin="$(echo "$bins" | head -n 1)"
echo "pacote: $pkg, executável: $bin"

if ldd "$bin" | grep "not found"; then
  echo "FALHOU: $bin tem bibliotecas que o pacote não puxou (acima)" >&2
  exit 1
fi
echo "ldd: ok"

# Abre o app numa tela virtual por 20 s. Se ele morrer antes, o código de saída não é 124 (o do timeout).
set +e
HOME=/tmp/guide-home timeout 20 xvfb-run -a "$bin" >/tmp/app.log 2>&1
code=$?
set -e
if [ "$code" != "124" ]; then
  echo "FALHOU: o app saiu com $code antes dos 20 s" >&2
  cat /tmp/app.log >&2
  exit 1
fi
echo "OK: o .deb instala num Ubuntu limpo e o app ficou de pé por 20 s."
'
