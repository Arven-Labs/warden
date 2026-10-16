#!/usr/bin/env python3
"""Mede o RSS do `warden-server` em release (P145, P146, P147): ocioso sem e com os modelos de busca, depois do 1º turno
com cofres de tamanhos diferentes, depois de o modelo do cofre ser descarregado por ociosidade e com vários membros, contra
o que a Sessão 222 mediu (HOSPEDAGEM.md seção K). Só biblioteca padrão, Linux.

Cada cenário sobe um hub novo numa pasta de dados nova, com um modelo de mentira (scripts/fake-openai.py) no lugar do
provedor; o turno vai pela API compatível com a OpenAI e o RSS vem de /proc/<pid>/status (conferido com GET /status), o
pico (VmHWM) também. Notas sintéticas em português, com semente fixa: "curta" = 2 a 8 linhas de 4 a 10 palavras, "longa" =
10 a 30 linhas de 8 a 18 palavras (a definição da seção K.1). Não é um cofre real e a qualidade da busca não é avaliada.
O modelo do histórico não é medido: ele só carrega num `search_history` pedido pelo modelo, e o de mentira não pede tool.

Uso: scripts/measure-rss.py [--bin target/release/warden-server] [--models-dir DIR] [--only TEXTO ...]
Imprime tabelas Markdown em stdout; o progresso vai para stderr.
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
CLK_TCK = os.sysconf("SC_CLK_TCK")
MEMBERS = 3
MEMBER_NOTES = 50
MEMBER_FOLLOW_UPS = 5
IDLE_SECS = 5  # --embedding-idle-secs do cenário de descarregamento

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


def write_notes(vault, count, size, seed=42):
    rng = random.Random(seed)
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


def cpu_seconds(pid):
    """CPU (usuário + sistema) que o processo já gastou, de /proc/<pid>/stat."""
    stat = Path(f"/proc/{pid}/stat").read_text()
    fields = stat[stat.rindex(")") + 2 :].split()  # depois do "(comm)"; o campo 3 do stat é o índice 0
    return (int(fields[11]) + int(fields[12])) / CLK_TCK


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


class Hub:
    """Um hub novo numa pasta de dados nova, com o modelo de mentira ao lado. Uso: `with Hub(binary, models) as hub:`."""

    def __init__(self, binary, models_dir):
        self.binary = binary
        self.models_dir = models_dir
        self.data = None
        self.proc = None
        self.fake = None
        self.env = dict(os.environ, WARDEN_SERVER_AUTH_KEY=KEY)
        self.url = f"http://127.0.0.1:{HUB_PORT}"

    def __enter__(self):
        self.data = Path(tempfile.mkdtemp(prefix="measure-rss-"))
        # Os modelos baixados são compartilhados entre os cenários (e, no CI, entre as rodadas).
        self.models_dir.mkdir(parents=True, exist_ok=True)
        (self.data / "models").symlink_to(self.models_dir)
        (self.data / "config.toml").write_text(
            'active_provider = "main"\n\n[[providers]]\nid = "main"\nkind = "openai"\napi_key = "measure"\n'
            f'base_url = "http://127.0.0.1:{FAKE_PORT}/v1"\nmodel = "fake"\n'
        )
        return self

    def __exit__(self, *_):
        for proc in (self.proc, self.fake):
            if proc is not None and proc.poll() is None:
                proc.terminate()
                try:
                    proc.wait(timeout=15)
                except subprocess.TimeoutExpired:
                    proc.kill()
        shutil.rmtree(self.data, ignore_errors=True)

    def cli(self, *args):
        """Um subcomando do próprio hub contra a pasta de dados; devolve o que ele imprimiu em stdout."""
        cmd = [str(self.binary), "--data-dir", str(self.data), *args]
        return subprocess.run(cmd, env=self.env, check=True, capture_output=True, text=True).stdout.strip()

    def start(self, warm_up=False, idle_secs=None):
        self.fake = subprocess.Popen([sys.executable, str(HERE / "fake-openai.py"), str(FAKE_PORT)])
        serve = [str(self.binary), "--data-dir", str(self.data), "serve", "--listen", f"127.0.0.1:{HUB_PORT}"]
        if not warm_up:
            serve.append("--no-embedding-warm-up")
        if idle_secs is not None:
            serve += ["--embedding-idle-secs", str(idle_secs)]
        self.proc = subprocess.Popen(serve, env=self.env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        for _ in range(120):
            try:
                http(f"{self.url}/health", timeout=2)
                return
            except OSError:
                if self.proc.poll() is not None:
                    raise RuntimeError("o hub parou antes de responder /health")
                time.sleep(0.5)
        raise RuntimeError("o hub não respondeu /health em 60 s")

    @property
    def pid(self):
        return self.proc.pid

    def turn(self, api_key):
        """Um turno pela API. Devolve (segundos de relógio, segundos de CPU do hub)."""
        started, cpu = time.time(), cpu_seconds(self.pid)
        payload = json.dumps({"model": "warden", "messages": [{"role": "user", "content": "Quando a chave roda?"}]})
        http(
            f"{self.url}/v1/chat/completions",
            {"Authorization": f"Bearer {api_key}", "Content-Type": "application/json"},
            payload.encode(),
            timeout=1800,
        )
        return time.time() - started, cpu_seconds(self.pid) - cpu


def run_scenario(binary, models_dir, scenario):
    name, notes, size, warm_up, turn, _, _ = scenario
    with Hub(binary, models_dir) as hub:
        write_notes(hub.data / "vault", notes, size)
        api_key = hub.cli("api-keys", "create", "measure")
        hub.start(warm_up=warm_up)
        before = rss_mb(hub.pid)[0]
        seconds = None
        if turn:
            seconds, _ = hub.turn(api_key)
            wait_settled(hub.pid, 2, 30)
        elif warm_up:
            wait_settled(hub.pid, 10, 300)
        after, peak = rss_mb(hub.pid)
        reported = status_rss_mb(hub.url)
        log(f"  {name}: antes {before:.0f} MB, depois {after:.0f} MB, pico {peak:.0f} MB, /status {reported:.0f} MB")
        return {"after": after, "peak": peak, "seconds": seconds}


def run_unload(binary, models_dir):
    """P146 c: depois do 1º turno com 1000 notas o modelo do cofre fica ocioso e é descarregado; um 2º turno o recarrega."""
    with Hub(binary, models_dir) as hub:
        write_notes(hub.data / "vault", 1000, "short")
        api_key = hub.cli("api-keys", "create", "measure")
        hub.start(idle_secs=IDLE_SECS)
        first, _ = hub.turn(api_key)
        wait_settled(hub.pid, 2, 30)
        loaded, loaded_peak = rss_mb(hub.pid)
        time.sleep(IDLE_SECS + 10)  # a tarefa confere a cada idle/4 (no mínimo 1 s)
        unloaded, _ = rss_mb(hub.pid)
        again, _ = hub.turn(api_key)
        wait_settled(hub.pid, 2, 30)
        reloaded, reloaded_peak = rss_mb(hub.pid)
        log(f"  descarregamento: carregado {loaded:.0f} MB, ocioso {unloaded:.0f} MB, recarregado {reloaded:.0f} MB")
        return [
            ("1000 notas curtas, depois do 1º turno", loaded, loaded_peak, first, "1700", "-"),
            (f"… {IDLE_SECS + 10} s ocioso (`--embedding-idle-secs {IDLE_SECS}`)", unloaded, None, None, "76 (só o modelo, K.2)", "~76"),
            ("… 2º turno (recarrega o modelo)", reloaded, reloaded_peak, again, "-", "-"),
        ]


def run_members(binary, models_dir):
    """P147: vários membros, cada um com a sua chave. O `Vault` (e o modelo) de cada um deve viver entre as requisições."""
    rows = []
    with Hub(binary, models_dir) as hub:
        keys = {}
        for i in range(1, MEMBERS + 1):
            member = f"m{i}"
            hub.cli("users", "add", member, "--name", f"Membro {i}")
            write_notes(hub.data / "users" / member / "vault", MEMBER_NOTES, "short", seed=i)
            keys[member] = hub.cli("api-keys", "create", f"key-{member}", "--user", member)
        hub.start()
        base = rss_mb(hub.pid)[0]
        log(f"  membros: hub parado com {MEMBERS} membros, {base:.0f} MB")
        for member, key in keys.items():
            first_secs, first_cpu = hub.turn(key)
            wait_settled(hub.pid, 2, 30)
            after = rss_mb(hub.pid)[0]
            follow = [hub.turn(key) for _ in range(MEMBER_FOLLOW_UPS)]
            secs = sum(s for s, _ in follow) / len(follow)
            cpu = sum(c for _, c in follow) / len(follow)
            log(f"  {member}: RSS {after:.0f} MB, 1ª {first_secs:.2f} s / {first_cpu:.2f} s de CPU, seguintes {secs:.2f} s / {cpu:.2f} s")
            rows.append((member, after, first_secs, first_cpu, secs, cpu))
    return rows


def table(header, aligns, rows):
    print("| " + " | ".join(header) + " |")
    print("|" + "|".join("---:" if a == "r" else "---" for a in aligns) + "|")
    for row in rows:
        print("| " + " | ".join(row) + " |")
    print()


def fmt(value, digits=0, empty="-"):
    return empty if value is None else f"{value:.{digits}f}"


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
    models = Path(args.models_dir)
    if not binary.exists():
        sys.exit(f"não achei o binário {binary} (cargo build --release -p warden-server)")

    def wanted(name):
        return not args.only or any(token in name for token in args.only)

    rows = []
    for scenario in SCENARIOS:
        if not wanted(scenario[0]):
            continue
        log(f"cenário: {scenario[0]}")
        r = run_scenario(binary, models, scenario)
        rows.append([scenario[0], fmt(r["after"]), fmt(r["peak"]), fmt(r["seconds"], 1), scenario[5], scenario[6]])
    if wanted("descarregamento"):
        log("cenário: descarregamento por ociosidade")
        for name, rss, peak, secs, ref, target in run_unload(binary, models):
            rows.append([name, fmt(rss), fmt(peak), fmt(secs, 1), ref, target])
    if rows:
        table(
            ["Cenário", "RSS depois (MB)", "Pico (MB)", "1º turno (s)", "Sessão 222 (MB)", "Alvo do benchmark (MB)"],
            "lrrrrr",
            rows,
        )

    if wanted("membros"):
        log("cenário: membros")
        member_rows = [
            [m, fmt(rss), fmt(s1, 2), fmt(c1, 2), fmt(s2, 2), fmt(c2, 2)] for m, rss, s1, c1, s2, c2 in run_members(binary, models)
        ]
        print(f"Membros ({MEMBERS}, {MEMBER_NOTES} notas curtas cada, uma chave por membro; a Sessão 222 mediu 532, 616 e 598 MB):\n")
        table(
            ["Membro", "RSS depois do 1º (MB)", "1ª requisição (s)", "CPU da 1ª (s)", f"Média das {MEMBER_FOLLOW_UPS} seguintes (s)", "CPU média das seguintes (s)"],
            "lrrrrr",
            member_rows,
        )


if __name__ == "__main__":
    main()
