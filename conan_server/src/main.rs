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
    match_logged: bool,
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
            match_logged: false,
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
        self.match_logged = false;
        self.flush_logs();
    }

    fn reset_match(&mut self, catalog: &CardCatalog) {
        let mut engine = MatchEngine::new(catalog.clone());
        engine.add_player("player1", &self.p1_deck);
        let p2_deck = self.p2_deck.as_ref().unwrap_or(&self.p1_deck);
        engine.add_player("player2", p2_deck);
        engine.start_match();
        self.engine = engine;
        self.match_logged = false;
        self.process_cpu_if_needed();
        self.flush_logs();
    }

fn send_logs_to_gas_webhook(rows: Vec<Value>) {
    let Ok(url) = std::env::var("GAS_LOG_WEBHOOK_URL") else {
        return;
    };
    let url = url.trim().to_string();
    if url.is_empty() {
        return;
    }
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn(async move {
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build();
            let Ok(client) = client else {
                return;
            };
            let payload = json!({ "rows": rows });
            if let Err(e) = client.post(&url).json(&payload).send().await {
                tracing::warn!("Failed to post logs to GAS webhook: {}", e);
            }
        });
    }
}

    fn flush_logs(&mut self) {
        use std::io::Write;
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        if !self.engine.pending_file_logs.is_empty() {
            let _ = std::fs::create_dir_all("logs");
            let lines: Vec<String> = self.engine.pending_file_logs.drain(..).collect();
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open("logs/all_rooms.log")
            {
                for line in &lines {
                    let _ = writeln!(f, "[{}][room:{}] {}", ts, self.room_id, line);
                }
            }
        }
        if !self.match_logged {
            if let Some(ref winner) = self.engine.winner_id {
                self.match_logged = true;
                let _ = std::fs::create_dir_all("logs");

                let p1_ev = self.engine.players.get("player1").map(|p| p.evidence.len()).unwrap_or(0);
                let p2_ev = self.engine.players.get("player2").map(|p| p.evidence.len()).unwrap_or(0);
                let p1_file = self.engine.players.get("player1").map(|p| p.effective_file_count()).unwrap_or(0);
                let p2_file = self.engine.players.get("player2").map(|p| p.effective_file_count()).unwrap_or(0);
                let first_player = self.engine.player_order.first().cloned().unwrap_or_else(|| "player1".to_string());

                // カード定義IDを「ID:カード名(Lv.X)」に変換するヘルパー
                let fmt_def = |def_id: &str| -> String {
                    if let Some(d) = self.engine.catalog.get(def_id) {
                        let name = d.names.first().map(|s| s.as_str()).unwrap_or(def_id);
                        format!("{}:{}(Lv.{})", def_id, name, d.level)
                    } else {
                        def_id.to_string()
                    }
                };

                // 1. デッキレシピの集計サマリーを作成するヘルパー
                let build_deck_analytics = |deck: &DeckDefinition| -> Value {
                    let mut level_curve: HashMap<i32, i32> = HashMap::new();
                    let mut cards_map = serde_json::Map::new();
                    for item in &deck.main_deck {
                        cards_map.insert(fmt_def(&item.definition_id), json!(item.count));
                        if let Some(d) = self.engine.catalog.get(&item.definition_id) {
                            *level_curve.entry(d.level).or_insert(0) += item.count;
                        }
                    }
                    json!({
                        "deckId": deck.deck_id,
                        "partner": fmt_def(&deck.partner_id),
                        "case": fmt_def(&deck.case_id),
                        "cards": cards_map,
                        "levelCurve": level_curve,
                    })
                };

                // 2. 試合終了時の各ゾーン（手札残り＝腐った札、現場、リムーブ、証拠、FILE）を集計するヘルパー
                let build_zone_analytics = |pid: &str, deck: &DeckDefinition| -> Value {
                    let Some(ps) = self.engine.players.get(pid) else {
                        return json!({});
                    };
                    let to_list = |cards: &[models::CardInstance]| -> Vec<String> {
                        cards.iter().map(|c| fmt_def(&c.definition_id)).collect()
                    };

                    // デッキ内にあったカードのうち、山札(deck)に残らず一度でも引いた・絡んだカードを算出
                    let mut remaining_deck_counts: HashMap<String, i32> = HashMap::new();
                    for c in &ps.deck {
                        *remaining_deck_counts.entry(c.definition_id.clone()).or_insert(0) += 1;
                    }
                    let mut seen_cards: Vec<String> = Vec::new();
                    for item in &deck.main_deck {
                        let rem = remaining_deck_counts.get(&item.definition_id).copied().unwrap_or(0);
                        let drawn_count = (item.count - rem).max(0);
                        for _ in 0..drawn_count {
                            seen_cards.push(fmt_def(&item.definition_id));
                        }
                    }

                    json!({
                        "field": to_list(&ps.field),
                        "endHand": to_list(&ps.hand),       // 最後まで使えず手札に残ったカード（事故要因の特定用）
                        "remove": to_list(&ps.remove),      // 使用済みイベント・リムーブされたカード
                        "evidence": to_list(&ps.evidence),
                        "file": to_list(&ps.file),
                        "seenCards": seen_cards,            // この試合中に山札から離れた（引いた/めくった）全カード
                    })
                };

                let p2_deck_ref = self.p2_deck.as_ref().unwrap_or(&self.p1_deck);
                let p1_deck_info = build_deck_analytics(&self.p1_deck);
                let p2_deck_info = build_deck_analytics(p2_deck_ref);
                let p1_zones = build_zone_analytics("player1", &self.p1_deck);
                let p2_zones = build_zone_analytics("player2", p2_deck_ref);

                let cpu_diff_str = if self.p2_is_cpu {
                    format!("{:?}", self.cpu_agent.as_ref().map(|a| a.difficulty).unwrap_or_default()).to_lowercase()
                } else {
                    "none".to_string()
                };

                // ローカルの logs/matches.jsonl に完全な構造化データを出力
                let rec = json!({
                    "timestamp": ts,
                    "roomId": self.room_id,
                    "p2IsCpu": self.p2_is_cpu,
                    "cpuDifficulty": cpu_diff_str,
                    "firstPlayer": first_player,
                    "winner": winner,
                    "finishReason": self.engine.finish_reason,
                    "turn": self.engine.turn,
                    "p1Evidence": p1_ev,
                    "p2Evidence": p2_ev,
                    "p1File": p1_file,
                    "p2File": p2_file,
                    "p1Deck": p1_deck_info,
                    "p2Deck": p2_deck_info,
                    "p1Zones": p1_zones,
                    "p2Zones": p2_zones,
                });
                if let Ok(mut f) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open("logs/matches.jsonl")
                {
                    let _ = writeln!(f, "{}", rec);
                }

                // 試合終了時にその試合の全ログと試合結果をまとめて GAS Webhook へ送信
                let mut webhook_rows: Vec<Value> = self
                    .engine
                    .logs
                    .iter()
                    .map(|line| {
                        json!({
                            "timestamp": ts,
                            "roomId": self.room_id,
                            "type": "LOG",
                            "message": line,
                        })
                    })
                    .collect();

                let format_field = |pid: &str| -> String {
                    self.engine.players.get(pid).map(|ps| {
                        ps.field.iter().map(|c| {
                            let def = self.engine.catalog.get(&c.definition_id);
                            let name = def.and_then(|d| d.names.first()).map(|s| s.as_str()).unwrap_or(&c.definition_id);
                            let ap = self.engine.get_effective_ap(pid, c);
                            let lp = def.map(|d| d.lp).unwrap_or(1);
                            format!("{}:{}(AP:{},LP:{})", c.definition_id, name, ap, lp)
                        }).collect::<Vec<_>>().join(",")
                    }).unwrap_or_default()
                };

                let format_hand = |pid: &str| -> String {
                    self.engine.players.get(pid).map(|ps| {
                        ps.hand.iter().map(|c| fmt_def(&c.definition_id)).collect::<Vec<_>>().join(",")
                    }).unwrap_or_default()
                };

                let format_deck_compact = |deck: &DeckDefinition| -> String {
                    let main_str = deck.main_deck.iter()
                        .map(|item| format!("{}x{}", item.definition_id, item.count))
                        .collect::<Vec<_>>()
                        .join(",");
                    format!("P:{}/C:{}/M:[{}]", deck.partner_id, deck.case_id, main_str)
                };

                // 既存の MATCH 行に p1_hand, p2_hand, p1_deck, p2_deck を追加
                webhook_rows.push(json!({
                    "timestamp": ts,
                    "roomId": self.room_id,
                    "type": "MATCH",
                    "message": format!(
                        "winner={} first={} reason={:?} turn={} p1_ev={} p2_ev={} p1_file={} p2_file={} cpu={} cpu_diff={} p1_field=[{}] p2_field=[{}] p1_hand=[{}] p2_hand=[{}] p1_deck={} p2_deck={}",
                        winner,
                        first_player,
                        self.engine.finish_reason,
                        self.engine.turn,
                        p1_ev,
                        p2_ev,
                        p1_file,
                        p2_file,
                        self.p2_is_cpu,
                        cpu_diff_str,
                        format_field("player1"),
                        format_field("player2"),
                        format_hand("player1"),
                        format_hand("player2"),
                        format_deck_compact(&self.p1_deck),
                        format_deck_compact(p2_deck_ref)
                    ),
                }));
                Self::send_logs_to_gas_webhook(webhook_rows);
            }
        }
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
        self.flush_logs();
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
    difficulty: Option<agent::CpuDifficulty>,
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
    if room_id.starts_with("solo_") && !mgr.rooms.contains_key(&room_id) {
        let solo_room = MatchRoom::new(&room_id, mgr.catalog.clone(), mgr.deck_def.clone(), true, mgr.deck_def.clone());
        mgr.rooms.insert(room_id.clone(), solo_room);
    }
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
    obs["cpuDifficulty"] = json!(room.cpu_agent.as_ref().map(|a| a.difficulty).unwrap_or_default());
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
    if safe_id.starts_with("preset_") {
        return (StatusCode::FORBIDDEN, Json(json!({"error": "プリセットデッキは削除できません"}))).into_response();
    }
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
        let prev_diff = room.cpu_agent.as_ref().map(|a| a.difficulty).unwrap_or_default();
        let diff = body.difficulty.unwrap_or(prev_diff);
        room.p2_is_cpu = body.is_cpu;
        room.cpu_agent = if body.is_cpu {
            Some(RandomAgent::with_difficulty("player2", diff))
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

    // Validate maximum 3 copies per canonical ID (card_id), unless the card text allows unlimited copies
    let mut canonical_counts: HashMap<String, i32> = HashMap::new();
    for item in &body.main_deck {
        let card_def = mgr_read.catalog.get(&item.definition_id);
        let is_unlimited = card_def
            .map(|d| d.description.contains("デッキに何枚でも入れることができる"))
            .unwrap_or(false);
        if is_unlimited {
            continue;
        }
        let canonical_id = card_def
            .map(|d| d.canonical_id.clone())
            .unwrap_or_else(|| item.definition_id.clone());

        let count = canonical_counts.entry(canonical_id.clone()).or_insert(0);
        *count += item.count;
        if *count > 3 {
            let card_title = card_def
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

#[derive(Deserialize)]
struct ImportOfficialDeckRequest {
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

fn parse_official_deck_html(html: &str) -> Result<serde_json::Value, String> {
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

async fn handle_import_official_deck(
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

    let args: Vec<String> = std::env::args().collect();
    let mgr = MatchManager::new().expect("MatchManager 初期化に失敗しました");

    if args.get(1).map(|s| s.as_str()) == Some("train") {
        let generations: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(10);
        let games_per_gen: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(20);
        let p1_deck = if let Some(path) = args.get(4) {
            match MatchManager::load_deck(&PathBuf::from(path)) {
                Ok(d) => {
                    println!("📘 P1デッキを読み込みました: {} ({})", d.deck_name.as_deref().unwrap_or("名称未設定"), path);
                    d
                }
                Err(e) => {
                    eprintln!("❌ P1デッキの読み込みに失敗しました ({}): {}", path, e);
                    return Err(e);
                }
            }
        } else {
            mgr.deck_def.clone()
        };
        let p2_deck = if let Some(path) = args.get(5) {
            match MatchManager::load_deck(&PathBuf::from(path)) {
                Ok(d) => {
                    println!("📕 P2(環境)デッキを読み込みました: {} ({})", d.deck_name.as_deref().unwrap_or("名称未設定"), path);
                    d
                }
                Err(e) => {
                    eprintln!("❌ P2デッキの読み込みに失敗しました ({}): {}", path, e);
                    return Err(e);
                }
            }
        } else {
            p1_deck.clone()
        };
        agent::train_cpu_self_play(
            &mgr.catalog,
            &p1_deck,
            &p2_deck,
            generations,
            games_per_gen,
            "data/cpu_weights.json",
        );
        return Ok(());
    }

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
        .route("/api/deck/import_official", post(handle_import_official_deck))
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
                let card_def = catalog.get(&item.definition_id);
                let is_unlimited = card_def
                    .map(|d| d.description.contains("デッキに何枚でも入れることができる"))
                    .unwrap_or(false);
                if is_unlimited {
                    continue;
                }
                let canonical_id = card_def
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

        // 0627 (犯人) allows unlimited copies -> 10 copies should succeed
        if let Some(hannin_def) = catalog.definitions.values().find(|d| d.canonical_id == "0627") {
            let unlimited_items = vec![
                DeckCardItem { definition_id: hannin_def.definition_id.clone(), count: 10 },
            ];
            assert!(validate_deck_counts(&unlimited_items).is_ok());
        }

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
            ..Default::default()
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
                ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
                ..Default::default()
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
            ..Default::default()
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
                ..Default::default()
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
            ..Default::default()
        });

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        // 1. Trigger resolution transition -> prompts hand discard, then hand size goes from 1 to 0
        engine.check_resolution_transition("p1");
        assert_eq!(engine.players.get("p1").unwrap().case_phase, models::CasePhase::RESOLUTION);
        let discard_pd = engine.pending_decision.as_ref().unwrap().clone();
        assert_eq!(discard_pd["kind"], "targetSelect");
        assert_eq!(discard_pd["options"]["effectType"], "DISCARD_HAND");
        let discard_dec_id = discard_pd["decisionId"].as_str().unwrap().to_string();
        let target_hand_id = discard_pd["options"]["candidates"][0]["instanceId"].as_str().unwrap().to_string();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": discard_dec_id,
            "revision": engine.revision,
            "value": {"targetId": target_hand_id, "effectType": "DISCARD_HAND"}
        }));
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

        // Fill p1 field to max 5 chars (cannot play char, but event is still playable once per turn)
        ps1.normal_hand_played_this_turn = false;
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
                ..Default::default()
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
                ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
    fn test_hirameki_extended_ops_and_trigger_isolation() {
        let non_hirameki = "【登場時】自分のリムーブエリアにある【カットイン】と【ヒラメキ】以外の元の能力を持たないキャラを1枚まで選び、登場させる。";
        assert!(effects::extract_clauses(non_hirameki, effects::TriggerKind::Hirameki, models::CardType::CHARACTER).is_empty());

        let p_hand = effects::parse_effect_steps("【ヒラメキ】このカードを手札に加える。", None, models::CardType::CHARACTER);
        assert!(p_hand.immediate_before.iter().any(|op| matches!(op, effects::ImmediateOp::MoveSelfFromEvidenceToHand)));

        let p_partner = effects::parse_effect_steps("【ヒラメキ】このカードをパートナーエリアに移す。", None, models::CardType::CHARACTER);
        assert!(p_partner.immediate_before.iter().any(|op| matches!(op, effects::ImmediateOp::MoveSelfFromEvidenceToPartner)));

        let p_opp_disc = effects::parse_effect_steps("【ヒラメキ】相手は手札を1枚リムーブする。", None, models::CardType::CHARACTER);
        assert!(p_opp_disc.immediate_before.iter().any(|op| matches!(op, effects::ImmediateOp::OppDiscardHand(1))));

        let p_opp_mill = effects::parse_effect_steps("【ヒラメキ】相手のデッキのカードを上から4枚リムーブする。", None, models::CardType::CHARACTER);
        assert!(p_opp_mill.immediate_before.iter().any(|op| matches!(op, effects::ImmediateOp::OppMillDeck(4))));

        let p_opp_flip = effects::parse_effect_steps("【ヒラメキ】相手の裏向きの証拠を1つまで選び、表向きにする。", None, models::CardType::CHARACTER);
        assert!(p_opp_flip.immediate_before.iter().any(|op| matches!(op, effects::ImmediateOp::OppFlipEvidenceUp(1))));
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

        // 追加検証: FILE1枚 + パートナーアシスト(実効FILE2)・手札にキャラ0枚の状態でD01014を使い、デッキからLv2青キャラ(B01019)を加えてそのまま登場できること
        let mut engine2 = engine::MatchEngine::new(load_test_catalog());
        engine2.phase = models::MatchPhase::MAIN;
        engine2.turn_player_id = "p1".to_string();
        engine2.player_order = vec!["p1".to_string(), "p2".to_string()];
        let mut p1_s = engine::PlayerState::new("p1".to_string());
        let p2_s = engine::PlayerState::new("p2".to_string());
        p1_s.r#case = Some(test_inst("p1_case", "B01002", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
        p1_s.file.push(test_inst("f2_0", "B01021", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
        p1_s.partner = Some(test_inst("p1_prt", "B01001", "p1", models::Zone::FILE, models::CardOrientation::SLEEP)); // パートナーのアシスト中（実効FILE = 2）
        p1_s.deck.push(test_inst("d2_0", "B01020", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        p1_s.deck.push(test_inst("d2_1", "B01020", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        p1_s.deck.push(test_inst("d2_lv2", "B01019", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        p1_s.hand.push(test_inst("h2_ev", "D01014", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        engine2.players.insert("p1".to_string(), p1_s);
        engine2.players.insert("p2".to_string(), p2_s);

        engine2.prompt_main_action("p1");
        let pd2_1 = engine2.pending_decision.as_ref().unwrap().clone();
        let _ = engine2.submit_decision("p1", &serde_json::json!({
            "decisionId": pd2_1["decisionId"],
            "revision": engine2.revision,
            "value": {"type": "HAND_PLAY", "cardId": "h2_ev"}
        }));
        let pd2_ds = engine2.pending_decision.as_ref().unwrap().clone();
        assert_eq!(pd2_ds["kind"], "deckSearch");
        let _ = engine2.submit_decision("p1", &serde_json::json!({
            "decisionId": pd2_ds["decisionId"],
            "revision": engine2.revision,
            "value": {"chosenCardId": "d2_lv2", "actorId": "h2_ev"}
        }));
        let pd2_ts = engine2.pending_decision.as_ref().expect("FILE2でも手札に加えたLv2キャラが登場選択に出るべき").clone();
        assert_eq!(pd2_ts["kind"], "targetSelect");
        assert_eq!(pd2_ts["options"]["effectType"], "PLAY_FROM_HAND");
        let cands2 = pd2_ts["options"]["candidates"].as_array().unwrap();
        assert!(cands2.iter().any(|c| c["instanceId"] == "d2_lv2"), "手札に加えたLv2キャラが候補に含まれる");
        let _ = engine2.submit_decision("p1", &serde_json::json!({
            "decisionId": pd2_ts["decisionId"],
            "revision": engine2.revision,
            "value": {"targetId": "d2_lv2", "actorId": "h2_ev", "effectType": "PLAY_FROM_HAND"}
        }));
        assert!(engine2.players.get("p1").unwrap().field.iter().any(|c| c.instance_id == "d2_lv2"));
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

    #[test]
    fn test_contact_disguise_and_cutin_limit() {
        let mut catalog = CardCatalog::load_from_json(std::path::Path::new("../data/dummy/cards.json"))
            .expect("catalog load failed");

        catalog.definitions.insert("T_BASE".to_string(), models::CardDefinition {
            definition_id: "T_BASE".to_string(),
            canonical_id: "T_BASE".to_string(),
            card_type: models::CardType::CHARACTER,
            colors: vec!["青".to_string()],
            level: 2,
            ap: 2000,
            lp: 1,
            names: vec!["ベースキャラ".to_string()],
            description: String::new(),
            traits: vec![],
            case_levels: None,
            main_thumb: None,
            main_path: None,
        });
        catalog.definitions.insert("T_DISG".to_string(), models::CardDefinition {
            definition_id: "T_DISG".to_string(),
            canonical_id: "T_DISG".to_string(),
            card_type: models::CardType::CHARACTER,
            colors: vec!["青".to_string()],
            level: 2,
            ap: 5000,
            lp: 1,
            names: vec!["変装キャラ".to_string()],
            description: "【変装】自分のキャラ1人と入れ替える。".to_string(),
            traits: vec![],
            case_levels: None,
            main_thumb: None,
            main_path: None,
        });
        catalog.definitions.insert("T_CUTIN".to_string(), models::CardDefinition {
            definition_id: "T_CUTIN".to_string(),
            canonical_id: "T_CUTIN".to_string(),
            card_type: models::CardType::CHARACTER,
            colors: vec!["青".to_string()],
            level: 1,
            ap: 1000,
            lp: 1,
            names: vec!["カットインキャラ".to_string()],
            description: "【カットイン】自分のキャラ1人のAP+1000。".to_string(),
            traits: vec![],
            case_levels: None,
            main_thumb: None,
            main_path: None,
        });

        let mut engine = MatchEngine::new(catalog);
        engine.turn = 3;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];
        engine.phase = models::MatchPhase::MAIN;

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let mut ps2 = engine::PlayerState::new("p2".to_string());

        let mk = |id: &str, def: &str, owner: &str, zone: models::Zone, ori: models::CardOrientation| {
            models::CardInstance {
                instance_id: id.to_string(),
                definition_id: def.to_string(),
                owner_id: owner.to_string(),
                zone,
                orientation: ori,
                entered_turn: 1,
                face_up: true,
                temp_ap_bonus: 0,
                has_rush: false,
                rush_target: None,
                used_declare_this_turn: false,
                ..Default::default()
            }
        };

        for i in 0..5 {
            ps1.file.push(mk(&format!("f1_{}", i), "T_BASE", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
            ps2.file.push(mk(&format!("f2_{}", i), "T_BASE", "p2", models::Zone::FILE, models::CardOrientation::ACTIVE));
        }
        ps2.deck.push(mk("d2_top", "T_BASE", "p2", models::Zone::DECK, models::CardOrientation::ACTIVE));

        ps1.field.push(mk("p1_atk", "T_BASE", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps2.field.push(mk("p2_def", "T_BASE", "p2", models::Zone::FIELD, models::CardOrientation::SLEEP));
        ps2.field.push(mk("p2_guard", "T_BASE", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps2.hand.push(mk("p2_disg", "T_DISG", "p2", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps2.hand.push(mk("p2_cutin", "T_CUTIN", "p2", models::Zone::HAND, models::CardOrientation::ACTIVE));

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        // 1. MAINフェイズにはDISGUISEが含まれない
        engine.prompt_main_action("p1");
        let pd_main = engine.pending_decision.clone().unwrap();
        let actions = pd_main["options"]["actions"].as_array().unwrap();
        assert!(!actions.iter().any(|a| a["type"] == "DISGUISE"));

        // 2. p1がp2_defへコンタクト
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_main["decisionId"],
            "revision": engine.revision,
            "value": {"type": "ACTION", "actorId": "p1_atk", "targetId": "p2_def"}
        }));

        // 3. まずp2のガードフェーズ（ガードをスルー）
        let pd_guard = engine.pending_decision.clone().unwrap();
        assert_eq!(pd_guard["kind"], "guard");
        assert_eq!(pd_guard["playerId"], "p2");
        let _ = engine.submit_decision("p2", &serde_json::json!({
            "decisionId": pd_guard["decisionId"],
            "revision": engine.revision,
            "value": {}
        }));

        // 4. カットインフェーズでp2にdisguiseOptionsとcutinCardIdsの両方が提示される
        let pd_cutin = engine.pending_decision.clone().unwrap();
        assert_eq!(pd_cutin["kind"], "guard");
        assert_eq!(pd_cutin["playerId"], "p2");
        assert_eq!(pd_cutin["options"]["disguiseTargetId"], "p2_def");
        assert!(!pd_cutin["options"]["disguiseOptions"].as_array().unwrap().is_empty());
        assert!(!pd_cutin["options"]["cutinCardIds"].as_array().unwrap().is_empty());

        // 5. p2が変装を使用
        let _ = engine.submit_decision("p2", &serde_json::json!({
            "decisionId": pd_cutin["decisionId"],
            "revision": engine.revision,
            "value": {"disguiseCardId": "p2_disg", "targetId": "p2_def"}
        }));

        // p2_defはデッキの一番下(index 0)へ移動し、p2_disgが場に出る
        let ps2_after = engine.players.get("p2").unwrap();
        assert_eq!(ps2_after.deck[0].instance_id, "p2_def");
        assert!(ps2_after.field.iter().any(|c| c.instance_id == "p2_disg"));

        // 6. カットイン・変装は各プレイヤー1回までのため、p1にカットインがなければバトル解決してmainActionへ戻る
        let pd_after = engine.pending_decision.clone().unwrap();
        assert_eq!(pd_after["kind"], "mainAction");
    }

    #[test]
    fn test_b05005_next_hint_blue_trigger_and_independent_turn1_declare() {
        let mut catalog = load_test_catalog();
        // Ensure a known non-blue opponent character (AP 8000) and a non-blue card in hand exist
        let b05005_lv = catalog.get("B05005").unwrap().level;
        assert!(b05005_lv == 9);
        catalog.definitions.insert(
            "TEST_NONBLUE_8000".to_string(),
            models::CardDefinition {
                definition_id: "TEST_NONBLUE_8000".to_string(),
                canonical_id: "TEST_NONBLUE_8000".to_string(),
                card_type: models::CardType::CHARACTER,
                colors: vec!["緑".to_string()],
                level: 7,
                ap: 8000,
                lp: 1,
                names: vec!["テスト緑8000".to_string()],
                description: String::new(),
                traits: vec![],
                case_levels: None,
                main_thumb: None,
                main_path: None,
            },
        );
        catalog.definitions.insert(
            "TEST_NONBLUE_LOW".to_string(),
            models::CardDefinition {
                definition_id: "TEST_NONBLUE_LOW".to_string(),
                canonical_id: "TEST_NONBLUE_LOW".to_string(),
                card_type: models::CardType::CHARACTER,
                colors: vec!["青".to_string()], // Blue only for playing via Blue case, separate test for non-blue below
                level: 2,
                ap: 2000,
                lp: 1,
                names: vec!["テスト青2".to_string()],
                description: String::new(),
                traits: vec![],
                case_levels: None,
                main_thumb: None,
                main_path: None,
            },
        );
        // Check color matching directly: green hint must NOT match B05005's 【青】のカードを使用したとき
        let b05005_desc = catalog.get("B05005").unwrap().description.clone();
        let hint_clauses = effects::extract_clauses(&b05005_desc, effects::TriggerKind::OnHintUse, models::CardType::CHARACTER);
        assert_eq!(hint_clauses.len(), 1);
        assert!(!effects::check_hint_use_match(&hint_clauses[0], &["緑".to_string()]));
        assert!(effects::check_hint_use_match(&hint_clauses[0], &["青".to_string()]));

        let mut engine = MatchEngine::new(catalog);
        engine.turn = 4;
        engine.phase = models::MatchPhase::MAIN;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let mut ps2 = engine::PlayerState::new("p2".to_string());
        ps1.r#case = Some(test_inst("p1_case", "B01002", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
        ps1.partner = Some(test_inst("p1_partner", "B01001", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
        ps1.field.push(test_inst("p1_b05005", "B05005", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        for i in 0..5 {
            ps1.file.push(test_inst(&format!("f1_{}", i), "B01021", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
        }
        ps1.hand.push(test_inst("h_blue_1", "TEST_NONBLUE_LOW", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("h_blue_2", "TEST_NONBLUE_LOW", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));

        ps2.field.push(test_inst("opp_g8000", "TEST_NONBLUE_8000", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps2.deck.push(test_inst("opp_dtop", "TEST_NONBLUE_LOW", "p2", models::Zone::DECK, models::CardOrientation::ACTIVE));

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        // 1. ネクストヒントでFILEから手札に加え、【青】カード(TEST_NONBLUE_LOW)を使用
        engine.prompt_main_action("p1");
        let pd1 = engine.pending_decision.as_ref().unwrap().clone();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd1["decisionId"],
            "revision": engine.revision,
            "value": {"type": "HINT_TAKE"}
        }));
        let pd_hint = engine.pending_decision.as_ref().unwrap().clone();
        assert_eq!(pd_hint["kind"], "hintPlay");
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_hint["decisionId"],
            "revision": engine.revision,
            "value": {"cardId": "h_blue_1"}
        }));

        // 2. B05005の【パートナー青】【ターン1】が誘発し、AP8000以下の非青キャラ(opp_g8000)もデッキボトム対象として選べる！
        let pd_ts = engine.pending_decision.as_ref().expect("B05005 OnHintUse targetSelect must open");
        assert_eq!(pd_ts["kind"], "targetSelect");
        assert_eq!(pd_ts["options"]["effectType"], "BOTTOM_DECK");
        let candidates = pd_ts["options"]["candidates"].as_array().unwrap();
        assert!(candidates.iter().any(|c| c["instanceId"] == "opp_g8000"));
        let ts_id = pd_ts["decisionId"].as_str().unwrap().to_string();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": ts_id,
            "revision": engine.revision,
            "value": {"targetId": "opp_g8000", "actorId": "p1_b05005", "effectType": "BOTTOM_DECK"}
        }));

        let ps2_after = engine.players.get("p2").unwrap();
        assert!(ps2_after.field.is_empty());
        assert_eq!(ps2_after.deck[0].instance_id, "opp_g8000");

        // 3. 同じターン内でも B05005 の【宣言】【ターン1】はまだ未使用であるため、DECLARE候補に残る（共有バグが直っていること）
        let pd_main2 = engine.pending_decision.as_ref().unwrap();
        let actions2 = pd_main2["options"]["actions"].as_array().unwrap();
        let decl_opt = actions2.iter().find(|a| a["type"] == "DECLARE").expect("DECLARE action must still be available on B05005");
        let decl_actors = decl_opt["actors"].as_array().unwrap();
        assert!(decl_actors.iter().any(|a| a["actorId"] == "p1_b05005"));

        // 4. 同じターンにもう一度青ヒント使用を呼び出しても【ターン1】により再発動しない
        assert!(!engine.trigger_on_hint_use_with_colors("p1", 2, &["青".to_string()]));
    }

    #[test]
    fn test_passive_aura_ap_and_partner_lp_effects() {
        let mut catalog = load_test_catalog();
        catalog.definitions.insert(
            "TEST_TAKAGI_GREEN".to_string(),
            models::CardDefinition {
                definition_id: "TEST_TAKAGI_GREEN".to_string(),
                canonical_id: "TEST_TAKAGI_GREEN".to_string(),
                card_type: models::CardType::CHARACTER,
                colors: vec!["黄".to_string()],
                level: 4,
                ap: 4000,
                lp: 1,
                names: vec!["高木渉".to_string()],
                description: String::new(),
                traits: vec!["警察".to_string(), "警視庁".to_string()],
                case_levels: None,
                main_thumb: None,
                main_path: None,
            },
        );

        let mut engine = MatchEngine::new(catalog);
        engine.turn = 4;
        engine.phase = models::MatchPhase::MAIN;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let ps2 = engine::PlayerState::new("p2".to_string());
        // 緑パートナー D02001
        ps1.partner = Some(test_inst("p1_p", "D02001", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
        // B01038 (自分ターン中、このキャラ以外の【緑】キャラAP+1000)
        // B01032 (緑 Lv3 base AP=3000)
        // B01027 (緑 Lv7 base LP=0, 【パートナー緑】LP+1)
        // B02012 (自分ターン中、自分の現場にいるレベル5以下の【青】キャラをAP+1000, 自身Lv7 base AP=6000)
        // B01011 (青 Lv4 base AP=2000)
        ps1.field.push(test_inst("c_b01038", "B01038", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.field.push(test_inst("c_b01032", "B01032", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.field.push(test_inst("c_b01027", "B01027", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.field.push(test_inst("c_b02012", "B02012", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.field.push(test_inst("c_b01011", "B01011", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.deck.push(test_inst("p1_d1", "B01021", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        {
            let ps1_ref = engine.players.get("p1").unwrap();
            let c38 = &ps1_ref.field[0];
            let c32 = &ps1_ref.field[1];
            let c27 = &ps1_ref.field[2];
            let c2012 = &ps1_ref.field[3];
            let c11 = &ps1_ref.field[4];

            let b01038_base = engine.catalog.get("B01038").unwrap().ap;
            assert_eq!(engine.get_effective_ap("p1", c38), b01038_base); // 「このキャラ以外の」なので自身は増えない
            assert_eq!(engine.get_effective_ap("p1", c32), 4000); // 3000 + B01038(+1000)
            assert_eq!(engine.get_effective_ap("p1", c27), 6000); // 5000 + B01038(+1000)
            assert_eq!(engine.get_effective_ap("p1", c2012), 6000); // Lv7なのでB02012のLv5以下バフは対象外
            assert_eq!(engine.get_effective_ap("p1", c11), 3000); // Lv4青: 2000 + B02012(+1000)

            assert_eq!(engine.get_effective_lp("p1", c27), 1); // 【パートナー緑】LP+1 (base 0 -> 1)
        }

        // B01027 で DEDUCE を実行すると LP 1 分の証拠を獲得する
        engine.prompt_main_action("p1");
        let pd1 = engine.pending_decision.as_ref().unwrap().clone();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd1["decisionId"],
            "revision": engine.revision,
            "value": {"type": "DEDUCE", "actorId": "c_b01027"}
        }));
        assert_eq!(engine.players.get("p1").unwrap().evidence.len(), 1);

        // 相手ターンになると【自分ターン中】オーラが消える
        engine.turn_player_id = "p2".to_string();
        let ps1_ref = engine.players.get("p1").unwrap();
        assert_eq!(engine.get_effective_ap("p1", &ps1_ref.field[1]), 3000);
        assert_eq!(engine.get_effective_ap("p1", &ps1_ref.field[4]), 2000);

        // パートナーエリア有効能力 (B08062: すべてが佐藤美和子/高木渉なら全味方AP+1000)
        engine.turn_player_id = "p1".to_string();
        {
            let ps1_mut = engine.players.get_mut("p1").unwrap();
            ps1_mut.partner = Some(test_inst("p1_b08062", "B08062", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
            ps1_mut.field = vec![test_inst("c_takagi", "TEST_TAKAGI_GREEN", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE)];
        }
        let ps1_ref2 = engine.players.get("p1").unwrap();
        assert_eq!(engine.get_effective_ap("p1", &ps1_ref2.field[0]), 5000); // 4000 + B08062 in Partner(+1000)
    }

    #[test]
    fn test_on_action_variants_char_case_and_deduce() {
        let catalog = load_test_catalog();
        let mut engine = MatchEngine::new(catalog);
        engine.turn = 4;
        engine.phase = models::MatchPhase::MAIN;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let mut ps2 = engine::PlayerState::new("p2".to_string());
        // 赤パートナー B01060
        ps1.partner = Some(test_inst("p1_p", "B01060", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
        ps2.r#case = Some(test_inst("p2_case", "B01002", "p2", models::Zone::CASE, models::CardOrientation::ACTIVE));
        ps2.evidence.push(test_inst("p2_ev1", "B01021", "p2", models::Zone::EVIDENCE, models::CardOrientation::ACTIVE));
        ps2.field.push(test_inst("opp_sleep", "B01021", "p2", models::Zone::FIELD, models::CardOrientation::SLEEP)); // Lv3 < Lv7
        ps2.field.push(test_inst("opp_guard", "B01021", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE)); // ACTIVEなのでガード権あり
        ps1.deck.push(test_inst("d1", "B01021", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));

        // B01032: 【ターン1】このキャラがアクション［キャラ］したときAP+1000
        // B01062: 【パートナー赤】【ターン1】自分の現場にいるキャラがアクション［事件］したときLv7以下リムーブ ＋ 【宣言】【ターン1】別枠
        ps1.field.push(test_inst("c_b01032", "B01032", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.field.push(test_inst("c_b01062", "B01062", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        // 1. B01032 が相手キャラ (opp_sleep) にアクション［キャラ］ -> B01032 APが +1000 される (3000 -> 4000)
        //    B01062 の アクション［事件］トリガーは発動しない
        engine.prompt_main_action("p1");
        let pd1 = engine.pending_decision.as_ref().unwrap().clone();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd1["decisionId"],
            "revision": engine.revision,
            "value": {"type": "ACTION", "actorId": "c_b01032", "targetId": "opp_sleep"}
        }));
        let pd_guard = engine.pending_decision.as_ref().unwrap().clone();
        assert_eq!(pd_guard["kind"], "guard");
        assert_eq!(engine.players.get("p1").unwrap().field[0].temp_ap_bonus, 1000);

        // ガード/カットインをパスして解決へ
        for _ in 0..4 {
            let pd = match engine.pending_decision.as_ref() {
                Some(d) if d["kind"] == "guard" => d.clone(),
                _ => break,
            };
            let pid = pd["playerId"].as_str().unwrap().to_string();
            let _ = engine.submit_decision(&pid, &serde_json::json!({
                "decisionId": pd["decisionId"],
                "revision": engine.revision,
                "value": {"pass": true}
            }));
        }

        // 2. 今度は c_b01032 をアクティブに戻し、別のLv4青(B01011)を出してから相手事件へアクション［事件］
        {
            let ps1_m = engine.players.get_mut("p1").unwrap();
            ps1_m.field.push(test_inst("c_atk_case", "B01011", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
            ps1_m.field.last_mut().unwrap().entered_turn = 1;
            let ps2_m = engine.players.get_mut("p2").unwrap();
            ps2_m.field.push(test_inst("opp_lv3", "B01021", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        }
        engine.prompt_main_action("p1");
        let pd2 = engine.pending_decision.as_ref().unwrap().clone();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd2["decisionId"],
            "revision": engine.revision,
            "value": {"type": "ACTION", "actorId": "c_atk_case", "targetId": "p2_case"}
        }));

        // B01062 の【パートナー赤】【ターン1】が誘発し、targetSelect (REMOVE) が開く！
        let pd_ts = engine.pending_decision.as_ref().expect("B01062 OnAction(Case) must prompt targetSelect");
        assert_eq!(pd_ts["kind"], "targetSelect");
        assert_eq!(pd_ts["options"]["effectType"], "REMOVE");
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_ts["decisionId"],
            "revision": engine.revision,
            "value": {"targetId": "opp_lv3", "actorId": "c_b01062", "effectType": "REMOVE"}
        }));

        // opp_lv3 はリムーブされ、そのまま事件アタックのカットイン/ガード確認へ進む
        assert!(engine.players.get("p2").unwrap().field.iter().all(|c| c.instance_id != "opp_lv3"));
        assert_eq!(engine.pending_decision.as_ref().unwrap()["kind"], "guard");
    }

    #[test]
    fn test_on_ally_play_triggers_b11003_and_d02005() {
        let catalog = load_test_catalog();
        let mut engine = MatchEngine::new(catalog);
        engine.turn = 4;
        engine.phase = models::MatchPhase::MAIN;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let mut ps2 = engine::PlayerState::new("p2".to_string());
        ps1.partner = Some(test_inst("p1_p", "B01001", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
        ps1.r#case = Some(test_inst("p1_case", "B01002", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
        for i in 0..5 {
            ps1.file.push(test_inst(&format!("f{}", i), "B01021", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
        }
        // B11003: 【パートナー青】【自分ターン中】【ターン1】レベル4の〚特徴［少年探偵団］〛のキャラが自分の現場に登場したとき、AP8000以下のキャラを1枚まで選び、リムーブする。
        ps1.field.push(test_inst("c_b11003", "B11003", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("h_b01011", "B01011", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE)); // Lv4 青 少年探偵団
        ps2.field.push(test_inst("opp_target", "B01021", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE));

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        engine.prompt_main_action("p1");
        let pd1 = engine.pending_decision.as_ref().unwrap().clone();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd1["decisionId"],
            "revision": engine.revision,
            "value": {"type": "HAND_PLAY", "cardId": "h_b01011"}
        }));

        // B11003 の登場時ウォッチャー効果が発火し targetSelect (REMOVE) になる
        let pd_ts = engine.pending_decision.as_ref().expect("B11003 OnAllyPlay must prompt targetSelect");
        assert_eq!(pd_ts["kind"], "targetSelect", "logs: {:?}", engine.logs);
        assert_eq!(pd_ts["options"]["effectType"], "REMOVE");
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_ts["decisionId"],
            "revision": engine.revision,
            "value": {"targetId": "opp_target", "actorId": "c_b11003", "effectType": "REMOVE"}
        }));
        assert!(engine.players.get("p2").unwrap().field.is_empty());
        assert_eq!(engine.players.get("p2").unwrap().remove.len(), 1);

        // 後半: D02005 自身が登場したときの「このキャラか〚カード名［服部平次］〛が自分の現場に登場したとき、キャラを1枚まで選び、スリープさせる。」も実戦フローで発火する
        {
            engine.catalog.definitions.insert(
                "TEST_CASE_GREEN".to_string(),
                models::CardDefinition {
                    definition_id: "TEST_CASE_GREEN".to_string(),
                    canonical_id: "TEST_CASE_GREEN".to_string(),
                    card_type: models::CardType::CASE,
                    colors: vec!["緑".to_string()],
                    level: 0,
                    ap: 0,
                    lp: 0,
                    names: vec!["テスト緑事件".to_string()],
                    description: String::new(),
                    traits: vec![],
                    case_levels: None,
                    main_thumb: None,
                    main_path: None,
                },
            );
            let d02005_lv = engine.catalog.get("D02005").unwrap().level;
            let ps1_m = engine.players.get_mut("p1").unwrap();
            ps1_m.normal_hand_played_this_turn = false;
            for i in 0..d02005_lv {
                if (ps1_m.file.len() as i32) < i {
                    ps1_m.file.push(test_inst(&format!("f_ext_{}", i), "B01021", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
                }
            }
            ps1_m.hand.push(test_inst("h_d02005", "D02005", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
            ps1_m.r#case = Some(test_inst("p1_case_g", "TEST_CASE_GREEN", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
            engine.players.get_mut("p2").unwrap().field.push(test_inst("opp_act", "B01021", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        }
        engine.prompt_main_action("p1");
        let pd2 = engine.pending_decision.as_ref().unwrap().clone();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd2["decisionId"],
            "revision": engine.revision,
            "value": {"type": "HAND_PLAY", "cardId": "h_d02005"}
        }));
        let pd_ts2 = engine.pending_decision.as_ref().expect("D02005 self play must prompt SLEEP targetSelect");
        assert_eq!(pd_ts2["kind"], "targetSelect");
        assert_eq!(pd_ts2["options"]["effectType"], "SLEEP");
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_ts2["decisionId"],
            "revision": engine.revision,
            "value": {"targetId": "opp_act", "actorId": "h_d02005", "effectType": "SLEEP"}
        }));
        assert_eq!(engine.players.get("p2").unwrap().field[0].orientation, models::CardOrientation::SLEEP);
    }

    #[test]
    fn test_case_attack_guard_then_cutin_both_sides_and_case_resolve() {
        let mut catalog = load_test_catalog();
        catalog.definitions.insert("T_AP4000_BLUE".to_string(), models::CardDefinition {
            definition_id: "T_AP4000_BLUE".to_string(),
            canonical_id: "T_AP4000_BLUE".to_string(),
            card_type: models::CardType::CHARACTER,
            colors: vec!["青".to_string()],
            level: 4,
            ap: 4000,
            lp: 1,
            names: vec!["アタッカー4000".to_string()],
            description: String::new(),
            traits: vec![],
            case_levels: None,
            main_thumb: None,
            main_path: None,
        });
        catalog.definitions.insert("T_CUTIN_P1000_BLUE".to_string(), models::CardDefinition {
            definition_id: "T_CUTIN_P1000_BLUE".to_string(),
            canonical_id: "T_CUTIN_P1000_BLUE".to_string(),
            card_type: models::CardType::CHARACTER,
            colors: vec!["青".to_string()],
            level: 1,
            ap: 1000,
            lp: 1,
            names: vec!["カットイン青1000".to_string()],
            description: "【カットイン】自分のキャラ1人のAP+1000。".to_string(),
            traits: vec![],
            case_levels: None,
            main_thumb: None,
            main_path: None,
        });

        let mut engine = MatchEngine::new(catalog);
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];
        engine.phase = models::MatchPhase::MAIN;

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let mut ps2 = engine::PlayerState::new("p2".to_string());
        ps1.r#case = Some(test_inst("p1_case", "B01002", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
        ps2.r#case = Some(test_inst("p2_case", "B01002", "p2", models::Zone::CASE, models::CardOrientation::ACTIVE));
        for i in 0..5 {
            ps1.file.push(test_inst(&format!("f1_{}", i), "T_AP4000_BLUE", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
            ps2.file.push(test_inst(&format!("f2_{}", i), "T_AP4000_BLUE", "p2", models::Zone::FILE, models::CardOrientation::ACTIVE));
        }
        for i in 0..3 {
            ps1.deck.push(test_inst(&format!("d1_{}", i), "T_AP4000_BLUE", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
            ps2.deck.push(test_inst(&format!("d2_{}", i), "T_AP4000_BLUE", "p2", models::Zone::DECK, models::CardOrientation::ACTIVE));
        }
        ps2.evidence.push(test_inst("ev2_0", "T_AP4000_BLUE", "p2", models::Zone::EVIDENCE, models::CardOrientation::ACTIVE));
        ps1.field.push(test_inst("p1_atk", "T_AP4000_BLUE", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps2.field.push(test_inst("p2_guard", "T_AP4000_BLUE", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("p1_cutin", "T_CUTIN_P1000_BLUE", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps2.hand.push(test_inst("p2_cutin", "T_CUTIN_P1000_BLUE", "p2", models::Zone::HAND, models::CardOrientation::ACTIVE));

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        // 1. p1がp2の事件にアタック
        engine.prompt_main_action("p1");
        let pd_main = engine.pending_decision.clone().unwrap();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_main["decisionId"],
            "revision": engine.revision,
            "value": {"type": "ACTION", "actorId": "p1_atk", "targetId": "p2_case"}
        }));

        // 2. p2がp2_guardでガードを宣言
        let pd_guard = engine.pending_decision.clone().unwrap();
        assert_eq!(pd_guard["kind"], "guard");
        assert_eq!(pd_guard["playerId"], "p2");
        let _ = engine.submit_decision("p2", &serde_json::json!({
            "decisionId": pd_guard["decisionId"],
            "revision": engine.revision,
            "value": {"guardCardId": "p2_guard"}
        }));

        // 3. ガードによるコンタクト発生後、APが低い方（同APなら攻撃側p1）からカットインフェーズへ移行する
        let pd_cut_atk = engine.pending_decision.clone().expect("attacker contactCutin should be prompted first when AP <= defender");
        assert_eq!(pd_cut_atk["kind"], "guard");
        assert_eq!(pd_cut_atk["playerId"], "p1");
        assert!(pd_cut_atk["options"]["guardCardIds"].as_array().unwrap().is_empty());
        assert!(pd_cut_atk["options"]["cutinCardIds"].as_array().unwrap().iter().any(|v| v["instanceId"] == "p1_cutin"));
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_cut_atk["decisionId"],
            "revision": engine.revision,
            "value": {}
        }));

        // 4. p1がパスしたら、防御側p2のカットインフェーズに移る（即座にバトル解決しない）
        assert_eq!(engine.players.get("p2").unwrap().remove.len(), 0);
        let pd_cut_def = engine.pending_decision.clone().expect("defender contactCutin should be prompted after attacker pass");
        assert_eq!(pd_cut_def["kind"], "guard");
        assert_eq!(pd_cut_def["playerId"], "p2");
        assert!(pd_cut_def["options"]["guardCardIds"].as_array().unwrap().is_empty());
        assert!(pd_cut_def["options"]["cutinCardIds"].as_array().unwrap().iter().any(|v| v["instanceId"] == "p2_cutin"));

        // 5. 攻撃側p1がガードされた事件アタック中にカットインを使用できる
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_cut_atk["decisionId"],
            "revision": engine.revision,
            "value": {"cutinCardId": "p1_cutin"}
        }));
        if let Some(pd_after) = engine.pending_decision.clone() {
            if pd_after["kind"] == "targetSelect" {
                let _ = engine.submit_decision("p1", &serde_json::json!({
                    "decisionId": pd_after["decisionId"],
                    "revision": engine.revision,
                    "value": {"targetId": "p1_atk", "actorId": "p1_cutin", "effectType": "AP_BUFF"}
                }));
            }
        }

        // 6. p1がカットイン使用後、まだ未使用のp2に1度チャンスが回り、p2がパスして解決する
        if let Some(pd_cut_def2) = engine.pending_decision.clone() {
            if pd_cut_def2["kind"] == "guard" {
                assert_eq!(pd_cut_def2["playerId"], "p2");
                let _ = engine.submit_decision("p2", &serde_json::json!({
                    "decisionId": pd_cut_def2["decisionId"],
                    "revision": engine.revision,
                    "value": {}
                }));
            }
        }

        // p1_atkがAP5000でp2_guard(AP4000)に勝利し、p2_guardがリムーブされ、p1_atkは場に残る
        assert!(engine.players.get("p1").unwrap().field.iter().any(|c| c.instance_id == "p1_atk"));
        assert!(engine.players.get("p2").unwrap().remove.iter().any(|c| c.instance_id == "p2_guard"));
        assert!(!engine.players.get("p2").unwrap().field.iter().any(|c| c.instance_id == "p2_guard"));
    }

    #[test]
    fn test_char_attack_equal_ap_defender_pass_prompts_attacker_cutin() {
        let mut catalog = load_test_catalog();
        catalog.definitions.insert("T_EQ_4000".to_string(), models::CardDefinition {
            definition_id: "T_EQ_4000".to_string(),
            canonical_id: "T_EQ_4000".to_string(),
            card_type: models::CardType::CHARACTER,
            colors: vec!["青".to_string()],
            level: 4,
            ap: 4000,
            lp: 1,
            names: vec!["同APキャラ".to_string()],
            description: String::new(),
            traits: vec![],
            case_levels: None,
            main_thumb: None,
            main_path: None,
        });
        catalog.definitions.insert("T_EQ_CUTIN".to_string(), models::CardDefinition {
            definition_id: "T_EQ_CUTIN".to_string(),
            canonical_id: "T_EQ_CUTIN".to_string(),
            card_type: models::CardType::CHARACTER,
            colors: vec!["青".to_string()],
            level: 1,
            ap: 1000,
            lp: 1,
            names: vec!["同AP突破カットイン".to_string()],
            description: "【カットイン】自分のキャラ1人のAP+1000。".to_string(),
            traits: vec![],
            case_levels: None,
            main_thumb: None,
            main_path: None,
        });

        let mut engine = MatchEngine::new(catalog);
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];
        engine.phase = models::MatchPhase::MAIN;

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let mut ps2 = engine::PlayerState::new("p2".to_string());
        ps1.r#case = Some(test_inst("p1_case", "B01002", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
        ps2.r#case = Some(test_inst("p2_case", "B01002", "p2", models::Zone::CASE, models::CardOrientation::ACTIVE));
        for i in 0..5 {
            ps1.file.push(test_inst(&format!("f1_{}", i), "T_EQ_4000", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
            ps2.file.push(test_inst(&format!("f2_{}", i), "T_EQ_4000", "p2", models::Zone::FILE, models::CardOrientation::ACTIVE));
        }
        ps1.field.push(test_inst("p1_atk", "T_EQ_4000", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps2.field.push(test_inst("p2_def", "T_EQ_4000", "p2", models::Zone::FIELD, models::CardOrientation::SLEEP));
        ps1.hand.push(test_inst("p1_cutin", "T_EQ_CUTIN", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps2.hand.push(test_inst("p2_cutin", "T_EQ_CUTIN", "p2", models::Zone::HAND, models::CardOrientation::ACTIVE));

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        // 1. p1が同APのp2_defにアタック
        engine.prompt_main_action("p1");
        let pd_main = engine.pending_decision.clone().unwrap();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_main["decisionId"],
            "revision": engine.revision,
            "value": {"type": "ACTION", "actorId": "p1_atk", "targetId": "p2_def"}
        }));

        // 2. p2がカットイン/ガードを使わずにパス
        let pd_guard = engine.pending_decision.clone().unwrap();
        assert_eq!(pd_guard["kind"], "guard");
        let _ = engine.submit_decision("p2", &serde_json::json!({
            "decisionId": pd_guard["decisionId"],
            "revision": engine.revision,
            "value": {}
        }));

        // 3. p2_defはまだリムーブされず、p1のカットインフェーズへ移る
        assert_eq!(engine.players.get("p2").unwrap().remove.len(), 0);
        let pd_cut_atk = engine.pending_decision.clone().expect("attacker contactCutin must be prompted");
        assert_eq!(pd_cut_atk["kind"], "guard");
        assert_eq!(pd_cut_atk["playerId"], "p1");
        assert!(pd_cut_atk["options"]["guardCardIds"].as_array().unwrap().is_empty());
        assert!(pd_cut_atk["options"]["cutinCardIds"].as_array().unwrap().iter().any(|v| v["instanceId"] == "p1_cutin"));

        // 4. p1がカットインを使用し、p2がパスして決着
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_cut_atk["decisionId"],
            "revision": engine.revision,
            "value": {"cutinCardId": "p1_cutin"}
        }));
        if let Some(pd_after) = engine.pending_decision.clone() {
            if pd_after["kind"] == "targetSelect" {
                let _ = engine.submit_decision("p1", &serde_json::json!({
                    "decisionId": pd_after["decisionId"],
                    "revision": engine.revision,
                    "value": {"targetId": "p1_atk", "actorId": "p1_cutin", "effectType": "AP_BUFF"}
                }));
            }
        }
        if let Some(pd_cut_def2) = engine.pending_decision.clone() {
            if pd_cut_def2["kind"] == "guard" {
                let _ = engine.submit_decision("p2", &serde_json::json!({
                    "decisionId": pd_cut_def2["decisionId"],
                    "revision": engine.revision,
                    "value": {}
                }));
            }
        }

        assert!(engine.players.get("p1").unwrap().field.iter().any(|c| c.instance_id == "p1_atk"));
        assert!(engine.players.get("p2").unwrap().remove.iter().any(|c| c.instance_id == "p2_def"));
    }

    #[test]
    fn test_rush_character_cannot_deduce_on_entered_turn() {
        let mut catalog = load_test_catalog();
        catalog.definitions.insert("T_RUSH_CHAR".to_string(), models::CardDefinition {
            definition_id: "T_RUSH_CHAR".to_string(),
            canonical_id: "T_RUSH_CHAR".to_string(),
            card_type: models::CardType::CHARACTER,
            colors: vec!["黄".to_string()],
            level: 4,
            ap: 4000,
            lp: 1,
            names: vec!["突撃テストキャラ".to_string()],
            description: "【登場時】ターン終了時までこのキャラは〚突撃［キャラ］〛を持つ。".to_string(),
            traits: vec![],
            case_levels: None,
            main_thumb: None,
            main_path: None,
        });

        let mut engine = MatchEngine::new(catalog);
        engine.turn = 3;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];
        engine.phase = models::MatchPhase::MAIN;

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let mut ps2 = engine::PlayerState::new("p2".to_string());
        let mut rush_c = test_inst("p1_rush", "T_RUSH_CHAR", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE);
        rush_c.entered_turn = 3;
        rush_c.has_rush = true;
        rush_c.rush_target = Some("CHAR".to_string());
        ps1.field.push(rush_c);
        ps2.field.push(test_inst("p2_sleep", "T_RUSH_CHAR", "p2", models::Zone::FIELD, models::CardOrientation::SLEEP));

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        engine.prompt_main_action("p1");
        let pd = engine.pending_decision.as_ref().unwrap();
        let actions = pd["options"]["actions"].as_array().unwrap();

        // 登場ターンは突撃を持っていても DEDUCE は不可
        let deduce_actors: Vec<&str> = actions
            .iter()
            .find(|a| a["type"] == "DEDUCE")
            .and_then(|a| a["actorIds"].as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        assert!(!deduce_actors.contains(&"p1_rush"), "Rush character must not be able to DEDUCE on entered turn");

        // 登場ターンでも ACTION（アタック）は可能
        let action_actors: Vec<&str> = actions
            .iter()
            .find(|a| a["type"] == "ACTION")
            .and_then(|a| a["actorIds"].as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        assert!(action_actors.contains(&"p1_rush"), "Rush character should be able to ACTION on entered turn");
    }

    #[test]
    fn test_b11017_gonin_bayashi_effect() {
        let catalog = load_test_catalog();
        assert!(catalog.definitions.contains_key("B11017"), "B11017 must exist in catalog");

        let mut engine = MatchEngine::new(catalog);
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];
        engine.phase = models::MatchPhase::MAIN;

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let ps2 = engine::PlayerState::new("p2".to_string());
        ps1.partner = Some(test_inst("p1_partner", "B01001", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
        ps1.r#case = Some(test_inst("p1_case", "B01002", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
        for i in 0..8 {
            ps1.file.push(test_inst(&format!("f{}", i), "B01021", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
        }

        ps1.hand.push(test_inst("h_b11017", "B11017", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps1.remove.push(test_inst("r_conan1", "B01011", "p1", models::Zone::REMOVE, models::CardOrientation::ACTIVE)); // Lv4 江戸川コナン (少年探偵団)
        ps1.remove.push(test_inst("r_conan2", "B01011", "p1", models::Zone::REMOVE, models::CardOrientation::ACTIVE)); // 同名 Lv4 江戸川コナン
        ps1.remove.push(test_inst("r_genta", "B01014", "p1", models::Zone::REMOVE, models::CardOrientation::ACTIVE));  // Lv4 小嶋元太 (少年探偵団)
        ps1.remove.push(test_inst("r_ai", "B01016", "p1", models::Zone::REMOVE, models::CardOrientation::ACTIVE));     // Lv4 灰原哀 (少年探偵団)
        ps1.remove.push(test_inst("r_other", "B01005", "p1", models::Zone::REMOVE, models::CardOrientation::ACTIVE));  // Lv5 非対象

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        engine.prompt_main_action("p1");
        let pd1 = engine.pending_decision.as_ref().unwrap().clone();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd1["decisionId"],
            "revision": engine.revision,
            "value": {"type": "HAND_PLAY", "cardId": "h_b11017"}
        }));

        let pd_ts = engine.pending_decision.as_ref().expect("B11017 must prompt targetSelect");
        assert_eq!(pd_ts["kind"], "targetSelect");
        assert_eq!(pd_ts["options"]["effectType"], "REVIVE_SPLIT_GONIN");
        assert_eq!(pd_ts["options"]["maxSelect"], 3);
        assert_eq!(pd_ts["options"]["distinctNames"], true);
        let cand_ids: Vec<&str> = pd_ts["options"]["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|c| c["instanceId"].as_str())
            .collect();
        assert!(!cand_ids.contains(&"r_other"));
        assert!(cand_ids.contains(&"r_conan1"));
        assert!(cand_ids.contains(&"r_genta"));
        assert!(cand_ids.contains(&"r_ai"));

        let ts_dec_id = pd_ts["decisionId"].as_str().unwrap().to_string();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": ts_dec_id,
            "revision": engine.revision,
            "value": {
                "targetIds": ["r_conan1", "r_genta", "r_ai"],
                "actorId": "h_b11017",
                "effectType": "REVIVE_SPLIT_GONIN"
            }
        }));

        let ps1_after = engine.players.get("p1").unwrap();
        let conan = ps1_after.field.iter().find(|c| c.instance_id == "r_conan1").expect("1st pick must deploy to field");
        assert_eq!(conan.orientation, models::CardOrientation::ACTIVE);
        let genta = ps1_after.field.iter().find(|c| c.instance_id == "r_genta").expect("2nd pick must deploy to field in SLEEP");
        assert_eq!(genta.orientation, models::CardOrientation::SLEEP);
        assert!(ps1_after.hand.iter().any(|c| c.instance_id == "r_ai"), "3rd pick must go to hand");
        assert!(ps1_after.remove.iter().any(|c| c.instance_id == "h_b11017"));
    }

    #[test]
    fn test_all_event_cards_have_valid_effects() {
        let catalog = load_test_catalog();
        let events: Vec<&models::CardDefinition> = catalog
            .definitions
            .values()
            .filter(|d| d.card_type == models::CardType::EVENT)
            .collect();
        assert!(events.len() >= 200, "Expected >= 200 event cards, got {}", events.len());

        let mut unparsed: Vec<String> = Vec::new();
        for ev in &events {
            if effects::parse_event_mode_choice(&ev.description).is_some() {
                continue;
            }
            let on_play_clauses = effects::extract_clauses(&ev.description, effects::TriggerKind::OnPlay, models::CardType::EVENT);
            let mut has_exec = false;
            for cl in &on_play_clauses {
                let p = effects::parse_effect_steps(cl, Some(ev.level), models::CardType::EVENT);
                if !p.immediate_before.is_empty()
                    || p.target_step.is_some()
                    || p.deck_search.is_some()
                    || p.follow_up_target.is_some()
                    || !p.extra_clauses.is_empty()
                {
                    has_exec = true;
                    break;
                }
            }
            if !has_exec {
                let decl = effects::extract_clauses(&ev.description, effects::TriggerKind::Declare, models::CardType::EVENT);
                let cutin = effects::extract_clauses(&ev.description, effects::TriggerKind::Cutin, models::CardType::EVENT);
                let hirameki = effects::extract_clauses(&ev.description, effects::TriggerKind::Hirameki, models::CardType::EVENT);
                if decl.is_empty() && cutin.is_empty() && hirameki.is_empty() {
                    unparsed.push(format!("{}: {}", ev.definition_id, ev.description));
                }
            }
        }
        assert!(
            unparsed.is_empty(),
            "Unparsed event cards ({}):\n{}",
            unparsed.len(),
            unparsed.join("\n")
        );

        // Runtime smoke 1: Mode choice event B04014 (最後の切り札)
        let mut engine_mode = MatchEngine::new(catalog.clone());
        engine_mode.turn = 4;
        engine_mode.turn_player_id = "p1".to_string();
        engine_mode.player_order = vec!["p1".to_string(), "p2".to_string()];
        engine_mode.phase = models::MatchPhase::MAIN;
        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let mut ps2 = engine::PlayerState::new("p2".to_string());
        ps1.partner = Some(test_inst("p1_p", "B01001", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
        ps1.r#case = Some(test_inst("p1_c", "B01002", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
        ps1.case_phase = models::CasePhase::RESOLUTION;
        for i in 0..8 {
            ps1.file.push(test_inst(&format!("mf{}", i), "B01021", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
        }
        ps1.hand.push(test_inst("h_mode", "B04014", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps2.field.push(test_inst("opp_c", "B01005", "p2", models::Zone::FIELD, models::CardOrientation::SLEEP));
        engine_mode.players.insert("p1".to_string(), ps1);
        engine_mode.players.insert("p2".to_string(), ps2);
        engine_mode.prompt_main_action("p1");
        let pd_m = engine_mode.pending_decision.as_ref().unwrap().clone();
        let _ = engine_mode.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_m["decisionId"],
            "revision": engine_mode.revision,
            "value": {"type": "HAND_PLAY", "cardId": "h_mode"}
        }));
        let pd_choice = engine_mode.pending_decision.as_ref().expect("B04014 should prompt modeSelect");
        assert_eq!(pd_choice["kind"], "modeSelect");

        // Runtime smoke 2: Set-card event B02013 (ターボエンジン付きスケートボード)
        let mut engine_set = MatchEngine::new(catalog);
        engine_set.turn = 4;
        engine_set.turn_player_id = "p1".to_string();
        engine_set.player_order = vec!["p1".to_string(), "p2".to_string()];
        engine_set.phase = models::MatchPhase::MAIN;
        let mut s_ps1 = engine::PlayerState::new("p1".to_string());
        let s_ps2 = engine::PlayerState::new("p2".to_string());
        s_ps1.partner = Some(test_inst("sp1_p", "B01001", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
        s_ps1.r#case = Some(test_inst("sp1_c", "B01002", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
        for i in 0..8 {
            s_ps1.file.push(test_inst(&format!("sf{}", i), "B01021", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
        }
        s_ps1.hand.push(test_inst("h_skate", "B02013", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        s_ps1.field.push(test_inst("p1_conan", "B01011", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        engine_set.players.insert("p1".to_string(), s_ps1);
        engine_set.players.insert("p2".to_string(), s_ps2);
        engine_set.prompt_main_action("p1");
        let spd = engine_set.pending_decision.as_ref().unwrap().clone();
        let _ = engine_set.submit_decision("p1", &serde_json::json!({
            "decisionId": spd["decisionId"],
            "revision": engine_set.revision,
            "value": {"type": "HAND_PLAY", "cardId": "h_skate"}
        }));
        let sts = engine_set.pending_decision.as_ref().expect("B02013 should prompt targetSelect to attach set card");
        assert_eq!(sts["kind"], "targetSelect");
        let _ = engine_set.submit_decision("p1", &serde_json::json!({
            "decisionId": sts["decisionId"],
            "revision": engine_set.revision,
            "value": {"targetId": "p1_conan", "actorId": "h_skate", "effectType": sts["options"]["effectType"]}
        }));
        let conan_inst = engine_set.players.get("p1").unwrap().field.iter().find(|c| c.instance_id == "p1_conan").unwrap();
        assert_eq!(conan_inst.set_cards.len(), 1);
        assert!(engine_set.effective_rush(conan_inst).0, "Skateboard set card must grant Rush");
    }

    #[test]
    fn test_bug1_and_bug4_rush_variants_swift_and_deduce_restrictions() {
        let catalog = load_test_catalog();

        // 1) Bug 1: B07094 (突撃[事件]) must NOT deduce on entered turn, only attack CASE
        {
            let mut engine = MatchEngine::new(catalog.clone());
            engine.turn = 3;
            engine.turn_player_id = "p1".to_string();
            engine.player_order = vec!["p1".to_string(), "p2".to_string()];
            engine.phase = models::MatchPhase::MAIN;

            let mut ps1 = engine::PlayerState::new("p1".to_string());
            let mut ps2 = engine::PlayerState::new("p2".to_string());
            ps1.partner = Some(test_inst("p1_p", "B03106", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE)); // 黒
            ps1.r#case = Some(test_inst("p1_c", "B01024", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
            ps2.r#case = Some(test_inst("p2_c", "B01024", "p2", models::Zone::CASE, models::CardOrientation::ACTIVE));
            let mut gin = test_inst("p1_gin", "B07094", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE);
            gin.entered_turn = 3;
            ps1.field.push(gin.clone());
            ps2.field.push(test_inst("p2_sleep", "B01011", "p2", models::Zone::FIELD, models::CardOrientation::SLEEP));
            ps2.evidence.push(test_inst("p2_ev1", "B01011", "p2", models::Zone::EVIDENCE, models::CardOrientation::ACTIVE));
            engine.players.insert("p1".to_string(), ps1);
            engine.players.insert("p2".to_string(), ps2);

            assert_eq!(engine.effective_rush(&gin), (true, Some("CASE".to_string())));
            assert!(!engine.effective_swift(&gin));

            engine.prompt_main_action("p1");
            let pd = engine.pending_decision.as_ref().unwrap().clone();
            let actions = pd["options"]["actions"].as_array().unwrap();
            let deduce_actors: Vec<&str> = actions
                .iter()
                .find(|a| a["type"] == "DEDUCE")
                .and_then(|a| a["actorIds"].as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();
            assert!(!deduce_actors.contains(&"p1_gin"), "突撃[事件] must not allow DEDUCE on entered turn");

            let action_entry = actions.iter().find(|a| a["type"] == "ACTION").unwrap();
            let action_actors: Vec<&str> = action_entry["actorIds"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|v| v.as_str())
                .collect();
            assert!(action_actors.contains(&"p1_gin"));
            let gin_targets: Vec<&str> = action_entry["actorTargetMap"]["p1_gin"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|v| v.as_str())
                .collect();
            assert_eq!(gin_targets, vec!["p2_c"], "突撃[事件] can only target CASE on entered turn");

            let _ = engine.submit_decision("p1", &serde_json::json!({
                "decisionId": pd["decisionId"],
                "revision": engine.revision,
                "value": {"type": "DEDUCE", "actorId": "p1_gin"}
            }));
            assert_eq!(
                engine.players.get("p1").unwrap().field[0].orientation,
                models::CardOrientation::ACTIVE,
                "Rejected DEDUCE for 突撃[事件] must keep character ACTIVE"
            );
            assert!(
                engine.players.get("p1").unwrap().evidence.is_empty(),
                "Rejected DEDUCE for 突撃[事件] must not add evidence"
            );
        }

        // 2) Bug 4: Conditional passive Rush variants (D07012, B07096, B09008, B08007)
        {
            let mut engine = MatchEngine::new(catalog.clone());
            engine.turn = 3;
            engine.turn_player_id = "p1".to_string();
            engine.player_order = vec!["p1".to_string(), "p2".to_string()];
            engine.phase = models::MatchPhase::MAIN;

            let mut ps1 = engine::PlayerState::new("p1".to_string());
            let mut ps2 = engine::PlayerState::new("p2".to_string());
            ps1.partner = Some(test_inst("p1_p_blue", "B01001", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE)); // 青
            ps1.r#case = Some(test_inst("p1_c", "B01024", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
            ps2.r#case = Some(test_inst("p2_c", "B01024", "p2", models::Zone::CASE, models::CardOrientation::ACTIVE));

            let mut vodka_d07012 = test_inst("p1_vodka", "D07012", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE);
            vodka_d07012.entered_turn = 3;
            let mut vodka_lv4 = test_inst("p1_vodka_lv4", "B07096", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE);
            vodka_lv4.entered_turn = 3;
            let mut akagi = test_inst("p1_akagi", "B09008", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE);
            akagi.entered_turn = 3;
            let mut mitsu = test_inst("p1_mitsu", "B08007", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE);
            mitsu.entered_turn = 3;

            ps1.field.push(vodka_d07012);
            ps1.field.push(vodka_lv4);
            ps1.field.push(akagi);
            ps1.field.push(mitsu);
            ps2.field.push(test_inst("p2_lv4", "B01011", "p2", models::Zone::FIELD, models::CardOrientation::SLEEP));
            ps2.field.push(test_inst("p2_lv8", "B01005", "p2", models::Zone::FIELD, models::CardOrientation::SLEEP));
            ps2.evidence.push(test_inst("p2_ev1", "B01011", "p2", models::Zone::EVIDENCE, models::CardOrientation::ACTIVE));
            engine.players.insert("p1".to_string(), ps1);
            engine.players.insert("p2".to_string(), ps2);

            // With Blue partner: D07012 has NO Rush; B08007 has 突撃[事件]
            {
                let ps1_ref = engine.players.get("p1").unwrap();
                assert_eq!(engine.effective_rush(&ps1_ref.field[0]), (false, None));
                assert_eq!(engine.effective_rush(&ps1_ref.field[2]), (false, None)); // B09008 AP5000 < 6000
                assert_eq!(engine.effective_rush(&ps1_ref.field[3]), (true, Some("CASE".to_string())));
            }

            // Switch to Black partner: D07012 gets full Rush, B07096 gets CHAR_LV4 Rush
            engine.players.get_mut("p1").unwrap().partner =
                Some(test_inst("p1_p_black", "B03106", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
            // Give B09008 +1000 AP (total 6000) and B08007 1 under_card (+ 突撃[キャラ])
            engine.players.get_mut("p1").unwrap().field[2].temp_ap_bonus = 1000;
            engine.players.get_mut("p1").unwrap().field[3].set_cards.push(
                test_inst("u1", "B01011", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE),
            );
            // Put back Blue partner? Check D07012 & B07096 with Black partner first
            {
                let ps1_ref = engine.players.get("p1").unwrap();
                assert_eq!(engine.effective_rush(&ps1_ref.field[0]), (true, None), "D07012 must gain 突撃 with Black partner");
                assert_eq!(engine.effective_rush(&ps1_ref.field[1]), (true, Some("CHAR_LV4".to_string())));
                assert_eq!(engine.effective_rush(&ps1_ref.field[2]), (true, None), "B09008 at AP6000 must gain 突撃");
            }
            engine.prompt_main_action("p1");
            let pd = engine.pending_decision.as_ref().unwrap();
            let actions = pd["options"]["actions"].as_array().unwrap();
            let action_entry = actions.iter().find(|a| a["type"] == "ACTION").unwrap();
            let vodka_targets: Vec<&str> = action_entry["actorTargetMap"]["p1_vodka"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|v| v.as_str())
                .collect();
            assert!(vodka_targets.contains(&"p2_c") && vodka_targets.contains(&"p2_lv4") && vodka_targets.contains(&"p2_lv8"));

            let lv4_targets: Vec<&str> = action_entry["actorTargetMap"]["p1_vodka_lv4"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|v| v.as_str())
                .collect();
            assert!(lv4_targets.contains(&"p2_lv4"));
            assert!(!lv4_targets.contains(&"p2_lv8"));
            assert!(!lv4_targets.contains(&"p2_c"));

            // With Blue partner + 1 under_card, B08007 merges 突撃[事件] + 突撃[キャラ] => (true, None)
            engine.players.get_mut("p1").unwrap().partner =
                Some(test_inst("p1_p_blue", "B01001", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
            let ps1_ref = engine.players.get("p1").unwrap();
            assert_eq!(engine.effective_rush(&ps1_ref.field[3]), (true, None));
        }

        // 3) Bug 1 / 4 cross-card: B10015 (迅速 with 4 distinct 毛利探偵事務所 incl. 妃英理) CAN deduce on entered turn
        {
            let mut engine = MatchEngine::new(catalog);
            engine.turn = 4;
            engine.turn_player_id = "p1".to_string();
            engine.player_order = vec!["p1".to_string(), "p2".to_string()];
            engine.phase = models::MatchPhase::MAIN;

            let mut ps1 = engine::PlayerState::new("p1".to_string());
            let ps2 = engine::PlayerState::new("p2".to_string());
            ps1.partner = Some(test_inst("p1_p", "B01001", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
            ps1.r#case = Some(test_inst("p1_c", "B01024", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
            let mut b10015 = test_inst("p1_mori", "B10015", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE);
            b10015.entered_turn = 4;
            ps1.field.push(b10015);
            ps1.field.push(test_inst("p1_conan", "B01005", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
            ps1.field.push(test_inst("p1_ran", "B01007", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
            engine.players.insert("p1".to_string(), ps1);
            engine.players.insert("p2".to_string(), ps2);

            // Only 3 distinct 毛利探偵事務所 -> no Swift yet
            {
                let m = &engine.players.get("p1").unwrap().field[0];
                assert!(!engine.effective_swift(m));
            }
            // Adding 妃英理 (B01013) -> granted 毛利探偵事務所 by B10015 -> 4 distinct names -> Swift!
            engine.players.get_mut("p1").unwrap().field.push(
                test_inst("p1_eri", "B01013", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE),
            );
            {
                let m = &engine.players.get("p1").unwrap().field[0];
                assert!(engine.effective_swift(m), "B10015 must grant Swift once 4 distinct 毛利探偵事務所 are present");
            }
            engine.prompt_main_action("p1");
            let pd = engine.pending_decision.as_ref().unwrap();
            let deduce_actors: Vec<&str> = pd["options"]["actions"]
                .as_array()
                .unwrap()
                .iter()
                .find(|a| a["type"] == "DEDUCE")
                .and_then(|a| a["actorIds"].as_array())
                .unwrap()
                .iter()
                .filter_map(|v| v.as_str())
                .collect();
            assert!(deduce_actors.contains(&"p1_mori"), "Character with 迅速 must be able to DEDUCE on entered turn");
        }
    }

    #[test]
    fn test_bug2_cutin_self_turn_ap_buff_and_cross_cards() {
        let catalog = load_test_catalog();

        // 1) Standard cut-in: 【カットイン】【自分ターン中】AP＋3000 (B02007) + B10085 aura (+2000)
        {
            let mut engine = MatchEngine::new(catalog.clone());
            engine.turn = 4;
            engine.turn_player_id = "p1".to_string();
            engine.player_order = vec!["p1".to_string(), "p2".to_string()];
            engine.phase = models::MatchPhase::MAIN;

            let mut ps1 = engine::PlayerState::new("p1".to_string());
            let mut ps2 = engine::PlayerState::new("p2".to_string());
            ps1.partner = Some(test_inst("p1_p", "B01001", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
            ps1.r#case = Some(test_inst("p1_c", "B01024", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
            ps2.r#case = Some(test_inst("p2_c", "B01024", "p2", models::Zone::CASE, models::CardOrientation::ACTIVE));
            // Attacker AP6000 vs Defender AP7000
            ps1.field.push(test_inst("p1_atk", "B01008", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
            ps1.hand.push(test_inst("h_cut3000", "B02007", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
            ps2.field.push(test_inst("p2_def", "B01005", "p2", models::Zone::FIELD, models::CardOrientation::SLEEP));
            engine.players.insert("p1".to_string(), ps1);
            engine.players.insert("p2".to_string(), ps2);

            engine.prompt_main_action("p1");
            let pd1 = engine.pending_decision.clone().unwrap();
            let _ = engine.submit_decision("p1", &serde_json::json!({
                "decisionId": pd1["decisionId"],
                "revision": engine.revision,
                "value": {"type": "ACTION", "actorId": "p1_atk", "targetId": "p2_def"}
            }));

            let pd_cut = engine.pending_decision.clone().expect("p1 contactCutin should open");
            assert_eq!(pd_cut["kind"], "guard");
            assert_eq!(pd_cut["playerId"], "p1");
            let _ = engine.submit_decision("p1", &serde_json::json!({
                "decisionId": pd_cut["decisionId"],
                "revision": engine.revision,
                "value": {"cutinCardId": "h_cut3000"}
            }));

            let p1_atk = engine.players.get("p1").unwrap().field.iter().find(|c| c.instance_id == "p1_atk").unwrap();
            assert_eq!(p1_atk.temp_ap_bonus, 3000, "B02007 self-turn cutin must grant +3000 AP to contacting char");
            assert!(engine.players.get("p2").unwrap().remove.iter().any(|c| c.instance_id == "p2_def"), "6000+3000 > 7000 must win combat");
        }

        // 2) B10085 (in field/partner) adds +2000 on own-turn cutin on top of B02007 (+3000) => +5000 total
        {
            let mut engine = MatchEngine::new(catalog.clone());
            engine.turn = 4;
            engine.turn_player_id = "p1".to_string();
            engine.player_order = vec!["p1".to_string(), "p2".to_string()];
            engine.phase = models::MatchPhase::MAIN;

            let mut ps1 = engine::PlayerState::new("p1".to_string());
            let mut ps2 = engine::PlayerState::new("p2".to_string());
            ps1.partner = Some(test_inst("p1_p", "B01001", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
            ps1.r#case = Some(test_inst("p1_c", "B01024", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
            ps1.field.push(test_inst("p1_atk", "B01008", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
            ps1.field.push(test_inst("p1_chianti", "B10085", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
            ps1.hand.push(test_inst("h_cut3000", "B02007", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
            ps2.field.push(test_inst("p2_def", "B01005", "p2", models::Zone::FIELD, models::CardOrientation::SLEEP));
            engine.players.insert("p1".to_string(), ps1);
            engine.players.insert("p2".to_string(), ps2);

            engine.prompt_main_action("p1");
            let pd1 = engine.pending_decision.clone().unwrap();
            let _ = engine.submit_decision("p1", &serde_json::json!({
                "decisionId": pd1["decisionId"],
                "revision": engine.revision,
                "value": {"type": "ACTION", "actorId": "p1_atk", "targetId": "p2_def"}
            }));
            let pd_cut = engine.pending_decision.clone().unwrap();
            let _ = engine.submit_decision("p1", &serde_json::json!({
                "decisionId": pd_cut["decisionId"],
                "revision": engine.revision,
                "value": {"cutinCardId": "h_cut3000"}
            }));
            let p1_atk = engine.players.get("p1").unwrap().field.iter().find(|c| c.instance_id == "p1_atk").unwrap();
            assert_eq!(p1_atk.temp_ap_bonus, 5000);
        }

        // 3) B11015: 工藤新一 => +3000, non-工藤新一 => +1000
        for (is_shinichi, expected_ap) in [(false, 1000), (true, 3000)] {
            let mut engine = MatchEngine::new(catalog.clone());
            engine.turn = 4;
            engine.turn_player_id = "p1".to_string();
            engine.player_order = vec!["p1".to_string(), "p2".to_string()];
            engine.phase = models::MatchPhase::MAIN;

            let mut ps1 = engine::PlayerState::new("p1".to_string());
            let mut ps2 = engine::PlayerState::new("p2".to_string());
            ps1.partner = Some(test_inst("p1_p", "B01001", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
            ps1.r#case = Some(test_inst("p1_c", "B01024", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
            let atk_def = if is_shinichi { "B01009" } else { "B01008" };
            ps1.field.push(test_inst("p1_atk", atk_def, "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
            ps1.hand.push(test_inst("h_b11015", "B11015", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
            ps2.field.push(test_inst("p2_def", "B01005", "p2", models::Zone::FIELD, models::CardOrientation::SLEEP));
            engine.players.insert("p1".to_string(), ps1);
            engine.players.insert("p2".to_string(), ps2);

            engine.prompt_main_action("p1");
            let pd1 = engine.pending_decision.clone().unwrap();
            let _ = engine.submit_decision("p1", &serde_json::json!({
                "decisionId": pd1["decisionId"],
                "revision": engine.revision,
                "value": {"type": "ACTION", "actorId": "p1_atk", "targetId": "p2_def"}
            }));
            let pd_cut = engine.pending_decision.clone().unwrap();
            let _ = engine.submit_decision("p1", &serde_json::json!({
                "decisionId": pd_cut["decisionId"],
                "revision": engine.revision,
                "value": {"cutinCardId": "h_b11015"}
            }));
            let p1_atk = engine.players.get("p1").unwrap().field.iter().find(|c| c.instance_id == "p1_atk").unwrap();
            assert_eq!(p1_atk.temp_ap_bonus, expected_ap);
        }

        // 4) B05040: optional discard hand scales AP by removed level (lv8 => +8000, skip => 0)
        for do_discard in [true, false] {
            let mut engine = MatchEngine::new(catalog.clone());
            engine.turn = 4;
            engine.turn_player_id = "p1".to_string();
            engine.player_order = vec!["p1".to_string(), "p2".to_string()];
            engine.phase = models::MatchPhase::MAIN;

            let mut ps1 = engine::PlayerState::new("p1".to_string());
            let mut ps2 = engine::PlayerState::new("p2".to_string());
            ps1.partner = Some(test_inst("p1_p", "B01001", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
            ps1.r#case = Some(test_inst("p1_c", "B01024", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
            ps1.field.push(test_inst("p1_atk", "B01008", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
            ps1.hand.push(test_inst("h_b05040", "B05040", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
            ps1.hand.push(test_inst("h_lv8", "B01005", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
            ps2.field.push(test_inst("p2_def", "B01005", "p2", models::Zone::FIELD, models::CardOrientation::SLEEP));
            engine.players.insert("p1".to_string(), ps1);
            engine.players.insert("p2".to_string(), ps2);

            engine.prompt_main_action("p1");
            let pd1 = engine.pending_decision.clone().unwrap();
            let _ = engine.submit_decision("p1", &serde_json::json!({
                "decisionId": pd1["decisionId"],
                "revision": engine.revision,
                "value": {"type": "ACTION", "actorId": "p1_atk", "targetId": "p2_def"}
            }));
            let pd_cut = engine.pending_decision.clone().unwrap();
            let _ = engine.submit_decision("p1", &serde_json::json!({
                "decisionId": pd_cut["decisionId"],
                "revision": engine.revision,
                "value": {"cutinCardId": "h_b05040"}
            }));
            let pd_disc = engine.pending_decision.clone().expect("B05040 must prompt optional DISCARD_HAND");
            assert_eq!(pd_disc["kind"], "targetSelect");
            assert_eq!(pd_disc["options"]["effectType"], "DISCARD_HAND");
            let answer_val = if do_discard {
                serde_json::json!({"targetId": "h_lv8", "actorId": "h_b05040", "effectType": "DISCARD_HAND"})
            } else {
                serde_json::json!({"skip": true})
            };
            let _ = engine.submit_decision("p1", &serde_json::json!({
                "decisionId": pd_disc["decisionId"],
                "revision": engine.revision,
                "value": answer_val
            }));
            let p1_atk = engine.players.get("p1").unwrap().field.iter().find(|c| c.instance_id == "p1_atk").unwrap();
            assert_eq!(p1_atk.temp_ap_bonus, if do_discard { 8000 } else { 0 });
        }
    }

    #[test]
    fn test_bug3_b07094_gin_and_optional_compound_costs() {
        let catalog = load_test_catalog();

        // Case A: B07094 alone (no lv<=7 char on own field) => TurnEnd must NOT fire opponent removal
        {
            let mut engine = MatchEngine::new(catalog.clone());
            engine.turn = 4;
            engine.turn_player_id = "p1".to_string();
            engine.player_order = vec!["p1".to_string(), "p2".to_string()];
            engine.phase = models::MatchPhase::END_TRIGGERS;

            let mut ps1 = engine::PlayerState::new("p1".to_string());
            let mut ps2 = engine::PlayerState::new("p2".to_string());
            ps1.partner = Some(test_inst("p1_p", "B03106", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE)); // 黒
            ps1.field.push(test_inst("p1_gin", "B07094", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE)); // lv8
            ps1.hand.push(test_inst("h1", "B01011", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
            ps2.field.push(test_inst("opp_c", "B01011", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE));
            engine.players.insert("p1".to_string(), ps1);
            engine.players.insert("p2".to_string(), ps2);

            engine.trigger_on_turn_end("p1");
            assert!(engine.pending_decision.is_none(), "Without self lv<=7 field char, B07094 compound cost must not prompt");
            assert_eq!(engine.players.get("p1").unwrap().hand.len(), 1);
            assert_eq!(engine.players.get("p2").unwrap().field.len(), 1);
        }

        // Case B: B07094 + self_lv4 present -> pay hand discard -> SKIP self_lv4 remove -> opponent char NOT removed
        {
            let mut engine = MatchEngine::new(catalog.clone());
            engine.turn = 4;
            engine.turn_player_id = "p1".to_string();
            engine.player_order = vec!["p1".to_string(), "p2".to_string()];
            engine.phase = models::MatchPhase::END_TRIGGERS;

            let mut ps1 = engine::PlayerState::new("p1".to_string());
            let mut ps2 = engine::PlayerState::new("p2".to_string());
            ps1.partner = Some(test_inst("p1_p", "B03106", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
            ps1.r#case = Some(test_inst("p1_c", "B01024", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
            ps2.r#case = Some(test_inst("p2_c", "B01024", "p2", models::Zone::CASE, models::CardOrientation::ACTIVE));
            ps1.field.push(test_inst("p1_gin", "B07094", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
            ps1.field.push(test_inst("self_lv4", "B01011", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
            ps1.hand.push(test_inst("h1", "B01011", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
            ps2.field.push(test_inst("opp_c", "B01014", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE));
            engine.players.insert("p1".to_string(), ps1);
            engine.players.insert("p2".to_string(), ps2);

            engine.trigger_on_turn_end("p1");
            let pd1 = engine.pending_decision.clone().expect("Step 1 DISCARD_HAND must prompt");
            assert_eq!(pd1["kind"], "targetSelect");
            assert_eq!(pd1["options"]["effectType"], "DISCARD_HAND");
            let _ = engine.submit_decision("p1", &serde_json::json!({
                "decisionId": pd1["decisionId"],
                "revision": engine.revision,
                "value": {"targetId": "h1", "actorId": "p1_gin", "effectType": "DISCARD_HAND"}
            }));

            let pd2 = engine.pending_decision.clone().expect("Step 2 self lv<=7 REMOVE must prompt");
            assert_eq!(pd2["kind"], "targetSelect");
            assert_eq!(pd2["options"]["effectType"], "REMOVE");
            let cands2: Vec<&str> = pd2["options"]["candidates"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|c| c["instanceId"].as_str())
                .collect();
            assert_eq!(cands2, vec!["self_lv4"]);

            // Skip step 2 -> must abort follow-up opponent removal
            let _ = engine.submit_decision("p1", &serde_json::json!({
                "decisionId": pd2["decisionId"],
                "revision": engine.revision,
                "value": {"skip": true}
            }));
            assert!(engine.players.get("p1").unwrap().field.iter().any(|c| c.instance_id == "self_lv4"));
            assert!(engine.players.get("p2").unwrap().field.iter().any(|c| c.instance_id == "opp_c"), "opp_c must NOT be removed when step 2 is skipped");
        }

        // Case C: Pay both hand discard AND self_lv4 remove -> prompts opponent removal & removes opp_c
        {
            let mut engine = MatchEngine::new(catalog.clone());
            engine.turn = 4;
            engine.turn_player_id = "p1".to_string();
            engine.player_order = vec!["p1".to_string(), "p2".to_string()];
            engine.phase = models::MatchPhase::END_TRIGGERS;

            let mut ps1 = engine::PlayerState::new("p1".to_string());
            let mut ps2 = engine::PlayerState::new("p2".to_string());
            ps1.partner = Some(test_inst("p1_p", "B03106", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
            ps1.r#case = Some(test_inst("p1_c", "B01024", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
            ps2.r#case = Some(test_inst("p2_c", "B01024", "p2", models::Zone::CASE, models::CardOrientation::ACTIVE));
            ps1.field.push(test_inst("p1_gin", "B07094", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
            ps1.field.push(test_inst("self_lv4", "B01011", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
            ps1.hand.push(test_inst("h1", "B01011", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
            ps2.field.push(test_inst("opp_c", "B01014", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE));
            for i in 0..4 {
                ps1.deck.push(test_inst(&format!("c_d1_{}", i), "B01011", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
                ps2.deck.push(test_inst(&format!("c_d2_{}", i), "B01011", "p2", models::Zone::DECK, models::CardOrientation::ACTIVE));
            }
            engine.players.insert("p1".to_string(), ps1);
            engine.players.insert("p2".to_string(), ps2);

            engine.trigger_on_turn_end("p1");
            let pd1 = engine.pending_decision.clone().unwrap();
            let _ = engine.submit_decision("p1", &serde_json::json!({
                "decisionId": pd1["decisionId"],
                "revision": engine.revision,
                "value": {"targetId": "h1", "actorId": "p1_gin", "effectType": "DISCARD_HAND"}
            }));
            let pd2 = engine.pending_decision.clone().unwrap();
            let _ = engine.submit_decision("p1", &serde_json::json!({
                "decisionId": pd2["decisionId"],
                "revision": engine.revision,
                "value": {"targetId": "self_lv4", "actorId": "p1_gin", "effectType": "REMOVE"}
            }));

            assert!(engine.players.get("p1").unwrap().remove.iter().any(|c| c.instance_id == "self_lv4"));
            let pd3 = engine.pending_decision.clone().expect("Step 3 character REMOVE must prompt after paying both costs");
            assert_eq!(pd3["kind"], "targetSelect");
            assert_eq!(pd3["options"]["effectType"], "REMOVE");
            let cands3: Vec<&str> = pd3["options"]["candidates"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|c| c["instanceId"].as_str())
                .collect();
            assert!(cands3.contains(&"opp_c"));
            let _ = engine.submit_decision("p1", &serde_json::json!({
                "decisionId": pd3["decisionId"],
                "revision": engine.revision,
                "value": {"targetId": "opp_c", "actorId": "p1_gin", "effectType": "REMOVE"}
            }));
            assert!(engine.players.get("p1").unwrap().remove.iter().any(|c| c.instance_id == "self_lv4"));
            assert!(engine.players.get("p2").unwrap().remove.iter().any(|c| c.instance_id == "opp_c"));
        }

        // Case D: Cross-card B04049 (このキャラをスリープさせ、手札からFBIを1枚リムーブしてもよい) skip leaves self ACTIVE and opp intact
        {
            let mut engine = MatchEngine::new(catalog);
            engine.turn = 4;
            engine.turn_player_id = "p1".to_string();
            engine.player_order = vec!["p1".to_string(), "p2".to_string()];
            engine.phase = models::MatchPhase::MAIN;

            let mut ps1 = engine::PlayerState::new("p1".to_string());
            let mut ps2 = engine::PlayerState::new("p2".to_string());
            ps1.partner = Some(test_inst("p1_p", "B01060", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE)); // 赤
            ps1.r#case = Some(test_inst("p1_c", "B01024", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
            for i in 0..8 {
                ps1.file.push(test_inst(&format!("f{}", i), "B01062", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
            }
            ps1.hand.push(test_inst("h_subaru", "B04049", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
            ps1.hand.push(test_inst("h_fbi", "B01062", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
            ps2.field.push(test_inst("opp_c", "B01014", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE));
            engine.players.insert("p1".to_string(), ps1);
            engine.players.insert("p2".to_string(), ps2);

            engine.prompt_main_action("p1");
            let pd1 = engine.pending_decision.clone().unwrap();
            let _ = engine.submit_decision("p1", &serde_json::json!({
                "decisionId": pd1["decisionId"],
                "revision": engine.revision,
                "value": {"type": "HAND_PLAY", "cardId": "h_subaru"}
            }));
            let pd_opt = engine.pending_decision.clone().expect("B04049 optional DISCARD_HAND must prompt");
            assert_eq!(pd_opt["options"]["effectType"], "DISCARD_HAND");
            let _ = engine.submit_decision("p1", &serde_json::json!({
                "decisionId": pd_opt["decisionId"],
                "revision": engine.revision,
                "value": {"skip": true}
            }));
            let subaru = engine.players.get("p1").unwrap().field.iter().find(|c| c.instance_id == "h_subaru").unwrap();
            assert_eq!(subaru.orientation, models::CardOrientation::ACTIVE, "Skipping B04049 optional cost must not sleep self");
            assert!(engine.players.get("p2").unwrap().field.iter().any(|c| c.instance_id == "opp_c"));
        }
    }

    #[test]
    fn test_bug5_b11009_mitsuhiko_contact_only_target_and_contact_abort() {
        let catalog = load_test_catalog();

        // 1) Fewer than 2 distinct lv4 少年探偵団 => B11009 cutin is NOT available
        {
            let mut engine = MatchEngine::new(catalog.clone());
            engine.turn = 4;
            engine.turn_player_id = "p1".to_string();
            engine.player_order = vec!["p1".to_string(), "p2".to_string()];
            engine.phase = models::MatchPhase::MAIN;

            let mut ps1 = engine::PlayerState::new("p1".to_string());
            let mut ps2 = engine::PlayerState::new("p2".to_string());
            ps1.partner = Some(test_inst("p1_p", "B01001", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
            ps1.r#case = Some(test_inst("p1_c", "B01024", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
            ps1.field.push(test_inst("p1_conan", "B01011", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
            ps1.hand.push(test_inst("h_b11009", "B11009", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
            ps2.field.push(test_inst("p2_contact", "B01011", "p2", models::Zone::FIELD, models::CardOrientation::SLEEP));
            engine.players.insert("p1".to_string(), ps1);
            engine.players.insert("p2".to_string(), ps2);

            engine.prompt_main_action("p1");
            let pd1 = engine.pending_decision.clone().unwrap();
            let _ = engine.submit_decision("p1", &serde_json::json!({
                "decisionId": pd1["decisionId"],
                "revision": engine.revision,
                "value": {"type": "ACTION", "actorId": "p1_conan", "targetId": "p2_contact"}
            }));
            // Without 2 distinct lv4 少年探偵団, cutin cannot be used so attack resolves directly
            assert!(engine.players.get("p1").unwrap().hand.iter().any(|c| c.instance_id == "h_b11009"));
        }

        // 2) With 2 distinct lv4 少年探偵団: B11009 only offers contacting lv<=7 chars (NOT non-contacting idle or lv8) and aborts contact on remove
        {
            let mut engine = MatchEngine::new(catalog.clone());
            engine.turn = 4;
            engine.turn_player_id = "p1".to_string();
            engine.player_order = vec!["p1".to_string(), "p2".to_string()];
            engine.phase = models::MatchPhase::MAIN;

            let mut ps1 = engine::PlayerState::new("p1".to_string());
            let mut ps2 = engine::PlayerState::new("p2".to_string());
            ps1.partner = Some(test_inst("p1_p", "B01001", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
            ps1.r#case = Some(test_inst("p1_c", "B01024", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
            ps2.r#case = Some(test_inst("p2_c", "B01024", "p2", models::Zone::CASE, models::CardOrientation::ACTIVE));
            ps1.field.push(test_inst("p1_conan", "B01011", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE)); // lv4 少年探偵団 (江戸川コナン)
            ps1.field.push(test_inst("p1_genta", "B01014", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE)); // lv4 少年探偵団 (小嶋元太)
            ps1.hand.push(test_inst("h_b11009", "B11009", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
            ps1.deck.push(test_inst("dk1", "B01011", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));

            ps2.field.push(test_inst("p2_contact", "B01016", "p2", models::Zone::FIELD, models::CardOrientation::SLEEP)); // lv4 AP4000
            ps2.field.push(test_inst("p2_idle", "B01014", "p2", models::Zone::FIELD, models::CardOrientation::SLEEP));    // lv4 AP5000 (not contacting)
            engine.players.insert("p1".to_string(), ps1);
            engine.players.insert("p2".to_string(), ps2);

            engine.prompt_main_action("p1");
            let pd1 = engine.pending_decision.clone().unwrap();
            let _ = engine.submit_decision("p1", &serde_json::json!({
                "decisionId": pd1["decisionId"],
                "revision": engine.revision,
                "value": {"type": "ACTION", "actorId": "p1_conan", "targetId": "p2_contact"}
            }));

            let pd_cut = engine.pending_decision.clone().expect("B11009 cutin must be offered");
            assert_eq!(pd_cut["kind"], "guard");
            let _ = engine.submit_decision("p1", &serde_json::json!({
                "decisionId": pd_cut["decisionId"],
                "revision": engine.revision,
                "value": {"cutinCardId": "h_b11009"}
            }));

            let pd_ts = engine.pending_decision.clone().expect("B11009 must prompt targetSelect");
            assert_eq!(pd_ts["kind"], "targetSelect");
            assert_eq!(pd_ts["options"]["effectType"], "REMOVE");
            let cands: Vec<&str> = pd_ts["options"]["candidates"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|c| c["instanceId"].as_str())
                .collect();
            assert!(cands.contains(&"p2_contact"), "Contacting lv<=7 opponent char must be selectable");
            assert!(!cands.contains(&"p2_idle"), "Non-contacting field char must NOT be selectable");
            assert!(!cands.contains(&"p1_genta"), "Non-contacting own field char must NOT be selectable");

            let _ = engine.submit_decision("p1", &serde_json::json!({
                "decisionId": pd_ts["decisionId"],
                "revision": engine.revision,
                "value": {"targetId": "p2_contact", "actorId": "h_b11009", "effectType": "REMOVE"}
            }));

            assert!(engine.players.get("p2").unwrap().remove.iter().any(|c| c.instance_id == "p2_contact"));
            assert!(engine.players.get("p2").unwrap().field.iter().any(|c| c.instance_id == "p2_idle"));
            assert!(engine.action_context.is_none(), "Contact must abort when contacting char leaves field");
            assert_eq!(engine.pending_decision.as_ref().unwrap()["kind"], "mainAction");
            assert!(engine.players.get("p1").unwrap().field.iter().any(|c| c.instance_id == "p1_conan"), "Attacker stays alive after contact abort");
            assert!(engine.players.get("p1").unwrap().hand.iter().any(|c| c.instance_id == "dk1"), "B11009 must draw 1 card");
        }

        // 3) Cross-card B01098 (コンタクト中のすべてのキャラをリムーブする): only removes contacting chars, leaves non-contacting chars
        {
            let mut engine = MatchEngine::new(catalog);
            engine.turn = 4;
            engine.turn_player_id = "p1".to_string();
            engine.player_order = vec!["p1".to_string(), "p2".to_string()];
            engine.phase = models::MatchPhase::MAIN;

            let mut ps1 = engine::PlayerState::new("p1".to_string());
            let mut ps2 = engine::PlayerState::new("p2".to_string());
            ps1.partner = Some(test_inst("p1_p", "B03106", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
            ps1.r#case = Some(test_inst("p1_c", "B01024", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
            ps1.field.push(test_inst("p1_atk", "B01098", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
            ps1.hand.push(test_inst("h_b01098", "B01098", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
            ps2.field.push(test_inst("p2_contact", "B01005", "p2", models::Zone::FIELD, models::CardOrientation::SLEEP));
            ps2.field.push(test_inst("p2_idle", "B01014", "p2", models::Zone::FIELD, models::CardOrientation::SLEEP));
            engine.players.insert("p1".to_string(), ps1);
            engine.players.insert("p2".to_string(), ps2);

            engine.prompt_main_action("p1");
            let pd1 = engine.pending_decision.clone().unwrap();
            let _ = engine.submit_decision("p1", &serde_json::json!({
                "decisionId": pd1["decisionId"],
                "revision": engine.revision,
                "value": {"type": "ACTION", "actorId": "p1_atk", "targetId": "p2_contact"}
            }));
            let pd_cut = engine.pending_decision.clone().unwrap();
            let _ = engine.submit_decision("p1", &serde_json::json!({
                "decisionId": pd_cut["decisionId"],
                "revision": engine.revision,
                "value": {"cutinCardId": "h_b01098"}
            }));

            assert!(engine.players.get("p1").unwrap().remove.iter().any(|c| c.instance_id == "p1_atk"));
            assert!(engine.players.get("p2").unwrap().remove.iter().any(|c| c.instance_id == "p2_contact"));
            assert!(engine.players.get("p2").unwrap().field.iter().any(|c| c.instance_id == "p2_idle"), "B01098 must NOT remove non-contacting chars");
        }
    }

    #[test]
    fn test_id0371_sherry_single_discard_and_cross_card_deck_search_conditionals() {
        let catalog = load_test_catalog();

        // 1) ID0371 シェリー (B03122 / D07019): 複数手札があっても手札に加えた後のリムーブは1回（1枚）のみ
        for (tag, def_id) in [("B03122", "B03122"), ("D07019", "D07019")] {
            let mut engine = MatchEngine::new(catalog.clone());
            engine.turn = 4;
            engine.turn_player_id = "p1".to_string();
            engine.player_order = vec!["p1".to_string(), "p2".to_string()];
            engine.phase = models::MatchPhase::MAIN;

            let mut ps1 = engine::PlayerState::new("p1".to_string());
            let ps2 = engine::PlayerState::new("p2".to_string());
            ps1.partner = Some(test_inst("p1_p", "B03106", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
            ps1.r#case = Some(test_inst("p1_c", "B01024", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
            for i in 0..8 {
                ps1.file.push(test_inst(&format!("f{}", i), "B01021", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
            }
            ps1.hand.push(test_inst("h_sherry", def_id, "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
            ps1.hand.push(test_inst("h_ex1", "B01021", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
            ps1.hand.push(test_inst("h_ex2", "B01020", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
            for i in 0..3 {
                ps1.deck.push(test_inst(&format!("dk_other_{}", i), "B01021", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
            }
            ps1.deck.push(test_inst("dk_black", "B03122", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
            engine.players.insert("p1".to_string(), ps1);
            engine.players.insert("p2".to_string(), ps2);

            engine.prompt_main_action("p1");
            let pd_main = engine.pending_decision.clone().unwrap();
            let _ = engine.submit_decision("p1", &serde_json::json!({
                "decisionId": pd_main["decisionId"],
                "revision": engine.revision,
                "value": {"type": "HAND_PLAY", "cardId": "h_sherry"}
            }));

            let pd_ds = engine.pending_decision.clone().expect("deckSearch must open");
            assert_eq!(pd_ds["kind"], "deckSearch");
            assert_eq!(pd_ds["options"]["postAddDiscard"], 1);
            let _ = engine.submit_decision("p1", &serde_json::json!({
                "decisionId": pd_ds["decisionId"],
                "revision": engine.revision,
                "value": {"chosenCardId": "dk_black", "actorId": "h_sherry"}
            }));

            let pd_disc = engine.pending_decision.clone().expect("single DISCARD_HAND prompt expected");
            assert_eq!(pd_disc["kind"], "targetSelect");
            assert_eq!(pd_disc["options"]["effectType"], "DISCARD_HAND");
            assert_eq!(pd_disc["options"]["maxSelect"], 1);
            assert_eq!(pd_disc["options"]["optional"], false);

            let _ = engine.submit_decision("p1", &serde_json::json!({
                "decisionId": pd_disc["decisionId"],
                "revision": engine.revision,
                "value": {"targetId": "h_ex1", "actorId": "h_sherry", "effectType": "DISCARD_HAND"}
            }));

            assert_eq!(
                engine.pending_decision.as_ref().unwrap()["kind"],
                "mainAction",
                "{} must NOT force a second hand discard",
                tag
            );
            let p1_after = engine.players.get("p1").unwrap();
            assert_eq!(p1_after.hand.len(), 2, "{} hand should be h_ex2 + dk_black", tag);
            assert_eq!(p1_after.remove.len(), 1, "{} remove should have only h_ex1", tag);
            assert!(p1_after.remove.iter().any(|c| c.instance_id == "h_ex1"));

            // Skip picking in deckSearch -> 0 discards
            {
                let ps1m = engine.players.get_mut("p1").unwrap();
                ps1m.normal_hand_played_this_turn = false;
                ps1m.hand.push(test_inst("h_sherry2", def_id, "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
                for i in 0..4 {
                    ps1m.deck.push(test_inst(&format!("dk_skip_{}", i), "B03122", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
                }
            }
            let pd_main2 = engine.pending_decision.clone().unwrap();
            let _ = engine.submit_decision("p1", &serde_json::json!({
                "decisionId": pd_main2["decisionId"],
                "revision": engine.revision,
                "value": {"type": "HAND_PLAY", "cardId": "h_sherry2"}
            }));
            let pd_ds2 = engine.pending_decision.clone().unwrap();
            assert_eq!(pd_ds2["kind"], "deckSearch");
            let _ = engine.submit_decision("p1", &serde_json::json!({
                "decisionId": pd_ds2["decisionId"],
                "revision": engine.revision,
                "value": {"chosenCardIds": [], "actorId": "h_sherry2"}
            }));
            assert_eq!(engine.pending_decision.as_ref().unwrap()["kind"], "mainAction");
            assert_eq!(engine.players.get("p1").unwrap().remove.len(), 1);
        }

        // 2) Cross-card B10054 (レベル6以下のカードを手札に加えた場合、手札を1枚リムーブする)
        for (pick_lv6, expect_discard) in [(true, true), (false, false)] {
            let mut engine = MatchEngine::new(catalog.clone());
            engine.turn = 4;
            engine.turn_player_id = "p1".to_string();
            engine.player_order = vec!["p1".to_string(), "p2".to_string()];
            engine.phase = models::MatchPhase::MAIN;

            let mut ps1 = engine::PlayerState::new("p1".to_string());
            let ps2 = engine::PlayerState::new("p2".to_string());
            ps1.partner = Some(test_inst("p1_p", "B01060", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
            ps1.r#case = Some(test_inst("p1_c", "B03138", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE)); // 赤女の悲劇 (赤/黄)
            for i in 0..8 {
                ps1.file.push(test_inst(&format!("f{}", i), "B01021", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
            }
            ps1.hand.push(test_inst("h_niide", "B10054", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
            ps1.hand.push(test_inst("h_ex1", "B01021", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
            ps1.hand.push(test_inst("h_ex2", "B01020", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
            ps1.deck.push(test_inst("dk1", "B01021", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
            ps1.deck.push(test_inst("dk2", "B01021", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
            ps1.deck.push(test_inst("dk_lv6", "B04051", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE)); // Lv6 赤
            ps1.deck.push(test_inst("dk_lv8", "B01062", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE)); // Lv8 赤
            engine.players.insert("p1".to_string(), ps1);
            engine.players.insert("p2".to_string(), ps2);

            engine.prompt_main_action("p1");
            let pd_main = engine.pending_decision.clone().unwrap();
            let _ = engine.submit_decision("p1", &serde_json::json!({
                "decisionId": pd_main["decisionId"],
                "revision": engine.revision,
                "value": {"type": "HAND_PLAY", "cardId": "h_niide"}
            }));
            let pd_ds = engine.pending_decision.clone().unwrap();
            assert_eq!(pd_ds["kind"], "deckSearch");
            let chosen_id = if pick_lv6 { "dk_lv6" } else { "dk_lv8" };
            let _ = engine.submit_decision("p1", &serde_json::json!({
                "decisionId": pd_ds["decisionId"],
                "revision": engine.revision,
                "value": {"chosenCardId": chosen_id, "actorId": "h_niide"}
            }));
            if expect_discard {
                let pd_disc = engine.pending_decision.clone().unwrap();
                assert_eq!(pd_disc["kind"], "targetSelect");
                assert_eq!(pd_disc["options"]["effectType"], "DISCARD_HAND");
                assert_eq!(pd_disc["options"]["maxSelect"], 1);
                let _ = engine.submit_decision("p1", &serde_json::json!({
                    "decisionId": pd_disc["decisionId"],
                    "revision": engine.revision,
                    "value": {"targetId": "h_ex1", "actorId": "h_niide", "effectType": "DISCARD_HAND"}
                }));
                assert_eq!(engine.pending_decision.as_ref().unwrap()["kind"], "mainAction");
                assert_eq!(engine.players.get("p1").unwrap().remove.len(), 1);
            } else {
                assert_eq!(engine.pending_decision.as_ref().unwrap()["kind"], "mainAction", "Lv8 pick must NOT trigger discard");
                assert_eq!(engine.players.get("p1").unwrap().remove.len(), 0);
            }
        }

        // 3) Cross-card B08050 (諸星大/宮野志保/宮野エレーナ/宮野厚司 以外を手札に加えた場合、手札を1枚リムーブ)
        for (pick_excluded, expect_discard) in [(true, false), (false, true)] {
            let mut engine = MatchEngine::new(catalog.clone());
            engine.turn = 4;
            engine.turn_player_id = "p1".to_string();
            engine.player_order = vec!["p1".to_string(), "p2".to_string()];
            engine.phase = models::MatchPhase::MAIN;

            let mut ps1 = engine::PlayerState::new("p1".to_string());
            let ps2 = engine::PlayerState::new("p2".to_string());
            ps1.partner = Some(test_inst("p1_p", "B01060", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
            ps1.r#case = Some(test_inst("p1_c", "B01024", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
            for i in 0..8 {
                ps1.file.push(test_inst(&format!("f{}", i), "B01021", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
            }
            ps1.hand.push(test_inst("h_akemi", "B08050", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
            ps1.hand.push(test_inst("h_ex1", "B01021", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
            ps1.hand.push(test_inst("h_ex2", "B01020", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
            ps1.deck.push(test_inst("dk1", "B01021", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
            ps1.deck.push(test_inst("dk_shiho", "B01018", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
            ps1.deck.push(test_inst("dk_sherry", "B03122", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
            engine.players.insert("p1".to_string(), ps1);
            engine.players.insert("p2".to_string(), ps2);

            engine.prompt_main_action("p1");
            let pd_main = engine.pending_decision.clone().unwrap();
            let _ = engine.submit_decision("p1", &serde_json::json!({
                "decisionId": pd_main["decisionId"],
                "revision": engine.revision,
                "value": {"type": "HAND_PLAY", "cardId": "h_akemi"}
            }));
            let pd_ds = engine.pending_decision.clone().unwrap();
            assert_eq!(pd_ds["kind"], "deckSearch");
            let chosen_id = if pick_excluded { "dk_shiho" } else { "dk_sherry" };
            let _ = engine.submit_decision("p1", &serde_json::json!({
                "decisionId": pd_ds["decisionId"],
                "revision": engine.revision,
                "value": {"chosenCardId": chosen_id, "actorId": "h_akemi"}
            }));
            if expect_discard {
                let pd_disc = engine.pending_decision.clone().unwrap();
                assert_eq!(pd_disc["kind"], "targetSelect");
                assert_eq!(pd_disc["options"]["effectType"], "DISCARD_HAND");
                let _ = engine.submit_decision("p1", &serde_json::json!({
                    "decisionId": pd_disc["decisionId"],
                    "revision": engine.revision,
                    "value": {"targetId": "h_ex1", "actorId": "h_akemi", "effectType": "DISCARD_HAND"}
                }));
                assert_eq!(engine.pending_decision.as_ref().unwrap()["kind"], "mainAction");
                assert!(engine.players.get("p1").unwrap().remove.iter().any(|c| c.instance_id == "h_ex1"));
            } else {
                assert_eq!(engine.pending_decision.as_ref().unwrap()["kind"], "mainAction", "宮野志保 must NOT trigger discard");
                assert!(!engine.players.get("p1").unwrap().remove.iter().any(|c| c.zone == models::Zone::REMOVE && c.instance_id == "h_ex1"));
            }
        }

        // 4) Cross-card B07004 (キャラを登場させた場合、手札を1枚リムーブする)
        for (deploy_genta, expect_discard) in [(true, true), (false, false)] {
            let mut engine = MatchEngine::new(catalog.clone());
            engine.turn = 4;
            engine.turn_player_id = "p1".to_string();
            engine.player_order = vec!["p1".to_string(), "p2".to_string()];
            engine.phase = models::MatchPhase::MAIN;

            let mut ps1 = engine::PlayerState::new("p1".to_string());
            let ps2 = engine::PlayerState::new("p2".to_string());
            ps1.partner = Some(test_inst("p1_p", "B01001", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
            ps1.r#case = Some(test_inst("p1_c", "B01024", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
            ps1.field.push(test_inst("f_ayumi", "B07004", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
            ps1.hand.push(test_inst("h_ex1", "B01021", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
            ps1.hand.push(test_inst("h_ex2", "B01020", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
            for i in 0..3 {
                ps1.deck.push(test_inst(&format!("dk{}", i), "B01021", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
            }
            ps1.deck.push(test_inst("dk_genta", "B01014", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
            engine.players.insert("p1".to_string(), ps1);
            engine.players.insert("p2".to_string(), ps2);

            engine.prompt_main_action("p1");
            let pd_main = engine.pending_decision.clone().unwrap();
            let _ = engine.submit_decision("p1", &serde_json::json!({
                "decisionId": pd_main["decisionId"],
                "revision": engine.revision,
                "value": {"type": "DECLARE", "actorId": "f_ayumi"}
            }));
            let pd_ds = engine.pending_decision.clone().unwrap();
            assert_eq!(pd_ds["kind"], "deckSearch");
            let val = if deploy_genta {
                serde_json::json!({"chosenCardId": "dk_genta", "actorId": "f_ayumi"})
            } else {
                serde_json::json!({"chosenCardIds": [], "actorId": "f_ayumi"})
            };
            let _ = engine.submit_decision("p1", &serde_json::json!({
                "decisionId": pd_ds["decisionId"],
                "revision": engine.revision,
                "value": val
            }));
            if expect_discard {
                let pd_disc = engine.pending_decision.clone().unwrap();
                assert_eq!(pd_disc["kind"], "targetSelect");
                assert_eq!(pd_disc["options"]["effectType"], "DISCARD_HAND");
                let _ = engine.submit_decision("p1", &serde_json::json!({
                    "decisionId": pd_disc["decisionId"],
                    "revision": engine.revision,
                    "value": {"targetId": "h_ex1", "actorId": "f_ayumi", "effectType": "DISCARD_HAND"}
                }));
                assert_eq!(engine.pending_decision.as_ref().unwrap()["kind"], "mainAction");
                assert!(engine.players.get("p1").unwrap().field.iter().any(|c| c.instance_id == "dk_genta"));
                assert_eq!(engine.players.get("p1").unwrap().remove.len(), 1);
            } else {
                assert_eq!(engine.pending_decision.as_ref().unwrap()["kind"], "mainAction");
                assert_eq!(engine.players.get("p1").unwrap().remove.len(), 0);
            }
        }

        // 5) Cross-card PR265 (カードを手札に加えた場合、自分のデッキのカードを上からそのカードのレベルと同じ枚数リムーブする)
        {
            let mut engine = MatchEngine::new(catalog);
            engine.turn = 4;
            engine.turn_player_id = "p1".to_string();
            engine.player_order = vec!["p1".to_string(), "p2".to_string()];
            engine.phase = models::MatchPhase::MAIN;

            let mut ps1 = engine::PlayerState::new("p1".to_string());
            let ps2 = engine::PlayerState::new("p2".to_string());
            ps1.partner = Some(test_inst("p1_p", "B01060", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
            ps1.r#case = Some(test_inst("p1_c", "B01024", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
            for i in 0..8 {
                ps1.file.push(test_inst(&format!("f{}", i), "B01021", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
            }
            ps1.hand.push(test_inst("h_kazami", "PR265", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
            for i in 0..6 {
                ps1.deck.push(test_inst(&format!("dk_rem_{}", i), "B01021", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
            }
            ps1.deck.push(test_inst("dk_sato", "B01089", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE)); // Lv5 警視庁
            engine.players.insert("p1".to_string(), ps1);
            engine.players.insert("p2".to_string(), ps2);

            engine.prompt_main_action("p1");
            let pd_main = engine.pending_decision.clone().unwrap();
            let _ = engine.submit_decision("p1", &serde_json::json!({
                "decisionId": pd_main["decisionId"],
                "revision": engine.revision,
                "value": {"type": "HAND_PLAY", "cardId": "h_kazami"}
            }));
            let pd_ds = engine.pending_decision.clone().unwrap();
            assert_eq!(pd_ds["kind"], "deckSearch");
            let _ = engine.submit_decision("p1", &serde_json::json!({
                "decisionId": pd_ds["decisionId"],
                "revision": engine.revision,
                "value": {"chosenCardId": "dk_sato", "actorId": "h_kazami"}
            }));

            assert_eq!(engine.pending_decision.as_ref().unwrap()["kind"], "mainAction");
            let p1_after = engine.players.get("p1").unwrap();
            assert!(p1_after.hand.iter().any(|c| c.instance_id == "dk_sato"));
            assert_eq!(p1_after.remove.len(), 5, "PR265 must mill exact level (5) cards from deck");
        }
    }

    #[test]
    fn test_self_ability_remove_revive_sleep_vodka_and_cross_cards() {
        let catalog = catalog::CardCatalog::load_from_json("../data/dummy/cards.json").unwrap();
        let vodka_id = catalog.definitions.keys().find(|k| k.starts_with("B11085")).cloned().expect("B11085 must exist in cards.json");
        let black_partner_id = catalog.definitions.values()
            .find(|d| d.card_type == models::CardType::PARTNER && d.colors.iter().any(|c| c.contains("黒")))
            .map(|d| d.definition_id.clone())
            .expect("black partner must exist");

        let mut engine = engine::MatchEngine::new(catalog);
        engine.turn = 1;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        ps1.partner = Some(models::CardInstance {
            instance_id: "p1_partner".to_string(),
            definition_id: black_partner_id,
            owner_id: "p1".to_string(),
            zone: models::Zone::PARTNER,
            orientation: models::CardOrientation::ACTIVE,
            ..Default::default()
        });
        ps1.field.push(models::CardInstance {
            instance_id: "vodka_1".to_string(),
            definition_id: vodka_id.clone(),
            owner_id: "p1".to_string(),
            zone: models::Zone::FIELD,
            orientation: models::CardOrientation::ACTIVE,
            ..Default::default()
        });
        ps1.field.push(models::CardInstance {
            instance_id: "vodka_2".to_string(),
            definition_id: vodka_id.clone(),
            owner_id: "p1".to_string(),
            zone: models::Zone::FIELD,
            orientation: models::CardOrientation::SLEEP,
            ..Default::default()
        });
        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), engine::PlayerState::new("p2".to_string()));

        // 1) アクションしていない vodka_1 が自分の効果でリムーブされても復活しない
        {
            let ps = engine.players.get_mut("p1").unwrap();
            let pos = ps.field.iter().position(|c| c.instance_id == "vodka_1").unwrap();
            let mut c = ps.field.remove(pos);
            c.zone = models::Zone::REMOVE;
            ps.remove.push(c);
        }
        engine.trigger_on_remove_char("p1", "vodka_1", &vodka_id, true);
        assert!(engine.players.get("p1").unwrap().remove.iter().any(|c| c.instance_id == "vodka_1"));
        assert!(!engine.players.get("p1").unwrap().field.iter().any(|c| c.instance_id == "vodka_1"));

        // 2) アクションしていた vodka_2 が自分の効果でリムーブされたらスリープ状態で現場に登場する
        engine.acted_this_turn_ids.insert("vodka_2".to_string());
        {
            let ps = engine.players.get_mut("p1").unwrap();
            let pos = ps.field.iter().position(|c| c.instance_id == "vodka_2").unwrap();
            let mut c = ps.field.remove(pos);
            c.zone = models::Zone::REMOVE;
            ps.remove.push(c);
        }
        engine.trigger_on_remove_char("p1", "vodka_2", &vodka_id, true);
        let v2_field = engine.players.get("p1").unwrap().field.iter().find(|c| c.instance_id == "vodka_2");
        assert!(v2_field.is_some(), "vodka_2 must revive to field when acted this turn");
        assert_eq!(v2_field.unwrap().orientation, models::CardOrientation::SLEEP);

        // 3) 相手の効果でリムーブされた場合は復活しない
        {
            let ps = engine.players.get_mut("p1").unwrap();
            let pos = ps.field.iter().position(|c| c.instance_id == "vodka_2").unwrap();
            let mut c = ps.field.remove(pos);
            c.zone = models::Zone::REMOVE;
            ps.remove.push(c);
        }
        engine.trigger_on_remove_char("p1", "vodka_2", &vodka_id, false);
        assert!(!engine.players.get("p1").unwrap().field.iter().any(|c| c.instance_id == "vodka_2"));
    }

    #[test]
    fn test_b11082_gin_vermouth_declare_stun_and_grant_on_remove_draw() {
        let catalog = catalog::CardCatalog::load_from_json("../data/dummy/cards.json").unwrap();
        let gv_id = catalog.definitions.keys().find(|k| k.starts_with("B11082")).cloned().expect("B11082 must exist");
        let gv_def = catalog.definitions.get(&gv_id).unwrap().clone();
        let declare_clauses = effects::extract_clauses(&gv_def.description, effects::TriggerKind::Declare, gv_def.card_type);
        assert!(!declare_clauses.is_empty(), "B11082 must have Declare clause");
        let plan = effects::parse_effect_steps(&declare_clauses[0], None, gv_def.card_type);
        assert!(plan.immediate_before.is_empty(), "Must not draw immediately without cost");
        assert!(plan.target_step.is_some());
        assert_eq!(plan.target_step.as_ref().unwrap().action, effects::TargetAction::StunAndGrantOnRemoveDraw);

        let mut engine = engine::MatchEngine::new(catalog);
        engine.turn = 1;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        ps1.field.push(models::CardInstance {
            instance_id: "blk_lv6".to_string(),
            definition_id: "B11085".to_string(),
            owner_id: "p1".to_string(),
            zone: models::Zone::FIELD,
            orientation: models::CardOrientation::ACTIVE,
            ..Default::default()
        });
        ps1.deck.push(models::CardInstance {
            instance_id: "d1".to_string(),
            definition_id: "B11085".to_string(),
            owner_id: "p1".to_string(),
            zone: models::Zone::DECK,
            ..Default::default()
        });
        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), engine::PlayerState::new("p2".to_string()));

        // 効果適用をシミュレート
        engine.execute_clause_plan("p1", "gv_1", "ジン＆ベルモット", Some(9), &plan, "MAIN", None);
        let pd = engine.pending_decision.clone().expect("Must prompt target selection");
        assert_eq!(pd["kind"], "targetSelect");
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd["decisionId"],
            "revision": engine.revision,
            "value": {"targetId": "blk_lv6"}
        }));

        // 対象がスタンし、付与されていること
        let target = engine.players.get("p1").unwrap().field.iter().find(|c| c.instance_id == "blk_lv6").unwrap();
        assert_eq!(target.orientation, models::CardOrientation::STUN);
        assert!(engine.granted_on_remove_draw_until_opp_end.contains_key("blk_lv6"));

        // 現場からリムーブされたら1枚引くこと
        let hand_before = engine.players.get("p1").unwrap().hand.len();
        engine.trigger_on_remove_char("p1", "blk_lv6", "B11085", false);
        assert_eq!(engine.players.get("p1").unwrap().hand.len(), hand_before + 1);
    }

    #[test]
    fn test_b03006_shounen_tanteidan_on_enter_stack() {
        let catalog = catalog::CardCatalog::load_from_json("../data/dummy/cards.json").unwrap();
        let card = catalog.get("B03006").expect("B03006 must exist in catalog");
        let clauses = effects::extract_clauses(&card.description, effects::TriggerKind::OnPlay, models::CardType::CHARACTER);
        assert!(!clauses.is_empty(), "Must extract 【登場時】");
        let plan = effects::parse_effect_steps(&clauses[0], Some(8), models::CardType::CHARACTER);
        let ts = plan.target_step.as_ref().expect("Must have target_step");
        assert_eq!(ts.action, effects::TargetAction::StackUnderSelf);
        assert_eq!(ts.filter.zone, effects::TargetZone::Remove);
        assert_eq!(ts.filter.count, 5);
    }

    #[test]
    fn test_parse_official_deck_html() {
        let html = r#"<html><body><form><input type="hidden" name="deck" value="{&quot;partner&quot;:[&quot;B01001&quot;],&quot;incident&quot;:[&quot;B01002&quot;],&quot;other&quot;:[&quot;B01003&quot;,&quot;B01003&quot;,&quot;B01004&quot;]}"></form></body></html>"#;
        let deck = parse_official_deck_html(html).expect("Must parse valid official deck html");
        assert_eq!(deck["partner"][0], "B01001");
        assert_eq!(deck["incident"][0], "B01002");
        assert_eq!(deck["other"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn test_b06103_gin_hand_declare() {
        let catalog = catalog::CardCatalog::load_from_json("../data/dummy/cards.json").unwrap();
        let mut engine = engine::MatchEngine::new(catalog.clone());
        engine.turn = 2;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let mut cs = models::CardInstance {
            instance_id: "case1".to_string(),
            definition_id: "B01002".to_string(),
            owner_id: "p1".to_string(),
            zone: models::Zone::CASE,
            ..Default::default()
        };
        cs.orientation = models::CardOrientation::ACTIVE;
        ps1.r#case = Some(cs);
        ps1.case_phase = models::CasePhase::RESOLUTION;

        ps1.field.push(models::CardInstance {
            instance_id: "blk_1".to_string(),
            definition_id: "B11085".to_string(),
            owner_id: "p1".to_string(),
            zone: models::Zone::FIELD,
            ..Default::default()
        });
        ps1.hand.push(models::CardInstance {
            instance_id: "gin_h".to_string(),
            definition_id: "B06103".to_string(),
            owner_id: "p1".to_string(),
            zone: models::Zone::HAND,
            ..Default::default()
        });
        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), engine::PlayerState::new("p2".to_string()));
        engine.prompt_main_action("p1");

        let b06103 = catalog.get("B06103").unwrap();
        let clauses = effects::extract_clauses(&b06103.description, effects::TriggerKind::Declare, b06103.card_type);
        assert!(!clauses.is_empty(), "B06103 must have Declare clause: {}", b06103.description);
        let obs = build_player_observation(&engine, "p1");
        let actions = obs["pendingDecision"]["options"]["actions"]
            .as_array()
            .expect("pendingDecision.options.actions must exist");
        let declare_action = actions.iter().find(|a| a["type"] == "DECLARE")
            .unwrap_or_else(|| panic!("DECLARE action must exist; clauses={:?}, actions={:?}", clauses, actions));
        let actors = declare_action["actors"].as_array().expect("actors must exist");
        assert!(
            actors.iter().any(|a| a["actorId"].as_str() == Some("gin_h")),
            "手札のB06103ジンがDECLAREのactorsに含まれること: {:?}",
            actors
        );
    }

    #[test]
    fn test_index_html_no_undefined_is_secret_in_card_slot() {
        let html = std::fs::read_to_string("../src/conan_engine/web/static/index.html")
            .expect("index.html must be readable");
        let start = html.find("function createCardSlot(").expect("createCardSlot must exist");
        let end = html[start..].find("\n    function ").map(|i| start + i).unwrap_or(html.len());
        let fn_body = &html[start..end];
        assert!(
            !fn_body.contains("isSecret"),
            "createCardSlot must not reference undefined variable isSecret"
        );
    }
}

