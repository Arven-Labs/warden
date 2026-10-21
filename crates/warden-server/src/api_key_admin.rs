//! The Warden API's keys (P12) from a client: the same list/create/revoke as `warden-server
//! api-keys`, for the web's settings. Listing is open to any paired device, like the device list;
//! creating or revoking asks for the pairing key again, with the same 1 s wait and the same per-hub
//! lock as a settings save, so they all share one guessing rate.
//!
//! P84 slice 2: a member has keys of their own. They see and change only theirs, confirming with
//! their own password where the owner gives the pairing key, and a key they make can only speak as
//! an agent they see.

use warden_server_protocol::protocol::ApiKeyDto;
use warden_server_protocol::ServerMessage;

use crate::api_keys::{ApiKey, ApiKeyStore};
use crate::settings::{keys_match, SettingsHost, WRONG_KEY_DELAY};

pub fn dto(key: ApiKey) -> ApiKeyDto {
    ApiKeyDto {
        id: key.id,
        name: key.name,
        shown: key.shown,
        created_at_ms: key.created_at_ms,
        last_used_at_ms: key.last_used_at_ms,
        agent_id: key.agent_id,
        user: key.user,
        raw: key.raw,
    }
}

/// Every id of a raw-access list (P131) has to be `*` or a provider or combo of the config at `config_path`; the store
/// itself doesn't read the config. Shared by the web's message, the desktop's command and `warden-server api-keys raw`.
pub fn check_raw_ids(config_path: &std::path::Path, raw: &[String]) -> anyhow::Result<()> {
    if raw.iter().all(|id| id.trim().is_empty() || id.trim() == crate::api_keys::RAW_ALL) {
        return Ok(());
    }
    let config = warden_bootstrap::load_config_from_path(config_path, false)?;
    for id in raw.iter().map(|id| id.trim()).filter(|id| !id.is_empty() && *id != crate::api_keys::RAW_ALL) {
        anyhow::ensure!(
            config.providers.iter().any(|p| p.id == id) || config.combos.iter().any(|c| c.id == id),
            "'{id}' is neither a provider nor a combo of {}",
            config_path.display()
        );
    }
    Ok(())
}

/// A key can only be bound to an agent the hub's config has — one `member` sees, for a member's key
/// (`None`: the owner's agents). Blank is a general key.
pub fn check_agent_exists(config_path: &std::path::Path, agent_id: Option<&str>, member: Option<&str>) -> anyhow::Result<()> {
    let Some(agent_id) = agent_id.map(str::trim).filter(|id| !id.is_empty()) else {
        return Ok(());
    };
    let config = warden_bootstrap::load_config_from_path(config_path, false)?;
    anyhow::ensure!(
        config.agents.iter().any(|a| a.id == agent_id && warden_bootstrap::users::agent_visible_to(a, member)),
        "there is no agent '{agent_id}'"
    );
    Ok(())
}

/// A key can only belong to a member the config at `config_path` has. For `warden-server api-keys create --user`, where
/// the operator makes the key on a member's behalf (a member signed in on the web makes their own through the message).
pub fn check_member_exists(config_path: &std::path::Path, user: &str) -> anyhow::Result<()> {
    let config = warden_bootstrap::load_config_from_path(config_path, false)?;
    anyhow::ensure!(config.users.iter().any(|u| u.id == user), "there is no member '{user}'");
    Ok(())
}

fn api_key_error(request_id: u64, message: String, auth_rejected: bool) -> ServerMessage {
    ServerMessage::ApiKeyError { request_id, message, auth_rejected }
}

/// Every key for the owner (`member` = `None`), a member's own for them.
fn list(store: &ApiKeyStore, member: Option<&str>) -> anyhow::Result<Vec<ApiKeyDto>> {
    Ok(store.list()?.into_iter().filter(|k| member.is_none() || k.user.as_deref() == member).map(dto).collect())
}

/// Answers `ListApiKeys`. `store` is `None` on a hub without the API; `member` is who asks (`None`:
/// the owner).
pub fn handle_list_api_keys(store: Option<&ApiKeyStore>, member: Option<&str>, request_id: u64) -> ServerMessage {
    let Some(store) = store else {
        return api_key_error(request_id, "this hub doesn't offer the Warden API".to_string(), false);
    };
    match list(store, member) {
        Ok(keys) => ServerMessage::ApiKeyList { request_id, keys },
        Err(err) => api_key_error(request_id, format!("failed to read the API keys: {err:#}"), false),
    }
}

/// What `CreateApiKey`/`RevokeApiKey`/`SetApiKeyRaw` asks for.
pub enum ApiKeyChange {
    Create { name: String, agent_id: Option<String> },
    Revoke { id: String },
    /// P131: the providers and combos the key may call straight. The owner's, for a general key.
    SetRaw { id: String, raw: Vec<String> },
}

/// Whether `proof` is the member's own password (P84) — the member's stand-in for the pairing key.
fn member_password_ok(settings: Option<&dyn SettingsHost>, member: &str, proof: &str) -> bool {
    let Some(host) = settings else { return false };
    warden_bootstrap::load_config_from_path(&host.config_path(), false)
        .ok()
        .and_then(|config| config.users.into_iter().find(|u| u.id == member))
        .is_some_and(|user| warden_bootstrap::users::verify_password(&user.password_hash, proof))
}

/// Answers `CreateApiKey` (with `ApiKeyCreated`) or `RevokeApiKey` (with the updated list).
/// `settings` is where the agents are, for a key bound to one; without it only general keys can be
/// made. `member` is who asks (`None`: the owner): `proof` is then their password, not the pairing
/// key, and only their own keys are theirs to see and revoke.
#[allow(clippy::too_many_arguments)]
pub async fn handle_api_key_change(
    store: Option<&ApiKeyStore>,
    settings: Option<&dyn SettingsHost>,
    lock: &tokio::sync::Mutex<()>,
    auth_key: &str,
    member: Option<&str>,
    request_id: u64,
    proof: &str,
    change: ApiKeyChange,
) -> ServerMessage {
    let Some(store) = store else {
        return api_key_error(request_id, "this hub doesn't offer the Warden API".to_string(), false);
    };
    let _serialized = lock.lock().await;
    let proven = match member {
        None => keys_match(proof, auth_key),
        Some(member) => member_password_ok(settings, member, proof),
    };
    if !proven {
        tokio::time::sleep(WRONG_KEY_DELAY).await;
        let what = if member.is_some() { "wrong password" } else { "wrong pairing key" };
        return api_key_error(request_id, what.to_string(), true);
    }
    let result = match change {
        ApiKeyChange::Create { name, agent_id } => {
            let checked = match (agent_id.as_deref().map(str::trim).filter(|id| !id.is_empty()), settings) {
                (None, _) => Ok(()),
                (Some(_), None) => Err(anyhow::anyhow!("this hub has no settings file, so it has no agents to bind a key to")),
                (Some(_), Some(host)) => check_agent_exists(&host.config_path(), agent_id.as_deref(), member),
            };
            checked
                .and_then(|()| store.create_for(&name, agent_id.as_deref(), member))
                .and_then(|created| Ok(ServerMessage::ApiKeyCreated { request_id, key: created.key, keys: list(store, member)? }))
        }
        ApiKeyChange::SetRaw { id, raw } => {
            set_raw(store, settings, member, &id, &raw).and_then(|()| list(store, member)).map(|keys| ServerMessage::ApiKeyList { request_id, keys })
        }
        ApiKeyChange::Revoke { id } => {
            let theirs = store.list().map(|keys| keys.iter().any(|k| k.id == id && (member.is_none() || k.user.as_deref() == member)));
            match theirs {
                Ok(false) => Err(anyhow::anyhow!("no API key with id '{id}'")),
                Err(err) => Err(err),
                Ok(true) => match store.revoke(&id) {
                    Ok(true) => list(store, member).map(|keys| ServerMessage::ApiKeyList { request_id, keys }),
                    Ok(false) => Err(anyhow::anyhow!("no API key with id '{id}'")),
                    Err(err) => Err(err),
                },
            }
        }
    };
    result.unwrap_or_else(|err| api_key_error(request_id, format!("{err:#}"), false))
}

/// `SetApiKeyRaw` (P131): the owner's only, because a member's key can't call a model directly.
fn set_raw(store: &ApiKeyStore, settings: Option<&dyn SettingsHost>, member: Option<&str>, id: &str, raw: &[String]) -> anyhow::Result<()> {
    anyhow::ensure!(member.is_none(), "only the owner can let a key call a model directly");
    anyhow::ensure!(store.list()?.iter().any(|k| k.id == id), "no API key with id '{id}'");
    if !raw.is_empty() {
        let host = settings.ok_or_else(|| anyhow::anyhow!("this hub has no settings file, so it has no providers to choose from"))?;
        check_raw_ids(&host.config_path(), raw)?;
    }
    anyhow::ensure!(store.set_raw(id, raw.to_vec())?, "no API key with id '{id}'");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "pairing-key-0123456789-0123456789";

    fn store(name: &str) -> ApiKeyStore {
        ApiKeyStore::new(std::env::temp_dir().join(format!("warden-hub-api-keys-{name}-{}", std::process::id())).join("api_keys.json"))
    }

    #[test]
    fn a_key_for_a_member_needs_the_member_in_the_config() {
        let dir = std::env::temp_dir().join(format!("warden-hub-api-keys-member-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let config_path = dir.join("config.toml");
        let mut config = warden_bootstrap::FileConfig::default();
        warden_bootstrap::users::add_user(&mut config, "ana", "Ana", "provisional-pass").unwrap();
        warden_bootstrap::save_config(&config_path, &config).unwrap();

        check_member_exists(&config_path, "ana").unwrap();
        assert!(check_member_exists(&config_path, "ghost").is_err());

        let store = store("member");
        let created = store.create_for("ana-script", None, Some("ana")).unwrap();
        let found = store.authenticate(&created.key).unwrap().expect("the key works");
        assert_eq!(found.user.as_deref(), Some("ana"), "a call with it runs as the member");
    }

    #[tokio::test]
    async fn creating_needs_the_pairing_key_and_shows_the_key_once() {
        let store = store("create");
        let lock = tokio::sync::Mutex::new(());
        let started = std::time::Instant::now();
        let reply = handle_api_key_change(Some(&store), None, &lock, KEY, None, 1, "wrong", ApiKeyChange::Create { name: "n8n".into(), agent_id: None }).await;
        assert!(matches!(reply, ServerMessage::ApiKeyError { auth_rejected: true, .. }), "{reply:?}");
        assert!(started.elapsed() >= WRONG_KEY_DELAY);
        assert!(store.list().unwrap().is_empty(), "nothing was created");

        let reply = handle_api_key_change(Some(&store), None, &lock, KEY, None, 2, KEY, ApiKeyChange::Create { name: "n8n".into(), agent_id: None }).await;
        let ServerMessage::ApiKeyCreated { key, keys, .. } = reply else { panic!("{reply:?}") };
        assert!(store.authenticate(&key).unwrap().is_some());
        let ServerMessage::ApiKeyList { keys: listed, .. } = handle_list_api_keys(Some(&store), None, 3) else { panic!() };
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, keys[0].id);

        let reply = handle_api_key_change(Some(&store), None, &lock, KEY, None, 4, KEY, ApiKeyChange::Revoke { id: keys[0].id.clone() }).await;
        assert!(matches!(reply, ServerMessage::ApiKeyList { ref keys, .. } if keys.is_empty()), "{reply:?}");
        assert!(store.authenticate(&key).unwrap().is_none());
        let reply = handle_api_key_change(Some(&store), None, &lock, KEY, None, 5, KEY, ApiKeyChange::Revoke { id: "ghost".into() }).await;
        assert!(matches!(reply, ServerMessage::ApiKeyError { auth_rejected: false, .. }));
    }

    struct Host(std::path::PathBuf);

    #[async_trait::async_trait]
    impl SettingsHost for Host {
        fn config_path(&self) -> std::path::PathBuf {
            self.0.clone()
        }

        async fn build(&self) -> anyhow::Result<warden_core::orchestrator::Orchestrator> {
            anyhow::bail!("not used")
        }
    }

    #[tokio::test]
    async fn a_key_is_only_bound_to_an_agent_that_exists() {
        let store = store("bound");
        let config = std::env::temp_dir().join(format!("warden-hub-api-keys-config-{}.toml", std::process::id()));
        std::fs::write(&config, "[[agents]]\nid = \"poet\"\npersona = \"p\"\n").unwrap();
        let host = Host(config);
        let lock = tokio::sync::Mutex::new(());
        let create = |agent: &str| ApiKeyChange::Create { name: format!("for-{agent}"), agent_id: Some(agent.to_string()) };

        let reply = handle_api_key_change(Some(&store), Some(&host), &lock, KEY, None, 1, KEY, create("ghost")).await;
        assert!(matches!(reply, ServerMessage::ApiKeyError { auth_rejected: false, ref message, .. } if message.contains("no agent 'ghost'")), "{reply:?}");
        let reply = handle_api_key_change(Some(&store), None, &lock, KEY, None, 2, KEY, create("poet")).await;
        assert!(matches!(reply, ServerMessage::ApiKeyError { .. }), "no settings, no agents: {reply:?}");
        assert!(store.list().unwrap().is_empty(), "nothing was created");

        let reply = handle_api_key_change(Some(&store), Some(&host), &lock, KEY, None, 3, KEY, create("poet")).await;
        let ServerMessage::ApiKeyCreated { keys, .. } = reply else { panic!("{reply:?}") };
        assert_eq!(keys[0].agent_id.as_deref(), Some("poet"));
    }

    fn raw_config(name: &str) -> Host {
        let config = std::env::temp_dir().join(format!("warden-hub-api-keys-raw-{name}-{}.toml", std::process::id()));
        std::fs::write(
            &config,
            "[[providers]]\nid = \"spare\"\nkind = \"openai\"\nmodel = \"m\"\n\n[[providers]]\nid = \"far\"\nkind = \"node\"\nnode = \"n\"\nmodel = \"m\"\n\n[[combos]]\nid = \"fast\"\nproviders = [\"spare\"]\n\n[[agents]]\nid = \"poet\"\npersona = \"p\"\n",
        )
        .unwrap();
        Host(config)
    }

    fn set(id: &str, raw: &[&str]) -> ApiKeyChange {
        ApiKeyChange::SetRaw { id: id.to_string(), raw: raw.iter().map(|r| r.to_string()).collect() }
    }

    fn raw_of(reply: &ServerMessage) -> Vec<Vec<String>> {
        let ServerMessage::ApiKeyList { keys, .. } = reply else { panic!("{reply:?}") };
        keys.iter().map(|k| k.raw.clone()).collect()
    }

    #[tokio::test]
    async fn the_owner_sets_what_a_key_may_call_with_the_pairing_key() {
        let store = store("raw-set");
        let host = raw_config("set");
        let lock = tokio::sync::Mutex::new(());
        let id = store.create("script", None).unwrap().info.id;

        // The wrong proof changes nothing.
        let started = std::time::Instant::now();
        let reply = handle_api_key_change(Some(&store), Some(&host), &lock, KEY, None, 1, "wrong", set(&id, &["spare"])).await;
        assert!(matches!(reply, ServerMessage::ApiKeyError { auth_rejected: true, .. }), "{reply:?}");
        assert!(started.elapsed() >= WRONG_KEY_DELAY);
        assert!(store.list().unwrap()[0].raw.is_empty());

        // A provider, a combo, a node's model named on purpose, and `*`.
        let reply = handle_api_key_change(Some(&store), Some(&host), &lock, KEY, None, 2, KEY, set(&id, &["spare", "fast", "far"])).await;
        assert_eq!(raw_of(&reply), [vec!["spare".to_string(), "fast".to_string(), "far".to_string()]]);
        let reply = handle_api_key_change(Some(&store), Some(&host), &lock, KEY, None, 3, KEY, set(&id, &["*"])).await;
        assert_eq!(raw_of(&reply), [vec!["*".to_string()]]);
        assert_eq!(store.list().unwrap()[0].raw, ["*"], "it reached the file");

        // The list is listed back to whoever asks, and emptying it takes the access away.
        assert_eq!(raw_of(&handle_list_api_keys(Some(&store), None, 4)), [vec!["*".to_string()]]);
        let reply = handle_api_key_change(Some(&store), Some(&host), &lock, KEY, None, 5, KEY, set(&id, &[])).await;
        assert_eq!(raw_of(&reply), [Vec::<String>::new()]);
    }

    #[tokio::test]
    async fn an_id_the_config_does_not_have_is_refused_and_leaves_the_list_as_it_was() {
        let store = store("raw-unknown");
        let host = raw_config("unknown");
        let lock = tokio::sync::Mutex::new(());
        let id = store.create("script", None).unwrap().info.id;
        store.set_raw(&id, vec!["spare".into()]).unwrap();

        let reply = handle_api_key_change(Some(&store), Some(&host), &lock, KEY, None, 1, KEY, set(&id, &["spare", "ghost"])).await;
        assert!(matches!(reply, ServerMessage::ApiKeyError { auth_rejected: false, ref message, .. } if message.contains("'ghost'")), "{reply:?}");
        assert_eq!(store.list().unwrap()[0].raw, ["spare"]);
        let reply = handle_api_key_change(Some(&store), Some(&host), &lock, KEY, None, 2, KEY, set("no-such-key", &["spare"])).await;
        assert!(matches!(reply, ServerMessage::ApiKeyError { ref message, .. } if message.contains("no API key")), "{reply:?}");
        let reply = handle_api_key_change(Some(&store), None, &lock, KEY, None, 3, KEY, set(&id, &["spare"])).await;
        assert!(matches!(reply, ServerMessage::ApiKeyError { .. }), "no settings file, no providers to choose from: {reply:?}");
        // `*` and an empty list need no config at all.
        let reply = handle_api_key_change(Some(&store), None, &lock, KEY, None, 4, KEY, set(&id, &[])).await;
        assert_eq!(raw_of(&reply), [Vec::<String>::new()]);
    }

    #[tokio::test]
    async fn a_key_of_an_agent_or_a_member_cannot_be_given_a_list() {
        let store = store("raw-bound");
        let host = raw_config("bound");
        let lock = tokio::sync::Mutex::new(());
        let bound = store.create("bot", Some("poet")).unwrap().info.id;
        let reply = handle_api_key_change(Some(&store), Some(&host), &lock, KEY, None, 1, KEY, set(&bound, &["spare"])).await;
        assert!(matches!(reply, ServerMessage::ApiKeyError { auth_rejected: false, ref message, .. } if message.contains("bound to an agent")), "{reply:?}");

        let members = store.create_for("bia-key", None, Some("bia")).unwrap().info.id;
        let reply = handle_api_key_change(Some(&store), Some(&host), &lock, KEY, None, 2, KEY, set(&members, &["spare"])).await;
        assert!(matches!(reply, ServerMessage::ApiKeyError { auth_rejected: false, .. }), "{reply:?}");
        assert!(store.list().unwrap().iter().all(|k| k.raw.is_empty()));
    }

    #[test]
    fn a_member_is_refused_before_anything_is_looked_at() {
        let store = store("raw-member");
        let id = store.create_for("bia-key", None, Some("bia")).unwrap().info.id;
        let err = set_raw(&store, None, Some("bia"), &id, &["spare".to_string()]).unwrap_err();
        assert!(format!("{err:#}").contains("only the owner"), "{err:#}");
        // Even to take the access away: nothing a member asks reaches the list.
        assert!(set_raw(&store, None, Some("bia"), &id, &[]).is_err());
    }

    #[test]
    fn check_raw_ids_takes_star_blanks_providers_and_combos() {
        let host = raw_config("check");
        let ok = |ids: &[&str]| check_raw_ids(&host.0, &ids.iter().map(|i| i.to_string()).collect::<Vec<_>>());
        assert!(ok(&[]).is_ok() && ok(&["*"]).is_ok() && ok(&[" ", ""]).is_ok());
        assert!(ok(&["spare", "fast", "far", " spare "]).is_ok());
        let err = ok(&["spare", "nope"]).unwrap_err();
        assert!(format!("{err:#}").contains("'nope'"), "{err:#}");
        assert!(ok(&["poet"]).is_err(), "an agent is neither a provider nor a combo");
        // Star and blanks never read the config, so a missing one is fine for them.
        assert!(check_raw_ids(std::path::Path::new("/nonexistent/config.toml"), &["*".to_string()]).is_ok());
    }
}
