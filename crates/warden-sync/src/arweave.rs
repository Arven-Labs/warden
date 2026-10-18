use serde_json::Value;

const DEFAULT_GRAPHQL_URL: &str = "https://arweave.net/graphql";
const DEFAULT_GATEWAY_BASE: &str = "https://arweave.net";

pub struct ArweaveClient {
    graphql_url: String,
    gateway_base: String,
    http: reqwest::Client,
}

impl ArweaveClient {
    pub fn new_default() -> Self {
        Self::new(DEFAULT_GRAPHQL_URL, DEFAULT_GATEWAY_BASE)
    }

    /// Points at a different gateway/GraphQL endpoint — used by tests to target a local fake
    /// gateway instead of the real Arweave network.
    pub fn new(graphql_url: impl Into<String>, gateway_base: impl Into<String>) -> Self {
        Self { graphql_url: graphql_url.into(), gateway_base: gateway_base.into(), http: reqwest::Client::new() }
    }

    /// Most recent transaction id published by `owner_address`, or `None` if that wallet has
    /// never published anything. There's no way to filter by app/tag (`pin()` only ever tags
    /// `App-Name: TruthID`), so "latest by this owner" is the only discovery mechanism available.
    pub async fn latest_tx_by_owner(&self, owner_address: &str) -> anyhow::Result<Option<String>> {
        let query = r#"query($owners:[String!]){ transactions(owners:$owners, first:1, sort:HEIGHT_DESC){ edges { node { id } } } }"#;
        let body = serde_json::json!({ "query": query, "variables": { "owners": [owner_address] } });
        let resp = self.http.post(&self.graphql_url).json(&body).send().await?;
        let text = resp.text().await?;
        Ok(parse_latest_tx_response(&text))
    }

    /// The wallet address that published `tx_id` — used once, right after this device's very
    /// first successful push, to learn the owner address to remember from then on.
    pub async fn owner_of_tx(&self, tx_id: &str) -> anyhow::Result<Option<String>> {
        let query = r#"query($id:ID!){ transaction(id:$id){ owner { address } } }"#;
        let body = serde_json::json!({ "query": query, "variables": { "id": tx_id } });
        let resp = self.http.post(&self.graphql_url).json(&body).send().await?;
        let text = resp.text().await?;
        Ok(parse_owner_response(&text))
    }

    /// Raw bytes stored at `tx_id` — our own already-encrypted bundle blob, since Arweave just
    /// stores whatever bytes the TruthID phone published on our behalf.
    pub async fn fetch_tx_data(&self, tx_id: &str) -> anyhow::Result<Vec<u8>> {
        let url = format!("{}/{}", self.gateway_base, tx_id);
        let resp = self.http.get(&url).send().await?.error_for_status()?;
        Ok(resp.bytes().await?.to_vec())
    }

    /// The bytes at `tx_id` when they fit in `max_bytes`, for content that is not ours and so not to be trusted with the
    /// memory (P160: a published skill package). Refuses a body over the cap by its `Content-Length` and, since that can lie or
    /// be missing, again while reading it; gives up after 30 s; and follows a redirect only to the gateway's own host or to
    /// `*.arweave.net` (the real gateway hands data over to sandbox subdomains), never to anywhere else.
    pub async fn fetch_tx_data_capped(&self, tx_id: &str, max_bytes: usize) -> anyhow::Result<Vec<u8>> {
        if tx_id.is_empty() || !tx_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            anyhow::bail!("'{tx_id}' is not an Arweave transaction id");
        }
        let gateway_host = reqwest::Url::parse(&self.gateway_base)?.host_str().map(str::to_string);
        let redirects = reqwest::redirect::Policy::custom(move |attempt| {
            let allowed = attempt
                .url()
                .host_str()
                .is_some_and(|host| Some(host) == gateway_host.as_deref() || host == "arweave.net" || host.ends_with(".arweave.net"));
            if !allowed {
                attempt.error("the gateway sent the data to another site, which is not followed")
            } else if attempt.previous().len() >= 3 {
                attempt.error("too many redirects")
            } else {
                attempt.follow()
            }
        });
        let client = reqwest::Client::builder().redirect(redirects).timeout(std::time::Duration::from_secs(30)).build()?;

        let url = format!("{}/{}", self.gateway_base, tx_id);
        let mut resp = client.get(&url).send().await?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            anyhow::bail!("nothing at that address yet: a publication can take a few minutes to reach the gateways");
        }
        let mut resp = resp.error_for_status()?;
        let too_big = || anyhow::anyhow!("what is at that address is bigger than the {} KiB allowed", max_bytes / 1024);
        if resp.content_length().is_some_and(|len| len > max_bytes as u64) {
            return Err(too_big());
        }
        let mut data = Vec::new();
        while let Some(chunk) = resp.chunk().await? {
            if data.len() + chunk.len() > max_bytes {
                return Err(too_big());
            }
            data.extend_from_slice(&chunk);
        }
        Ok(data)
    }
}

/// An Arweave transaction id is 43 characters of base64url.
const TX_ID_LEN: usize = 43;

/// The transaction id in an address a person typed or pasted: `ar://<id>`, the id alone, or the gateway's own link
/// `https://arweave.net/<id>`. Nothing else is taken, so what goes into the request is only ever an id.
pub fn parse_address(input: &str) -> anyhow::Result<String> {
    let input = input.trim();
    let id = input.strip_prefix("ar://").or_else(|| input.strip_prefix("https://arweave.net/")).unwrap_or(input).trim_end_matches('/');
    if id.len() == TX_ID_LEN && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        Ok(id.to_string())
    } else {
        anyhow::bail!("this is not an Arweave address: expected ar:// followed by the {TX_ID_LEN}-character id")
    }
}

fn parse_latest_tx_response(json: &str) -> Option<String> {
    let value: Value = serde_json::from_str(json).ok()?;
    value
        .get("data")?
        .get("transactions")?
        .get("edges")?
        .as_array()?
        .first()?
        .get("node")?
        .get("id")?
        .as_str()
        .map(String::from)
}

fn parse_owner_response(json: &str) -> Option<String> {
    let value: Value = serde_json::from_str(json).ok()?;
    value.get("data")?.get("transaction")?.get("owner")?.get("address")?.as_str().map(String::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMN-_0";

    #[test]
    fn an_address_is_ar_scheme_the_bare_id_or_the_gateways_own_link() {
        assert_eq!(ID.len(), TX_ID_LEN);
        for input in [format!("ar://{ID}"), ID.to_string(), format!("https://arweave.net/{ID}"), format!("  ar://{ID}/ \n")] {
            assert_eq!(parse_address(&input).unwrap(), ID, "{input:?}");
        }
    }

    #[test]
    fn anything_else_is_not_an_address() {
        for input in [
            String::new(),
            "ar://".to_string(),
            "ar://short".to_string(),
            format!("ar://{ID}x"),
            format!("ar://{}", &ID[1..]),
            format!("http://arweave.net/{ID}"),
            format!("https://evil.test/{ID}"),
            format!("ar://{ID}/../x"),
            format!("ar://{}%", &ID[1..]),
            "ar://abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMN-_/".to_string(),
        ] {
            assert!(parse_address(&input).is_err(), "{input:?}");
        }
    }

    #[test]
    fn parses_a_real_shaped_latest_tx_success_response() {
        let json = r#"{"data":{"transactions":{"edges":[{"node":{"id":"abc123"}}]}}}"#;
        assert_eq!(parse_latest_tx_response(json), Some("abc123".to_string()));
    }

    #[test]
    fn parses_a_real_shaped_no_results_response() {
        let json = r#"{"data":{"transactions":{"edges":[]}}}"#;
        assert_eq!(parse_latest_tx_response(json), None);
    }

    #[test]
    fn latest_tx_response_never_panics_on_malformed_json() {
        assert_eq!(parse_latest_tx_response("not json"), None);
        assert_eq!(parse_latest_tx_response(r#"{"errors":[{"message":"boom"}]}"#), None);
    }

    #[test]
    fn parses_a_real_shaped_owner_response() {
        let json = r#"{"data":{"transaction":{"owner":{"address":"wallet-abc"}}}}"#;
        assert_eq!(parse_owner_response(json), Some("wallet-abc".to_string()));
    }

    #[test]
    fn owner_response_never_panics_on_malformed_json() {
        assert_eq!(parse_owner_response("not json"), None);
        assert_eq!(parse_owner_response(r#"{"data":{"transaction":null}}"#), None);
    }
}
