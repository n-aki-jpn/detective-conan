use crate::engine::MatchEngine;
use crate::models::{CardOrientation, CardType, CasePhase};
use rand::seq::SliceRandom;
use rand::Rng;
use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub struct RandomAgent {
    pub player_id: String,
}

impl RandomAgent {
    pub fn new(player_id: &str) -> Self {
        Self {
            player_id: player_id.to_string(),
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

                // 2. パートナーアシスト: FILE 6枚なら即解決編突入のため最優先、それ以外でも中盤なら積極的にFILEへ
                if find_act("ASSIST").is_some() && my_ps.case_phase != CasePhase::RESOLUTION {
                    let eff_file = my_ps.effective_file_count();
                    if eff_file == 6 || (eff_file >= 4 && my_ps.field.len() >= 2) {
                        return json!({"value": {"type": "ASSIST"}});
                    }
                }

                // 3. 手札登場・イベント使用: 出せる中で最も高レベル・高APのカードを優先
                if let Some(act) = find_act("HAND_PLAY") {
                    if let Some(card_ids) = act.get("cardIds").and_then(|v| v.as_array()) {
                        let mut best_cid: Option<&str> = None;
                        let mut best_score = i32::MIN;
                        for cid_val in card_ids {
                            if let Some(cid) = cid_val.as_str() {
                                if let Some(c) = my_ps.hand.iter().find(|h| h.instance_id == cid) {
                                    if let Some(def) = engine.catalog.get(&c.definition_id) {
                                        let mut score = def.level * 1000 + def.ap + def.lp * 500;
                                        if def.description.contains("突撃") {
                                            score += 1200;
                                        }
                                        if def.description.contains("【登場時】") {
                                            score += 800;
                                        }
                                        // イベントはキャラ登場を優先した後に使うよう少し調整（ただし除去イベント等は高評価）
                                        if def.card_type == CardType::EVENT {
                                            score -= 400;
                                            if def.description.contains("リムーブ") || def.description.contains("スタン") {
                                                // 相手フィールドが空なら温存
                                                let opp_has_field = opp_ps.map(|o| !o.field.is_empty()).unwrap_or(false);
                                                if !opp_has_field {
                                                    score -= 5000;
                                                } else {
                                                    score += 700;
                                                }
                                            }
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

                // 1. カットインがあれば使うか判断（相手よりAPが足りない／事件攻撃を止めたい）
                if let Some(cutins) = opts.get("cutinCardIds").and_then(|v| v.as_array()) {
                    if let Some(first_cutin) = cutins.first().and_then(|v| v.as_str()) {
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
                if let Some(candidates) = opts.get("candidates").and_then(|v| v.as_array()) {
                    // 相手への妨害も味方へのバフも、候補の中で最もAP（次いでレベル）が高い対象を選ぶのが基本最適
                    let mut best_tid: Option<&str> = None;
                    let mut best_score = i32::MIN;
                    for cand in candidates {
                        let tid = match cand.get("instanceId").and_then(|v| v.as_str()) {
                            Some(s) => s,
                            None => continue,
                        };
                        let ap = cand.get("ap").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                        let lvl = cand.get("level").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                        let score = ap + lvl * 500;
                        if score > best_score {
                            best_score = score;
                            best_tid = Some(tid);
                        }
                    }
                    if let Some(tid) = best_tid {
                        return json!({
                            "value": {
                                "targetId": tid,
                                "actorId": actor_id,
                                "effectType": effect_type
                            }
                        });
                    }
                }
                json!({
                    "value": {
                        "targetId": null,
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
                if let Some(candidates) = opts.get("candidates").and_then(|v| v.as_array()) {
                    if let Some(chosen) = candidates.choose(&mut rng) {
                        let tid = chosen.get("instanceId").and_then(|v| v.as_str());
                        return json!({
                            "value": {
                                "targetId": tid,
                                "actorId": actor_id,
                                "effectType": effect_type
                            }
                        });
                    }
                }
                json!({
                    "value": {
                        "targetId": null,
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
