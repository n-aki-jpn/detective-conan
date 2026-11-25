//! T1〜T3: HTTPハンドラの認証・権限・移行APIの統合テスト
//! axum + tower::ServiceExt::oneshot で、実サーバーを立てずにエンドポイント挙動を検証する。
use super::*;
use crate::handlers::build_api_router;
use crate::models::{DeckCardItem, DeckDefinition};
use crate::rate_limiter::RateLimiter;
use crate::room::MatchManager;
use crate::user::UserManager;
use axum::body::Body;
use axum::http::{header, Method, StatusCode};
use axum::response::Response;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tower::ServiceExt; // for oneshot

type SharedState = Arc<Mutex<MatchManager>>;

/// テスト専用の一時 MatchManager を構築（実データに干渉しないよう全パスを一時dirへ）
fn test_state() -> (SharedState, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "conan_http_test_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::create_dir_all(&dir);

    let catalog = load_test_catalog();
    let deck_def = deck40("seed");
    let mgr = MatchManager {
        deck_path: dir.join("deck.json"),
        decks_dir: dir.join("decks"),
        user_decks_dir: dir.join("user_decks"),
        catalog: catalog.clone(),
        deck_def,
        rooms: HashMap::new(),
        user_manager: UserManager::new(&dir),
        rate_limiter: RateLimiter::new(),
    };
    let state: SharedState = Arc::new(Mutex::new(mgr));
    // preset を1つ配置（削除拒否テスト用）
    let _ = std::fs::create_dir_all(dir.join("decks"));
    let _ = std::fs::write(
        dir.join("decks").join("preset_test.json"),
        serde_json::to_string_pretty(&deck40("preset_test")).unwrap(),
    );
    (state, dir)
}

/// 40枚・パートナー/事件ありの有効デッキ
fn deck40(id: &str) -> DeckDefinition {
    DeckDefinition {
        deck_id: Some(id.to_string()),
        deck_name: Some(id.to_string()),
        catalog_version: None,
        partner_id: "B01005P".to_string(),
        case_id: "B01005".to_string(),
        main_deck: (0..20)
            .map(|i| DeckCardItem {
                definition_id: format!("CARD{}", i),
                count: 2,
            })
            .collect(),
    }
}

fn make_req(
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
    ip: &str,
) -> axum::http::Request<Body> {
    let mut b = axum::http::Request::builder()
        .method(method)
        .uri(uri)
        .header("x-forwarded-for", ip);
    if let Some(t) = token {
        b = b.header(header::AUTHORIZATION, format!("Bearer {}", t));
    }
    if body.is_some() {
        b = b.header(header::CONTENT_TYPE, "application/json");
    }
    let body = match body {
        Some(v) => Body::from(v.to_string()),
        None => Body::empty(),
    };
    b.body(body).unwrap()
}

async fn json_of(resp: Response) -> Value {
    let bytes = BodyExt::collect(resp.into_body())
        .await
        .unwrap()
        .to_bytes();
    serde_json::from_slice(&bytes).unwrap_or_else(|_| json!({}))
}

/// 登録して (token, user_id) を返す
async fn register(state: &SharedState, nickname: &str, ip: &str) -> (String, String) {
    let app = build_api_router(state.clone());
    let resp = app
        .oneshot(make_req(
            Method::POST,
            "/api/register",
            None,
            Some(json!({ "nickname": nickname })),
            ip,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "登録は200のはず");
    let v = json_of(resp).await;
    (
        v["token"].as_str().unwrap().to_string(),
        v["user"]["id"].as_str().unwrap().to_string(),
    )
}

// ─── T1: 認証統合テスト（401 / 403 / 400 / 200）─────────────────────────────

#[tokio::test]
async fn t1_requires_auth_returns_401() {
    let (state, dir) = test_state();
    let app = build_api_router(state.clone());
    // トークンなしで /api/me
    let resp = app
        .oneshot(make_req(Method::GET, "/api/me", None, None, "ip-t1"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "未認証は401");

    // 無効トークンでも401
    let app = build_api_router(state.clone());
    let resp = app
        .oneshot(make_req(
            Method::GET,
            "/api/me",
            Some("invalid_token_xxx"),
            None,
            "ip-t1",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "無効トークンは401");

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn t1_valid_token_access_ok_200() {
    let (state, dir) = test_state();
    let (token, _) = register(&state, "T1ユーザー", "ip-t1-ok").await;
    let app = build_api_router(state.clone());
    let resp = app
        .oneshot(make_req(Method::GET, "/api/me", Some(&token), None, "ip-t1-ok"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "有効トークンは200");
    let v = json_of(resp).await;
    assert_eq!(v["user"]["nickname"], json!("T1ユーザー"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn t1_delete_preset_returns_403() {
    let (state, dir) = test_state();
    let (token, _) = register(&state, "T1削除", "ip-t1-del").await;
    let app = build_api_router(state.clone());
    let resp = app
        .oneshot(make_req(
            Method::DELETE,
            "/api/decks/preset_test",
            Some(&token),
            None,
            "ip-t1-del",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN, "プリセット削除は403");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn t1_save_deck_invalid_returns_400() {
    let (state, dir) = test_state();
    let (token, _) = register(&state, "T1不正", "ip-t1-bad").await;
    // 39枚（40枚ではない）で保存を試みる
    let mut bad = deck40("bad");
    bad.main_deck.pop(); // 1種減らして38枚
    let app = build_api_router(state.clone());
    let resp = app
        .oneshot(make_req(
            Method::POST,
            "/api/save_deck",
            Some(&token),
            Some(serde_json::to_value(&bad).unwrap()),
            "ip-t1-bad",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "枚数不正は400");
    let _ = std::fs::remove_dir_all(&dir);
}

// ─── T2: 移行APIの冪等性テスト ──────────────────────────────────────────────

#[tokio::test]
async fn t2_migrate_is_idempotent_and_skips_preset() {
    let (state, dir) = test_state();
    let (token, _uid) = register(&state, "T2ユーザー", "ip-t2").await;

    let decks = vec![
        {
            let mut d = deck40("mig1");
            d.deck_id = Some("mig1".to_string());
            d
        },
        {
            let mut d = deck40("mig2");
            d.deck_id = Some("mig2".to_string());
            d
        },
        {
            // preset_ 接頭辞はスキップされるはず
            let mut d = deck40("preset_skip");
            d.deck_id = Some("preset_skip".to_string());
            d
        },
    ];
    let payload = json!({ "decks": decks });

    // 1回目: mig1/mig2 を import(2)、preset_skip は skip(1)
    let app = build_api_router(state.clone());
    let resp = app
        .oneshot(make_req(
            Method::POST,
            "/api/decks/migrate",
            Some(&token),
            Some(payload.clone()),
            "ip-t2",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v = json_of(resp).await;
    assert_eq!(v["imported"], json!(2), "初回は2件import");
    assert_eq!(v["skipped"], json!(1), "presetはskip");

    // 2回目: 全て既存/presetなので import=0, skip=3（冪等）
    let app = build_api_router(state.clone());
    let resp = app
        .oneshot(make_req(
            Method::POST,
            "/api/decks/migrate",
            Some(&token),
            Some(payload),
            "ip-t2",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v = json_of(resp).await;
    assert_eq!(v["imported"], json!(0), "再実行でimportは0（冪等）");
    assert_eq!(v["skipped"], json!(3), "既存2+preset1でskip3");

    let _ = std::fs::remove_dir_all(&dir);
}

// ─── T3: 他ユーザーのデッキへのアクセス拒否テスト ──────────────────────────

#[tokio::test]
async fn t3_cannot_access_other_users_deck() {
    let (state, dir) = test_state();
    let (token_a, _uid_a) = register(&state, "ユーザーA", "ip-t3-a").await;
    let (token_b, _uid_b) = register(&state, "ユーザーB", "ip-t3-b").await;

    // A が自分のデッキを保存
    let mut deck = deck40("secret_deck");
    deck.deck_id = Some("secret_deck".to_string());
    let app = build_api_router(state.clone());
    let resp = app
        .oneshot(make_req(
            Method::POST,
            "/api/save_deck",
            Some(&token_a),
            Some(serde_json::to_value(&deck).unwrap()),
            "ip-t3-a",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "Aの保存は成功");

    // A 自身は取得できる
    let app = build_api_router(state.clone());
    let resp = app
        .oneshot(make_req(
            Method::GET,
            "/api/decks/secret_deck",
            Some(&token_a),
            None,
            "ip-t3-a",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "A本人は取得できる");

    // B は同じ deck_id を取得できない（404）
    let app = build_api_router(state.clone());
    let resp = app
        .oneshot(make_req(
            Method::GET,
            "/api/decks/secret_deck",
            Some(&token_b),
            None,
            "ip-t3-b",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "BはAのデッキを取得できない");

    // B は同じ deck_id を削除できない（404）
    let app = build_api_router(state.clone());
    let resp = app
        .oneshot(make_req(
            Method::DELETE,
            "/api/decks/secret_deck",
            Some(&token_b),
            None,
            "ip-t3-b",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "BはAのデッキを削除できない");

    // 削除されていない＝A はまだ取得できる
    let app = build_api_router(state.clone());
    let resp = app
        .oneshot(make_req(
            Method::GET,
            "/api/decks/secret_deck",
            Some(&token_a),
            None,
            "ip-t3-a",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "Bの試行後もAのデッキは残存");

    let _ = std::fs::remove_dir_all(&dir);
}
