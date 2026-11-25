use crate::agent::{self, RandomAgent};
use crate::models::DeckDefinition;
use crate::observation::build_player_observation;
use crate::room::{sanitize_deck, MatchRoom, SharedState};
use axum::{
    extract::{FromRequestParts, Path as AxumPath, Query, State},
    http::{request::Parts, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;

// ─── Request / Response Types ──────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct StateQuery {
    pub player_id: Option<String>,
    pub room_id: Option<String>,
}

#[derive(Deserialize)]
pub struct LogsQuery {
    pub room_id: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Deserialize)]
pub struct ReplaysQuery {
    pub limit: Option<usize>,
}

#[derive(Deserialize)]
pub struct SetCpuRequest {
    #[serde(rename = "isCpu")]
    pub is_cpu: bool,
    #[serde(rename = "roomId")]
    pub room_id: Option<String>,
    pub difficulty: Option<agent::CpuDifficulty>,
}

#[derive(Deserialize)]
pub struct DecisionRequest {
    #[serde(rename = "playerId")]
    pub player_id: String,
    #[serde(rename = "roomId")]
    pub room_id: Option<String>,
    pub value: Option<Value>,
}

#[derive(Deserialize)]
pub struct ResetRequest {
    #[serde(rename = "roomId")]
    pub room_id: Option<String>,
    #[serde(rename = "p1DeckId")]
    pub p1_deck_id: Option<String>,
    #[serde(rename = "p2DeckId")]
    pub p2_deck_id: Option<String>,
    #[serde(rename = "p1Deck")]
    pub p1_deck: Option<DeckDefinition>,
    #[serde(rename = "p2Deck")]
    pub p2_deck: Option<DeckDefinition>,
}

#[derive(Deserialize)]
pub struct CreateRoomRequest {
    #[serde(default)]
    pub is_cpu: bool,
    pub deck: Option<DeckDefinition>,
}

#[derive(Deserialize)]
pub struct JoinRoomRequest {
    #[serde(rename = "roomId")]
    pub room_id: String,
    pub deck: Option<DeckDefinition>,
}

#[derive(Serialize)]
pub struct StatusOk {
    pub status: &'static str,
}

// ─── 認証（Bearer トークン）抽出器 ──────────────────────────────────────────────

/// `Authorization: Bearer <token>` ヘッダーからユーザーを解決する抽出器。
#[derive(Clone)]
pub struct AuthUser(pub crate::models::User);

#[axum::async_trait]
impl FromRequestParts<SharedState> for AuthUser {
    type Rejection = (StatusCode, Json<Value>);

    async fn from_request_parts(
        parts: &mut Parts,
        state: &SharedState,
    ) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "));
        let token = match token {
            Some(t) => t,
            None => {
                return Err((
                    StatusCode::UNAUTHORIZED,
                    Json(json!({"error": "未ログインです。ログインしてください。"})),
                ))
            }
        };
        let mut mgr = state.lock().unwrap();
        match mgr.user_manager.resolve_token(token) {
            Some(user) => Ok(AuthUser(user)),
            None => Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "セッションの有効期限が切れました。再度ログインしてください。"})),
            )),
        }
    }
}

#[derive(Deserialize)]
pub struct AuthRequest {
    pub nickname: String,
}

#[derive(Deserialize)]
pub struct MigrateDecksRequest {
    pub decks: Vec<DeckDefinition>,
}

// ─── Handlers ─────────────────────────────────────────────────────────────────

pub async fn handle_register(
    State(state): State<SharedState>,
    Json(body): Json<AuthRequest>,
) -> impl IntoResponse {
    let mut mgr = state.lock().unwrap();
    match mgr.user_manager.register(&body.nickname) {
        Ok((user, session)) => Json(json!({
            "status": "ok",
            "user": {"id": user.id, "nickname": user.nickname},
            "token": session.token,
        }))
        .into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(json!({"error": e}))).into_response(),
    }
}

pub async fn handle_login(
    State(state): State<SharedState>,
    Json(body): Json<AuthRequest>,
) -> impl IntoResponse {
    let mut mgr = state.lock().unwrap();
    match mgr.user_manager.login(&body.nickname) {
        Ok((user, session)) => Json(json!({
            "status": "ok",
            "user": {"id": user.id, "nickname": user.nickname},
            "token": session.token,
        }))
        .into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(json!({"error": e}))).into_response(),
    }
}

pub async fn handle_me(user: AuthUser) -> impl IntoResponse {
    Json(json!({
        "status": "ok",
        "user": {"id": user.0.id, "nickname": user.0.nickname},
    }))
    .into_response()
}

pub async fn handle_migrate_decks(
    State(state): State<SharedState>,
    user: AuthUser,
    Json(body): Json<MigrateDecksRequest>,
) -> impl IntoResponse {
    let mut mgr = state.lock().unwrap();
    let (imported, skipped) = mgr.migrate_user_decks(&user.0.id, body.decks);
    Json(json!({"status": "ok", "imported": imported, "skipped": skipped})).into_response()
}

pub async fn handle_state(
    State(state): State<SharedState>,
    _user: AuthUser,
    Query(params): Query<StateQuery>,
) -> impl IntoResponse {
    let mut mgr = state.lock().unwrap();

    let room_id = params.room_id.unwrap_or_else(|| "default".to_string());
    if room_id.starts_with("solo_") && !mgr.rooms.contains_key(&room_id) {
        let solo_room = MatchRoom::new(
            &room_id,
            mgr.catalog.clone(),
            mgr.deck_def.clone(),
            true,
            mgr.deck_def.clone(),
        );
        mgr.rooms.insert(room_id.clone(), solo_room);
    }
    let room = match mgr.rooms.get_mut(&room_id) {
        Some(r) => r,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "Room not found", "roomId": room_id})),
            )
                .into_response();
        }
    };

    room.process_cpu_if_needed();

    let player_id = params
        .player_id
        .filter(|id| room.engine.players.contains_key(id.as_str()))
        .unwrap_or_else(|| "player1".to_string());

    let mut obs = build_player_observation(&room.engine, &player_id);
    obs["p2IsCpu"] = json!(room.p2_is_cpu);
    obs["cpuDifficulty"] = json!(room
        .cpu_agent
        .as_ref()
        .map(|a| a.difficulty)
        .unwrap_or_default());
    obs["roomId"] = json!(room.room_id);
    obs["p1Connected"] = json!(room.p1_connected);
    obs["p2Connected"] = json!(room.p2_connected);
    obs["waitingForPlayer2"] = json!(!room.p2_is_cpu && !room.p2_connected);

    Json(obs).into_response()
}

/// クローラー・デバッグ用: 指定ルームの完全ログ（DEBUG含む）およびローカルログファイルの末尾を取得
pub async fn handle_get_logs(
    State(state): State<SharedState>,
    Query(params): Query<LogsQuery>,
) -> impl IntoResponse {
    let mut mgr = state.lock().unwrap();
    let limit = params.limit.unwrap_or(200).min(2000);

    if let Some(ref room_id) = params.room_id {
        if let Some(room) = mgr.rooms.get_mut(room_id) {
            room.flush_logs();
            let total = room.engine.logs.len();
            let start = total.saturating_sub(limit);
            let slice: Vec<String> = room.engine.logs[start..].to_vec();
            return Json(json!({
                "roomId": room_id,
                "totalLines": total,
                "logs": slice,
            }))
            .into_response();
        }
    }

    // room_id未指定時は logs/all_rooms.log と logs/matches.jsonl の末尾を返す
    let file_logs: Vec<String> = std::fs::read_to_string("logs/all_rooms.log")
        .map(|s| {
            let all: Vec<&str> = s.lines().collect();
            let start = all.len().saturating_sub(limit);
            all[start..].iter().map(|l| (*l).to_string()).collect()
        })
        .unwrap_or_default();

    let match_records: Vec<Value> = std::fs::read_to_string("logs/matches.jsonl")
        .map(|s| {
            let all: Vec<&str> = s.lines().collect();
            let start = all.len().saturating_sub(50);
            all[start..]
                .iter()
                .filter_map(|l| serde_json::from_str::<Value>(l).ok())
                .collect()
        })
        .unwrap_or_default();

    Json(json!({
        "recentFileLogs": file_logs,
        "recentMatches": match_records,
    }))
    .into_response()
}

/// リプレイ一覧を取得（新しい順。各試合の要約）
pub async fn handle_list_replays(
    State(state): State<SharedState>,
    Query(params): Query<ReplaysQuery>,
) -> impl IntoResponse {
    let mgr = state.lock().unwrap();
    let limit = params.limit.unwrap_or(50).min(500);
    let replays = mgr.list_replays(limit);
    Json(json!({ "count": replays.len(), "replays": replays })).into_response()
}

/// 指定 match_id の全リプレイイベントを取得（再生入力として使用可能）
pub async fn handle_get_replay(
    State(state): State<SharedState>,
    AxumPath(match_id): AxumPath<String>,
) -> impl IntoResponse {
    let mgr = state.lock().unwrap();
    match mgr.get_replay(&match_id) {
        Some(events) => {
            Json(json!({ "matchId": match_id, "eventCount": events.len(), "events": events }))
                .into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "replay not found", "matchId": match_id })),
        )
            .into_response(),
    }
}

pub async fn handle_create_room(
    State(state): State<SharedState>,
    _user: AuthUser,
    Json(body): Json<CreateRoomRequest>,
) -> impl IntoResponse {
    let mut mgr = state.lock().unwrap();
    let room_id = mgr.create_room(body.is_cpu, body.deck);
    Json(json!({
        "status": "ok",
        "roomId": room_id,
        "assignedPlayerId": "player1"
    }))
    .into_response()
}

pub async fn handle_join_room(
    State(state): State<SharedState>,
    _user: AuthUser,
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
            )
                .into_response();
        }

        let p2_deck = sanitize_deck(body.deck, &default_deck);
        room.join_p2(&catalog, p2_deck);

        Json(json!({
            "status": "ok",
            "roomId": room_id,
            "assignedPlayerId": "player2"
        }))
        .into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": format!("ルーム番号「{}」が見つかりません", room_id)})),
        )
            .into_response()
    }
}

pub async fn handle_catalog(State(state): State<SharedState>) -> impl IntoResponse {
    let mgr = state.lock().unwrap();
    let total = mgr.catalog.definitions.len();
    let cats = mgr.catalog.raw_json.clone();

    let mut resp = cats.clone();
    resp["totalCards"] = json!(total);

    Json(resp).into_response()
}

pub async fn handle_deck(
    State(state): State<SharedState>,
    _user: AuthUser,
) -> impl IntoResponse {
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

pub async fn handle_list_decks(
    State(state): State<SharedState>,
    user: AuthUser,
) -> impl IntoResponse {
    let mgr = state.lock().unwrap();
    let list = mgr.list_user_decks(&user.0.id);
    Json(json!({ "decks": list })).into_response()
}

pub async fn handle_get_deck_by_id(
    State(state): State<SharedState>,
    user: AuthUser,
    AxumPath(deck_id): AxumPath<String>,
) -> impl IntoResponse {
    let mgr = state.lock().unwrap();
    if let Some(deck) = mgr.get_user_deck(&user.0.id, &deck_id) {
        return match serde_json::to_value(&deck) {
            Ok(val) => Json(val).into_response(),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
                .into_response(),
        };
    }
    (
        StatusCode::NOT_FOUND,
        Json(json!({"error": "Deck not found"})),
    )
        .into_response()
}

pub async fn handle_delete_deck_by_id(
    State(state): State<SharedState>,
    user: AuthUser,
    AxumPath(deck_id): AxumPath<String>,
) -> impl IntoResponse {
    let mut mgr = state.lock().unwrap();
    match mgr.delete_user_deck(&user.0.id, &deck_id) {
        Ok(()) => Json(StatusOk { status: "ok" }).into_response(),
        Err(e) if e.contains("プリセット") => {
            (StatusCode::FORBIDDEN, Json(json!({"error": e}))).into_response()
        }
        Err(e) if e.contains("見つかりません") => {
            (StatusCode::NOT_FOUND, Json(json!({"error": e}))).into_response()
        }
        Err(e) => (StatusCode::BAD_REQUEST, Json(json!({"error": e}))).into_response(),
    }
}

pub async fn handle_select_deck(
    State(state): State<SharedState>,
    user: AuthUser,
    AxumPath(deck_id): AxumPath<String>,
) -> impl IntoResponse {
    let mut mgr = state.lock().unwrap();
    let deck_def = match mgr.get_user_deck(&user.0.id, &deck_id) {
        Some(d) => d,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Failed to load deck"})),
            )
                .into_response();
        }
    };
    if let Ok(content) = serde_json::to_string_pretty(&deck_def) {
        let _ = std::fs::write(&mgr.deck_path, content);
    }
    mgr.deck_def = deck_def.clone();
    let catalog = mgr.catalog.clone();
    if let Some(default_room) = mgr.rooms.get_mut("default") {
        default_room.p1_deck = deck_def;
        default_room.reset_match(&catalog);
    }
    Json(StatusOk { status: "ok" }).into_response()
}

pub async fn handle_reset(
    State(state): State<SharedState>,
    user: AuthUser,
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
            .and_then(|id| mgr.get_user_deck(&user.0.id, id))
    });
    let p2_loaded = p2_deck_body.or_else(|| {
        p2_deck_id
            .as_deref()
            .filter(|s| !s.is_empty())
            .and_then(|id| mgr.get_user_deck(&user.0.id, id))
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
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "Room not found"})),
        )
            .into_response()
    }
}

pub async fn handle_set_cpu(
    State(state): State<SharedState>,
    _user: AuthUser,
    Json(body): Json<SetCpuRequest>,
) -> impl IntoResponse {
    let mut mgr = state.lock().unwrap();
    let room_id = body.room_id.unwrap_or_else(|| "default".to_string());
    if let Some(room) = mgr.rooms.get_mut(&room_id) {
        let prev_diff = room
            .cpu_agent
            .as_ref()
            .map(|a| a.difficulty)
            .unwrap_or_default();
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
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "Room not found"})),
        )
            .into_response()
    }
}

pub async fn handle_save_deck(
    State(state): State<SharedState>,
    user: AuthUser,
    Json(body): Json<DeckDefinition>,
) -> impl IntoResponse {
    let mgr_read = state.lock().unwrap();
    let total: i32 = body.main_deck.iter().map(|item| item.count).sum();
    if total != 40 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": format!("メインデッキはちょうど40枚である必要があります（現在: {}枚）", total)})),
        )
            .into_response();
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
            )
                .into_response();
        }
    }
    drop(mgr_read);

    if body.partner_id.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "パートナーカードが選択されていません"})),
        )
            .into_response();
    }
    if body.case_id.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "事件カードが選択されていません"})),
        )
            .into_response();
    }

    let mut mgr = state.lock().unwrap();
    let deck_clone = body.clone();
    if let Err(e) = mgr.save_user_deck(&user.0.id, &body) {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": e}))).into_response();
    }
    let catalog = mgr.catalog.clone();
    if let Some(default_room) = mgr.rooms.get_mut("default") {
        default_room.p1_deck = deck_clone;
        default_room.reset_match(&catalog);
    }
    Json(StatusOk { status: "ok" }).into_response()
}

pub async fn handle_decision(
    State(state): State<SharedState>,
    _user: AuthUser,
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
            Err(e) => {
                room.engine
                    .debug_log(format!("[ERROR][DECISION] player={} err={}", body.player_id, e));
                room.flush_logs();
                (StatusCode::BAD_REQUEST, Json(json!({"error": e}))).into_response()
            }
        }
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "Room not found"})),
        )
            .into_response()
    }
}
