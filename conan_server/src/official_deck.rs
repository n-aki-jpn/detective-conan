use crate::handlers::AuthUser;
pub use crate::official_deck_parser::{parse_official_deck_html, resolve_color_code};
use crate::official_deck_parser::{
    build_imported_deck, extract_d_code_from_location, extract_official_code,
    imported_deck_response_json, ImportOfficialDeckRequest, PublishDeckRequest,
};
use crate::room::SharedState;
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde_json::json;

pub async fn handle_import_official_deck(
    State(_state): State<SharedState>,
    _user: AuthUser,
    Json(body): Json<ImportOfficialDeckRequest>,
) -> impl IntoResponse {
    let raw_input = body.code.as_deref().or(body.url.as_deref()).unwrap_or("");
    let code = match extract_official_code(raw_input) {
        Ok(c) => c,
        Err(msg) => {
            return (StatusCode::BAD_REQUEST, Json(json!({ "error": msg }))).into_response();
        }
    };

    let url = format!(
        "https://www.takaratomy.co.jp/products/conan-cardgame/deckrecipe/create?d_code={}",
        code
    );
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("HTTPクライアント初期化エラー: {}", e)})),
            )
                .into_response();
        }
    };

    let resp = match client
        .get(&url)
        .header("Accept", "text/html,application/xhtml+xml")
        .header("Accept-Language", "ja")
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({"error": format!("公式サイトへの通信に失敗しました: {}", e)})),
            )
                .into_response();
        }
    };

    let html = match resp.text().await {
        Ok(t) => t,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({"error": format!("HTMLの読み取りに失敗しました: {}", e)})),
            )
                .into_response();
        }
    };

    let deck = match parse_official_deck_html(&html) {
        Ok(d) => d,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response();
        }
    };

    let imported = build_imported_deck(deck);
    (
        StatusCode::OK,
        Json(imported_deck_response_json(
            &code, &imported, false, None, None,
        )),
    )
        .into_response()
}

pub async fn handle_publish_deck(
    State(state): State<SharedState>,
    _user: AuthUser,
    Json(body): Json<PublishDeckRequest>,
) -> impl IntoResponse {
    let (partner_ids, incident_ids, other_ids, color_code) = {
        let mgr = state.lock().unwrap();

        let p_id = body
            .partner
            .and_then(|p| p.into_iter().next())
            .or(body.partner_id)
            .unwrap_or_default();

        let inc_id = body
            .incident
            .and_then(|i| i.into_iter().next())
            .or(body.case_id)
            .unwrap_or_default();

        let mut o_ids = Vec::new();
        if let Some(other) = body.other {
            o_ids = other;
        } else if let Some(items) = body.main_deck {
            for item in items {
                for _ in 0..item.count {
                    o_ids.push(item.definition_id.clone());
                }
            }
        }

        let col = if let Some(c) = body.color {
            if c.is_empty() {
                "B".to_string()
            } else {
                c
            }
        } else if let Some(p_card) = mgr.catalog.get(&p_id) {
            resolve_color_code(&p_card.colors)
        } else {
            "B".to_string()
        };

        (vec![p_id], vec![inc_id], o_ids, col)
    };

    if partner_ids.is_empty() || partner_ids[0].is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "パートナーカードが選択されていません"})),
        )
            .into_response();
    }
    if incident_ids.is_empty() || incident_ids[0].is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "事件カードが選択されていません"})),
        )
            .into_response();
    }
    if other_ids.len() != 40 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": format!("メインデッキは40枚である必要があります（現在: {}枚）", other_ids.len())})),
        )
            .into_response();
    }

    let payload = json!({
        "partner": partner_ids,
        "incident": incident_ids,
        "other": other_ids,
    });

    let deck_json_str = serde_json::to_string(&payload).unwrap_or_default();
    let endpoint = "https://www.takaratomy.co.jp/products/conan-cardgame/deckrecipe/store";

    let client = match reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64)")
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("HTTPクライアント初期化エラー: {}", e)})),
            )
                .into_response();
        }
    };

    let response = match client
        .get(endpoint)
        .query(&[("color", &color_code), ("deck", &deck_json_str)])
        .send()
        .await
    {
        Ok(res) => res,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({"error": format!("公式サイトへの接続に失敗しました: {}", e)})),
            )
                .into_response();
        }
    };

    let status = response.status().as_u16();
    let location_header = response
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    let mut full_location = None;
    let mut d_code = None;

    if let Some(loc) = &location_header {
        match extract_d_code_from_location(endpoint, loc) {
            Ok((full, code)) => {
                full_location = Some(full);
                d_code = code;
            }
            Err(msg) => {
                return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": msg })))
                    .into_response();
            }
        }
    }

    Json(json!({
        "status": status,
        "location": full_location,
        "code": d_code,
        "success": d_code.is_some()
    }))
    .into_response()
}
