//! OAuth 2.0 grants against the testkit authorization server.

use std::time::Duration;

use zorvik_engine::{Client, RequestOptions};
use zorvik_formats::{ClientAuthMethod, GrantType, OAuth2Config};
use zorvik_testkit::TestServer;
use zorvik_workspace::oauth2::{TokenCache, TokenSet, authorization_code_flow, cache_key, ensure_token};

fn config(server: &TestServer, grant: GrantType) -> OAuth2Config {
    OAuth2Config {
        grant_type: grant,
        token_url: server.url("/oauth/token"),
        auth_url: server.url("/oauth/authorize"),
        client_id: "test-client".into(),
        client_secret: "test-secret".into(),
        username: "alice".into(),
        password: "wonderland".into(),
        scope: "read".into(),
        ..Default::default()
    }
}

#[tokio::test]
async fn client_credentials_is_cached() {
    let server = TestServer::start().await;
    let client = Client::new();
    let cache = TokenCache::in_memory();
    let opts = RequestOptions::default();
    let cfg = config(&server, GrantType::ClientCredentials);
    let first = ensure_token(&client, &opts, &cfg, &cache, "w").await.unwrap();
    let second = ensure_token(&client, &opts, &cfg, &cache, "w").await.unwrap();
    assert_eq!(first.access_token, second.access_token);
    assert!(first.access_token.starts_with("access-"));
    assert_eq!(first.scope.as_deref(), Some("read"));

    let body_auth = OAuth2Config { client_auth: ClientAuthMethod::Body, scope: "other".into(), ..cfg.clone() };
    let token = ensure_token(&client, &opts, &body_auth, &cache, "w").await.unwrap();
    assert_ne!(token.access_token, first.access_token);

    let bad = OAuth2Config { client_secret: "wrong".into(), scope: "x".into(), ..cfg };
    let err = ensure_token(&client, &opts, &bad, &cache, "w").await.unwrap_err();
    assert!(err.message.contains("invalid_client"), "{}", err.message);
}

#[tokio::test]
async fn password_grant_and_refresh() {
    let server = TestServer::start().await;
    let client = Client::new();
    let cache = TokenCache::in_memory();
    let opts = RequestOptions::default();
    let cfg = config(&server, GrantType::Password);
    let token = ensure_token(&client, &opts, &cfg, &cache, "w").await.unwrap();
    // Expire it; the refresh token is used instead of the password.
    cache.put(&cache_key("w", &cfg), TokenSet { expires_at: Some(0), ..token.clone() });
    let refreshed = ensure_token(&client, &opts, &cfg, &cache, "w").await.unwrap();
    assert_ne!(refreshed.access_token, token.access_token);
}

#[tokio::test]
async fn authorization_code_requires_interaction_then_works_with_pkce() {
    let server = TestServer::start().await;
    let client = Client::new();
    let cache = TokenCache::in_memory();
    let opts = RequestOptions::default();
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let cfg = OAuth2Config {
        redirect_uri: format!("http://127.0.0.1:{port}/callback"),
        ..config(&server, GrantType::AuthorizationCode)
    };
    let err = ensure_token(&client, &opts, &cfg, &cache, "w").await.unwrap_err();
    assert!(err.message.contains("Get token"));

    // "Browser": follow the authorize redirect to our loopback listener.
    let browser = |url: &str| {
        let url = url.to_string();
        tokio::spawn(async move {
            let req =
                zorvik_engine::HttpRequest { method: "GET".into(), url, headers: vec![], body: Default::default() };
            let resp = Client::new().send(req, &RequestOptions::default(), None).await.unwrap();
            assert!(String::from_utf8_lossy(&resp.body).contains("Done"));
        });
        Ok(())
    };
    let token =
        authorization_code_flow(&client, &opts, &cfg, &cache, "w", browser, Duration::from_secs(10)).await.unwrap();
    assert!(token.access_token.starts_with("access-"));
    // Now cached for normal sends.
    let again = ensure_token(&client, &opts, &cfg, &cache, "w").await.unwrap();
    assert_eq!(again.access_token, token.access_token);
}

#[tokio::test]
async fn authorization_code_rejects_non_loopback_redirect() {
    let server = TestServer::start().await;
    let cfg =
        OAuth2Config { redirect_uri: "https://example.com/cb".into(), ..config(&server, GrantType::AuthorizationCode) };
    let err = authorization_code_flow(
        &Client::new(),
        &RequestOptions::default(),
        &cfg,
        &TokenCache::in_memory(),
        "w",
        |_| Ok(()),
        Duration::from_secs(1),
    )
    .await
    .unwrap_err();
    assert!(err.message.contains("loopback"));
}

#[tokio::test]
async fn idle_connection_does_not_block_the_redirect() {
    let server = TestServer::start().await;
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let cfg = OAuth2Config {
        redirect_uri: format!("http://127.0.0.1:{port}/callback"),
        ..config(&server, GrantType::AuthorizationCode)
    };
    let browser = move |url: &str| {
        let url = url.to_string();
        tokio::spawn(async move {
            // Browsers may open a speculative connection that never sends a request.
            let _idle = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
            let req =
                zorvik_engine::HttpRequest { method: "GET".into(), url, headers: vec![], body: Default::default() };
            let resp = Client::new().send(req, &RequestOptions::default(), None).await.unwrap();
            assert!(String::from_utf8_lossy(&resp.body).contains("Done"));
        });
        Ok(())
    };
    let token = authorization_code_flow(
        &Client::new(),
        &RequestOptions::default(),
        &cfg,
        &TokenCache::in_memory(),
        "w",
        browser,
        Duration::from_secs(20),
    )
    .await
    .unwrap();
    assert!(token.access_token.starts_with("access-"));
}

#[tokio::test]
async fn a_forged_redirect_cannot_cancel_the_sign_in() {
    let server = TestServer::start().await;
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let cfg = OAuth2Config {
        redirect_uri: format!("http://127.0.0.1:{port}/callback"),
        ..config(&server, GrantType::AuthorizationCode)
    };
    let browser = move |url: &str| {
        let url = url.to_string();
        tokio::spawn(async move {
            let get = |url: String| zorvik_engine::HttpRequest {
                method: "GET".into(),
                url,
                headers: vec![],
                body: Default::default(),
            };
            // Any web page can make the browser hit the loopback port first.
            let forged = format!("http://127.0.0.1:{port}/callback?error=access_denied&state=forged");
            let resp = Client::new().send(get(forged), &RequestOptions::default(), None).await.unwrap();
            assert!(String::from_utf8_lossy(&resp.body).contains("does not belong"));
            let resp = Client::new().send(get(url), &RequestOptions::default(), None).await.unwrap();
            assert!(String::from_utf8_lossy(&resp.body).contains("Done"));
        });
        Ok(())
    };
    let token = authorization_code_flow(
        &Client::new(),
        &RequestOptions::default(),
        &cfg,
        &TokenCache::in_memory(),
        "w",
        browser,
        Duration::from_secs(20),
    )
    .await
    .unwrap();
    assert!(token.access_token.starts_with("access-"));
}

#[tokio::test]
async fn implicit_grant_reads_the_token_from_the_fragment() {
    let server = TestServer::start().await;
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let cfg = OAuth2Config {
        redirect_uri: format!("http://127.0.0.1:{port}/callback"),
        ..config(&server, GrantType::Implicit)
    };
    // A stand-in for the browser: the provider redirects to `callback#access_token=…`; the
    // browser loads `callback` (no fragment on the wire), and the page sends the fragment back.
    let browser = move |url: &str| {
        let authorize = url::Url::parse(url).unwrap();
        let params: std::collections::HashMap<String, String> = authorize.query_pairs().into_owned().collect();
        assert_eq!(params["response_type"], "token");
        assert!(!params.contains_key("code_challenge"), "no PKCE for the implicit grant");
        let state = params["state"].clone();
        tokio::spawn(async move {
            let get = |url: String| zorvik_engine::HttpRequest {
                method: "GET".into(),
                url,
                headers: vec![],
                body: Default::default(),
            };
            let page = Client::new()
                .send(get(format!("http://127.0.0.1:{port}/callback")), &RequestOptions::default(), None)
                .await
                .unwrap();
            assert!(String::from_utf8_lossy(&page.body).contains("location.hash"));
            let back = format!(
                "http://127.0.0.1:{port}/callback?access_token=implicit-1&token_type=Bearer&expires_in=3600&state={state}"
            );
            let resp = Client::new().send(get(back), &RequestOptions::default(), None).await.unwrap();
            assert!(String::from_utf8_lossy(&resp.body).contains("Done"));
        });
        Ok(())
    };
    let cache = TokenCache::in_memory();
    let token = authorization_code_flow(
        &Client::new(),
        &RequestOptions::default(),
        &cfg,
        &cache,
        "w",
        browser,
        Duration::from_secs(20),
    )
    .await
    .unwrap();
    assert_eq!((token.access_token.as_str(), token.refresh_token.as_deref()), ("implicit-1", None));
    assert!(token.is_fresh());
    assert_eq!(cache.get(&cache_key("w", &cfg)).unwrap().access_token, "implicit-1");
    // Later sends use the cached token without signing in again.
    let again = ensure_token(&Client::new(), &RequestOptions::default(), &cfg, &cache, "w").await.unwrap();
    assert_eq!(again.access_token, "implicit-1");
}
