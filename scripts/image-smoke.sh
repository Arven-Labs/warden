#!/usr/bin/env bash
# Teste de fumaça da imagem do hub (P143 e): sobe o contêiner com a raiz SÓ DE LEITURA e confere que ele funciona com
# só /data (volume) e /tmp (tmpfs) graváveis. Uso: scripts/image-smoke.sh [imagem]   (padrão: warden-server:smoke)
set -euo pipefail

IMAGE="${1:-warden-server:smoke}"
NAME="warden-smoke-$$"
PORT="${PORT:-17420}"
KEY="$(head -c 24 /dev/urandom | od -An -tx1 | tr -d ' \n')"
VOLUME="warden-smoke-data-$$"

cleanup() {
  docker rm -f "$NAME" >/dev/null 2>&1 || true
  docker volume rm "$VOLUME" >/dev/null 2>&1 || true
}
trap cleanup EXIT

fail() {
  echo "FALHOU: $*" >&2
  echo "---- log do contêiner ----" >&2
  docker logs "$NAME" >&2 2>&1 || true
  exit 1
}

docker volume create "$VOLUME" >/dev/null
docker run -d --name "$NAME" --read-only --tmpfs /tmp -v "$VOLUME":/data -p "$PORT":7420 \
  -e WARDEN_SERVER_AUTH_KEY="$KEY" -e GEMINI_API_KEY=smoke "$IMAGE" >/dev/null

# 1. /health responde sem chave.
up=""
for _ in $(seq 1 60); do
  if curl -fsS "http://127.0.0.1:$PORT/health" >/dev/null 2>&1; then up=1; break; fi
  [ "$(docker inspect -f '{{.State.Running}}' "$NAME")" = "true" ] || fail "o contêiner parou antes de responder /health"
  sleep 1
done
[ -n "$up" ] || fail "/health não respondeu em 60 s"
curl -fsS "http://127.0.0.1:$PORT/health" | grep -q '"status":"ok"' || fail "/health sem status ok"

# 2. /status exige a chave e devolve o estado com ela.
code="$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$PORT/status")"
[ "$code" = "401" ] || fail "/status sem chave deu $code, esperava 401"
body="$(curl -fsS -H "Authorization: Bearer $KEY" "http://127.0.0.1:$PORT/status")" || fail "/status com a chave falhou"
echo "$body" | grep -q '"rss_bytes"' || fail "/status sem rss_bytes: $body"
echo "status: $body"

# 3. O que o hub gravou foi para /data; a raiz continua só de leitura.
docker exec "$NAME" sh -c 'touch /etc/warden-write-test' 2>/dev/null && fail "a raiz do contêiner aceitou escrita"
docker exec "$NAME" sh -c 'ls -A /data | head -n 1' | grep -q . || echo "aviso: /data está vazio (o hub ainda não gravou nada)"

# 4. SIGTERM: sai com 0 e diz que parou.
docker stop -t 15 "$NAME" >/dev/null
exit_code="$(docker inspect -f '{{.State.ExitCode}}' "$NAME")"
[ "$exit_code" = "0" ] || fail "saiu com $exit_code depois do SIGTERM, esperava 0"
docker logs "$NAME" 2>&1 | grep -q 'stopped' || fail "o log não diz que o hub parou"

# 5. Nenhuma queixa de escrita no log inteiro.
if docker logs "$NAME" 2>&1 | grep -Ei 'read-only file system|permission denied'; then
  fail "o log tem erro de escrita: a raiz só de leitura quebrou alguma coisa"
fi

echo "OK: a imagem sobe com a raiz só de leitura, responde /health e /status e para com SIGTERM."
