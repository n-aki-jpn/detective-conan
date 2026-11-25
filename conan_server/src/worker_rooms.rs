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
    let deck: DeckDefinition = serde_json::from_str(DEFAULT_DECK_JSON).map_err(|e| e.to_string())?;
    let mut engine = MatchEngine::new(catalog);
    engine.add_player("player1", &deck);
    engine.add_player("player2", &deck);
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
    let rec = RoomStateRecord {
        seed,
        is_cpu: true,
        p2_joined: false,
        decisions: Vec::new(),
    };
    save_room(room_id, &rec, env).await?;
    Ok(rec)
}

async fn save_room(room_id: &str, rec: &RoomStateRecord, env: &Env) -> Result<()> {
    let db = env.d1("DB")?;
    let state_json = serde_json::to_string(rec).unwrap_or_else(|_| "{}".to_string());
    db.prepare(
        "INSERT INTO rooms (id, state_json, p2_joined, is_cpu, updated_at)
         VALUES (?1, ?2, ?3, ?4, datetime('now'))
         ON CONFLICT(id) DO UPDATE SET
           state_json = excluded.state_json,
           p2_joined = excluded.p2_joined,
           is_cpu = excluded.is_cpu,
           updated_at = datetime('now')",
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

fn build_cpu_decision(pd: &Value) -> Value {
    let decision_id = pd.get("decisionId").and_then(|v| v.as_str()).unwrap_or("");
    let revision = pd.get("revision").and_then(|v| v.as_u64()).unwrap_or(0);
    let decision_type = pd.get("type").and_then(|v| v.as_str()).unwrap_or("");

    let mut val = json!({});
    if let Some(opts) = pd.get("options").and_then(|v| v.as_array()) {
        if let Some(first) = opts.first() {
            val = first.clone();
        }
    } else if decision_type == "MULLIGAN" {
        val = json!({ "discardIds": [] });
    }
    json!({
        "decisionId": decision_id,
        "revision": revision,
        "value": val
    })
}

fn run_cpu_turns(engine: &mut MatchEngine, rec: &mut RoomStateRecord) -> bool {
    if !rec.is_cpu {
        return false;
    }
    let mut changed = false;
    for _ in 0..30 {
        let Some(pd) = engine.pending_decision.clone() else {
            break;
        };
        if pd.get("playerId").and_then(|v| v.as_str()) != Some("player2") {
            break;
        }
        let dec = build_cpu_decision(&pd);
        if engine.submit_decision("player2", &dec).is_ok() {
            rec.decisions.push(("player2".to_string(), dec));
            changed = true;
        } else {
            break;
        }
    }
    changed
}

fn state_response(engine: &MatchEngine, player_id: &str, room_id: &str, rec: &RoomStateRecord) -> Result<Response> {
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
    Response::from_json(&obs)
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
    let mut engine = match build_engine(catalog_json, &rec) {
        Ok(e) => e,
        Err(e) => return json_err(&format!("エンジン初期化エラー: {}", e), 500),
    };
    if run_cpu_turns(&mut engine, &mut rec) {
        save_room(&room_id, &rec, &env).await?;
    }
    state_response(&engine, &player_id, &room_id, &rec)
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
    save_room(&room_id, &rec, &env).await?;
    state_response(&engine, &player_id, &room_id, &rec)
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

    let rec = RoomStateRecord {
        seed: rand::random(),
        is_cpu: true,
        p2_joined: false,
        decisions: Vec::new(),
    };
    save_room(&room_id, &rec, &env).await?;
    let engine = match build_engine(catalog_json, &rec) {
        Ok(e) => e,
        Err(e) => return json_err(&format!("リセットエラー: {}", e), 500),
    };
    state_response(&engine, &player_id, &room_id, &rec)
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
