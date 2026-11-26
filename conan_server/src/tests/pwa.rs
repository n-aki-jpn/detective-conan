//! PWA 構成（manifest.webmanifest, index.html, sw.js, pwa.js, HTTP 配信ヘッダー）の検証テスト。
use super::*;
use crate::handlers::build_app_router;
use crate::models::{DeckCardItem, DeckDefinition};
use crate::rate_limiter::RateLimiter;
use crate::room::{MatchManager, SharedState};
use crate::user::UserManager;
use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

fn static_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../src/conan_engine/web/static")
}

fn test_state() -> (SharedState, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "conan_pwa_test_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::create_dir_all(&dir);

    let deck_def = DeckDefinition {
        deck_id: Some("pwa_test".to_string()),
        deck_name: Some("pwa_test".to_string()),
        catalog_version: None,
        partner_id: "B01005P".to_string(),
        case_id: "B01005".to_string(),
        main_deck: (0..20)
            .map(|i| DeckCardItem {
                definition_id: format!("CARD{}", i),
                count: 2,
            })
            .collect(),
    };

    let mgr = MatchManager {
        deck_path: dir.join("deck.json"),
        decks_dir: dir.join("decks"),
        user_decks_dir: dir.join("user_decks"),
        catalog: load_test_catalog(),
        deck_def,
        rooms: HashMap::new(),
        user_manager: UserManager::new(&dir),
        rate_limiter: RateLimiter::new(),
    };
    (Arc::new(Mutex::new(mgr)), dir)
}

#[test]
fn test_manifest_valid_json_and_icons_exist() {
    let base = static_dir();
    let manifest_raw = std::fs::read_to_string(base.join("manifest.webmanifest"))
        .expect("manifest.webmanifest must be readable");
    let manifest: Value =
        serde_json::from_str(&manifest_raw).expect("manifest.webmanifest must be valid JSON");

    for field in ["id", "name", "short_name", "start_url", "display"] {
        let val = manifest
            .get(field)
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        assert!(!val.is_empty(), "manifest field '{}' must be non-empty", field);
    }

    let icons = manifest
        .get("icons")
        .and_then(|v| v.as_array())
        .expect("manifest must have icons array");
    assert!(!icons.is_empty(), "manifest icons must not be empty");

    let mut has_192 = false;
    let mut has_512 = false;
    let mut has_maskable = false;

    for icon in icons {
        let src = icon
            .get("src")
            .and_then(|v| v.as_str())
            .expect("each icon must have src");
        let sizes = icon
            .get("sizes")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let purpose = icon
            .get("purpose")
            .and_then(|v| v.as_str())
            .unwrap_or_default();

        if sizes.split_whitespace().any(|s| s == "192x192") {
            has_192 = true;
        }
        if sizes.split_whitespace().any(|s| s == "512x512") {
            has_512 = true;
        }
        if purpose.split_whitespace().any(|p| p == "maskable") {
            has_maskable = true;
        }

        let rel = src.trim_start_matches('/');
        let file_path = base.join(rel);
        let meta = std::fs::metadata(&file_path)
            .unwrap_or_else(|e| panic!("icon file missing ({}): {}", file_path.display(), e));
        assert!(meta.len() > 0, "icon file must not be empty: {}", file_path.display());
    }

    assert!(has_192, "manifest icons must include 192x192");
    assert!(has_512, "manifest icons must include 512x512");
    assert!(has_maskable, "manifest icons must include maskable purpose");
}

#[test]
fn test_index_html_pwa_tags() {
    let html = std::fs::read_to_string(static_dir().join("index.html"))
        .expect("index.html must be readable");
    assert!(
        html.contains(r#"rel="manifest""#),
        "index.html must include rel=\"manifest\""
    );
    assert!(
        html.contains("theme-color"),
        "index.html must include theme-color meta tag"
    );
    assert!(
        html.contains("apple-touch-icon"),
        "index.html must include apple-touch-icon link tag"
    );
    assert!(
        html.contains("/pwa.js"),
        "index.html must reference /pwa.js"
    );
}

#[test]
fn test_sw_and_pwa_js_syntax() {
    let base = static_dir();
    for file in ["sw.js", "pwa.js", "cardSE.js"] {
        let path = base.join(file);
        assert!(path.exists(), "{} must exist", path.display());
        let out = std::process::Command::new("node")
            .arg("--check")
            .arg(&path)
            .output()
            .expect("node must be available");
        assert!(
            out.status.success(),
            "{} syntax check failed:\n{}",
            file,
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[tokio::test]
async fn test_pwa_http_headers_and_offline_page() {
    let (state, tmp_dir) = test_state();
    let app = build_app_router(state, static_dir());

    // /manifest.webmanifest
    let manifest_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/manifest.webmanifest")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(manifest_resp.status(), StatusCode::OK);
    let manifest_ct = manifest_resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    assert!(
        manifest_ct.contains("application/manifest+json"),
        "unexpected manifest Content-Type: {}",
        manifest_ct
    );
    let manifest_cc = manifest_resp
        .headers()
        .get(header::CACHE_CONTROL)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    assert!(
        manifest_cc.contains("no-cache"),
        "unexpected manifest Cache-Control: {}",
        manifest_cc
    );

    // /sw.js
    let sw_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/sw.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(sw_resp.status(), StatusCode::OK);
    let sw_ct = sw_resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    assert!(
        sw_ct.contains("javascript"),
        "unexpected sw.js Content-Type: {}",
        sw_ct
    );
    let sw_cc = sw_resp
        .headers()
        .get(header::CACHE_CONTROL)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    assert!(
        sw_cc.contains("no-cache"),
        "unexpected sw.js Cache-Control: {}",
        sw_cc
    );
    let sw_allowed = sw_resp
        .headers()
        .get("service-worker-allowed")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    assert_eq!(
        sw_allowed, "/",
        "unexpected Service-Worker-Allowed: {}",
        sw_allowed
    );

    // /offline.html
    let offline_resp = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/offline.html")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(offline_resp.status(), StatusCode::OK);

    let _ = std::fs::remove_dir_all(tmp_dir);
}
