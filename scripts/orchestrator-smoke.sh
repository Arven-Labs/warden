#!/usr/bin/env bash
# Teste de fumaça do orquestrador (P138, fase 4) com Docker de verdade: provisiona uma instância a partir da imagem do hub,
# suspende, retoma, recicla, faz backup cifrado, verifica e restaura, e apaga, conferindo em cada passo o que o Docker mostra
# e que os dados do cliente sobrevivem.
# Uso: scripts/orchestrator-smoke.sh [imagem] [binário]   (padrões: warden-server:smoke e target/release/warden-orchestrator).
# Precisa de docker e curl. A imagem é a mesma da fumaça da imagem (scripts/image-smoke.sh).
set -euo pipefail

IMAGE="${1:-warden-server:smoke}"
ORCH="${2:-target/release/warden-orchestrator}"
CUSTOMER="smoke$$"
CONTAINER="warden-$CUSTOMER"
VOLUME="warden-data-$CUSTOMER"
STATE_DIR="$(mktemp -d)"

# Um hub sem provedor de modelo se recusa a subir, então a instância nasce com uma config (o provedor nunca é chamado
# aqui: o endereço não existe). É o que o `provision --config-file` põe no volume.
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
  echo "FALHOU: $*" >&2
  echo "---- log do contêiner ----" >&2
  docker logs "$CONTAINER" >&2 2>&1 || true
  exit 1
}

started_at() { docker inspect -f '{{.State.StartedAt}}' "$CONTAINER"; }
# "true", "false" ou "gone" (o docker inspect de um contêiner que não existe sai com erro, mas imprime uma linha em branco).
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
  fail "o hub não respondeu /health em 60 s ($url)"
}

# 1. Provisionar: o contêiner sobe, publicado só no loopback, com a raiz só de leitura e sem capacidades.
out="$(orch provision "$CUSTOMER" --image "$IMAGE" --config-file "$CONFIG_FILE")" || fail "provision falhou"
echo "provision: $out"
PORT="${out##*:}"
wait_health
[ "$(running)" = "true" ] || fail "o contêiner não está rodando depois do provision"
docker inspect -f '{{.HostConfig.ReadonlyRootfs}}' "$CONTAINER" | grep -q true || fail "a raiz do contêiner não é só de leitura"
docker inspect -f '{{.HostConfig.CapDrop}}' "$CONTAINER" | grep -q ALL || fail "o contêiner não largou as capacidades"
docker inspect -f '{{range $p, $b := .NetworkSettings.Ports}}{{range $b}}{{.HostIp}} {{end}}{{end}}' "$CONTAINER" | grep -q '127.0.0.1' \
  || fail "a porta não está publicada no 127.0.0.1"
echo "contêiner de pé em 127.0.0.1:$PORT, raiz só de leitura, sem capacidades: ok"

# 2. A chave do orquestrador abre o /status do hub; sem ela, não.
KEY="$(orch key "$CUSTOMER")"
[ "$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$PORT/status")" = "401" ] || fail "/status sem chave não deu 401"
curl -fsS -H "Authorization: Bearer $KEY" "http://127.0.0.1:$PORT/status" | grep -q '"rss_bytes"' || fail "/status com a chave do orquestrador falhou"
echo "/status com a chave: ok"

# 3. Pedir de novo, igual, não muda nada; pedir diferente é recusado.
first_start="$(started_at)"
again="$(orch provision "$CUSTOMER" --image "$IMAGE" --config-file "$CONFIG_FILE")" || fail "o segundo provision, igual, falhou"
[ "$again" = "$out" ] || fail "o segundo provision mudou a resposta: '$again' contra '$out'"
[ "$(started_at)" = "$first_start" ] || fail "o segundo provision reiniciou o contêiner"
if orch provision "$CUSTOMER" --image "$IMAGE" --config-file "$CONFIG_FILE" --memory-mb 256 >/dev/null 2>&1; then
  fail "provision com outro corpo para o mesmo cliente deveria ser recusado"
fi
echo "provision idempotente: ok"

# 4. Um dado do cliente em /data, para provar que sobrevive a tudo abaixo.
docker exec "$CONTAINER" sh -c 'echo guardado > /data/marker' || fail "não consegui gravar em /data"

# 5. Suspender: o contêiner para, o volume fica e a reciclagem não mexe nele.
orch suspend "$CUSTOMER" >/dev/null || fail "suspend falhou"
[ "$(running)" = "false" ] || fail "o contêiner continua rodando depois do suspend"
docker volume inspect "$VOLUME" >/dev/null 2>&1 || fail "o volume sumiu no suspend"
orch recycle --rss-threshold-mb 1 --min-idle-secs 0 --min-uptime-secs 0 >/dev/null 2>&1 || fail "recycle falhou com uma instância suspensa"
[ "$(running)" = "false" ] || fail "a reciclagem acordou uma instância suspensa"
echo "suspend: ok"

# 6. Retomar: sobe de novo com os mesmos dados.
orch resume "$CUSTOMER" >/dev/null || fail "resume falhou"
wait_health
docker exec "$CONTAINER" cat /data/marker | grep -q guardado || fail "o dado do cliente não sobreviveu ao suspend e ao resume"
echo "resume com os mesmos dados: ok"

# 7. Reciclagem: com a política padrão o hub leve é deixado em paz; com limiares que o pegam, é reiniciado e os dados ficam.
before="$(started_at)"
recycle_out="$(orch recycle 2>&1)" || fail "recycle (política padrão) falhou: $recycle_out"
echo "$recycle_out" | grep -q 'kept' || fail "a política padrão devia deixar o hub em paz: $recycle_out"
[ "$(started_at)" = "$before" ] || fail "a política padrão reiniciou um hub leve"
recycle_out="$(orch recycle --rss-threshold-mb 1 --min-idle-secs 0 --min-uptime-secs 0 2>&1)" || fail "recycle (forçado) falhou: $recycle_out"
echo "$recycle_out" | grep -q 'recycled' || fail "o recycle forçado não reciclou: $recycle_out"
[ "$(started_at)" != "$before" ] || fail "o recycle disse que reciclou mas o contêiner não reiniciou"
wait_health
docker exec "$CONTAINER" cat /data/marker | grep -q guardado || fail "o dado do cliente não sobreviveu à reciclagem"
echo "reciclagem: ok"

# 8. Backup: o hub para durante a cópia e volta, o arquivo é cifrado (o marcador não aparece em claro), e a verificação
# sobe um hub descartável (sem rede) num volume novo, sem deixar nada para trás. Um arquivo adulterado é recusado.
backup_file="$(orch backup "$CUSTOMER")" || fail "backup falhou"
[ -f "$backup_file" ] || fail "o backup não criou o arquivo ($backup_file)"
orch backups "$CUSTOMER" | grep -q "$backup_file" || fail "o backup não aparece em 'backups'"
if grep -a -q guardado "$backup_file"; then fail "o marcador aparece em claro no arquivo do backup"; fi
[ "$(running)" = "true" ] || fail "o hub não voltou depois do backup"
wait_health
docker exec "$CONTAINER" cat /data/marker | grep -q guardado || fail "o dado do cliente se perdeu no backup"
echo "backup cifrado, hub de volta: ok"

# Fora do host: o comando do operador roda depois do backup e a cópia é idêntica e passa no verify; se ele falha, o backup
# continua aqui e o comando sai com erro dizendo que não foi enviado. A chave sai por um comando à parte.
OFFSITE="$STATE_DIR/offsite"
mkdir -p "$OFFSITE"
shipped="$(orch --after-backup "cp {file} $OFFSITE/" backup "$CUSTOMER")" || fail "backup com --after-backup falhou"
copy="$OFFSITE/$(basename "$shipped")"
[ -f "$copy" ] || fail "o comando do operador não deixou a cópia em $copy"
cmp "$shipped" "$copy" || fail "a cópia de fora não é idêntica ao backup"
orch verify "$CUSTOMER" "$copy" >/dev/null || fail "o verify recusou a cópia de fora"
if ship_out="$(orch --after-backup false backup "$CUSTOMER" 2>&1)"; then
  fail "um --after-backup que falha deveria fazer o backup sair com erro: $ship_out"
fi
echo "$ship_out" | grep -q 'could not be shipped' || fail "o erro não diz que o backup não foi enviado: $ship_out"
echo "$ship_out" | grep -q '\.wbk' || fail "o erro não diz onde o backup ficou: $ship_out"
[ "$(running)" = "true" ] || fail "o hub não voltou depois de um envio que falhou"
wait_health
key_hex="$(orch backup-key 2>/dev/null)"
[ "${#key_hex}" -eq 64 ] || fail "o backup-key não imprimiu 64 caracteres: '$key_hex'"
[ "$key_hex" = "$(cat "$STATE_DIR/backup.key")" ] || fail "o backup-key difere do arquivo backup.key"
echo "backup fora do host (cópia idêntica e válida, falha dita, chave à parte): ok"

orch verify "$CUSTOMER" || fail "verify de um backup bom falhou"
[ -z "$(docker ps -a -q --filter name=warden-verify-)" ] || fail "o verify deixou um contêiner de teste"
[ -z "$(docker volume ls -q --filter name=warden-data-verify-)" ] || fail "o verify deixou um volume de teste"
bad_file="$STATE_DIR/adulterado.wbk"
cp "$backup_file" "$bad_file"
printf '\377' | dd of="$bad_file" bs=1 seek=40 conv=notrunc 2>/dev/null
if orch verify "$CUSTOMER" "$bad_file" >/dev/null 2>&1; then fail "um backup adulterado foi aceito"; fi
[ -z "$(docker volume ls -q --filter name=warden-data-verify-)" ] || fail "o verify de um backup ruim deixou um volume de teste"
echo "verify (bom aceito, adulterado recusado, sem sobras): ok"

# Restaurar: apagar os dados do cliente, trazer de volta do backup e provisionar igual; o marcador volta.
orch delete "$CUSTOMER" --purge-data >/dev/null || fail "delete --purge-data falhou antes do restore"
if docker volume inspect "$VOLUME" >/dev/null 2>&1; then fail "o volume continua existindo antes do restore"; fi
orch restore "$CUSTOMER" latest --image "$IMAGE" >/dev/null || fail "restore falhou"
docker volume inspect "$VOLUME" >/dev/null 2>&1 || fail "o restore não criou o volume"
if orch restore "$CUSTOMER" latest --image "$IMAGE" >/dev/null 2>&1; then fail "restore sobre um volume que já existe deveria ser recusado"; fi
out="$(orch provision "$CUSTOMER" --image "$IMAGE" --config-file "$CONFIG_FILE")" || fail "provision depois do restore falhou"
PORT="${out##*:}"
wait_health
docker exec "$CONTAINER" cat /data/marker | grep -q guardado || fail "o dado do cliente não voltou com o restore"
echo "restore: ok"

# 9. Upgrade: a imagem nova (a mesma, com outra etiqueta) entra no lugar da antiga no mesmo volume, com backup antes; pedir de
# novo não faz nada. Uma imagem que nunca fica saudável é desfeita: o contêiner volta à imagem anterior, de pé e com os dados.
NEXT="$IMAGE-next"
BAD="$IMAGE-bad"
docker tag "$IMAGE" "$NEXT"
upgrade_out="$(orch upgrade --image "$NEXT" --soak-secs 2 2>&1)" || fail "upgrade para a imagem nova falhou: $upgrade_out"
echo "$upgrade_out" | grep -q 'upgraded' || fail "o upgrade não disse 'upgraded': $upgrade_out"
[ "$(docker inspect -f '{{.Config.Image}}' "$CONTAINER")" = "$NEXT" ] || fail "o contêiner não está na imagem nova"
wait_health
docker exec "$CONTAINER" cat /data/marker | grep -q guardado || fail "o dado do cliente não sobreviveu ao upgrade"
[ "$(orch backups "$CUSTOMER" | wc -l)" -ge 2 ] || fail "o upgrade não tirou um backup antes"
again_out="$(orch upgrade --image "$NEXT" 2>&1)" || fail "o segundo upgrade, igual, falhou: $again_out"
echo "$again_out" | grep -q 'already on' || fail "o segundo upgrade devia dizer que está tudo na imagem: $again_out"
echo "upgrade: ok"

printf 'FROM %s\nENTRYPOINT ["sh","-c","exit 1"]\n' "$NEXT" | docker build -q -t "$BAD" - >/dev/null || fail "não consegui montar a imagem ruim"
if bad_out="$(orch upgrade --image "$BAD" --soak-secs 0 --health-timeout-secs 15 2>&1)"; then
  fail "o upgrade para uma imagem que não sobe deveria sair com erro: $bad_out"
fi
echo "$bad_out" | grep -q 'rolled back' || fail "o upgrade ruim devia dizer 'rolled back': $bad_out"
[ "$(docker inspect -f '{{.Config.Image}}' "$CONTAINER")" = "$NEXT" ] || fail "depois do rollback o contêiner não voltou à imagem anterior"
[ "$(running)" = "true" ] || fail "o hub não está de pé depois do rollback"
wait_health
docker exec "$CONTAINER" cat /data/marker | grep -q guardado || fail "o dado do cliente não sobreviveu ao rollback"
echo "rollback de uma imagem que não sobe: ok"

# 10. Apagar: o contêiner e o registro vão embora, o volume fica; com --purge-data, o volume também. Apagar de novo é ok.
orch delete "$CUSTOMER" >/dev/null || fail "delete falhou"
[ "$(running)" = "gone" ] || fail "o contêiner continua existindo depois do delete"
docker volume inspect "$VOLUME" >/dev/null 2>&1 || fail "o delete apagou o volume sem --purge-data"
if orch list | grep -q "$CUSTOMER"; then fail "o registro continua na lista depois do delete"; fi
orch delete "$CUSTOMER" --purge-data >/dev/null || fail "delete --purge-data falhou"
if docker volume inspect "$VOLUME" >/dev/null 2>&1; then fail "o volume continua existindo depois do --purge-data"; fi
orch delete "$CUSTOMER" >/dev/null || fail "apagar de novo deveria dar certo"
echo "delete: ok"

echo "OK: o orquestrador provisiona, suspende, retoma, recicla, faz backup cifrado, verifica, restaura, faz upgrade com rollback e apaga uma instância de verdade, sem perder os dados do cliente."
