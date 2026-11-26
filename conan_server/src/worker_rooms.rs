use crate::catalog::CardCatalog;
use crate::engine::MatchEngine;
use crate::models::DeckDefinition;
use crate::observation::build_player_observation;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use worker::*;

const DEFAULT_DECK_JSON: &str = include_str!("../../data/dummy/deck.json");

#[derive(Debug, Serialize, Deserialize)]
struct RoomStateRecord {
    seed: u64,
    is_cpu: bool,
    p2_joined: bool,
    decisions: Vec<(String, Value)>,
    #[serde(default)]
    p1_deck_json: Option<String>,
    #[serde(default)]
    p2_deck_json: Option<String>,
    #[serde(default)]
    cached_obs_p1: Option<Value>,
    #[serde(default)]
    cached_obs_p2: Option<Value>,
    #[serde(default)]
    snapshot_decision_len: Option<usize>,
    #[serde(default)]
    snapshot_actor: Option<String>,
    #[serde(default)]
    others_decided_since_snapshot: usize,
    #[serde(default)]
    rewind_request: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct RoomRow {
    id: String,
    state_json: String,
    p2_joined: i64,
    is_cpu: i64,
}

fn json_err(msg: &str, status: u16) -> Result<Response> {
    let mut res = Response::from_json(&json!({ "error": msg }))?;
    res = res.with_status(status);
    Ok(res)
}

#[derive(Default)]
struct RewindTrack {
    snapshot_decision_len: Option<usize>,
    snapshot_actor: Option<String>,
    others_decided_since_snapshot: usize,
}

fn track_before_decision(engine: &MatchEngine, track: &mut RewindTrack, decision_idx: usize) {
    let (kind, actor) = engine
        .pending_decision
        .as_ref()
        .map(|d| {
            (
                d.get("kind").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                d.get("playerId").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            )
        })
        .unwrap_or_default();

    if kind == "mainAction" || kind == "hintPlay" {
        track.snapshot_decision_len = Some(decision_idx);
        track.snapshot_actor = Some(actor);
        track.others_decided_since_snapshot = 0;
    } else if track.snapshot_actor.as_deref() == Some(actor.as_str()) {
        // 同プレイヤーの応答系（対象選択等）は隠情報を開示しないためカウントしない
    } else if track.snapshot_actor.is_some() {
        track.others_decided_since_snapshot += 1;
    }
}

fn apply_track_to_record(rec: &mut RoomStateRecord, track: RewindTrack) {
    rec.snapshot_decision_len = track.snapshot_decision_len;
    rec.snapshot_actor = track.snapshot_actor;
    rec.others_decided_since_snapshot = track.others_decided_since_snapshot;
}

fn reset_rewind_fields(rec: &mut RoomStateRecord) {
    rec.snapshot_decision_len = None;
    rec.snapshot_actor = None;
    rec.others_decided_since_snapshot = 0;
    rec.rewind_request = None;
}

fn can_rewind_by(engine: &MatchEngine, rec: &RoomStateRecord, requester: &str) -> bool {
    rec.snapshot_decision_len.is_some()
        && engine.winner_id.is_none()
        && rec.snapshot_actor.as_deref() == Some(requester)
        && rec.others_decided_since_snapshot == 0
}

fn build_engine(catalog_json: &str, state: &mut RoomStateRecord) -> std::result::Result<MatchEngine, String> {
    let catalog = CardCatalog::load_from_str(catalog_json).map_err(|e| e.to_string())?;
    let default_deck: DeckDefinition = serde_json::from_str(DEFAULT_DECK_JSON).map_err(|e| e.to_string())?;
    let p1_deck: DeckDefinition = state
        .p1_deck_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_else(|| default_deck.clone());
    let p2_deck: DeckDefinition = state
        .p2_deck_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or(default_deck);
    let mut engine = MatchEngine::new_with_seed(catalog, state.seed);
    engine.add_player("player1", &p1_deck);
    engine.add_player("player2", &p2_deck);
    engine.start_match();
    let mut track = RewindTrack::default();
    for (idx, (pid, dec)) in state.decisions.iter().enumerate() {
        track_before_decision(&engine, &mut track, idx);
        let _ = engine.submit_decision(pid, dec);
    }
    apply_track_to_record(state, track);
    Ok(engine)
}

async fn load_or_create_room(room_id: &str, env: &Env) -> Result<RoomStateRecord> {
    let db = env.d1("DB")?;
    let row = db
        .prepare("SELECT id, state_json, p2_joined, is_cpu FROM rooms WHERE id = ?1 LIMIT 1")
        .bind(&[room_id.into()])?
        .first::<RoomRow>(None)
        .await?;

    if let Some(r) = row {
        if let Ok(parsed) = serde_json::from_str::<RoomStateRecord>(&r.state_json) {
            return Ok(parsed);
        }
    }

    let seed: u64 = rand::random();
    let mut rec = RoomStateRecord {
        seed,
        is_cpu: true,
        p2_joined: false,
        decisions: Vec::new(),
        p1_deck_json: None,
        p2_deck_json: None,
        cached_obs_p1: None,
        cached_obs_p2: None,
        snapshot_decision_len: None,
        snapshot_actor: None,
        others_decided_since_snapshot: 0,
        rewind_request: None,
    };
    save_room(room_id, &rec, env).await?;
    Ok(rec)
}

async fn save_room(room_id: &str, rec: &RoomStateRecord, env: &Env) -> Result<()> {
    let db = env.d1("DB")?;
    let state_json = serde_json::to_string(rec).unwrap_or_else(|_| "{}".to_string());
    db.prepare(
        "INSERT OR REPLACE INTO rooms (id, state_json, p2_joined, is_cpu, updated_at)
         VALUES (?1, ?2, ?3, ?4, datetime('now'))",
    )
    .bind(&[
        room_id.into(),
        state_json.as_str().into(),
        (if rec.p2_joined { 1 } else { 0 }).into(),
        (if rec.is_cpu { 1 } else { 0 }).into(),
    ])?
    .run()
    .await?;
    Ok(())
}

fn run_cpu_turns(engine: &mut MatchEngine, rec: &mut RoomStateRecord) -> bool {
    if !rec.is_cpu {
        return false;
    }
    let agent = crate::agent::RandomAgent::with_difficulty(
        "player2",
        crate::agent::CpuDifficulty::Normal,
    );
    let mut changed = false;
    let mut last_decision_id = String::new();
    let mut same_count = 0usize;

    for _step in 0..30 {
        let Some(pd) = engine.pending_decision.clone() else {
            break;
        };
        if pd.get("playerId").and_then(|v| v.as_str()) != Some("player2") {
            break;
        }
        let decision_id = pd.get("decisionId").and_then(|v| v.as_str()).unwrap_or("");
        let revision = pd.get("revision").and_then(|v| v.as_u64()).unwrap_or(0);

        let mut track = RewindTrack {
            snapshot_decision_len: rec.snapshot_decision_len,
            snapshot_actor: rec.snapshot_actor.clone(),
            others_decided_since_snapshot: rec.others_decided_since_snapshot,
        };
        if decision_id == last_decision_id {
            same_count += 1;
            if same_count >= 2 {
                // 同じdecisionIdで停滞した場合は即座にEND_TURNを試みて抜ける
                let fallback = json!({
                    "decisionId": decision_id,
                    "revision": revision,
                    "value": { "type": "END_TURN" }
                });
                track_before_decision(engine, &mut track, rec.decisions.len());
                if engine.submit_decision("player2", &fallback).is_ok() {
                    rec.decisions.push(("player2".to_string(), fallback));
                    apply_track_to_record(rec, track);
                    changed = true;
                }
                break;
            }
        } else {
            last_decision_id = decision_id.to_string();
            same_count = 0;
        }

        let ans = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            agent.choose_action_with_engine(engine, &pd)
        })) {
            Ok(v) => v,
            Err(_) => {
                engine.logs.push("⚠️ CPU思考パニック捕捉: END_TURNへフォールバック".to_string());
                json!({
                    "decisionId": decision_id,
                    "revision": revision,
                    "value": { "type": "END_TURN" }
                })
            }
        };
        let mut step_track = RewindTrack {
            snapshot_decision_len: rec.snapshot_decision_len,
            snapshot_actor: rec.snapshot_actor.clone(),
            others_decided_since_snapshot: rec.others_decided_since_snapshot,
        };
        track_before_decision(engine, &mut step_track, rec.decisions.len());
        match engine.submit_decision("player2", &ans) {
            Ok(_) => {
                rec.decisions.push(("player2".to_string(), ans));
                apply_track_to_record(rec, step_track);
                changed = true;
            }
            Err(e) => {
                let fallback = json!({
                    "decisionId": decision_id,
                    "revision": revision,
                    "value": { "type": "END_TURN" }
                });
                engine.logs.push(format!("⚠️ CPU思考エラー: {}", e));
                let mut fb_track = RewindTrack {
                    snapshot_decision_len: rec.snapshot_decision_len,
                    snapshot_actor: rec.snapshot_actor.clone(),
                    others_decided_since_snapshot: rec.others_decided_since_snapshot,
                };
                track_before_decision(engine, &mut fb_track, rec.decisions.len());
                if engine.submit_decision("player2", &fallback).is_ok() {
                    rec.decisions.push(("player2".to_string(), fallback));
                    apply_track_to_record(rec, fb_track);
                    changed = true;
                } else {
                    break;
                }
            }
        }
    }
    changed
}

fn build_obs_for(engine: &MatchEngine, player_id: &str, room_id: &str, rec: &RoomStateRecord) -> Value {
    let mut obs = build_player_observation(engine, player_id);
    if let Some(obj) = obs.as_object_mut() {
        obj.insert("p2IsCpu".to_string(), json!(rec.is_cpu));
        obj.insert("cpuDifficulty".to_string(), json!(0));
        obj.insert("roomId".to_string(), json!(room_id));
        obj.insert("p1Connected".to_string(), json!(true));
        obj.insert("p2Connected".to_string(), json!(rec.p2_joined));
        obj.insert("waitingForPlayer2".to_string(), json!(!rec.is_cpu && !rec.p2_joined));
        let can_rewind = can_rewind_by(engine, rec, player_id);
        let requested_by = rec.rewind_request.clone();
        let can_accept = requested_by
            .as_deref()
            .map(|req| req != player_id && can_rewind_by(engine, rec, req))
            .unwrap_or(false);
        obj.insert("rewind".to_string(), json!({
            "available": can_rewind,
            "requestedBy": requested_by,
            "canAccept": can_accept
        }));
    }
    obs
}

fn update_cached_obs(engine: &MatchEngine, room_id: &str, rec: &mut RoomStateRecord) {
    rec.cached_obs_p1 = Some(build_obs_for(engine, "player1", room_id, rec));
    rec.cached_obs_p2 = Some(build_obs_for(engine, "player2", room_id, rec));
}

fn state_response_from_rec(player_id: &str, rec: &RoomStateRecord) -> Result<Response> {
    let obs = if player_id == "player2" {
        rec.cached_obs_p2.as_ref().or(rec.cached_obs_p1.as_ref())
    } else {
        rec.cached_obs_p1.as_ref()
    };
    match obs {
        Some(v) => Response::from_json(v),
        None => json_err("盤面キャッシュが見つかりません", 500),
    }
}

pub async fn get_state(req: Request, env: Env, catalog_json: &str) -> Result<Response> {
    let url = req.url()?;
    let mut player_id = "player1".to_string();
    let mut room_id = "default".to_string();
    for (k, v) in url.query_pairs() {
        match k.as_ref() {
            "player_id" if !v.trim().is_empty() => {
                player_id = match v.as_ref() {
                    "p1" => "player1".to_string(),
                    "p2" => "player2".to_string(),
                    other => other.to_string(),
                };
            }
            "room_id" if !v.trim().is_empty() => room_id = v.into_owned(),
            _ => {}
        }
    }

    let mut rec = load_or_create_room(&room_id, &env).await?;
    if rec.cached_obs_p1.is_some() {
        return state_response_from_rec(&player_id, &rec);
    }

    let mut engine = match build_engine(catalog_json, &mut rec) {
        Ok(e) => e,
        Err(e) => return json_err(&format!("エンジン初期化エラー: {}", e), 500),
    };
    run_cpu_turns(&mut engine, &mut rec);
    update_cached_obs(&engine, &room_id, &mut rec);
    save_room(&room_id, &rec, &env).await?;
    state_response_from_rec(&player_id, &rec)
}

pub async fn submit_decision(mut req: Request, env: Env, catalog_json: &str) -> Result<Response> {
    let body: Value = match req.json().await {
        Ok(v) => v,
        Err(_) => return json_err("リクエストJSONが不正です", 400),
    };
    let room_id = body.get("roomId").or_else(|| body.get("room_id")).and_then(|v| v.as_str()).unwrap_or("default").to_string();
    let player_id = body
        .get("playerId")
        .or_else(|| body.get("player_id"))
        .and_then(|v| v.as_str())
        .map(|s| match s {
            "p1" => "player1",
            "p2" => "player2",
            other => other,
        })
        .unwrap_or("player1")
        .to_string();

    let mut rec = load_or_create_room(&room_id, &env).await?;
    rec.decisions.push((player_id.clone(), body));
    rec.rewind_request = None;

    let mut engine = match build_engine(catalog_json, &mut rec) {
        Ok(e) => e,
        Err(e) => return json_err(&format!("エンジンエラー: {}", e), 500),
    };
    run_cpu_turns(&mut engine, &mut rec);
    update_cached_obs(&engine, &room_id, &mut rec);
    save_room(&room_id, &rec, &env).await?;
    state_response_from_rec(&player_id, &rec)
}

#[derive(Debug, Deserialize)]
struct DeckJsonRow {
    deck_json: String,
}

async fn resolve_deck_json(deck_val: Option<&Value>, deck_id: Option<&str>, env: &Env) -> Option<String> {
    if let Some(v) = deck_val {
        if v.is_object() {
            if let Ok(s) = serde_json::to_string(v) {
                return Some(s);
            }
        }
    }
    if let Some(id) = deck_id.filter(|s| !s.trim().is_empty()) {
        if let Some(raw) = crate::worker_auth::find_preset_deck_json(id) {
            return Some(raw.to_string());
        }
        if let Ok(db) = env.d1("DB") {
            if let Ok(stmt) = db.prepare("SELECT deck_json FROM decks WHERE id = ?1 LIMIT 1").bind(&[id.into()]) {
                if let Ok(Some(row)) = stmt.first::<DeckJsonRow>(None).await {
                    return Some(row.deck_json);
                }
            }
        }
    }
    None
}

pub async fn reset_room(mut req: Request, env: Env, catalog_json: &str) -> Result<Response> {
    let body: Value = req.json().await.unwrap_or_else(|_| json!({}));
    let room_id = body.get("roomId").or_else(|| body.get("room_id")).and_then(|v| v.as_str()).unwrap_or("default").to_string();
    let player_id = body
        .get("playerId")
        .or_else(|| body.get("player_id"))
        .and_then(|v| v.as_str())
        .map(|s| match s {
            "p1" => "player1",
            "p2" => "player2",
            other => other,
        })
        .unwrap_or("player1")
        .to_string();

    let prev = load_or_create_room(&room_id, &env).await.ok();
    let p1_deck_json = resolve_deck_json(
        body.get("p1Deck"),
        body.get("p1DeckId").and_then(|v| v.as_str()),
        &env,
    )
    .await
    .or_else(|| prev.as_ref().and_then(|p| p.p1_deck_json.clone()));
    let p2_deck_json = resolve_deck_json(
        body.get("p2Deck"),
        body.get("p2DeckId").and_then(|v| v.as_str()),
        &env,
    )
    .await
    .or_else(|| prev.as_ref().and_then(|p| p.p2_deck_json.clone()));

    let mut rec = RoomStateRecord {
        seed: rand::random(),
        is_cpu: prev.as_ref().map(|p| p.is_cpu).unwrap_or(true),
        p2_joined: prev.as_ref().map(|p| p.p2_joined).unwrap_or(false),
        decisions: Vec::new(),
        p1_deck_json,
        p2_deck_json,
        cached_obs_p1: None,
        cached_obs_p2: None,
        snapshot_decision_len: None,
        snapshot_actor: None,
        others_decided_since_snapshot: 0,
        rewind_request: None,
    };
    let mut engine = match build_engine(catalog_json, &mut rec) {
        Ok(e) => e,
        Err(e) => return json_err(&format!("リセットエラー: {}", e), 500),
    };
    run_cpu_turns(&mut engine, &mut rec);
    update_cached_obs(&engine, &room_id, &mut rec);
    save_room(&room_id, &rec, &env).await?;
    state_response_from_rec(&player_id, &rec)
}

pub async fn set_cpu(mut req: Request, env: Env) -> Result<Response> {
    let body: Value = req.json().await.unwrap_or_else(|_| json!({}));
    let room_id = body.get("roomId").or_else(|| body.get("room_id")).and_then(|v| v.as_str()).unwrap_or("default").to_string();
    let is_cpu = body.get("isCpu").or_else(|| body.get("is_cpu")).and_then(|v| v.as_bool()).unwrap_or(true);

    let mut rec = load_or_create_room(&room_id, &env).await?;
    rec.is_cpu = is_cpu;
    save_room(&room_id, &rec, &env).await?;
    Response::from_json(&json!({ "success": true, "isCpu": is_cpu }))
}

pub async fn create_room(_req: Request, env: Env) -> Result<Response> {
    let code: u32 = (rand::random::<u32>() % 9000) + 1000;
    let room_id = code.to_string();
    let rec = RoomStateRecord {
        seed: rand::random(),
        is_cpu: false,
        p2_joined: false,
        decisions: Vec::new(),
        p1_deck_json: None,
        p2_deck_json: None,
        cached_obs_p1: None,
        cached_obs_p2: None,
        snapshot_decision_len: None,
        snapshot_actor: None,
        others_decided_since_snapshot: 0,
        rewind_request: None,
    };
    save_room(&room_id, &rec, &env).await?;
    Response::from_json(&json!({
        "success": true,
        "roomId": room_id,
        "playerId": "p1"
    }))
}

pub async fn join_room(mut req: Request, env: Env) -> Result<Response> {
    let body: Value = req.json().await.unwrap_or_else(|_| json!({}));
    let room_id = body.get("roomId").or_else(|| body.get("room_id")).and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    if room_id.is_empty() {
        return json_err("ルームコードを入力してください", 400);
    }
    let mut rec = load_or_create_room(&room_id, &env).await?;
    rec.p2_joined = true;
    rec.is_cpu = false;
    save_room(&room_id, &rec, &env).await?;
    Response::from_json(&json!({
        "success": true,
        "roomId": room_id,
        "playerId": "p2"
    }))
}

fn parse_rewind_req(body: &Value) -> (String, String) {
    let room_id = body
        .get("roomId")
        .or_else(|| body.get("room_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("default")
        .to_string();
    let player_id = body
        .get("playerId")
        .or_else(|| body.get("player_id"))
        .and_then(|v| v.as_str())
        .map(|s| match s {
            "p1" => "player1",
            "p2" => "player2",
            other => other,
        })
        .unwrap_or("player1")
        .to_string();
    (room_id, player_id)
}

fn apply_rewind_on_record(
    rec: &mut RoomStateRecord,
    room_id: &str,
    catalog_json: &str,
) -> std::result::Result<MatchEngine, String> {
    let snap_len = rec
        .snapshot_decision_len
        .ok_or_else(|| "スナップショットがありません".to_string())?;
    rec.decisions.truncate(snap_len);
    reset_rewind_fields(rec);
    let mut engine = build_engine(catalog_json, rec)?;
    engine.log("↩️ 1手巻き戻しました".to_string());
    update_cached_obs(&engine, room_id, rec);
    Ok(engine)
}

pub async fn request_rewind(mut req: Request, env: Env, catalog_json: &str) -> Result<Response> {
    let body: Value = req.json().await.unwrap_or_else(|_| json!({}));
    let (room_id, player_id) = parse_rewind_req(&body);

    let mut rec = load_or_create_room(&room_id, &env).await?;
    let engine = match build_engine(catalog_json, &mut rec) {
        Ok(e) => e,
        Err(e) => return json_err(&format!("エンジンエラー: {}", e), 500),
    };

    if !can_rewind_by(&engine, &rec, &player_id) {
        return json_err(
            "現在は巻き戻しできません（既に相手の応答が出ているか、自分の手ではありません）",
            400,
        );
    }

    rec.rewind_request = Some(player_id.clone());
    let auto_applied = if rec.is_cpu && player_id == "player1" {
        if let Err(e) = apply_rewind_on_record(&mut rec, &room_id, catalog_json) {
            return json_err(&e, 400);
        }
        true
    } else {
        update_cached_obs(&engine, &room_id, &mut rec);
        false
    };

    save_room(&room_id, &rec, &env).await?;
    Response::from_json(&json!({
        "status": "ok",
        "autoApplied": auto_applied
    }))
}

pub async fn accept_rewind(mut req: Request, env: Env, catalog_json: &str) -> Result<Response> {
    let body: Value = req.json().await.unwrap_or_else(|_| json!({}));
    let (room_id, acceptor) = parse_rewind_req(&body);

    let mut rec = load_or_create_room(&room_id, &env).await?;
    let Some(requester) = rec.rewind_request.clone() else {
        return json_err("巻き戻し要求はありません", 400);
    };
    if acceptor == requester {
        return json_err("相手側の同意が必要です", 400);
    }

    let engine = match build_engine(catalog_json, &mut rec) {
        Ok(e) => e,
        Err(e) => return json_err(&format!("エンジンエラー: {}", e), 500),
    };

    if !can_rewind_by(&engine, &rec, &requester) {
        rec.rewind_request = None;
        update_cached_obs(&engine, &room_id, &mut rec);
        save_room(&room_id, &rec, &env).await?;
        return json_err("巻き戻し条件が満たされなくなりました", 400);
    }

    if let Err(e) = apply_rewind_on_record(&mut rec, &room_id, catalog_json) {
        return json_err(&e, 400);
    }

    save_room(&room_id, &rec, &env).await?;
    Response::from_json(&json!({ "status": "ok" }))
}

pub async fn reject_rewind(mut req: Request, env: Env, catalog_json: &str) -> Result<Response> {
    let body: Value = req.json().await.unwrap_or_else(|_| json!({}));
    let (room_id, _rejector) = parse_rewind_req(&body);

    let mut rec = load_or_create_room(&room_id, &env).await?;
    rec.rewind_request = None;
    let engine = match build_engine(catalog_json, &mut rec) {
        Ok(e) => e,
        Err(e) => return json_err(&format!("エンジンエラー: {}", e), 500),
    };
    update_cached_obs(&engine, &room_id, &mut rec);
    save_room(&room_id, &rec, &env).await?;
    Response::from_json(&json!({ "status": "ok" }))
}
