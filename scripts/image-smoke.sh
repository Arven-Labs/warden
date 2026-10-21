#!/usr/bin/env bash
# Smoke test of the hub image (P143 e): starts the container with a READ-ONLY root and checks that it works with
# only /data (volume) and /tmp (tmpfs) writable: idle, and then with a real turn against a fake model
# (scripts/fake-openai.py), which also makes the semantic search download its model to /data/models.
# Usage: scripts/image-smoke.sh [image]   (default: warden-server:smoke). Needs docker, curl and python3.
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
  echo "FAILED: $*" >&2
  echo "---- container log ----" >&2
  docker logs "$NAME" >&2 2>&1 || true
  exit 1
}

# The fake model runs here, on the host; the container reaches it through the gateway of Docker's default network (a
# private address, the only one the hub accepts over plain http besides localhost).
GATEWAY="$(docker network inspect bridge -f '{{(index .IPAM.Config 0).Gateway}}')"
python3 "$(dirname "$0")/fake-openai.py" "$FAKE_PORT" &
FAKE_PID=$!

docker volume create "$VOLUME" >/dev/null
# The configuration goes into the volume before the hub starts, as the container's user (the same one that will read it).
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

# Shortcut for the hub's own commands, running inside the container as its user.
warden() { docker exec "$NAME" warden-server --data-dir /data "$@"; }

# 1. /health answers without a key.
up=""
for _ in $(seq 1 60); do
  if curl -fsS "http://127.0.0.1:$PORT/health" >/dev/null 2>&1; then up=1; break; fi
  [ "$(docker inspect -f '{{.State.Running}}' "$NAME")" = "true" ] || fail "the container stopped before answering /health"
  sleep 1
done
[ -n "$up" ] || fail "/health did not answer in 60 s"
curl -fsS "http://127.0.0.1:$PORT/health" | grep -q '"status":"ok"' || fail "/health without status ok"

# 2. /status requires the key and returns the state with it.
code="$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$PORT/status")"
[ "$code" = "401" ] || fail "/status without a key gave $code, expected 401"
body="$(curl -fsS -H "Authorization: Bearer $KEY" "http://127.0.0.1:$PORT/status")" || fail "/status with the key failed"
echo "$body" | grep -q '"rss_bytes"' || fail "/status without rss_bytes: $body"
echo "status: $body"

# 2b. A scheduled task, created right away: the hub sees it on the next tick (30 s) and runs it 1 min later; the result is checked
#     at the end, while the other steps go on.
warden tasks add smoke-task --prompt "Say pong." --every 1m >/dev/null || fail "tasks add failed (did the config.toml in /data refuse the write?)"

# 3. What the hub wrote went to /data; the root is still read-only.
docker exec "$NAME" sh -c 'touch /etc/warden-write-test' 2>/dev/null && fail "the container root accepted a write"
docker exec "$NAME" sh -c 'ls -A /data | head -n 1' | grep -q . || echo "warning: /data is empty (the hub has not written anything yet)"

# 4. A real turn through the API, against the fake model, with a note in the vault: writes the API key, the vault and the
#    index to /data and, on the first semantic search, downloads the search model to /data/models.
docker exec "$NAME" sh -c 'mkdir -p /data/vault && echo "The rotation of the signing keys happens every Friday." > /data/vault/keys.md' \
  || fail "could not write a note to the vault in /data"
API_KEY="$(warden api-keys create smoke 2>/dev/null)" || fail "api-keys create failed"
[ -n "$API_KEY" ] || fail "api-keys create did not return the key"
reply="$(curl -fsS --max-time 600 -H "Authorization: Bearer $API_KEY" -H 'Content-Type: application/json' \
  -d '{"model":"warden","messages":[{"role":"user","content":"When are the signing keys rotated?"}]}' \
  "http://127.0.0.1:$PORT/v1/chat/completions")" || fail "the turn through the API failed"
echo "$reply" | grep -q 'pong' || fail "the turn did not bring the fake model's answer: $reply"
echo "turn: ok"
docker exec "$NAME" sh -c 'ls -A /data/models 2>/dev/null | head -n 1' | grep -q . \
  || fail "the search model was not downloaded to /data/models (the semantic search did not run or did not write)"
echo "search model in /data/models: ok"
body="$(curl -fsS -H "Authorization: Bearer $KEY" "http://127.0.0.1:$PORT/status")"
echo "status after the turn: $body"
echo "$body" | grep -q '"in_flight":0' || fail "the turn is over but /status still says there is work: $body"

# 4b. A webhook: the token goes to webhook_tokens.json in /data and the outside call triggers a turn with no open connection.
warden webhooks add smoke-hook --prompt "Say pong about this event." >/dev/null || fail "webhooks add failed"
HOOK_TOKEN="$(warden webhooks token smoke-hook 2>/dev/null)" || fail "webhooks token failed"
[ -n "$HOOK_TOKEN" ] || fail "webhooks token did not return the token"
code="$(curl -s -o /dev/null -w '%{http_code}' -X POST -H "Authorization: Bearer $HOOK_TOKEN" --data '{"event":"smoke"}' \
  "http://127.0.0.1:$PORT/hooks/smoke-hook")"
case "$code" in 200|202) ;; *) fail "the webhook answered $code, expected 200 or 202" ;; esac
hook_seen=""
for _ in $(seq 1 30); do
  if docker exec "$NAME" sh -c 'find /data -name "*task-hook-smoke-hook*" | head -n 1' | grep -q .; then hook_seen=1; break; fi
  sleep 1
done
[ -n "$hook_seen" ] || fail "the webhook answered $code but its conversation did not show up in /data within 30 s"
echo "webhook: ok ($code, conversation written to /data)"

# 4c. Sync through git, against a local remote in /data: git runs inside the container with HOME=/tmp, and the local repository, the
#     manifest and the vault key stay in /data.
docker exec "$NAME" git init --bare -q /data/remote.git || fail "git init --bare failed in /data"
docker exec "$NAME" sh -c 'printf "\n[git_sync]\nremote_url = \"/data/remote.git\"\ntoken = \"smoke\"\n" >> /data/config.toml' \
  || fail "could not append [git_sync] to the config"
warden sync init >/dev/null || fail "sync init failed"
sync_out="$(warden sync now 2>&1)" || fail "sync now failed: $sync_out"
echo "$sync_out" | grep -q 'pushed' || fail "sync now pushed nothing to the remote: $sync_out"
echo "sync: ok ($(echo "$sync_out" | tr '\n' ' '))"

# 4d. The scheduled task ran (the check that was left for the end, so the 1.5 to 2 min wait goes along with the rest).
task_line=""
for _ in $(seq 1 90); do
  task_line="$(warden tasks list 2>/dev/null | grep smoke-task || true)"
  case "$task_line" in *"last ran"*) break ;; esac
  sleep 2
done
case "$task_line" in
  *"failed"*) fail "the scheduled task ran and failed: $task_line" ;;
  *"last ran"*) echo "scheduled task: ok ($task_line)" ;;
  *) fail "the scheduled task did not run in 3 min: $task_line" ;;
esac

# 4e. Export and import (P141): the package leaves /data, does not carry what belongs to the operator, and goes whole into a new volume.
warden export /data/export >/dev/null || fail "export failed"
docker exec "$NAME" sh -c 'test -f /data/export/warden-export.json && test -f /data/export/config.toml && test -f /data/export/vault/keys.md' \
  || fail "the exported package lacks the manifest, the config and the vault"
docker exec "$NAME" sh -c 'test ! -e /data/export/devices.json && test ! -e /data/export/api_keys.json && test ! -e /data/export/webhook_tokens.json && test ! -e /data/export/config.secrets.toml' \
  || fail "the exported package carries something that belongs to the operator or a secret without --with-secrets"
docker volume create "$VOLUME2" >/dev/null
docker run --rm --read-only --tmpfs /tmp -v "$VOLUME":/src:ro -v "$VOLUME2":/data --entrypoint warden-server "$IMAGE" \
  --data-dir /data import /src/export >/dev/null || fail "import into a new volume failed"
docker run --rm --entrypoint sh -v "$VOLUME2":/data "$IMAGE" -c 'test -f /data/config.toml && test -f /data/vault/keys.md && test ! -e /data/devices.json' \
  || fail "the imported volume lacks the config and the vault, or has the other one's pairings"
echo "export and import: ok"

# 4f. The config.toml example in GUIA-DE-USO.md (the first ```toml block) is what the user copies: the hub must accept it.
#     `deny_unknown_fields` refuses an invented or misnamed field, so an example that rots breaks the smoke test.
#     (The guide is in Portuguese: its placeholder is SUA_CHAVE_DE_MODELO.)
GUIDE="$(dirname "$0")/../GUIA-DE-USO.md"
guide_config="$(awk '/^```toml/{ if (!done) { on=1; next } } on && /^```/{ on=0; done=1 } on' "$GUIDE")"
[ -n "$guide_config" ] || fail "could not find the \`\`\`toml block in GUIA-DE-USO.md"
printf '%s\n' "$guide_config" | sed 's/SUA_CHAVE_DE_MODELO/test-key/' \
  | docker exec -i "$NAME" sh -c 'cat > /data/guide-config.toml' || fail "could not write the guide's example to /data"
guide_out="$(warden tasks list --config /data/guide-config.toml 2>&1)" \
  || fail "the hub refused the config.toml of GUIA-DE-USO.md: $guide_out"
echo "guide config: ok"

# 5. SIGTERM: exits with 0 and says it stopped.
docker stop -t 15 "$NAME" >/dev/null
exit_code="$(docker inspect -f '{{.State.ExitCode}}' "$NAME")"
[ "$exit_code" = "0" ] || fail "exited with $exit_code after SIGTERM, expected 0"
docker logs "$NAME" 2>&1 | grep -q 'stopped' || fail "the log does not say the hub stopped"

# 6. No write complaint in the whole log.
if docker logs "$NAME" 2>&1 | grep -Ei 'read-only file system|permission denied'; then
  fail "the log has a write error: the read-only root broke something"
fi

echo "OK: with the read-only root the image answers /health and /status, runs a turn with semantic search, a scheduled task, a webhook and a git sync, and stops on SIGTERM."
