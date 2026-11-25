use crate::agent::RandomAgent;
use crate::catalog::CardCatalog;
use crate::engine::MatchEngine;
use crate::models::{self, DeckDefinition, MatchPhase};
use rand::Rng;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

// ─── Room / Session Management ───────────────────────────────────────────────

pub struct MatchRoom {
    pub room_id: String,
    pub engine: MatchEngine,
    pub p2_is_cpu: bool,
    pub cpu_agent: Option<RandomAgent>,
    pub p1_deck: DeckDefinition,
    pub p2_deck: Option<DeckDefinition>,
    pub p1_connected: bool,
    pub p2_connected: bool,
    pub created_at: std::time::Instant,
    pub match_logged: bool,
    /// リプレイ記録の一意ID（match_start ごとに採番）
    pub match_id: String,
    /// replay_log のうち既にファイルへ書き出した件数
    pub replay_flushed_count: usize,
    // ─── 巻き戻し（Undo）状態 ───────────────────────────────────────────────
    /// 直前の「自分の手（mainAction/hintPlay）」提出直前のエンジン状態
    pub snapshot: Option<MatchEngine>,
    /// その手を打ったプレイヤーID
    pub snapshot_actor: Option<String>,
    /// その decision の kind
    pub snapshot_kind: Option<String>,
    /// スナップショット採番後に「打者以外」が意思決定を提出した回数（0なら巻き戻し可）
    pub others_decided_since_snapshot: usize,
    /// 巻き戻し要求中の要求者ID（None=要求なし）
    pub rewind_request: Option<String>,
}

/// リプレイ用ディレクトリ
pub fn replays_dir() -> PathBuf {
    PathBuf::from("logs/replays")
}

/// 一意な match_id を生成する（ファイル名安全な英数字のみ）
fn gen_match_id() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("m{}{}", ts, rng.gen_range(1000..9999))
}

impl MatchRoom {
    pub fn new(
        room_id: &str,
        catalog: CardCatalog,
        p1_deck: DeckDefinition,
        is_cpu: bool,
        default_p2_deck: DeckDefinition,
    ) -> Self {
        let seed: u64 = {
            use rand::Rng;
            rand::thread_rng().gen()
        };
        let engine = MatchEngine::new_with_seed(catalog.clone(), seed);
        let cpu_agent = if is_cpu {
            Some(RandomAgent::new("player2"))
        } else {
            None
        };

        let mut room = Self {
            room_id: room_id.to_string(),
            engine,
            p2_is_cpu: is_cpu,
            cpu_agent,
            p1_deck: p1_deck.clone(),
            p2_deck: if is_cpu {
                Some(default_p2_deck.clone())
            } else {
                None
            },
            p1_connected: true,
            p2_connected: is_cpu,
            created_at: std::time::Instant::now(),
            match_logged: false,
            match_id: gen_match_id(),
            replay_flushed_count: 0,
            snapshot: None,
            snapshot_actor: None,
            snapshot_kind: None,
            others_decided_since_snapshot: 0,
            rewind_request: None,
        };
        room.engine.set_match_id(&room.match_id);

        // 両プレイヤーの状態を初期化（クライアントでの観戦・表示用）
        room.engine.add_player("player1", &p1_deck);
        let p2_init_deck = if is_cpu {
            default_p2_deck
        } else {
            p1_deck.clone()
        };
        room.engine.add_player("player2", &p2_init_deck);

        if is_cpu {
            room.record_match_start();
            room.engine.start_match();
            room.process_cpu_if_needed();
        } else {
            // 対人オンライン部屋はP2参加（join_p2）まで待機フェーズ
            room.engine.phase = MatchPhase::WAITING;
            room.flush_logs();
        }

        room
    }

    pub fn join_p2(&mut self, catalog: &CardCatalog, p2_deck: DeckDefinition) {
        self.flush_logs();

        self.p2_deck = Some(p2_deck.clone());
        self.p2_connected = true;
        self.p2_is_cpu = false;
        self.cpu_agent = None;

        // P2参加と同時に、新鮮な両者デッキで試合開始！
        let seed: u64 = {
            use rand::Rng;
            rand::thread_rng().gen()
        };
        let mut engine = MatchEngine::new_with_seed(catalog.clone(), seed);
        self.match_id = gen_match_id();
        self.replay_flushed_count = 0;
        engine.set_match_id(&self.match_id);
        engine.add_player("player1", &self.p1_deck);
        engine.add_player("player2", &p2_deck);
        self.engine = engine;
        self.match_logged = false;
        self.reset_rewind_state();
        self.record_match_start();
        self.engine.start_match();
        self.flush_logs();
    }

    pub fn reset_match(&mut self, catalog: &CardCatalog) {
        // リセット前の未出力ファイル/標準出力ログをフラッシュ（未完了試合は外部送信しない）
        self.flush_logs();

        let seed: u64 = {
            use rand::Rng;
            rand::thread_rng().gen()
        };
        let mut engine = MatchEngine::new_with_seed(catalog.clone(), seed);
        self.match_id = gen_match_id();
        self.replay_flushed_count = 0;
        engine.set_match_id(&self.match_id);
        engine.add_player("player1", &self.p1_deck);
        let p2_deck = self.p2_deck.as_ref().unwrap_or(&self.p1_deck);
        engine.add_player("player2", p2_deck);
        self.engine = engine;
        self.match_logged = false;
        self.reset_rewind_state();
        self.record_match_start();
        self.engine.start_match();
        self.process_cpu_if_needed();
        self.flush_logs();
    }

    /// match_start イベントを記録する（両者デッキ・シード・先攻順の再現情報）
    pub fn record_match_start(&mut self) {
        let p2_ref = self.p2_deck.as_ref().unwrap_or(&self.p1_deck);
        let deck_compact = |deck: &DeckDefinition| -> Value {
            json!({
                "partnerId": deck.partner_id,
                "caseId": deck.case_id,
                "mainDeck": deck
                    .main_deck
                    .iter()
                    .map(|i| json!({ "definitionId": i.definition_id, "count": i.count }))
                    .collect::<Vec<_>>(),
            })
        };
        self.engine.record_event(json!({
            "type": "match_start",
            "timestamp": crate::engine::now_ts(),
            "seed": self.engine.match_seed,
            "p1Deck": deck_compact(&self.p1_deck),
            "p2Deck": deck_compact(p2_ref),
            "p2IsCpu": self.p2_is_cpu,
        }));
    }

    pub fn send_logs_to_gas_webhook(rows: Vec<Value>) {
        if rows.is_empty() {
            return;
        }
        let Ok(url) = std::env::var("GAS_LOG_WEBHOOK_URL") else {
            return;
        };
        let url = url.trim().to_string();
        if url.is_empty() {
            return;
        }
        let row_count = rows.len();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let client = reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(30))
                    .build();
                let Ok(client) = client else {
                    eprintln!("⚠️ [GAS Webhook] HTTPクライアントの構築に失敗しました");
                    return;
                };
                println!("📡 [GAS Webhook] 試合完了ログ ({}行) を送信中...", row_count);
                let payload = json!({ "rows": rows });
                match client.post(&url).json(&payload).send().await {
                    Ok(resp) => {
                        let status = resp.status();
                        if status.is_success() {
                            println!("✅ [GAS Webhook] 送信完了 (HTTP {})", status);
                        } else {
                            let body_txt = resp.text().await.unwrap_or_default();
                            eprintln!(
                                "⚠️ [GAS Webhook] HTTP {} 応答: {}",
                                status,
                                body_txt.chars().take(200).collect::<String>()
                            );
                        }
                    }
                    Err(e) => {
                        eprintln!("⚠️ [GAS Webhook] 送信失敗: {}", e);
                        tracing::warn!("Failed to post logs to GAS webhook: {}", e);
                    }
                }
            });
        }
    }

    pub fn flush_logs(&mut self) {
        use std::io::Write;
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        if !self.engine.pending_file_logs.is_empty() {
            let _ = std::fs::create_dir_all("logs");
            let lines: Vec<String> = self.engine.pending_file_logs.drain(..).collect();

            // 1. 標準出力へリアルタイム出力
            for line in &lines {
                println!("[{}][room:{}] {}", ts, self.room_id, line);
            }
            let _ = std::io::stdout().flush();

            // 2. ローカルファイル logs/all_rooms.log へ追記（上限超過時は自動ローテーション）
            for line in &lines {
                crate::logging::append_line(
                    std::path::Path::new("logs/all_rooms.log"),
                    &format!("[{}][room:{}] {}", ts, self.room_id, line),
                );
            }
        }

        // 2.5 リプレイイベントを logs/replays/<match_id>.jsonl へ追記（上限超過時は自動ローテーション）
        if self.engine.replay_log.len() > self.replay_flushed_count {
            let new_events: Vec<Value> =
                self.engine.replay_log[self.replay_flushed_count..].to_vec();
            self.replay_flushed_count = self.engine.replay_log.len();
            let path = replays_dir().join(format!("{}.jsonl", self.match_id));
            for ev in &new_events {
                crate::logging::append_line(&path, &ev.to_string());
            }
        }

        // 3. 試合が完了したとき（winner_id 確定時）のみ matches.jsonl 記録と外部 GAS Webhook 送信を行う
        if !self.match_logged {
            if let Some(ref winner) = self.engine.winner_id {
                self.match_logged = true;
                let _ = std::fs::create_dir_all("logs");

                let p1_ev = self
                    .engine
                    .players
                    .get("player1")
                    .map(|p| p.evidence.len())
                    .unwrap_or(0);
                let p2_ev = self
                    .engine
                    .players
                    .get("player2")
                    .map(|p| p.evidence.len())
                    .unwrap_or(0);
                let p1_file = self
                    .engine
                    .players
                    .get("player1")
                    .map(|p| p.effective_file_count())
                    .unwrap_or(0);
                let p2_file = self
                    .engine
                    .players
                    .get("player2")
                    .map(|p| p.effective_file_count())
                    .unwrap_or(0);
                let first_player = self
                    .engine
                    .player_order
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "player1".to_string());

                // カード定義IDを「ID:カード名(Lv.X)」に変換するヘルパー
                let fmt_def = |def_id: &str| -> String {
                    if let Some(d) = self.engine.catalog.get(def_id) {
                        let name = d.names.first().map(|s| s.as_str()).unwrap_or(def_id);
                        format!("{}:{}(Lv.{})", def_id, name, d.level)
                    } else {
                        def_id.to_string()
                    }
                };

                // 1. デッキレシピの集計サマリーを作成するヘルパー
                let build_deck_analytics = |deck: &DeckDefinition| -> Value {
                    let mut level_curve: HashMap<i32, i32> = HashMap::new();
                    let mut cards_map = serde_json::Map::new();
                    for item in &deck.main_deck {
                        cards_map.insert(fmt_def(&item.definition_id), json!(item.count));
                        if let Some(d) = self.engine.catalog.get(&item.definition_id) {
                            *level_curve.entry(d.level).or_insert(0) += item.count;
                        }
                    }
                    json!({
                        "deckId": deck.deck_id,
                        "partner": fmt_def(&deck.partner_id),
                        "case": fmt_def(&deck.case_id),
                        "cards": cards_map,
                        "levelCurve": level_curve,
                    })
                };

                // 2. 試合終了時の各ゾーン（手札残り＝腐った札、現場、リムーブ、証拠、FILE）を集計するヘルパー
                let build_zone_analytics = |pid: &str, deck: &DeckDefinition| -> Value {
                    let Some(ps) = self.engine.players.get(pid) else {
                        return json!({});
                    };
                    let to_list = |cards: &[models::CardInstance]| -> Vec<String> {
                        cards.iter().map(|c| fmt_def(&c.definition_id)).collect()
                    };

                    // デッキ内にあったカードのうち、山札(deck)に残らず一度でも引いた・絡んだカードを算出
                    let mut remaining_deck_counts: HashMap<String, i32> = HashMap::new();
                    for c in &ps.deck {
                        *remaining_deck_counts
                            .entry(c.definition_id.clone())
                            .or_insert(0) += 1;
                    }
                    let mut seen_cards: Vec<String> = Vec::new();
                    for item in &deck.main_deck {
                        let rem = remaining_deck_counts
                            .get(&item.definition_id)
                            .copied()
                            .unwrap_or(0);
                        let drawn_count = (item.count - rem).max(0);
                        for _ in 0..drawn_count {
                            seen_cards.push(fmt_def(&item.definition_id));
                        }
                    }

                    json!({
                        "field": to_list(&ps.field),
                        "endHand": to_list(&ps.hand),       // 最後まで使えず手札に残ったカード（事故要因の特定用）
                        "remove": to_list(&ps.remove),      // 使用済みイベント・リムーブされたカード
                        "evidence": to_list(&ps.evidence),
                        "file": to_list(&ps.file),
                        "seenCards": seen_cards,            // この試合中に山札から離れた（引いた/めくった）全カード
                    })
                };

                let p2_deck_ref = self.p2_deck.as_ref().unwrap_or(&self.p1_deck);
                let p1_deck_info = build_deck_analytics(&self.p1_deck);
                let p2_deck_info = build_deck_analytics(p2_deck_ref);
                let p1_zones = build_zone_analytics("player1", &self.p1_deck);
                let p2_zones = build_zone_analytics("player2", p2_deck_ref);

                let cpu_diff_str = if self.p2_is_cpu {
                    format!(
                        "{:?}",
                        self.cpu_agent
                            .as_ref()
                            .map(|a| a.difficulty)
                            .unwrap_or_default()
                    )
                    .to_lowercase()
                } else {
                    "none".to_string()
                };

                // ローカルの logs/matches.jsonl に完全な構造化データを出力
                let rec = json!({
                    "timestamp": ts,
                    "roomId": self.room_id,
                    "p2IsCpu": self.p2_is_cpu,
                    "cpuDifficulty": cpu_diff_str,
                    "firstPlayer": first_player,
                    "winner": winner,
                    "finishReason": self.engine.finish_reason,
                    "turn": self.engine.turn,
                    "p1Evidence": p1_ev,
                    "p2Evidence": p2_ev,
                    "p1File": p1_file,
                    "p2File": p2_file,
                    "p1Deck": p1_deck_info,
                    "p2Deck": p2_deck_info,
                    "p1Zones": p1_zones,
                    "p2Zones": p2_zones,
                });
                // 上限超過時は自動ローテーション
                crate::logging::append_line(
                    std::path::Path::new("logs/matches.jsonl"),
                    &rec.to_string(),
                );

                // 試合終了時にその試合の全ログと試合結果をまとめて GAS Webhook へ送信
                let mut webhook_rows: Vec<Value> = self
                    .engine
                    .logs
                    .iter()
                    .map(|line| {
                        json!({
                            "timestamp": ts,
                            "roomId": self.room_id,
                            "type": "LOG",
                            "message": line,
                        })
                    })
                    .collect();

                let format_field = |pid: &str| -> String {
                    self.engine
                        .players
                        .get(pid)
                        .map(|ps| {
                            ps.field
                                .iter()
                                .map(|c| {
                                    let def = self.engine.catalog.get(&c.definition_id);
                                    let name = def
                                        .and_then(|d| d.names.first())
                                        .map(|s| s.as_str())
                                        .unwrap_or(&c.definition_id);
                                    let ap = self.engine.get_effective_ap(pid, c);
                                    let lp = def.map(|d| d.lp).unwrap_or(1);
                                    format!("{}:{}(AP:{},LP:{})", c.definition_id, name, ap, lp)
                                })
                                .collect::<Vec<_>>()
                                .join(",")
                        })
                        .unwrap_or_default()
                };

                let format_hand = |pid: &str| -> String {
                    self.engine
                        .players
                        .get(pid)
                        .map(|ps| {
                            ps.hand
                                .iter()
                                .map(|c| fmt_def(&c.definition_id))
                                .collect::<Vec<_>>()
                                .join(",")
                        })
                        .unwrap_or_default()
                };

                let format_deck_compact = |deck: &DeckDefinition| -> String {
                    let main_str = deck
                        .main_deck
                        .iter()
                        .map(|item| format!("{}x{}", item.definition_id, item.count))
                        .collect::<Vec<_>>()
                        .join(",");
                    format!("P:{}/C:{}/M:[{}]", deck.partner_id, deck.case_id, main_str)
                };

                let match_summary_msg = format!(
                    "winner={} first={} reason={:?} turn={} p1_ev={} p2_ev={} p1_file={} p2_file={} cpu={} cpu_diff={} p1_field=[{}] p2_field=[{}] p1_hand=[{}] p2_hand=[{}] p1_deck={} p2_deck={}",
                    winner,
                    first_player,
                    self.engine.finish_reason,
                    self.engine.turn,
                    p1_ev,
                    p2_ev,
                    p1_file,
                    p2_file,
                    self.p2_is_cpu,
                    cpu_diff_str,
                    format_field("player1"),
                    format_field("player2"),
                    format_hand("player1"),
                    format_hand("player2"),
                    format_deck_compact(&self.p1_deck),
                    format_deck_compact(p2_deck_ref)
                );

                // 標準出力にも試合完了サマリーを出力
                println!("[{}][room:{}][MATCH] {}", ts, self.room_id, match_summary_msg);
                let _ = std::io::stdout().flush();

                // 既存の MATCH 行に p1_hand, p2_hand, p1_deck, p2_deck を追加
                webhook_rows.push(json!({
                    "timestamp": ts,
                    "roomId": self.room_id,
                    "type": "MATCH",
                    "message": match_summary_msg,
                }));
                Self::send_logs_to_gas_webhook(webhook_rows);
            }
        }
    }

    pub fn process_cpu_if_needed(&mut self) {
        let max_steps = 50;
        let mut steps = 0;
        loop {
            if steps >= max_steps || !self.p2_is_cpu {
                break;
            }
            match &self.engine.pending_decision {
                Some(dec) => {
                    if dec.get("playerId").and_then(|v| v.as_str()) != Some("player2") {
                        break;
                    }
                    let ans = match &self.cpu_agent {
                        Some(agent) => agent.choose_action_with_engine(&self.engine, dec),
                        None => break,
                    };
                    // CPUの応答もトラッキング（打者以外の応答＝隠情報開示として巻き戻し不可化）
                    if self.submit_decision_tracked("player2", &ans).is_err() {
                        break;
                    }
                }
                None => break,
            }
            steps += 1;
        }
        self.flush_logs();
    }

    // ─── 巻き戻し（Undo）─────────────────────────────────────────────────────

    /// 巻き戻し状態を初期化（マッチ開始・リセット時）
    pub fn reset_rewind_state(&mut self) {
        self.snapshot = None;
        self.snapshot_actor = None;
        self.snapshot_kind = None;
        self.others_decided_since_snapshot = 0;
        self.rewind_request = None;
    }

    /// 意思決定を記録付きで提出する。
    /// 主行動(mainAction)・ヒント使用(hintPlay)の直前にエンジン状態をスナップショット保存し、
    /// それ以降に「打者以外」が意思決定したら巻き戻し不可にする（隠情報リーク防止）。
    pub fn submit_decision_tracked(&mut self, player_id: &str, answer: &Value) -> Result<(), String> {
        let (kind, actor) = self
            .engine
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
            // 主行動の直前スナップショット（この1手が巻き戻し対象）
            self.snapshot = Some(self.engine.clone());
            self.snapshot_actor = Some(actor.clone());
            self.snapshot_kind = Some(kind.clone());
            self.others_decided_since_snapshot = 0;
            self.rewind_request = None;
        } else if self.snapshot_actor.as_deref() == Some(actor.as_str()) {
            // 同プレイヤーの応答系（対象選択等）は隠情報を開示しないためカウントしない
        } else if self.snapshot_actor.is_some() {
            // 相手側の応答（ガード成否・手札開示等）＝隠情報開示 → 巻き戻し不可化
            self.others_decided_since_snapshot += 1;
        }

        self.engine.submit_decision(player_id, answer)
    }

    /// 指定プレイヤーが巻き戻し可能かを判定
    pub fn can_rewind_by(&self, requester: &str) -> bool {
        self.snapshot.is_some()
            && self.engine.winner_id.is_none()
            && self.snapshot_actor.as_deref() == Some(requester)
            && self.others_decided_since_snapshot == 0
    }

    /// 巻き戻しを要求。CPU対戦で要求者が人間(player1)なら CPU が自動同意する。
    pub fn request_rewind(&mut self, requester: &str) -> Result<(), String> {
        if !self.can_rewind_by(requester) {
            return Err("現在は巻き戻しできません（既に相手の応答が出ているか、自分の手ではありません）".to_string());
        }
        self.rewind_request = Some(requester.to_string());
        // CPU対戦: 人間側の要求には自動同意
        if self.p2_is_cpu && requester == "player1" {
            return self.accept_rewind("player2");
        }
        Ok(())
    }

    /// 相手側が同意して巻き戻しを実行する。
    pub fn accept_rewind(&mut self, acceptor: &str) -> Result<(), String> {
        let requester = self
            .rewind_request
            .clone()
            .ok_or_else(|| "巻き戻し要求はありません".to_string())?;
        // 同意者は要求者以外の対戦相手であること
        if acceptor == requester {
            return Err("相手側の同意が必要です".to_string());
        }
        if !self.can_rewind_by(&requester) {
            self.rewind_request = None;
            return Err("巻き戻し条件が満たされなくなりました".to_string());
        }
        let snap = self
            .snapshot
            .take()
            .ok_or_else(|| "スナップショットがありません".to_string())?;

        // スナップショット（手提出直前）でエンジンを復元。
        // リプレイファイルは追記監査ログなので、復元時点までの行は既に書き出し済みとみなす。
        self.engine = snap;
        self.replay_flushed_count = self.engine.replay_log.len();
        self.engine.record_event(json!({
            "type": "rewind",
            "timestamp": crate::engine::now_ts(),
            "requester": requester.clone(),
            "acceptedBy": acceptor.clone(),
            "revision": self.engine.revision,
            "turn": self.engine.turn,
        }));
        self.reset_rewind_state();
        self.flush_logs();
        Ok(())
    }

    /// 相手側が巻き戻しを拒否する。
    pub fn reject_rewind(&mut self, rejector: &str) -> Result<(), String> {
        let _ = rejector;
        self.rewind_request = None;
        Ok(())
    }
}

// ─── Match Manager ────────────────────────────────────────────────────────────

pub struct MatchManager {
    pub deck_path: PathBuf,
    pub decks_dir: PathBuf,
    pub user_decks_dir: PathBuf,
    pub catalog: CardCatalog,
    pub deck_def: DeckDefinition,
    pub rooms: HashMap<String, MatchRoom>,
    pub user_manager: crate::user::UserManager,
    /// 認証系エンドポイントのレートリミッタ
    pub rate_limiter: crate::rate_limiter::RateLimiter,
}

pub fn sanitize_deck(
    sent_deck: Option<DeckDefinition>,
    fallback_deck: &DeckDefinition,
) -> DeckDefinition {
    if let Some(d) = sent_deck {
        let count: i32 = d.main_deck.iter().map(|item| item.count).sum();
        if count == 40 && !d.partner_id.is_empty() && !d.case_id.is_empty() {
            return d;
        }
    }
    fallback_deck.clone()
}

pub fn load_deck_from_dir(decks_dir: &Path, id: &str) -> Option<DeckDefinition> {
    let safe_id: String = id
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    if safe_id.is_empty() {
        return None;
    }
    let target = decks_dir.join(format!("{}.json", safe_id));
    if let Ok(content) = std::fs::read_to_string(&target) {
        if let Ok(deck) = serde_json::from_str::<DeckDefinition>(&content) {
            return Some(deck);
        }
    }
    if let Ok(entries) = std::fs::read_dir(decks_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(deck) = serde_json::from_str::<DeckDefinition>(&content) {
                        if deck.deck_id.as_deref() == Some(id) {
                            return Some(deck);
                        }
                    }
                }
            }
        }
    }
    None
}

impl MatchManager {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let catalog_path = PathBuf::from("data/dummy/cards.json");
        let deck_path = PathBuf::from("data/dummy/deck.json");
        let decks_dir = PathBuf::from("data/dummy/decks");
        let user_decks_dir = PathBuf::from("data/dummy/user_decks");

        if !decks_dir.exists() {
            let _ = std::fs::create_dir_all(&decks_dir);
        }
        if !user_decks_dir.exists() {
            let _ = std::fs::create_dir_all(&user_decks_dir);
        }

        let catalog = CardCatalog::load_from_json(&catalog_path)?;
        let deck_def = Self::load_deck(&deck_path)?;
        let user_manager = crate::user::UserManager::new(Path::new("data/dummy"));

        let mut mgr = MatchManager {
            deck_path,
            decks_dir,
            user_decks_dir,
            catalog: catalog.clone(),
            deck_def: deck_def.clone(),
            rooms: HashMap::new(),
            user_manager,
            rate_limiter: crate::rate_limiter::RateLimiter::new(),
        };

        // Create initial default local room
        let default_room =
            MatchRoom::new("default", catalog, deck_def.clone(), true, deck_def);
        mgr.rooms.insert("default".to_string(), default_room);

        Ok(mgr)
    }

    pub fn load_deck(path: &PathBuf) -> Result<DeckDefinition, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let deck: DeckDefinition = serde_json::from_str(&content)?;
        Ok(deck)
    }

    pub fn save_deck(&mut self, new_def: DeckDefinition) -> Result<(), String> {
        // クライアント側（localStorage）保存に移行したため、サーバー側ファイルには書き込まずメモリのみ保持
        self.deck_def = new_def;
        Ok(())
    }

    pub fn list_decks(&self) -> Vec<Value> {
        let mut list = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.decks_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("json") {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        if let Ok(val) = serde_json::from_str::<Value>(&content) {
                            let file_stem = path
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or("")
                                .to_string();
                            let deck_name = val
                                .get("deckName")
                                .and_then(|v| v.as_str())
                                .unwrap_or(&file_stem)
                                .to_string();
                            let partner_id = val
                                .get("partnerId")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            let case_id = val
                                .get("caseId")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            let total_cards: i64 = val
                                .get("mainDeck")
                                .and_then(|v| v.as_array())
                                .map(|arr| {
                                    arr.iter()
                                        .filter_map(|item| {
                                            item.get("count").and_then(|c| c.as_i64())
                                        })
                                        .sum()
                                })
                                .unwrap_or(0);

                            list.push(json!({
                                "fileId": file_stem,
                                "deckId": val.get("deckId").and_then(|v| v.as_str()).unwrap_or(&file_stem),
                                "deckName": deck_name,
                                "partnerId": partner_id,
                                "caseId": case_id,
                                "totalCards": total_cards,
                                "isActive": self.deck_def.deck_id.as_deref() == val.get("deckId").and_then(|v| v.as_str())
                            }));
                        }
                    }
                }
            }
        }
        list
    }

    pub fn create_room(&mut self, is_cpu: bool, p1_deck: Option<DeckDefinition>) -> String {
        let mut rng = rand::thread_rng();
        let code: u32 = rng.gen_range(1000..9999);
        let room_id = format!("{}", code);

        let deck = sanitize_deck(p1_deck, &self.deck_def);
        let room = MatchRoom::new(
            &room_id,
            self.catalog.clone(),
            deck,
            is_cpu,
            self.deck_def.clone(),
        );
        self.rooms.insert(room_id.clone(), room);
        room_id
    }

    // ─── ユーザー単位のデッキ管理 ────────────────────────────────────────────────

    fn user_decks_dir_for(&self, user_id: &str) -> PathBuf {
        let safe_id: String = user_id
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
            .collect();
        self.user_decks_dir
            .join(if safe_id.is_empty() { "unknown" } else { &safe_id })
    }

    fn deck_meta_from_value(val: &Value, file_stem: &str, is_preset: bool) -> Value {
        let deck_name = val
            .get("deckName")
            .and_then(|v| v.as_str())
            .unwrap_or(file_stem)
            .to_string();
        let partner_id = val
            .get("partnerId")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let case_id = val
            .get("caseId")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let total_cards: i64 = val
            .get("mainDeck")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| item.get("count").and_then(|c| c.as_i64()))
                    .sum()
            })
            .unwrap_or(0);
        json!({
            "fileId": file_stem,
            "deckId": val.get("deckId").and_then(|v| v.as_str()).unwrap_or(file_stem),
            "deckName": deck_name,
            "partnerId": partner_id,
            "caseId": case_id,
            "totalCards": total_cards,
            "isPreset": is_preset,
        })
    }

    /// ユーザーが利用可能なデッキ一覧（共有プリセット + 自分のデッキ）。
    pub fn list_user_decks(&self, user_id: &str) -> Vec<Value> {
        let mut list = Vec::new();
        // 共有プリセット（data/dummy/decks 配下）
        if let Ok(entries) = std::fs::read_dir(&self.decks_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("json") {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        if let Ok(val) = serde_json::from_str::<Value>(&content) {
                            let file_stem = path
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or("")
                                .to_string();
                            let is_preset = file_stem.starts_with("preset_");
                            let mut meta = Self::deck_meta_from_value(&val, &file_stem, is_preset);
                            let deck_id = meta
                                .get("deckId")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            meta["isActive"] =
                                json!(self.deck_def.deck_id.as_deref() == Some(deck_id.as_str()));
                            list.push(meta);
                        }
                    }
                }
            }
        }
        // ユーザー自身のデッキ（data/dummy/user_decks/{user_id} 配下）
        let dir = self.user_decks_dir_for(user_id);
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("json") {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        if let Ok(val) = serde_json::from_str::<Value>(&content) {
                            let file_stem = path
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or("")
                                .to_string();
                            let mut meta = Self::deck_meta_from_value(&val, &file_stem, false);
                            let deck_id = meta
                                .get("deckId")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            meta["isActive"] =
                                json!(self.deck_def.deck_id.as_deref() == Some(deck_id.as_str()));
                            list.push(meta);
                        }
                    }
                }
            }
        }
        list
    }

    /// ユーザーのデッキを保存。ファイル名はdeckIdをサニタイズして使用。
    pub fn save_user_deck(&mut self, user_id: &str, deck: &DeckDefinition) -> Result<String, String> {
        let dir = self.user_decks_dir_for(user_id);
        if let Err(e) = std::fs::create_dir_all(&dir) {
            return Err(format!("デッキディレクトリの作成に失敗しました: {}", e));
        }
        let deck_id = deck
            .deck_id
            .clone()
            .unwrap_or_else(|| format!("deck_{}", std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)));
        let safe_id: String = deck_id
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
            .collect();
        if safe_id.is_empty() || safe_id.starts_with("preset_") {
            return Err("無効なデッキIDです".to_string());
        }
        let path = dir.join(format!("{}.json", safe_id));
        let content = serde_json::to_string_pretty(deck)
            .map_err(|e| format!("デッキのシリアライズに失敗しました: {}", e))?;
        std::fs::write(&path, content).map_err(|e| format!("デッキファイルの書き込みに失敗しました: {}", e))?;
        // デフォルトルーム（ソロ）のアクティブデッキも同期
        self.deck_def = deck.clone();
        Ok(deck_id)
    }

    /// IDからデッキを読み込み（共有プリセット優先、次にユーザーディレクトリ）。
    pub fn get_user_deck(&self, user_id: &str, deck_id: &str) -> Option<DeckDefinition> {
        if let Some(deck) = load_deck_from_dir(&self.decks_dir, deck_id) {
            return Some(deck);
        }
        let dir = self.user_decks_dir_for(user_id);
        load_deck_from_dir(&dir, deck_id)
    }

    /// ユーザーのデッキを削除。共有プリセット（preset_ 接頭辞）は削除不可。
    pub fn delete_user_deck(&mut self, user_id: &str, deck_id: &str) -> Result<(), String> {
        let safe_id: String = deck_id
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
            .collect();
        if safe_id.starts_with("preset_") {
            return Err("共有プリセットデッキは削除できません".to_string());
        }
        let dir = self.user_decks_dir_for(user_id);
        let path = dir.join(format!("{}.json", safe_id));
        if path.exists() {
            std::fs::remove_file(&path)
                .map_err(|e| format!("デッキファイルの削除に失敗しました: {}", e))?;
            return Ok(());
        }
        // fileIdが一致しない場合、deckIdフィールドで検索して削除
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.extension().and_then(|s| s.to_str()) == Some("json") {
                    if let Ok(content) = std::fs::read_to_string(&p) {
                        if let Ok(deck) = serde_json::from_str::<DeckDefinition>(&content) {
                            if deck.deck_id.as_deref() == Some(deck_id) {
                                std::fs::remove_file(&p)
                                    .map_err(|e| format!("デッキファイルの削除に失敗しました: {}", e))?;
                                return Ok(());
                            }
                        }
                    }
                }
            }
        }
        Err("デッキが見つかりません".to_string())
    }

    /// クライアント（localStorage）のデッキをユーザーディレクトリへ移行。
    /// 冪等：既存のdeckIdはスキップ。戻り値は (imported, skipped)。
    pub fn migrate_user_decks(&mut self, user_id: &str, decks: Vec<DeckDefinition>) -> (usize, usize) {
        let dir = self.user_decks_dir_for(user_id);
        let _ = std::fs::create_dir_all(&dir);
        let mut imported = 0usize;
        let mut skipped = 0usize;
        for deck in decks {
            let deck_id = match deck.deck_id.as_deref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
                Some(id) => id.to_string(),
                None => {
                    skipped += 1;
                    continue;
                }
            };
            if deck_id.starts_with("preset_") || deck_id.starts_with("user_preset_") {
                skipped += 1;
                continue;
            }
            let safe_id: String = deck_id
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
                .collect();
            if safe_id.is_empty() {
                skipped += 1;
                continue;
            }
            let path = dir.join(format!("{}.json", safe_id));
            if path.exists() {
                skipped += 1;
                continue;
            }
            match serde_json::to_string_pretty(&deck) {
                Ok(content) => {
                    if std::fs::write(&path, content).is_ok() {
                        imported += 1;
                    } else {
                        skipped += 1;
                    }
                }
                Err(_) => skipped += 1,
            }
        }
        (imported, skipped)
    }

    // ─── リプレイ記録の取得 ────────────────────────────────────────────────────

    /// match_id をファイル名安全な文字のみへ正規化する
    fn sanitize_match_id(id: &str) -> String {
        id.chars()
            .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
            .collect()
    }

    /// 保存済みリプレイの一覧を返す（各ファイルの match_start / match_end から要約を抽出）
    pub fn list_replays(&self, limit: usize) -> Vec<Value> {
        let dir = replays_dir();
        let mut items: Vec<(u64, Value)> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
                    continue;
                }
                let content = match std::fs::read_to_string(&path) {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                let mut start: Option<Value> = None;
                let mut end: Option<Value> = None;
                let mut event_count = 0usize;
                for line in content.lines() {
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    if let Ok(ev) = serde_json::from_str::<Value>(line) {
                        event_count += 1;
                        match ev.get("type").and_then(|t| t.as_str()) {
                            Some("match_start") => start = Some(ev),
                            Some("match_end") => end = Some(ev),
                            _ => {}
                        }
                    }
                }
                let match_id = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();
                let ts = start
                    .as_ref()
                    .and_then(|s| s.get("timestamp").and_then(|v| v.as_u64()))
                    .unwrap_or(0);
                items.push((
                    ts,
                    json!({
                        "matchId": match_id,
                        "eventCount": event_count,
                        "seed": start.as_ref().and_then(|s| s.get("seed").cloned()),
                        "p1DeckPartner": start.as_ref()
                            .and_then(|s| s.get("p1Deck").and_then(|d| d.get("partnerId")).cloned()),
                        "p2DeckPartner": start.as_ref()
                            .and_then(|s| s.get("p2Deck").and_then(|d| d.get("partnerId")).cloned()),
                        "p2IsCpu": start.as_ref().and_then(|s| s.get("p2IsCpu").cloned()),
                        "winnerId": end.as_ref().and_then(|e| e.get("winnerId").cloned()),
                        "finishReason": end.as_ref().and_then(|e| e.get("finishReason").cloned()),
                        "finished": end.is_some(),
                        "startedAt": ts,
                    }),
                ));
            }
        }
        // 新しい順にソートして limit 件
        items.sort_by(|a, b| b.0.cmp(&a.0));
        items.into_iter().take(limit).map(|(_, v)| v).collect()
    }

    /// 指定 match_id の全リプレイイベントを返す
    pub fn get_replay(&self, match_id: &str) -> Option<Vec<Value>> {
        let safe = Self::sanitize_match_id(match_id);
        if safe.is_empty() {
            return None;
        }
        let path = replays_dir().join(format!("{}.jsonl", safe));
        let content = std::fs::read_to_string(&path).ok()?;
        let mut events = Vec::new();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(ev) = serde_json::from_str::<Value>(line) {
                events.push(ev);
            }
        }
        Some(events)
    }
}

pub type SharedState = Arc<Mutex<MatchManager>>;
