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
mod effects;
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

    fn test_inst(iid: &str, did: &str, owner: &str, zone: models::Zone, ori: models::CardOrientation) -> models::CardInstance {
        models::CardInstance {
            instance_id: iid.to_string(),
            definition_id: did.to_string(),
            owner_id: owner.to_string(),
            zone,
            orientation: ori,
            entered_turn: 1,
            face_up: true,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
        }
    }

    fn load_test_catalog() -> CardCatalog {
        let path = if std::path::Path::new("data/dummy/cards.json").exists() {
            "data/dummy/cards.json"
        } else {
            "../data/dummy/cards.json"
        };
        CardCatalog::load_from_json(path).expect("catalog load failed")
    }

    #[test]
    fn test_trait_target_filtering_mori_office_and_server_validation() {
        let catalog = load_test_catalog();
        // Verify catalog loaded traits properly
        let b04005 = catalog.get("B04005").expect("B04005 exists");
        assert!(b04005.traits.iter().any(|t| t == "毛利探偵事務所"));

        let mut engine = MatchEngine::new(catalog);
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let ps2 = engine::PlayerState::new("p2".to_string());
        // B04005: 【宣言】【ターン1】このキャラ以外の〚特徴［毛利探偵事務所］〛のキャラを1枚まで選び、AP+1000
        ps1.field.push(test_inst("p1_conan_b04005", "B04005", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        // B01007: 毛利蘭 (毛利探偵事務所) -> MUST match
        ps1.field.push(test_inst("p1_ran", "B01007", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        // B01006: 阿笠博士 (少年探偵団, 科学者 - NOT 毛利探偵事務所!) -> MUST BE EXCLUDED
        ps1.field.push(test_inst("p1_agasa", "B01006", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        for i in 0..7 {
            ps1.file.push(test_inst(&format!("f{}", i), "B01005", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
        }

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        engine.prompt_main_action("p1");
        let pd = engine.pending_decision.as_ref().unwrap().clone();
        let dec_id = pd["decisionId"].as_str().unwrap().to_string();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": dec_id,
            "revision": engine.revision,
            "value": {"type": "DECLARE", "actorId": "p1_conan_b04005"}
        }));

        let ts_pd = engine.pending_decision.as_ref().unwrap().clone();
        assert_eq!(ts_pd["kind"], "targetSelect");
        let cands = ts_pd["options"]["candidates"].as_array().unwrap();
        let cand_ids: Vec<&str> = cands.iter().filter_map(|c| c["instanceId"].as_str()).collect();
        // Only p1_ran matches! Both self (p1_conan_b04005) and non-Mori-office (p1_agasa) must be excluded.
        assert_eq!(cand_ids, vec!["p1_ran"]);

        // Server-side validation must reject tampering with invalid target p1_agasa
        let ts_dec_id = ts_pd["decisionId"].as_str().unwrap().to_string();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": ts_dec_id,
            "revision": engine.revision,
            "value": {"targetId": "p1_agasa", "actorId": "p1_conan_b04005", "effectType": "AP_BUFF"}
        }));
        assert_eq!(engine.players.get("p1").unwrap().field[2].temp_ap_bonus, 0);
    }

    #[test]
    fn test_b04005_clause_isolation_and_post_draw_discard() {
        let catalog = load_test_catalog();
        let mut engine = MatchEngine::new(catalog);
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let ps2 = engine::PlayerState::new("p2".to_string());
        for i in 0..7 {
            ps1.file.push(test_inst(&format!("f{}", i), "B01005", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
        }
        ps1.deck.push(test_inst("d1", "B01007", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        ps1.deck.push(test_inst("d2", "B01006", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("h_b04005", "B04005", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps1.field.push(test_inst("p1_ran", "B01007", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        engine.prompt_main_action("p1");
        let pd = engine.pending_decision.as_ref().unwrap().clone();
        let dec_id = pd["decisionId"].as_str().unwrap().to_string();
        // Play B04005 from empty remaining hand: should draw 2 then remove 2 from hand (NOT fizzle, and NOT trigger Declare AP+1000)
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": dec_id,
            "revision": engine.revision,
            "value": {"type": "HAND_PLAY", "cardId": "h_b04005"}
        }));

        // B04005: 2枚ドロー後、手札を2枚選ぶ DISCARD_HAND が開く
        let pd_disc = engine.pending_decision.as_ref().unwrap().clone();
        assert_eq!(pd_disc["kind"], "targetSelect");
        assert_eq!(pd_disc["options"]["effectType"], "DISCARD_HAND");
        assert_eq!(pd_disc["options"]["maxSelect"], 2);
        assert_eq!(pd_disc["options"]["optional"], false);

        let cands = pd_disc["options"]["candidates"].as_array().unwrap();
        assert_eq!(cands.len(), 2);
        let ids: Vec<&str> = cands.iter().map(|c| c["instanceId"].as_str().unwrap()).collect();
        let disc_id = pd_disc["decisionId"].as_str().unwrap().to_string();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": disc_id,
            "revision": engine.revision,
            "value": {"targetIds": ids, "actorId": "h_b04005", "effectType": "DISCARD_HAND"}
        }));

        let ps1_after = engine.players.get("p1").unwrap();
        assert_eq!(ps1_after.deck.len(), 0);
        assert_eq!(ps1_after.remove.len(), 2);
        assert_eq!(ps1_after.hand.len(), 0);
        // Should return cleanly to mainAction without opening Declare's targetSelect
        assert_eq!(engine.pending_decision.as_ref().unwrap()["kind"], "mainAction");
    }

    #[test]
    fn test_b05006_state_branching_presence_cond_and_followup_draw() {
        let catalog = load_test_catalog();
        let mut engine = MatchEngine::new(catalog);
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let mut ps2 = engine::PlayerState::new("p2".to_string());
        ps1.partner = Some(test_inst("p1_partner", "B01001", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE)); // Blue partner
        ps1.field.push(test_inst("p1_b05006", "B05006", "p1", models::Zone::FIELD, models::CardOrientation::SLEEP));
        ps1.deck.push(test_inst("d1", "B01005", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));

        // Enemy low AP (<=8000) vs high AP (9000 -> must be filtered out!)
        ps2.field.push(test_inst("opp_low", "B01005", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps2.field.push(test_inst("opp_hi", "B01004", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE)); // Check AP below if >8000, else add synthetic 9000

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        // Without another 毛利探偵事務所 or 妃英理 on field, DECLARE must NOT be offered for B05006
        engine.prompt_main_action("p1");
        let pd1 = engine.pending_decision.as_ref().unwrap();
        let has_decl = pd1["options"]["actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["type"] == "DECLARE");
        assert!(!has_decl);

        // Add 毛利蘭 (毛利探偵事務所) and make sure opp_hi is AP 9000
        engine.players.get_mut("p1").unwrap().field.push(test_inst("p1_ran", "B01007", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        engine.players.get_mut("p2").unwrap().field[1].temp_ap_bonus = 9000;

        engine.prompt_main_action("p1");
        let pd2 = engine.pending_decision.as_ref().unwrap().clone();
        let dec_id = pd2["decisionId"].as_str().unwrap().to_string();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": dec_id,
            "revision": engine.revision,
            "value": {"type": "DECLARE", "actorId": "p1_b05006"}
        }));

        let ts_pd = engine.pending_decision.as_ref().unwrap().clone();
        assert_eq!(ts_pd["kind"], "targetSelect");
        let cands = ts_pd["options"]["candidates"].as_array().unwrap();
        let cand_ids: Vec<&str> = cands.iter().filter_map(|c| c["instanceId"].as_str()).collect();
        assert_eq!(cand_ids, vec!["opp_low"]); // opp_hi (AP > 8000) excluded!

        let ts_dec_id = ts_pd["decisionId"].as_str().unwrap().to_string();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": ts_dec_id,
            "revision": engine.revision,
            "value": {"targetId": "opp_low", "actorId": "p1_b05006", "effectType": "REMOVE"}
        }));

        // Enemy opp_low removed AND follow-up "カードを1枚引く" drew d1 into hand!
        assert_eq!(engine.players.get("p2").unwrap().field.len(), 1);
        assert_eq!(engine.players.get("p1").unwrap().hand.len(), 1);
    }

    #[test]
    fn test_deck_search_selectable_filtering_and_validation() {
        let mut catalog = load_test_catalog();
        catalog.definitions.insert(
            "TEST_SEARCH_LP0_BLUE".to_string(),
            models::CardDefinition {
                definition_id: "TEST_SEARCH_LP0_BLUE".to_string(),
                canonical_id: "TEST_SEARCH_LP0_BLUE".to_string(),
                card_type: models::CardType::CHARACTER,
                colors: vec!["青".to_string()],
                level: 2,
                ap: 2000,
                lp: 0,
                names: vec!["サーチャー".to_string()],
                description: "【登場時】自分のデッキのカードを上から2枚見る。その中からLP0の【青】のキャラを1枚まで公開して手札に加え、残りを好きな順番でデッキの下に移す。".to_string(),
                traits: vec![],
                case_levels: None,
                main_thumb: None,
                main_path: None,
            },
        );
        catalog.definitions.insert(
            "TEST_RED_LP0".to_string(),
            models::CardDefinition {
                definition_id: "TEST_RED_LP0".to_string(),
                canonical_id: "TEST_RED_LP0".to_string(),
                card_type: models::CardType::CHARACTER,
                colors: vec!["赤".to_string()],
                level: 2,
                ap: 2000,
                lp: 0,
                names: vec!["非対象赤".to_string()],
                description: "".to_string(),
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
        for i in 0..5 {
            ps1.file.push(test_inst(&format!("f{}", i), "B01005", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
        }
        // Top of deck is last elements: bottom="B01005"(bottom), top2=["TEST_RED_LP0", "TEST_SEARCH_LP0_BLUE"]
        ps1.deck.push(test_inst("dk_bot", "B01005", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        ps1.deck.push(test_inst("dk_red", "TEST_RED_LP0", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        ps1.deck.push(test_inst("dk_blue0", "TEST_SEARCH_LP0_BLUE", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("h_src", "TEST_SEARCH_LP0_BLUE", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        engine.prompt_main_action("p1");
        let pd = engine.pending_decision.as_ref().unwrap().clone();
        let dec_id = pd["decisionId"].as_str().unwrap().to_string();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": dec_id,
            "revision": engine.revision,
            "value": {"type": "HAND_PLAY", "cardId": "h_src"}
        }));

        let ds_pd = engine.pending_decision.as_ref().unwrap().clone();
        assert_eq!(ds_pd["kind"], "deckSearch");
        let revealed = ds_pd["options"]["revealedCards"].as_array().unwrap();
        assert_eq!(revealed.len(), 2);
        let r_blue = revealed.iter().find(|c| c["instanceId"] == "dk_blue0").unwrap();
        let r_red = revealed.iter().find(|c| c["instanceId"] == "dk_red").unwrap();
        assert_eq!(r_blue["selectable"], true);
        assert_eq!(r_red["selectable"], false);

        // Picking valid dk_blue0 adds it to hand and places dk_red on bottom of deck
        let ds_id = ds_pd["decisionId"].as_str().unwrap().to_string();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": ds_id,
            "revision": engine.revision,
            "value": {"chosenCardId": "dk_blue0", "actorId": "h_src"}
        }));

        let ps1_after = engine.players.get("p1").unwrap();
        assert_eq!(ps1_after.hand.len(), 1);
        assert_eq!(ps1_after.hand[0].instance_id, "dk_blue0");
        assert_eq!(ps1_after.deck.len(), 2);
        assert_eq!(ps1_after.deck[0].instance_id, "dk_red"); // placed at deck bottom
    }

    #[test]
    fn test_hirameki_trigger_and_disguise_deck_bottom() {
        let catalog = load_test_catalog();
        let mut engine = MatchEngine::new(catalog);
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let mut ps2 = engine::PlayerState::new("p2".to_string());

        // Give p1 a white case (B01099 or white case) & 6 files & disguise B02038 + char on field
        ps1.r#case = Some(test_inst("p1_case", "B01100", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE)); // 青＆白
        for i in 0..6 {
            ps1.file.push(test_inst(&format!("f{}", i), "B01005", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
        }
        ps1.field.push(test_inst("p1_f1", "B01005", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("h_kid", "B02038", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE)); // 【変装】【事件白】【FILE6】
        ps1.deck.push(test_inst("p1_d1", "B01005", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));

        // p2 has evidence D03013 (【ヒラメキ】キャラを1枚まで選び、スリープさせる) & active attacker
        ps2.r#case = Some(test_inst("p2_case", "B01002", "p2", models::Zone::CASE, models::CardOrientation::ACTIVE));
        ps2.evidence.push(test_inst("p2_ev1", "D03013", "p2", models::Zone::EVIDENCE, models::CardOrientation::ACTIVE));

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        // 1) Disguise B02038 onto p1_f1 -> p1_f1 must move to DECK bottom (NOT remove), draw 1 from 【変装時】!
        engine.prompt_main_action("p1");
        let pd = engine.pending_decision.as_ref().unwrap().clone();
        let dec_id = pd["decisionId"].as_str().unwrap().to_string();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": dec_id,
            "revision": engine.revision,
            "value": {"type": "DISGUISE", "cardId": "h_kid", "targetId": "p1_f1"}
        }));

        let ps1_after_disguise = engine.players.get("p1").unwrap();
        assert_eq!(ps1_after_disguise.remove.len(), 0);
        assert_eq!(ps1_after_disguise.deck[0].instance_id, "p1_f1");
        assert_eq!(ps1_after_disguise.hand.len(), 1); // drew p1_d1 from 【変装時】

        // 2) Simulate Case attack hitting D03013 in p2's evidence -> Hirameki opens targetSelect for p2 to sleep p1's Kid!
        engine.action_context = Some(serde_json::json!({
            "attackerPlayerId": "p1",
            "defenderPlayerId": "p2",
            "actorId": "h_kid",
            "targetId": "p2_case"
        }));
        let g_pd_dec = engine.generate_decision_id();
        engine.pending_decision = Some(serde_json::json!({
            "decisionId": g_pd_dec,
            "revision": engine.revision,
            "playerId": "p2",
            "kind": "guard",
            "options": {}
        }));
        let _ = engine.submit_decision("p2", &serde_json::json!({
            "decisionId": g_pd_dec,
            "revision": engine.revision,
            "value": {"guardCardId": null}
        }));

        let h_pd = engine.pending_decision.as_ref().unwrap().clone();
        assert_eq!(h_pd["kind"], "targetSelect");
        assert_eq!(h_pd["playerId"], "p2");
        assert_eq!(h_pd["options"]["effectType"], "SLEEP");
        let h_dec_id = h_pd["decisionId"].as_str().unwrap().to_string();
        let _ = engine.submit_decision("p2", &serde_json::json!({
            "decisionId": h_dec_id,
            "revision": engine.revision,
            "value": {"targetId": "h_kid", "actorId": "p2_ev1", "effectType": "SLEEP"}
        }));

        assert_eq!(engine.players.get("p1").unwrap().field[0].orientation, models::CardOrientation::SLEEP);
        assert_eq!(engine.players.get("p1").unwrap().evidence.len(), 1); // p1 gained 1 evidence from case hit
    }

    #[test]
    fn test_hint_use_bottom_deck_and_remove_revival() {
        let catalog = load_test_catalog();
        let mut engine = MatchEngine::new(catalog);
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let mut ps2 = engine::PlayerState::new("p2".to_string());
        // B01005 Partner: 【パートナー青】【ターン1】自分がネクストヒントで手札を使用したとき、そのカードのレベル以下のキャラを1枚まで選び、デッキの下に移す。
        ps1.partner = Some(test_inst("p1_p_b01005", "B01005", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
        // B01001 Partner or Blue? B01005 Partner isn't Blue unless Partner colors include 青 - wait B01005 itself is 青 char, check B01001 Partner vs trigger
        ps1.partner = Some(test_inst("p1_partner", "B01001", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
        ps1.field.push(test_inst("p1_watcher", "B01005", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        // Enemy field: Lv4 vs Lv8
        ps2.field.push(test_inst("opp_lv4", "B01007", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE)); // Check actual levels or make sure
        // Put Lv4 in remove for B02038 revival check too
        ps1.remove.push(test_inst("rem_lv4_w", "B02038", "p1", models::Zone::REMOVE, models::CardOrientation::ACTIVE));

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        // Trigger OnHintUse with hint card level 5: B01007(Lv5? Let's verify) vs Lv8
        let lv_b01007 = engine.catalog.get("B01007").unwrap().level;
        let triggered = engine.trigger_on_hint_use("p1", lv_b01007);
        assert!(triggered);
        let ts = engine.pending_decision.as_ref().unwrap().clone();
        assert_eq!(ts["options"]["effectType"], "BOTTOM_DECK");
        let ds_id = ts["decisionId"].as_str().unwrap().to_string();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": ds_id,
            "revision": engine.revision,
            "value": {"targetId": "opp_lv4", "actorId": "p1_watcher", "effectType": "BOTTOM_DECK"}
        }));

        assert_eq!(engine.players.get("p2").unwrap().field.len(), 0);
        assert_eq!(engine.players.get("p2").unwrap().deck.len(), 1);
    }

    #[test]
    fn test_id0102_small_detective_deck_search_then_file_level_hand_play() {
        let catalog = load_test_catalog();
        let mut engine = engine::MatchEngine::new(catalog);
        engine.phase = models::MatchPhase::MAIN;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let ps2 = engine::PlayerState::new("p2".to_string());

        ps1.r#case = Some(test_inst("p1_case", "B01002", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE)); // 青事件
        // FILEエリア3枚（よって手札登場の上限レベルは 3 以下）
        for i in 0..3 {
            ps1.file.push(test_inst(&format!("f{}", i), "B01021", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
        }

        // 山札上3枚（Vecの末尾がデッキトップ）: 青イベント(B01022), 緑キャラ(B01029), 青キャラ(B01021)
        ps1.deck.push(test_inst("d_base", "B01020", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        ps1.deck.push(test_inst("d_ev", "B01022", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        ps1.deck.push(test_inst("d_green", "B01029", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        ps1.deck.push(test_inst("d_blue", "B01021", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));

        // 手札: D01014 (小さくなった名探偵, ID0102), Lv3青キャラ(B01021), Lv5青キャラ(B01020), Lv5緑キャラ(B01029)
        ps1.hand.push(test_inst("h_ev", "D01014", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("h_b3", "B01021", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("h_b5", "B01020", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("h_g5", "B01029", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        // D01014 をイベントプレイ
        engine.prompt_main_action("p1");
        let pd1 = engine.pending_decision.as_ref().unwrap().clone();
        let d1_id = pd1["decisionId"].as_str().unwrap().to_string();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": d1_id,
            "revision": engine.revision,
            "value": {"type": "HAND_PLAY", "cardId": "h_ev"}
        }));

        // 1段階目: deckSearch (TO_HAND であり PLAY_ACTIVE ではないこと！)
        let pd_ds = engine.pending_decision.as_ref().unwrap().clone();
        assert_eq!(pd_ds["kind"], "deckSearch");
        assert_eq!(pd_ds["options"]["pickAction"], "TO_HAND");
        let revealed = pd_ds["options"]["revealedCards"].as_array().unwrap();
        assert_eq!(revealed.len(), 3);
        let s_blue = revealed.iter().find(|c| c["instanceId"] == "d_blue").unwrap()["selectable"].as_bool().unwrap();
        let s_green = revealed.iter().find(|c| c["instanceId"] == "d_green").unwrap()["selectable"].as_bool().unwrap();
        let s_ev = revealed.iter().find(|c| c["instanceId"] == "d_ev").unwrap()["selectable"].as_bool().unwrap();
        assert!(s_blue, "【青】キャラは選択可能");
        assert!(!s_green, "【緑】キャラは選択不可");
        assert!(!s_ev, "【青】イベントは選択不可");

        let ds_id = pd_ds["decisionId"].as_str().unwrap().to_string();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": ds_id,
            "revision": engine.revision,
            "value": {"chosenCardId": "d_blue", "actorId": "h_ev"}
        }));

        // d_blue は手札へ、残り2枚(d_green, d_ev)はデッキ下へ
        assert!(engine.players.get("p1").unwrap().hand.iter().any(|c| c.instance_id == "d_blue"));
        assert_eq!(engine.players.get("p1").unwrap().deck.len(), 3);

        // 2段階目: 続けて「手札から自分のFILEエリアの枚数(3)以下のレベルの【青】のキャラを1枚まで登場させる」が開く！
        let pd_ts = engine.pending_decision.as_ref().expect("followUpTarget で targetSelect が開くべき").clone();
        assert_eq!(pd_ts["kind"], "targetSelect");
        assert_eq!(pd_ts["options"]["effectType"], "PLAY_FROM_HAND");
        assert_eq!(pd_ts["options"]["targetZone"], "HAND");
        let cands = pd_ts["options"]["candidates"].as_array().unwrap();
        let cand_ids: Vec<&str> = cands.iter().filter_map(|c| c["instanceId"].as_str()).collect();
        assert!(cand_ids.contains(&"h_b3"), "Lv3青キャラは候補に含まれる");
        assert!(cand_ids.contains(&"d_blue"), "先ほど加えたLv3青キャラも候補に含まれる");
        assert!(!cand_ids.contains(&"h_b5"), "FILE3枚時にLv5青キャラは候補外");
        assert!(!cand_ids.contains(&"h_g5"), "緑キャラは候補外");

        let ts_id = pd_ts["decisionId"].as_str().unwrap().to_string();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": ts_id,
            "revision": engine.revision,
            "value": {"targetId": "h_b3", "actorId": "h_ev", "effectType": "PLAY_FROM_HAND"}
        }));

        assert!(engine.players.get("p1").unwrap().field.iter().any(|c| c.instance_id == "h_b3"));
    }

    #[test]
    fn test_b03025_remainder_to_remove_and_follow_up_hand_play() {
        let catalog = load_test_catalog();
        let mut engine = engine::MatchEngine::new(catalog);
        engine.phase = models::MatchPhase::MAIN;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let ps2 = engine::PlayerState::new("p2".to_string());
        ps1.r#case = Some(test_inst("p1_case", "B01002", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
        for i in 0..6 {
            ps1.file.push(test_inst(&format!("f{}", i), "B01021", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
        }

        // 山札上5枚: 少年探偵団(B01021) x1 + 非探偵団(B01020) x4
        for i in 0..4 {
            ps1.deck.push(test_inst(&format!("d_other_{}", i), "B01020", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        }
        ps1.deck.push(test_inst("d_dt", "B01021", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));

        ps1.hand.push(test_inst("h_ev", "B03025", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("h_dt", "B01021", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        engine.prompt_main_action("p1");
        let pd1 = engine.pending_decision.as_ref().unwrap().clone();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd1["decisionId"],
            "revision": engine.revision,
            "value": {"type": "HAND_PLAY", "cardId": "h_ev"}
        }));

        let pd_ds = engine.pending_decision.as_ref().unwrap().clone();
        assert_eq!(pd_ds["kind"], "deckSearch");
        assert_eq!(pd_ds["options"]["pickAction"], "TO_HAND");
        assert_eq!(pd_ds["options"]["remainderToRemove"], true);

        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_ds["decisionId"],
            "revision": engine.revision,
            "value": {"chosenCardId": "d_dt", "actorId": "h_ev"}
        }));

        // 残り4枚はデッキ下ではなくリムーブエリアへ (B03025自身+残り4枚=5枚)
        let ps1_mid = engine.players.get("p1").unwrap();
        assert_eq!(ps1_mid.deck.len(), 0);
        assert_eq!(ps1_mid.remove.len(), 5);

        // 続けて手札から少年探偵団を登場させる targetSelect が開く
        let pd_ts = engine.pending_decision.as_ref().unwrap().clone();
        assert_eq!(pd_ts["kind"], "targetSelect");
        assert_eq!(pd_ts["options"]["effectType"], "PLAY_FROM_HAND");
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_ts["decisionId"],
            "revision": engine.revision,
            "value": {"targetId": "h_dt", "actorId": "h_ev", "effectType": "PLAY_FROM_HAND"}
        }));
        assert!(engine.players.get("p1").unwrap().field.iter().any(|c| c.instance_id == "h_dt"));
    }

    #[test]
    fn test_interactive_hand_discard_cost_optional_and_deck_search() {
        let catalog = load_test_catalog();
        let mut engine = engine::MatchEngine::new(catalog);
        engine.phase = models::MatchPhase::MAIN;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let ps2 = engine::PlayerState::new("p2".to_string());
        ps1.partner = Some(test_inst("p1_partner", "B01001", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE)); // 青
        ps1.r#case = Some(test_inst("p1_case", "B01002", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
        for i in 0..8 {
            ps1.file.push(test_inst(&format!("f{}", i), "B01021", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
        }

        // 1) B01007 (【パートナー青】【宣言】〚手札を1枚リムーブする〛：AP+1000 & 突撃)
        ps1.field.push(test_inst("f_ran", "B01007", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("h_keep", "B01021", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("h_drop", "B01020", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        engine.prompt_main_action("p1");
        let pd1 = engine.pending_decision.as_ref().unwrap().clone();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd1["decisionId"],
            "revision": engine.revision,
            "value": {"type": "DECLARE", "actorId": "f_ran"}
        }));

        // コストの手札1枚をプレイヤーが選べること
        let pd_cost = engine.pending_decision.as_ref().unwrap().clone();
        assert_eq!(pd_cost["kind"], "targetSelect");
        assert_eq!(pd_cost["options"]["effectType"], "DISCARD_HAND");
        assert_eq!(pd_cost["options"]["optional"], false);
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_cost["decisionId"],
            "revision": engine.revision,
            "value": {"targetId": "h_drop", "actorId": "f_ran", "effectType": "DISCARD_HAND"}
        }));

        let ps1_after_decl = engine.players.get("p1").unwrap();
        assert!(ps1_after_decl.hand.iter().any(|c| c.instance_id == "h_keep"));
        assert!(ps1_after_decl.remove.iter().any(|c| c.instance_id == "h_drop"));
        let ran = ps1_after_decl.field.iter().find(|c| c.instance_id == "f_ran").unwrap();
        assert_eq!(ran.temp_ap_bonus, 1000);
        assert!(ran.has_rush);

        // 2) PR041 (【登場時】手札から【緑】のイベントを1枚リムーブしてもよい。そうした場合、カードを2枚引く。)
        {
            let ps1_mut = engine.players.get_mut("p1").unwrap();
            ps1_mut.hand.push(test_inst("h_pr041", "PR041", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
            ps1_mut.hand.push(test_inst("h_green_ev", "B04026", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE)); // 緑イベント
            ps1_mut.deck.push(test_inst("d1", "B01021", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
            ps1_mut.deck.push(test_inst("d2", "B01021", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        }
        let pd2 = engine.pending_decision.as_ref().unwrap().clone();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd2["decisionId"],
            "revision": engine.revision,
            "value": {"type": "HAND_PLAY", "cardId": "h_pr041"}
        }));

        let pd_opt = engine.pending_decision.as_ref().unwrap().clone();
        assert_eq!(pd_opt["kind"], "targetSelect");
        assert_eq!(pd_opt["options"]["effectType"], "DISCARD_HAND");
        assert_eq!(pd_opt["options"]["optional"], true);
        let opt_cands = pd_opt["options"]["candidates"].as_array().unwrap();
        let opt_ids: Vec<&str> = opt_cands.iter().filter_map(|c| c["instanceId"].as_str()).collect();
        assert!(opt_ids.contains(&"h_green_ev"), "【緑】イベントは候補に含まれる");
        assert!(!opt_ids.contains(&"h_keep"), "【青】キャラは候補に含まれない");

        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_opt["decisionId"],
            "revision": engine.revision,
            "value": {"targetId": "h_green_ev", "actorId": "h_pr041", "effectType": "DISCARD_HAND"}
        }));
        let ps1_after_pr = engine.players.get("p1").unwrap();
        assert!(ps1_after_pr.remove.iter().any(|c| c.instance_id == "h_green_ev"));
        // h_keep(1) + d1 + d2 = 3枚
        assert_eq!(ps1_after_pr.hand.len(), 3);

        // 3) B01016 (【登場時】デッキ4枚見て【青】手札に加え、加えた場合手札を1枚リムーブ)
        {
            let ps1_mut = engine.players.get_mut("p1").unwrap();
            ps1_mut.normal_hand_played_this_turn = false;
            ps1_mut.hand.push(test_inst("h_b01016", "B01016", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
            for i in 0..4 {
                ps1_mut.deck.push(test_inst(&format!("ds_{}", i), "B01021", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
            }
        }
        let pd3 = engine.pending_decision.as_ref().unwrap().clone();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd3["decisionId"],
            "revision": engine.revision,
            "value": {"type": "HAND_PLAY", "cardId": "h_b01016"}
        }));
        let pd_ds = engine.pending_decision.as_ref().unwrap().clone();
        assert_eq!(pd_ds["kind"], "deckSearch");
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_ds["decisionId"],
            "revision": engine.revision,
            "value": {"chosenCardId": "ds_0", "actorId": "h_b01016"}
        }));

        // 手札に加えたあと、捨てる手札をプレイヤーが選べる targetSelect (DISCARD_HAND) が開くこと！
        let pd_post_disc = engine.pending_decision.as_ref().unwrap().clone();
        assert_eq!(pd_post_disc["kind"], "targetSelect");
        assert_eq!(pd_post_disc["options"]["effectType"], "DISCARD_HAND");
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_post_disc["decisionId"],
            "revision": engine.revision,
            "value": {"targetId": "ds_0", "actorId": "h_b01016", "effectType": "DISCARD_HAND"}
        }));
        assert!(engine.players.get("p1").unwrap().remove.iter().any(|c| c.instance_id == "ds_0"));
    }
}

