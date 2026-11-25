//! Cloudflare Workers (workers-rs) エントリーポイント。
//! ネイティブビルド（ローカルサーバー / `cargo test`）時は `main.rs` が使用されます。

pub mod catalog;
pub mod models;

#[cfg(target_arch = "wasm32")]
mod worker_images;

#[cfg(target_arch = "wasm32")]
use worker::*;

#[cfg(target_arch = "wasm32")]
const CARDS_JSON: &str = include_str!("../../data/dummy/cards.json");

#[cfg(target_arch = "wasm32")]
#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    console_error_panic_hook::set_once();

    let router = Router::new();

    router
        .get("/api/health", |_req, _ctx| {
            Response::from_json(&serde_json::json!({
                "status": "ok",
                "runtime": "cloudflare-workers"
            }))
        })
        .get("/api/catalog", |_req, _ctx| {
            match catalog::CardCatalog::load_from_str(CARDS_JSON) {
                Ok(cat) => Response::from_json(&cat.raw_json),
                Err(e) => Response::error(format!("カタログ読み込みエラー: {}", e), 500),
            }
        })
        .post_async("/api/images/upload", |req, ctx| async move {
            worker_images::upload_image(req, ctx.env).await
        })
        .get_async("/api/images", |req, ctx| async move {
            worker_images::list_images(req, ctx.env).await
        })
        .get_async("/api/images/:key", |_req, ctx| async move {
            let key = ctx.param("key").cloned().unwrap_or_default();
            worker_images::get_image(&key, ctx.env).await
        })
        .get_async("/card-images/:filename", |_req, ctx| async move {
            let filename = ctx.param("filename").cloned().unwrap_or_default();
            let key = format!("cards/{}", filename);
            worker_images::get_image(&key, ctx.env).await
        })
        .run(req, env)
        .await
}
