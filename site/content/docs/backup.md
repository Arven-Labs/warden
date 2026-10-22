# Backup and updates

What matters to keep: the **`/data` volume** (or the config folder and the vault, on path A) and the
**pairing key**.

## Back up the hub

```bash
# a copy of the whole volume (config, provider keys, vault, conversations, devices)
docker stop warden
docker run --rm -v warden-data:/data:ro -v "$PWD":/out debian:trixie-slim \
  tar czf /out/warden-data-$(date +%F).tgz -C /data .
docker start warden
```

This file is **not encrypted and contains your provider keys**: keep it the way you keep a password. To
restore, create a new volume and extract the `.tgz` into it (`tar xzf ... -C /data`, as the image's own user,
`--user 10001`).

`warden backup --out <empty folder>` is something else: it copies only the **members'** data (see
[People](/docs/people/)), already encrypted with each person's password:

```bash
warden-server backup --out ~/warden-backup [--user ana]   # encrypted; unreadable without each member's password or code
warden-server restore ~/warden-backup [--user ana]        # also brings back a member you removed by mistake
```

## Update

To update the hub:

```bash
docker pull ghcr.io/arven-labs/warden-server:latest
docker rm -f warden            # the data stays in the volume
# and run the "docker run -d ..." from the hub page again
```

On path A, download the new version from the release and install over it; the configuration and the vault
stay.
