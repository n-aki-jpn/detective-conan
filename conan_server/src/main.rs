use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
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

// ─── Shared State ──────────────────────────────────────────────────────────────

struct MatchManager {
    catalog_path: PathBuf,
    deck_path: PathBuf,
    catalog: CardCatalog,
    deck_def: DeckDefinition,
    engine: MatchEngine,
    p2_is_cpu: bool,
    cpu_agent: Option<RandomAgent>,
}

impl MatchManager {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let catalog_path = PathBuf::from("data/dummy/cards.json");
        let deck_path = PathBuf::from("data/dummy/deck.json");

        let catalog = CardCatalog::load_from_json(&catalog_path)?;
        let deck_def = Self::load_deck(&deck_path)?;

        let mut mgr = MatchManager {
            catalog_path,
            deck_path,
            catalog: catalog.clone(),
            deck_def: deck_def.clone(),
            engine: MatchEngine::new(catalog),
            p2_is_cpu: true,
            cpu_agent: Some(RandomAgent::new("player2")),
        };

        mgr.reset_match_inner(&deck_def);
        Ok(mgr)
    }

    fn load_deck(path: &PathBuf) -> Result<DeckDefinition, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let deck: DeckDefinition = serde_json::from_str(&content)?;
        Ok(deck)
    }

    fn reset_match_inner(&mut self, deck_def: &DeckDefinition) {
        let mut engine = MatchEngine::new(self.catalog.clone());
        engine.add_player("player1", deck_def);
        engine.add_player("player2", deck_def);
        engine.start_match();
        self.engine = engine;
        self.process_cpu_if_needed();
    }

    fn reset_match(&mut self) {
        let deck_def = match Self::load_deck(&self.deck_path) {
            Ok(d) => d,
            Err(_) => self.deck_def.clone(),
        };
        self.deck_def = deck_def.clone();
        self.reset_match_inner(&deck_def);
    }

    fn save_deck(&mut self, new_def: DeckDefinition) -> Result<(), String> {
        let content = serde_json::to_string_pretty(&new_def)
            .map_err(|e| e.to_string())?;
        std::fs::write(&self.deck_path, content).map_err(|e| e.to_string())?;
        self.deck_def = new_def;
        Ok(())
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

type SharedState = Arc<Mutex<MatchManager>>;

// ─── Request / Response Types ──────────────────────────────────────────────────

#[derive(Deserialize)]
struct PlayerIdQuery {
    player_id: Option<String>,
}

#[derive(Deserialize)]
struct SetCpuRequest {
    #[serde(rename = "isCpu")]
    is_cpu: bool,
}

#[derive(Deserialize)]
struct DecisionRequest {
    #[serde(rename = "playerId")]
    player_id: String,
    value: Option<Value>,
}

#[derive(Serialize)]
struct StatusOk {
    status: &'static str,
}

// ─── Handlers ─────────────────────────────────────────────────────────────────

async fn handle_state(
    State(state): State<SharedState>,
    Query(params): Query<PlayerIdQuery>,
) -> impl IntoResponse {
    let mut mgr = state.lock().unwrap();
    mgr.process_cpu_if_needed();

    let player_id = params
        .player_id
        .filter(|id| mgr.engine.players.contains_key(id.as_str()))
        .unwrap_or_else(|| "player1".to_string());

    let mut obs = build_player_observation(&mgr.engine, &player_id);
    obs["p2IsCpu"] = json!(mgr.p2_is_cpu);

    Json(obs).into_response()
}

async fn handle_catalog(State(state): State<SharedState>) -> impl IntoResponse {
    let mgr = state.lock().unwrap();
    let total = mgr.catalog.definitions.len();
    let cats = mgr.catalog.raw_json.clone();

    // Build normalized response
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
        )
            .into_response(),
    }
}

async fn handle_reset(State(state): State<SharedState>) -> impl IntoResponse {
    let mut mgr = state.lock().unwrap();
    mgr.reset_match();
    Json(StatusOk { status: "ok" }).into_response()
}

async fn handle_set_cpu(
    State(state): State<SharedState>,
    Json(body): Json<SetCpuRequest>,
) -> impl IntoResponse {
    let mut mgr = state.lock().unwrap();
    mgr.p2_is_cpu = body.is_cpu;
    mgr.cpu_agent = if body.is_cpu {
        Some(RandomAgent::new("player2"))
    } else {
        None
    };
    mgr.process_cpu_if_needed();
    Json(StatusOk { status: "ok" }).into_response()
}

async fn handle_save_deck(
    State(state): State<SharedState>,
    Json(body): Json<DeckDefinition>,
) -> impl IntoResponse {
    // Validate
    let total: i32 = body.main_deck.iter().map(|item| item.count).sum();
    if total != 40 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": format!("メインデッキはちょうど40枚である必要があります（現在: {}枚）", total)})),
        )
            .into_response();
    }
    for item in &body.main_deck {
        if item.count > 3 {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": format!("同一カードは最大3枚までです: {}", item.definition_id)})),
            )
                .into_response();
        }
    }

    let mut mgr = state.lock().unwrap();
    if let Err(e) = mgr.save_deck(body) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": e})),
        )
            .into_response();
    }
    mgr.reset_match();
    Json(StatusOk { status: "ok" }).into_response()
}

async fn handle_decision(
    State(state): State<SharedState>,
    Json(body): Json<DecisionRequest>,
) -> impl IntoResponse {
    let mut mgr = state.lock().unwrap();
    let ans = json!({ "value": body.value.unwrap_or(json!({})) });
    match mgr.engine.submit_decision(&body.player_id, &ans) {
        Ok(_) => {
            mgr.process_cpu_if_needed();
            Json(StatusOk { status: "ok" }).into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": e})),
        )
            .into_response(),
    }
}

// ─── Main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let mgr = MatchManager::new().expect("MatchManager 初期化に失敗しました");
    let shared_state: SharedState = Arc::new(Mutex::new(mgr));

    // Static files directory (card images, HTML)
    let static_dir = PathBuf::from("src/conan_engine/web/static");

    let app = Router::new()
        .route("/api/state", get(handle_state))
        .route("/api/catalog", get(handle_catalog))
        .route("/api/deck", get(handle_deck))
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
    println!("🚀 Detective Conan TCG Server (Rust) running at http://localhost:{port}");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
