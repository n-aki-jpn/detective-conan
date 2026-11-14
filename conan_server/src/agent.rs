use crate::catalog::CardCatalog;
use crate::engine::MatchEngine;
use crate::models::{CardOrientation, CardType, CasePhase, DeckDefinition};
use rand::seq::SliceRandom;
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

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
            level_weight: 1000.0,
            ap_weight: 1.0,
            lp_weight: 500.0,
            rush_bonus: 1200.0,
            on_play_bonus: 800.0,
            event_penalty: -400.0,
            removal_bonus: 700.0,
            assist_min_field: 2,
            deduce_lp_weight: 1000.0,
            attack_remove_weight: 1200.0,
            case_attack_bonus: 800.0,
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

#[derive(Debug, Clone)]
pub struct RandomAgent {
    pub player_id: String,
    pub weights: CpuWeights,
}

impl RandomAgent {
    pub fn new(player_id: &str) -> Self {
        Self {
            player_id: player_id.to_string(),
            weights: CpuWeights::load_or_default("data/cpu_weights.json"),
        }
    }

    pub fn with_weights(player_id: &str, weights: CpuWeights) -> Self {
        Self {
            player_id: player_id.to_string(),
            weights,
        }
    }

    /// 盤面の公開情報と自分の手札を参照して戦略的に意思決定を行う
    pub fn choose_action_with_engine(&self, engine: &MatchEngine, decision: &Value) -> Value {
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
                // Lv4以上の重いカードは戻して序盤の展開札を探す
                let mut return_ids = vec![];
                if let Some(hand_ids) = opts.get("handCardIds").and_then(|v| v.as_array()) {
                    for id_val in hand_ids {
                        if let Some(cid) = id_val.as_str() {
                            if let Some(c) = my_ps.hand.iter().find(|h| h.instance_id == cid) {
                                let lvl = engine.catalog.get(&c.definition_id).map(|d| d.level).unwrap_or(0);
                                if lvl >= 4 {
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

                // 2. パートナーアシスト: FILE 6枚以上なら即解決編突入のため最優先、それ以外でも中盤なら積極的にFILEへ
                if find_act("ASSIST").is_some() && my_ps.case_phase != CasePhase::RESOLUTION {
                    let eff_file = my_ps.effective_file_count();
                    if eff_file >= 6 || (eff_file >= 4 && my_ps.field.len() >= self.weights.assist_min_field) {
                        return json!({"value": {"type": "ASSIST"}});
                    }
                }

                // 3. 手札登場・イベント使用: 学習済み重み(self.weights)で評価
                if let Some(act) = find_act("HAND_PLAY") {
                    if let Some(card_ids) = act.get("cardIds").and_then(|v| v.as_array()) {
                        let mut best_cid: Option<&str> = None;
                        let mut best_score = i32::MIN;
                        for cid_val in card_ids {
                            if let Some(cid) = cid_val.as_str() {
                                if let Some(c) = my_ps.hand.iter().find(|h| h.instance_id == cid) {
                                    if let Some(def) = engine.catalog.get(&c.definition_id) {
                                        let mut score_f = (def.level as f64) * self.weights.level_weight
                                            + (def.ap as f64) * self.weights.ap_weight
                                            + (def.lp as f64) * self.weights.lp_weight;
                                        if def.description.contains("突撃") {
                                            score_f += self.weights.rush_bonus;
                                        }
                                        if def.description.contains("【登場時】") {
                                            score_f += self.weights.on_play_bonus;
                                        }
                                        if def.card_type == CardType::EVENT {
                                            score_f += self.weights.event_penalty;
                                            if def.description.contains("リムーブ") || def.description.contains("スタン") {
                                                let opp_has_field = opp_ps.map(|o| !o.field.is_empty()).unwrap_or(false);
                                                if !opp_has_field {
                                                    score_f -= 5000.0;
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

                // 4. 宣言効果 (DECLARE): 攻撃・推理前に強化/妨害を積極発動
                if let Some(act) = find_act("DECLARE") {
                    if let Some(actors) = act.get("actors").and_then(|v| v.as_array()) {
                        if let Some(first_actor) = actors.first() {
                            if let Some(aid) = first_actor.get("actorId").and_then(|v| v.as_str()) {
                                return json!({"value": {"type": "DECLARE", "actorId": aid}});
                            }
                        }
                    }
                }

                // 5. 変装 (DISGUISE): 場のキャラより明らかに強い（APが高い、またはスリープ済みキャラを入れ替えられる）なら実行
                if let Some(act) = find_act("DISGUISE") {
                    let card_ids = act.get("cardIds").and_then(|v| v.as_array());
                    let target_ids = act.get("targetIds").and_then(|v| v.as_array());
                    if let (Some(cids), Some(tids)) = (card_ids, target_ids) {
                        let mut best_pair: Option<(&str, &str)> = None;
                        let mut best_gain = 0;
                        for cid_v in cids {
                            let cid = match cid_v.as_str() { Some(s) => s, None => continue };
                            let h_ap = my_ps.hand.iter().find(|c| c.instance_id == cid)
                                .and_then(|c| engine.catalog.get(&c.definition_id))
                                .map(|d| d.ap)
                                .unwrap_or(0);
                            for tid_v in tids {
                                let tid = match tid_v.as_str() { Some(s) => s, None => continue };
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

                // 6. ネクストヒント: 展開余地がありFILEが十分あるなら使用
                if find_act("HINT_TAKE").is_some()
                    && my_ps.field.len() < 5
                    && my_ps.effective_file_count() >= 4
                    && my_ps.hand.len() <= 4
                {
                    return json!({"value": {"type": "HINT_TAKE"}});
                }

                // 7. 攻撃 (ACTION): 有利トレード（確実に相手を倒せる）または事件攻撃を評価
                if let Some(act) = find_act("ACTION") {
                    let actor_ids = act.get("actorIds").and_then(|v| v.as_array());
                    let target_ids = act.get("targetIds").and_then(|v| v.as_array());
                    let actor_target_map = act.get("actorTargetMap").and_then(|v| v.as_object());

                    let mut best_action: Option<(&str, &str)> = None;
                    let mut best_score = 0; // 0より大きい有利アクションのみ実行（無謀な特攻はしない）

                    if let (Some(aids), Some(opp)) = (actor_ids, opp_ps) {
                        for aid_v in aids {
                            let aid = match aid_v.as_str() { Some(s) => s, None => continue };
                            let my_card = match my_ps.field.iter().find(|c| c.instance_id == aid) {
                                Some(c) => c,
                                None => continue,
                            };
                            let my_ap = engine.get_effective_ap(&self.player_id, my_card);
                            let my_lp = engine.catalog.get(&my_card.definition_id).map(|d| d.lp).unwrap_or(0);

                            let valid_tids = actor_target_map
                                .and_then(|m| m.get(aid))
                                .and_then(|v| v.as_array())
                                .or(target_ids);

                            if let Some(tids) = valid_tids {
                                for tid_v in tids {
                                    let tid = match tid_v.as_str() { Some(s) => s, None => continue };
                                    if let Some(opp_c) = opp.field.iter().find(|c| c.instance_id == tid) {
                                        let opp_ap = engine.get_effective_ap(&opp_id, opp_c);
                                        if my_ap >= opp_ap {
                                            // 倒せる相手！相手APが高いほど高評価（無駄にオーバーキルしないよう自APが低い方を優先）
                                            let score = 5000 + opp_ap - (my_ap - opp_ap) / 4;
                                            if score > best_score {
                                                best_score = score;
                                                best_action = Some((aid, tid));
                                            }
                                        }
                                    } else if opp.r#case.as_ref().map(|c| c.instance_id == tid).unwrap_or(false) {
                                        // 事件攻撃: 相手の証拠があり、LPが低い（推理より攻撃の方が価値が高い）または相手が解決編で危険なとき
                                        if !opp.evidence.is_empty() {
                                            let urgency = if opp.case_phase == CasePhase::RESOLUTION { 2000 } else { 0 };
                                            let lp_penalty = my_lp * 1200;
                                            let score = 3500 + urgency - lp_penalty;
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

                // 8. 推理 (DEDUCE): LPが高いキャラ・パートナーを優先して証拠を稼ぐ
                if let Some(act) = find_act("DEDUCE") {
                    if let Some(actor_ids) = act.get("actorIds").and_then(|v| v.as_array()) {
                        let mut best_aid: Option<&str> = None;
                        let mut best_lp = -1;
                        for aid_v in actor_ids {
                            if let Some(aid) = aid_v.as_str() {
                                let lp = if my_ps.partner.as_ref().map(|p| p.instance_id == aid).unwrap_or(false) {
                                    my_ps.partner.as_ref()
                                        .and_then(|p| engine.catalog.get(&p.definition_id))
                                        .map(|d| d.lp)
                                        .unwrap_or(1)
                                } else {
                                    my_ps.field.iter().find(|c| c.instance_id == aid)
                                        .and_then(|c| engine.catalog.get(&c.definition_id))
                                        .map(|d| d.lp)
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

                json!({"value": {"type": "END_TURN"}})
            }
            "hintPlay" => {
                if let Some(playable) = opts.get("playableCardIds").and_then(|v| v.as_array()) {
                    let mut best_cid: Option<&str> = None;
                    let mut best_score = i32::MIN;
                    for cid_v in playable {
                        if let Some(cid) = cid_v.as_str() {
                            if let Some(c) = my_ps.hand.iter().find(|h| h.instance_id == cid) {
                                if let Some(def) = engine.catalog.get(&c.definition_id) {
                                    let score = def.level * 1000 + def.ap;
                                    if score > best_score {
                                        best_score = score;
                                        best_cid = Some(cid);
                                    }
                                }
                            }
                        }
                    }
                    if let Some(cid) = best_cid {
                        return json!({"value": {"cardId": cid}});
                    }
                }
                json!({"value": {"cardId": null}})
            }
            "guard" => {
                // 攻撃側APを取得
                let attacker_ap = engine.action_context.as_ref().and_then(|ctx| {
                    let att_pid = ctx.get("attackerPlayerId").and_then(|v| v.as_str())?;
                    let actor_id = ctx.get("actorId").and_then(|v| v.as_str())?;
                    let att_ps = engine.players.get(att_pid)?;
                    let att_card = att_ps.field.iter().find(|c| c.instance_id == actor_id)?;
                    Some(engine.get_effective_ap(att_pid, att_card))
                }).unwrap_or(3000);

                let is_case_attack = engine.action_context.as_ref().and_then(|ctx| {
                    let tid = ctx.get("targetId").and_then(|v| v.as_str())?;
                    my_ps.r#case.as_ref().map(|c| c.instance_id == tid)
                }).unwrap_or(false);

                // 1. 変装があればAPが大きく上がる場合に使用
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
                                if new_ap - cur_ap >= self.weights.disguise_ap_threshold {
                                    return json!({"value": {"disguiseCardId": did, "targetId": target_id}});
                                }
                            }
                        }
                    }
                }

                // 2. カットインがあれば使うか判断
                if let Some(cutins) = opts.get("cutinCardIds").and_then(|v| v.as_array()) {
                    if let Some(first_cutin) = cutins.first().and_then(|v| {
                        v.get("instanceId").and_then(|s| s.as_str()).or_else(|| v.as_str())
                    }) {
                        return json!({"value": {"cutinCardId": first_cutin}});
                    }
                }

                // 2. ガード: 相手APより高い（返り討ち・守備成功できる）最小十分APのキャラでブロック！
                if let Some(guards) = opts.get("guardCardIds").and_then(|v| v.as_array()) {
                    let mut best_win_guard: Option<(&str, i32)> = None;
                    let mut weakest_guard: Option<(&str, i32)> = None;

                    for g_val in guards {
                        if let Some(gid) = g_val.as_str() {
                            if let Some(gc) = my_ps.field.iter().find(|c| c.instance_id == gid) {
                                let g_ap = engine.get_effective_ap(&self.player_id, gc);
                                // 守備成功条件: g_ap > attacker_ap
                                if g_ap > attacker_ap {
                                    if best_win_guard.map(|(_, b_ap)| g_ap < b_ap).unwrap_or(true) {
                                        best_win_guard = Some((gid, g_ap));
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
                    // 事件攻撃で証拠が残り1枚以下のピンチなら最弱キャラを盾にしてでも守る
                    if is_case_attack && my_ps.evidence.len() <= 1 {
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
                let max_select = opts.get("maxSelect").and_then(|v| v.as_u64()).unwrap_or(1).max(1) as usize;
                if let Some(candidates) = opts.get("candidates").and_then(|v| v.as_array()) {
                    let mut scored: Vec<(&str, i32)> = candidates
                        .iter()
                        .filter_map(|cand| {
                            let tid = cand.get("instanceId").and_then(|v| v.as_str())?;
                            let ap = cand.get("ap").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                            let lvl = cand.get("level").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                            Some((tid, ap + lvl * 500))
                        })
                        .collect();
                    if effect_type == "DISCARD_HAND" {
                        scored.sort_by_key(|&(_, s)| s);
                    } else {
                        scored.sort_by(|a, b| b.1.cmp(&a.1));
                    }
                    let chosen_ids: Vec<&str> = scored.into_iter().take(max_select).map(|(id, _)| id).collect();
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
                if let Some(cards) = opts.get("revealedCards").and_then(|v| v.as_array()) {
                    let mut best_cid: Option<&str> = None;
                    let mut best_score = i32::MIN;
                    let eff_file = my_ps.effective_file_count() as i32;
                    for c in cards {
                        if c.get("selectable").and_then(|v| v.as_bool()) == Some(false) {
                            continue;
                        }
                        let cid = match c.get("instanceId").and_then(|v| v.as_str()) {
                            Some(s) => s,
                            None => continue,
                        };
                        let lvl = c.get("level").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                        let ap = c.get("ap").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                        // 現在出せる範囲で最も強いカードを優先
                        let playable_bonus = if lvl <= eff_file + 2 { 3000 } else { 0 };
                        let score = playable_bonus + lvl * 500 + ap;
                        if score > best_score {
                            best_score = score;
                            best_cid = Some(cid);
                        }
                    }
                    if let Some(cid) = best_cid {
                        return json!({
                            "value": {
                                "chosenCardId": cid,
                                "actorId": actor_id
                            }
                        });
                    }
                }
                json!({
                    "value": {
                        "chosenCardId": null,
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
                if let Some(guards) = opts.get("guardCardIds").and_then(|v| v.as_array()) {
                    if !guards.is_empty() && rng.gen_bool(0.5) {
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
                if let Some(candidates) = opts.get("candidates").and_then(|v| v.as_array()) {
                    let chosen_ids: Vec<&str> = candidates
                        .choose_multiple(&mut rng, max_select.min(candidates.len()))
                        .filter_map(|c| c.get("instanceId").and_then(|v| v.as_str()))
                        .collect();
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
                if let Some(cards) = opts.get("revealedCards").and_then(|v| v.as_array()) {
                    let selectable: Vec<&Value> = cards
                        .iter()
                        .filter(|c| c.get("selectable").and_then(|v| v.as_bool()).unwrap_or(true))
                        .collect();
                    if let Some(chosen) = selectable.choose(&mut rng) {
                        let cid = chosen.get("instanceId").and_then(|v| v.as_str());
                        return json!({
                            "value": {
                                "chosenCardId": cid,
                                "actorId": actor_id
                            }
                        });
                    }
                }
                json!({
                    "value": {
                        "chosenCardId": null,
                        "actorId": actor_id
                    }
                })
            }
            _ => json!({"value": {}}),
        }
    }
}

/// 2つの重みセットで1試合シミュレーションし、(勝者: Option<"p1"|"p2">, p1証拠 - p2証拠) を返す
fn simulate_match(
    catalog: &CardCatalog,
    p1_deck: &DeckDefinition,
    p2_deck: &DeckDefinition,
    w1: &CpuWeights,
    w2: &CpuWeights,
) -> (Option<String>, i32) {
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
    (engine.winner_id, p1_ev - p2_ev)
}

/// 自己対戦（Evolution Strategies）でCPUの重みを学習し、`data/cpu_weights.json` に保存する
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
        cand.disguise_ap_threshold = (cand.disguise_ap_threshold + rng.gen_range(-400..=400)).max(0);

        let mut cand_wins = 0usize;
        let mut best_wins = 0usize;
        let mut ev_diff_sum = 0i32;

        for g in 0..games_per_gen {
            let cand_is_p1 = g % 2 == 0;
            let (winner, diff) = if cand_is_p1 {
                simulate_match(catalog, p1_deck, p2_deck, &cand, &best)
            } else {
                simulate_match(catalog, p1_deck, p2_deck, &best, &cand)
            };
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

    best
}
