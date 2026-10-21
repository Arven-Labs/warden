#!/usr/bin/env python3
"""Measures the RSS of `warden-server` in release (P145, P146, P147): idle without and with the search models, after the 1st turn
with vaults of different sizes, after the vault model is unloaded for idleness and with several members, against
what Session 222 measured (HOSPEDAGEM.md section K). It also measures the wait of searches by different members at the same time,
since the vault model is one per process (P153). Standard library only, Linux.

Each scenario starts a new hub in a new data folder, with a fake model (scripts/fake-openai.py) in place of the
provider; the turn goes through the OpenAI-compatible API and RSS comes from /proc/<pid>/status (checked against GET /status), and so does
the peak (VmHWM). Synthetic notes in Portuguese, with a fixed seed: "short" = 2 to 8 lines of 4 to 10 words, "long" =
10 to 30 lines of 8 to 18 words (the definition of section K.1). It is not a real vault and search quality is not evaluated.
The history model is not measured: it only loads on a `search_history` the model asks for, and the fake one asks for no tool.

Usage: scripts/measure-rss.py [--bin target/release/warden-server] [--models-dir DIR] [--only TEXT ...]
Prints Markdown tables to stdout; progress goes to stderr.
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
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

HERE = Path(__file__).resolve().parent
HUB_PORT = 17430
FAKE_PORT = 17431
KEY = "measure-rss-pairing-key-0123456789abcdef"  # the hub requires 32 characters or more
CLK_TCK = os.sysconf("SC_CLK_TCK")
MEMBERS = 3
MEMBER_NOTES = 50
MEMBER_FOLLOW_UPS = 5
CONCURRENT_MEMBERS = 4  # members of the simultaneous-searches scenario (P153), each with MEMBER_NOTES notes
BIG_NOTES = 1000  # the member that indexes a big vault while the others ask
CONCURRENT_ROUNDS = 5
IDLE_SECS = 5  # --embedding-idle-secs of the unload scenario

# Portuguese on purpose: the benchmark of section K measured notes in Portuguese.
WORDS = (
    "chave rotação sexta projeto reunião cliente servidor memória nota agenda tarefa relatório código revisão "
    "entrega prazo equipe orçamento contrato fatura pagamento backup instância cofre modelo busca índice vetor "
    "conversa mensagem usuário senha acesso permissão agente skill integração fluxo etapa decisão risco custo "
    "margem crédito saldo plano assinatura suporte incidente alerta métrica painel gráfico dado tabela arquivo"
).split()

# Scenario: (name, notes, size, with warm-up, runs a turn?, what Session 222 measured in MB, and the benchmark target)
SCENARIOS = [
    ("idle, no models", 0, "short", False, False, "24.8", "~25"),
    ("idle, both models warmed up", 0, "short", True, False, "960", "~960"),
    ("1st turn, 50 short notes", 50, "short", False, True, "358", "-"),
    ("1st turn, 300 short notes", 300, "short", False, True, "1290", "-"),
    ("1st turn, 1000 short notes", 1000, "short", False, True, "1700", "~470"),
    ("1st turn, 300 long notes", 300, "long", False, True, "8200", "~1111"),
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
    """RSS and peak, in MB, from /proc/<pid>/status."""
    fields = {}
    for line in Path(f"/proc/{pid}/status").read_text().splitlines():
        if line.startswith(("VmRSS:", "VmHWM:")):
            name, value = line.split(":", 1)
            fields[name] = int(value.split()[0]) / 1024
    return fields["VmRSS"], fields["VmHWM"]


def cpu_seconds(pid):
    """CPU (user + system) the process has already spent, from /proc/<pid>/stat."""
    stat = Path(f"/proc/{pid}/stat").read_text()
    fields = stat[stat.rindex(")") + 2 :].split()  # after the "(comm)"; field 3 of stat is index 0
    return (int(fields[11]) + int(fields[12])) / CLK_TCK


def status_rss_mb(hub_url):
    body = http(f"{hub_url}/status", {"Authorization": f"Bearer {KEY}"})
    return json.loads(body)["rss_bytes"] / (1024 * 1024)


def wait_settled(pid, minimum, maximum):
    """Waits for RSS to stop moving (the warm-up loads the models in the background)."""
    start = time.time()
    last, stable = -1.0, 0
    while time.time() - start < maximum:
        time.sleep(1)
        now = rss_mb(pid)[0]
        stable = stable + 1 if abs(now - last) < 1.0 else 0
        last = now
        if stable >= 5 and time.time() - start >= minimum:
            return
    log("  warning: RSS did not settle in time")


class Hub:
    """A new hub in a new data folder, with the fake model next to it. Usage: `with Hub(binary, models) as hub:`."""

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
        # The downloaded models are shared between scenarios (and, in CI, between runs).
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
        """A subcommand of the hub itself against the data folder; returns what it printed to stdout."""
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
                    raise RuntimeError("the hub stopped before answering /health")
                time.sleep(0.5)
        raise RuntimeError("the hub did not answer /health in 60 s")

    @property
    def pid(self):
        return self.proc.pid

    def turn(self, api_key):
        """One turn through the API. Returns (wall-clock seconds, the hub's CPU seconds)."""
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
        log(f"  {name}: before {before:.0f} MB, after {after:.0f} MB, peak {peak:.0f} MB, /status {reported:.0f} MB")
        return {"after": after, "peak": peak, "seconds": seconds}


def run_unload(binary, models_dir):
    """P146 c: after the 1st turn with 1000 notes the vault model goes idle and is unloaded; a 2nd turn reloads it."""
    with Hub(binary, models_dir) as hub:
        write_notes(hub.data / "vault", 1000, "short")
        api_key = hub.cli("api-keys", "create", "measure")
        hub.start(idle_secs=IDLE_SECS)
        first, _ = hub.turn(api_key)
        wait_settled(hub.pid, 2, 30)
        loaded, loaded_peak = rss_mb(hub.pid)
        time.sleep(IDLE_SECS + 10)  # the task checks every idle/4 (at least 1 s)
        unloaded, _ = rss_mb(hub.pid)
        again, _ = hub.turn(api_key)
        wait_settled(hub.pid, 2, 30)
        reloaded, reloaded_peak = rss_mb(hub.pid)
        log(f"  unload: loaded {loaded:.0f} MB, idle {unloaded:.0f} MB, reloaded {reloaded:.0f} MB")
        return [
            ("1000 short notes, after the 1st turn", loaded, loaded_peak, first, "1700", "-"),
            (f"… {IDLE_SECS + 10} s idle (`--embedding-idle-secs {IDLE_SECS}`)", unloaded, None, None, "76 (the model only, K.2)", "~76"),
            ("… 2nd turn (reloads the model)", reloaded, reloaded_peak, again, "-", "-"),
        ]


def run_members(binary, models_dir):
    """P147: several members, each with their own key. Each one's `Vault` (and model) must live between requests."""
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
        log(f"  members: idle hub with {MEMBERS} members, {base:.0f} MB")
        for member, key in keys.items():
            first_secs, first_cpu = hub.turn(key)
            wait_settled(hub.pid, 2, 30)
            after = rss_mb(hub.pid)[0]
            follow = [hub.turn(key) for _ in range(MEMBER_FOLLOW_UPS)]
            secs = sum(s for s, _ in follow) / len(follow)
            cpu = sum(c for _, c in follow) / len(follow)
            log(f"  {member}: RSS {after:.0f} MB, 1st {first_secs:.2f} s / {first_cpu:.2f} s of CPU, following {secs:.2f} s / {cpu:.2f} s")
            rows.append((member, after, first_secs, first_cpu, secs, cpu))
    return rows


def add_members(hub, members):
    """Creates the members `(name, notes, seed)` with their notes and one key each; returns {name: key}."""
    keys = {}
    for member, notes, seed in members:
        hub.cli("users", "add", member, "--name", f"Membro {member}")
        write_notes(hub.data / "users" / member / "vault", notes, "short", seed=seed)
        keys[member] = hub.cli("api-keys", "create", f"key-{member}", "--user", member)
    return keys


def wall_secs(hub, key):
    return hub.turn(key)[0]


def average(values):
    return sum(values) / len(values) if values else None


def run_concurrency(binary, models_dir):
    """P153: the vault model is one, so searches by different members wait their turn. Measures the wait: (1) the 1st request of
    several members in sequence and at the same time, (2) requests on vaults already indexed, in sequence and at the same time, and (3) the
    requests of those members while another indexes a big vault (indexing releases the model every slice of 16 chunks)."""
    small = [(f"m{i}", MEMBER_NOTES, i) for i in range(1, CONCURRENT_MEMBERS + 1)]
    with Hub(binary, models_dir) as hub:
        keys = add_members(hub, small)
        hub.start()
        sequential_first = [wall_secs(hub, key) for key in keys.values()]
        wait_settled(hub.pid, 2, 30)
        rss_sequential = rss_mb(hub.pid)[0]
        log(f"  1st request in sequence: {sum(sequential_first):.2f} s in total, RSS {rss_sequential:.0f} MB")
    with Hub(binary, models_dir) as hub:
        keys = add_members(hub, small + [("big", BIG_NOTES, 99)])
        big_key = keys.pop("big")
        hub.start()
        with ThreadPoolExecutor(max_workers=len(keys)) as pool:
            started = time.time()
            parallel_first = list(pool.map(lambda key: wall_secs(hub, key), keys.values()))
            parallel_total = time.time() - started
            wait_settled(hub.pid, 2, 30)
            log(f"  1st request at the same time: {parallel_total:.2f} s in total, the slowest {max(parallel_first):.2f} s")
            sequential_warm = [wall_secs(hub, key) for _ in range(CONCURRENT_ROUNDS) for key in keys.values()]
            concurrent_warm = []
            for _ in range(CONCURRENT_ROUNDS):
                concurrent_warm += pool.map(lambda key: wall_secs(hub, key), keys.values())
            log(f"  already indexed: average {average(sequential_warm):.3f} s in sequence, {average(concurrent_warm):.3f} s at the same time")
        during = []
        with ThreadPoolExecutor(max_workers=1) as pool:
            big = pool.submit(wall_secs, hub, big_key)
            time.sleep(2)  # the big member's turn is already indexing
            while not big.done():
                for key in keys.values():
                    if big.done():
                        break
                    during.append(wall_secs(hub, key))
            big_secs = big.result()
        wait_settled(hub.pid, 2, 30)
        rss_concurrent = rss_mb(hub.pid)[0]
        log(f"  during the indexing of {BIG_NOTES} notes ({big_secs:.1f} s): {len(during)} requests, the slowest {max(during, default=0):.2f} s")
    return {
        "sequential_first": sequential_first,
        "parallel_first": parallel_first,
        "parallel_total": parallel_total,
        "sequential_warm": sequential_warm,
        "concurrent_warm": concurrent_warm,
        "during": during,
        "big_secs": big_secs,
        "rss": (rss_sequential, rss_concurrent),
    }


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
    parser.add_argument("--bin", default="target/release/warden-server", help="the release binary")
    parser.add_argument(
        "--models-dir",
        default=str(Path.home() / ".cache" / "warden" / "models"),
        help="where the already downloaded search models live (shared between scenarios)",
    )
    parser.add_argument("--only", nargs="*", help="run only the scenarios whose name contains any of these texts")
    parser.add_argument(
        "--json",
        metavar="FILE",
        help="also writes the RSS and the peak (MB) of each row of the main table, for scripts/compare-rss.py (P162)",
    )
    args = parser.parse_args()
    binary = Path(args.bin).resolve()
    models = Path(args.models_dir)
    if not binary.exists():
        sys.exit(f"could not find the binary {binary} (cargo build --release -p warden-server)")

    def wanted(name):
        return not args.only or any(token in name for token in args.only)

    rows = []
    results = {}  # what goes to --json: only RSS, which repeats between runs (the 1st-turn time varies about 15%)
    for scenario in SCENARIOS:
        if not wanted(scenario[0]):
            continue
        log(f"scenario: {scenario[0]}")
        r = run_scenario(binary, models, scenario)
        rows.append([scenario[0], fmt(r["after"]), fmt(r["peak"]), fmt(r["seconds"], 1), scenario[5], scenario[6]])
        results[scenario[0]] = {"rss": round(r["after"], 1), "peak": round(r["peak"], 1)}
    if wanted("unload"):
        log("scenario: unload on idleness")
        for name, rss, peak, secs, ref, target in run_unload(binary, models):
            rows.append([name, fmt(rss), fmt(peak), fmt(secs, 1), ref, target])
            results[name] = {"rss": round(rss, 1), "peak": None if peak is None else round(peak, 1)}
    if args.json:
        Path(args.json).write_text(json.dumps(results, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    if rows:
        table(
            ["Scenario", "RSS after (MB)", "Peak (MB)", "1st turn (s)", "Session 222 (MB)", "Benchmark target (MB)"],
            "lrrrrr",
            rows,
        )

    if wanted("members"):
        log("scenario: members")
        member_rows = [
            [m, fmt(rss), fmt(s1, 2), fmt(c1, 2), fmt(s2, 2), fmt(c2, 2)] for m, rss, s1, c1, s2, c2 in run_members(binary, models)
        ]
        print(f"Members ({MEMBERS}, {MEMBER_NOTES} short notes each, one key per member; Session 222 measured 532, 616 and 598 MB):\n")
        table(
            ["Member", "RSS after the 1st (MB)", "1st request (s)", "CPU of the 1st (s)", f"Average of the next {MEMBER_FOLLOW_UPS} (s)", "Average CPU of the next ones (s)"],
            "lrrrrr",
            member_rows,
        )

    if wanted("concurrent"):
        log("scenario: simultaneous searches")
        r = run_concurrency(binary, models)
        warm = r["sequential_warm"]
        print(
            f"Simultaneous searches ({CONCURRENT_MEMBERS} members with {MEMBER_NOTES} short notes and one with {BIG_NOTES}; "
            "seconds per request, on a runner that may have other cores than your machine):\n"
        )
        table(
            ["Measure", "Sequential (s)", "Simultaneous (s)"],
            "lrr",
            [
                [f"1st request of the {CONCURRENT_MEMBERS} members, total time", fmt(sum(r["sequential_first"]), 2), fmt(r["parallel_total"], 2)],
                ["1st request, the slowest", fmt(max(r["sequential_first"]), 2), fmt(max(r["parallel_first"]), 2)],
                ["vault already indexed, average per request", fmt(average(warm), 3), fmt(average(r["concurrent_warm"]), 3)],
                ["vault already indexed, the slowest", fmt(max(warm), 3), fmt(max(r["concurrent_warm"]), 3)],
                [f"during the indexing of another member's {BIG_NOTES} notes, average", fmt(average(warm), 3), fmt(average(r["during"]), 3)],
                [f"during the indexing of another member's {BIG_NOTES} notes, the slowest", fmt(max(warm), 3), fmt(max(r["during"], default=None), 3)],
                [f"turn of the member with {BIG_NOTES} notes ({len(r['during'])} requests from the others in between)", "-", fmt(r["big_secs"], 1)],
                ["RSS at the end (MB; the simultaneous one includes the big member)", fmt(r["rss"][0]), fmt(r["rss"][1])],
            ],
        )


if __name__ == "__main__":
    main()
