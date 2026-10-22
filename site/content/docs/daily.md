# Day to day

The most useful features to start with. Each has its own page or a section in the
[repository README](https://github.com/Arven-Labs/warden#readme).

- **Scheduled tasks** ("every weekday at 8, summarize the news"): `warden tasks add`, or the Tasks tab. They
  only run on a hub started with `--run-tasks`. See [Scheduled tasks](/docs/tasks/).
- **Agents** with their own personality, memory and tools: in Settings, or from the terminal (`warden`, command
  `/agents`).
- **Skills**: instructions the model loads when it needs them; ask for one in a conversation ("create a skill
  for...").
- **OpenAI-compatible API**: `http://<hub>:7420/v1`, with a key from `warden api-keys create`. See
  [Warden API](/docs/api/).
- **More people** (family, team): `warden users add ana`, each with an encrypted vault and conversations. See
  [People](/docs/people/).
- **Lend another machine** (a home PC, a second VPS) to your agents: `warden-server node ...`. See
  [Nodes](/docs/nodes/).
