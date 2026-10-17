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
VOLUME2="warden-smoke-import-$$"
FAKE_PORT="${FAKE_PORT:-17421}"
FAKE_PID=""

cleanup() {
  [ -z "$FAKE_PID" ] || kill "$FAKE_PID" >/dev/null 2>&1 || true
  docker rm -f "$NAME" >/dev/null 2>&1 || true
  docker volume rm "$VOLUME" "$VOLUME2" >/dev/null 2>&1 || true
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
  -e WARDEN_SERVER_AUTH_KEY="$KEY" "$IMAGE" serve --no-embedding-warm-up --run-tasks >/dev/null

# Atalho para os comandos do próprio hub, rodando dentro do contêiner como o usuário dele.
warden() { docker exec "$NAME" warden-server --data-dir /data "$@"; }

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

# 2b. Uma tarefa agendada, criada já: o hub a vê no próximo tick (30 s) e a roda 1 min depois; o resultado é conferido
#     no fim, enquanto os outros passos andam.
warden tasks add smoke-task --prompt "Say pong." --every 1m >/dev/null || fail "tasks add falhou (a config.toml em /data não aceitou escrita?)"

# 3. O que o hub gravou foi para /data; a raiz continua só de leitura.
docker exec "$NAME" sh -c 'touch /etc/warden-write-test' 2>/dev/null && fail "a raiz do contêiner aceitou escrita"
docker exec "$NAME" sh -c 'ls -A /data | head -n 1' | grep -q . || echo "aviso: /data está vazio (o hub ainda não gravou nada)"

# 4. Um turno de verdade pela API, contra o modelo de mentira, com uma nota no cofre: grava a chave da API, o cofre e o
#    índice em /data e, na primeira busca por significado, baixa o modelo de busca para /data/models.
docker exec "$NAME" sh -c 'mkdir -p /data/vault && echo "The rotation of the signing keys happens every Friday." > /data/vault/keys.md' \
  || fail "não consegui gravar uma nota no cofre em /data"
API_KEY="$(warden api-keys create smoke 2>/dev/null)" || fail "api-keys create falhou"
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

# 4b. Um webhook: o token vai para webhook_tokens.json em /data e a chamada de fora dispara um turno sem conexão aberta.
warden webhooks add smoke-hook --prompt "Say pong about this event." >/dev/null || fail "webhooks add falhou"
HOOK_TOKEN="$(warden webhooks token smoke-hook 2>/dev/null)" || fail "webhooks token falhou"
[ -n "$HOOK_TOKEN" ] || fail "webhooks token não devolveu o token"
code="$(curl -s -o /dev/null -w '%{http_code}' -X POST -H "Authorization: Bearer $HOOK_TOKEN" --data '{"event":"smoke"}' \
  "http://127.0.0.1:$PORT/hooks/smoke-hook")"
case "$code" in 200|202) ;; *) fail "o webhook respondeu $code, esperava 200 ou 202" ;; esac
hook_seen=""
for _ in $(seq 1 30); do
  if docker exec "$NAME" sh -c 'find /data -name "*task-hook-smoke-hook*" | head -n 1' | grep -q .; then hook_seen=1; break; fi
  sleep 1
done
[ -n "$hook_seen" ] || fail "o webhook respondeu $code mas a conversa dele não apareceu em /data em 30 s"
echo "webhook: ok ($code, conversa gravada em /data)"

# 4c. Sync por git, contra um remoto local em /data: o git roda dentro do contêiner com HOME=/tmp, e o repositório local, o
#     manifesto e a chave do cofre ficam em /data.
docker exec "$NAME" git init --bare -q /data/remote.git || fail "git init --bare falhou em /data"
docker exec "$NAME" sh -c 'printf "\n[git_sync]\nremote_url = \"/data/remote.git\"\ntoken = \"smoke\"\n" >> /data/config.toml' \
  || fail "não consegui acrescentar [git_sync] à config"
warden sync init >/dev/null || fail "sync init falhou"
sync_out="$(warden sync now 2>&1)" || fail "sync now falhou: $sync_out"
echo "$sync_out" | grep -q 'pushed' || fail "sync now não enviou nada ao remoto: $sync_out"
echo "sync: ok ($(echo "$sync_out" | tr '\n' ' '))"

# 4d. A tarefa agendada rodou (a conferência que ficou para o fim, para o 1,5 a 2 min de espera andar junto com o resto).
task_line=""
for _ in $(seq 1 90); do
  task_line="$(warden tasks list 2>/dev/null | grep smoke-task || true)"
  case "$task_line" in *"last ran"*) break ;; esac
  sleep 2
done
case "$task_line" in
  *"failed"*) fail "a tarefa agendada rodou e falhou: $task_line" ;;
  *"last ran"*) echo "tarefa agendada: ok ($task_line)" ;;
  *) fail "a tarefa agendada não rodou em 3 min: $task_line" ;;
esac

# 4e. Exportar e importar (P141): o pacote sai de /data, não leva o que é do operador, e entra inteiro num volume novo.
warden export /data/export >/dev/null || fail "export falhou"
docker exec "$NAME" sh -c 'test -f /data/export/warden-export.json && test -f /data/export/config.toml && test -f /data/export/vault/keys.md' \
  || fail "o pacote exportado não tem o manifesto, a config e o cofre"
docker exec "$NAME" sh -c 'test ! -e /data/export/devices.json && test ! -e /data/export/api_keys.json && test ! -e /data/export/webhook_tokens.json && test ! -e /data/export/config.secrets.toml' \
  || fail "o pacote exportado leva algo que é do operador ou um segredo sem --with-secrets"
docker volume create "$VOLUME2" >/dev/null
docker run --rm --read-only --tmpfs /tmp -v "$VOLUME":/src:ro -v "$VOLUME2":/data --entrypoint warden-server "$IMAGE" \
  --data-dir /data import /src/export >/dev/null || fail "import num volume novo falhou"
docker run --rm --entrypoint sh -v "$VOLUME2":/data "$IMAGE" -c 'test -f /data/config.toml && test -f /data/vault/keys.md && test ! -e /data/devices.json' \
  || fail "o volume importado não tem a config e o cofre, ou tem os pareamentos do outro"
echo "export e import: ok"

# 4f. O exemplo de config.toml do GUIA-DE-USO.md (o primeiro bloco ```toml) é o que o usuário copia: o hub tem de aceitá-lo.
#     `deny_unknown_fields` recusa um campo inventado ou com o nome errado, então um exemplo que apodrece derruba a fumaça.
GUIDE="$(dirname "$0")/../GUIA-DE-USO.md"
guide_config="$(awk '/^```toml/{ if (!done) { on=1; next } } on && /^```/{ on=0; done=1 } on' "$GUIDE")"
[ -n "$guide_config" ] || fail "não achei o bloco \`\`\`toml em GUIA-DE-USO.md"
printf '%s\n' "$guide_config" | sed 's/SUA_CHAVE_DE_MODELO/chave-de-teste/' \
  | docker exec -i "$NAME" sh -c 'cat > /data/guide-config.toml' || fail "não consegui gravar o exemplo do guia em /data"
guide_out="$(warden tasks list --config /data/guide-config.toml 2>&1)" \
  || fail "o hub recusou o config.toml do GUIA-DE-USO.md: $guide_out"
echo "config do guia: ok"

# 5. SIGTERM: sai com 0 e diz que parou.
docker stop -t 15 "$NAME" >/dev/null
exit_code="$(docker inspect -f '{{.State.ExitCode}}' "$NAME")"
[ "$exit_code" = "0" ] || fail "saiu com $exit_code depois do SIGTERM, esperava 0"
docker logs "$NAME" 2>&1 | grep -q 'stopped' || fail "o log não diz que o hub parou"

# 6. Nenhuma queixa de escrita no log inteiro.
if docker logs "$NAME" 2>&1 | grep -Ei 'read-only file system|permission denied'; then
  fail "o log tem erro de escrita: a raiz só de leitura quebrou alguma coisa"
fi

echo "OK: com a raiz só de leitura a imagem responde /health e /status, roda um turno com busca por significado, uma tarefa agendada, um webhook e um sync por git, e para com SIGTERM."
