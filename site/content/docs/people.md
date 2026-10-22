# People

You're the owner: whoever holds the pairing key. Add other people and they sign in on the web or the phone
with their own username and password, each with their own vault and conversations:

```bash
warden-server users add ana --name "Ana"   # prints a provisional password, once
warden-server users list
warden-server users reset-password ana
warden-server users remove ana             # revokes her devices; her vault and conversations stay on disk
warden-server users removed                # removed members whose encrypted data is still there
warden-server users restore ana            # brings her back, same password, same data
warden-server users purge ana --yes        # deletes her data and the key for good
```

The web's People tab and the desktop's Workspace do the same. A member picks their own password on the first
sign-in and gets none of the hub's administration. What else they get is up to you.

## What a member gets

- **Agents**: only the ones you share with them (`shared_with = ["ana"]` or `["*"]`, or "Shared with" in
  Settings), always with *their* memory. They can also make agents of their own, which only they see.
- **Tools**: a safe default (their own vault's files and skills, web search), or the list you set for them in
  People. The shell, nodes and your MCP integrations only if you tick them.
- **Spending**: a `[[limits]]` entry with `scope = "person"` and `target = "ana"` caps them on every channel,
  and they can't approve going past it.
- **Warden API**: keys of their own (the web's API tab), which speak as them.
- **Your memory**: none of it, unless you share a folder of your vault with them (see below).

## Shared spaces

A shared folder shows up at `compartilhado/<name>/` in the member's own vault, reading only or writing too,
from their next message:

```bash
warden-server spaces add casa --folder casa --reader ana --writer bruno   # or --reader '*' for everyone
warden-server spaces list
warden-server spaces remove casa   # the folder and its notes stay in your vault
```

The web's People tab ("Espaços compartilhados") and the desktop's Workspace ("Shared spaces") do the same.

## Their data is theirs

A member's vault (contents and file names), search index and conversations are encrypted on the hub's disk
with a key made from *their own* password, never the provisional one. You can't read them, and neither can a
copy of the disk. They get a **recovery code** once, on the web, for the day they forget the password or you
reset it; without it a reset means the data is gone. The hub keeps the key in memory from sign-in until it
restarts, and a member signs in with the password once after that. The web says so, and the honest limit
stays: whoever controls the hub's machine can see what an agent sees while it works.

## Who can help when they lose both

You choose, for the whole workspace:

- `private`: nobody (the default).
- `consent`: you and *their* recovery code together; neither opens it alone.
- `company`: you alone, with your recovery key; each use is recorded and they're told when they next sign in.

The key is shown once and typed in for each recovery: the hub only keeps its public half. A change to a
weaker policy waits for each person's yes.

```bash
warden-server recovery policy consent                  # makes your recovery key the first time and shows it once
warden-server recovery recover ana --key <key> --code <ana's code>   # consent needs both; company only --key
warden-server recovery log
```

For backing up members' data, see [Backup and updates](/docs/backup/).
