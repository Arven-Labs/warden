# Desktop app

Path A: the app on your computer. Download the file for your system from the
[release page](https://github.com/Arven-Labs/warden/releases/latest).

## Install

| System | File | How |
|---|---|---|
| Debian, Ubuntu | `Warden_<version>_amd64.deb` | `sudo apt install ./Warden_*_amd64.deb` (`apt` pulls in any missing libraries) |
| Fedora | `Warden-<version>-1.x86_64.rpm` | `sudo dnf install ./Warden-*.rpm` |
| Any Linux | `Warden_<version>_amd64.AppImage` | `chmod +x Warden_*.AppImage && ./Warden_*.AppImage` |
| Windows | `Warden_<version>_x64_en-US.msi` (or `..._x64-setup.exe`) | double-click |
| macOS (Apple Silicon) | `Warden_<version>_aarch64.dmg` | drag to Applications |

The installers aren't signed yet: Windows (SmartScreen) and macOS (Gatekeeper) warn that the publisher is
unknown. That's expected; check that the file came from the release above.

## Connect a model

1. Open Warden and go to **Settings → Model providers → + Add provider**.
2. Pick the type and fill it in:
   - **gemini**, **openai**, **anthropic**: paste your account key.
   - **openai_compatible**: any server that speaks the OpenAI format (Ollama, OpenRouter, Groq, DeepSeek).
     Give the `base_url` (Ollama's is `http://localhost:11434/v1`; the key can be any text) and the model name.
3. Mark the provider as active and save.
4. Open a conversation and write something.

Keys are stored in `config.secrets.toml` (readable only by your user), not in `config.toml`, which is the
file people copy and paste around.

## Node.js (optional)

Web search and the Google integration run through `npx`: install [Node.js](https://nodejs.org) if you want
those two. Without it everything else works, and the error says what's missing. (The hub's Docker image
doesn't ship Node.js, so those two tools are left out there.)

## Where things live

| What | Where (Linux) |
|---|---|
| Configuration | `~/.config/warden/config.toml` |
| Provider keys | `~/.config/warden/config.secrets.toml` |
| Memory vault (markdown notes) | `~/Warden/vault` (changeable in Settings) |

On Windows and macOS the configuration lives in the system's config folder, under `warden`. The vault is a
folder of `.md` files that Obsidian can open too.

## Let your computer be the hub (optional)

For your phone or another computer to use this app as a server, go to **Workspace → Be this network's hub**,
choose a port and switch it on. Once on, it comes back by itself every time the app opens, and other devices
on the same network find it on their own. To use it from outside your home, [the always-on hub](/docs/hub/)
is better: it doesn't depend on your PC being on.
