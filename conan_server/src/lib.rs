//! Cloudflare Workers (workers-rs) エントリーポイント。
//! ネイティブビルド（ローカルサーバー / `cargo test`）時は `main.rs` が使用されます。

#[cfg(target_arch = "wasm32")]
mod worker_images;

#[cfg(target_arch = "wasm32")]
use worker::*;

#[cfg(target_arch = "wasm32")]
#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    console_error_panic_hook::set_once();

    let router = Router::new();

    router
        .get("/", |_req, _ctx| {
            Response::ok("Conan TCG Worker is running! Endpoints: GET /api/health, POST /api/images/upload, GET /api/images, GET /api/images/:key")
        })
        .get("/api/health", |_req, _ctx| {
            Response::from_json(&serde_json::json!({
                "status": "ok",
                "runtime": "cloudflare-workers"
            }))
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
        .run(req, env)
        .await
}
