use super::*;

impl MatchEngine {

    pub fn prompt_main_action(&mut self, player_id: &str) {
        self.field_overflow_excluded_ids.clear();
        if self.phase == MatchPhase::FINISHED {
            return;
        }
        if self.phase == MatchPhase::END_TRIGGERS {
            if self.check_field_overflow(player_id, "END_TURN") {
                return;
            }
            let next_turn = self.turn + 1;
            let cur_turn_pid = self.turn_player_id.clone();
            let next_pid = self
                .player_order
                .iter()
                .find(|pid| *pid != &cur_turn_pid)
                .cloned()
                .unwrap_or(cur_turn_pid);
            self.start_turn(next_turn, &next_pid);
            return;
        }

        let ps = match self.players.get(player_id) {
            Some(p) => p,
            None => return,
        };
        let opp_id = self.player_order.iter().find(|p| *p != player_id).unwrap().clone();
        let opp_ps = self.players.get(&opp_id).unwrap();

        let effective_file = ps.effective_file_count() as i32;
        let case_def = ps.r#case.as_ref().and_then(|c| self.catalog.get(&c.definition_id));
        let case_colors = case_def.map(|d| normalize_color_set(&d.colors)).unwrap_or_default();

        let mut actions: Vec<Value> = vec![];

        // 1. End turn
        actions.push(json!({"type": "END_TURN"}));

        // 2. Hand Play (キャラ・イベント合わせて通常手札使用は1ターン1回、キャラは6体目以降も登場可→ルール処理で5体までリムーブ)
        {
            let cond_ctx = self.build_cond_info(player_id, None, CardOrientation::ACTIVE, None);
            let can_normal_play = !ps.normal_hand_played_this_turn && !ps.hint_played_this_turn;
            let can_play_char = can_normal_play;
            let event_blocked = self.turn_event_blocked_players.contains(player_id);
            let mut playable_cards = vec![];
            if can_normal_play {
                for c in &ps.hand {
                    if let Some(c_def) = self.catalog.get(&c.definition_id) {
                        // ターン内・カード名BAN（B06103 ジン等）: 使用・登場を禁止
                        if self.is_name_banned_this_turn(player_id, c, c_def) {
                            continue;
                        }
                        let card_colors = normalize_color_set(&c_def.colors);
                        let color_ok = card_colors.is_empty() || case_colors.is_empty() || card_colors.is_subset(&case_colors);
                        let eff_lv = card_effective_level(c_def, &cond_ctx);
                        if eff_lv <= effective_file && color_ok {
                            if c_def.card_type == CardType::EVENT {
                                if !event_blocked && check_event_play_conditions(&c_def.description, &cond_ctx).is_ok() {
                                    playable_cards.push(c.instance_id.clone());
                                }
                            } else if can_play_char {
                                playable_cards.push(c.instance_id.clone());
                            }
                        }
                    }
                }
            }
            if !playable_cards.is_empty() {
                actions.push(json!({
                    "type": "HAND_PLAY",
                    "cardIds": playable_cards
                }));
            }
        }

        // 3. Hint Take
        if !ps.file.is_empty() && !self.turn_hint_blocked_players.contains(player_id) {
            actions.push(json!({"type": "HINT_TAKE"}));
        }

        // 4. Assist
        if let Some(ref p) = ps.partner {
            if p.zone == Zone::PARTNER && p.orientation == CardOrientation::ACTIVE {
                actions.push(json!({"type": "ASSIST"}));
            }
        }

        // 5. Resolve Case
        if ps.case_phase == CasePhase::RESOLUTION {
            if let Some(ref p) = ps.partner {
                if p.zone == Zone::PARTNER && p.orientation == CardOrientation::ACTIVE {
                    let target_evidence = if let Some(cd) = case_def {
                        cd.case_levels.as_ref().and_then(|cl| {
                            let key = if ps.is_first_player { "first" } else { "second" };
                            cl.get(key).cloned()
                        }).unwrap_or(6)
                    } else {
                        6
                    };
                    if ps.evidence.len() >= target_evidence as usize {
                        actions.push(json!({"type": "RESOLVE_CASE"}));
                    }
                }
            }
        }

        // 6. Deduce
        let mut deduce_actors = vec![];
        if let Some(ref p) = ps.partner {
            if p.zone == Zone::PARTNER && p.orientation == CardOrientation::ACTIVE {
                deduce_actors.push(p.instance_id.clone());
            }
        }
        for c in &ps.field {
            // 突撃（has_rush）は登場ターンの「アクション」のみ可能であり、「迅速（SWIFT）」がある場合のみ登場ターンに推理も可能
            let can_deduce = c.orientation == CardOrientation::ACTIVE
                && (c.entered_turn != self.turn || self.effective_swift(c));
            if can_deduce {
                deduce_actors.push(c.instance_id.clone());
            }
        }
        if !deduce_actors.is_empty() {
            actions.push(json!({
                "type": "DEDUCE",
                "actorIds": deduce_actors
            }));
        }

        // 7. Action
        let mut char_target_ids = vec![];
        let mut active_opp_char_ids = vec![];
        for opp_c in &opp_ps.field {
            if opp_c.orientation == CardOrientation::SLEEP || opp_c.orientation == CardOrientation::STUN {
                char_target_ids.push(opp_c.instance_id.clone());
            } else if opp_c.orientation == CardOrientation::ACTIVE {
                active_opp_char_ids.push(opp_c.instance_id.clone());
            }
        }
        let mut case_target_id: Option<String> = None;
        if !opp_ps.evidence.is_empty() {
            if let Some(ref opp_case) = opp_ps.r#case {
                case_target_id = Some(opp_case.instance_id.clone());
            }
        }

        let mut all_target_ids = char_target_ids.clone();
        for aid in &active_opp_char_ids {
            all_target_ids.push(aid.clone());
        }
        if let Some(ref cid) = case_target_id {
            all_target_ids.push(cid.clone());
        }

        let mut action_actors = vec![];
        let mut dbg_lines: Vec<String> = Vec::new();
        let mut actor_target_map: HashMap<String, Vec<String>> = HashMap::new();
        for c in &ps.field {
            let (has_rush, rush_target) = self.effective_rush(c);
            let can_act = c.orientation == CardOrientation::ACTIVE && (c.entered_turn != self.turn || has_rush);
            if c.entered_turn == self.turn || !c.set_cards.is_empty() {
                dbg_lines.push(format!(
                    "[DEBUG][ACTION_CHECK] turn={} player={} card={}({}) ori={:?} entered={} set_cards={} has_rush={} rush_target={:?} can_act={} case_target={:?} opp_sleep_chars={}",
                    self.turn,
                    player_id,
                    c.definition_id,
                    c.instance_id,
                    c.orientation,
                    c.entered_turn,
                    c.set_cards.len(),
                    has_rush,
                    rush_target,
                    can_act,
                    case_target_id,
                    char_target_ids.len(),
                ));
            }
            if !can_act {
                continue;
            }
            let can_atk_active = self.effective_can_attack_active(c);
            let mut valid_char_targets = char_target_ids.clone();
            if can_atk_active {
                valid_char_targets.extend(active_opp_char_ids.iter().cloned());
            }
            let valid_targets = if c.entered_turn == self.turn && has_rush {
                match rush_target.as_deref() {
                    Some("CHAR") => valid_char_targets,
                    Some("CHAR_LV4") => valid_char_targets
                        .into_iter()
                        .filter(|tid| {
                            opp_ps
                                .field
                                .iter()
                                .find(|oc| &oc.instance_id == tid)
                                .and_then(|oc| {
                                    self.catalog
                                        .get(&oc.definition_id)
                                        .map(|od| (od.level + oc.temp_level_delta) <= 4)
                                })
                                .unwrap_or(false)
                        })
                        .collect(),
                    Some("CHAR_LV4+CASE") => {
                        let mut v: Vec<String> = valid_char_targets
                            .into_iter()
                            .filter(|tid| {
                                opp_ps
                                    .field
                                    .iter()
                                    .find(|oc| &oc.instance_id == tid)
                                    .and_then(|oc| {
                                        self.catalog
                                            .get(&oc.definition_id)
                                            .map(|od| (od.level + oc.temp_level_delta) <= 4)
                                    })
                                    .unwrap_or(false)
                            })
                            .collect();
                        if let Some(ref cid) = case_target_id {
                            v.push(cid.clone());
                        }
                        v
                    }
                    Some("CASE") => case_target_id.iter().cloned().collect(),
                    _ => {
                        let mut v = valid_char_targets;
                        if let Some(ref cid) = case_target_id {
                            v.push(cid.clone());
                        }
                        v
                    }
                }
            } else {
                let mut v = valid_char_targets;
                if let Some(ref cid) = case_target_id {
                    v.push(cid.clone());
                }
                v
            };
            if !valid_targets.is_empty() {
                action_actors.push(c.instance_id.clone());
                actor_target_map.insert(c.instance_id.clone(), valid_targets);
            } else if c.entered_turn == self.turn || !c.set_cards.is_empty() {
                dbg_lines.push(format!(
                    "[DEBUG][ACTION_SKIP] turn={} player={} card={}({}) valid_targets is empty",
                    self.turn, player_id, c.definition_id, c.instance_id
                ));
            }
        }
        for l in dbg_lines {
            self.pending_file_logs.push(l);
        }

        if !action_actors.is_empty() && !all_target_ids.is_empty() {
            actions.push(json!({
                "type": "ACTION",
                "actorIds": action_actors,
                "targetIds": all_target_ids,
                "actorTargetMap": actor_target_map
            }));
        }

        // 8. Declare (宣言効果)
        let can_declare_clause = |actor_id: &str, actor_ori: CardOrientation, used_turn: bool, in_partner_area: bool, in_hand: bool, def: &crate::models::CardDefinition| -> bool {
            let mut clauses = extract_clauses(&def.description, TriggerKind::Declare, def.card_type);
            if let Some(granted_list) = self.granted_abilities_this_turn.get(actor_id) {
                for g in granted_list {
                    clauses.extend(extract_clauses(g, TriggerKind::Declare, CardType::CHARACTER));
                }
            }
            if clauses.is_empty() {
                return false;
            }
            let ctx = self.build_cond_info(player_id, Some(actor_id), actor_ori, None);
            let single_clause_legacy_used = used_turn && clauses.len() == 1;
            clauses.iter().any(|clause| {
                if !in_hand && clause.contains("この能力はこのカードが手札にあり") {
                    return false;
                }
                if in_partner_area && def.card_type != CardType::PARTNER && !allows_partner_area(clause) {
                    return false;
                }
                if check_clause_conditions(clause, &ctx).is_err() {
                    return false;
                }
                if single_clause_legacy_used || !self.can_use_clause_this_turn(actor_id, clause) {
                    return false;
                }
                let cost = parse_clause_cost(clause);
                self.can_pay_clause_cost(ps, actor_id, &cost)
            })
        };

        let mut declare_actors = vec![];
        for c in &ps.field {
            if let Some(c_def) = self.catalog.get(&c.definition_id) {
                if can_declare_clause(&c.instance_id, c.orientation, c.used_declare_this_turn, false, false, c_def) {
                    declare_actors.push(json!({
                        "actorId": c.instance_id.clone(),
                        "name": self.card_name_by_def(&c.definition_id),
                        "desc": c_def.description.clone(),
                    }));
                }
            }
            for sc in &c.set_cards {
                if !sc.face_up {
                    continue;
                }
                if let Some(sc_def) = self.catalog.get(&sc.definition_id) {
                    if can_declare_clause(&sc.instance_id, c.orientation, sc.used_declare_this_turn, false, false, sc_def) {
                        declare_actors.push(json!({
                            "actorId": sc.instance_id.clone(),
                            "name": format!("🔧 {}", self.card_name_by_def(&sc.definition_id)),
                            "desc": sc_def.description.clone(),
                        }));
                    }
                }
            }
        }
        if let Some(ref p) = ps.partner {
            if p.zone == Zone::PARTNER {
                if let Some(p_def) = self.catalog.get(&p.definition_id) {
                    if can_declare_clause(&p.instance_id, p.orientation, p.used_declare_this_turn, true, false, p_def) {
                        declare_actors.push(json!({
                            "actorId": p.instance_id.clone(),
                            "name": self.card_name_by_def(&p.definition_id),
                            "desc": p_def.description.clone(),
                        }));
                    }
                }
            }
        }
        for ep in &ps.extra_partners {
            if let Some(ep_def) = self.catalog.get(&ep.definition_id) {
                if can_declare_clause(&ep.instance_id, ep.orientation, ep.used_declare_this_turn, true, false, ep_def) {
                    declare_actors.push(json!({
                        "actorId": ep.instance_id.clone(),
                        "name": self.card_name_by_def(&ep.definition_id),
                        "desc": ep_def.description.clone(),
                    }));
                }
            }
        }
        for hc in &ps.hand {
            if let Some(h_def) = self.catalog.get(&hc.definition_id) {
                // ターン内・カード名BAN（B06103 ジン等）: 手札からの使用を禁止
                if self.is_name_banned_this_turn(player_id, hc, h_def) {
                    continue;
                }
                if (h_def.card_type == CardType::EVENT
                    || (h_def.card_type == CardType::CHARACTER
                        && h_def.description.contains("この能力はこのカードが手札にあり")))
                    && can_declare_clause(&hc.instance_id, hc.orientation, hc.used_declare_this_turn, false, true, h_def)
                {
                    declare_actors.push(json!({
                        "actorId": hc.instance_id.clone(),
                        "name": format!("📜(手札) {}", self.card_name_by_def(&hc.definition_id)),
                        "desc": h_def.description.clone(),
                    }));
                }
            }
        }
        if let Some(ref cs) = ps.r#case {
            if let Some(c_def) = self.catalog.get(&cs.definition_id) {
                if can_declare_clause(&cs.instance_id, cs.orientation, cs.used_declare_this_turn, false, false, c_def) {
                    declare_actors.push(json!({
                        "actorId": cs.instance_id.clone(),
                        "name": format!("📜 事件: {}", self.card_name_by_def(&cs.definition_id)),
                        "desc": c_def.description.clone(),
                    }));
                }
            }
        }
        if !declare_actors.is_empty() {
            actions.push(json!({
                "type": "DECLARE",
                "actors": declare_actors
            }));
        }

        // 9. Disguise (変装)
        if !ps.field.is_empty() {
        }

        let dec_id = self.generate_decision_id();
        self.pending_decision = Some(json!({
            "decisionId": dec_id,
            "revision": self.revision,
            "playerId": player_id,
            "kind": "mainAction",
            "options": {
                "actions": actions
            }
        }));
    }

    pub(super) fn handle_main_action(&mut self, player_id: &str, val: &Value) {
        let action_type = val.get("type").and_then(|v| v.as_str()).unwrap_or("");

        match action_type {
            "END_TURN" => {
                if self.phase == MatchPhase::END_TRIGGERS || self.phase == MatchPhase::FINISHED {
                    return;
                }
                self.phase = MatchPhase::END_TRIGGERS;
                self.log(format!("⏭ {} がターンを終了しました", player_id));
                // ターン終了時トリガーの解決
                self.trigger_on_turn_end(player_id);
                if self.phase == MatchPhase::FINISHED || self.pending_decision.is_some() {
                    return;
                }
                let next_turn = self.turn + 1;
                let next_pid = self.player_order.iter().find(|pid| *pid != player_id).unwrap().clone();
                self.start_turn(next_turn, &next_pid);
            }
            "HAND_PLAY" => {
                let card_id = val.get("cardId").and_then(|v| v.as_str()).unwrap_or("");
                let turn = self.turn;
                let mut played_def_id = None;
                let mut played_inst_id = None;
                let mut is_event = false;
                // ターン内・カード名BAN（B06103 ジン等）の防御的再チェック
                {
                    let (ban_def_id, banned) = {
                        let c_opt = self
                            .players
                            .get(player_id)
                            .and_then(|ps| ps.hand.iter().find(|c| c.instance_id == card_id));
                        let did = c_opt.map(|c| c.definition_id.clone()).unwrap_or_default();
                        let banned = match (c_opt, did.as_str()) {
                            (Some(c), d) => self.is_name_banned_this_turn(player_id, c, self.catalog.get(d).unwrap_or_else(|| panic!("catalog missing {d}"))),
                            _ => false,
                        };
                        (did, banned)
                    };
                    if banned {
                        self.log(format!("🚫 {} はこのターン中、〚カード名［{}］〛を使用・登場させられません", player_id, self.card_name_by_def(&ban_def_id)));
                        self.prompt_main_action(player_id);
                        return;
                    }
                }
                if let Some(ps) = self.players.get_mut(player_id) {
                    if let Some(idx) = ps.hand.iter().position(|c| c.instance_id == card_id) {
                        let mut card = ps.hand.remove(idx);
                        let card_type = self.catalog.get(&card.definition_id).map(|d| d.card_type).unwrap_or(CardType::CHARACTER);
                        played_def_id = Some(card.definition_id.clone());
                        played_inst_id = Some(card.instance_id.clone());
                        card.face_up = true;
                        if card_type == CardType::EVENT {
                            is_event = true;
                            card.zone = Zone::REMOVE;
                            ps.remove.push(card);
                            ps.normal_hand_played_this_turn = true;
                        } else {
                            card.zone = Zone::FIELD;
                            card.orientation = CardOrientation::ACTIVE;
                            card.entered_turn = turn;
                            self.field_overflow_excluded_ids.insert(card.instance_id.clone());
                            ps.field.push(card);
                            ps.normal_hand_played_this_turn = true;
                        }
                    }
                }
                if let (Some(def_id), Some(inst_id)) = (played_def_id, played_inst_id) {
                    let card_name = self.card_name_by_def(&def_id);
                    let lvl = self.catalog.get(&def_id).map(|d| d.level).unwrap_or(0);
                    if is_event {
                        self.played_by_effect_events = false;
                        self.log(format!("📜 {} がイベント [{}] (Lv.{}) を使用しました", player_id, card_name, lvl));
                        if self.resolve_on_play_effects(player_id, &inst_id, &def_id, true) {
                            return;
                        }
                    } else {
                        self.log(format!("🃏 {} が [{}] (Lv.{}) を登場させました", player_id, card_name, lvl));
                        if self.resolve_character_enter_field_with_source(player_id, &inst_id, &def_id, CardOrientation::ACTIVE, None, "MAIN") {
                            return;
                        }
                    }
                }
                if self.check_field_overflow(player_id, "MAIN") {
                    return;
                }
                self.prompt_main_action(player_id);
            }
            "HINT_TAKE" => {
                if self.turn_hint_blocked_players.contains(player_id) {
                    self.log(format!("🚫 {} はこのターン【ネクストヒント】を使用できません", player_id));
                    self.prompt_main_action(player_id);
                    return;
                }
                let mut playable_cards = vec![];
                let event_blocked = self.turn_event_blocked_players.contains(player_id);
                if let Some(ps) = self.players.get_mut(player_id) {
                    if let Some(mut top_file) = ps.file.pop() {
                        top_file.zone = Zone::HAND;
                        top_file.face_up = true;
                        ps.hand.push(top_file);
                        ps.hint_played_this_turn = true;
                    }
                }
                let cond_ctx = self.build_cond_info(player_id, None, CardOrientation::ACTIVE, None);
                if let Some(ps) = self.players.get(player_id) {
                    let effective_file = ps.effective_file_count() as i32;
                    let case_def = ps.r#case.as_ref().and_then(|c| self.catalog.get(&c.definition_id));
                    let case_colors = case_def.map(|d| normalize_color_set(&d.colors)).unwrap_or_default();

                    for c in &ps.hand {
                        if let Some(c_def) = self.catalog.get(&c.definition_id) {
                            // ターン内・カード名BAN（B06103 ジン等）: 使用・登場を禁止
                            if self.is_name_banned_this_turn(player_id, c, c_def) {
                                continue;
                            }
                            let card_colors = normalize_color_set(&c_def.colors);
                            let color_ok = card_colors.is_empty() || case_colors.is_empty() || card_colors.is_subset(&case_colors);
                            let slot_ok = true;
                            let eff_lv = card_effective_level(c_def, &cond_ctx);
                            let ev_ok = c_def.card_type != CardType::EVENT
                                || (!event_blocked && check_event_play_conditions(&c_def.description, &cond_ctx).is_ok());
                            if eff_lv <= effective_file && color_ok && slot_ok && ev_ok {
                                playable_cards.push(c.instance_id.clone());
                            }
                        }
                    }
                }
                self.log(format!("💡 {} が ネクストヒント を使用（FILEから手札へ1枚回収）", player_id));

                let dec_id = self.generate_decision_id();
                self.pending_decision = Some(json!({
                    "decisionId": dec_id,
                    "revision": self.revision,
                    "playerId": player_id,
                    "kind": "hintPlay",
                    "options": {
                        "playableCardIds": playable_cards
                    }
                }));
            }
            "ASSIST" => {
                if let Some(ps) = self.players.get_mut(player_id) {
                    if let Some(ref mut p) = ps.partner {
                        if p.zone == Zone::PARTNER {
                            p.orientation = CardOrientation::SLEEP;
                            p.zone = Zone::FILE;
                        }
                    }
                }
                self.log(format!("🤝 {} が パートナーのアシスト を宣言（パートナーをFILEへ移動）", player_id));
                self.check_resolution_transition(player_id);
                if self.phase == MatchPhase::FINISHED || self.pending_decision.is_some() {
                    return;
                }
                self.prompt_main_action(player_id);
            }
            "RESOLVE_CASE" => {
                self.log(format!("🎉 {} が 事件解決 を宣言！", player_id));
                self.finish_match(player_id, "CASE_RESOLVED");
            }
            "DEDUCE" => {
                let actor_id = val.get("actorId").and_then(|v| v.as_str()).unwrap_or("").to_string();
                // サーバー側でも登場ターン推理不可・アクティブ状態の検証を行う（〚迅速〛は例外）
                let deduce_valid = self.players.get(player_id).map(|ps| {
                    if let Some(ref p) = ps.partner {
                        if p.instance_id == actor_id {
                            return p.orientation == CardOrientation::ACTIVE;
                        }
                    }
                    if let Some(c) = ps.field.iter().find(|c| c.instance_id == actor_id) {
                        return c.orientation == CardOrientation::ACTIVE
                            && (c.entered_turn != self.turn || self.effective_swift(c));
                    }
                    false
                }).unwrap_or(false);
                if !deduce_valid {
                    self.log("⚠️ そのキャラは現在推理できません（登場ターンに突撃のみで推理不可、またはアクティブ状態ではありません）");
                    self.prompt_main_action(player_id);
                    return;
                }
                let mut actor_card: Option<CardInstance> = None;
                if let Some(ps) = self.players.get_mut(player_id) {
                    if let Some(ref mut p) = ps.partner {
                        if p.instance_id == actor_id {
                            p.orientation = CardOrientation::SLEEP;
                            actor_card = Some(p.clone());
                        }
                    }
                    if actor_card.is_none() {
                        if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == actor_id) {
                            c.orientation = CardOrientation::SLEEP;
                            actor_card = Some(c.clone());
                        }
                    }
                }
                let lp = actor_card
                    .as_ref()
                    .map(|c| self.get_effective_lp(player_id, c))
                    .unwrap_or(0);

                let actor_name = self.card_name_by_inst(&actor_id);
                if lp > 0 {
                    self.gain_evidence(player_id, lp as usize);
                    self.log(format!("🔎 {} の [{}] が 推理 を行いました（証拠 +{}）", player_id, actor_name, lp));
                } else {
                    self.log(format!("🔎 {} の [{}] が 推理 を行いました", player_id, actor_name));
                }
                if self.trigger_on_action(player_id, &actor_id, ActionEventKind::Deduce, None, "MAIN") {
                    return;
                }
                self.prompt_main_action(player_id);
            }
            "ACTION" => {
                let actor_id = val.get("actorId").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let target_id = val.get("targetId").and_then(|v| v.as_str()).unwrap_or("").to_string();

                let opp_pid = self.player_order.iter().find(|p| *p != player_id).unwrap().clone();

                let is_case_target = self
                    .players
                    .get(&opp_pid)
                    .and_then(|opp_ps| opp_ps.r#case.as_ref())
                    .map(|c| c.instance_id == target_id)
                    .unwrap_or(false);

                // 突撃・登場ターン制限・攻撃対象制限チェック
                let rush_violation = {
                    let ps = self.players.get(player_id);
                    let target_eff_level = self
                        .players
                        .get(&opp_pid)
                        .and_then(|ops| ops.field.iter().find(|oc| oc.instance_id == target_id))
                        .and_then(|oc| {
                            self.catalog
                                .get(&oc.definition_id)
                                .map(|od| od.level + oc.temp_level_delta)
                        })
                        .unwrap_or(99);
                    if let Some(ps) = ps {
                        if let Some(actor_c) = ps.field.iter().find(|c| c.instance_id == actor_id) {
                            let (has_rush, rush_target) = self.effective_rush(actor_c);
                            if actor_c.orientation != CardOrientation::ACTIVE {
                                Some("アクティブ状態ではないためアクションできません")
                            } else if actor_c.entered_turn == self.turn && !has_rush {
                                Some("登場したターンは突撃がないためアクションできません")
                            } else if actor_c.entered_turn == self.turn && has_rush {
                                match rush_target.as_deref() {
                                    Some("CHAR") if is_case_target => Some("〚突撃［キャラ］〛は事件を攻撃できません"),
                                    Some("CHAR_LV4") if is_case_target || target_eff_level > 4 => {
                                        Some("〚突撃［レベル4以下のキャラ］〛はレベル4以下のキャラ以外を攻撃できません")
                                    }
                                    Some("CHAR_LV4+CASE") if !is_case_target && target_eff_level > 4 => {
                                        Some("レベル5以上のキャラを攻撃できません")
                                    }
                                    Some("CASE") if !is_case_target => Some("〚突撃［事件］〛はキャラを攻撃できません"),
                                    _ => None,
                                }
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                };
                if let Some(msg) = rush_violation {
                    self.log(format!("⚠️ {}", msg));
                    self.prompt_main_action(player_id);
                    return;
                }

                if let Some(ps) = self.players.get_mut(player_id) {
                    if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == actor_id) {
                        c.orientation = CardOrientation::SLEEP;
                    }
                }

                let actor_name = self.card_name_by_inst(&actor_id);
                let target_name = self.card_name_by_inst(&target_id);
                self.log(format!("⚔️ {} の [{}] が {} の [{}] に攻撃を仕掛けました！", player_id, actor_name, opp_pid, target_name));

                self.contact_ap_buffs.clear();
                self.action_context = Some(json!({
                    "actorId": actor_id,
                    "targetId": target_id,
                    "attackerPlayerId": player_id,
                    "defenderPlayerId": opp_pid
                }));

                // アクション時トリガーの解決
                self.acted_this_turn_ids.insert(actor_id.clone());
                let action_kind = if is_case_target {
                    ActionEventKind::AttackCase
                } else {
                    ActionEventKind::AttackChar
                };
                let start_guard_ctx = format!("START_GUARD:{}", opp_pid);
                if self.trigger_on_action(player_id, &actor_id, action_kind, Some(&target_id), &start_guard_ctx) {
                    return;
                }

                if !self.is_action_target_still_valid() {
                    self.action_context = None;
                self.contact_ap_buffs.clear();
                    self.log("⚠️ 指定されていた対象が現場を離れたためアクションを終了します");
                    self.prompt_main_action(player_id);
                    return;
                }

                self.prompt_guard_or_cutin(&opp_pid);
            }
            "DECLARE" => {
                let actor_id = val.get("actorId").and_then(|v| v.as_str()).unwrap_or("").to_string();
                self.handle_declare_action(player_id, &actor_id);
            }
            "DISGUISE" => {
                if self.turn_disguise_blocked_players.contains(player_id) {
                    self.log(format!("🚫 {} はこのターン【変装】を使用できません", player_id));
                    self.prompt_main_action(player_id);
                    return;
                }
                let card_id = val.get("cardId").and_then(|v| v.as_str()).unwrap_or("");
                let target_id = val.get("targetId").and_then(|v| v.as_str()).unwrap_or("");
                let turn = self.turn;
                let mut disguised_def_id = None;
                let mut disguised_inst_id = None;
                let mut replaced_def_id = None;

                if let Some(ps) = self.players.get_mut(player_id) {
                    if let (Some(h_idx), Some(f_idx)) = (
                        ps.hand.iter().position(|c| c.instance_id == card_id),
                        ps.field.iter().position(|c| c.instance_id == target_id),
                    ) {
                        let mut hand_card = ps.hand.remove(h_idx);
                        let mut old_card = ps.field.remove(f_idx);
                        replaced_def_id = Some(old_card.definition_id.clone());
                        disguised_def_id = Some(hand_card.definition_id.clone());
                        disguised_inst_id = Some(hand_card.instance_id.clone());

                        hand_card.zone = Zone::FIELD;
                        hand_card.orientation = old_card.orientation;
                        hand_card.entered_turn = turn;
                        hand_card.face_up = true;
                        ps.field.push(hand_card);

                        old_card.zone = Zone::DECK;
                        old_card.face_up = false;
                        ps.deck.insert(0, old_card);
                    }
                }

                if let (Some(def_id), Some(inst_id), Some(old_def_id)) = (disguised_def_id, disguised_inst_id, replaced_def_id) {
                    let new_name = self.card_name_by_def(&def_id);
                    let old_name = self.card_name_by_def(&old_def_id);
                    self.last_disguise_replaced_name = self
                        .catalog
                        .get(&old_def_id)
                        .map(|d| d.names.join("|"))
                        .or_else(|| Some(old_name.clone()));
                    self.log(format!("🎭 {} が [{}] と入れ替えて [{}] を【変装】登場させました！", player_id, old_name, new_name));
                    let need_ui = self.trigger_on_disguise(player_id, &inst_id, &def_id);
                    if need_ui {
                        return;
                    }
                }
                if self.check_field_overflow(player_id, "MAIN") {
                    return;
                }
                self.prompt_main_action(player_id);
            }
            _ => {
                self.prompt_main_action(player_id);
            }
        }
    }

    pub(super) fn handle_declare_action(&mut self, player_id: &str, actor_id: &str) {
        let mut is_set_card = false;
        let mut set_host_id: Option<String> = None;
        let mut is_hand_event_declare = false;

        let info_opt = self.players.get(player_id).and_then(|ps| {
            if let Some(ref p) = ps.partner {
                if p.instance_id == actor_id {
                    return Some((p.definition_id.clone(), p.zone == Zone::PARTNER, p.used_declare_this_turn));
                }
            }
            for ep in &ps.extra_partners {
                if ep.instance_id == actor_id {
                    return Some((ep.definition_id.clone(), true, ep.used_declare_this_turn));
                }
            }
            if let Some(ref cs) = ps.r#case {
                if cs.instance_id == actor_id {
                    return Some((cs.definition_id.clone(), false, cs.used_declare_this_turn));
                }
            }
            if let Some(c) = ps.field.iter().find(|c| c.instance_id == actor_id) {
                return Some((c.definition_id.clone(), false, c.used_declare_this_turn));
            }
            for c in &ps.field {
                if let Some(sc) = c.set_cards.iter().find(|s| s.instance_id == actor_id) {
                    is_set_card = true;
                    set_host_id = Some(c.instance_id.clone());
                    return Some((sc.definition_id.clone(), false, sc.used_declare_this_turn));
                }
            }
            if let Some(hc) = ps.hand.iter().find(|h| h.instance_id == actor_id) {
                is_hand_event_declare = true;
                return Some((hc.definition_id.clone(), false, hc.used_declare_this_turn));
            }
            None
        });

        let Some((def_id, in_partner_area, legacy_used)) = info_opt else {
            self.prompt_main_action(player_id);
            return;
        };
        let Some(def) = self.catalog.get(&def_id).cloned() else {
            self.prompt_main_action(player_id);
            return;
        };

        let card_name = self.card_name_by_def(&def_id);
        let ori = self.find_actor_orientation(player_id, set_host_id.as_deref().unwrap_or(actor_id));
        let mut clauses = extract_clauses(&def.description, TriggerKind::Declare, def.card_type);
        if let Some(granted_list) = self.granted_abilities_this_turn.get(actor_id) {
            for g in granted_list {
                for inner in extract_clauses(g, TriggerKind::Declare, CardType::CHARACTER) {
                    if !clauses.contains(&inner) {
                        clauses.push(inner);
                    }
                }
            }
        }
        if is_set_card {
            for inner in self.extract_set_card_granted_clauses(&def.description, TriggerKind::Declare) {
                if !clauses.contains(&inner) {
                    clauses.push(inner);
                }
            }
        }
        let single_clause_legacy_used = legacy_used && clauses.len() == 1;

        // 先に条件判定（immutable borrow を完了）
        let mut cond_ok_clauses: Vec<String> = Vec::new();
        {
            let ctx = self.build_cond_info(player_id, Some(actor_id), ori, None);
            for cl in &clauses {
                if in_partner_area && def.card_type != CardType::PARTNER && !allows_partner_area(cl) {
                    continue;
                }
                if check_clause_conditions(cl, &ctx).is_err() {
                    continue;
                }
                cond_ok_clauses.push(cl.clone());
            }
        }

        // 実行可能な宣言句をすべて収集（複数ある場合はプレイヤーに選択させる）
        let mut usable_clauses: Vec<String> = Vec::new();
        for cl in &cond_ok_clauses {
            if single_clause_legacy_used || !self.can_use_clause_this_turn(actor_id, cl) {
                continue;
            }
            let cost = parse_clause_cost(cl);
            let cost_actor = set_host_id.as_deref().unwrap_or(actor_id);
            let can_pay_set = self.check_and_pay_set_card_cost(player_id, cost_actor, cl, true);
            let can_pay = can_pay_set
                && self
                    .players
                    .get(player_id)
                    .map(|ps| self.can_pay_clause_cost(ps, cost_actor, &cost))
                    .unwrap_or(false);
            if can_pay {
                usable_clauses.push(cl.clone());
            }
        }

        if usable_clauses.is_empty() {
            self.log(format!("⚠️ [{}] の【宣言】条件またはコストを満たしていません", card_name));
            self.prompt_main_action(player_id);
            return;
        }
        if usable_clauses.len() >= 2 {
            let dec_id = self.generate_decision_id();
            self.pending_decision = Some(json!({
                "decisionId": dec_id,
                "revision": self.revision,
                "playerId": player_id,
                "kind": "modeSelect",
                "options": {
                    "specialKind": "DECLARE_CLAUSE_SELECT",
                    "actorId": actor_id,
                    "actorName": card_name,
                    "definitionId": def_id,
                    "isHandEventDeclare": is_hand_event_declare,
                    "setHostId": set_host_id,
                    "minPick": 1,
                    "maxPick": 1,
                    "modes": usable_clauses,
                    "resumeContext": "MAIN",
                    "title": format!("[{}] 使用する【宣言】効果を選択してください", card_name)
                }
            }));
            return;
        }

        let clause = usable_clauses.remove(0);
        self.execute_declare_clause(
            player_id,
            actor_id,
            &def_id,
            is_hand_event_declare,
            set_host_id,
            ori,
            &clause,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn execute_declare_clause(
        &mut self,
        player_id: &str,
        actor_id: &str,
        def_id: &str,
        is_hand_event_declare: bool,
        set_host_id: Option<String>,
        ori: CardOrientation,
        clause: &str,
    ) {
        let Some(def) = self.catalog.get(def_id).cloned() else {
            self.prompt_main_action(player_id);
            return;
        };
        let card_name = self.card_name_by_def(def_id);

        let cost = parse_clause_cost(&clause);
        let pay_auto = cost.hand_remove == 0 && cost.evidence_flip_up == 0 && cost.field_sleep == 0;
        self.last_cost_flipped_evidence_count = 0;
        let cost_actor = set_host_id.clone().unwrap_or_else(|| actor_id.to_string());
        // 証拠を表向きにするコストは後段のインタラクティブ選択（枚数分）で支払うため、ここでは自動で表向きにしない
        let face_down_for_cost = self
            .players
            .get(player_id)
            .map(|ps| ps.evidence.iter().filter(|e| !e.face_up).count())
            .unwrap_or(0);
        if cost.evidence_flip_up > 0 && face_down_for_cost < cost.evidence_flip_up {
            self.log(format!("⚠️ [{}] の【宣言】コストを満たしていません（裏向きの証拠が不足）", card_name));
            self.prompt_main_action(player_id);
            return;
        }
        let mut auto_cost = cost.clone();
        auto_cost.evidence_flip_up = 0;
        if !self.check_and_pay_set_card_cost(player_id, &cost_actor, &clause, false)
            || !self.pay_clause_cost_internal(player_id, &cost_actor, &auto_cost, pay_auto)
        {
            self.prompt_main_action(player_id);
            return;
        }

        // B10018 hand declare: keep hand card available as actor for SetThisEvent
        let _ = is_hand_event_declare;

        self.mark_clause_used_this_turn(actor_id, &clause);
        if def_id == "B08068" || def_id == "B08068P" {
            self.prompt_b08068_reveal_hand(player_id, actor_id);
            return;
        }
        if let Some(ps) = self.players.get_mut(player_id) {
            if let Some(ref mut p) = ps.partner {
                if p.instance_id == actor_id {
                    p.used_declare_this_turn = true;
                }
            }
            for ep in &mut ps.extra_partners {
                if ep.instance_id == actor_id {
                    ep.used_declare_this_turn = true;
                }
            }
            if let Some(ref mut cs) = ps.r#case {
                if cs.instance_id == actor_id {
                    cs.used_declare_this_turn = true;
                }
            }
            if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == actor_id) {
                c.used_declare_this_turn = true;
            }
            for c in &mut ps.field {
                if let Some(sc) = c.set_cards.iter_mut().find(|s| s.instance_id == actor_id) {
                    sc.used_declare_this_turn = true;
                }
            }
        }

        self.log(format!("⚡ {} が [{}] の【宣言】を発動！", player_id, card_name));
        let resolved_clause = resolve_state_branch(&clause, ori);
        let plan = parse_effect_steps(&resolved_clause, None, def.card_type);
        if cost.evidence_flip_up > 0 {
            let face_down_count = self
                .players
                .get(player_id)
                .map(|ps| ps.evidence.iter().filter(|e| !e.face_up).count())
                .unwrap_or(0);
            if face_down_count >= cost.evidence_flip_up {
                let max_flip = if cost.evidence_flip_up_any {
                    face_down_count
                } else {
                    cost.evidence_flip_up
                };
                let flip_step = crate::effects::TargetStep {
                    action: TargetAction::FlipEvidenceFaceUp,
                    filter: crate::effects::TargetFilter {
                        owner: crate::effects::OwnerScope::SelfOnly,
                        zone: crate::effects::TargetZone::Evidence,
                        count: max_flip,
                        up_to: cost.evidence_flip_up_any,
                        face_down_only: true,
                        ..Default::default()
                    },
                    bonuses: Vec::new(),
                    follow_up: Vec::new(),
                    conditional_follow_up: None,
                    optional: false,
                    then_abort_if_skipped: false,
                    scale_by_cost_flipped_evidence: false,
                };
                if self.prompt_target_step_with_plan(
                    player_id,
                    actor_id,
                    &card_name,
                    Some(def.level),
                    &flip_step,
                    None,
                    Some(&plan),
                    "MAIN",
                    None,
                ) {
                    return;
                }
            }
        }
        if cost.field_sleep > 0 {
            if let Some(mut sleep_filter) = cost.field_sleep_filter.clone() {
                sleep_filter.count = cost.field_sleep;
                let sleep_step = crate::effects::TargetStep {
                    action: TargetAction::Sleep,
                    filter: sleep_filter,
                    bonuses: Vec::new(),
                    follow_up: Vec::new(),
                    conditional_follow_up: None,
                    optional: false,
                    then_abort_if_skipped: true,
                    scale_by_cost_flipped_evidence: false,
                };
                if self.prompt_target_step_with_plan(
                    player_id,
                    actor_id,
                    &card_name,
                    Some(def.level),
                    &sleep_step,
                    None,
                    Some(&plan),
                    "MAIN",
                    None,
                ) {
                    return;
                }
            }
        }
        if cost.hand_remove > 0 {
            if self.prompt_hand_discard(
                player_id,
                actor_id,
                &card_name,
                Some(def.level),
                cost.hand_remove,
                cost.hand_filter.as_ref(),
                false,
                false,
                &[],
                None,
                None,
                Some(&plan),
                "MAIN",
                None,
            ) {
                return;
            }
        }
        if self.execute_clause_plan(player_id, actor_id, &card_name, Some(def.level), &plan, "MAIN", None) {
            return;
        }
        if self.check_field_overflow(player_id, "MAIN") {
            return;
        }
        self.prompt_main_action(player_id);
    }

    pub(super) fn handle_hint_play_answer(&mut self, player_id: &str, val: &Value) {
        let mut played_def_id = None;
        let mut played_inst_id = None;
        let mut is_event = false;
        if let Some(card_id) = val.get("cardId").and_then(|v| v.as_str()) {
            let turn = self.turn;
            // ターン内・カード名BAN（B06103 ジン等）の防御的再チェック
            {
                let (ban_def_id, banned) = {
                    let c_opt = self
                        .players
                        .get(player_id)
                        .and_then(|ps| ps.hand.iter().find(|c| c.instance_id == card_id));
                    let did = c_opt.map(|c| c.definition_id.clone()).unwrap_or_default();
                    let banned = match (c_opt, did.as_str()) {
                        (Some(c), d) => self.is_name_banned_this_turn(player_id, c, self.catalog.get(d).unwrap_or_else(|| panic!("catalog missing {d}"))),
                        _ => false,
                    };
                    (did, banned)
                };
                if banned {
                    self.log(format!("🚫 {} はこのターン中、〚カード名［{}］〛を使用・登場させられません", player_id, self.card_name_by_def(&ban_def_id)));
                    self.prompt_main_action(player_id);
                    return;
                }
            }
            if let Some(ps) = self.players.get_mut(player_id) {
                if let Some(idx) = ps.hand.iter().position(|c| c.instance_id == card_id) {
                    let def_id_peek = ps.hand[idx].definition_id.clone();
                    let card_type = self.catalog.get(&def_id_peek).map(|d| d.card_type).unwrap_or(CardType::CHARACTER);
                    let mut card = ps.hand.remove(idx);
                    played_def_id = Some(card.definition_id.clone());
                    played_inst_id = Some(card.instance_id.clone());
                    card.face_up = true;
                    if card_type == CardType::EVENT {
                        is_event = true;
                        card.zone = Zone::REMOVE;
                        ps.remove.push(card);
                        ps.normal_hand_played_this_turn = true;
                    } else {
                        card.zone = Zone::FIELD;
                        card.orientation = CardOrientation::ACTIVE;
                        card.entered_turn = turn;
                        self.field_overflow_excluded_ids.insert(card.instance_id.clone());
                        ps.field.push(card);
                    }
                }
            }
        }
        if let (Some(def_id), Some(inst_id)) = (played_def_id, played_inst_id) {
            let card_name = self.card_name_by_def(&def_id);
            let (card_lv, card_colors) = self
                .catalog
                .get(&def_id)
                .map(|d| (d.level, d.colors.clone()))
                .unwrap_or((0, vec![]));
            self.pending_hint_trigger = Some((player_id.to_string(), card_lv, card_colors.clone()));
            let need_target = if is_event {
                self.log(format!("📜 {} がイベント [{}] を使用しました", player_id, card_name));
                self.resolve_on_play_effects(player_id, &inst_id, &def_id, true)
            } else {
                self.log(format!("🃏 {} が [{}] を登場させました", player_id, card_name));
                self.resolve_character_enter_field_with_source(
                    player_id,
                    &inst_id,
                    &def_id,
                    CardOrientation::ACTIVE,
                    None,
                    "MAIN",
                )
            };
           if need_target {
               if let Some(ref mut pd) = self.pending_decision {
                   if let Some(opts) = pd.get_mut("options").and_then(|v| v.as_object_mut()) {
                       opts.insert("afterHintLevel".to_string(), json!(card_lv));
                   }
               }
               return;
           }
           if self.check_field_overflow(player_id, "MAIN") {
               if let Some(ref mut pd) = self.pending_decision {
                   if let Some(opts) = pd.get_mut("options").and_then(|v| v.as_object_mut()) {
                       opts.insert("afterHintLevel".to_string(), json!(card_lv));
                   }
               }
               return;
           }
           self.pending_hint_trigger = None;
           if self.trigger_on_hint_use_with_colors(player_id, card_lv, &card_colors) {
               return;
           }
       } else {
           self.log(format!("{} はヒントからの登場をスキップしました", player_id));
       }
       if self.check_field_overflow(player_id, "MAIN") {
           return;
       }
       self.prompt_main_action(player_id);
   }
}
