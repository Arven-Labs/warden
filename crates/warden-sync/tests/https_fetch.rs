//! The guarded fetch of an address a stranger chose (P160), against a server of its own on this machine. That server is reachable
//! only with `allow_private`, which is also what proves the default refuses it.

use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Redirect};
use axum::routing::get;
use axum::Router;
use warden_sync::HttpsFetcher;

async fn spawn() -> String {
    let router = Router::new()
        .route("/small", get(|| async { "hello" }))
        .route("/big", get(|| async { vec![b'x'; 4096] }))
        .route(
            "/unannounced",
            get(|| async {
                // No Content-Length: the body is streamed in pieces, so only the count of what was read can stop it.
                let chunks: Vec<Result<Vec<u8>, std::io::Error>> = (0..8).map(|_| Ok(vec![b'x'; 512])).collect();
                axum::body::Body::from_stream(futures_util::stream::iter(chunks))
            }),
        )
        .route("/missing", get(|| async { StatusCode::NOT_FOUND }))
        .route("/elsewhere", get(|| async { Redirect::temporary("ftp://example.org/a") }))
        .route("/hop/{n}", get(|axum::extract::Path(n): axum::extract::Path<u32>| async move { Redirect::temporary(&format!("/hop/{}", n + 1)) }))
        .route("/onward", get(|| async { Redirect::temporary("/small") }))
        .route("/typed", get(|| async { ([(header::CONTENT_TYPE, "application/json")], "{}").into_response() }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    format!("http://{addr}")
}

#[tokio::test]
async fn a_body_that_fits_comes_back_and_a_redirect_within_the_rules_is_followed() {
    let base = spawn().await;
    let fetcher = HttpsFetcher::new(true);
    assert_eq!(fetcher.fetch_capped(&format!("{base}/small"), 1024).await.unwrap(), b"hello".to_vec());
    assert_eq!(fetcher.fetch_capped(&format!("{base}/onward"), 1024).await.unwrap(), b"hello".to_vec());
    assert_eq!(fetcher.fetch_capped(&format!("{base}/typed"), 1024).await.unwrap(), b"{}".to_vec());
}

#[tokio::test]
async fn a_body_over_the_cap_is_refused_announced_or_not() {
    let base = spawn().await;
    let fetcher = HttpsFetcher::new(true);
    for path in ["big", "unannounced"] {
        let err = fetcher.fetch_capped(&format!("{base}/{path}"), 1024).await.unwrap_err().to_string();
        assert!(err.contains("bigger than the 1 KiB allowed"), "{path}: {err}");
    }
}

#[tokio::test]
async fn a_missing_page_is_said_and_a_redirect_somewhere_else_or_without_end_is_not_followed() {
    let base = spawn().await;
    let fetcher = HttpsFetcher::new(true);
    assert!(fetcher.fetch_capped(&format!("{base}/missing"), 1024).await.unwrap_err().to_string().contains("404"));
    let err = format!("{:#}", fetcher.fetch_capped(&format!("{base}/elsewhere"), 1024).await.unwrap_err());
    assert!(err.contains("only https:// addresses are followed"), "{err}");
    let err = format!("{:#}", fetcher.fetch_capped(&format!("{base}/hop/0"), 1024).await.unwrap_err());
    assert!(err.contains("too many redirects"), "{err}");
}

#[tokio::test]
async fn by_default_nothing_on_this_machine_or_this_network_is_reached() {
    let base = spawn().await;
    let fetcher = HttpsFetcher::default();
    // The server is on this machine and speaks http: the scheme alone refuses it, and so does the address.
    let err = fetcher.fetch_capped(&format!("{base}/small"), 1024).await.unwrap_err().to_string();
    assert!(err.contains("only https://"), "{err}");
    let port = base.rsplit(':').next().unwrap();
    for url in [format!("https://127.0.0.1:{port}/small"), "https://10.0.0.5/a".into(), "https://[::1]/a".into(), "https://169.254.169.254/latest/meta-data/".into()] {
        let err = fetcher.fetch_capped(&url, 1024).await.unwrap_err().to_string();
        assert!(err.contains("not a public address"), "{url}: {err}");
    }
    // A name that points to this machine is refused by our resolver, not by the address check.
    let err = format!("{:#}", fetcher.fetch_capped(&format!("https://localhost:{port}/small"), 1024).await.unwrap_err());
    assert!(err.contains("does not point to a public address"), "{err}");
    for bad in ["not an address", "ftp://example.org/a", "file:///etc/passwd"] {
        assert!(fetcher.fetch_capped(bad, 1024).await.is_err(), "{bad}");
    }
}
