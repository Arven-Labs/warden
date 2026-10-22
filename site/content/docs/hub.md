# Always-on hub

Path B: a hub that stays on. A small VPS is enough; an idle hub uses about 25 MB of memory in this image.

## Start it

```bash
# the pairing key: whoever holds it is the owner. Keep it (password manager).
KEY="$(openssl rand -hex 32)"
echo "$KEY"

docker volume create warden-data
```

Create the configuration inside the volume. Replace `YOUR_MODEL_KEY` and, if you aren't using Gemini, the kind
and the model:

```toml
active_provider = "main"

[[providers]]
id = "main"
kind = "gemini"
api_key = "YOUR_MODEL_KEY"
model = "gemini-3.5-flash"
```

Save the text above as `config.toml` and copy it into the volume, running as the image's own user:

```bash
docker run --rm -i --entrypoint sh -v warden-data:/data ghcr.io/arven-labs/warden-server:latest \
  -c 'cat > /data/config.toml' < config.toml
```

Start the hub:

```bash
docker run -d --name warden --restart unless-stopped \
  --read-only --tmpfs /tmp -v warden-data:/data -p 7420:7420 \
  -e WARDEN_SERVER_AUTH_KEY="$KEY" \
  ghcr.io/arven-labs/warden-server:latest serve --no-embedding-warm-up --run-tasks
```

`--run-tasks` turns on [scheduled tasks](/docs/tasks/) on this hub; leave it on one hub only. Check it:

```bash
curl http://localhost:7420/health        # {"status":"ok", ...}
```

Admin commands run inside the container:

```bash
warden() { docker exec warden warden-server --data-dir /data "$@"; }
warden api-keys create my-script     # OpenAI-compatible API key (shown once)
warden tasks list
```

## Connect your devices

- **Browser**: open `http://<hub address>:7420`. The first sign-in asks for the pairing key (`$KEY`).
- **Desktop app**: under **Workspace → Hubs → Add hub**, enter the address (`ws://<address>:7420`) and the
  key. On the same network, the discovery button finds the hub by itself.
- **Phone (Android)**: download `Warden_<version>_android-arm64.apk` from the release (from v0.1.2 on; before
  that, build it with `cd mobile && flutter build apk`) and connect the same way. The APK isn't signed with a
  store key: Android asks you to allow "unknown sources", and to install a newer version you **uninstall the
  old one first**. iPhone: not tested yet.

## Away from home: use TLS

Without encryption, the pairing key and your conversations travel in the clear. To leave your local network,
pick one:

- **Tailscale** (simplest): install Tailscale on the hub's machine and on your devices, and start the hub with
  `--tailscale-cert` (you need to turn on HTTPS certificates in the Tailscale admin panel). The address becomes
  `https://<machine>.<tailnet>.ts.net:7420`.
- **Your own certificate**: `--tls-cert fullchain.pem --tls-key privkey.pem --tls-host your.domain`.

Don't expose port 7420 to the internet without TLS.

## Without Docker (systemd)

If you'd rather run the `warden-server` binary directly (built with `cargo build --release -p warden-server`):

```ini
# /etc/systemd/system/warden.service
[Service]
User=warden
Environment=WARDEN_SERVER_AUTH_KEY=<the key>
ExecStart=/usr/local/bin/warden-server --data-dir /var/lib/warden serve --no-embedding-warm-up --run-tasks
Restart=on-failure
StateDirectory=warden

[Install]
WantedBy=multi-user.target
```
