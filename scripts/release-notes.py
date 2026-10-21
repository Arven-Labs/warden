#!/usr/bin/env python3
"""Builds the notes of a release from the commits between two tags (P164 a).

Warden's commits go straight to main, with the message `type: what changed, P123, Session 45`. Here they are grouped by type:
`feat` under "Features", `fix` under "Fixes" and the rest (docs, test, ci, chore, refactor, perf, no type) under "Other changes",
folded, because for whoever downloads the installer only the first two matter. Each line carries the short hash. The internal
references (", P152", ", Session 225") stay on the line: whoever wrote the commit wanted them there.

It is a PROPOSAL: `release-notes.yml` only writes it into the body of a draft release, which Fabio reviews and publishes.

Usage: scripts/release-notes.py TAG [--previous TAG]   (without --previous, the nearest earlier `v*` tag; the first release lists everything)
Standard library only; needs the full history and the tags (`fetch-depth: 0`).
"""
import argparse
import re
import subprocess
import sys

KINDS = {"feat": "Features", "fix": "Fixes"}
SUBJECT = re.compile(r"^(?P<kind>[a-z]+)(\([^)]*\))?!?:\s*(?P<text>.+)$")


def git(*args):
    return subprocess.run(["git", *args], check=True, capture_output=True, text=True).stdout


def previous_tag(tag):
    """The `v*` tag that comes before `tag` in the history, or None for the first release."""
    try:
        return git("describe", "--tags", "--abbrev=0", "--match", "v*", f"{tag}^").strip() or None
    except subprocess.CalledProcessError:
        return None


def commits(tag, previous):
    span = f"{previous}..{tag}" if previous else tag
    out = git("log", "--no-merges", "--format=%h%x09%s", span)
    return [line.split("\t", 1) for line in out.splitlines() if "\t" in line]


def notes(tag, previous):
    groups = {"Features": [], "Fixes": [], "Other changes": []}
    for sha, subject in commits(tag, previous):
        m = SUBJECT.match(subject)
        kind = KINDS.get(m["kind"]) if m else None
        text = m["text"] if m and kind else (m["text"] if m else subject)
        label = kind or "Other changes"
        # Under "Other changes" the type helps to find things (docs, test, ci...): it stays on the line.
        if label == "Other changes" and m:
            text = f"{m['kind']}: {text}"
        groups[label].append(f"- {text} (`{sha}`)")

    lines = [f"## Warden {tag}", ""]
    if previous:
        lines += [f"Changes since {previous}. This is a proposal built from the commits: review it before publishing.", ""]
    else:
        lines += ["First version. This is a proposal built from the commits: review it before publishing.", ""]
    for label in ("Features", "Fixes"):
        if groups[label]:
            lines += [f"### {label}", "", *groups[label], ""]
    if groups["Other changes"]:
        lines += [f"<details><summary>Other changes ({len(groups['Other changes'])})</summary>", "", *groups["Other changes"], "", "</details>", ""]
    if not any(groups.values()):
        lines += ["No new commits between the two tags.", ""]
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("tag")
    parser.add_argument("--previous", help="the tag to count from (default: the previous `v*`)")
    args = parser.parse_args()
    try:
        git("rev-parse", "--verify", "--quiet", f"{args.tag}^{{commit}}")
    except subprocess.CalledProcessError:
        sys.exit(f"could not find the tag {args.tag} (the checkout needs `fetch-depth: 0`)")
    sys.stdout.write(notes(args.tag, args.previous or previous_tag(args.tag)))


if __name__ == "__main__":
    main()
