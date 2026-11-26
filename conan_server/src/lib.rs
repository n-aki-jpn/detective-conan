//! Cloudflare Workers (workers-rs) エントリーポイント。
//! ネイティブビルド（ローカルサーバー / `cargo test`）時は `main.rs` が使用されます。

pub mod agent;
pub mod catalog;
pub mod effects;
pub mod engine;
pub mod models;
pub mod observation;

#[cfg(target_arch = "wasm32")]
mod worker_auth;
#[cfg(target_arch = "wasm32")]
mod worker_images;
#[cfg(target_arch = "wasm32")]
mod worker_rooms;

#[cfg(target_arch = "wasm32")]
use worker::*;

#[cfg(target_arch = "wasm32")]
const CARDS_JSON: &str = include_str!("../../data/dummy/cards.json");

#[cfg(target_arch = "wasm32")]
#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    console_error_panic_hook::set_once();

    let router = Router::new();

    let res = router
        .get("/favicon.ico", |_req, _ctx| Response::empty().map(|r| r.with_status(204)))
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
        .post_async("/api/register", |req, ctx| async move {
            worker_auth::register(req, ctx.env).await
        })
        .post_async("/api/login", |req, ctx| async move {
            worker_auth::login(req, ctx.env).await
        })
        .get_async("/api/me", |req, ctx| async move {
            worker_auth::me(req, ctx.env).await
        })
        .get_async("/api/deck", |req, ctx| async move {
            worker_auth::get_active_deck(req, ctx.env).await
        })
        .get_async("/api/decks", |req, ctx| async move {
            worker_auth::list_decks(req, ctx.env).await
        })
        .get_async("/api/decks/:id", |_req, ctx| async move {
            let id = ctx.param("id").cloned().unwrap_or_default();
            worker_auth::get_deck_by_id(&id, ctx.env).await
        })
        .delete_async("/api/decks/:id", |req, ctx| async move {
            let id = ctx.param("id").cloned().unwrap_or_default();
            worker_auth::delete_deck(&id, req, ctx.env).await
        })
        .post_async("/api/decks/:id/select", |req, ctx| async move {
            let id = ctx.param("id").cloned().unwrap_or_default();
            worker_auth::select_deck(&id, req, ctx.env).await
        })
        .post_async("/api/save_deck", |req, ctx| async move {
            worker_auth::save_deck(req, ctx.env).await
        })
        .get_async("/api/state", |req, ctx| async move {
            worker_rooms::get_state(req, ctx.env, CARDS_JSON).await
        })
        .post_async("/api/decision", |req, ctx| async move {
            worker_rooms::submit_decision(req, ctx.env, CARDS_JSON).await
        })
        .post_async("/api/reset", |req, ctx| async move {
            worker_rooms::reset_room(req, ctx.env, CARDS_JSON).await
        })
        .post_async("/api/set_cpu", |req, ctx| async move {
            worker_rooms::set_cpu(req, ctx.env).await
        })
        .post_async("/api/rooms/create", |req, ctx| async move {
            worker_rooms::create_room(req, ctx.env).await
        })
        .post_async("/api/rooms/join", |req, ctx| async move {
            worker_rooms::join_room(req, ctx.env).await
        })
        .post_async("/api/rewind", |req, _ctx| async move {
            worker_rooms::rewind_stub(req).await
        })
        .post_async("/api/rewind/accept", |req, _ctx| async move {
            worker_rooms::rewind_stub(req).await
        })
        .post_async("/api/rewind/reject", |req, _ctx| async move {
            worker_rooms::rewind_stub(req).await
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
        .await;

    match res {
        Ok(r) => Ok(r),
        Err(e) => Response::from_json(&serde_json::json!({
            "error": format!("Worker internal error: {}", e)
        }))
        .map(|r| r.with_status(500)),
    }
}
