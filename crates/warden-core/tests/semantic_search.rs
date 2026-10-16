//! End-to-end coverage for `Vault::search_semantic` (Fase 4.5). The deterministic pieces
//! (chunking/hashing/cosine math) are unit-tested in `memory/semantic.rs` without a real model.
//! These tests exercise the real ONNX model via `fastembed`, which downloads it from Hugging Face
//! on first use — `#[ignore]`d by default so `cargo test`/CI stay hermetic and fast regardless of
//! network access, same posture already accepted for other real-external-service checks in this
//! project (see PENDING.md P29/P30/P31/P38). Run explicitly with:
//!   cargo test -p warden-core --test semantic_search -- --ignored

use std::sync::{Mutex, MutexGuard};
use warden_core::memory::Vault;

/// The vault model is one for the whole process (P153), so tests that load and unload it can't overlap.
fn one_at_a_time() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn temp_vault() -> Vault {
    let dir = std::env::temp_dir().join(format!(
        "warden-semantic-e2e-{}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    Vault::new(dir)
}

#[test]
#[ignore = "downloads a real ONNX model from Hugging Face on first run — needs network"]
fn search_semantic_ranks_by_meaning_not_substring() {
    let _turn = one_at_a_time();
    let vault = temp_vault();
    vault.write("dentist.md", "Appointment with the dentist on Friday morning.").unwrap();
    vault.write("groceries.md", "Buy eggs, bread and milk at the store.").unwrap();

    // No literal word overlap with either note, but "medical checkup" is semantically closest to
    // the dentist appointment — a substring grep would find nothing here at all.
    let hits = vault.search_semantic("do I have a medical checkup coming up?", 1).unwrap();

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "dentist.md");
}

#[test]
#[ignore = "downloads a real ONNX model from Hugging Face on first run — needs network"]
fn search_semantic_is_incremental_across_calls() {
    let _turn = one_at_a_time();
    let vault = temp_vault();
    vault.write("a.md", "The quick brown fox jumps over the lazy dog.").unwrap();

    let first = vault.search_semantic("fox", 5).unwrap();
    assert_eq!(first.len(), 1);

    // A second call with no vault changes should reuse the persisted index rather than fail or
    // duplicate entries.
    let second = vault.search_semantic("fox", 5).unwrap();
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].path, first[0].path);

    // Editing the file (as if from outside the app entirely, e.g. Obsidian) must be picked up.
    vault.write("a.md", "Completely different content about spreadsheets and budgets.").unwrap();
    let after_edit = vault.search_semantic("budgets", 5).unwrap();
    assert_eq!(after_edit.len(), 1);
    assert!(after_edit[0].line.contains("budgets"));
}

/// P145: indexing runs the model 16 chunks at a time, so 40 notes take three batches; the one that answers must still
/// come out on top, wherever it fell.
#[test]
#[ignore = "downloads a real ONNX model from Hugging Face on first run — needs network"]
fn search_semantic_indexes_a_vault_bigger_than_one_batch() {
    let _turn = one_at_a_time();
    let vault = temp_vault();
    for i in 0..40 {
        if i == 27 {
            vault.write(&format!("note-{i:02}.md"), "Appointment with the dentist on Friday morning.").unwrap();
        } else {
            vault.write(&format!("note-{i:02}.md"), &format!("Grocery list number {i}: eggs, bread and milk.")).unwrap();
        }
    }

    let hits = vault.search_semantic("do I have a medical checkup coming up?", 1).unwrap();

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "note-27.md");
}

/// P146 (c): a model nobody has used for a while is let go, and the next search loads it again and still answers.
#[test]
#[ignore = "downloads a real ONNX model from Hugging Face on first run — needs network"]
fn an_idle_embedder_is_unloaded_and_the_next_search_loads_it_again() {
    let _turn = one_at_a_time();
    let vault = temp_vault();
    vault.write("dentist.md", "Appointment with the dentist on Friday morning.").unwrap();
    vault.write("groceries.md", "Buy eggs, bread and milk at the store.").unwrap();
    vault.warm_up().unwrap();

    assert!(!vault.unload_idle_embedder(std::time::Duration::from_secs(3600)), "used just now, so it stays");
    assert!(vault.unload_idle_embedder(std::time::Duration::ZERO), "idle long enough, so it goes");
    assert!(!vault.unload_idle_embedder(std::time::Duration::ZERO), "and there is nothing left to unload");

    let hits = vault.search_semantic("do I have a medical checkup coming up?", 1).unwrap();
    assert_eq!(hits.first().map(|h| h.path.as_str()), Some("dentist.md"), "loaded again and found the right note");
    assert!(vault.unload_idle_embedder(std::time::Duration::ZERO), "the search loaded it again");
}

/// P153: every vault of the process uses the same model, so a second vault doesn't load another copy, still ranks right,
/// and a model one vault loaded is the one another vault unloads.
#[test]
#[ignore = "downloads a real ONNX model from Hugging Face on first run — needs network"]
fn two_vaults_share_one_model() {
    let _turn = one_at_a_time();
    let (first, second) = (temp_vault(), temp_vault());
    first.write("dentist.md", "Appointment with the dentist on Friday morning.").unwrap();
    first.write("groceries.md", "Buy eggs, bread and milk at the store.").unwrap();
    second.write("invoice.md", "Pay the electricity invoice before the end of the month.").unwrap();
    second.write("garden.md", "Water the tomato plants every morning.").unwrap();

    let hits = first.search_semantic("do I have a medical checkup coming up?", 1).unwrap();
    assert_eq!(hits.first().map(|h| h.path.as_str()), Some("dentist.md"));
    let hits = second.search_semantic("what bill do I owe?", 1).unwrap();
    assert_eq!(hits.first().map(|h| h.path.as_str()), Some("invoice.md"), "the second vault answered with the same model");

    assert!(second.unload_idle_embedder(std::time::Duration::ZERO), "the model the first vault loaded is the one the second unloads");
    assert!(!first.unload_idle_embedder(std::time::Duration::ZERO), "and it was the only one");
}
