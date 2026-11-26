use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use worker::*;

const PRESET_DECKS: &[(&str, &str)] = &[
    ("preset_black", include_str!("../../data/dummy/decks/preset_black.json")),
    ("preset_blue_white", include_str!("../../data/dummy/decks/preset_blue_white.json")),
    ("preset_green", include_str!("../../data/dummy/decks/preset_green.json")),
    ("preset_green_red", include_str!("../../data/dummy/decks/preset_green_red.json")),
    ("preset_red", include_str!("../../data/dummy/decks/preset_red.json")),
    ("preset_yellow", include_str!("../../data/dummy/decks/preset_yellow.json")),
];

pub fn find_preset_deck_json(id: &str) -> Option<&'static str> {
    let norm = id.strip_suffix(".json").unwrap_or(id);
    PRESET_DECKS.iter().find(|(k, _)| *k == norm).map(|(_, v)| *v)
}

#[derive(Deserialize)]
struct AuthReq {
    nickname: Option<String>,
    username: Option<String>,
    password: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct UserRow {
    id: String,
    username: String,
    password_hash: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct SessionRow {
    token: String,
    user_id: String,
    username: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct DeckRow {
    id: String,
    user_id: String,
    deck_name: String,
    deck_json: String,
    is_public: i64,
    is_active: i64,
    updated_at: String,
}

fn json_err(msg: &str, status: u16) -> Result<Response> {
    let mut res = Response::from_json(&json!({ "error": msg }))?;
    res = res.with_status(status);
    Ok(res)
}

fn simple_hash(password: &str) -> String {
    // 簡易ソルト付きハッシュ（FNV-1a 64bit x2）
    let mut h1: u64 = 0xcbf29ce484222325;
    let mut h2: u64 = 0x84222325cbf29ce4;
    for b in password.as_bytes() {
        h1 ^= *b as u64;
        h1 = h1.wrapping_mul(0x100000001b3);
        h2 ^= (*b as u64).wrapping_add(0x9e);
        h2 = h2.wrapping_mul(0x100000001b3);
    }
    format!("{:016x}{:016x}", h1, h2)
}

fn gen_token() -> String {
    let r1: u64 = rand::random();
    let r2: u64 = rand::random();
    format!("{:016x}{:016x}", r1, r2)
}

async fn resolve_user_id(req: &Request, env: &Env) -> Option<(String, String)> {
    let auth = req.headers().get("Authorization").ok().flatten()?;
    let token = auth.strip_prefix("Bearer ")?.trim();
    if token.is_empty() {
        return None;
    }
    let db = env.d1("DB").ok()?;
    let stmt = db.prepare("SELECT token, user_id, username FROM sessions WHERE token = ?1 LIMIT 1");
    let res = stmt.bind(&[token.into()]).ok()?.first::<SessionRow>(None).await.ok()??;
    Some((res.user_id, res.username))
}

pub async fn register(mut req: Request, env: Env) -> Result<Response> {
    let body: AuthReq = match req.json().await {
        Ok(b) => b,
        Err(_) => return json_err("リクエスト形式が不正です", 400),
    };
    let raw_name = body.nickname.or(body.username).unwrap_or_default();
    let username = raw_name.trim();
    if username.len() < 2 || username.len() > 16 {
        return json_err("ニックネームは2〜16文字で入力してください", 400);
    }

    let db = env.d1("DB")?;
    let check = db
        .prepare("SELECT id, username, password_hash FROM users WHERE username = ?1 LIMIT 1")
        .bind(&[username.into()])?
        .first::<UserRow>(None)
        .await?;
    if check.is_some() {
        return json_err("このニックネームは既に使用されています", 409);
    }

    let user_id = format!("u_{}", gen_token());
    let pw_hash = simple_hash(body.password.as_deref().unwrap_or(""));
    db.prepare("INSERT INTO users (id, username, password_hash) VALUES (?1, ?2, ?3)")
        .bind(&[user_id.as_str().into(), username.into(), pw_hash.as_str().into()])?
        .run()
        .await?;

    let token = gen_token();
    db.prepare("INSERT INTO sessions (token, user_id, username) VALUES (?1, ?2, ?3)")
        .bind(&[token.as_str().into(), user_id.as_str().into(), username.into()])?
        .run()
        .await?;

    Response::from_json(&json!({
        "token": token,
        "user": { "id": user_id, "nickname": username, "username": username }
    }))
}

pub async fn login(mut req: Request, env: Env) -> Result<Response> {
    let body: AuthReq = match req.json().await {
        Ok(b) => b,
        Err(_) => return json_err("リクエスト形式が不正です", 400),
    };
    let raw_name = body.nickname.or(body.username).unwrap_or_default();
    let username = raw_name.trim();
    let db = env.d1("DB")?;
    let user = db
        .prepare("SELECT id, username, password_hash FROM users WHERE username = ?1 LIMIT 1")
        .bind(&[username.into()])?
        .first::<UserRow>(None)
        .await?;

    let Some(u) = user else {
        return json_err("ニックネームが見つかりません", 401);
    };
    if let Some(pw) = body.password.as_deref() {
        if u.password_hash != simple_hash(pw) {
            return json_err("パスワードが正しくありません", 401);
        }
    }

    let token = gen_token();
    db.prepare("INSERT INTO sessions (token, user_id, username) VALUES (?1, ?2, ?3)")
        .bind(&[token.as_str().into(), u.id.as_str().into(), u.username.as_str().into()])?
        .run()
        .await?;

    Response::from_json(&json!({
        "token": token,
        "user": { "id": u.id, "nickname": u.username, "username": u.username }
    }))
}

pub async fn me(req: Request, env: Env) -> Result<Response> {
    match resolve_user_id(&req, &env).await {
        Some((user_id, username)) => Response::from_json(&json!({
            "user": { "id": user_id, "nickname": username, "username": username }
        })),
        None => json_err("未ログインです", 401),
    }
}

pub async fn get_active_deck(req: Request, env: Env) -> Result<Response> {
    let uid = resolve_user_id(&req, &env)
        .await
        .map(|(id, _)| id)
        .unwrap_or_else(|| "guest".to_string());
    let db = env.d1("DB")?;
    let row = db
        .prepare("SELECT id, user_id, deck_name, deck_json, is_public, is_active, updated_at FROM decks WHERE user_id = ?1 AND is_active = 1 ORDER BY updated_at DESC LIMIT 1")
        .bind(&[uid.as_str().into()])?
        .first::<DeckRow>(None)
        .await?;

    if let Some(d) = row {
        let val: Value = serde_json::from_str(&d.deck_json).unwrap_or_else(|_| json!({}));
        Response::from_json(&val)
    } else {
        Response::from_json(&json!({
            "deckId": "default",
            "deckName": "初期デッキ",
            "cards": []
        }))
    }
}

pub async fn list_decks(req: Request, env: Env) -> Result<Response> {
    let uid = resolve_user_id(&req, &env)
        .await
        .map(|(id, _)| id)
        .unwrap_or_else(|| "guest".to_string());
    let db = env.d1("DB")?;
    let res = db
        .prepare("SELECT id, user_id, deck_name, deck_json, is_public, is_active, updated_at FROM decks WHERE user_id = ?1 OR is_public = 1 ORDER BY updated_at DESC LIMIT 100")
        .bind(&[uid.as_str().into()])?
        .all()
        .await?;

    let rows: Vec<DeckRow> = res.results()?;
    let mut decks: Vec<Value> = rows
        .into_iter()
        .map(|d| {
            json!({
                "deckId": d.id,
                "fileId": d.id,
                "deckName": d.deck_name,
                "isPublic": d.is_public == 1,
                "isActive": d.is_active == 1,
                "isOwner": d.user_id == uid,
                "isPreset": false,
                "updatedAt": d.updated_at
            })
        })
        .collect();

    for (pid, raw) in PRESET_DECKS {
        let name = serde_json::from_str::<Value>(raw)
            .ok()
            .and_then(|v| v.get("deckName").and_then(|n| n.as_str()).map(|s| s.to_string()))
            .unwrap_or_else(|| pid.to_string());
        decks.push(json!({
            "deckId": pid,
            "fileId": pid,
            "deckName": name,
            "isPublic": true,
            "isActive": false,
            "isOwner": false,
            "isPreset": true,
            "updatedAt": "preset"
        }));
    }

    Response::from_json(&json!({ "decks": decks }))
}

pub async fn get_deck_by_id(id: &str, env: Env) -> Result<Response> {
    if let Some(raw) = find_preset_deck_json(id) {
        let val: Value = serde_json::from_str(raw).unwrap_or_else(|_| json!({}));
        return Response::from_json(&val);
    }
    let db = env.d1("DB")?;
    let row = db
        .prepare("SELECT id, user_id, deck_name, deck_json, is_public, is_active, updated_at FROM decks WHERE id = ?1 LIMIT 1")
        .bind(&[id.into()])?
        .first::<DeckRow>(None)
        .await?;

    let Some(d) = row else {
        return json_err("デッキが見つかりません", 404);
    };
    let val: Value = serde_json::from_str(&d.deck_json).unwrap_or_else(|_| json!({}));
    Response::from_json(&val)
}

pub async fn save_deck(mut req: Request, env: Env) -> Result<Response> {
    let uid = resolve_user_id(&req, &env)
        .await
        .map(|(id, _)| id)
        .unwrap_or_else(|| "guest".to_string());
    let mut val: Value = match req.json().await {
        Ok(v) => v,
        Err(_) => return json_err("デッキJSONが不正です", 400),
    };

    let deck_id = val
        .get("deckId")
        .or_else(|| val.get("fileId"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty() && !s.starts_with("preset_"))
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("deck_{}", gen_token()));

    let deck_name = val
        .get("deckName")
        .and_then(|v| v.as_str())
        .unwrap_or("名称未設定デッキ")
        .to_string();

    if let Some(obj) = val.as_object_mut() {
        obj.insert("deckId".to_string(), json!(deck_id));
    }
    let deck_json = serde_json::to_string(&val).unwrap_or_else(|_| "{}".to_string());

    let db = env.d1("DB")?;
    db.prepare("UPDATE decks SET is_active = 0 WHERE user_id = ?1")
        .bind(&[uid.as_str().into()])?
        .run()
        .await?;

    db.prepare(
        "INSERT INTO decks (id, user_id, deck_name, deck_json, is_active, updated_at)
         VALUES (?1, ?2, ?3, ?4, 1, datetime('now'))
         ON CONFLICT(id) DO UPDATE SET
           deck_name = excluded.deck_name,
           deck_json = excluded.deck_json,
           is_active = 1,
           updated_at = datetime('now')",
    )
    .bind(&[
        deck_id.as_str().into(),
        uid.as_str().into(),
        deck_name.as_str().into(),
        deck_json.as_str().into(),
    ])?
    .run()
    .await?;

    Response::from_json(&json!({
        "success": true,
        "deckId": deck_id,
        "deckName": deck_name
    }))
}

pub async fn delete_deck(id: &str, req: Request, env: Env) -> Result<Response> {
    let uid = resolve_user_id(&req, &env)
        .await
        .map(|(id, _)| id)
        .unwrap_or_else(|| "guest".to_string());
    let db = env.d1("DB")?;
    db.prepare("DELETE FROM decks WHERE id = ?1 AND user_id = ?2")
        .bind(&[id.into(), uid.as_str().into()])?
        .run()
        .await?;

    Response::from_json(&json!({ "success": true }))
}

pub async fn select_deck(id: &str, req: Request, env: Env) -> Result<Response> {
    let uid = resolve_user_id(&req, &env)
        .await
        .map(|(id, _)| id)
        .unwrap_or_else(|| "guest".to_string());
    let db = env.d1("DB")?;
    db.prepare("UPDATE decks SET is_active = 0 WHERE user_id = ?1")
        .bind(&[uid.as_str().into()])?
        .run()
        .await?;
    db.prepare("UPDATE decks SET is_active = 1 WHERE id = ?1 AND user_id = ?2")
        .bind(&[id.into(), uid.as_str().into()])?
        .run()
        .await?;

    Response::from_json(&json!({ "success": true, "deckId": id }))
}

pub async fn migrate_decks(mut req: Request, env: Env) -> Result<Response> {
    let Some((uid, _)) = resolve_user_id(&req, &env).await else {
        return json_err("未ログインです", 401);
    };
    let body: Value = match req.json().await {
        Ok(v) => v,
        Err(_) => return json_err("リクエストJSONが不正です", 400),
    };
    let decks = body
        .get("decks")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let db = env.d1("DB")?;
    let mut migrated = 0usize;
    for mut d in decks {
        let deck_id = d
            .get("deckId")
            .or_else(|| d.get("fileId"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("deck_{}", gen_token()));
        let deck_name = d
            .get("deckName")
            .and_then(|v| v.as_str())
            .unwrap_or("移行デッキ")
            .to_string();
        if let Some(obj) = d.as_object_mut() {
            obj.insert("deckId".to_string(), json!(deck_id));
        }
        let deck_json = serde_json::to_string(&d).unwrap_or_else(|_| "{}".to_string());
        if db
            .prepare(
                "INSERT INTO decks (id, user_id, deck_name, deck_json, is_active, updated_at)
                 VALUES (?1, ?2, ?3, ?4, 0, datetime('now'))
                 ON CONFLICT(id) DO UPDATE SET
                   deck_name = excluded.deck_name,
                   deck_json = excluded.deck_json,
                   updated_at = datetime('now')",
            )
            .bind(&[
                deck_id.as_str().into(),
                uid.as_str().into(),
                deck_name.as_str().into(),
                deck_json.as_str().into(),
            ])?
            .run()
            .await
            .is_ok()
        {
            migrated += 1;
        }
    }

    Response::from_json(&json!({ "success": true, "migrated": migrated }))
}
