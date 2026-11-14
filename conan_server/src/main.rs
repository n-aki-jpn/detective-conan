use axum::{
    extract::{Path as AxumPath, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tower_http::services::ServeDir;
use tracing_subscriber;

mod agent;
mod catalog;
mod engine;
mod models;
mod observation;

use agent::RandomAgent;
use catalog::CardCatalog;
use engine::MatchEngine;
use models::{DeckCardItem, DeckDefinition, MatchPhase};
use observation::build_player_observation;

// ─── Room / Session Management ───────────────────────────────────────────────

struct MatchRoom {
    room_id: String,
    engine: MatchEngine,
    p2_is_cpu: bool,
    cpu_agent: Option<RandomAgent>,
    p1_deck: DeckDefinition,
    p2_deck: Option<DeckDefinition>,
    p1_connected: bool,
    p2_connected: bool,
    created_at: std::time::Instant,
}

impl MatchRoom {
    fn new(room_id: &str, catalog: CardCatalog, p1_deck: DeckDefinition, is_cpu: bool, default_p2_deck: DeckDefinition) -> Self {
        let mut engine = MatchEngine::new(catalog.clone());
        let cpu_agent = if is_cpu { Some(RandomAgent::new("player2")) } else { None };

        let mut room = Self {
            room_id: room_id.to_string(),
            engine,
            p2_is_cpu: is_cpu,
            cpu_agent,
            p1_deck: p1_deck.clone(),
            p2_deck: if is_cpu { Some(default_p2_deck.clone()) } else { None },
            p1_connected: true,
            p2_connected: is_cpu,
            created_at: std::time::Instant::now(),
        };

        // 両プレイヤーの状態を初期化（クライアントでの観戦・表示用）
        room.engine.add_player("player1", &p1_deck);
        let p2_init_deck = if is_cpu { default_p2_deck } else { p1_deck.clone() };
        room.engine.add_player("player2", &p2_init_deck);

        if is_cpu {
            room.engine.start_match();
            room.process_cpu_if_needed();
        } else {
            // 対人オンライン部屋はP2参加（join_p2）まで待機フェーズ
            room.engine.phase = MatchPhase::WAITING;
        }

        room
    }

    fn join_p2(&mut self, catalog: &CardCatalog, p2_deck: DeckDefinition) {
        self.p2_deck = Some(p2_deck.clone());
        self.p2_connected = true;
        self.p2_is_cpu = false;
        self.cpu_agent = None;

        // P2参加と同時に、新鮮な両者デッキで試合開始！
        let mut engine = MatchEngine::new(catalog.clone());
        engine.add_player("player1", &self.p1_deck);
        engine.add_player("player2", &p2_deck);
        engine.start_match();
        self.engine = engine;
    }

    fn reset_match(&mut self, catalog: &CardCatalog) {
        let mut engine = MatchEngine::new(catalog.clone());
        engine.add_player("player1", &self.p1_deck);
        let p2_deck = self.p2_deck.as_ref().unwrap_or(&self.p1_deck);
        engine.add_player("player2", p2_deck);
        engine.start_match();
        self.engine = engine;
        self.process_cpu_if_needed();
    }

    fn process_cpu_if_needed(&mut self) {
        let max_steps = 50;
        let mut steps = 0;
        loop {
            if steps >= max_steps || !self.p2_is_cpu {
                break;
            }
            match &self.engine.pending_decision {
                Some(dec) => {
                    if dec.get("playerId").and_then(|v| v.as_str()) != Some("player2") {
                        break;
                    }
                    let ans = match &self.cpu_agent {
                        Some(agent) => agent.choose_action_with_engine(&self.engine, dec),
                        None => break,
                    };
                    if self.engine.submit_decision("player2", &ans).is_err() {
                        break;
                    }
                }
                None => break,
            }
            steps += 1;
        }
    }
}

// ─── Match Manager ────────────────────────────────────────────────────────────

struct MatchManager {
    deck_path: PathBuf,
    decks_dir: PathBuf,
    catalog: CardCatalog,
    deck_def: DeckDefinition,
    rooms: HashMap<String, MatchRoom>,
}

fn sanitize_deck(sent_deck: Option<DeckDefinition>, fallback_deck: &DeckDefinition) -> DeckDefinition {
    if let Some(d) = sent_deck {
        let count: i32 = d.main_deck.iter().map(|item| item.count).sum();
        if count == 40 && !d.partner_id.is_empty() && !d.case_id.is_empty() {
            return d;
        }
    }
    fallback_deck.clone()
}

impl MatchManager {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let catalog_path = PathBuf::from("data/dummy/cards.json");
        let deck_path = PathBuf::from("data/dummy/deck.json");
        let decks_dir = PathBuf::from("data/dummy/decks");

        if !decks_dir.exists() {
            let _ = std::fs::create_dir_all(&decks_dir);
        }

        let catalog = CardCatalog::load_from_json(&catalog_path)?;
        let deck_def = Self::load_deck(&deck_path)?;

        let mut mgr = MatchManager {
            deck_path,
            decks_dir,
            catalog: catalog.clone(),
            deck_def: deck_def.clone(),
            rooms: HashMap::new(),
        };

        // Create initial default local room
        let default_room = MatchRoom::new("default", catalog, deck_def.clone(), true, deck_def);
        mgr.rooms.insert("default".to_string(), default_room);

        Ok(mgr)
    }

    fn load_deck(path: &PathBuf) -> Result<DeckDefinition, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let deck: DeckDefinition = serde_json::from_str(&content)?;
        Ok(deck)
    }

    fn save_deck(&mut self, new_def: DeckDefinition) -> Result<(), String> {
        // クライアント側（localStorage）保存に移行したため、サーバー側ファイルには書き込まずメモリのみ保持
        self.deck_def = new_def;
        Ok(())
    }

    fn list_decks(&self) -> Vec<Value> {
        let mut list = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.decks_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("json") {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        if let Ok(val) = serde_json::from_str::<Value>(&content) {
                            let file_stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
                            let deck_name = val.get("deckName").and_then(|v| v.as_str()).unwrap_or(&file_stem).to_string();
                            let partner_id = val.get("partnerId").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let case_id = val.get("caseId").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let total_cards: i64 = val.get("mainDeck")
                                .and_then(|v| v.as_array())
                                .map(|arr| arr.iter().filter_map(|item| item.get("count").and_then(|c| c.as_i64())).sum())
                                .unwrap_or(0);

                            list.push(json!({
                                "fileId": file_stem,
                                "deckId": val.get("deckId").and_then(|v| v.as_str()).unwrap_or(&file_stem),
                                "deckName": deck_name,
                                "partnerId": partner_id,
                                "caseId": case_id,
                                "totalCards": total_cards,
                                "isActive": self.deck_def.deck_id.as_deref() == val.get("deckId").and_then(|v| v.as_str())
                            }));
                        }
                    }
                }
            }
        }
        list
    }

    fn create_room(&mut self, is_cpu: bool, p1_deck: Option<DeckDefinition>) -> String {
        let mut rng = rand::thread_rng();
        let code: u32 = rng.gen_range(1000..9999);
        let room_id = format!("{}", code);

        let deck = sanitize_deck(p1_deck, &self.deck_def);
        let room = MatchRoom::new(&room_id, self.catalog.clone(), deck, is_cpu, self.deck_def.clone());
        self.rooms.insert(room_id.clone(), room);
        room_id
    }
}

type SharedState = Arc<Mutex<MatchManager>>;

// ─── Request / Response Types ──────────────────────────────────────────────────

#[derive(Deserialize)]
struct StateQuery {
    player_id: Option<String>,
    room_id: Option<String>,
}

#[derive(Deserialize)]
struct SetCpuRequest {
    #[serde(rename = "isCpu")]
    is_cpu: bool,
    #[serde(rename = "roomId")]
    room_id: Option<String>,
}

#[derive(Deserialize)]
struct DecisionRequest {
    #[serde(rename = "playerId")]
    player_id: String,
    #[serde(rename = "roomId")]
    room_id: Option<String>,
    value: Option<Value>,
}

#[derive(Deserialize)]
struct ResetRequest {
    #[serde(rename = "roomId")]
    room_id: Option<String>,
    #[serde(rename = "p1DeckId")]
    p1_deck_id: Option<String>,
    #[serde(rename = "p2DeckId")]
    p2_deck_id: Option<String>,
    #[serde(rename = "p1Deck")]
    p1_deck: Option<DeckDefinition>,
    #[serde(rename = "p2Deck")]
    p2_deck: Option<DeckDefinition>,
}

#[derive(Deserialize)]
struct CreateRoomRequest {
    #[serde(default)]
    is_cpu: bool,
    deck: Option<DeckDefinition>,
}

#[derive(Deserialize)]
struct JoinRoomRequest {
    #[serde(rename = "roomId")]
    room_id: String,
    deck: Option<DeckDefinition>,
}

#[derive(Serialize)]
struct StatusOk {
    status: &'static str,
}

// ─── Handlers ─────────────────────────────────────────────────────────────────

async fn handle_state(
    State(state): State<SharedState>,
    Query(params): Query<StateQuery>,
) -> impl IntoResponse {
    let mut mgr = state.lock().unwrap();

    let room_id = params.room_id.unwrap_or_else(|| "default".to_string());
    let room = match mgr.rooms.get_mut(&room_id) {
        Some(r) => r,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "Room not found", "roomId": room_id})),
            ).into_response();
        }
    };

    room.process_cpu_if_needed();

    let player_id = params
        .player_id
        .filter(|id| room.engine.players.contains_key(id.as_str()))
        .unwrap_or_else(|| "player1".to_string());

    let mut obs = build_player_observation(&room.engine, &player_id);
    obs["p2IsCpu"] = json!(room.p2_is_cpu);
    obs["roomId"] = json!(room.room_id);
    obs["p1Connected"] = json!(room.p1_connected);
    obs["p2Connected"] = json!(room.p2_connected);
    obs["waitingForPlayer2"] = json!(!room.p2_is_cpu && !room.p2_connected);

    Json(obs).into_response()
}

async fn handle_create_room(
    State(state): State<SharedState>,
    Json(body): Json<CreateRoomRequest>,
) -> impl IntoResponse {
    let mut mgr = state.lock().unwrap();
    let room_id = mgr.create_room(body.is_cpu, body.deck);
    Json(json!({
        "status": "ok",
        "roomId": room_id,
        "assignedPlayerId": "player1"
    })).into_response()
}

async fn handle_join_room(
    State(state): State<SharedState>,
    Json(body): Json<JoinRoomRequest>,
) -> impl IntoResponse {
    let mut mgr = state.lock().unwrap();
    let room_id = body.room_id.trim();

    let default_deck = mgr.deck_def.clone();
    let catalog = mgr.catalog.clone();

    if let Some(room) = mgr.rooms.get_mut(room_id) {
        if room.p2_connected && !room.p2_is_cpu {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "ルームは既に満員です"})),
            ).into_response();
        }

        let p2_deck = sanitize_deck(body.deck, &default_deck);
        room.join_p2(&catalog, p2_deck);

        Json(json!({
            "status": "ok",
            "roomId": room_id,
            "assignedPlayerId": "player2"
        })).into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": format!("ルーム番号「{}」が見つかりません", room_id)})),
        ).into_response()
    }
}

async fn handle_catalog(State(state): State<SharedState>) -> impl IntoResponse {
    let mgr = state.lock().unwrap();
    let total = mgr.catalog.definitions.len();
    let cats = mgr.catalog.raw_json.clone();

    let mut resp = cats.clone();
    resp["totalCards"] = json!(total);

    Json(resp).into_response()
}

async fn handle_deck(State(state): State<SharedState>) -> impl IntoResponse {
    let mgr = state.lock().unwrap();
    match std::fs::read_to_string(&mgr.deck_path) {
        Ok(content) => {
            let val: Value = serde_json::from_str(&content).unwrap_or(json!({}));
            Json(val).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        ).into_response(),
    }
}

async fn handle_list_decks(State(state): State<SharedState>) -> impl IntoResponse {
    let mgr = state.lock().unwrap();
    let list = mgr.list_decks();
    Json(json!({ "decks": list })).into_response()
}

async fn handle_get_deck_by_id(
    State(state): State<SharedState>,
    AxumPath(deck_id): AxumPath<String>,
) -> impl IntoResponse {
    let mgr = state.lock().unwrap();
    let safe_id: String = deck_id
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let target = mgr.decks_dir.join(format!("{}.json", safe_id));
    if let Ok(content) = std::fs::read_to_string(target) {
        if let Ok(val) = serde_json::from_str::<Value>(&content) {
            return Json(val).into_response();
        }
    }
    (StatusCode::NOT_FOUND, Json(json!({"error": "Deck not found"}))).into_response()
}

async fn handle_delete_deck_by_id(
    State(state): State<SharedState>,
    AxumPath(deck_id): AxumPath<String>,
) -> impl IntoResponse {
    let mgr = state.lock().unwrap();
    let safe_id: String = deck_id
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let target = mgr.decks_dir.join(format!("{}.json", safe_id));
    if target.exists() {
        let _ = std::fs::remove_file(target);
        return Json(StatusOk { status: "ok" }).into_response();
    }
    (StatusCode::NOT_FOUND, Json(json!({"error": "Deck not found"}))).into_response()
}

async fn handle_select_deck(
    State(state): State<SharedState>,
    AxumPath(deck_id): AxumPath<String>,
) -> impl IntoResponse {
    let mut mgr = state.lock().unwrap();
    let safe_id: String = deck_id
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let target = mgr.decks_dir.join(format!("{}.json", safe_id));
    if let Ok(content) = std::fs::read_to_string(&target) {
        if let Ok(deck_def) = serde_json::from_str::<DeckDefinition>(&content) {
            let _ = std::fs::write(&mgr.deck_path, &content);
            mgr.deck_def = deck_def.clone();
            let catalog = mgr.catalog.clone();
            if let Some(default_room) = mgr.rooms.get_mut("default") {
                default_room.p1_deck = deck_def;
                default_room.reset_match(&catalog);
            }
            return Json(StatusOk { status: "ok" }).into_response();
        }
    }
    (StatusCode::BAD_REQUEST, Json(json!({"error": "Failed to load deck"}))).into_response()
}

fn load_deck_from_dir(decks_dir: &std::path::Path, id: &str) -> Option<DeckDefinition> {
    let safe_id: String = id
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    if safe_id.is_empty() {
        return None;
    }
    let target = decks_dir.join(format!("{}.json", safe_id));
    let content = std::fs::read_to_string(&target).ok()?;
    serde_json::from_str::<DeckDefinition>(&content).ok()
}

async fn handle_reset(
    State(state): State<SharedState>,
    Json(body): Json<Option<ResetRequest>>,
) -> impl IntoResponse {
    let mut mgr = state.lock().unwrap();
    let (room_id, p1_deck_id, p2_deck_id, p1_deck_body, p2_deck_body) = match body {
        Some(b) => (
            b.room_id.unwrap_or_else(|| "default".to_string()),
            b.p1_deck_id,
            b.p2_deck_id,
            b.p1_deck,
            b.p2_deck,
        ),
        None => ("default".to_string(), None, None, None, None),
    };

    let p1_loaded = p1_deck_body.or_else(|| {
        p1_deck_id
            .as_deref()
            .filter(|s| !s.is_empty())
            .and_then(|id| load_deck_from_dir(&mgr.decks_dir, id))
    });
    let p2_loaded = p2_deck_body.or_else(|| {
        p2_deck_id
            .as_deref()
            .filter(|s| !s.is_empty())
            .and_then(|id| load_deck_from_dir(&mgr.decks_dir, id))
    });

    let catalog = mgr.catalog.clone();
    if let Some(room) = mgr.rooms.get_mut(&room_id) {
        if let Some(d1) = p1_loaded {
            room.p1_deck = d1;
        }
        if let Some(d2) = p2_loaded {
            room.p2_deck = Some(d2);
        }
        room.reset_match(&catalog);
        Json(StatusOk { status: "ok" }).into_response()
    } else {
        (StatusCode::NOT_FOUND, Json(json!({"error": "Room not found"}))).into_response()
    }
}

async fn handle_set_cpu(
    State(state): State<SharedState>,
    Json(body): Json<SetCpuRequest>,
) -> impl IntoResponse {
    let mut mgr = state.lock().unwrap();
    let room_id = body.room_id.unwrap_or_else(|| "default".to_string());
    if let Some(room) = mgr.rooms.get_mut(&room_id) {
        room.p2_is_cpu = body.is_cpu;
        room.cpu_agent = if body.is_cpu {
            Some(RandomAgent::new("player2"))
        } else {
            None
        };
        room.process_cpu_if_needed();
        Json(StatusOk { status: "ok" }).into_response()
    } else {
        (StatusCode::NOT_FOUND, Json(json!({"error": "Room not found"}))).into_response()
    }
}

async fn handle_save_deck(
    State(state): State<SharedState>,
    Json(body): Json<DeckDefinition>,
) -> impl IntoResponse {
    let mgr_read = state.lock().unwrap();
    let total: i32 = body.main_deck.iter().map(|item| item.count).sum();
    if total != 40 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": format!("メインデッキはちょうど40枚である必要があります（現在: {}枚）", total)})),
        ).into_response();
    }

    // Validate maximum 3 copies per canonical ID (card_id)
    let mut canonical_counts: HashMap<String, i32> = HashMap::new();
    for item in &body.main_deck {
        let canonical_id = mgr_read
            .catalog
            .get(&item.definition_id)
            .map(|d| d.canonical_id.clone())
            .unwrap_or_else(|| item.definition_id.clone());

        let count = canonical_counts.entry(canonical_id.clone()).or_insert(0);
        *count += item.count;
        if *count > 3 {
            let card_title = mgr_read
                .catalog
                .get(&item.definition_id)
                .and_then(|d| d.names.first())
                .cloned()
                .unwrap_or_else(|| item.definition_id.clone());
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": format!("同一カードID（{} / {}）はパラレル版も含めて合計3枚までです（現在: {}枚）", canonical_id, card_title, *count)})),
            ).into_response();
        }
    }
    drop(mgr_read);

    if body.partner_id.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "パートナーカードが選択されていません"})),
        ).into_response();
    }
    if body.case_id.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "事件カードが選択されていません"})),
        ).into_response();
    }

    let mut mgr = state.lock().unwrap();
    let deck_clone = body.clone();
    if let Err(e) = mgr.save_deck(body) {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": e}))).into_response();
    }
    let catalog = mgr.catalog.clone();
    if let Some(default_room) = mgr.rooms.get_mut("default") {
        default_room.p1_deck = deck_clone;
        default_room.reset_match(&catalog);
    }
    Json(StatusOk { status: "ok" }).into_response()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublishDeckRequest {
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

fn resolve_color_code(colors: &[String]) -> String {
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

async fn handle_publish_deck(
    State(state): State<SharedState>,
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

async fn handle_decision(
    State(state): State<SharedState>,
    Json(body): Json<DecisionRequest>,
) -> impl IntoResponse {
    let mut mgr = state.lock().unwrap();
    let room_id = body.room_id.unwrap_or_else(|| "default".to_string());

    if let Some(room) = mgr.rooms.get_mut(&room_id) {
        let ans = json!({ "value": body.value.unwrap_or(json!({})) });
        match room.engine.submit_decision(&body.player_id, &ans) {
            Ok(_) => {
                room.process_cpu_if_needed();
                Json(StatusOk { status: "ok" }).into_response()
            }
            Err(e) => (StatusCode::BAD_REQUEST, Json(json!({"error": e}))).into_response(),
        }
    } else {
        (StatusCode::NOT_FOUND, Json(json!({"error": "Room not found"}))).into_response()
    }
}

// ─── Main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let mgr = MatchManager::new().expect("MatchManager 初期化に失敗しました");
    let shared_state: SharedState = Arc::new(Mutex::new(mgr));

    let static_dir = PathBuf::from("src/conan_engine/web/static");

    let app = Router::new()
        .route("/api/state", get(handle_state))
        .route("/api/catalog", get(handle_catalog))
        .route("/api/deck", get(handle_deck))
        .route("/api/decks", get(handle_list_decks))
        .route("/api/decks/:id", get(handle_get_deck_by_id))
        .route("/api/decks/:id", delete(handle_delete_deck_by_id))
        .route("/api/decks/:id/select", post(handle_select_deck))
        .route("/api/rooms/create", post(handle_create_room))
        .route("/api/rooms/join", post(handle_join_room))
        .route("/api/reset", post(handle_reset))
        .route("/api/set_cpu", post(handle_set_cpu))
        .route("/api/save_deck", post(handle_save_deck))
        .route("/api/deck/publish", post(handle_publish_deck))
        .route("/api/decision", post(handle_decision))
        .fallback_service(ServeDir::new(&static_dir))
        .with_state(shared_state);

    let port: u16 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(8000);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    println!("🚀 Detective Conan TCG Server (Rust) running at http://0.0.0.0:{port}");
    println!("📲 スマホから接続する場合: 同じWi-Fiに接続し http://<このPCのIPアドレス>:{port} をブラウザで開いてください");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use models::DeckCardItem;

    #[test]
    fn test_canonical_id_validation() {
        let path = if std::path::Path::new("data/dummy/cards.json").exists() {
            "data/dummy/cards.json"
        } else {
            "../data/dummy/cards.json"
        };
        let catalog = CardCatalog::load_from_json(path).expect("catalog load failed");
        
        let card1 = catalog.get("B01005").expect("B01005 should exist");
        let card2 = catalog.get("B01005P").expect("B01005P should exist");
        assert_eq!(card1.canonical_id, "0001");
        assert_eq!(card2.canonical_id, "0001");

        // Helper to validate deck items
        let validate_deck_counts = |items: &[DeckCardItem]| -> Result<(), String> {
            let mut canonical_counts: HashMap<String, i32> = HashMap::new();
            for item in items {
                let canonical_id = catalog
                    .get(&item.definition_id)
                    .map(|d| d.canonical_id.clone())
                    .unwrap_or_else(|| item.definition_id.clone());

                let count = canonical_counts.entry(canonical_id.clone()).or_insert(0);
                *count += item.count;
                if *count > 3 {
                    return Err(format!("同一カードID（{}）はパラレル版も含めて合計3枚までです（現在: {}枚）", canonical_id, *count));
                }
            }
            Ok(())
        };

        // 2 + 2 = 4 (Same canonical ID) -> should fail
        let invalid_items = vec![
            DeckCardItem { definition_id: "B01005".to_string(), count: 2 },
            DeckCardItem { definition_id: "B01005P".to_string(), count: 2 },
        ];
        assert!(validate_deck_counts(&invalid_items).is_err());

        // 2 + 1 = 3 (Same canonical ID) -> should succeed
        let valid_items = vec![
            DeckCardItem { definition_id: "B01005".to_string(), count: 2 },
            DeckCardItem { definition_id: "B01005P".to_string(), count: 1 },
            DeckCardItem { definition_id: "B01006".to_string(), count: 3 },
        ];
        assert!(validate_deck_counts(&valid_items).is_ok());
    }

    #[test]
    fn test_color_matching_hand_play() {
        let path = if std::path::Path::new("data/dummy/cards.json").exists() {
            "data/dummy/cards.json"
        } else {
            "../data/dummy/cards.json"
        };
        let catalog = CardCatalog::load_from_json(path).expect("catalog load failed");

        // Verify case card B05120P (all colors or 5 colors)
        if let Some(case_card) = catalog.get("B05120P") {
            assert!(case_card.colors.len() >= 5 || case_card.colors.contains(&"青".to_string()));
        }

        // Verify character card B01005 (blue)
        let blue_card = catalog.get("B01005").expect("B01005 should exist");
        assert!(blue_card.colors.contains(&"青".to_string()));
    }

    #[test]
    fn test_color_code_resolution() {
        assert_eq!(resolve_color_code(&["青".to_string()]), "B");
        assert_eq!(resolve_color_code(&["緑".to_string()]), "G");
        assert_eq!(resolve_color_code(&["白".to_string()]), "W");
        assert_eq!(resolve_color_code(&["赤".to_string()]), "R");
        assert_eq!(resolve_color_code(&["黄".to_string()]), "Y");
        assert_eq!(resolve_color_code(&["黒".to_string()]), "K");
        assert_eq!(resolve_color_code(&["青".to_string(), "緑".to_string()]), "BG");
    }

    #[test]
    fn test_effect_condition_and_cost_checking() {
        let path = if std::path::Path::new("data/dummy/cards.json").exists() {
            "data/dummy/cards.json"
        } else {
            "../data/dummy/cards.json"
        };
        let catalog = CardCatalog::load_from_json(path).expect("catalog load failed");
        let mut engine = MatchEngine::new(catalog);

        // Setup mock player with Blue partner and 4 files
        let mut p1 = models::CardInstance {
            instance_id: "c1".to_string(),
            definition_id: "B01001".to_string(), // Partner Conan (Blue)
            owner_id: "p1".to_string(),
            zone: models::Zone::PARTNER,
            orientation: models::CardOrientation::ACTIVE,
            entered_turn: 0,
            face_up: true,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
        };
        let mut ps = engine::PlayerState::new("p1".to_string());
        ps.partner = Some(p1);
        for i in 0..4 {
            ps.file.push(models::CardInstance {
                instance_id: format!("f{}", i),
                definition_id: "B01005".to_string(),
                owner_id: "p1".to_string(),
                zone: models::Zone::FILE,
                orientation: models::CardOrientation::ACTIVE,
                entered_turn: 0,
                face_up: false,
                temp_ap_bonus: 0,
                has_rush: false,
                rush_target: None,
                used_declare_this_turn: false,
            });
        }
        engine.players.insert("p1".to_string(), ps);

        // Partner color condition check
        assert!(engine.check_effect_conditions("p1", "【パートナー青】効果テキスト"));
        assert!(!engine.check_effect_conditions("p1", "【パートナー赤】効果テキスト"));

        // FILE condition check (currently 4 files)
        assert!(engine.check_effect_conditions("p1", "【FILE4】効果テキスト"));
        assert!(!engine.check_effect_conditions("p1", "【FILE5】効果テキスト"));

        // Add 1 more file -> now 5 files
        engine.players.get_mut("p1").unwrap().file.push(models::CardInstance {
            instance_id: "f4".to_string(),
            definition_id: "B01005".to_string(),
            owner_id: "p1".to_string(),
            zone: models::Zone::FILE,
            orientation: models::CardOrientation::ACTIVE,
            entered_turn: 0,
            face_up: false,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
        });
        assert!(engine.check_effect_conditions("p1", "【FILE5】効果テキスト"));
    }

    #[test]
    fn test_on_remove_and_turn_end_triggers() {
        let path = if std::path::Path::new("data/dummy/cards.json").exists() {
            "data/dummy/cards.json"
        } else {
            "../data/dummy/cards.json"
        };
        let catalog = CardCatalog::load_from_json(path).expect("catalog load failed");
        let mut engine = MatchEngine::new(catalog);

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let mut ps2 = engine::PlayerState::new("p2".to_string());

        // Give p1 a card in deck
        ps1.deck.push(models::CardInstance {
            instance_id: "d1".to_string(),
            definition_id: "B01005".to_string(),
            owner_id: "p1".to_string(),
            zone: models::Zone::DECK,
            orientation: models::CardOrientation::ACTIVE,
            entered_turn: 0,
            face_up: false,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
        });

        // Field card with turn end trigger
        ps1.field.push(models::CardInstance {
            instance_id: "f1".to_string(),
            definition_id: "B02088".to_string(), // 自分か相手のターン終了時、自分の証拠を上から1つリムーブする
            owner_id: "p1".to_string(),
            zone: models::Zone::FIELD,
            orientation: models::CardOrientation::ACTIVE,
            entered_turn: 0,
            face_up: true,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
        });
        // 1 evidence
        ps1.evidence.push(models::CardInstance {
            instance_id: "ev1".to_string(),
            definition_id: "B01005".to_string(),
            owner_id: "p1".to_string(),
            zone: models::Zone::EVIDENCE,
            orientation: models::CardOrientation::ACTIVE,
            entered_turn: 0,
            face_up: true,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
        });

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);
        engine.turn_player_id = "p2".to_string(); // p2's turn, so p1 is in opponent's turn

        // Test on remove trigger on card D03013 (【相手ターン中】【現場リムーブ時】カードを1枚引く。)
        assert_eq!(engine.players.get("p1").unwrap().hand.len(), 0);
        engine.trigger_on_remove("p1", "D03013");
        assert_eq!(engine.players.get("p1").unwrap().hand.len(), 1);

        // Test turn end trigger on B02088
        assert_eq!(engine.players.get("p1").unwrap().evidence.len(), 1);
        engine.trigger_on_turn_end("p1");
        assert_eq!(engine.players.get("p1").unwrap().evidence.len(), 0);
        assert_eq!(engine.players.get("p1").unwrap().remove.len(), 1);
    }

    #[test]
    fn test_trait_condition_rush_restriction_and_contact() {
        let path = if std::path::Path::new("data/dummy/cards.json").exists() {
            "data/dummy/cards.json"
        } else {
            "../data/dummy/cards.json"
        };
        let mut catalog = CardCatalog::load_from_json(path).expect("catalog load failed");

        // Insert synthetic test definitions for deterministic verification
        catalog.definitions.insert(
            "TEST_TRAIT_DETECTIVE".to_string(),
            models::CardDefinition {
                definition_id: "TEST_TRAIT_DETECTIVE".to_string(),
                canonical_id: "TEST_TRAIT_DETECTIVE".to_string(),
                card_type: models::CardType::CHARACTER,
                colors: vec!["青".to_string()],
                level: 2,
                ap: 3000,
                lp: 1,
                names: vec!["テスト探偵団".to_string()],
                description: "このキャラがコンタクトしたとき、AP＋1000する。\n相手の現場にいるキャラがこのキャラとのコンタクトによってリムーブされたとき、カードを1枚引く。".to_string(),
                traits: vec!["少年探偵団".to_string()],
                case_levels: None,
                main_thumb: None,
                main_path: None,
            },
        );

        let mut engine = MatchEngine::new(catalog);
        engine.turn = 3;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let mut ps2 = engine::PlayerState::new("p2".to_string());

        // Initially no 少年探偵団 on p1's field
        ps1.deck.push(models::CardInstance {
            instance_id: "p1_d1".to_string(),
            definition_id: "B01005".to_string(),
            owner_id: "p1".to_string(),
            zone: models::Zone::DECK,
            orientation: models::CardOrientation::ACTIVE,
            entered_turn: 0,
            face_up: false,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
        });

        // p2 has 1 SLEEP character, 1 case, and 1 evidence
        ps2.field.push(models::CardInstance {
            instance_id: "opp_c1".to_string(),
            definition_id: "B01005".to_string(),
            owner_id: "p2".to_string(),
            zone: models::Zone::FIELD,
            orientation: models::CardOrientation::SLEEP,
            entered_turn: 1,
            face_up: true,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
        });
        ps2.r#case = Some(models::CardInstance {
            instance_id: "opp_case".to_string(),
            definition_id: "B01002".to_string(),
            owner_id: "p2".to_string(),
            zone: models::Zone::CASE,
            orientation: models::CardOrientation::ACTIVE,
            entered_turn: 0,
            face_up: true,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
        });
        ps2.evidence.push(models::CardInstance {
            instance_id: "opp_ev1".to_string(),
            definition_id: "B01005".to_string(),
            owner_id: "p2".to_string(),
            zone: models::Zone::EVIDENCE,
            orientation: models::CardOrientation::ACTIVE,
            entered_turn: 0,
            face_up: false,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
        });

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        // 1. Trait condition check (before adding 少年探偵団)
        let trait_cond = "自分の現場に〚特徴［少年探偵団］〛のキャラがいる場合、カードを1枚引く。";
        assert!(!engine.check_effect_conditions("p1", trait_cond));

        // Add character with 少年探偵団 trait & 突撃［キャラ］
        engine.players.get_mut("p1").unwrap().field.push(models::CardInstance {
            instance_id: "p1_c1".to_string(),
            definition_id: "TEST_TRAIT_DETECTIVE".to_string(),
            owner_id: "p1".to_string(),
            zone: models::Zone::FIELD,
            orientation: models::CardOrientation::ACTIVE,
            entered_turn: 3, // entered this turn
            face_up: true,
            temp_ap_bonus: 0,
            has_rush: true,
            rush_target: Some("CHAR".to_string()),
            used_declare_this_turn: false,
        });
        assert!(engine.check_effect_conditions("p1", trait_cond));

        // 2. Rush［キャラ］ target restriction check via prompt_main_action
        engine.prompt_main_action("p1");
        let pd = engine.pending_decision.as_ref().unwrap();
        let actions = pd["options"]["actions"].as_array().unwrap();
        let action_opt = actions.iter().find(|a| a["type"] == "ACTION").unwrap();
        let valid_targets = action_opt["actorTargetMap"]["p1_c1"].as_array().unwrap();
        assert_eq!(valid_targets.len(), 1);
        assert_eq!(valid_targets[0], "opp_c1"); // Case "opp_case" must be excluded

        // 3. Contact trigger & contact win trigger
        engine.trigger_on_contact("p1", "p1_c1", "p2", "opp_c1");
        let bonus = engine.players.get("p1").unwrap().field[0].temp_ap_bonus;
        assert_eq!(bonus, 1000);

        assert_eq!(engine.players.get("p1").unwrap().hand.len(), 0);
        engine.trigger_on_contact_win("p1", "p1_c1", "TEST_TRAIT_DETECTIVE");
        assert_eq!(engine.players.get("p1").unwrap().hand.len(), 1);
    }

    #[test]
    fn test_cutin_target_passive_ap_and_disguise() {
        let path = if std::path::Path::new("data/dummy/cards.json").exists() {
            "data/dummy/cards.json"
        } else {
            "../data/dummy/cards.json"
        };
        let mut catalog = CardCatalog::load_from_json(path).expect("catalog load failed");

        catalog.definitions.insert(
            "TEST_PASSIVE_OPP_TURN".to_string(),
            models::CardDefinition {
                definition_id: "TEST_PASSIVE_OPP_TURN".to_string(),
                canonical_id: "TEST_PASSIVE_OPP_TURN".to_string(),
                card_type: models::CardType::CHARACTER,
                colors: vec!["青".to_string()],
                level: 3,
                ap: 3000,
                lp: 1,
                names: vec!["守備キャラ".to_string()],
                description: "【相手ターン中】AP＋1000".to_string(),
                traits: vec![],
                case_levels: None,
                main_thumb: None,
                main_path: None,
            },
        );
        catalog.definitions.insert(
            "TEST_CUTIN_SELECT".to_string(),
            models::CardDefinition {
                definition_id: "TEST_CUTIN_SELECT".to_string(),
                canonical_id: "TEST_CUTIN_SELECT".to_string(),
                card_type: models::CardType::CHARACTER,
                colors: vec!["青".to_string()],
                level: 2,
                ap: 2000,
                lp: 1,
                names: vec!["カットイン選択".to_string()],
                description: "【カットイン】相手の現場にいるキャラを1枚まで選び、スリープさせる。".to_string(),
                traits: vec![],
                case_levels: None,
                main_thumb: None,
                main_path: None,
            },
        );
        catalog.definitions.insert(
            "TEST_DISGUISE_KID".to_string(),
            models::CardDefinition {
                definition_id: "TEST_DISGUISE_KID".to_string(),
                canonical_id: "TEST_DISGUISE_KID".to_string(),
                card_type: models::CardType::CHARACTER,
                colors: vec!["白".to_string()],
                level: 4,
                ap: 4000,
                lp: 1,
                names: vec!["テストキッド".to_string()],
                description: "【変装時】カードを1枚引く。このキャラをAP＋1000する。".to_string(),
                traits: vec![],
                case_levels: None,
                main_thumb: None,
                main_path: None,
            },
        );

        let mut engine = MatchEngine::new(catalog);
        engine.turn = 2;
        engine.turn_player_id = "p2".to_string(); // p2's turn -> opponent turn for p1
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let mut ps2 = engine::PlayerState::new("p2".to_string());

        let passive_card = models::CardInstance {
            instance_id: "p1_def".to_string(),
            definition_id: "TEST_PASSIVE_OPP_TURN".to_string(),
            owner_id: "p1".to_string(),
            zone: models::Zone::FIELD,
            orientation: models::CardOrientation::ACTIVE,
            entered_turn: 1,
            face_up: true,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
        };
        ps1.field.push(passive_card.clone());
        ps1.hand.push(models::CardInstance {
            instance_id: "p1_cutin".to_string(),
            definition_id: "TEST_CUTIN_SELECT".to_string(),
            owner_id: "p1".to_string(),
            zone: models::Zone::HAND,
            orientation: models::CardOrientation::ACTIVE,
            entered_turn: 0,
            face_up: true,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
        });
        ps1.deck.push(models::CardInstance {
            instance_id: "p1_dk1".to_string(),
            definition_id: "B01005".to_string(),
            owner_id: "p1".to_string(),
            zone: models::Zone::DECK,
            orientation: models::CardOrientation::ACTIVE,
            entered_turn: 0,
            face_up: false,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
        });

        ps2.field.push(models::CardInstance {
            instance_id: "p2_atk".to_string(),
            definition_id: "B01005".to_string(),
            owner_id: "p2".to_string(),
            zone: models::Zone::FIELD,
            orientation: models::CardOrientation::ACTIVE,
            entered_turn: 1,
            face_up: true,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
        });

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        // 1. Passive AP check: opponent turn -> 3000 + 1000 = 4000
        assert_eq!(engine.get_effective_ap("p1", &passive_card), 4000);
        // Switch turn to p1 -> own turn -> 3000
        engine.turn_player_id = "p1".to_string();
        assert_eq!(engine.get_effective_ap("p1", &passive_card), 3000);
        engine.turn_player_id = "p2".to_string();

        // 2. Cut-in target select check
        engine.action_context = Some(serde_json::json!({
            "actorId": "p2_atk",
            "targetId": "p1_def",
            "attackerPlayerId": "p2",
            "defenderPlayerId": "p1"
        }));
        let dec_id = engine.generate_decision_id();
        engine.pending_decision = Some(serde_json::json!({
            "decisionId": dec_id,
            "revision": engine.revision,
            "playerId": "p1",
            "kind": "guard",
            "options": {"guardCardIds": [], "cutinCardIds": ["p1_cutin"]}
        }));
        engine.submit_decision("p1", &serde_json::json!({
            "decisionId": dec_id,
            "revision": engine.revision,
            "value": {"cutinCardId": "p1_cutin"}
        }));
        let pd = engine.pending_decision.as_ref().unwrap();
        assert_eq!(pd["kind"], "targetSelect");
        let ts_dec_id = pd["decisionId"].as_str().unwrap().to_string();
        engine.submit_decision("p1", &serde_json::json!({
            "decisionId": ts_dec_id,
            "revision": engine.revision,
            "value": {"targetId": "p2_atk", "actorId": "p1_cutin", "effectType": "SLEEP"}
        }));
        assert_eq!(engine.players.get("p2").unwrap().field[0].orientation, models::CardOrientation::SLEEP);
        assert_eq!(engine.pending_decision.as_ref().unwrap()["kind"], "guard");

        // 3. Disguise trigger check
        engine.players.get_mut("p1").unwrap().field.push(models::CardInstance {
            instance_id: "p1_kid".to_string(),
            definition_id: "TEST_DISGUISE_KID".to_string(),
            owner_id: "p1".to_string(),
            zone: models::Zone::FIELD,
            orientation: models::CardOrientation::ACTIVE,
            entered_turn: 2,
            face_up: true,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
        });
        let hand_before = engine.players.get("p1").unwrap().hand.len();
        let need_ui = engine.trigger_on_disguise("p1", "p1_kid", "TEST_DISGUISE_KID");
        assert!(!need_ui);
        assert_eq!(engine.players.get("p1").unwrap().hand.len(), hand_before + 1);
        assert_eq!(engine.players.get("p1").unwrap().field.last().unwrap().temp_ap_bonus, 1000);
    }

    #[test]
    fn test_case_card_resolution_and_declare_effects() {
        let path = if std::path::Path::new("data/dummy/cards.json").exists() {
            "data/dummy/cards.json"
        } else {
            "../data/dummy/cards.json"
        };
        let catalog = CardCatalog::load_from_json(path).expect("catalog load failed");
        let mut engine = MatchEngine::new(catalog);
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let mut ps2 = engine::PlayerState::new("p2".to_string());

        // Case B11099: 解決編になったとき手札1枚リムーブ / 【解決編】【宣言】【ターン1】裏向き証拠2つ表向き -> AP-3000
        ps1.r#case = Some(models::CardInstance {
            instance_id: "p1_case".to_string(),
            definition_id: "B11099".to_string(),
            owner_id: "p1".to_string(),
            zone: models::Zone::CASE,
            orientation: models::CardOrientation::ACTIVE,
            entered_turn: 0,
            face_up: true,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
        });
        for i in 0..7 {
            ps1.file.push(models::CardInstance {
                instance_id: format!("p1_f{}", i),
                definition_id: "B01005".to_string(),
                owner_id: "p1".to_string(),
                zone: models::Zone::FILE,
                orientation: models::CardOrientation::ACTIVE,
                entered_turn: 0,
                face_up: false,
                temp_ap_bonus: 0,
                has_rush: false,
                rush_target: None,
                used_declare_this_turn: false,
            });
        }
        ps1.hand.push(models::CardInstance {
            instance_id: "p1_h1".to_string(),
            definition_id: "B01005".to_string(),
            owner_id: "p1".to_string(),
            zone: models::Zone::HAND,
            orientation: models::CardOrientation::ACTIVE,
            entered_turn: 0,
            face_up: true,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
        });
        for i in 0..2 {
            ps1.evidence.push(models::CardInstance {
                instance_id: format!("p1_ev{}", i),
                definition_id: "B01005".to_string(),
                owner_id: "p1".to_string(),
                zone: models::Zone::EVIDENCE,
                orientation: models::CardOrientation::ACTIVE,
                entered_turn: 0,
                face_up: false, // face down
                temp_ap_bonus: 0,
                has_rush: false,
                rush_target: None,
                used_declare_this_turn: false,
            });
        }

        ps2.field.push(models::CardInstance {
            instance_id: "p2_c1".to_string(),
            definition_id: "B01005".to_string(),
            owner_id: "p2".to_string(),
            zone: models::Zone::FIELD,
            orientation: models::CardOrientation::ACTIVE,
            entered_turn: 1,
            face_up: true,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
        });

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        // 1. Trigger resolution transition -> hand size goes from 1 to 0
        engine.check_resolution_transition("p1");
        assert_eq!(engine.players.get("p1").unwrap().case_phase, models::CasePhase::RESOLUTION);
        assert_eq!(engine.players.get("p1").unwrap().hand.len(), 0);

        // 2. Declare option should include case card
        engine.prompt_main_action("p1");
        let pd = engine.pending_decision.as_ref().unwrap().clone();
        let actions = pd["options"]["actions"].as_array().unwrap();
        let declare_act = actions.iter().find(|a| a["type"] == "DECLARE").unwrap();
        let actors = declare_act["actors"].as_array().unwrap();
        assert!(actors.iter().any(|a| a["actorId"] == "p1_case"));

        // 3. Execute DECLARE on case card
        let dec_id = pd["decisionId"].as_str().unwrap().to_string();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": dec_id,
            "revision": engine.revision,
            "value": {"type": "DECLARE", "actorId": "p1_case"}
        }));

        // Two evidence cards should now be face_up
        assert!(engine.players.get("p1").unwrap().evidence.iter().all(|e| e.face_up));

        // Target select for AP-3000 should be pending
        let ts_pd = engine.pending_decision.as_ref().unwrap().clone();
        assert_eq!(ts_pd["kind"], "targetSelect");
        let ts_dec_id = ts_pd["decisionId"].as_str().unwrap().to_string();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": ts_dec_id,
            "revision": engine.revision,
            "value": {"targetId": "p2_c1", "actorId": "p1_case", "effectType": "AP_DEBUFF"}
        }));

        assert_eq!(engine.players.get("p2").unwrap().field[0].temp_ap_bonus, -3000);
    }

    #[test]
    fn test_event_card_play_and_remove_zone() {
        let path = if std::path::Path::new("data/dummy/cards.json").exists() {
            "data/dummy/cards.json"
        } else {
            "../data/dummy/cards.json"
        };
        let mut catalog = CardCatalog::load_from_json(path).expect("catalog load failed");

        catalog.definitions.insert(
            "TEST_EVENT_DRAW".to_string(),
            models::CardDefinition {
                definition_id: "TEST_EVENT_DRAW".to_string(),
                canonical_id: "TEST_EVENT_DRAW".to_string(),
                card_type: models::CardType::EVENT,
                colors: vec!["青".to_string()],
                level: 2,
                ap: 0,
                lp: 0,
                names: vec!["テストイベント".to_string()],
                description: "カードを1枚引く。".to_string(),
                traits: vec![],
                case_levels: None,
                main_thumb: None,
                main_path: None,
            },
        );

        let mut engine = MatchEngine::new(catalog);
        engine.turn = 3;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let ps2 = engine::PlayerState::new("p2".to_string());

        // Fill p1 field to max 5 chars and mark normal_hand_played_this_turn = true
        ps1.normal_hand_played_this_turn = true;
        for i in 0..5 {
            ps1.field.push(models::CardInstance {
                instance_id: format!("p1_f{}", i),
                definition_id: "B01005".to_string(),
                owner_id: "p1".to_string(),
                zone: models::Zone::FIELD,
                orientation: models::CardOrientation::ACTIVE,
                entered_turn: 1,
                face_up: true,
                temp_ap_bonus: 0,
                has_rush: false,
                rush_target: None,
                used_declare_this_turn: false,
            });
        }
        // Add 3 FILEs
        for i in 0..3 {
            ps1.file.push(models::CardInstance {
                instance_id: format!("p1_fl{}", i),
                definition_id: "B01005".to_string(),
                owner_id: "p1".to_string(),
                zone: models::Zone::FILE,
                orientation: models::CardOrientation::ACTIVE,
                entered_turn: 0,
                face_up: false,
                temp_ap_bonus: 0,
                has_rush: false,
                rush_target: None,
                used_declare_this_turn: false,
            });
        }
        // Add event card to hand and 1 card in deck
        ps1.hand.push(models::CardInstance {
            instance_id: "p1_ev_card".to_string(),
            definition_id: "TEST_EVENT_DRAW".to_string(),
            owner_id: "p1".to_string(),
            zone: models::Zone::HAND,
            orientation: models::CardOrientation::ACTIVE,
            entered_turn: 0,
            face_up: true,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
        });
        ps1.deck.push(models::CardInstance {
            instance_id: "p1_dk1".to_string(),
            definition_id: "B01005".to_string(),
            owner_id: "p1".to_string(),
            zone: models::Zone::DECK,
            orientation: models::CardOrientation::ACTIVE,
            entered_turn: 0,
            face_up: false,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
        });

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        // Even with 5 field cards & normal_hand_played = true, event card should be playable
        engine.prompt_main_action("p1");
        let pd = engine.pending_decision.as_ref().unwrap().clone();
        let actions = pd["options"]["actions"].as_array().unwrap();
        let hand_play = actions.iter().find(|a| a["type"] == "HAND_PLAY").unwrap();
        assert!(hand_play["cardIds"].as_array().unwrap().contains(&serde_json::json!("p1_ev_card")));

        // Play event card
        let dec_id = pd["decisionId"].as_str().unwrap().to_string();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": dec_id,
            "revision": engine.revision,
            "value": {"type": "HAND_PLAY", "cardId": "p1_ev_card"}
        }));

        let p1_after = engine.players.get("p1").unwrap();
        // Field stays 5, event goes to remove, and 1 card drawn into hand
        assert_eq!(p1_after.field.len(), 5);
        assert_eq!(p1_after.remove.len(), 1);
        assert_eq!(p1_after.remove[0].definition_id, "TEST_EVENT_DRAW");
        assert_eq!(p1_after.hand.len(), 1);
    }

    #[test]
    fn test_smart_cpu_agent_decisions() {
        let path = if std::path::Path::new("data/dummy/cards.json").exists() {
            "data/dummy/cards.json"
        } else {
            "../data/dummy/cards.json"
        };
        let mut catalog = CardCatalog::load_from_json(path).expect("catalog load failed");

        for (id, ap, lvl) in [
            ("CPU_LOW", 2000, 2),
            ("CPU_MID", 3000, 3),
            ("CPU_HIGH", 5000, 5),
        ] {
            catalog.definitions.insert(
                id.to_string(),
                models::CardDefinition {
                    definition_id: id.to_string(),
                    canonical_id: id.to_string(),
                    card_type: models::CardType::CHARACTER,
                    colors: vec!["青".to_string()],
                    level: lvl,
                    ap,
                    lp: 1,
                    names: vec![id.to_string()],
                    description: String::new(),
                    traits: vec![],
                    case_levels: None,
                    main_thumb: None,
                    main_path: None,
                },
            );
        }

        let mut engine = MatchEngine::new(catalog);
        engine.turn = 3;
        engine.turn_player_id = "player2".to_string();
        engine.player_order = vec!["player1".to_string(), "player2".to_string()];

        let mut ps1 = engine::PlayerState::new("player1".to_string());
        let mut ps2 = engine::PlayerState::new("player2".to_string());

        // player1 (opponent) has AP2000 (SLEEP) and AP5000 (SLEEP)
        ps1.field.push(models::CardInstance {
            instance_id: "p1_weak".to_string(),
            definition_id: "CPU_LOW".to_string(),
            owner_id: "player1".to_string(),
            zone: models::Zone::FIELD,
            orientation: models::CardOrientation::SLEEP,
            entered_turn: 1,
            face_up: true,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
        });
        ps1.field.push(models::CardInstance {
            instance_id: "p1_strong".to_string(),
            definition_id: "CPU_HIGH".to_string(),
            owner_id: "player1".to_string(),
            zone: models::Zone::FIELD,
            orientation: models::CardOrientation::SLEEP,
            entered_turn: 1,
            face_up: true,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
        });

        // player2 (CPU) has AP3000 (ACTIVE)
        ps2.field.push(models::CardInstance {
            instance_id: "p2_mid".to_string(),
            definition_id: "CPU_MID".to_string(),
            owner_id: "player2".to_string(),
            zone: models::Zone::FIELD,
            orientation: models::CardOrientation::ACTIVE,
            entered_turn: 1,
            face_up: true,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
        });
        // player2 hand has Lv2 and Lv5 for mulligan test
        ps2.hand.push(models::CardInstance {
            instance_id: "h_lv2".to_string(),
            definition_id: "CPU_LOW".to_string(),
            owner_id: "player2".to_string(),
            zone: models::Zone::HAND,
            orientation: models::CardOrientation::ACTIVE,
            entered_turn: 0,
            face_up: true,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
        });
        ps2.hand.push(models::CardInstance {
            instance_id: "h_lv5".to_string(),
            definition_id: "CPU_HIGH".to_string(),
            owner_id: "player2".to_string(),
            zone: models::Zone::HAND,
            orientation: models::CardOrientation::ACTIVE,
            entered_turn: 0,
            face_up: true,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
        });

        engine.players.insert("player1".to_string(), ps1);
        engine.players.insert("player2".to_string(), ps2);

        let cpu = agent::RandomAgent::new("player2");

        // 1. Mulligan test: returns Lv5, keeps Lv2
        let mul_dec = serde_json::json!({
            "kind": "mulligan",
            "options": {"handCardIds": ["h_lv2", "h_lv5"]}
        });
        let mul_ans = cpu.choose_action_with_engine(&engine, &mul_dec);
        assert_eq!(mul_ans["value"]["cardIds"], serde_json::json!(["h_lv5"]));

        // 2. MainAction test: AP3000 attacks AP2000 (not AP5000)
        let act_dec = serde_json::json!({
            "kind": "mainAction",
            "options": {
                "actions": [
                    {"type": "END_TURN"},
                    {
                        "type": "ACTION",
                        "actorIds": ["p2_mid"],
                        "targetIds": ["p1_weak", "p1_strong"],
                        "actorTargetMap": {"p2_mid": ["p1_weak", "p1_strong"]}
                    }
                ]
            }
        });
        let act_ans = cpu.choose_action_with_engine(&engine, &act_dec);
        assert_eq!(act_ans["value"]["type"], "ACTION");
        assert_eq!(act_ans["value"]["targetId"], "p1_weak");

        // 3. Guard test: if attacker AP is 2000, AP3000 blocks it
        engine.action_context = Some(serde_json::json!({
            "attackerPlayerId": "player1",
            "actorId": "p1_weak",
            "targetId": "p2_case"
        }));
        let guard_dec = serde_json::json!({
            "kind": "guard",
            "options": {"guardCardIds": ["p2_mid"], "cutinCardIds": []}
        });
        let guard_ans = cpu.choose_action_with_engine(&engine, &guard_dec);
        assert_eq!(guard_ans["value"]["guardCardId"], "p2_mid");
    }
}

