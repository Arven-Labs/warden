---
name: commit-message
description: Write a good commit message from a diff or a description of a change. Use when the person asks for a commit message or a pull request title and description.
---
Write the commit message in this shape:

- A first line of at most 72 characters, in the imperative mood ("Fix", "Add", "Remove"), that says what the change does. No trailing period.
- A blank line.
- A body of short paragraphs or bullets that explains why the change was needed and anything a reviewer could miss: a behavior that changes, a limit, a follow-up left for later. Wrap lines near 72 characters.

Rules:
- Describe the change that is in the diff, not what was planned.
- Match the language and the style of earlier commit messages if the person shows some; otherwise write in English.
- One logical change per message. If the diff mixes unrelated changes, say so and suggest splitting it.
- Do not invent ticket numbers or authors. Leave them out when unknown.
