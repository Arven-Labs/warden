//! How much memory the history model gives back when it is unloaded (P156). `#[ignore]`d like `history_embed.rs`: the
//! first run downloads the ONNX model (~120 MB). Its own test binary because it reads the RSS of the whole process, so
//! no other test may run beside it. Run with
//!   cargo test -p warden-core --test history_embed_memory -- --ignored --nocapture
//! and, to see whether fewer glibc arenas help, again with `MALLOC_ARENA_MAX=1` in the environment.
//!
//! Before `malloc_trim` was added to the unload, dropping the model gave back ~190 MB of ~754 MB and ~560 MB stayed (`HOSPEDAGEM.md` K.2):
//! either the ONNX runtime holds it or the heap is fragmented. `malloc_trim` gives back free heap pages, so what it
//! returns is the heap's share.

use std::time::Duration;

use warden_core::memory::embed::{cosine_similarity, embed, unload_idle_history_model, Kind};

fn rss_mb() -> f64 {
    let status = std::fs::read_to_string("/proc/self/status").expect("Linux only");
    let line = status.lines().find(|l| l.starts_with("VmRSS:")).expect("VmRSS in /proc/self/status");
    line.split_whitespace().nth(1).and_then(|kb| kb.parse::<f64>().ok()).expect("VmRSS in kB") / 1024.0
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn trim_heap() {
    extern "C" {
        fn malloc_trim(pad: usize) -> i32;
    }
    // SAFETY: `malloc_trim` takes no pointers and only returns free heap pages to the system.
    unsafe {
        malloc_trim(0);
    }
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
fn trim_heap() {}

/// 400 long messages, the load the K.2 numbers were measured with: 10 to 30 lines of 8 to 18 words.
fn long_messages() -> Vec<String> {
    const WORDS: &[&str] = &[
        "chave", "rotação", "sexta", "projeto", "reunião", "cliente", "servidor", "memória", "nota", "agenda", "tarefa", "relatório", "código", "revisão",
        "entrega", "prazo", "equipe", "orçamento", "contrato", "fatura", "pagamento", "backup", "instância", "cofre", "modelo", "busca", "índice",
    ];
    (0..400)
        .map(|i| {
            let lines = 10 + i % 21;
            (0..lines)
                .map(|l| (0..8 + (i + l) % 11).map(|w| WORDS[(i * 7 + l * 3 + w * 5) % WORDS.len()]).collect::<Vec<_>>().join(" "))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .collect()
}

#[test]
#[ignore = "downloads a real ONNX model from Hugging Face on first run — needs network"]
fn unloading_the_history_model_gives_its_memory_back() {
    let arena_max = std::env::var("MALLOC_ARENA_MAX").unwrap_or_else(|_| "(unset)".into());
    let before_load = rss_mb();
    let probe = embed(Kind::Query, vec!["como configurei o e-mail do servidor?".to_string()]).unwrap().remove(0);
    let loaded = rss_mb();
    embed(Kind::Passage, long_messages()).unwrap();
    let after_work = rss_mb();

    assert!(unload_idle_history_model(Duration::ZERO), "nothing was unloaded");
    // The unload hands the heap back itself; this is what it left behind, and then what an explicit trim could still take.
    let unloaded = rss_mb();
    trim_heap();
    let trimmed = rss_mb();

    let again = embed(Kind::Query, vec!["como configurei o e-mail do servidor?".to_string()]).unwrap().remove(0);
    let reloaded = rss_mb();
    println!(
        "\nMALLOC_ARENA_MAX={arena_max}\n  before load {before_load:.0} MB\n  model loaded, 1 text {loaded:.0} MB\n  after 400 long messages {after_work:.0} MB\n  \
         unloaded (heap handed back by the unload) {unloaded:.0} MB\n  + another malloc_trim(0) {trimmed:.0} MB\n  loaded again, 1 text {reloaded:.0} MB"
    );
    assert!(unloaded < loaded / 2.0, "the unload left {unloaded:.0} MB of {loaded:.0} MB in the process");
    assert!(cosine_similarity(&probe, &again) > 0.9999, "the model gave other vectors after being loaded again");
}
