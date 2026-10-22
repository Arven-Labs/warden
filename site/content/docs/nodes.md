# Nodes

Any other machine (a home PC, a second VPS) can lend its shell and/or a folder to the hub's agents, while
everything else stays on the hub:

```bash
# on that machine — the pairing key is only needed the first time
warden-server node --hub wss://<hub>:7420 --auth-key <key> --description "home PC" --tag home --shell --files ~/shared
# on the hub (or the web's Devices tab / the desktop's Workspace)
warden-server devices approve <node id>
warden-server nodes allow <node id> --agent ops --approval
```

Agents then get `list_nodes`, `node_shell`, `node_read_file`, `node_write_file` and `node_list_files`.

## What a node can lend

- **Shell and files**: the flags above.
- **MCP servers** from its own `config.toml`, by name (`--mcp github --mcp postgres`): each of their tools
  shows up on the hub as `<node>__<tool>`, with its own schema, while the node is online.
- **A model** (`--model ollama`, a provider in its own config): on the hub, a provider with `kind = "node"`,
  the node's id and `model = "ollama"` streams from it. Put it in a combo with a cloud model and the cloud
  answers whenever the home machine is off.

## Two locks

The node chooses what it lends; the hub chooses which agents may use it and whether each call asks you first.
Every call is logged to `node_audit.jsonl`. A call cut short by the node dropping fails right away and is
never retried.

## The desktop app as a node

The desktop app can be a node too: **Workspace → Lend this computer** takes the hub's address, what to lend
(shell, a folder, MCP servers, models) and the pairing key once, reconnects on its own, comes back on at
launch, and lists what the hub's agents did on this computer. The setting stays on this computer (it isn't
synced).
