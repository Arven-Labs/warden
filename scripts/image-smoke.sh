#!/usr/bin/env bash
# Teste de fumaça da imagem do hub (P143 e): sobe o contêiner com a raiz SÓ DE LEITURA e confere que ele funciona com
# só /data (volume) e /tmp (tmpfs) graváveis: parado, e depois com um turno de verdade contra um modelo de mentira
# (scripts/fake-openai.py), que também faz a busca por significado baixar o modelo para /data/models.
# Uso: scripts/image-smoke.sh [imagem]   (padrão: warden-server:smoke). Precisa de docker, curl e python3.
set -euo pipefail

IMAGE="${1:-warden-server:smoke}"
NAME="warden-smoke-$$"
PORT="${PORT:-17420}"
KEY="$(head -c 24 /dev/urandom | od -An -tx1 | tr -d ' \n')"
VOLUME="warden-smoke-data-$$"
FAKE_PORT="${FAKE_PORT:-17421}"
FAKE_PID=""

cleanup() {
  [ -z "$FAKE_PID" ] || kill "$FAKE_PID" >/dev/null 2>&1 || true
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

# O modelo de mentira roda aqui, no hospedeiro; o contêiner o alcança pelo gateway da rede padrão do Docker (um endereço
# privado, o único que o hub aceita em http simples além de localhost).
GATEWAY="$(docker network inspect bridge -f '{{(index .IPAM.Config 0).Gateway}}')"
python3 "$(dirname "$0")/fake-openai.py" "$FAKE_PORT" &
FAKE_PID=$!

docker volume create "$VOLUME" >/dev/null
# A configuração entra no volume antes do hub subir, como o usuário do contêiner (o mesmo que vai ler).
docker run --rm -i --entrypoint sh -v "$VOLUME":/data "$IMAGE" -c 'cat > /data/config.toml' <<EOF
active_provider = "main"

[[providers]]
id = "main"
kind = "openai"
api_key = "smoke"
base_url = "http://$GATEWAY:$FAKE_PORT/v1"
model = "fake"
EOF
docker run -d --name "$NAME" --read-only --tmpfs /tmp -v "$VOLUME":/data -p "$PORT":7420 \
  -e WARDEN_SERVER_AUTH_KEY="$KEY" "$IMAGE" >/dev/null

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

# 4. Um turno de verdade pela API, contra o modelo de mentira, com uma nota no cofre: grava a chave da API, o cofre e o
#    índice em /data e, na primeira busca por significado, baixa o modelo de busca para /data/models.
docker exec "$NAME" sh -c 'mkdir -p /data/vault && echo "The rotation of the signing keys happens every Friday." > /data/vault/keys.md' \
  || fail "não consegui gravar uma nota no cofre em /data"
API_KEY="$(docker exec "$NAME" warden-server --data-dir /data api-keys create smoke 2>/dev/null)" || fail "api-keys create falhou"
[ -n "$API_KEY" ] || fail "api-keys create não devolveu a chave"
reply="$(curl -fsS --max-time 600 -H "Authorization: Bearer $API_KEY" -H 'Content-Type: application/json' \
  -d '{"model":"warden","messages":[{"role":"user","content":"When are the signing keys rotated?"}]}' \
  "http://127.0.0.1:$PORT/v1/chat/completions")" || fail "o turno pela API falhou"
echo "$reply" | grep -q 'pong' || fail "o turno não trouxe a resposta do modelo de mentira: $reply"
echo "turno: ok"
docker exec "$NAME" sh -c 'ls -A /data/models 2>/dev/null | head -n 1' | grep -q . \
  || fail "o modelo de busca não foi baixado para /data/models (a busca por significado não rodou ou não gravou)"
echo "modelo de busca em /data/models: ok"
body="$(curl -fsS -H "Authorization: Bearer $KEY" "http://127.0.0.1:$PORT/status")"
echo "status depois do turno: $body"
echo "$body" | grep -q '"in_flight":0' || fail "o turno acabou mas /status ainda diz que há trabalho: $body"

# 5. SIGTERM: sai com 0 e diz que parou.
docker stop -t 15 "$NAME" >/dev/null
exit_code="$(docker inspect -f '{{.State.ExitCode}}' "$NAME")"
[ "$exit_code" = "0" ] || fail "saiu com $exit_code depois do SIGTERM, esperava 0"
docker logs "$NAME" 2>&1 | grep -q 'stopped' || fail "o log não diz que o hub parou"

# 6. Nenhuma queixa de escrita no log inteiro.
if docker logs "$NAME" 2>&1 | grep -Ei 'read-only file system|permission denied'; then
  fail "o log tem erro de escrita: a raiz só de leitura quebrou alguma coisa"
fi

echo "OK: a imagem sobe com a raiz só de leitura, responde /health e /status e para com SIGTERM."
