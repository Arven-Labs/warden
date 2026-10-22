# Scheduled tasks

A task is a prompt an agent runs on a schedule, kept as `[[tasks]]` in `config.toml`. Only a hub started with
`--run-tasks` runs them (the config syncs, so switch it on in the one hub that's always up):

```bash
warden-server tasks add news --agent reader --cron "0 8 * * 1-5" --timezone America/Sao_Paulo \
  --prompt "Summarize today's headlines"
warden-server tasks add check --every 2h --prompt "Is the site up?"   # or --once 2026-10-01T09:00
warden-server tasks list          # schedule, last run and next run
warden-server tasks pause news    # resume / remove / run <id> (run it now, here)
warden-server serve --run-tasks
```

Each run adds the prompt and the answer to the task's conversation (`task-<id>`), which every device lists and
can keep talking in. A run missed while the hub was down happens once when it's back. Nobody is watching a run,
so a tool that needs approval refuses, and the spending limits count it under the `tasks` channel.

## On screen

The web app's Tasks tab (every change asks for the pairing key, like the API keys) and the desktop's **Tasks**
screen do the same. The desktop screen also has this computer's own switch to run tasks on the embedded hub,
kept in `hub-local.json`, outside the synced config.

In a chat, an agent with "can create and edit scheduled tasks" switched on (Settings or `/agents`) turns
"every weekday at 8, summarize the news" into a task, after you approve the card it shows.
