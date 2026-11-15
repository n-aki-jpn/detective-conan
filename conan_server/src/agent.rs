use crate::catalog::CardCatalog;
use crate::engine::MatchEngine;
use crate::models::{CardOrientation, CardType, CasePhase, DeckDefinition};
use rand::seq::SliceRandom;
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuWeights {
    pub level_weight: f64,
    pub ap_weight: f64,
    pub lp_weight: f64,
    pub rush_bonus: f64,
    pub on_play_bonus: f64,
    pub event_penalty: f64,
    pub removal_bonus: f64,
    pub assist_min_field: usize,
    pub deduce_lp_weight: f64,
    pub attack_remove_weight: f64,
    pub case_attack_bonus: f64,
    pub disguise_ap_threshold: i32,
}

impl Default for CpuWeights {
    fn default() -> Self {
        Self {
            level_weight: 1050.0,
            ap_weight: 1.0,
            lp_weight: 600.0,
            rush_bonus: 1400.0,
            on_play_bonus: 950.0,
            event_penalty: -200.0,
            removal_bonus: 1100.0,
            assist_min_field: 2,
            deduce_lp_weight: 1200.0,
            attack_remove_weight: 1500.0,
            case_attack_bonus: 1100.0,
            disguise_ap_threshold: 1000,
        }
    }
}

impl CpuWeights {
    pub fn load_or_default(path: &str) -> Self {
        if let Ok(content) = std::fs::read_to_string(path) {
            if let Ok(w) = serde_json::from_str::<Self>(&content) {
                return w;
            }
        }
        Self::default()
    }

    pub fn save(&self, path: &str) -> std::io::Result<()> {
        if let Some(parent) = std::path::Path::new(path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let s = serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string());
        std::fs::write(path, s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CpuDifficulty {
    Easy,
    Normal,
    Hard,
}

impl Default for CpuDifficulty {
    fn default() -> Self {
        Self::Hard
    }
}

#[derive(Debug)]
pub struct RandomAgent {
    pub player_id: String,
    pub weights: CpuWeights,
    pub difficulty: CpuDifficulty,
    last_turn: AtomicI32,
    hints_used_this_turn: AtomicUsize,
    last_hint_skipped: AtomicBool,
}

impl Clone for RandomAgent {
    fn clone(&self) -> Self {
        Self {
            player_id: self.player_id.clone(),
            weights: self.weights.clone(),
            difficulty: self.difficulty,
            last_turn: AtomicI32::new(self.last_turn.load(Ordering::Relaxed)),
            hints_used_this_turn: AtomicUsize::new(self.hints_used_this_turn.load(Ordering::Relaxed)),
            last_hint_skipped: AtomicBool::new(self.last_hint_skipped.load(Ordering::Relaxed)),
        }
    }
}

impl RandomAgent {
    pub fn new(player_id: &str) -> Self {
        Self {
            player_id: player_id.to_string(),
            weights: CpuWeights::load_or_default("data/cpu_weights.json"),
            difficulty: CpuDifficulty::Hard,
            last_turn: AtomicI32::new(-1),
            hints_used_this_turn: AtomicUsize::new(0),
            last_hint_skipped: AtomicBool::new(false),
        }
    }

    pub fn with_difficulty(player_id: &str, difficulty: CpuDifficulty) -> Self {
        Self {
            player_id: player_id.to_string(),
            weights: CpuWeights::load_or_default("data/cpu_weights.json"),
            difficulty,
            last_turn: AtomicI32::new(-1),
            hints_used_this_turn: AtomicUsize::new(0),
            last_hint_skipped: AtomicBool::new(false),
        }
    }

    pub fn with_weights(player_id: &str, weights: CpuWeights) -> Self {
        Self {
            player_id: player_id.to_string(),
            weights,
            difficulty: CpuDifficulty::Hard,
            last_turn: AtomicI32::new(-1),
            hints_used_this_turn: AtomicUsize::new(0),
            last_hint_skipped: AtomicBool::new(false),
        }
    }

    fn reset_turn_state_if_needed(&self, turn_number: i32) {
        let prev = self.last_turn.load(Ordering::Relaxed);
        if prev != turn_number {
            self.last_turn.store(turn_number, Ordering::Relaxed);
            self.hints_used_this_turn.store(0, Ordering::Relaxed);
            self.last_hint_skipped.store(false, Ordering::Relaxed);
        }
    }

    /// 【宣言】効果が空撃ちや自傷にならないか事前検証する
    fn is_declare_beneficial(&self, engine: &MatchEngine, actor_id: &str, opp_id: &str) -> bool {
        let Some(my_ps) = engine.players.get(&self.player_id) else {
            return false;
        };
        let opp_ps = engine.players.get(opp_id);

        // 事件カードの宣言（青の古城探索事件 / ブラックインパクト！など）
        if let Some(case_card) = &my_ps.r#case {
            if case_card.instance_id == actor_id {
                let Some(def) = engine.catalog.get(&case_card.definition_id) else {
                    return false;
                };
                let desc = &def.description;
                let name = def.names.first().map(|s| s.as_str()).unwrap_or("");

                // 青の古城探索事件など：特定特徴やレベル条件のキャラ対象が必要な宣言の空撃ち防止
                if name.contains("青の古城") || desc.contains("少年探偵団") {
                    let has_valid_target = my_ps.field.iter().any(|fc| {
                        engine
                            .catalog
                            .get(&fc.definition_id)
                            .map(|d| d.traits.iter().any(|t| t.contains("少年探偵団")))
                            .unwrap_or(false)
                    }) && opp_ps.map(|o| !o.field.is_empty()).unwrap_or(false);
                    return has_valid_target;
                }

                // ブラックインパクト！（相手の手札リムーブ）：相手の手札が0枚なら空撃ちになるので使わない
                if name.contains("ブラックインパクト") || desc.contains("相手は手札") {
                    let opp_has_hand = opp_ps.map(|o| !o.hand.is_empty()).unwrap_or(false);
                    return opp_has_hand;
                }

                // 相手キャラ対象の効果（APマイナスやリムーブ等）なら相手フィールドにキャラが必要
                if desc.contains("相手の現場") || desc.contains("キャラを1枚") {
                    return opp_ps.map(|o| !o.field.is_empty()).unwrap_or(false);
                }
                return true;
            }
        }

        // フィールドキャラの宣言
        if let Some(fc) = my_ps.field.iter().find(|c| c.instance_id == actor_id) {
            let Some(def) = engine.catalog.get(&fc.definition_id) else {
                return false;
            };
            let desc = &def.description;
            let name = def.names.first().map(|s| s.as_str()).unwrap_or("");

            // ベルモット等：自分のキャラをリムーブして相手をリムーブする宣言は、相手の場にキャラがいて、
            // かつ自分の場に「復活可能なウォッカ」または「低レベルキャラ(Lv.4以下)」がいる時のみ発動
            if name.contains("ベルモット") && !name.contains("ジン") && desc.contains("リムーブ") {
                let opp_has_target = opp_ps.map(|o| !o.field.is_empty()).unwrap_or(false);
                let has_fodder = my_ps.field.iter().any(|c| {
                    c.instance_id != actor_id
                        && (engine.acted_this_turn_ids.contains(&c.instance_id)
                            || engine.catalog.get(&c.definition_id).map(|d| d.level <= 4).unwrap_or(false))
                });
                return opp_has_target && has_fodder;
            }

            // 江戸川コナン等の味方バフ宣言：「このキャラ以外の」条件がある場合、他に味方がいなければ空撃ちになる
            if desc.contains("このキャラ以外の") && my_ps.field.len() <= 1 {
                return false;
            }

            // 手札コストが必要な宣言は手札がある時のみ
            if desc.contains("手札を1枚リムーブ") && my_ps.hand.is_empty() {
                return false;
            }

            // 相手の現場を対象にとる宣言は相手の現場にキャラがいる時のみ
            if (desc.contains("相手の現場") || desc.contains("相手キャラ"))
                && !opp_ps.map(|o| !o.field.is_empty()).unwrap_or(false)
            {
                return false;
            }

            return true;
        }

        false
    }

    /// 盤面の公開情報と自分の手札を参照して戦略的に意思決定を行う
    pub fn choose_action_with_engine(&self, engine: &MatchEngine, decision: &Value) -> Value {
        if self.difficulty == CpuDifficulty::Easy {
            return self.choose_action(decision);
        }
        self.reset_turn_state_if_needed(engine.turn);
        let kind = decision.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        let opts = decision.get("options").cloned().unwrap_or(json!({}));

        let my_ps = match engine.players.get(&self.player_id) {
            Some(ps) => ps,
            None => return self.choose_action(decision),
        };
        let opp_id = engine
            .player_order
            .iter()
            .find(|pid| *pid != &self.player_id)
            .cloned()
            .unwrap_or_default();
        let opp_ps = engine.players.get(&opp_id);

        match kind {
            "mulligan" => {
                // 初動カード（Lv.1〜2キャラ、烏丸蓮耶の影、小さくなった名探偵、灰原哀Lv.4）を最大3枚までキープし、
                // Lv.5以上の高コストカードや後半用イベントは積極的に引き直す（勝者ログ平均3.7枚マリガン）
                let mut return_ids = vec![];
                let mut kept_early_cards = 0;
                let mut seen_names = HashSet::new();

                if let Some(hand_ids) = opts.get("handCardIds").and_then(|v| v.as_array()) {
                    for id_val in hand_ids {
                        if let Some(cid) = id_val.as_str() {
                            if let Some(c) = my_ps.hand.iter().find(|h| h.instance_id == cid) {
                                let def = engine.catalog.get(&c.definition_id);
                                let lvl = def.map(|d| d.level).unwrap_or(5);
                                let name = def
                                    .and_then(|d| d.names.first())
                                    .map(|s| s.as_str())
                                    .unwrap_or("");
                                let is_search_event = name.contains("烏丸蓮耶の影") || name.contains("小さくなった名探偵");
                                let is_haibara_lv4 = name.contains("灰原哀") && lvl <= 4;
                                let is_low_char = def.map(|d| d.card_type == CardType::CHARACTER).unwrap_or(false) && lvl <= 2;

                                // 同名カードの2枚目以降は序盤の動きを広げるためマリガン対象にする（烏丸蓮耶の影を除く）
                                let is_duplicate = !name.is_empty() && seen_names.contains(name) && !is_search_event;

                                if (is_search_event || is_low_char || is_haibara_lv4)
                                    && !is_duplicate
                                    && kept_early_cards < 3
                                {
                                    kept_early_cards += 1;
                                    if !name.is_empty() {
                                        seen_names.insert(name.to_string());
                                    }
                                } else {
                                    return_ids.push(json!(cid));
                                }
                            }
                        }
                    }
                }
                json!({"value": {"cardIds": return_ids}})
            }
            "mainAction" => {
                let empty_vec = vec![];
                let actions = opts.get("actions").and_then(|v| v.as_array()).unwrap_or(&empty_vec);
                let find_act = |t: &str| {
                    actions
                        .iter()
                        .find(|a| a.get("type").and_then(|v| v.as_str()) == Some(t))
                };

                // 1. 勝てるなら即座に事件解決！
                if find_act("RESOLVE_CASE").is_some() {
                    return json!({"value": {"type": "RESOLVE_CASE"}});
                }

                // 2. ターン開始時の ASSIST 先行化:
                // HAND_PLAY の前に ASSIST を宣言してFILEを増やす（【FILE5】不発防止 ＆ 高Lvカード解禁 ＆ 解決編移行）
                // ただし既にFILEが8枚以上あり、手札にLv.9（ジン＆ベルモット等）がない場合はパートナー推理（証拠+1）を優先
                let eff_file = my_ps.effective_file_count();
                if find_act("ASSIST").is_some() {
                    let has_lv9_in_hand = my_ps.hand.iter().any(|c| {
                        engine.catalog.get(&c.definition_id).map(|d| d.level >= 9).unwrap_or(false)
                    });
                    let should_assist_first = if self.difficulty == CpuDifficulty::Hard {
                        eff_file < 8 || (eff_file == 8 && has_lv9_in_hand) || find_act("DEDUCE").is_none()
                    } else {
                        my_ps.case_phase != CasePhase::RESOLUTION && eff_file >= 6
                    };
                    if should_assist_first {
                        return json!({"value": {"type": "ASSIST"}});
                    }
                }

                // 3. 手札登場・イベント使用: 学習済み重み(self.weights)とカード相性で評価
                if let Some(act) = find_act("HAND_PLAY") {
                    if let Some(card_ids) = act.get("cardIds").and_then(|v| v.as_array()) {
                        let mut best_cid: Option<&str> = None;
                        let mut best_score = i32::MIN;
                        let opp_field_count = opp_ps.map(|o| o.field.len()).unwrap_or(0);
                        let my_field_count = my_ps.field.len();

                        for cid_val in card_ids {
                            if let Some(cid) = cid_val.as_str() {
                                if let Some(c) = my_ps.hand.iter().find(|h| h.instance_id == cid) {
                                    if let Some(def) = engine.catalog.get(&c.definition_id) {
                                        let name = def.names.first().map(|s| s.as_str()).unwrap_or("");
                                        let desc = &def.description;

                                        let mut score_f = (def.level as f64) * self.weights.level_weight
                                            + (def.ap as f64) * self.weights.ap_weight
                                            + (def.lp as f64) * self.weights.lp_weight;

                                        if desc.contains("突撃") || desc.contains("迅速") {
                                            score_f += self.weights.rush_bonus;
                                        }
                                        if desc.contains("【登場時】") {
                                            score_f += self.weights.on_play_bonus;
                                        }

                                        // 【FILE5】〜【FILE7】条件を持つカードは条件未満なら大幅に優先度を下げる（効果不発防止）
                                        for req_f in [5usize, 6, 7] {
                                            let tag1 = format!("【FILE{}】", req_f);
                                            let tag2 = format!("【FILE {}】", req_f);
                                            if eff_file < req_f && (desc.contains(&tag1) || desc.contains(&tag2)) {
                                                score_f -= 4000.0;
                                            }
                                        }

                                        // ジン＆ベルモット等の「他のキャラが登場したとき」誘発持ちは最優先で先に出す
                                        if desc.contains("自分の現場に登場したとき") {
                                            score_f += 1800.0;
                                        }

                                        if def.card_type == CardType::EVENT {
                                            score_f += self.weights.event_penalty;

                                            // 初動サーチ・展開イベント（烏丸蓮耶の影、小さくなった名探偵、少年探偵団）は高評価
                                            if name.contains("烏丸蓮耶の影") || name.contains("小さくなった名探偵") {
                                                score_f += 3500.0;
                                            } else if name == "少年探偵団" {
                                                if !my_ps.hand.is_empty() && my_field_count < 4 {
                                                    score_f += 3200.0;
                                                } else {
                                                    score_f -= 4000.0;
                                                }
                                            } else if name.contains("五人囃子") || desc.contains("リムーブエリアから") {
                                                // 蘇生イベントはリムーブエリアに対象キャラがいないなら絶対に使わない（不発防止）
                                                let has_revivable = my_ps.remove.iter().any(|rc| {
                                                    engine
                                                        .catalog
                                                        .get(&rc.definition_id)
                                                        .map(|d| d.card_type == CardType::CHARACTER && d.level <= 4)
                                                        .unwrap_or(false)
                                                });
                                                if has_revivable && my_field_count < 4 {
                                                    score_f += 2000.0;
                                                } else {
                                                    score_f -= 8000.0;
                                                }
                                            } else if name == "カッ" || desc.contains("すべてをリムーブ") {
                                                // 全体除去「カッ」は相手の場が多く自分の場が少ない劣勢時のみ使う
                                                if opp_field_count >= 2 && opp_field_count > my_field_count {
                                                    score_f += 3500.0;
                                                } else {
                                                    score_f -= 6000.0;
                                                }
                                            } else if desc.contains("リムーブ") || desc.contains("スリープ") || desc.contains("スタン") {
                                                if opp_field_count == 0 {
                                                    score_f -= 6000.0;
                                                } else {
                                                    score_f += self.weights.removal_bonus;
                                                }
                                            }
                                        }

                                        let score = score_f as i32;
                                        if score > best_score {
                                            best_score = score;
                                            best_cid = Some(cid);
                                        }
                                    }
                                }
                            }
                        }
                        if let Some(cid) = best_cid {
                            if best_score > -1000 {
                                return json!({"value": {"type": "HAND_PLAY", "cardId": cid}});
                            }
                        }
                    }
                }

                // 4. パートナーやLP2キャラの推理（DEDUCE）を先に行う (Hardのみ)
                // ※ブラックインパクト！の宣言コスト（裏向き証拠3枚）やリーサル計算のため、推理で証拠を増やしてから宣言・攻撃に移る
                if self.difficulty == CpuDifficulty::Hard {
                    if let Some(ded_act) = find_act("DEDUCE") {
                        if let Some(actor_ids) = ded_act.get("actorIds").and_then(|v| v.as_array()) {
                            let mut best_priority_deducer: Option<(&str, i32, i32, i32)> = None;
                            for aid_v in actor_ids {
                                if let Some(aid) = aid_v.as_str() {
                                    let is_partner = my_ps.partner.as_ref().map(|p| p.instance_id == aid).unwrap_or(false);
                                    let (lp, ap) = if is_partner {
                                        let lp = my_ps
                                            .partner
                                            .as_ref()
                                            .and_then(|p| engine.catalog.get(&p.definition_id))
                                            .map(|d| d.lp)
                                            .unwrap_or(1);
                                        (lp, 0)
                                    } else if let Some(fc) = my_ps.field.iter().find(|c| c.instance_id == aid) {
                                        let lp = engine.get_effective_lp(&self.player_id, fc);
                                        let ap = engine.get_effective_ap(&self.player_id, fc);
                                        (lp, ap)
                                    } else {
                                        (1, 2000)
                                    };
                                    // パートナーまたは実効LP2以上ならACTIONより優先して推理させる
                                    if is_partner || lp >= 2 {
                                        let key = (if is_partner { 1 } else { 0 }, lp, -ap);
                                        let better = best_priority_deducer
                                            .map(|(_, bp, blp, bap)| key > (bp, blp, bap))
                                            .unwrap_or(true);
                                        if better {
                                            best_priority_deducer = Some((aid, key.0, key.1, key.2));
                                        }
                                    }
                                }
                            }
                            if let Some((aid, _, _, _)) = best_priority_deducer {
                                return json!({"value": {"type": "DEDUCE", "actorId": aid}});
                            }
                        }
                    }
                }

                // 5. 宣言効果 (DECLARE): 空撃ちにならない有効な宣言のみを発動
                if let Some(act) = find_act("DECLARE") {
                    if let Some(actors) = act.get("actors").and_then(|v| v.as_array()) {
                        for actor in actors {
                            if let Some(aid) = actor.get("actorId").and_then(|v| v.as_str()) {
                                if self.is_declare_beneficial(engine, aid, &opp_id) {
                                    return json!({"value": {"type": "DECLARE", "actorId": aid}});
                                }
                            }
                        }
                    }
                }

                // 6. 変装 (DISGUISE): 場のキャラより明らかに強い（またはスリープ済みキャラを起こせる）なら実行
                if let Some(act) = find_act("DISGUISE") {
                    let card_ids = act.get("cardIds").and_then(|v| v.as_array());
                    let target_ids = act.get("targetIds").and_then(|v| v.as_array());
                    if let (Some(cids), Some(tids)) = (card_ids, target_ids) {
                        let mut best_pair: Option<(&str, &str)> = None;
                        let mut best_gain = 0;
                        for cid_v in cids {
                            let cid = match cid_v.as_str() {
                                Some(s) => s,
                                None => continue,
                            };
                            let h_ap = my_ps
                                .hand
                                .iter()
                                .find(|c| c.instance_id == cid)
                                .and_then(|c| engine.catalog.get(&c.definition_id))
                                .map(|d| d.ap)
                                .unwrap_or(0);
                            for tid_v in tids {
                                let tid = match tid_v.as_str() {
                                    Some(s) => s,
                                    None => continue,
                                };
                                if let Some(fc) = my_ps.field.iter().find(|c| c.instance_id == tid) {
                                    let f_ap = engine.get_effective_ap(&self.player_id, fc);
                                    let bonus = if fc.orientation != CardOrientation::ACTIVE { 1500 } else { 0 };
                                    let gain = (h_ap - f_ap) + bonus;
                                    if gain > best_gain {
                                        best_gain = gain;
                                        best_pair = Some((cid, tid));
                                    }
                                }
                            }
                        }
                        if let Some((cid, tid)) = best_pair {
                            return json!({"value": {"type": "DISGUISE", "cardId": cid, "targetId": tid}});
                        }
                    }
                }

                // 7. 攻撃 (ACTION): 有利トレード（確実に相手を倒せる）または事件攻撃を評価
                if let Some(act) = find_act("ACTION") {
                    let actor_ids = act.get("actorIds").and_then(|v| v.as_array());
                    let target_ids = act.get("targetIds").and_then(|v| v.as_array());
                    let actor_target_map = act.get("actorTargetMap").and_then(|v| v.as_object());

                    let mut best_action: Option<(&str, &str)> = None;
                    let mut best_score = 0;

                    if let (Some(aids), Some(opp)) = (actor_ids, opp_ps) {
                        // 相手のアクティブなブロッカーの最大APを確認（AP1000がAP4000シェリー等に無謀突撃するのを防ぐ）
                        let max_opp_active_blocker_ap = opp
                            .field
                            .iter()
                            .filter(|c| c.orientation == CardOrientation::ACTIVE)
                            .map(|c| engine.get_effective_ap(&opp_id, c))
                            .max()
                            .unwrap_or(0);

                        for aid_v in aids {
                            let aid = match aid_v.as_str() {
                                Some(s) => s,
                                None => continue,
                            };
                            let my_card = match my_ps.field.iter().find(|c| c.instance_id == aid) {
                                Some(c) => c,
                                None => continue,
                            };
                            let my_ap = engine.get_effective_ap(&self.player_id, my_card);
                            let my_lp = engine.get_effective_lp(&self.player_id, my_card);
                            let can_deduce = find_act("DEDUCE")
                                .and_then(|d| d.get("actorIds"))
                                .and_then(|v| v.as_array())
                                .map(|arr| arr.iter().any(|x| x.as_str() == Some(aid)))
                                .unwrap_or(false);

                            let valid_tids = actor_target_map
                                .and_then(|m| m.get(aid))
                                .and_then(|v| v.as_array())
                                .or(target_ids);

                            if let Some(tids) = valid_tids {
                                for tid_v in tids {
                                    let tid = match tid_v.as_str() {
                                        Some(s) => s,
                                        None => continue,
                                    };
                                    if let Some(opp_c) = opp.field.iter().find(|c| c.instance_id == tid) {
                                        let opp_ap = engine.get_effective_ap(&opp_id, opp_c);
                                        let opp_lp = engine.get_effective_lp(&opp_id, opp_c);
                                        if my_ap >= opp_ap {
                                            // 相手の強キャラ（高AP・高LP）を倒せるなら最優先！
                                            let base_rem = (self.weights.attack_remove_weight * 3.5) as i32;
                                            let lp_threat = opp_lp * 1200;
                                            let score = base_rem + opp_ap + lp_threat - (my_ap - opp_ap) / 4;
                                            if score > best_score {
                                                best_score = score;
                                                best_action = Some((aid, tid));
                                            }
                                        }
                                    } else if opp.r#case.as_ref().map(|c| c.instance_id == tid).unwrap_or(false) {
                                        // ルール上、相手の証拠が1枚以上ある時のみ事件への攻撃が可能
                                        if !opp.evidence.is_empty() {
                                            // 相手に無傷でブロックされる（相手アクティブAP > 自分AP）かつ自分が推理可能なら、無駄死にせず推理に回す
                                            if max_opp_active_blocker_ap > my_ap && can_deduce {
                                                continue;
                                            }

                                            let urgency = if opp.case_phase == CasePhase::RESOLUTION { 2000 } else { 0 };
                                            let case_bonus = self.weights.case_attack_bonus as i32;
                                            // 推理できないターン（登場ターンの突撃やLP0キャラ）はペナルティなし
                                            let lp_penalty = if can_deduce {
                                                (my_lp as f64 * self.weights.deduce_lp_weight * 0.7) as i32
                                            } else {
                                                0
                                            };
                                            let score = 3800 + case_bonus + urgency - lp_penalty;
                                            if score > best_score {
                                                best_score = score;
                                                best_action = Some((aid, tid));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if let Some((aid, tid)) = best_action {
                        return json!({"value": {"type": "ACTION", "actorId": aid, "targetId": tid}});
                    }
                }

                // 8. 推理 (DEDUCE): 残ったアクティブキャラで確実に証拠を稼ぐ（相手の証拠が0枚の時もここで推理する）
                if let Some(act) = find_act("DEDUCE") {
                    if let Some(actor_ids) = act.get("actorIds").and_then(|v| v.as_array()) {
                        let mut best_aid: Option<&str> = None;
                        let mut best_lp = -1;
                        for aid_v in actor_ids {
                            if let Some(aid) = aid_v.as_str() {
                                let lp = if my_ps.partner.as_ref().map(|p| p.instance_id == aid).unwrap_or(false) {
                                    my_ps
                                        .partner
                                        .as_ref()
                                        .and_then(|p| engine.catalog.get(&p.definition_id))
                                        .map(|d| d.lp)
                                        .unwrap_or(1)
                                } else {
                                    my_ps
                                        .field
                                        .iter()
                                        .find(|c| c.instance_id == aid)
                                        .map(|c| engine.get_effective_lp(&self.player_id, c))
                                        .unwrap_or(0)
                                };
                                if lp > best_lp {
                                    best_lp = lp;
                                    best_aid = Some(aid);
                                }
                            }
                        }
                        if let Some(aid) = best_aid {
                            return json!({"value": {"type": "DEDUCE", "actorId": aid}});
                        }
                    }
                }

                // 9. まだアシストしていないなら実行
                if find_act("ASSIST").is_some() {
                    return json!({"value": {"type": "ASSIST"}});
                }

                // 10. ネクストヒント（HINT_TAKE）の厳選使用:
                // ヒントを使った結果（FILEが1枚減った状態 `eff_file - 1`）でも手札または獲得するFILEトップのカードを
                // 実際にプレイできるかを事前チェックし、無意味なヒント空撃ちを完全に防ぐ！
                if find_act("HINT_TAKE").is_some()
                    && !self.last_hint_skipped.load(Ordering::Relaxed)
                    && my_ps.field.len() < 5
                    && eff_file > 5
                {
                    let next_file_level = (eff_file - 1) as i32;
                    let top_file_playable = my_ps
                        .file
                        .last()
                        .and_then(|fc| engine.catalog.get(&fc.definition_id))
                        .map(|d| d.card_type == CardType::CHARACTER && d.level <= next_file_level)
                        .unwrap_or(false);
                    let hand_has_playable_char = my_ps.hand.iter().any(|hc| {
                        engine
                            .catalog
                            .get(&hc.definition_id)
                            .map(|d| d.card_type == CardType::CHARACTER && d.level <= next_file_level)
                            .unwrap_or(false)
                    });

                    let max_hints = if eff_file >= 7 { 2 } else { 1 };
                    let used = self.hints_used_this_turn.load(Ordering::Relaxed);
                    if used < max_hints && (hand_has_playable_char || top_file_playable) {
                        self.hints_used_this_turn.store(used + 1, Ordering::Relaxed);
                        return json!({"value": {"type": "HINT_TAKE"}});
                    }
                }

                json!({"value": {"type": "END_TURN"}})
            }
            "hintPlay" => {
                let eff_file = my_ps.effective_file_count();
                if let Some(playable) = opts.get("playableCardIds").and_then(|v| v.as_array()) {
                    let mut best_cid: Option<&str> = None;
                    let mut best_score = i32::MIN;
                    for cid_v in playable {
                        if let Some(cid) = cid_v.as_str() {
                            if let Some(c) = my_ps.hand.iter().find(|h| h.instance_id == cid) {
                                if let Some(def) = engine.catalog.get(&c.definition_id) {
                                    // イベントの空撃ちを防ぎ、突撃持ちや高レベルキャラを優先展開
                                    if def.card_type == CardType::EVENT {
                                        let name = def.names.first().map(|s| s.as_str()).unwrap_or("");
                                        if name.contains("五人囃子") && my_ps.remove.len() < 2 {
                                            continue;
                                        }
                                    }
                                    let mut score = def.level * 1000 + def.ap + def.lp * 400;
                                    if def.description.contains("突撃") {
                                        score += 1500;
                                    }
                                    if eff_file < 5
                                        && (def.description.contains("【FILE 5】") || def.description.contains("【FILE5】"))
                                    {
                                        score -= 3500;
                                    }
                                    if score > best_score {
                                        best_score = score;
                                        best_cid = Some(cid);
                                    }
                                }
                            }
                        }
                    }
                    if let Some(cid) = best_cid {
                        self.last_hint_skipped.store(false, Ordering::Relaxed);
                        return json!({"value": {"cardId": cid}});
                    }
                }
                self.last_hint_skipped.store(true, Ordering::Relaxed);
                json!({"value": {"cardId": null}})
            }
            "guard" => {
                // バトルの文脈（攻撃側AP、守備側AP、事件攻撃かどうか）を正確に取得
                let (attacker_ap, defender_ap, is_case_attack, i_am_attacker) = engine
                    .action_context
                    .as_ref()
                    .map(|ctx| {
                        let att_pid = ctx.get("attackerPlayerId").and_then(|v| v.as_str()).unwrap_or("");
                        let def_pid = ctx.get("defenderPlayerId").and_then(|v| v.as_str()).unwrap_or("");
                        let actor_id = ctx.get("actorId").and_then(|v| v.as_str()).unwrap_or("");
                        let target_id = ctx.get("targetId").and_then(|v| v.as_str()).unwrap_or("");

                        let att_ap = engine
                            .players
                            .get(att_pid)
                            .and_then(|ps| ps.field.iter().find(|c| c.instance_id == actor_id))
                            .map(|c| engine.get_effective_ap(att_pid, c))
                            .unwrap_or(3000);

                        let def_ap = engine
                            .players
                            .get(def_pid)
                            .and_then(|ps| ps.field.iter().find(|c| c.instance_id == target_id))
                            .map(|c| engine.get_effective_ap(def_pid, c))
                            .unwrap_or(0);

                        let is_case = my_ps.r#case.as_ref().map(|c| c.instance_id == target_id).unwrap_or(false);
                        (att_ap, def_ap, is_case, att_pid == self.player_id)
                    })
                    .unwrap_or((3000, 0, false, false));

                // 1. 変装があればAPが大きく上がる（または攻撃を耐えられる）場合に使用
                if let (Some(disguises), Some(target_id)) = (
                    opts.get("disguiseOptions").and_then(|v| v.as_array()),
                    opts.get("disguiseTargetId").and_then(|v| v.as_str()),
                ) {
                    if let Some(cur_card) = my_ps.field.iter().find(|c| c.instance_id == target_id) {
                        let cur_ap = engine.get_effective_ap(&self.player_id, cur_card);
                        for d_val in disguises {
                            if let (Some(did), Some(def_id)) = (
                                d_val.get("instanceId").and_then(|v| v.as_str()),
                                d_val.get("definitionId").and_then(|v| v.as_str()),
                            ) {
                                let new_ap = engine.catalog.get(def_id).map(|d| d.ap).unwrap_or(0);
                                if new_ap - cur_ap >= self.weights.disguise_ap_threshold || (cur_ap <= attacker_ap && new_ap > attacker_ap) {
                                    return json!({"value": {"disguiseCardId": did, "targetId": target_id}});
                                }
                            }
                        }
                    }
                }

                // 2. カットインの賢い判断（勝敗が覆る時・同APを突破する時のみ使用し、無駄撃ちを防ぐ）
                if let Some(cutins) = opts.get("cutinCardIds").and_then(|v| v.as_array()) {
                    if !cutins.is_empty() {
                        let need_cutin = if i_am_attacker {
                            // 自分が攻撃側：相手の守備APと同値以下で突破できない時に使う
                            defender_ap > 0 && attacker_ap < defender_ap
                        } else {
                            // 自分が守備側：自分のキャラが狙われていて負ける（かつ+2000〜3000で耐えられる）時に使う
                            !is_case_attack && defender_ap > 0 && attacker_ap >= defender_ap && (attacker_ap - defender_ap) <= 3000
                        };

                        if need_cutin {
                            if let Some(first_cutin) = cutins.first().and_then(|v| {
                                v.get("instanceId").and_then(|s| s.as_str()).or_else(|| v.as_str())
                            }) {
                                return json!({"value": {"cutinCardId": first_cutin}});
                            }
                        }
                    }
                }

                // 3. ガード: 相手APより高い（守備成功）か、AP同値（相打ち）でLP1以下のキャラでブロック！
                if let Some(guards) = opts.get("guardCardIds").and_then(|v| v.as_array()) {
                    let mut best_win_guard: Option<(&str, i32)> = None;
                    let mut best_trade_guard: Option<(&str, i32)> = None;
                    let mut weakest_guard: Option<(&str, i32)> = None;

                    for g_val in guards {
                        if let Some(gid) = g_val.as_str() {
                            if let Some(gc) = my_ps.field.iter().find(|c| c.instance_id == gid) {
                                let g_ap = engine.get_effective_ap(&self.player_id, gc);
                                let g_lp = engine.get_effective_lp(&self.player_id, gc);
                                // 守備成功条件: g_ap > attacker_ap
                                if g_ap > attacker_ap {
                                    if best_win_guard.map(|(_, b_ap)| g_ap < b_ap).unwrap_or(true) {
                                        best_win_guard = Some((gid, g_ap));
                                    }
                                } else if self.difficulty == CpuDifficulty::Hard && g_ap == attacker_ap && g_lp <= 1 {
                                    // 相打ちで相手アタッカーを道連れにできる（LP2主力は温存）
                                    if best_trade_guard.map(|(_, t_ap)| g_ap < t_ap).unwrap_or(true) {
                                        best_trade_guard = Some((gid, g_ap));
                                    }
                                }
                                if weakest_guard.map(|(_, w_ap)| g_ap < w_ap).unwrap_or(true) {
                                    weakest_guard = Some((gid, g_ap));
                                }
                            }
                        }
                    }

                    if let Some((gid, _)) = best_win_guard {
                        return json!({"value": {"guardCardId": gid}});
                    }
                    if let Some((gid, _)) = best_trade_guard {
                        return json!({"value": {"guardCardId": gid}});
                    }
                    // 相手がリーサル圏内（解決編で証拠5枚以上）または自分の証拠が残り1枚以下のピンチなら最弱キャラで守る
                    let opp_near_lethal = opp_ps
                        .map(|o| o.case_phase == CasePhase::RESOLUTION && o.evidence.len() >= 5)
                        .unwrap_or(false);
                    if is_case_attack && (my_ps.evidence.len() <= 1 || opp_near_lethal) {
                        if let Some((gid, _)) = weakest_guard {
                            return json!({"value": {"guardCardId": gid}});
                        }
                    }
                }
                json!({"value": {"guardCardId": null}})
            }
            "targetSelect" => {
                let actor_id = opts.get("actorId").and_then(|v| v.as_str()).unwrap_or("");
                let effect_type = opts.get("effectType").and_then(|v| v.as_str()).unwrap_or("");
                let optional = opts.get("optional").and_then(|v| v.as_bool()).unwrap_or(false);
                let max_select = opts.get("maxSelect").and_then(|v| v.as_u64()).unwrap_or(1).max(1) as usize;
                let distinct_names = opts.get("distinctNames").and_then(|v| v.as_bool()).unwrap_or(false);
                let distinct_levels = opts.get("distinctLevels").and_then(|v| v.as_bool()).unwrap_or(false);
                let distinct_colors = opts.get("distinctColors").and_then(|v| v.as_bool()).unwrap_or(false);
                let max_total_level = opts.get("maxTotalLevel").and_then(|v| v.as_i64());

                if let Some(candidates) = opts.get("candidates").and_then(|v| v.as_array()) {
                    // 自分のフィールドキャラをリムーブする任意効果（ジンのターン終了時効果など）で、
                    // 相手の場に倒すべきキャラがいない場合はスキップして味方を温存する
                    let all_cand_are_my_field = !candidates.is_empty()
                        && candidates.iter().all(|c| {
                            c.get("instanceId")
                                .and_then(|v| v.as_str())
                                .map(|iid| my_ps.field.iter().any(|fc| fc.instance_id == iid))
                                .unwrap_or(false)
                        });
                    if optional && effect_type == "REMOVE" && all_cand_are_my_field {
                        let opp_has_field = opp_ps.map(|o| !o.field.is_empty()).unwrap_or(false);
                        if !opp_has_field {
                            return json!({"value": {"skip": true, "actorId": actor_id, "effectType": effect_type}});
                        }
                    }

                    let mut scored: Vec<(&Value, &str, i32)> = candidates
                        .iter()
                        .filter_map(|cand| {
                            let tid = cand.get("instanceId").and_then(|v| v.as_str())?;
                            let ap = cand.get("ap").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                            let lvl = cand.get("level").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                            let name = cand.get("name").and_then(|v| v.as_str()).unwrap_or("");

                            let mut score = ap + lvl * 500;

                            if effect_type == "DISCARD_HAND" {
                                // 手札コスト：五人囃子や高レベルで出せない被りカード、低バリューカードを優先して捨てる（スコアが低い順）
                                if name.contains("五人囃子") || name.contains("沖野ヨーコ") {
                                    score -= 5000;
                                }
                                if name.contains("烏丸蓮耶の影") || name.contains("ジン") || name.contains("結成") {
                                    score += 4000;
                                }
                            } else if effect_type == "REMOVE" && all_cand_are_my_field {
                                // 自分のキャラをコストでリムーブする場合：復活効果を持つ行動済みウォッカを最優先で選ぶ！
                                if name.contains("ウォッカ") && engine.acted_this_turn_ids.contains(tid) {
                                    score = -10000;
                                }
                            }

                            Some((cand, tid, score))
                        })
                        .collect();

                    if effect_type == "DISCARD_HAND" || (effect_type == "REMOVE" && all_cand_are_my_field) {
                        scored.sort_by_key(|&(_, _, s)| s);
                    } else {
                        scored.sort_by(|a, b| b.2.cmp(&a.2));
                    }

                    let mut chosen_ids: Vec<&str> = Vec::new();
                    let mut used_names = HashSet::new();
                    let mut used_levels = HashSet::new();
                    let mut used_colors = HashSet::new();
                    let mut sum_level: i64 = 0;

                    for (c, iid, _) in scored {
                        if chosen_ids.len() >= max_select {
                            break;
                        }
                        let cname = c.get("name").and_then(|v| v.as_str()).unwrap_or("");
                        let clv = c.get("level").and_then(|v| v.as_i64()).unwrap_or(0);
                        let ccols: Vec<&str> = c
                            .get("colors")
                            .and_then(|v| v.as_array())
                            .map(|arr| arr.iter().filter_map(|x| x.as_str()).collect())
                            .unwrap_or_default();

                        if distinct_names && !cname.is_empty() && used_names.contains(cname) {
                            continue;
                        }
                        if distinct_levels && used_levels.contains(&clv) {
                            continue;
                        }
                        if distinct_colors && ccols.iter().any(|col| used_colors.contains(*col)) {
                            continue;
                        }
                        if let Some(cap) = max_total_level {
                            if sum_level + clv > cap {
                                continue;
                            }
                        }

                        if distinct_names && !cname.is_empty() {
                            used_names.insert(cname.to_string());
                        }
                        if distinct_levels {
                            used_levels.insert(clv);
                        }
                        if distinct_colors {
                            for col in ccols {
                                used_colors.insert(col.to_string());
                            }
                        }
                        sum_level += clv;
                        chosen_ids.push(iid);
                    }

                    if let Some(&first_id) = chosen_ids.first() {
                        return json!({
                            "value": {
                                "targetId": first_id,
                                "targetIds": chosen_ids,
                                "actorId": actor_id,
                                "effectType": effect_type
                            }
                        });
                    }
                }
                if optional {
                    return json!({"value": {"skip": true, "actorId": actor_id, "effectType": effect_type}});
                }
                json!({
                    "value": {
                        "targetId": null,
                        "targetIds": [],
                        "actorId": actor_id,
                        "effectType": effect_type
                    }
                })
            }
            "deckSearch" => {
                let actor_id = opts.get("actorId").and_then(|v| v.as_str()).unwrap_or("");
                let max_select = opts.get("maxSelect").and_then(|v| v.as_u64()).unwrap_or(1).max(1) as usize;
                let distinct_colors = opts.get("distinctColors").and_then(|v| v.as_bool()).unwrap_or(false);

                if let Some(cards) = opts.get("revealedCards").and_then(|v| v.as_array()) {
                    let eff_file = my_ps.effective_file_count() as i32;
                    let mut scored: Vec<(&Value, &str, i32)> = cards
                        .iter()
                        .filter(|c| c.get("selectable").and_then(|v| v.as_bool()).unwrap_or(true))
                        .filter_map(|c| {
                            let cid = c.get("instanceId").and_then(|v| v.as_str())?;
                            let lvl = c.get("level").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                            let ap = c.get("ap").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                            let playable_bonus = if lvl <= eff_file + 2 { 3000 } else { 0 };
                            Some((c, cid, playable_bonus + lvl * 500 + ap))
                        })
                        .collect();

                    scored.sort_by(|a, b| b.2.cmp(&a.2));

                    let mut chosen_ids: Vec<&str> = Vec::new();
                    let mut used_colors = HashSet::new();

                    for (c, cid, _) in scored {
                        if chosen_ids.len() >= max_select {
                            break;
                        }
                        let ccols: Vec<&str> = c
                            .get("colors")
                            .and_then(|v| v.as_array())
                            .map(|arr| arr.iter().filter_map(|x| x.as_str()).collect())
                            .unwrap_or_default();
                        if distinct_colors && ccols.iter().any(|col| used_colors.contains(*col)) {
                            continue;
                        }
                        if distinct_colors {
                            for col in ccols {
                                used_colors.insert(col.to_string());
                            }
                        }
                        chosen_ids.push(cid);
                    }

                    if let Some(&first_id) = chosen_ids.first() {
                        return json!({
                            "value": {
                                "chosenCardId": first_id,
                                "chosenCardIds": chosen_ids,
                                "actorId": actor_id
                            }
                        });
                    }
                }
                json!({
                    "value": {
                        "chosenCardId": null,
                        "chosenCardIds": [],
                        "actorId": actor_id
                    }
                })
            }
            _ => self.choose_action(decision),
        }
    }

    pub fn choose_action(&self, decision: &Value) -> Value {
        let kind = decision.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        let opts = decision.get("options").cloned().unwrap_or(json!({}));
        let mut rng = rand::thread_rng();

        match kind {
            "mulligan" => {
                let hand = opts.get("handCardIds").and_then(|v| v.as_array());
                if let Some(cards) = hand {
                    if !cards.is_empty() && rng.gen_bool(0.3) {
                        let chosen = cards.choose(&mut rng).unwrap();
                        return json!({"value": {"cardIds": [chosen]}});
                    }
                }
                json!({"value": {"cardIds": []}})
            }
            "mainAction" => {
                let empty_vec = vec![];
                let actions = opts.get("actions").and_then(|v| v.as_array()).unwrap_or(&empty_vec);
                let types: Vec<&str> = actions
                    .iter()
                    .filter_map(|a| a.get("type").and_then(|t| t.as_str()))
                    .collect();

                if types.contains(&"RESOLVE_CASE") {
                    return json!({"value": {"type": "RESOLVE_CASE"}});
                }
                if types.contains(&"HAND_PLAY") {
                    if let Some(act) = actions.iter().find(|a| a.get("type").and_then(|t| t.as_str()) == Some("HAND_PLAY")) {
                        if let Some(card_ids) = act.get("cardIds").and_then(|v| v.as_array()) {
                            if let Some(cid) = card_ids.choose(&mut rng) {
                                return json!({"value": {"type": "HAND_PLAY", "cardId": cid}});
                            }
                        }
                    }
                }
                if types.contains(&"DEDUCE") {
                    if let Some(act) = actions.iter().find(|a| a.get("type").and_then(|t| t.as_str()) == Some("DEDUCE")) {
                        if let Some(actor_ids) = act.get("actorIds").and_then(|v| v.as_array()) {
                            if let Some(aid) = actor_ids.choose(&mut rng) {
                                return json!({"value": {"type": "DEDUCE", "actorId": aid}});
                            }
                        }
                    }
                }
                if types.contains(&"ACTION") {
                    if let Some(act) = actions.iter().find(|a| a.get("type").and_then(|t| t.as_str()) == Some("ACTION")) {
                        let actor_ids = act.get("actorIds").and_then(|v| v.as_array());
                        let target_ids = act.get("targetIds").and_then(|v| v.as_array());
                        let actor_target_map = act.get("actorTargetMap").and_then(|v| v.as_object());
                        if let Some(aids) = actor_ids {
                            if let Some(aid_val) = aids.choose(&mut rng) {
                                let aid_str = aid_val.as_str().unwrap_or("");
                                let valid_tids = actor_target_map
                                    .and_then(|m| m.get(aid_str))
                                    .and_then(|v| v.as_array())
                                    .or(target_ids);
                                if let Some(tids) = valid_tids {
                                    if let Some(tid) = tids.choose(&mut rng) {
                                        return json!({"value": {"type": "ACTION", "actorId": aid_val, "targetId": tid}});
                                    }
                                }
                            }
                        }
                    }
                }
                if types.contains(&"DECLARE") && rng.gen_bool(0.7) {
                    if let Some(act) = actions.iter().find(|a| a.get("type").and_then(|t| t.as_str()) == Some("DECLARE")) {
                        if let Some(actors) = act.get("actors").and_then(|v| v.as_array()) {
                            if let Some(actor) = actors.choose(&mut rng) {
                                if let Some(aid) = actor.get("actorId").and_then(|v| v.as_str()) {
                                    return json!({"value": {"type": "DECLARE", "actorId": aid}});
                                }
                            }
                        }
                    }
                }
                if types.contains(&"DISGUISE") && rng.gen_bool(0.5) {
                    if let Some(act) = actions.iter().find(|a| a.get("type").and_then(|t| t.as_str()) == Some("DISGUISE")) {
                        let card_ids = act.get("cardIds").and_then(|v| v.as_array());
                        let target_ids = act.get("targetIds").and_then(|v| v.as_array());
                        if let (Some(cids), Some(tids)) = (card_ids, target_ids) {
                            if let (Some(cid), Some(tid)) = (cids.choose(&mut rng), tids.choose(&mut rng)) {
                                return json!({"value": {"type": "DISGUISE", "cardId": cid, "targetId": tid}});
                            }
                        }
                    }
                }
                if types.contains(&"ASSIST") && rng.gen_bool(0.5) {
                    return json!({"value": {"type": "ASSIST"}});
                }
                if types.contains(&"HINT_TAKE") && rng.gen_bool(0.3) {
                    return json!({"value": {"type": "HINT_TAKE"}});
                }

                json!({"value": {"type": "END_TURN"}})
            }
            "hintPlay" => {
                if let Some(playable) = opts.get("playableCardIds").and_then(|v| v.as_array()) {
                    if let Some(cid) = playable.choose(&mut rng) {
                        return json!({"value": {"cardId": cid}});
                    }
                }
                json!({"value": {"cardId": null}})
            }
            "guard" => {
                let must_guard = opts.get("mustGuard").and_then(|v| v.as_bool()).unwrap_or(false);
                if let Some(guards) = opts.get("guardCardIds").and_then(|v| v.as_array()) {
                    if !guards.is_empty() && (must_guard || rng.gen_bool(0.5)) {
                        let chosen = guards.choose(&mut rng).unwrap();
                        return json!({"value": {"guardCardId": chosen}});
                    }
                }
                json!({"value": {"guardCardId": null}})
            }
            "targetSelect" => {
                let actor_id = opts.get("actorId").and_then(|v| v.as_str()).unwrap_or("");
                let effect_type = opts.get("effectType").and_then(|v| v.as_str()).unwrap_or("");
                let max_select = opts.get("maxSelect").and_then(|v| v.as_u64()).unwrap_or(1).max(1) as usize;
                let distinct_names = opts.get("distinctNames").and_then(|v| v.as_bool()).unwrap_or(false);
                let distinct_levels = opts.get("distinctLevels").and_then(|v| v.as_bool()).unwrap_or(false);
                let distinct_colors = opts.get("distinctColors").and_then(|v| v.as_bool()).unwrap_or(false);
                let max_total_level = opts.get("maxTotalLevel").and_then(|v| v.as_i64());

                if let Some(candidates) = opts.get("candidates").and_then(|v| v.as_array()) {
                    let mut shuffled: Vec<&Value> = candidates.iter().collect();
                    shuffled.shuffle(&mut rng);

                    let mut chosen_ids: Vec<&str> = Vec::new();
                    let mut used_names = HashSet::new();
                    let mut used_levels = HashSet::new();
                    let mut used_colors = HashSet::new();
                    let mut sum_level: i64 = 0;

                    for c in shuffled {
                        if chosen_ids.len() >= max_select {
                            break;
                        }
                        let Some(iid) = c.get("instanceId").and_then(|v| v.as_str()) else {
                            continue;
                        };
                        let cname = c.get("name").and_then(|v| v.as_str()).unwrap_or("");
                        let clv = c.get("level").and_then(|v| v.as_i64()).unwrap_or(0);
                        let ccols: Vec<&str> = c
                            .get("colors")
                            .and_then(|v| v.as_array())
                            .map(|arr| arr.iter().filter_map(|x| x.as_str()).collect())
                            .unwrap_or_default();

                        if distinct_names && used_names.contains(cname) {
                            continue;
                        }
                        if distinct_levels && used_levels.contains(&clv) {
                            continue;
                        }
                        if distinct_colors && ccols.iter().any(|col| used_colors.contains(*col)) {
                            continue;
                        }
                        if let Some(cap) = max_total_level {
                            if sum_level + clv > cap {
                                continue;
                            }
                        }

                        if distinct_names {
                            used_names.insert(cname.to_string());
                        }
                        if distinct_levels {
                            used_levels.insert(clv);
                        }
                        if distinct_colors {
                            for col in ccols {
                                used_colors.insert(col.to_string());
                            }
                        }
                        sum_level += clv;
                        chosen_ids.push(iid);
                    }
                    if let Some(&first_id) = chosen_ids.first() {
                        return json!({
                            "value": {
                                "targetId": first_id,
                                "targetIds": chosen_ids,
                                "actorId": actor_id,
                                "effectType": effect_type
                            }
                        });
                    }
                }
                json!({
                    "value": {
                        "targetId": null,
                        "targetIds": [],
                        "actorId": actor_id,
                        "effectType": effect_type
                    }
                })
            }
            "deckSearch" => {
                let actor_id = opts.get("actorId").and_then(|v| v.as_str()).unwrap_or("");
                let max_select = opts.get("maxSelect").and_then(|v| v.as_u64()).unwrap_or(1).max(1) as usize;
                let distinct_colors = opts.get("distinctColors").and_then(|v| v.as_bool()).unwrap_or(false);
                if let Some(cards) = opts.get("revealedCards").and_then(|v| v.as_array()) {
                    let mut selectable: Vec<&Value> = cards
                        .iter()
                        .filter(|c| c.get("selectable").and_then(|v| v.as_bool()).unwrap_or(true))
                        .collect();
                    selectable.shuffle(&mut rng);
                    let mut chosen_ids: Vec<&str> = Vec::new();
                    let mut used_colors = HashSet::new();
                    for c in selectable {
                        if chosen_ids.len() >= max_select {
                            break;
                        }
                        let Some(cid) = c.get("instanceId").and_then(|v| v.as_str()) else {
                            continue;
                        };
                        let ccols: Vec<&str> = c
                            .get("colors")
                            .and_then(|v| v.as_array())
                            .map(|arr| arr.iter().filter_map(|x| x.as_str()).collect())
                            .unwrap_or_default();
                        if distinct_colors && ccols.iter().any(|col| used_colors.contains(*col)) {
                            continue;
                        }
                        if distinct_colors {
                            for col in ccols {
                                used_colors.insert(col.to_string());
                            }
                        }
                        chosen_ids.push(cid);
                    }
                    if let Some(&first_id) = chosen_ids.first() {
                        return json!({
                            "value": {
                                "chosenCardId": first_id,
                                "chosenCardIds": chosen_ids,
                                "actorId": actor_id
                            }
                        });
                    }
                }
                json!({
                    "value": {
                        "chosenCardId": null,
                        "chosenCardIds": [],
                        "actorId": actor_id
                    }
                })
            }
            "modeSelect" => {
                let modes_len = opts.get("modes").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
                let min_pick = opts.get("minPick").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
                let max_pick = opts.get("maxPick").and_then(|v| v.as_u64()).unwrap_or(1).max(1) as usize;
                let pick_count = min_pick.max(1).min(max_pick).min(modes_len);
                let mut indices: Vec<usize> = (0..modes_len).collect();
                indices.shuffle(&mut rng);
                let chosen: Vec<usize> = indices.into_iter().take(pick_count).collect();
                let first_idx = chosen.first().copied().unwrap_or(0);
                json!({
                    "value": {
                        "mode": first_idx,
                        "modeIndex": first_idx,
                        "modes": chosen
                    }
                })
            }
            _ => json!({"value": {}}),
        }
    }
}

/// 2つの重みセットで1試合シミュレーションし、(勝者, p1証拠 - p2証拠, 勝者の場・リムーブに出たカード定義ID, 敗者の終了時手札に残ったカード定義ID) を返す
fn simulate_match(
    catalog: &CardCatalog,
    p1_deck: &DeckDefinition,
    p2_deck: &DeckDefinition,
    w1: &CpuWeights,
    w2: &CpuWeights,
) -> (Option<String>, i32, Vec<String>, Vec<String>) {
    let mut engine = MatchEngine::new(catalog.clone());
    engine.add_player("p1", p1_deck);
    engine.add_player("p2", p2_deck);
    engine.start_match();

    let a1 = RandomAgent::with_weights("p1", w1.clone());
    let a2 = RandomAgent::with_weights("p2", w2.clone());

    for _ in 0..800 {
        if engine.winner_id.is_some() {
            break;
        }
        let Some(dec) = engine.pending_decision.clone() else {
            break;
        };
        let pid = dec.get("playerId").and_then(|v| v.as_str()).unwrap_or("");
        let ans = if pid == "p1" {
            a1.choose_action_with_engine(&engine, &dec)
        } else if pid == "p2" {
            a2.choose_action_with_engine(&engine, &dec)
        } else {
            break;
        };
        if engine.submit_decision(pid, &ans).is_err() {
            break;
        }
    }

    let p1_ev = engine.players.get("p1").map(|p| p.evidence.len() as i32).unwrap_or(0);
    let p2_ev = engine.players.get("p2").map(|p| p.evidence.len() as i32).unwrap_or(0);

    let mut winner_active_cards = Vec::new();
    let mut loser_stranded_hand = Vec::new();
    if let Some(ref w_id) = engine.winner_id {
        let l_id = if w_id == "p1" { "p2" } else { "p1" };
        if let Some(w_ps) = engine.players.get(w_id) {
            for c in w_ps.field.iter().chain(w_ps.remove.iter()) {
                winner_active_cards.push(c.definition_id.clone());
            }
        }
        if let Some(l_ps) = engine.players.get(l_id) {
            for c in &l_ps.hand {
                loser_stranded_hand.push(c.definition_id.clone());
            }
        }
    }

    (engine.winner_id, p1_ev - p2_ev, winner_active_cards, loser_stranded_hand)
}

/// 自己対戦（Evolution Strategies）でCPUの重みを学習し、`data/cpu_weights.json` に保存するとともにカード別強さ統計も出力する
pub fn train_cpu_self_play(
    catalog: &CardCatalog,
    p1_deck: &DeckDefinition,
    p2_deck: &DeckDefinition,
    generations: usize,
    games_per_gen: usize,
    save_path: &str,
) -> CpuWeights {
    let mut best = CpuWeights::load_or_default(save_path);
    let mut rng = rand::thread_rng();

    let mut win_card_counts: HashMap<String, usize> = HashMap::new();
    let mut stranded_hand_counts: HashMap<String, usize> = HashMap::new();

    println!(
        "🧠 CPU自己対戦学習を開始します (世代数: {}, 1世代あたり試合数: {}, 保存先: {})",
        generations, games_per_gen, save_path
    );

    for gen in 1..=generations {
        let mut cand = best.clone();
        let jitter = |rng: &mut rand::rngs::ThreadRng, v: f64, scale: f64| -> f64 {
            v + rng.gen_range(-scale..=scale)
        };
        cand.level_weight = jitter(&mut rng, cand.level_weight, 180.0).max(100.0);
        cand.ap_weight = jitter(&mut rng, cand.ap_weight, 0.35).max(0.1);
        cand.lp_weight = jitter(&mut rng, cand.lp_weight, 150.0).max(0.0);
        cand.rush_bonus = jitter(&mut rng, cand.rush_bonus, 250.0).max(0.0);
        cand.on_play_bonus = jitter(&mut rng, cand.on_play_bonus, 200.0).max(0.0);
        cand.event_penalty = jitter(&mut rng, cand.event_penalty, 200.0);
        cand.removal_bonus = jitter(&mut rng, cand.removal_bonus, 250.0).max(0.0);
        cand.deduce_lp_weight = jitter(&mut rng, cand.deduce_lp_weight, 200.0).max(100.0);
        cand.attack_remove_weight = jitter(&mut rng, cand.attack_remove_weight, 250.0).max(200.0);
        cand.case_attack_bonus = jitter(&mut rng, cand.case_attack_bonus, 200.0).max(0.0);
        cand.disguise_ap_threshold = (cand.disguise_ap_threshold + rng.gen_range(-400..=400)).max(0);

        let mut cand_wins = 0usize;
        let mut best_wins = 0usize;
        let mut ev_diff_sum = 0i32;

        for g in 0..games_per_gen {
            let cand_is_p1 = g % 2 == 0;
            let (winner, diff, win_cards, lose_hand) = if cand_is_p1 {
                simulate_match(catalog, p1_deck, p2_deck, &cand, &best)
            } else {
                simulate_match(catalog, p1_deck, p2_deck, &best, &cand)
            };

            for cid in win_cards {
                *win_card_counts.entry(cid).or_insert(0) += 1;
            }
            for cid in lose_hand {
                *stranded_hand_counts.entry(cid).or_insert(0) += 1;
            }

            let signed_diff = if cand_is_p1 { diff } else { -diff };
            ev_diff_sum += signed_diff;
            match winner.as_deref() {
                Some("p1") if cand_is_p1 => cand_wins += 1,
                Some("p2") if !cand_is_p1 => cand_wins += 1,
                Some("p1") | Some("p2") => best_wins += 1,
                _ => {}
            }
        }

        let improved = cand_wins > best_wins || (cand_wins == best_wins && ev_diff_sum > 0);
        if improved {
            best = cand;
            let _ = best.save(save_path);
            println!(
                "  [世代 {}/{}] ✅ 更新! 候補 {}勝 vs 現行 {}勝 (証拠差計: {:+}) -> 保存完了",
                gen, generations, cand_wins, best_wins, ev_diff_sum
            );
        } else {
            println!(
                "  [世代 {}/{}] ── 据置 (候補 {}勝 vs 現行 {}勝, 証拠差計: {:+})",
                gen, generations, cand_wins, best_wins, ev_diff_sum
            );
        }
    }

    // 学習完了時にデッキ内の「勝ち貢献カードTOP5」と「敗北時に手札で腐っていたカードTOP5」を表示
    let mut top_win: Vec<_> = win_card_counts.into_iter().collect();
    top_win.sort_by(|a, b| b.1.cmp(&a.1));
    println!("\n📊 【構築分析】勝利時に場・リムーブで活躍したカード TOP5:");
    for (cid, cnt) in top_win.into_iter().take(5) {
        let name = catalog
            .get(&cid)
            .and_then(|d| d.names.first())
            .map(|s| s.as_str())
            .unwrap_or(&cid);
        println!("   - {} ({}): {}回", name, cid, cnt);
    }

    let mut top_stranded: Vec<_> = stranded_hand_counts.into_iter().collect();
    top_stranded.sort_by(|a, b| b.1.cmp(&a.1));
    println!("⚠️ 【構築分析】敗北時に手札で腐っていた（使えなかった）カード TOP5:");
    for (cid, cnt) in top_stranded.into_iter().take(5) {
        let name = catalog
            .get(&cid)
            .and_then(|d| d.names.first())
            .map(|s| s.as_str())
            .unwrap_or(&cid);
        println!("   - {} ({}): {}回", name, cid, cnt);
    }

    recommend_tech_cards(catalog, p1_deck, p2_deck);

    best
}

/// 環境デッキ（P2）の傾向を分析し、P1デッキ外の有効な対策カード（テックカード）を推薦する
fn recommend_tech_cards(
    catalog: &CardCatalog,
    p1_deck: &DeckDefinition,
    p2_deck: &DeckDefinition,
) {
    let p1_colors: HashSet<String> = catalog
        .get(&p1_deck.partner_id)
        .map(|d| d.colors.iter().cloned().collect())
        .unwrap_or_default();

    let mut p1_canonical_ids: HashSet<String> = HashSet::new();
    for item in &p1_deck.main_deck {
        if let Some(def) = catalog.get(&item.definition_id) {
            p1_canonical_ids.insert(def.canonical_id.clone());
        } else {
            p1_canonical_ids.insert(item.definition_id.clone());
        }
    }

    // 環境デッキ（P2）の傾向を分析
    let mut p2_high_lv = 0usize;
    let mut p2_rush = 0usize;
    let mut p2_removal = 0usize;
    let mut p2_events = 0usize;
    for item in &p2_deck.main_deck {
        if let Some(def) = catalog.get(&item.definition_id) {
            let cnt = item.count.max(0) as usize;
            if def.level >= 6 {
                p2_high_lv += cnt;
            }
            if def.description.contains("突撃") {
                p2_rush += cnt;
            }
            if def.description.contains("リムーブ") || def.description.contains("スタン") || def.description.contains("手札に戻す") {
                p2_removal += cnt;
            }
            if def.card_type == CardType::EVENT {
                p2_events += cnt;
            }
        }
    }

    let mut seen_canonical: HashSet<String> = HashSet::new();
    let mut candidates: Vec<(i32, String, String, String, Vec<String>)> = Vec::new();

    for (id, def) in &catalog.definitions {
        if def.card_type != CardType::CHARACTER && def.card_type != CardType::EVENT {
            continue;
        }
        if p1_canonical_ids.contains(&def.canonical_id) || !seen_canonical.insert(def.canonical_id.clone()) {
            continue;
        }
        // P1パートナーと同じ色（または無色）のカードのみ候補にする
        let color_ok = p1_colors.is_empty()
            || def.colors.is_empty()
            || def.colors.iter().any(|c| p1_colors.contains(c));
        if !color_ok {
            continue;
        }

        let mut score: i32 = 0;
        let mut reasons: Vec<String> = Vec::new();
        let desc = &def.description;
        let lv = def.level;
        let ap = def.ap;
        let lp = def.lp;

        // 除去・妨害
        if desc.contains("リムーブ") || desc.contains("デッキの下") {
            let bonus = if p2_high_lv >= 8 { 45 } else { 30 };
            score += bonus;
            reasons.push("高Lv・脅威キャラの除去".to_string());
        } else if desc.contains("スタン") || desc.contains("手札に戻す") {
            score += 25;
            reasons.push("盤面テンポ阻害（スタン/バウンス）".to_string());
        }

        // 突撃・証拠レース
        if desc.contains("突撃") {
            let bonus = if p2_removal >= 6 { 35 } else { 25 };
            score += bonus;
            reasons.push("突撃による即時打点・証拠奪取".to_string());
        }

        // ドロー・リソース回復
        if desc.contains("カードを引く") || desc.contains("1枚引く") || desc.contains("2枚引く") || desc.contains("手札に加える") {
            score += 25;
            reasons.push("手札補充・安定化".to_string());
        }

        // 防御・カットイン
        if desc.contains("カットイン") || desc.contains("アタックできない") {
            let bonus = if p2_rush >= 4 { 35 } else { 20 };
            score += bonus;
            reasons.push("相手の速攻・アタック対策".to_string());
        }

        // ステータス効率（軽〜中コスト帯の優秀な打点・LP）
        if def.card_type == CardType::CHARACTER {
            if lv <= 4 && lp >= 2 {
                score += 20;
                reasons.push(format!("軽量高LP(Lv{}/LP{})", lv, lp));
            }
            if lv >= 5 && ap >= 6000 {
                score += 15;
            }
        }

        if score >= 35 && !reasons.is_empty() {
            let name = def.names.first().map(|s| s.as_str()).unwrap_or(id).to_string();
            let lv_str = if lv > 0 { format!("Lv{}", lv) } else { "Event".to_string() };
            candidates.push((score, name, id.clone(), lv_str, reasons));
        }
    }

    candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.2.cmp(&b.2)));

    println!("\n💡 【環境対策】相手デッキ（{}）に有効そうなデッキ外カード推薦 TOP5:", p2_deck.deck_name.as_deref().unwrap_or("名称未設定"));
    println!(
        "   (相手デッキ傾向: Lv6以上={}枚, 突撃={}枚, 除去={}枚, イベント={}枚)",
        p2_high_lv, p2_rush, p2_removal, p2_events
    );
    for (score, name, id, lv_str, reasons) in candidates.into_iter().take(5) {
        println!("   - {} ({} / {}) [評価:{}]: {}", name, id, lv_str, score, reasons.join("・"));
    }
}
