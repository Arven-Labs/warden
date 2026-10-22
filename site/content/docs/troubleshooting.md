# Troubleshooting

| Symptom | What to look at |
|---|---|
| The hub doesn't answer | `docker logs warden`; `curl localhost:7420/health`; is port 7420 already in use? |
| The container dies on start | `WARDEN_SERVER_AUTH_KEY` is missing (minimum 32 characters), or `config.toml` has a field that doesn't exist |
| "no API key configured" | the active provider's key wasn't filled in under Settings, or it's empty in `config.toml` |
| The browser doesn't ask for or accept the key | check that it's the `WARDEN_SERVER_AUTH_KEY` value, and whether the address is `http://` (no TLS) or `https://` |
| The app doesn't open on Linux | install from the `.deb`/`.rpm` (they pull in the libraries) and run it in a terminal to see the error (the `.deb`'s executable is `warden-desktop`, and `desktop` in release v0.1.1; for the AppImage, the file itself) |
| Slow answers on the first search | the semantic-search model downloads the first time and stays in `/data/models` |
