#!/usr/bin/env bash
# Smoke test of the gateway image (P168 j): starts the container with a READ-ONLY root and checks that it works with only /data
# (volume) and /tmp (tmpfs) writable: an account with credit, the models, a streamed chat against a fake provider
# (scripts/fake-openai.py) that is charged to the ledger, the ledger surviving a restart, and SIGTERM.
# Usage: scripts/gateway-smoke.sh [image]   (default: warden-gateway:smoke). Needs docker, curl and python3.
set -euo pipefail

IMAGE="${1:-warden-gateway:smoke}"
NAME="gateway-smoke-$$"
PORT="${PORT:-17430}"
FAKE_PORT="${FAKE_PORT:-17431}"
VOLUME="gateway-smoke-data-$$"
ADMIN="$(head -c 24 /dev/urandom | od -An -tx1 | tr -d ' \n')"
FAKE_PID=""
BASE="http://127.0.0.1:$PORT"

cleanup() {
  [ -z "$FAKE_PID" ] || kill "$FAKE_PID" >/dev/null 2>&1 || true
  docker rm -f "$NAME" >/dev/null 2>&1 || true
  docker volume rm "$VOLUME" >/dev/null 2>&1 || true
}
trap cleanup EXIT

fail() {
  echo "FAILED: $*" >&2
  echo "---- container log ----" >&2
  docker logs "$NAME" >&2 2>&1 || true
  exit 1
}

# The fake provider runs here, on the host; the container reaches it through the gateway of Docker's default network.
GATEWAY="$(docker network inspect bridge -f '{{(index .IPAM.Config 0).Gateway}}')"
python3 "$(dirname "$0")/fake-openai.py" "$FAKE_PORT" &
FAKE_PID=$!

docker volume create "$VOLUME" >/dev/null
# The catalog goes into the volume before the gateway starts, as the container's user (the same one that will read it).
docker run --rm -i --entrypoint sh -v "$VOLUME":/data "$IMAGE" -c 'cat > /data/catalog.toml' <<EOF
margin_percent = 30
default_max_tokens = 1000

[[upstreams]]
id = "fake"
base_url = "http://$GATEWAY:$FAKE_PORT/v1"

[[models]]
name = "fast"
upstream = "fake"
upstream_model = "real-fast"
cost_in = 1.0
cost_out = 2.0
EOF

start() {
  docker run -d --name "$NAME" --read-only --tmpfs /tmp -v "$VOLUME":/data -p "$PORT":7430 \
    -e WARDEN_GATEWAY_ADMIN_SECRET="$ADMIN" "$IMAGE" >/dev/null
  local up=""
  for _ in $(seq 1 60); do
    if curl -fsS "$BASE/health" >/dev/null 2>&1; then up=1; break; fi
    [ "$(docker inspect -f '{{.State.Running}}' "$NAME")" = "true" ] || fail "the container stopped before answering /health"
    sleep 1
  done
  [ -n "$up" ] || fail "/health did not answer in 60 s"
}

# Shortcut for the gateway's own commands, running inside the container as its user.
gw() { docker exec "$NAME" warden-gateway "$@"; }

# 1. /health answers without a token.
start
health="$(curl -fsS "$BASE/health")" || fail "/health failed"
echo "$health" | grep -q '"status":"ok"' || fail "/health without status ok: $health"

# 2. An account with five dollars of credit, made with the CLI inside the container.
created="$(gw accounts create smoke)" || fail "accounts create failed (is /data writable for the container's user?)"
ACCOUNT="$(echo "$created" | awk '$1 == "account" { print $2 }')"
TOKEN="$(echo "$created" | awk '$1 == "token" { print $2 }')"
[ -n "$ACCOUNT" ] && [ -n "$TOKEN" ] || fail "could not read the account and the token from: $created"
gw credit add "$ACCOUNT" 5 --ref smoke-topup >/dev/null || fail "credit add failed"

# 3. The token is checked; the models and the balance come back.
code="$(curl -s -o /dev/null -w '%{http_code}' "$BASE/v1/models")"
[ "$code" = "401" ] || fail "/v1/models without a token gave $code, expected 401"
models="$(curl -fsS -H "Authorization: Bearer $TOKEN" "$BASE/v1/models")" || fail "/v1/models with the token failed"
echo "$models" | grep -q '"fast"' || fail "/v1/models does not list the catalog's model: $models"
balance="$(curl -fsS -H "Authorization: Bearer $TOKEN" "$BASE/v1/balance")" || fail "/v1/balance failed"
echo "$balance" | grep -q '"balance_micro_usd":5000000' || fail "the balance is not US\$ 5: $balance"
echo "models and balance: ok"

# 4. A streamed chat against the fake provider is forwarded and charged.
reply="$(curl -fsS --max-time 60 -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"model":"fast","stream":true,"messages":[{"role":"user","content":"ping"}]}' \
  "$BASE/v1/chat/completions")" || fail "the chat failed"
echo "$reply" | grep -q 'pong' || fail "the chat did not bring the fake provider's answer: $reply"
after="$(curl -fsS -H "Authorization: Bearer $TOKEN" "$BASE/v1/balance")"
echo "$after" | grep -q '"balance_micro_usd":5000000' && fail "the chat was not charged: $after"
# The output is captured first: `grep -q` leaves at its first hit, and with `pipefail` the SIGPIPE it sends back to `docker exec` made
# this line fail now and then (the commit that bumped the version failed it with the same code that had passed).
ledger="$(gw ledger "$ACCOUNT")" || fail "the ledger command failed"
echo "$ledger" | grep -q 'debit' || fail "the ledger shows no debit after the chat: $ledger"
echo "chat charged: ok ($after)"

# 5. The site's signed routes refuse a call with no signature.
code="$(curl -s -o /dev/null -w '%{http_code}' "$BASE/v1/accounts/$ACCOUNT/balance")"
case "$code" in 401|403) ;; *) fail "an unsigned call to the account routes gave $code, expected 401 or 403" ;; esac
echo "unsigned account route refused: ok ($code)"

# 6. The ledger is in the volume: it survives the container being replaced.
want="$(echo "$after" | sed 's/.*"balance_micro_usd":\([0-9-]*\).*/\1/')"
docker rm -f "$NAME" >/dev/null
start
again="$(curl -fsS -H "Authorization: Bearer $TOKEN" "$BASE/v1/balance")" || fail "/v1/balance failed after the restart"
echo "$again" | grep -q "\"balance_micro_usd\":$want" || fail "the balance changed across the restart: wanted $want, got $again"
echo "restart: ok"

# 7. SIGTERM: exits with 0 and says it stopped.
docker stop -t 15 "$NAME" >/dev/null
exit_code="$(docker inspect -f '{{.State.ExitCode}}' "$NAME")"
[ "$exit_code" = "0" ] || fail "exited with $exit_code after SIGTERM, expected 0"
logs="$(docker logs "$NAME" 2>&1)"
echo "$logs" | grep -q 'gateway stopped' || fail "the log does not say the gateway stopped"

# 8. No write complaint in the whole log.
if docker logs "$NAME" 2>&1 | grep -Ei 'read-only file system|permission denied'; then
  fail "the log has a write error: the read-only root broke something"
fi

echo "OK: with the read-only root the image answers /health, checks the token, charges a streamed chat to a ledger that survives a restart, and stops on SIGTERM."
