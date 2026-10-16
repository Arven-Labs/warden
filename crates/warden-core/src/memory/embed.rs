//! A shared, multilingual text embedder (P115 c) — what `search_history` uses to find an earlier message by what it
//! means, not by its exact words. Separate from the vault's own embedder on purpose: the vault's model
//! (`AllMiniLML6V2`) is trained almost only on English, and the conversations this one reads are often in
//! Portuguese. One model per process, loaded on first use (the first-ever use downloads it, ~120 MB, once per machine).
//!
//! The model is a quantized `paraphrase-multilingual-MiniLM-L12-v2`, made to say how alike two sentences are: related
//! text scores 0.3-0.8 and unrelated text around 0 (measured in `tests/history_embed.rs`). `multilingual-e5-small` was
//! tried first and rejected: it scores everything 0.75-0.9, so nothing separates a match from noise.
//!
//! Behind the `semantic-search` feature like the vault's search; without it `embed` fails with a clear message and
//! callers fall back to matching words.

/// Identifies which model made an embedding, so a stored index made by another model is thrown away, not mixed in.
pub const HISTORY_MODEL_ID: &str = "ParaphraseMLMiniLML12V2Q";

/// Set (to anything) in the environment to keep the model from loading or downloading: callers fall back to words.
/// For tests and for a machine that is offline or short on memory.
pub const OFF_SWITCH: &str = "WARDEN_NO_SEMANTIC";

/// Which side of a search a text is. This model compares sentences symmetrically, so both are embedded the same way;
/// kept so a model that does tell them apart (the E5 family) can replace it without touching the callers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Query,
    Passage,
}

/// Embeds each of `texts`, in order. Slow the first time in a process (loads the model).
#[cfg(feature = "semantic-search")]
pub fn embed(_kind: Kind, texts: Vec<String>) -> anyhow::Result<Vec<Vec<f32>>> {
    use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
    use std::sync::Mutex;

    static MODEL: Mutex<Option<TextEmbedding>> = Mutex::new(None);

    if std::env::var_os(OFF_SWITCH).is_some_and(|v| !v.is_empty()) {
        anyhow::bail!("search by meaning is turned off ({OFF_SWITCH} is set)");
    }
    if texts.is_empty() {
        return Ok(Vec::new());
    }
    let mut guard = MODEL.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if guard.is_none() {
        // P146: the ORT arena off, since this model is never unloaded; with the slices below, 400 long messages
        // took 814 MB instead of 1237 MB with it on.
        let cpu = ort::ep::CPU::default().with_arena_allocator(false).build();
        *guard = Some(TextEmbedding::try_new(
            TextInitOptions::new(EmbeddingModel::ParaphraseMLMiniLML12V2Q)
                .with_cache_dir(super::model_cache_dir())
                .with_show_download_progress(false)
                .with_execution_providers(vec![cpu]),
        )?);
    }
    // P146: fastembed runs a quantized model in one single batch whatever `batch_size` says, and 400 long messages
    // reached 4.5 GB that way. Slices of 16 give the same vectors (cosine 1.00000).
    let model = guard.as_mut().expect("loaded just above");
    let mut embeddings = Vec::with_capacity(texts.len());
    for slice in texts.chunks(HISTORY_EMBED_SLICE) {
        embeddings.extend(model.embed(slice, None)?);
    }
    Ok(embeddings)
}

/// How many texts go through the history model at once (P146, measured in `HOSPEDAGEM.md` K.2).
#[cfg(feature = "semantic-search")]
const HISTORY_EMBED_SLICE: usize = 16;

/// The vault's search model (`AllMiniLML6V2`) and when it was last used, under one lock so that using it and unloading it can't race.
#[cfg(feature = "semantic-search")]
struct LoadedVaultModel {
    model: fastembed::TextEmbedding,
    last_used: std::time::Instant,
}

/// P153: one vault model for the whole process, not one per `Vault`: a hub keeps a `Vault` for each active member, and a copy
/// of the ~135 MB model in each cost ~175 MB of RSS per member. Searches of different vaults take turns on it.
#[cfg(feature = "semantic-search")]
static VAULT_MODEL: std::sync::Mutex<Option<LoadedVaultModel>> = std::sync::Mutex::new(None);

/// Lazily loads the vault's embedding model (downloads it on first-ever use — the one part of "local" search that needs
/// network, exactly once per machine) and runs `f` against it. Callers with a lot to embed call this once per slice, so
/// the model is free for someone else's search in between.
#[cfg(feature = "semantic-search")]
pub(super) fn with_vault_model<T>(f: impl FnOnce(&mut fastembed::TextEmbedding) -> anyhow::Result<T>) -> anyhow::Result<T> {
    use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};

    if std::env::var_os(OFF_SWITCH).is_some_and(|v| !v.is_empty()) {
        anyhow::bail!("search by meaning is turned off ({OFF_SWITCH} is set)");
    }
    let mut guard = VAULT_MODEL.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if guard.is_none() {
        let model = TextEmbedding::try_new(
            TextInitOptions::new(EmbeddingModel::AllMiniLML6V2)
                .with_cache_dir(super::model_cache_dir())
                .with_show_download_progress(false),
        )?;
        *guard = Some(LoadedVaultModel { model, last_used: std::time::Instant::now() });
    }
    let loaded = guard.as_mut().expect("loaded just above");
    let result = f(&mut loaded.model);
    loaded.last_used = std::time::Instant::now();
    result
}

/// Lets the vault model go if nothing has used it for `idle` (P146 c): an idle hub then weighs ~76 MB instead of ~475 MB,
/// and the next search loads it again. Returns whether it unloaded it. Waits for a search in progress rather than taking
/// the model from under it.
#[cfg(feature = "semantic-search")]
pub fn unload_idle_vault_model(idle: std::time::Duration) -> bool {
    let mut guard = VAULT_MODEL.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if guard.as_ref().is_some_and(|loaded| loaded.last_used.elapsed() >= idle) {
        *guard = None;
        return true;
    }
    false
}

#[cfg(not(feature = "semantic-search"))]
pub fn unload_idle_vault_model(_idle: std::time::Duration) -> bool {
    false
}

/// Whether the vault model is in memory right now.
#[cfg(all(feature = "semantic-search", test))]
pub(super) fn vault_model_loaded() -> bool {
    VAULT_MODEL.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).is_some()
}

/// Loads the model now (the first `embed` of a process does it), so the first search of the history doesn't wait. An error
/// (offline with no model yet, or the off switch) is for the caller to log; the first real `embed` will try again.
pub fn warm_up() -> anyhow::Result<()> {
    embed(Kind::Query, vec!["warm up".to_string()]).map(|_| ())
}

#[cfg(not(feature = "semantic-search"))]
pub fn embed(_kind: Kind, _texts: Vec<String>) -> anyhow::Result<Vec<Vec<f32>>> {
    anyhow::bail!("semantic search isn't built into this binary")
}

/// How alike two embeddings are, from -1 to 1 (1 = the same direction).
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let (norm_a, norm_b) = (a.iter().map(|x| x * x).sum::<f32>().sqrt(), b.iter().map(|x| x * x).sum::<f32>().sqrt());
    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot / (norm_a * norm_b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_similarity_is_one_for_the_same_direction_and_zero_for_unrelated_or_mismatched() {
        assert!((cosine_similarity(&[1.0, 0.0], &[2.0, 0.0]) - 1.0).abs() < 1e-6);
        assert_eq!(cosine_similarity(&[1.0, 0.0], &[0.0, 1.0]), 0.0);
        assert_eq!(cosine_similarity(&[1.0], &[1.0, 0.0]), 0.0);
        assert_eq!(cosine_similarity(&[], &[]), 0.0);
        assert_eq!(cosine_similarity(&[0.0, 0.0], &[1.0, 1.0]), 0.0);
    }
}
