#!/usr/bin/env python3
"""Monta as notas de uma release a partir dos commits entre duas tags (P164 a).

Os commits do Warden vão direto na main, com a mensagem `tipo: o que mudou, P123, Sessão 45`. Aqui eles se agrupam pelo tipo:
`feat` em "Novidades", `fix` em "Correções" e o resto (docs, test, ci, chore, refactor, perf, sem tipo) em "Outras mudanças",
dobradas, porque para quem baixa o instalador o que importa são as duas primeiras. Cada linha leva o hash curto. As referências
internas (", P152", ", Sessão 225") ficam na linha: quem escreveu o commit as quis lá.

É uma PROPOSTA: o `release-notes.yml` só a grava no corpo de um release em rascunho, que o Fabio revisa e publica.

Uso: scripts/release-notes.py TAG [--previous TAG]   (sem --previous, a tag `v*` anterior mais próxima; a primeira release lista tudo)
Só biblioteca padrão; precisa do histórico completo e das tags (`fetch-depth: 0`).
"""
import argparse
import re
import subprocess
import sys

KINDS = {"feat": "Novidades", "fix": "Correções"}
SUBJECT = re.compile(r"^(?P<kind>[a-z]+)(\([^)]*\))?!?:\s*(?P<text>.+)$")


def git(*args):
    return subprocess.run(["git", *args], check=True, capture_output=True, text=True).stdout


def previous_tag(tag):
    """A tag `v*` que vem antes de `tag` na história, ou None na primeira release."""
    try:
        return git("describe", "--tags", "--abbrev=0", "--match", "v*", f"{tag}^").strip() or None
    except subprocess.CalledProcessError:
        return None


def commits(tag, previous):
    span = f"{previous}..{tag}" if previous else tag
    out = git("log", "--no-merges", "--format=%h%x09%s", span)
    return [line.split("\t", 1) for line in out.splitlines() if "\t" in line]


def notes(tag, previous):
    groups = {"Novidades": [], "Correções": [], "Outras mudanças": []}
    for sha, subject in commits(tag, previous):
        m = SUBJECT.match(subject)
        kind = KINDS.get(m["kind"]) if m else None
        text = m["text"] if m and kind else (m["text"] if m else subject)
        label = kind or "Outras mudanças"
        # Em "Outras mudanças" o tipo ajuda a achar (docs, test, ci...): fica na linha.
        if label == "Outras mudanças" and m:
            text = f"{m['kind']}: {text}"
        groups[label].append(f"- {text} (`{sha}`)")

    lines = [f"## Warden {tag}", ""]
    if previous:
        lines += [f"Mudanças desde a {previous}. Esta é uma proposta montada dos commits: revise antes de publicar.", ""]
    else:
        lines += ["Primeira versão. Esta é uma proposta montada dos commits: revise antes de publicar.", ""]
    for label in ("Novidades", "Correções"):
        if groups[label]:
            lines += [f"### {label}", "", *groups[label], ""]
    if groups["Outras mudanças"]:
        lines += [f"<details><summary>Outras mudanças ({len(groups['Outras mudanças'])})</summary>", "", *groups["Outras mudanças"], "", "</details>", ""]
    if not any(groups.values()):
        lines += ["Nenhum commit novo entre as duas tags.", ""]
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("tag")
    parser.add_argument("--previous", help="a tag de onde contar (padrão: a `v*` anterior)")
    args = parser.parse_args()
    try:
        git("rev-parse", "--verify", "--quiet", f"{args.tag}^{{commit}}")
    except subprocess.CalledProcessError:
        sys.exit(f"não achei a tag {args.tag} (o checkout precisa de `fetch-depth: 0`)")
    sys.stdout.write(notes(args.tag, args.previous or previous_tag(args.tag)))


if __name__ == "__main__":
    main()
