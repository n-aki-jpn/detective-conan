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

fn build_engine(catalog_json: &str, state: &RoomStateRecord) -> std::result::Result<MatchEngine, String> {
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
    for (pid, dec) in &state.decisions {
        let _ = engine.submit_decision(pid, dec);
    }
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
    for step in 0..30 {
        let Some(pd) = engine.pending_decision.clone() else {
            break;
        };
        if pd.get("playerId").and_then(|v| v.as_str()) != Some("player2") {
            break;
        }
        let decision_id = pd.get("decisionId").and_then(|v| v.as_str()).unwrap_or("");
        let revision = pd.get("revision").and_then(|v| v.as_u64()).unwrap_or(0);
        let dec_type = pd.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let val = agent.choose_action_with_engine(engine, &pd);
        let act_type = val.get("type").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
        let dec = json!({
            "decisionId": decision_id,
            "revision": revision,
            "value": val
        });
        match engine.submit_decision("player2", &dec) {
            Ok(_) => {
                rec.decisions.push(("player2".to_string(), dec));
                changed = true;
            }
            Err(e) => {
                let fallback = json!({
                    "decisionId": decision_id,
                    "revision": revision,
                    "value": { "type": "END_TURN" }
                });
                engine.logs.push(format!("⚠️ CPU思考エラー(step={}, dec={}, act={}): {}", step, dec_type, act_type, e));
                if engine.submit_decision("player2", &fallback).is_ok() {
                    rec.decisions.push(("player2".to_string(), fallback));
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
        obj.insert("rewind".to_string(), json!({
            "available": false,
            "requestedBy": Value::Null,
            "canAccept": false
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

    let mut engine = match build_engine(catalog_json, &rec) {
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

    let mut engine = match build_engine(catalog_json, &rec) {
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
    };
    let mut engine = match build_engine(catalog_json, &rec) {
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

pub async fn rewind_stub(_req: Request) -> Result<Response> {
    Response::from_json(&json!({ "success": false, "error": "巻き戻しは現在準備中です" }))
}
