# Warden API

Warden speaks the OpenAI API, so any OpenAI client can talk to your agents. With a hub running
(`warden-server serve`, or the desktop's embedded hub), create a key and use it as the client's API key:

```bash
warden-server api-keys create n8n        # prints the key once; also in Settings (desktop and web)
curl http://<hub>:7420/v1/chat/completions \
  -H "Authorization: Bearer wdn_..." -H "Content-Type: application/json" \
  -d '{"model": "warden", "messages": [{"role": "user", "content": "What did I write about the trip?"}]}'
```

`GET /v1/models` lists `warden` and one `warden/<agent>` per configured agent. A key can also be bound to one
agent (`api-keys create bot --agent <id>`, or the selector in Settings): it then only speaks as that agent,
whatever `model` says. Streaming (`"stream": true`) works, nothing is saved as a conversation, and the spend
counts toward the `api` channel's limits.

## Function calling

Function calling works too, so coding harnesses (opencode and the like) can bring their own tools. The model
sees the client's `tools` next to the agent's own (the client's wins on a name clash). A call to one of the
client's tools comes back as `tool_calls` with `finish_reason: "tool_calls"`, and the turn continues when the
client sends the `tool` results back. To narrow which of the agent's own tools (shell, files...) such a client
can reach, bind its key to an agent with `allowed_tools`. `tool_choice: "none"` leaves the client's tools out,
and any other choice acts as `auto`.
