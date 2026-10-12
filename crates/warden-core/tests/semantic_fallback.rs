//! P56: when searching by meaning can't run (here the off switch, the same error a missing model or no network gives), a turn still
//! gets the vault's context from the plain word search instead of failing or going without. Its own test binary, because the switch
//! is an environment variable of the whole process.

use std::sync::Arc;

use async_trait::async_trait;

use warden_core::memory::Vault;
use warden_core::model::{ChatStream, Message, ModelProvider, Response, ToolCall, response_stream};
use warden_core::orchestrator::Orchestrator;
use warden_core::tool::ToolSpec;

/// Answers with every message it was handed, so a test can read what the turn put in front of the model.
struct EchoesMessages;

#[async_trait]
impl ModelProvider for EchoesMessages {
    async fn chat_stream(&self, messages: Vec<Message>, _tools: Vec<ToolSpec>) -> anyhow::Result<ChatStream> {
        let seen = messages.iter().map(|m| m.content.clone()).collect::<Vec<_>>().join("\n");
        Ok(response_stream(Response { content: seen, tool_calls: Vec::<ToolCall>::new(), usage: None }))
    }
}

fn vault_with_a_note() -> Arc<Vault> {
    let dir = std::env::temp_dir().join(format!("warden-semantic-fallback-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    let vault = Arc::new(Vault::new(dir));
    vault.write("notes/dentist.md", "Dentist appointment on Friday at 3pm").unwrap();
    vault
}

#[tokio::test]
async fn a_turn_without_search_by_meaning_still_gets_the_words_that_match() {
    std::env::set_var(warden_core::memory::embed::OFF_SWITCH, "1");
    let vault = vault_with_a_note();
    assert!(vault.search_semantic("dentist", 8).is_err(), "the semantic search is refused, so the fallback is what runs");
    assert_eq!(vault.search("dentist", 8).unwrap().len(), 1);

    let orchestrator = Orchestrator::new(Arc::new(EchoesMessages), vault);
    let seen = orchestrator.handle_turn(&[], "when is the dentist?", Vec::new(), None).await.unwrap().content;
    assert!(seen.contains("Relevant context found") && seen.contains("notes/dentist.md"), "the word search filled in: {seen}");
}
