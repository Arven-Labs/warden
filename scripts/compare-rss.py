#!/usr/bin/env python3
"""Compara uma medição de RSS do hub (scripts/measure-rss.py --json) com a de referência e diz se algum cenário passou do limite (P162).

Só o RSS conta: o tempo do 1º turno varia uns 15% entre rodadas do mesmo código (runs 38968961049 a 38993373650 do measure-rss.yml),
e o RSS, 1 a 2%. Um cenário passa do limite quando sobe mais que o maior entre `--limit` (fração da referência, 0,10) e `--floor-mb`
(5 MB, para o ocioso de 24 MB não alertar por 1 MB). Descer não alerta ("melhorou"); cenário que só existe de um lado é dito e não alerta.

A referência é `scripts/rss-baseline.json`, no git. **Quando uma mudança de propósito altera a memória (um modelo que passa a
descarregar, um lote novo), a referência é atualizada num commit à parte, com a medição nova**: é o que deixa a próxima semana
comparar com o que se aceitou, e não com o que já passou do limite.

Uso: scripts/compare-rss.py referencia.json atual.json [--limit 0.10] [--floor-mb 5]
Imprime a tabela em Markdown; com GITHUB_OUTPUT definido, escreve `regressed=true` ou `regressed=false`. Sai com 1 quando passou do limite.
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
    parser.add_argument("--limit", type=float, default=0.10, help="fração da referência que o RSS pode subir (0,10 = 10%%)")
    parser.add_argument("--floor-mb", type=float, default=5.0, help="subida mínima em MB para contar, qualquer que seja a fração")
    args = parser.parse_args()

    baseline = load(args.baseline)
    current = load(args.current)
    regressed = []
    rows = []
    for name in sorted(set(baseline) | set(current), key=lambda n: list(baseline).index(n) if n in baseline else len(baseline)):
        before = baseline.get(name, {}).get("rss")
        after = current.get(name, {}).get("rss")
        if before is None:
            rows.append((name, "-", f"{after:.0f}", "-", "novo (sem referência)"))
        elif after is None:
            rows.append((name, f"{before:.0f}", "-", "-", "não medido desta vez"))
        else:
            delta = after - before
            allowed = max(args.limit * before, args.floor_mb)
            if delta > allowed:
                state = f"**PASSOU** (limite +{allowed:.0f} MB)"
                regressed.append(name)
            elif delta < -allowed:
                state = "melhorou"
            else:
                state = "dentro do limite"
            rows.append((name, f"{before:.0f}", f"{after:.0f}", f"{delta:+.0f}", state))

    print("| Cenário | Referência (MB) | Agora (MB) | Diferença (MB) | Estado |")
    print("|---|---:|---:|---:|---|")
    for row in rows:
        print("| " + " | ".join(row) + " |")
    print()
    if regressed:
        print(f"{len(regressed)} cenário(s) passaram do limite de {args.limit:.0%} (no mínimo {args.floor_mb:.0f} MB): " + ", ".join(regressed) + ".")
    else:
        print(f"Nenhum cenário passou do limite de {args.limit:.0%} (no mínimo {args.floor_mb:.0f} MB).")

    out = os.environ.get("GITHUB_OUTPUT")
    if out:
        with open(out, "a", encoding="utf-8") as f:
            f.write(f"regressed={'true' if regressed else 'false'}\n")
    sys.exit(1 if regressed else 0)


if __name__ == "__main__":
    main()
