#!/usr/bin/env python3
"""Compares a hub RSS measurement (scripts/measure-rss.py --json) with the baseline and says whether any scenario went over the limit (P162).

Only RSS counts: the 1st-turn time varies about 15% between runs of the same code (runs 38968961049 to 38993373650 of measure-rss.yml),
and RSS, 1 to 2%. A scenario goes over the limit when it grows more than the larger of `--limit` (a fraction of the baseline, 0.10) and
`--floor-mb` (5 MB, so the 24 MB idle one does not alert for 1 MB). Going down does not alert ("improved"); a scenario that exists on only one side is
reported and does not alert.

The baseline is `scripts/rss-baseline.json`, in git. **When a deliberate change alters memory (a model that now unloads, a new
batch), the baseline is updated in a separate commit, with the new measurement**: that is what lets next week compare against what was
accepted, and not against what already went over the limit.

Usage: scripts/compare-rss.py baseline.json current.json [--limit 0.10] [--floor-mb 5]
Prints the table as Markdown; with GITHUB_OUTPUT set, writes `regressed=true` or `regressed=false`. Exits 1 when it went over the limit.
"""
import argparse
import json
import os
import sys


def load(path):
    with open(path, encoding="utf-8") as f:
        return json.load(f)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("baseline")
    parser.add_argument("current")
    parser.add_argument("--limit", type=float, default=0.10, help="fraction of the baseline that RSS may grow (0.10 = 10%%)")
    parser.add_argument("--floor-mb", type=float, default=5.0, help="minimum growth in MB to count, whatever the fraction")
    args = parser.parse_args()

    baseline = load(args.baseline)
    current = load(args.current)
    regressed = []
    rows = []
    for name in sorted(set(baseline) | set(current), key=lambda n: list(baseline).index(n) if n in baseline else len(baseline)):
        before = baseline.get(name, {}).get("rss")
        after = current.get(name, {}).get("rss")
        if before is None:
            rows.append((name, "-", f"{after:.0f}", "-", "new (no baseline)"))
        elif after is None:
            rows.append((name, f"{before:.0f}", "-", "-", "not measured this time"))
        else:
            delta = after - before
            allowed = max(args.limit * before, args.floor_mb)
            if delta > allowed:
                state = f"**OVER** (limit +{allowed:.0f} MB)"
                regressed.append(name)
            elif delta < -allowed:
                state = "improved"
            else:
                state = "within the limit"
            rows.append((name, f"{before:.0f}", f"{after:.0f}", f"{delta:+.0f}", state))

    print("| Scenario | Baseline (MB) | Now (MB) | Difference (MB) | State |")
    print("|---|---:|---:|---:|---|")
    for row in rows:
        print("| " + " | ".join(row) + " |")
    print()
    if regressed:
        print(f"{len(regressed)} scenario(s) went over the {args.limit:.0%} limit (at least {args.floor_mb:.0f} MB): " + ", ".join(regressed) + ".")
    else:
        print(f"No scenario went over the {args.limit:.0%} limit (at least {args.floor_mb:.0f} MB).")

    out = os.environ.get("GITHUB_OUTPUT")
    if out:
        with open(out, "a", encoding="utf-8") as f:
            f.write(f"regressed={'true' if regressed else 'false'}\n")
    sys.exit(1 if regressed else 0)


if __name__ == "__main__":
    main()
