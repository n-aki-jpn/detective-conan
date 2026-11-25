use serde::{Deserialize, Serialize};
use worker::*;

/// D1 `images` テーブルのレコード表現
#[derive(Debug, Serialize, Deserialize)]
pub struct ImageRecord {
    pub id: Option<i64>,
    pub object_key: String,
    pub public_url: String,
    pub card_id: Option<String>,
    pub title: Option<String>,
    pub content_type: String,
    pub size_bytes: i64,
    pub created_at: Option<String>,
}

/// POST /api/images/upload
/// リクエストボディの JPG バイナリを R2 バケット (`IMAGES_BUCKET`) に保存し、
/// 公開URL・オブジェクトキー・関連メタデータを D1 (`DB`) に記録する。
///
/// クエリパラメータ（任意）:
/// - `key`: 保存する R2 オブジェクトキー（省略時は自動生成）
/// - `card_id`: 関連するカードID
/// - `title`: 画像タイトルやメモ
pub async fn upload_image(mut req: Request, env: Env) -> Result<Response> {
    let url = req.url()?;
    let mut object_key: Option<String> = None;
    let mut card_id: Option<String> = None;
    let mut title: Option<String> = None;

    for (k, v) in url.query_pairs() {
        match k.as_ref() {
            "key" if !v.trim().is_empty() => object_key = Some(v.into_owned()),
            "card_id" if !v.trim().is_empty() => card_id = Some(v.into_owned()),
            "title" if !v.trim().is_empty() => title = Some(v.into_owned()),
            _ => {}
        }
    }

    let bytes = req.bytes().await?;
    if bytes.is_empty() {
        return Response::error("リクエストボディが空です（JPGバイナリを送信してください）", 400);
    }
    let size_bytes = bytes.len() as i64;

    let key = object_key.unwrap_or_else(|| {
        let ts = Date::now().as_millis();
        format!("cards/{}.jpg", ts)
    });

    // R2 バケットへアップロード
    let bucket = env.bucket("IMAGES_BUCKET")?;
    let mut http_metadata = HttpMetadata::default();
    http_metadata.content_type = Some("image/jpeg".to_string());

    bucket
        .put(&key, bytes)
        .http_metadata(http_metadata)
        .execute()
        .await?;

    // 公開URLの組み立て（環境変数 R2_PUBLIC_BASE_URL があればそれを優先、なければ Worker 経由の取得URL）
    let public_url = match env.var("R2_PUBLIC_BASE_URL") {
        Ok(base) if !base.to_string().trim().is_empty() => {
            format!("{}/{}", base.to_string().trim_end_matches('/'), key)
        }
        _ => format!("/api/images/{}", urlencoding_key(&key)),
    };

    // D1 へメタデータを保存
    let db = env.d1("DB")?;
    let stmt = db.prepare(
        "INSERT INTO images (object_key, public_url, card_id, title, content_type, size_bytes)
         VALUES (?1, ?2, ?3, ?4, 'image/jpeg', ?5)
         ON CONFLICT(object_key) DO UPDATE SET
           public_url = excluded.public_url,
           card_id = excluded.card_id,
           title = excluded.title,
           size_bytes = excluded.size_bytes",
    );

    let card_val: wasm_bindgen::JsValue = match &card_id {
        Some(c) => c.as_str().into(),
        None => wasm_bindgen::JsValue::NULL,
    };
    let title_val: wasm_bindgen::JsValue = match &title {
        Some(t) => t.as_str().into(),
        None => wasm_bindgen::JsValue::NULL,
    };

    stmt.bind(&[
        key.as_str().into(),
        public_url.as_str().into(),
        card_val,
        title_val,
        (size_bytes as f64).into(),
    ])?
    .run()
    .await?;

    Response::from_json(&serde_json::json!({
        "success": true,
        "object_key": key,
        "public_url": public_url,
        "card_id": card_id,
        "title": title,
        "size_bytes": size_bytes
    }))
}

/// GET /api/images
/// D1 から保存済み画像メタデータ一覧を取得する（`?card_id=...` で絞り込み可能）。
pub async fn list_images(req: Request, env: Env) -> Result<Response> {
    let url = req.url()?;
    let card_id = url
        .query_pairs()
        .find(|(k, _)| k == "card_id")
        .map(|(_, v)| v.into_owned());

    let db = env.d1("DB")?;
    let results = if let Some(cid) = card_id {
        let stmt = db.prepare(
            "SELECT id, object_key, public_url, card_id, title, content_type, size_bytes, created_at
             FROM images WHERE card_id = ?1 ORDER BY id DESC LIMIT 100",
        );
        stmt.bind(&[cid.as_str().into()])?.all().await?
    } else {
        let stmt = db.prepare(
            "SELECT id, object_key, public_url, card_id, title, content_type, size_bytes, created_at
             FROM images ORDER BY id DESC LIMIT 100",
        );
        stmt.all().await?
    };

    let rows: Vec<ImageRecord> = results.results()?;
    Response::from_json(&serde_json::json!({
        "images": rows
    }))
}

/// GET /api/images/:key
/// R2 バケットから指定キーの JPG 画像を取得して返却する。
pub async fn get_image(raw_key: &str, env: Env) -> Result<Response> {
    if raw_key.is_empty() {
        return Response::error("画像キーが指定されていません", 400);
    }
    let key = url::form_urlencoded::parse(format!("k={}", raw_key).as_bytes())
        .next()
        .map(|(_, v)| v.into_owned())
        .unwrap_or_else(|| raw_key.to_string());

    let bucket = env.bucket("IMAGES_BUCKET")?;
    let Some(obj) = bucket.get(&key).execute().await? else {
        return Response::error("指定された画像が見つかりません", 404);
    };

    let Some(body) = obj.body() else {
        return Response::error("画像データが空です", 404);
    };
    let bytes = body.bytes().await?;

    let mut headers = Headers::new();
    headers.set("Content-Type", "image/jpeg")?;
    headers.set("Cache-Control", "public, max-age=86400")?;

    Ok(Response::from_bytes(bytes)?.with_headers(headers))
}

fn urlencoding_key(key: &str) -> String {
    url::form_urlencoded::byte_serialize(key.as_bytes()).collect()
}
