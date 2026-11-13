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
use models::DeckDefinition;
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
        let mut engine = MatchEngine::new(catalog);
        let p2_deck_actual = if is_cpu { default_p2_deck.clone() } else { default_p2_deck.clone() };

        engine.add_player("player1", &p1_deck);
        engine.add_player("player2", &p2_deck_actual);
        engine.start_match();

        let cpu_agent = if is_cpu { Some(RandomAgent::new("player2")) } else { None };

        let mut room = Self {
            room_id: room_id.to_string(),
            engine,
            p2_is_cpu: is_cpu,
            cpu_agent,
            p1_deck,
            p2_deck: if is_cpu { Some(default_p2_deck) } else { None },
            p1_connected: true,
            p2_connected: is_cpu,
            created_at: std::time::Instant::now(),
        };
        room.process_cpu_if_needed();
        room
    }

    fn join_p2(&mut self, catalog: &CardCatalog, p2_deck: DeckDefinition) {
        self.p2_deck = Some(p2_deck.clone());
        self.p2_connected = true;
        self.p2_is_cpu = false;
        self.cpu_agent = None;

        // Restart match with fresh p1 & p2 decks
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
                        Some(agent) => agent.choose_action(dec),
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

        let default_saved = decks_dir.join("default.json");
        if !default_saved.exists() {
            if let Ok(content) = serde_json::to_string_pretty(&deck_def) {
                let _ = std::fs::write(&default_saved, content);
            }
        }

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
        let content = serde_json::to_string_pretty(&new_def).map_err(|e| e.to_string())?;

        std::fs::write(&self.deck_path, &content).map_err(|e| e.to_string())?;

        let deck_id = new_def.deck_id.clone().unwrap_or_else(|| "custom".to_string());
        let safe_id: String = deck_id
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
            .collect();
        let slot_file = self.decks_dir.join(format!("{}.json", safe_id));
        let _ = std::fs::write(slot_file, &content);

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

        let deck = p1_deck.unwrap_or_else(|| self.deck_def.clone());
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

        let p2_deck = body.deck.unwrap_or(default_deck);
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

async fn handle_reset(
    State(state): State<SharedState>,
    Json(body): Json<Option<ResetRequest>>,
) -> impl IntoResponse {
    let mut mgr = state.lock().unwrap();
    let room_id = body
        .and_then(|b| b.room_id)
        .unwrap_or_else(|| "default".to_string());

    let catalog = mgr.catalog.clone();
    if let Some(room) = mgr.rooms.get_mut(&room_id) {
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
}

