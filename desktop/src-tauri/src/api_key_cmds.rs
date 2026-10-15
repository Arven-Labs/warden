//! Tauri commands backing Settings' "Warden API" section (P12): the keys the embedded hub's
//! OpenAI-compatible routes accept. Stateless like `workspace_cmds.rs`: `ApiKeyStore` rereads
//! `api_keys.json` on every call, the same file `warden-server api-keys` and the web use.

use serde::Serialize;
use warden_server::api_keys::{ApiKey, ApiKeyStore};

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyInfo {
    id: String,
    name: String,
    shown: String,
    created_at_ms: i64,
    last_used_at_ms: Option<i64>,
    /// The only agent this key speaks as; `None` for a general key.
    agent_id: Option<String>,
    /// P84: the member the key belongs to; `None` for the owner's.
    user: Option<String>,
    /// P131: the providers and combos (ids, or `*`) the key may call straight; empty for none.
    raw: Vec<String>,
}

impl From<ApiKey> for ApiKeyInfo {
    fn from(key: ApiKey) -> Self {
        Self {
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
}

/// A key just created: `key` is shown once and kept nowhere.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedApiKeyInfo {
    key: String,
    info: ApiKeyInfo,
}

fn store() -> Result<ApiKeyStore, String> {
    warden_bootstrap::default_api_keys_path().map(ApiKeyStore::new).ok_or_else(|| "could not determine the OS config directory".to_string())
}

#[tauri::command]
pub fn list_api_keys() -> Result<Vec<ApiKeyInfo>, String> {
    Ok(store()?.list().map_err(|e| format!("{e:#}"))?.into_iter().map(Into::into).collect())
}

/// `agent_id` binds the key to that agent (it must be in the config); `None` or blank is general.
#[tauri::command]
pub fn create_api_key(name: String, agent_id: Option<String>) -> Result<CreatedApiKeyInfo, String> {
    if agent_id.as_deref().is_some_and(|a| !a.trim().is_empty()) {
        let config_path = warden_bootstrap::default_config_path().ok_or_else(|| "could not determine the OS config directory".to_string())?;
        warden_server::api_key_admin::check_agent_exists(&config_path, agent_id.as_deref(), None).map_err(|e| format!("{e:#}"))?;
    }
    let created = store()?.create(&name, agent_id.as_deref()).map_err(|e| format!("{e:#}"))?;
    Ok(CreatedApiKeyInfo { key: created.key, info: created.info.into() })
}

/// P131: which providers and combos the key may call straight (ids of the config, or `*`); empty takes it away. The same
/// rules as the web's `SetApiKeyRaw` — a general key of the owner's only — minus the pairing key, since this is the machine
/// itself. Returns the keys as they stand.
#[tauri::command]
pub fn set_api_key_raw(id: String, raw: Vec<String>) -> Result<Vec<ApiKeyInfo>, String> {
    let store = store()?;
    if !raw.is_empty() {
        let config_path = warden_bootstrap::default_config_path().ok_or_else(|| "could not determine the OS config directory".to_string())?;
        warden_server::api_key_admin::check_raw_ids(&config_path, &raw).map_err(|e| format!("{e:#}"))?;
    }
    if !store.set_raw(&id, raw).map_err(|e| format!("{e:#}"))? {
        return Err(format!("no API key with id '{id}'"));
    }
    Ok(store.list().map_err(|e| format!("{e:#}"))?.into_iter().map(Into::into).collect())
}

#[tauri::command]
pub fn revoke_api_key(id: String) -> Result<(), String> {
    if store()?.revoke(&id).map_err(|e| format!("{e:#}"))? {
        Ok(())
    } else {
        Err(format!("no API key with id '{id}'"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_raw_list_of_a_key_reaches_the_screen_as_the_store_has_it() {
        let dir = std::env::temp_dir().join(format!("warden-desktop-api-keys-{}", std::process::id()));
        let store = ApiKeyStore::new(dir.join("api_keys.json"));
        let general = store.create("script", None).unwrap().info.id;
        store.create("bot", Some("poet")).unwrap();
        store.set_raw(&general, vec!["spare".into(), "*".into()]).unwrap();

        let shown: Vec<ApiKeyInfo> = store.list().unwrap().into_iter().map(Into::into).collect();
        assert_eq!(shown[0].raw, ["spare", "*"]);
        assert!(shown[1].raw.is_empty(), "a key bound to an agent has none");
        // The wire name the screen reads, and no list at all when empty is still an (empty) list.
        let json = serde_json::to_value(&shown[1]).unwrap();
        assert_eq!(json["raw"], serde_json::json!([]));
        assert_eq!(serde_json::to_value(&shown[0]).unwrap()["raw"], serde_json::json!(["spare", "*"]));
    }
}
