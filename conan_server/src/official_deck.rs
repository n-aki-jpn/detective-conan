use crate::handlers::AuthUser;
use crate::models::DeckCardItem;
use crate::room::SharedState;
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishDeckRequest {
    #[serde(default)]
    partner_id: Option<String>,
    #[serde(default)]
    case_id: Option<String>,
    #[serde(default)]
    main_deck: Option<Vec<DeckCardItem>>,

    #[serde(default)]
    partner: Option<Vec<String>>,
    #[serde(default)]
    incident: Option<Vec<String>>,
    #[serde(default)]
    other: Option<Vec<String>>,

    #[serde(default)]
    color: Option<String>,
}

pub fn resolve_color_code(colors: &[String]) -> String {
    let mut code = String::new();
    for col in colors {
        for ch in col.chars() {
            let letter = match ch {
                '青' => 'B',
                '緑' => 'G',
                '白' => 'W',
                '赤' => 'R',
                '黄' => 'Y',
                '黒' => 'K',
                _ => continue,
            };
            if !code.contains(letter) {
                code.push(letter);
            }
        }
    }
    if code.is_empty() {
        "B".to_string()
    } else {
        code
    }
}

#[derive(Deserialize)]
pub struct ImportOfficialDeckRequest {
    code: String,
}

fn decode_html_entities(s: &str) -> String {
    s.replace("&quot;", "\"")
        .replace("&#34;", "\"")
        .replace("&#x22;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

fn extract_attr_value(tag: &str, attr_name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    for quote in ['"', '\''] {
        let pat = format!("{}={}", attr_name, quote);
        if let Some(pos) = lower.find(&pat) {
            let start = pos + pat.len();
            if let Some(end_rel) = tag[start..].find(quote) {
                return Some(decode_html_entities(&tag[start..start + end_rel]));
            }
        }
    }
    None
}

pub fn parse_official_deck_html(html: &str) -> Result<serde_json::Value, String> {
    let mut deck_values = Vec::new();
    let mut search_from = 0;
    while let Some(rel_start) = html[search_from..].find('<') {
        let tag_start = search_from + rel_start;
        let Some(rel_end) = html[tag_start..].find('>') else {
            break;
        };
        let tag = &html[tag_start..tag_start + rel_end + 1];
        search_from = tag_start + rel_end + 1;

        let tag_lower = tag.to_ascii_lowercase();
        if tag_lower.starts_with("<input") {
            if let Some(name) = extract_attr_value(tag, "name") {
                if name == "deck" {
                    if let Some(val) = extract_attr_value(tag, "value") {
                        deck_values.push(val);
                    }
                }
            }
        }
    }

    if deck_values.is_empty() {
        return Err("HTML内に <input name=\"deck\" value=\"...\"> がありません。コードが無効、またはページ構造が変わった可能性があります。".to_string());
    }

    let mut parsed_decks = Vec::new();
    for val in deck_values {
        let deck: serde_json::Value = serde_json::from_str(&val)
            .map_err(|e| format!("deck属性の値をJSONとして解析できませんでした: {}", e))?;
        let obj = deck
            .as_object()
            .ok_or_else(|| "deckがJSONオブジェクトではありません。".to_string())?;
        let mut normalized = serde_json::Map::new();
        for key in ["partner", "incident", "other"] {
            let arr = obj
                .get(key)
                .and_then(|v| v.as_array())
                .ok_or_else(|| format!("デッキ情報に '{}' がないか、配列ではありません。", key))?;
            if arr.iter().any(|c| c.as_str().map(|s| s.is_empty()).unwrap_or(true)) {
                return Err(format!("'{}' に不正なカード番号が含まれています。", key));
            }
            normalized.insert(key.to_string(), serde_json::Value::Array(arr.clone()));
        }
        parsed_decks.push(serde_json::Value::Object(normalized));
    }

    if parsed_decks.iter().any(|d| d != &parsed_decks[0]) {
        return Err("異なる内容のdeck属性が複数見つかりました。HTMLの確認が必要です。".to_string());
    }

    Ok(parsed_decks.into_iter().next().unwrap())
}

pub async fn handle_import_official_deck(
    State(_state): State<SharedState>,
    _user: AuthUser,
    Json(body): Json<ImportOfficialDeckRequest>,
) -> impl IntoResponse {
    let code = body.code.trim();
    if code.len() != 9 || !code.chars().all(|c| c.is_ascii_alphanumeric()) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "公式コードは9文字の英数字で入力してください。"})),
        )
            .into_response();
    }

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
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": e})),
            )
                .into_response();
        }
    };

    let partner_id = deck["partner"]
        .as_array()
        .and_then(|a| a.first())
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let case_id = deck["incident"]
        .as_array()
        .and_then(|a| a.first())
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let mut counts: Vec<(String, usize)> = Vec::new();
    if let Some(other) = deck["other"].as_array() {
        for item in other {
            if let Some(cid) = item.as_str() {
                if let Some(existing) = counts.iter_mut().find(|(id, _)| id == cid) {
                    existing.1 += 1;
                } else {
                    counts.push((cid.to_string(), 1));
                }
            }
        }
    }
    let main_deck: Vec<serde_json::Value> = counts
        .into_iter()
        .map(|(definition_id, count)| json!({"definitionId": definition_id, "count": count}))
        .collect();

    (
        StatusCode::OK,
        Json(json!({
            "code": code,
            "partnerId": partner_id,
            "caseId": case_id,
            "mainDeck": main_deck,
            "raw": deck
        })),
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
            if c.is_empty() { "B".to_string() } else { c }
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
        ).into_response();
    }
    if incident_ids.is_empty() || incident_ids[0].is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "事件カードが選択されていません"})),
        ).into_response();
    }
    if other_ids.len() != 40 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": format!("メインデッキは40枚である必要があります（現在: {}枚）", other_ids.len())})),
        ).into_response();
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
            ).into_response();
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
            ).into_response();
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
        let base_url = match url::Url::parse(endpoint) {
            Ok(u) => u,
            Err(_) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": "URL解析エラー"})),
                ).into_response();
            }
        };

        if let Ok(joined) = base_url.join(loc) {
            if let Some((_, code)) = joined.query_pairs().find(|(k, _)| k == "d_code") {
                d_code = Some(code.to_string());
            }
            full_location = Some(joined.to_string());
        } else {
            full_location = Some(loc.clone());
        }
    }

    Json(json!({
        "status": status,
        "location": full_location,
        "code": d_code,
        "success": d_code.is_some()
    })).into_response()
}

