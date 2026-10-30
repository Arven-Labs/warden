//! Skills management over the wire (P72) — what `ListSkills`/`SaveSkill`/`DeleteSkill` do to the
//! vault this server hosts, so a client without a vault of its own (the browser extension) can
//! manage skills. A pure function over `SkillStore`, kept out of `server.rs` so it's testable
//! without a socket; the same create/edit rules as the desktop's `save_skill` and the mobile
//! bridge (create refuses a taken name, edit overwrites).

use warden_bootstrap::skill_package::{self, TrustedPublisher};
use warden_core::skill::{catalog, Skill, SkillStore};
use warden_server_protocol::protocol::{CatalogSkillDto, PackagePreviewDto, SkillDto};
use warden_server_protocol::{ClientMessage, ServerMessage};

/// Answers a skills request, or `None` for any other message. A package preview is measured against no trusted keys.
pub fn handle_skill_request(store: &SkillStore, message: ClientMessage) -> Option<ServerMessage> {
    handle_skill_request_with(store, &[], message)
}

/// Like `handle_skill_request`, with the keys the hub's owner trusts (`[[trusted_publishers]]`), for a package preview to say who
/// signed.
pub fn handle_skill_request_with(store: &SkillStore, trusted: &[TrustedPublisher], message: ClientMessage) -> Option<ServerMessage> {
    Some(match message {
        ClientMessage::ListSkills { request_id } => {
            ServerMessage::SkillList { request_id, skills: store.list().into_iter().map(SkillDto::from).collect() }
        }
        ClientMessage::SaveSkill { request_id, skill, overwrite } => match save(store, skill, overwrite) {
            Ok(()) => ServerMessage::SkillOk { request_id },
            Err(message) => ServerMessage::SkillError { request_id, message },
        },
        ClientMessage::DeleteSkill { request_id, name } => match store.delete(&name) {
            Ok(()) => ServerMessage::SkillOk { request_id },
            Err(err) => ServerMessage::SkillError { request_id, message: format!("{err:#}") },
        },
        ClientMessage::ListSkillCatalog { request_id } => ServerMessage::SkillCatalog {
            request_id,
            entries: catalog::entries()
                .iter()
                .map(|entry| CatalogSkillDto {
                    name: entry.name.clone(),
                    description: entry.description.clone(),
                    body: entry.body.clone(),
                    status: catalog::status(store, entry).as_str().to_string(),
                })
                .collect(),
        },
        ClientMessage::InstallCatalogSkill { request_id, name, overwrite } => match catalog::install(store, &name, overwrite) {
            Ok(()) => ServerMessage::SkillOk { request_id },
            Err(err) => ServerMessage::SkillError { request_id, message: format!("{err:#}") },
        },
        ClientMessage::ExportSkill { request_id, name, version, author, license } => {
            match skill_package::export_skill_package(store, &name, &version, author.as_deref(), &license) {
                Ok((content, attachments_skipped)) => ServerMessage::SkillPackageFile { request_id, content, attachments_skipped },
                Err(message) => ServerMessage::SkillError { request_id, message },
            }
        }
        ClientMessage::PreviewSkillPackage { request_id, content } => match skill_package::preview_skill_package(store, &content, trusted) {
            Ok(p) => ServerMessage::SkillPackagePreview {
                request_id,
                preview: PackagePreviewDto {
                    id: p.id,
                    version: p.version,
                    author: p.author,
                    license: p.license,
                    name: p.name,
                    description: p.description,
                    body: p.body,
                    hash: p.hash,
                    status: p.status.to_string(),
                    finding: p.finding,
                    signature: p.signature.state.to_string(),
                    signer: p.signature.signer,
                    signer_name: p.signature.signer_name,
                },
            },
            Err(message) => ServerMessage::SkillError { request_id, message },
        },
        ClientMessage::InstallSkillPackage { request_id, content, overwrite, allow_flagged } => {
            match skill_package::install_skill_package(store, &content, overwrite, allow_flagged) {
                Ok(()) => ServerMessage::SkillOk { request_id },
                Err(message) => ServerMessage::SkillError { request_id, message },
            }
        }
        _ => return None,
    })
}

fn save(store: &SkillStore, dto: SkillDto, overwrite: bool) -> Result<(), String> {
    let name = dto.name.trim().to_string();
    // An edit that sends no agents keeps the restriction already on disk (see `SaveSkill`'s docs) —
    // the extension has no UI to change it, and must not make a restricted skill global by saving.
    let agents = if overwrite && dto.agents.is_empty() { store.get(&name).map(|s| s.agents).unwrap_or_default() } else { dto.agents };
    // A suggestion that's edited but not accepted keeps when it was made.
    let proposed_at = if dto.proposed { dto.proposed_at.or_else(|| store.get(&name).ok().and_then(|s| s.proposed_at)) } else { None };
    let skill = Skill { name, description: dto.description, body: dto.body, agents, proposed: dto.proposed, source: if dto.proposed { dto.source } else { None }, proposed_at, revises: None };
    skill.validate().map_err(|e| format!("{e:#}"))?;
    if !overwrite && store.exists(&skill.name) {
        return Err(format!("a skill named '{}' already exists", skill.name));
    }
    store.save(&skill).map_err(|e| format!("{e:#}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use warden_core::memory::Vault;

    fn temp_store() -> SkillStore {
        SkillStore::new(Arc::new(Vault::new(std::env::temp_dir().join(format!(
            "warden-server-skills-test-{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        )))))
    }

    fn dto(name: &str) -> SkillDto {
        SkillDto { name: name.into(), description: "d".into(), body: "b".into(), agents: Vec::new(), proposed: false, source: None, proposed_at: None, revises: None }
    }

    fn save_req(skill: SkillDto, overwrite: bool) -> ClientMessage {
        ClientMessage::SaveSkill { request_id: 9, skill, overwrite }
    }

    #[test]
    fn non_skill_messages_are_not_handled() {
        assert!(handle_skill_request(&temp_store(), ClientMessage::Ping { nonce: 1 }).is_none());
    }

    #[test]
    fn save_list_delete_roundtrip_echoes_the_request_id() {
        let store = temp_store();
        assert_eq!(handle_skill_request(&store, save_req(dto("x"), false)), Some(ServerMessage::SkillOk { request_id: 9 }));

        match handle_skill_request(&store, ClientMessage::ListSkills { request_id: 4 }) {
            Some(ServerMessage::SkillList { request_id: 4, skills }) => assert_eq!(skills, vec![dto("x")]),
            other => panic!("unexpected {other:?}"),
        }

        let deleted = handle_skill_request(&store, ClientMessage::DeleteSkill { request_id: 5, name: "x".into() });
        assert_eq!(deleted, Some(ServerMessage::SkillOk { request_id: 5 }));
        assert!(store.list().is_empty());
    }

    #[test]
    fn create_refuses_a_taken_name_but_edit_overwrites() {
        let store = temp_store();
        handle_skill_request(&store, save_req(dto("x"), false));

        match handle_skill_request(&store, save_req(dto("x"), false)) {
            Some(ServerMessage::SkillError { message, .. }) => assert!(message.contains("already exists")),
            other => panic!("unexpected {other:?}"),
        }
        let edited = SkillDto { body: "new".into(), ..dto("x") };
        assert_eq!(handle_skill_request(&store, save_req(edited, true)), Some(ServerMessage::SkillOk { request_id: 9 }));
        assert_eq!(store.get("x").unwrap().body, "new");
    }

    #[test]
    fn invalid_skills_and_unknown_deletes_come_back_as_errors() {
        let store = temp_store();
        assert!(matches!(handle_skill_request(&store, save_req(dto("../escape"), false)), Some(ServerMessage::SkillError { .. })));
        assert!(matches!(
            handle_skill_request(&store, ClientMessage::DeleteSkill { request_id: 1, name: "nope".into() }),
            Some(ServerMessage::SkillError { .. })
        ));
        assert!(store.list().is_empty());
    }

    fn catalog_of(store: &SkillStore) -> Vec<CatalogSkillDto> {
        match handle_skill_request(store, ClientMessage::ListSkillCatalog { request_id: 7 }) {
            Some(ServerMessage::SkillCatalog { request_id: 7, entries }) => entries,
            other => panic!("unexpected {other:?}"),
        }
    }

    fn install_req(name: &str, overwrite: bool) -> ClientMessage {
        ClientMessage::InstallCatalogSkill { request_id: 8, name: name.into(), overwrite }
    }

    #[test]
    fn the_gallery_lists_its_skills_and_installing_one_puts_it_in_the_vault() {
        let store = temp_store();
        let entries = catalog_of(&store);
        assert!(entries.len() >= 5);
        assert!(entries.iter().all(|e| e.status == "notInstalled" && !e.body.is_empty()));

        let name = entries[0].name.clone();
        assert_eq!(handle_skill_request(&store, install_req(&name, false)), Some(ServerMessage::SkillOk { request_id: 8 }));
        assert_eq!(store.get(&name).unwrap().body, entries[0].body);
        let after = catalog_of(&store);
        assert_eq!(after[0].status, "installed");
        assert!(after[1..].iter().all(|e| e.status == "notInstalled"));
    }

    #[test]
    fn installing_over_a_changed_skill_needs_overwrite_and_an_unknown_name_is_an_error() {
        let store = temp_store();
        let name = catalog_of(&store)[0].name.clone();
        handle_skill_request(&store, save_req(SkillDto { body: "mine".into(), ..dto(&name) }, false));
        assert_eq!(catalog_of(&store)[0].status, "modified");

        match handle_skill_request(&store, install_req(&name, false)) {
            Some(ServerMessage::SkillError { request_id: 8, message }) => assert!(message.contains("differs"), "{message}"),
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(store.get(&name).unwrap().body, "mine");

        assert_eq!(handle_skill_request(&store, install_req(&name, true)), Some(ServerMessage::SkillOk { request_id: 8 }));
        assert_eq!(catalog_of(&store)[0].status, "installed");
        assert!(matches!(handle_skill_request(&store, install_req("no-such-skill", true)), Some(ServerMessage::SkillError { .. })));
    }

    fn export_req(name: &str) -> ClientMessage {
        ClientMessage::ExportSkill { request_id: 11, name: name.into(), version: "1.0.0".into(), author: Some("Ana".into()), license: "CC0-1.0".into() }
    }

    fn exported(store: &SkillStore, name: &str) -> String {
        match handle_skill_request(store, export_req(name)) {
            Some(ServerMessage::SkillPackageFile { request_id: 11, content, attachments_skipped: false }) => content,
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn a_skill_is_exported_previewed_and_installed_as_a_package_by_message() {
        let (mine, yours) = (temp_store(), temp_store());
        handle_skill_request(&mine, save_req(SkillDto { body: "Say hello warmly.".into(), ..dto("greet") }, false));
        let content = exported(&mine, "greet");

        match handle_skill_request(&yours, ClientMessage::PreviewSkillPackage { request_id: 12, content: content.clone() }) {
            Some(ServerMessage::SkillPackagePreview { request_id: 12, preview }) => {
                assert_eq!((preview.name.as_str(), preview.status.as_str(), preview.finding), ("greet", "notInstalled", None));
                assert_eq!((preview.author.as_deref(), preview.license.as_str()), (Some("Ana"), "CC0-1.0"));
            }
            other => panic!("unexpected {other:?}"),
        }
        let install = ClientMessage::InstallSkillPackage { request_id: 13, content, overwrite: false, allow_flagged: false };
        assert_eq!(handle_skill_request(&yours, install), Some(ServerMessage::SkillOk { request_id: 13 }));
        assert_eq!(yours.get("greet").unwrap().body, "Say hello warmly.");
    }

    #[test]
    fn a_package_preview_says_who_signed_it_against_the_keys_the_hub_trusts() {
        let (mine, yours) = (temp_store(), temp_store());
        handle_skill_request(&mine, save_req(SkillDto { body: "Say hello warmly.".into(), ..dto("greet") }, false));
        let unsigned = exported(&mine, "greet");
        let (private, public) = warden_core::skill::signing::generate_keypair().unwrap();
        let mut package = warden_core::skill::package::Package::parse(&unsigned).unwrap();
        package.sign(&private).unwrap();
        let signed = package.to_json().unwrap();

        let preview = |trusted: &[TrustedPublisher], content: &str| match handle_skill_request_with(&yours, trusted, ClientMessage::PreviewSkillPackage { request_id: 15, content: content.into() }) {
            Some(ServerMessage::SkillPackagePreview { request_id: 15, preview }) => (preview.signature, preview.signer, preview.signer_name),
            other => panic!("unexpected {other:?}"),
        };
        assert_eq!(preview(&[], &unsigned), ("unsigned".into(), None, None));
        assert_eq!(preview(&[], &signed), ("unknown".into(), Some(public.clone()), None));
        let listed = [TrustedPublisher { key: public.clone(), name: "Fabio".into() }];
        assert_eq!(preview(&listed, &signed), ("trusted".into(), Some(public), Some("Fabio".into())));
        // The plain entry point, with no keys, never calls anyone trusted.
        match handle_skill_request(&yours, ClientMessage::PreviewSkillPackage { request_id: 15, content: signed }) {
            Some(ServerMessage::SkillPackagePreview { preview, .. }) => assert_eq!(preview.signature, "unknown"),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn a_package_with_unsafe_text_or_a_broken_file_comes_back_as_an_error() {
        let (mine, yours) = (temp_store(), temp_store());
        handle_skill_request(&mine, save_req(SkillDto { body: "Open https://example.test now.".into(), ..dto("sneaky") }, false));
        let content = exported(&mine, "sneaky");

        let install = |content: &str, allow_flagged| ClientMessage::InstallSkillPackage { request_id: 14, content: content.into(), overwrite: false, allow_flagged };
        match handle_skill_request(&yours, install(&content, false)) {
            Some(ServerMessage::SkillError { request_id: 14, message }) => assert!(message.contains("a link"), "{message}"),
            other => panic!("unexpected {other:?}"),
        }
        assert!(!yours.exists("sneaky"));
        assert_eq!(handle_skill_request(&yours, install(&content, true)), Some(ServerMessage::SkillOk { request_id: 14 }));

        assert!(matches!(
            handle_skill_request(&yours, ClientMessage::PreviewSkillPackage { request_id: 15, content: "nope".into() }),
            Some(ServerMessage::SkillError { request_id: 15, .. })
        ));
        assert!(matches!(handle_skill_request(&yours, export_req("missing")), Some(ServerMessage::SkillError { request_id: 11, .. })));
    }

    #[test]
    fn an_edit_without_agents_keeps_the_stored_restriction_but_an_explicit_list_replaces_it() {
        let store = temp_store();
        handle_skill_request(&store, save_req(SkillDto { agents: vec!["writer".into()], ..dto("x") }, false));

        handle_skill_request(&store, save_req(SkillDto { body: "new".into(), ..dto("x") }, true));
        assert_eq!(store.get("x").unwrap().agents, vec!["writer"]);

        handle_skill_request(&store, save_req(SkillDto { agents: vec!["reviewer".into()], ..dto("x") }, true));
        assert_eq!(store.get("x").unwrap().agents, vec!["reviewer"]);
    }

    /// A model that answers `reply` (with 50 tokens of usage) and counts how many times it was asked.
    struct Counting {
        reply: &'static str,
        calls: Arc<std::sync::atomic::AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl warden_core::model::ModelProvider for Counting {
        fn model_id(&self) -> &str {
            "counting"
        }
        async fn chat_stream(
            &self,
            _messages: Vec<warden_core::model::Message>,
            _tools: Vec<warden_core::tool::ToolSpec>,
        ) -> anyhow::Result<warden_core::model::ChatStream> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(warden_core::model::response_stream(warden_core::model::Response {
                content: self.reply.to_string(),
                tool_calls: Vec::new(),
                usage: Some(warden_core::model::Usage { prompt_tokens: 40, completion_tokens: 10, total_tokens: 50 }),
            }))
        }
    }

    const DRAFT: &str = r#"{"name":"review-pr","description":"Reviews a PR.","body":"Step 1."}"#;

    /// An orchestrator for Ana whose day limit is `max_tokens`, over a model that answers `reply`.
    fn orchestrator_for_ana(
        reply: &'static str,
        max_tokens: u64,
    ) -> (warden_core::orchestrator::Orchestrator, Arc<warden_core::spend::SpendGuard>, Arc<std::sync::atomic::AtomicUsize>) {
        use warden_core::spend::{Limit, MemoryStore, PriceTable, Scope, SpendContext, SpendGuard};
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let guard = Arc::new(SpendGuard::new(
            Arc::new(MemoryStore::default()),
            vec![Limit::new("ana-day", Scope::Person("ana".into()), 24).with_max_tokens(max_tokens)],
            PriceTable::new(Vec::new()),
        ));
        let orchestrator = warden_core::orchestrator::Orchestrator::new(
            Arc::new(Counting { reply, calls: calls.clone() }),
            Arc::new(Vault::new(std::env::temp_dir())),
        )
        .with_spend_guard(guard.clone())
        .with_spend_context(SpendContext::new("server").with_person("ana"));
        (orchestrator, guard, calls)
    }

    #[tokio::test]
    async fn a_draft_comes_back_unsaved_and_is_booked_to_the_person() {
        let (orchestrator, guard, calls) = orchestrator_for_ana(DRAFT, 1_000);

        let reply = draft_skill(&orchestrator, 7, "a skill for reviewing PRs").await;

        match reply {
            ServerMessage::SkillDraft { request_id: 7, skill } => {
                assert_eq!(skill.name, "review-pr");
                assert_eq!(skill.description, "Reviews a PR.");
                assert!(skill.agents.is_empty());
                assert!(!skill.proposed);
            }
            other => panic!("expected a draft, got {other:?}"),
        }
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        let status = guard.status(Some(&warden_core::spend::SpendContext::new("server").with_person("ana")));
        assert_eq!(status[0].used_tokens, 50, "the call was booked to Ana");
    }

    #[tokio::test]
    async fn an_empty_or_oversized_description_is_refused_without_calling_the_model() {
        let (orchestrator, _guard, calls) = orchestrator_for_ana(DRAFT, 1_000);

        for description in ["", "   \n ", &"x".repeat(2_001)] {
            match draft_skill(&orchestrator, 8, description).await {
                ServerMessage::SkillError { request_id: 8, .. } => {}
                other => panic!("expected an error for {:?}, got {other:?}", description.len()),
            }
        }
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn a_description_of_exactly_the_limit_is_accepted() {
        let (orchestrator, _guard, calls) = orchestrator_for_ana(DRAFT, 1_000);

        let reply = draft_skill(&orchestrator, 9, &"x".repeat(2_000)).await;

        assert!(matches!(reply, ServerMessage::SkillDraft { request_id: 9, .. }), "{reply:?}");
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_spending_limit_with_no_room_refuses_the_draft_without_calling_the_model() {
        // A 40-token limit: the first draft fits (0 used) and books 50, so the second has no room.
        let (orchestrator, _guard, calls) = orchestrator_for_ana(DRAFT, 40);

        assert!(matches!(draft_skill(&orchestrator, 1, "one").await, ServerMessage::SkillDraft { .. }));
        match draft_skill(&orchestrator, 2, "two").await {
            ServerMessage::SkillError { request_id: 2, message } => assert!(message.to_lowercase().contains("limit"), "{message}"),
            other => panic!("expected a limit error, got {other:?}"),
        }
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1, "the second request never reached the model");
    }

    #[tokio::test]
    async fn a_reply_that_is_not_a_skill_becomes_an_error_not_a_draft() {
        let (orchestrator, _guard, _calls) = orchestrator_for_ana("sorry, I cannot do that", 1_000);

        match draft_skill(&orchestrator, 3, "a skill").await {
            ServerMessage::SkillError { request_id: 3, .. } => {}
            other => panic!("expected an error, got {other:?}"),
        }
    }
}
