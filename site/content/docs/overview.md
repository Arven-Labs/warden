# Overview

Warden is a personal AI agent that lives on your own devices. It remembers things in a plain markdown vault
you fully own, and it reaches you wherever you already are: desktop, phone, terminal, browser, Telegram or
WhatsApp.

This guide takes you from zero to a conversation with your agent. You don't need to compile anything:
everything comes from the [latest release](https://github.com/Arven-Labs/warden/releases/latest).

## Choose your path

| | **A. Just the desktop app** | **B. Always-on hub** |
|---|---|---|
| For whom | trying it out and using it on a single computer | using it from your phone and browser, and having scheduled tasks run while your PC is off |
| What you install | the desktop app | the `warden-server` Docker image on a VPS, a home server or your own PC |
| You need | a model key (or a local Ollama) | Docker, a model key and one port (7420) |

You can start with A and add B later: the app connects to any hub.

- Path A is covered in [Desktop app](/docs/desktop/).
- Path B is covered in [Always-on hub](/docs/hub/).

## Where to go next

- [Day to day](/docs/daily/): the features worth knowing first.
- [Warden API](/docs/api/): use Warden from any OpenAI-compatible client.
- [Scheduled tasks](/docs/tasks/), [Nodes](/docs/nodes/) and [People](/docs/people/): the hub's bigger features.
- [Backup and updates](/docs/backup/) and [Troubleshooting](/docs/troubleshooting/).
- [What isn't verified yet](/docs/status/): an honest list of what has only run against a fake model.

The source code and the project's planning notes live in the
[repository](https://github.com/Arven-Labs/warden).
