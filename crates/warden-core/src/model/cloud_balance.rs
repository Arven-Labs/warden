//! Asking the hosted gateway how much credit an instance has left (`GET {base_url}/balance`, with the instance token).
//!
//! The gateway owns the balance; the instance only reads it, for its usage screen. A failure is never fatal: the caller
//! shows no balance and carries on.

use std::time::Duration;

use serde::Deserialize;

/// A short wait: the usage screen must not hang on a gateway that is slow or down.
const FETCH_TIMEOUT: Duration = Duration::from_secs(3);

/// What the gateway reports, in micro-USD (millionths of a dollar).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct CloudBalance {
    pub balance_micro_usd: i64,
    /// The balance minus what calls in flight hold.
    pub available_micro_usd: i64,
}

/// `GET {base_url}/balance` with `api_key` as the bearer. `Err` carries a short reason for the log.
pub async fn fetch(base_url: &str, api_key: &str) -> Result<CloudBalance, String> {
    let url = format!("{}/balance", base_url.trim_end_matches('/'));
    let response = super::key_check::client()
        .get(url)
        .bearer_auth(api_key)
        .timeout(FETCH_TIMEOUT)
        .send()
        .await
        .map_err(|e| if e.is_timeout() { "the gateway didn't answer in time".to_string() } else { "couldn't connect to the gateway".to_string() })?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("the gateway answered {}", status.as_u16()));
    }
    response.json::<CloudBalance>().await.map_err(|_| "the gateway's balance answer wasn't understood".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::key_check::fake;

    #[tokio::test]
    async fn the_balance_is_read_with_the_instance_token() {
        let server = fake::serve(200, "content-type: application/json\r\n", r#"{"balance_micro_usd":25000000,"available_micro_usd":24000000}"#).await;
        let balance = fetch(&format!("http://{}/v1", server.addr), "wdn_secret").await.unwrap();
        assert_eq!(balance, CloudBalance { balance_micro_usd: 25_000_000, available_micro_usd: 24_000_000 });

        let request = server.first();
        assert!(request.starts_with("get /v1/balance "), "{request}");
        assert!(request.contains("authorization: bearer wdn_secret"), "{request}");
    }

    #[tokio::test]
    async fn a_trailing_slash_on_the_address_is_harmless() {
        let server = fake::serve(200, "", r#"{"balance_micro_usd":1,"available_micro_usd":1}"#).await;
        fetch(&format!("http://{}/v1/", server.addr), "k").await.unwrap();
        assert!(server.first().starts_with("get /v1/balance "), "{}", server.first());
    }

    #[tokio::test]
    async fn a_refusing_or_odd_gateway_is_an_error_not_a_panic() {
        let refusing = fake::serve(401, "", r#"{"error":{"code":"invalid_api_key"}}"#).await;
        assert_eq!(fetch(&format!("http://{}/v1", refusing.addr), "bad").await.unwrap_err(), "the gateway answered 401");

        let odd = fake::serve(200, "", "not json").await;
        assert!(fetch(&format!("http://{}/v1", odd.addr), "k").await.unwrap_err().contains("wasn't understood"));

        // Nothing is listening on this port.
        let unused = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = unused.local_addr().unwrap();
        drop(unused);
        assert!(fetch(&format!("http://{addr}/v1"), "k").await.unwrap_err().contains("connect"));
    }
}
