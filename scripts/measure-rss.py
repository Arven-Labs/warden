#!/usr/bin/env python3
"""Mede o RSS do `warden-server` em release (P145, P146): ocioso sem e com os modelos de busca e depois do 1º turno com
cofres de tamanhos diferentes, contra o que a Sessão 222 mediu (HOSPEDAGEM.md seção K). Só biblioteca padrão, Linux.

Cada cenário sobe um hub novo numa pasta de dados nova, com um modelo de mentira (scripts/fake-openai.py) no lugar do
provedor; o turno vai pela API compatível com a OpenAI e o RSS vem de GET /status, o pico (VmHWM) de /proc/<pid>/status.
Notas sintéticas em português, com semente fixa: "curta" = 2 a 8 linhas de 4 a 10 palavras, "longa" = 10 a 30 linhas de
8 a 18 palavras (a definição da seção K.1). Não é um cofre real e a qualidade da busca não é avaliada.

Uso: scripts/measure-rss.py [--bin target/release/warden-server] [--models-dir DIR] [--only NOME ...]
Imprime uma tabela Markdown em stdout; o progresso vai para stderr.
"""
import argparse
import json
import os
import random
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
HUB_PORT = 17430
FAKE_PORT = 17431
KEY = "measure-rss-pairing-key-0123456789abcdef"  # o hub exige 32 caracteres ou mais

WORDS = (
    "chave rotação sexta projeto reunião cliente servidor memória nota agenda tarefa relatório código revisão "
    "entrega prazo equipe orçamento contrato fatura pagamento backup instância cofre modelo busca índice vetor "
    "conversa mensagem usuário senha acesso permissão agente skill integração fluxo etapa decisão risco custo "
    "margem crédito saldo plano assinatura suporte incidente alerta métrica painel gráfico dado tabela arquivo"
).split()

# Cenário: (nome, notas, tamanho, com warm-up, dá um turno?, o que a Sessão 222 mediu em MB, e o alvo do benchmark)
SCENARIOS = [
    ("ocioso, sem modelos", 0, "short", False, False, "24,8", "~25"),
    ("ocioso, warm-up dos dois modelos", 0, "short", True, False, "960", "~960"),
    ("1º turno, 50 notas curtas", 50, "short", False, True, "358", "-"),
    ("1º turno, 300 notas curtas", 300, "short", False, True, "1290", "-"),
    ("1º turno, 1000 notas curtas", 1000, "short", False, True, "1700", "~470"),
    ("1º turno, 300 notas longas", 300, "long", False, True, "8200", "~1111"),
]


def log(msg):
    print(msg, file=sys.stderr, flush=True)


def write_notes(vault, count, size):
    rng = random.Random(42)
    vault.mkdir(parents=True, exist_ok=True)
    lines_range, words_range = ((2, 8), (4, 10)) if size == "short" else ((10, 30), (8, 18))
    for i in range(count):
        lines = [" ".join(rng.choices(WORDS, k=rng.randint(*words_range))) for _ in range(rng.randint(*lines_range))]
        (vault / f"nota-{i:05d}.md").write_text(f"# Nota {i}\n\n" + "\n".join(lines) + "\n", encoding="utf-8")


def http(url, headers=None, body=None, timeout=30):
    req = urllib.request.Request(url, data=body, headers=headers or {})
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        return resp.read()


def rss_mb(pid):
    """RSS e pico, em MB, de /proc/<pid>/status."""
    fields = {}
    for line in Path(f"/proc/{pid}/status").read_text().splitlines():
        if line.startswith(("VmRSS:", "VmHWM:")):
            name, value = line.split(":", 1)
            fields[name] = int(value.split()[0]) / 1024
    return fields["VmRSS"], fields["VmHWM"]


def status_rss_mb(hub_url):
    body = http(f"{hub_url}/status", {"Authorization": f"Bearer {KEY}"})
    return json.loads(body)["rss_bytes"] / (1024 * 1024)


def wait_settled(pid, minimum, maximum):
    """Espera o RSS parar de mexer (o warm-up carrega os modelos em segundo plano)."""
    start = time.time()
    last, stable = -1.0, 0
    while time.time() - start < maximum:
        time.sleep(1)
        now = rss_mb(pid)[0]
        stable = stable + 1 if abs(now - last) < 1.0 else 0
        last = now
        if stable >= 5 and time.time() - start >= minimum:
            return
    log("  aviso: o RSS não estabilizou no prazo")


def run_scenario(binary, models_dir, scenario):
    name, notes, size, warm_up, turn, _, _ = scenario
    data = Path(tempfile.mkdtemp(prefix="measure-rss-"))
    hub = fake = None
    try:
        # Os modelos baixados são compartilhados entre os cenários (e, no CI, entre as rodadas).
        models_dir.mkdir(parents=True, exist_ok=True)
        (data / "models").symlink_to(models_dir)
        (data / "config.toml").write_text(
            'active_provider = "main"\n\n[[providers]]\nid = "main"\nkind = "openai"\napi_key = "measure"\n'
            f'base_url = "http://127.0.0.1:{FAKE_PORT}/v1"\nmodel = "fake"\n'
        )
        write_notes(data / "vault", notes, size)
        env = dict(os.environ, WARDEN_SERVER_AUTH_KEY=KEY)
        common = [str(binary), "--data-dir", str(data)]
        api_key = subprocess.run(
            common + ["api-keys", "create", "measure"], env=env, check=True, capture_output=True, text=True
        ).stdout.strip()
        fake = subprocess.Popen([sys.executable, str(HERE / "fake-openai.py"), str(FAKE_PORT)])
        serve = common + ["serve", "--listen", f"127.0.0.1:{HUB_PORT}"]
        if not warm_up:
            serve.append("--no-embedding-warm-up")
        hub = subprocess.Popen(serve, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        hub_url = f"http://127.0.0.1:{HUB_PORT}"
        for _ in range(120):
            try:
                http(f"{hub_url}/health", timeout=2)
                break
            except OSError:
                if hub.poll() is not None:
                    raise RuntimeError("o hub parou antes de responder /health")
                time.sleep(0.5)
        else:
            raise RuntimeError("o hub não respondeu /health em 60 s")
        before = rss_mb(hub.pid)[0]
        seconds = None
        if turn:
            started = time.time()
            payload = json.dumps({"model": "warden", "messages": [{"role": "user", "content": "Quando a chave roda?"}]})
            http(
                f"{hub_url}/v1/chat/completions",
                {"Authorization": f"Bearer {api_key}", "Content-Type": "application/json"},
                payload.encode(),
                timeout=1800,
            )
            seconds = time.time() - started
            wait_settled(hub.pid, 2, 30)
        elif warm_up:
            wait_settled(hub.pid, 10, 300)
        after, peak = rss_mb(hub.pid)
        reported = status_rss_mb(hub_url)
        log(f"  {name}: antes {before:.0f} MB, depois {after:.0f} MB, pico {peak:.0f} MB, /status {reported:.0f} MB")
        return {"after": after, "peak": peak, "seconds": seconds}
    finally:
        for proc in (hub, fake):
            if proc is not None and proc.poll() is None:
                proc.terminate()
                try:
                    proc.wait(timeout=15)
                except subprocess.TimeoutExpired:
                    proc.kill()
        shutil.rmtree(data, ignore_errors=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--bin", default="target/release/warden-server", help="o binário em release")
    parser.add_argument(
        "--models-dir",
        default=str(Path.home() / ".cache" / "warden" / "models"),
        help="onde ficam os modelos de busca já baixados (compartilhado entre os cenários)",
    )
    parser.add_argument("--only", nargs="*", help="rodar só os cenários cujo nome contém algum destes textos")
    args = parser.parse_args()
    binary = Path(args.bin).resolve()
    if not binary.exists():
        sys.exit(f"não achei o binário {binary} (cargo build --release -p warden-server)")

    rows = []
    for scenario in SCENARIOS:
        if args.only and not any(token in scenario[0] for token in args.only):
            continue
        log(f"cenário: {scenario[0]}")
        result = run_scenario(binary, Path(args.models_dir), scenario)
        rows.append((scenario, result))

    print("| Cenário | RSS depois (MB) | Pico (MB) | 1º turno (s) | Sessão 222 (MB) | Alvo do benchmark (MB) |")
    print("|---|---:|---:|---:|---:|---:|")
    for (name, _, _, _, _, before, target), r in rows:
        secs = "-" if r["seconds"] is None else f"{r['seconds']:.1f}"
        print(f"| {name} | {r['after']:.0f} | {r['peak']:.0f} | {secs} | {before} | {target} |")


if __name__ == "__main__":
    main()
