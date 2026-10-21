#!/usr/bin/env bash
# Smoke test of the orchestrator (P138, phase 4) with real Docker: provisions an instance from the hub image,
# suspends, resumes, recycles, makes an encrypted backup, verifies and restores, and deletes, checking at every step what Docker shows
# and that the customer's data survives.
# Usage: scripts/orchestrator-smoke.sh [image] [binary]   (defaults: warden-server:smoke and target/release/warden-orchestrator).
# Needs docker and curl. The image is the same as in the image smoke test (scripts/image-smoke.sh).
set -euo pipefail

IMAGE="${1:-warden-server:smoke}"
ORCH="${2:-target/release/warden-orchestrator}"
CUSTOMER="smoke$$"
CONTAINER="warden-$CUSTOMER"
VOLUME="warden-data-$CUSTOMER"
STATE_DIR="$(mktemp -d)"

# A hub with no model provider refuses to start, so the instance is born with a config (the provider is never called
# here: the address does not exist). It is what `provision --config-file` puts in the volume.
CONFIG_FILE="$STATE_DIR/start-config.toml"
cat > "$CONFIG_FILE" <<'EOF'
active_provider = "main"

[[providers]]
id = "main"
kind = "openai"
api_key = "smoke"
base_url = "http://127.0.0.1:1/v1"
model = "fake"
EOF

orch() { "$ORCH" --state-dir "$STATE_DIR" "$@"; }

cleanup() {
  orch delete "$CUSTOMER" --purge-data >/dev/null 2>&1 || true
  docker rm -f "$CONTAINER" >/dev/null 2>&1 || true
  docker volume rm "$VOLUME" >/dev/null 2>&1 || true
  docker rmi "$IMAGE-next" "$IMAGE-bad" >/dev/null 2>&1 || true
  rm -rf "$STATE_DIR"
}
trap cleanup EXIT

fail() {
  echo "FAILED: $*" >&2
  echo "---- container log ----" >&2
  docker logs "$CONTAINER" >&2 2>&1 || true
  exit 1
}

started_at() { docker inspect -f '{{.State.StartedAt}}' "$CONTAINER"; }
# "true", "false" or "gone" (docker inspect of a container that does not exist exits with an error, but prints a blank line).
running() {
  local out
  if out="$(docker inspect -f '{{.State.Running}}' "$CONTAINER" 2>/dev/null)"; then echo "$out"; else echo "gone"; fi
}

wait_health() {
  local url="http://127.0.0.1:$PORT/health"
  for _ in $(seq 1 60); do
    if curl -fsS "$url" >/dev/null 2>&1; then return 0; fi
    sleep 1
  done
  fail "the hub did not answer /health in 60 s ($url)"
}

# 1. Provision: the container starts, published on loopback only, with a read-only root and no capabilities.
out="$(orch provision "$CUSTOMER" --image "$IMAGE" --config-file "$CONFIG_FILE")" || fail "provision failed"
echo "provision: $out"
PORT="${out##*:}"
wait_health
[ "$(running)" = "true" ] || fail "the container is not running after provision"
docker inspect -f '{{.HostConfig.ReadonlyRootfs}}' "$CONTAINER" | grep -q true || fail "the container root is not read-only"
docker inspect -f '{{.HostConfig.CapDrop}}' "$CONTAINER" | grep -q ALL || fail "the container did not drop its capabilities"
docker inspect -f '{{range $p, $b := .NetworkSettings.Ports}}{{range $b}}{{.HostIp}} {{end}}{{end}}' "$CONTAINER" | grep -q '127.0.0.1' \
  || fail "the port is not published on 127.0.0.1"
echo "container up on 127.0.0.1:$PORT, read-only root, no capabilities: ok"

# 2. The orchestrator's key opens the hub's /status; without it, no.
KEY="$(orch key "$CUSTOMER")"
[ "$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$PORT/status")" = "401" ] || fail "/status without a key did not give 401"
curl -fsS -H "Authorization: Bearer $KEY" "http://127.0.0.1:$PORT/status" | grep -q '"rss_bytes"' || fail "/status with the orchestrator's key failed"
echo "/status with the key: ok"

# 3. Asking again, the same, changes nothing; asking differently is refused.
first_start="$(started_at)"
again="$(orch provision "$CUSTOMER" --image "$IMAGE" --config-file "$CONFIG_FILE")" || fail "the second, identical provision failed"
[ "$again" = "$out" ] || fail "the second provision changed the answer: '$again' against '$out'"
[ "$(started_at)" = "$first_start" ] || fail "the second provision restarted the container"
if orch provision "$CUSTOMER" --image "$IMAGE" --config-file "$CONFIG_FILE" --memory-mb 256 >/dev/null 2>&1; then
  fail "provision with a different body for the same customer should be refused"
fi
echo "idempotent provision: ok"

# 4. A piece of customer data in /data, to prove it survives everything below.
docker exec "$CONTAINER" sh -c 'echo customer-marker > /data/marker' || fail "could not write to /data"

# 5. Suspend: the container stops, the volume stays and recycling does not touch it.
orch suspend "$CUSTOMER" >/dev/null || fail "suspend failed"
[ "$(running)" = "false" ] || fail "the container is still running after suspend"
docker volume inspect "$VOLUME" >/dev/null 2>&1 || fail "the volume vanished on suspend"
orch recycle --rss-threshold-mb 1 --min-idle-secs 0 --min-uptime-secs 0 >/dev/null 2>&1 || fail "recycle failed with a suspended instance"
[ "$(running)" = "false" ] || fail "recycling woke up a suspended instance"
echo "suspend: ok"

# 6. Resume: starts again with the same data.
orch resume "$CUSTOMER" >/dev/null || fail "resume failed"
wait_health
docker exec "$CONTAINER" cat /data/marker | grep -q customer-marker || fail "the customer's data did not survive suspend and resume"
echo "resume with the same data: ok"

# 7. Recycling: with the default policy the light hub is left alone; with thresholds that catch it, it is restarted and the data stays.
before="$(started_at)"
recycle_out="$(orch recycle 2>&1)" || fail "recycle (default policy) failed: $recycle_out"
echo "$recycle_out" | grep -q 'kept' || fail "the default policy should have left the hub alone: $recycle_out"
[ "$(started_at)" = "$before" ] || fail "the default policy restarted a light hub"
recycle_out="$(orch recycle --rss-threshold-mb 1 --min-idle-secs 0 --min-uptime-secs 0 2>&1)" || fail "recycle (forced) failed: $recycle_out"
echo "$recycle_out" | grep -q 'recycled' || fail "the forced recycle did not recycle: $recycle_out"
[ "$(started_at)" != "$before" ] || fail "recycle said it recycled but the container did not restart"
wait_health
docker exec "$CONTAINER" cat /data/marker | grep -q customer-marker || fail "the customer's data did not survive recycling"
echo "recycling: ok"

# 8. Backup: the hub stops during the copy and comes back, the file is encrypted (the marker does not show in the clear), and the
# verification starts a throwaway hub (no network) on a new volume, leaving nothing behind. A tampered file is refused.
backup_file="$(orch backup "$CUSTOMER")" || fail "backup failed"
[ -f "$backup_file" ] || fail "the backup did not create the file ($backup_file)"
orch backups "$CUSTOMER" | grep -q "$backup_file" || fail "the backup does not show up in 'backups'"
if grep -a -q customer-marker "$backup_file"; then fail "the marker shows in the clear in the backup file"; fi
[ "$(running)" = "true" ] || fail "the hub did not come back after the backup"
wait_health
docker exec "$CONTAINER" cat /data/marker | grep -q customer-marker || fail "the customer's data was lost in the backup"
echo "encrypted backup, hub back: ok"

# Off the host: the operator's command runs after the backup and the copy is identical and passes verify; if it fails, the backup
# stays here and the command exits with an error saying it was not shipped. The key comes out through a separate command.
OFFSITE="$STATE_DIR/offsite"
mkdir -p "$OFFSITE"
shipped="$(orch --after-backup "cp {file} $OFFSITE/" backup "$CUSTOMER")" || fail "backup with --after-backup failed"
copy="$OFFSITE/$(basename "$shipped")"
[ -f "$copy" ] || fail "the operator's command did not leave the copy at $copy"
cmp "$shipped" "$copy" || fail "the off-host copy is not identical to the backup"
orch verify "$CUSTOMER" "$copy" >/dev/null || fail "verify refused the off-host copy"
if ship_out="$(orch --after-backup false backup "$CUSTOMER" 2>&1)"; then
  fail "an --after-backup that fails should make the backup exit with an error: $ship_out"
fi
echo "$ship_out" | grep -q 'could not be shipped' || fail "the error does not say the backup was not shipped: $ship_out"
echo "$ship_out" | grep -q '\.wbk' || fail "the error does not say where the backup stayed: $ship_out"
[ "$(running)" = "true" ] || fail "the hub did not come back after a failed shipment"
wait_health
key_hex="$(orch backup-key 2>/dev/null)"
[ "${#key_hex}" -eq 64 ] || fail "backup-key did not print 64 characters: '$key_hex'"
[ "$key_hex" = "$(cat "$STATE_DIR/backup.key")" ] || fail "backup-key differs from the backup.key file"
echo "off-host backup (identical, valid copy, failure reported, key apart): ok"

orch verify "$CUSTOMER" || fail "verify of a good backup failed"
[ -z "$(docker ps -a -q --filter name=warden-verify-)" ] || fail "verify left a test container"
[ -z "$(docker volume ls -q --filter name=warden-data-verify-)" ] || fail "verify left a test volume"
bad_file="$STATE_DIR/tampered.wbk"
cp "$backup_file" "$bad_file"
printf '\377' | dd of="$bad_file" bs=1 seek=40 conv=notrunc 2>/dev/null
if orch verify "$CUSTOMER" "$bad_file" >/dev/null 2>&1; then fail "a tampered backup was accepted"; fi
[ -z "$(docker volume ls -q --filter name=warden-data-verify-)" ] || fail "verify of a bad backup left a test volume"
echo "verify (good accepted, tampered refused, no leftovers): ok"

# Restore: delete the customer's data, bring it back from the backup and provision again; the marker comes back.
orch delete "$CUSTOMER" --purge-data >/dev/null || fail "delete --purge-data failed before the restore"
if docker volume inspect "$VOLUME" >/dev/null 2>&1; then fail "the volume still exists before the restore"; fi
orch restore "$CUSTOMER" latest --image "$IMAGE" >/dev/null || fail "restore failed"
docker volume inspect "$VOLUME" >/dev/null 2>&1 || fail "the restore did not create the volume"
if orch restore "$CUSTOMER" latest --image "$IMAGE" >/dev/null 2>&1; then fail "restore over a volume that already exists should be refused"; fi
out="$(orch provision "$CUSTOMER" --image "$IMAGE" --config-file "$CONFIG_FILE")" || fail "provision after the restore failed"
PORT="${out##*:}"
wait_health
docker exec "$CONTAINER" cat /data/marker | grep -q customer-marker || fail "the customer's data did not come back with the restore"
echo "restore: ok"

# 9. Upgrade: the new image (the same, with another tag) takes the old one's place on the same volume, with a backup first; asking
# again does nothing. An image that never becomes healthy is undone: the container goes back to the previous image, up and with the data.
NEXT="$IMAGE-next"
BAD="$IMAGE-bad"
docker tag "$IMAGE" "$NEXT"
upgrade_out="$(orch upgrade --image "$NEXT" --soak-secs 2 2>&1)" || fail "upgrade to the new image failed: $upgrade_out"
echo "$upgrade_out" | grep -q 'upgraded' || fail "the upgrade did not say 'upgraded': $upgrade_out"
[ "$(docker inspect -f '{{.Config.Image}}' "$CONTAINER")" = "$NEXT" ] || fail "the container is not on the new image"
wait_health
docker exec "$CONTAINER" cat /data/marker | grep -q customer-marker || fail "the customer's data did not survive the upgrade"
[ "$(orch backups "$CUSTOMER" | wc -l)" -ge 2 ] || fail "the upgrade did not take a backup first"
again_out="$(orch upgrade --image "$NEXT" 2>&1)" || fail "the second, identical upgrade failed: $again_out"
echo "$again_out" | grep -q 'already on' || fail "the second upgrade should say everything is on the image: $again_out"
echo "upgrade: ok"

printf 'FROM %s\nENTRYPOINT ["sh","-c","exit 1"]\n' "$NEXT" | docker build -q -t "$BAD" - >/dev/null || fail "could not build the bad image"
if bad_out="$(orch upgrade --image "$BAD" --soak-secs 0 --health-timeout-secs 15 2>&1)"; then
  fail "the upgrade to an image that does not start should exit with an error: $bad_out"
fi
echo "$bad_out" | grep -q 'rolled back' || fail "the bad upgrade should say 'rolled back': $bad_out"
[ "$(docker inspect -f '{{.Config.Image}}' "$CONTAINER")" = "$NEXT" ] || fail "after the rollback the container did not go back to the previous image"
[ "$(running)" = "true" ] || fail "the hub is not up after the rollback"
wait_health
docker exec "$CONTAINER" cat /data/marker | grep -q customer-marker || fail "the customer's data did not survive the rollback"
echo "rollback of an image that does not start: ok"

# 10. Delete: the container and the record go away, the volume stays; with --purge-data, the volume too. Deleting again is ok.
orch delete "$CUSTOMER" >/dev/null || fail "delete failed"
[ "$(running)" = "gone" ] || fail "the container still exists after delete"
docker volume inspect "$VOLUME" >/dev/null 2>&1 || fail "delete removed the volume without --purge-data"
if orch list | grep -q "$CUSTOMER"; then fail "the record is still in the list after delete"; fi
orch delete "$CUSTOMER" --purge-data >/dev/null || fail "delete --purge-data failed"
if docker volume inspect "$VOLUME" >/dev/null 2>&1; then fail "the volume still exists after --purge-data"; fi
orch delete "$CUSTOMER" >/dev/null || fail "deleting again should work"
echo "delete: ok"

echo "OK: the orchestrator provisions, suspends, resumes, recycles, makes an encrypted backup, verifies, restores, upgrades with rollback and deletes a real instance, without losing the customer's data."
