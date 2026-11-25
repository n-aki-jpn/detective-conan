use axum::{
    routing::{delete, get, post},
    Router,
};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tower_http::services::ServeDir;

mod agent;
mod catalog;
mod effects;
mod engine;
mod handlers;
mod logging;
mod models;
mod observation;
mod official_deck;
mod rate_limiter;
mod room;
mod user;
#[cfg(test)]
mod tests;

use handlers::{
    handle_catalog, handle_create_room, handle_decision, handle_deck, handle_delete_deck_by_id,
    handle_get_deck_by_id, handle_get_logs, handle_get_replay, handle_join_room, handle_list_decks,
    handle_list_replays, handle_login, handle_me, handle_migrate_decks, handle_register,
    handle_reset, handle_save_deck, handle_select_deck, handle_set_cpu, handle_state,
};
use official_deck::{handle_import_official_deck, handle_publish_deck};
use room::{MatchManager, SharedState};

/// カレントディレクトリの `.env` ファイルがあれば環境変数として読み込む（外部クレート不要）
fn load_dotenv_if_present() {
    let Ok(content) = std::fs::read_to_string(".env") else {
        return;
    };
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line).trim();
        if let Some((k, v)) = line.split_once('=') {
            let key = k.trim();
            let mut val = v.trim();
            if (val.starts_with('"') && val.ends_with('"'))
                || (val.starts_with('\'') && val.ends_with('\''))
            {
                val = &val[1..val.len() - 1];
            }
            if !key.is_empty() && std::env::var_os(key).is_none() {
                std::env::set_var(key, val);
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    load_dotenv_if_present();
    tracing_subscriber::fmt::init();

    if let Ok(url) = std::env::var("GAS_LOG_WEBHOOK_URL") {
        if !url.trim().is_empty() {
            println!("📡 GAS_LOG_WEBHOOK_URL が設定されています（試合完了時のWebhook送信有効）");
        } else {
            println!("ℹ️ GAS_LOG_WEBHOOK_URL が空です（ローカルファイル logs/ 及び標準出力のみ記録）");
        }
    } else {
        println!("ℹ️ GAS_LOG_WEBHOOK_URL 未設定（ローカルファイル logs/ 及び標準出力のみ記録）");
    }

    let args: Vec<String> = std::env::args().collect();
    let mgr = MatchManager::new().expect("MatchManager 初期化に失敗しました");

    if args.get(1).map(|s| s.as_str()) == Some("train") {
        let generations: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(10);
        let games_per_gen: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(20);
        let p1_deck = if let Some(path) = args.get(4) {
            match MatchManager::load_deck(&PathBuf::from(path)) {
                Ok(d) => {
                    println!(
                        "📘 P1デッキを読み込みました: {} ({})",
                        d.deck_name.as_deref().unwrap_or("名称未設定"),
                        path
                    );
                    d
                }
                Err(e) => {
                    eprintln!("❌ P1デッキの読み込みに失敗しました ({}): {}", path, e);
                    return Err(e);
                }
            }
        } else {
            mgr.deck_def.clone()
        };
        let p2_deck = if let Some(path) = args.get(5) {
            match MatchManager::load_deck(&PathBuf::from(path)) {
                Ok(d) => {
                    println!(
                        "📕 P2(環境)デッキを読み込みました: {} ({})",
                        d.deck_name.as_deref().unwrap_or("名称未設定"),
                        path
                    );
                    d
                }
                Err(e) => {
                    eprintln!("❌ P2デッキの読み込みに失敗しました ({}): {}", path, e);
                    return Err(e);
                }
            }
        } else {
            p1_deck.clone()
        };
        agent::train_cpu_self_play(
            &mgr.catalog,
            &p1_deck,
            &p2_deck,
            generations,
            games_per_gen,
            "data/cpu_weights.json",
        );
        return Ok(());
    }

    if args.get(1).map(|s| s.as_str()) == Some("simulate") {
        // 固定シード・シミュレーション:
        //   simulate [games] [seed] [p1_deck_path] [p2_deck_path]
        // 同一引数なら結果が完全に再現される（回帰テスト・バランス調整用）
        let games: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(100);
        let seed: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(0);
        let p1_deck = if let Some(path) = args.get(4) {
            match MatchManager::load_deck(&PathBuf::from(path)) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("❌ P1デッキの読み込みに失敗しました ({}): {}", path, e);
                    return Err(e);
                }
            }
        } else {
            mgr.deck_def.clone()
        };
        let p2_deck = if let Some(path) = args.get(5) {
            match MatchManager::load_deck(&PathBuf::from(path)) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("❌ P2デッキの読み込みに失敗しました ({}): {}", path, e);
                    return Err(e);
                }
            }
        } else {
            p1_deck.clone()
        };
        let w = agent::CpuWeights::load_or_default("data/cpu_weights.json");
        let summary = agent::run_fixed_seed_simulation(
            &mgr.catalog,
            &p1_deck,
            &p2_deck,
            &w,
            &w,
            games,
            seed,
        );
        println!("{}", serde_json::to_string_pretty(&summary).unwrap());
        return Ok(());
    }

    let shared_state: SharedState = Arc::new(Mutex::new(mgr));

    let static_dir = PathBuf::from("src/conan_engine/web/static");

    let app = Router::new()
        .route("/api/register", post(handle_register))
        .route("/api/login", post(handle_login))
        .route("/api/me", get(handle_me))
        .route("/api/state", get(handle_state))
        .route("/api/logs", get(handle_get_logs))
        .route("/api/replays", get(handle_list_replays))
        .route("/api/replays/:id", get(handle_get_replay))
        .route("/api/catalog", get(handle_catalog))
        .route("/api/deck", get(handle_deck))
        .route("/api/decks", get(handle_list_decks))
        .route("/api/decks/migrate", post(handle_migrate_decks))
        .route("/api/decks/:id", get(handle_get_deck_by_id))
        .route("/api/decks/:id", delete(handle_delete_deck_by_id))
        .route("/api/decks/:id/select", post(handle_select_deck))
        .route("/api/rooms/create", post(handle_create_room))
        .route("/api/rooms/join", post(handle_join_room))
        .route("/api/reset", post(handle_reset))
        .route("/api/set_cpu", post(handle_set_cpu))
        .route("/api/save_deck", post(handle_save_deck))
        .route("/api/deck/publish", post(handle_publish_deck))
        .route("/api/deck/import_official", post(handle_import_official_deck))
        .route("/api/decision", post(handle_decision))
        .fallback_service(ServeDir::new(&static_dir))
        .with_state(shared_state);

    let port: u16 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(8000);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    println!("🚀 Detective Conan TCG Server (Rust) running at http://0.0.0.0:{port}");
    println!("📝 ログ出力先: 標準出力 / logs/all_rooms.log / logs/matches.jsonl / GET /api/logs");
    println!(
        "♻️ ログローテーション: 1ファイル上限 {}MB / バックアップ保持数 {} 世代（LOG_MAX_BYTES / LOG_KEEP で調整可）",
        logging::max_bytes() / (1024 * 1024),
        logging::keep_count()
    );
    println!("📲 スマホから接続する場合: 同じWi-Fiに接続し http://<このPCのIPアドレス>:{port} をブラウザで開いてください");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

    Ok(())
}
