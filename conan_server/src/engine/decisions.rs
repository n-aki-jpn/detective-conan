use super::*;

impl MatchEngine {

    pub fn submit_decision(&mut self, player_id: &str, answer: &Value) -> Result<(), String> {
        let dec = match self.pending_decision.take() {
            Some(d) => d,
            None => return Err("No pending decision".to_string()),
        };

        let dec_player = dec["playerId"].as_str().unwrap_or("").to_string();
        if dec_player != player_id {
            self.pending_decision = Some(dec);
            return Err(format!("Expected decision from {}, got {}", dec_player, player_id));
        }

        let kind = dec["kind"].as_str().unwrap_or("").to_string();
        let val = answer.get("value").cloned().unwrap_or(json!({}));

        self.revision += 1;

        match kind.as_str() {
            "mulligan" => self.handle_mulligan_answer(player_id, &val),
            "mainAction" => self.handle_main_action(player_id, &val),
            "hintPlay" => self.handle_hint_play_answer(player_id, &val),
            "guard" => self.handle_guard_answer(player_id, &val),
            "cutin" => self.handle_cutin_answer(player_id, &val),
            "targetSelect" => self.handle_target_select_answer(player_id, &val, &dec),
            "deckSearch" => self.handle_deck_search_answer(player_id, &val, &dec),
            "modeSelect" => self.handle_mode_select_answer(player_id, &val, &dec),
            "triggerOrder" => self.handle_trigger_order_answer(player_id, &val),
            _ => return Err(format!("Unknown decision kind: {}", kind)),
        }

        Ok(())
    }

    pub(super) fn start_event_mode_choice(
        &mut self,
        player_id: &str,
        actor_id: &str,
        actor_name: &str,
        source_level: Option<i32>,
        mc: &EventModeChoice,
        resume_context: &str,
    ) -> bool {
        if !mc.pre_clauses.is_empty() {
            let mut rest_mc = mc.clone();
            rest_mc.pre_clauses.clear();
            let mut pre_tasks: Vec<PendingClauseTask> = mc
                .pre_clauses
                .iter()
                .map(|cl| PendingClauseTask {
                    player_id: player_id.to_string(),
                    actor_id: actor_id.to_string(),
                    actor_name: actor_name.to_string(),
                    source_level,
                    card_type: CardType::EVENT,
                    clause: cl.clone(),
                    resume_context: resume_context.to_string(),
                    evidence_source_inst_id: None,
                    post_target_ops: vec![],
                    plan: None,
                })
                .collect();
            let mc_plan = ParsedClausePlan {
                immediate_before: vec![ImmediateOp::EventModeChoice(rest_mc)],
                ..Default::default()
            };
            pre_tasks.push(PendingClauseTask {
                player_id: player_id.to_string(),
                actor_id: actor_id.to_string(),
                actor_name: actor_name.to_string(),
                source_level,
                card_type: CardType::EVENT,
                clause: String::new(),
                resume_context: resume_context.to_string(),
                evidence_source_inst_id: None,
                post_target_ops: vec![],
                plan: Some(mc_plan),
            });
            for t in pre_tasks.into_iter().rev() {
                self.pending_clause_queue.insert(0, t);
            }
            return self.drain_pending_clause_queue();
        }
        self.execute_event_mode_choice_body(player_id, actor_id, actor_name, source_level, mc, resume_context)
    }

    pub(super) fn execute_event_mode_choice_body(
        &mut self,
        player_id: &str,
        actor_id: &str,
        actor_name: &str,
        source_level: Option<i32>,
        mc: &EventModeChoice,
        resume_context: &str,
    ) -> bool {
        let ctx = self.build_cond_info(player_id, Some(actor_id), CardOrientation::ACTIVE, source_level);
        if let Some(ref gate) = mc.mode_gate_condition {
            if check_clause_conditions(gate, &ctx).is_err() || !eval_cond_phrase(gate, &ctx) {
                self.log(format!("⚠️ [{}] のモード発動条件を満たしていません", actor_name));
                return false;
            }
        }
        // 全て行う代替条件チェック
        if let Some(ref all_cond) = mc.all_condition {
            if check_clause_conditions(all_cond, &ctx).is_ok() && eval_cond_phrase(all_cond, &ctx) {
                self.log(format!("✨ [{}] の条件達成により選択肢をすべて上から順に行います！", actor_name));
                self.enqueue_mode_clauses(player_id, actor_id, actor_name, source_level, &mc.options, &mc.post_clauses, resume_context);
                return self.drain_pending_clause_queue();
            }
        }
        // 任意コストで全て行う（江戸川コナンをスリープ / アンドレ・キャメルを手札リムーブ）
        if let Some(ref opt_cost_text) = mc.optional_all_cost {
            let can_pay_opt = self.can_pay_optional_mode_cost(player_id, opt_cost_text);
            if can_pay_opt {
                let dec_id = self.generate_decision_id();
                self.pending_decision = Some(json!({
                    "decisionId": dec_id,
                    "revision": self.revision,
                    "playerId": player_id,
                    "kind": "modeSelect",
                    "options": {
                        "specialKind": "OPTIONAL_ALL_COST",
                        "actorId": actor_id,
                        "actorName": actor_name,
                        "sourceLevel": source_level,
                        "optionalCostText": opt_cost_text,
                        "eventModeChoice": mc,
                        "minPick": 1,
                        "maxPick": 1,
                        "modes": [
                            format!("コストを支払って全て行う（{}）", opt_cost_text),
                            "通常通り1つ選んで行う"
                        ],
                        "resumeContext": resume_context,
                        "title": format!("[{}] 追加コストを支払って全ての効果を行いますか？", actor_name)
                    }
                }));
                return true;
            }
        }

        let mut max_pick = mc.max_pick;
        let mut min_pick = mc.min_pick;
        if mc.distinct_colors_limit {
            let mut distinct_cols = HashSet::new();
            if let Some(ps) = self.players.get(player_id) {
                for c in &ps.field {
                    if let Some(def) = self.catalog.get(&c.definition_id) {
                        for col in normalize_color_set(&def.colors) {
                            distinct_cols.insert(col);
                        }
                    }
                }
            }
            max_pick = distinct_cols.len().min(mc.options.len());
            if max_pick == 0 {
                self.log(format!("⚠️ [{}] 現場にキャラの色が存在しないため不発になりました", actor_name));
                return false;
            }
            min_pick = 0;
        }
        max_pick = max_pick.min(mc.options.len()).max(1);
        min_pick = min_pick.min(max_pick);

        let dec_id = self.generate_decision_id();
        self.pending_decision = Some(json!({
            "decisionId": dec_id,
            "revision": self.revision,
            "playerId": player_id,
            "kind": "modeSelect",
            "options": {
                "actorId": actor_id,
                "actorName": actor_name,
                "sourceLevel": source_level,
                "cardType": "EVENT",
                "modes": mc.options,
                "postClauses": mc.post_clauses,
                "minPick": min_pick,
                "maxPick": max_pick,
                "resumeContext": resume_context,
                "title": if max_pick > 1 {
                    format!("[{}] の効果を{}つまで選んでください", actor_name, max_pick)
                } else {
                    format!("[{}] の選択効果を選んでください", actor_name)
                }
            }
        }));
        true
    }

    pub(super) fn can_pay_optional_mode_cost(&self, player_id: &str, opt_cost_text: &str) -> bool {
        let Some(ps) = self.players.get(player_id) else {
            return false;
        };
        let target_name = extract_bracketed_value(opt_cost_text, "カード名［", "］");
        if opt_cost_text.contains("スリープさせてもよい") {
            return ps.field.iter().any(|c| {
                c.orientation == CardOrientation::ACTIVE
                    && target_name
                        .as_ref()
                        .map(|n| self.catalog.get(&c.definition_id).map(|d| has_instance_name_match(c, d, n)).unwrap_or(false))
                        .unwrap_or(true)
            });
        }
        if opt_cost_text.contains("手札から") && opt_cost_text.contains("リムーブしてもよい") {
            return ps.hand.iter().any(|c| {
                target_name
                    .as_ref()
                    .map(|n| self.catalog.get(&c.definition_id).map(|d| has_instance_name_match(c, d, n)).unwrap_or(false))
                    .unwrap_or(true)
            });
        }
        false
    }

    pub(super) fn pay_optional_mode_cost(&mut self, player_id: &str, opt_cost_text: &str) -> bool {
        let target_name = extract_bracketed_value(opt_cost_text, "カード名［", "］");
        let mut paid_name = None;
        if let Some(ps) = self.players.get_mut(player_id) {
            if opt_cost_text.contains("スリープさせてもよい") {
                if let Some(c) = ps.field.iter_mut().find(|c| {
                    c.orientation == CardOrientation::ACTIVE
                        && target_name
                            .as_ref()
                            .map(|n| self.catalog.get(&c.definition_id).map(|d| has_instance_name_match(c, d, n)).unwrap_or(false))
                            .unwrap_or(true)
                }) {
                    c.orientation = CardOrientation::SLEEP;
                    paid_name = self.catalog.get(&c.definition_id).and_then(|d| d.names.first().cloned());
                }
            } else if opt_cost_text.contains("手札から") && opt_cost_text.contains("リムーブしてもよい") {
                if let Some(pos) = ps.hand.iter().position(|c| {
                    target_name
                        .as_ref()
                        .map(|n| self.catalog.get(&c.definition_id).map(|d| has_instance_name_match(c, d, n)).unwrap_or(false))
                        .unwrap_or(true)
                }) {
                    let mut hc = ps.hand.remove(pos);
                    hc.zone = Zone::REMOVE;
                    hc.face_up = true;
                    paid_name = self.catalog.get(&hc.definition_id).and_then(|d| d.names.first().cloned());
                    ps.remove.push(hc);
                }
            }
        }
        if let Some(pn) = paid_name {
            self.log(format!("💸 追加コスト支払い: [{}] ({})", pn, opt_cost_text));
            true
        } else {
            false
        }
    }

    pub(super) fn enqueue_mode_clauses(
        &mut self,
        player_id: &str,
        actor_id: &str,
        actor_name: &str,
        source_level: Option<i32>,
        chosen_modes: &[String],
        post_clauses: &[String],
        resume_context: &str,
    ) {
        let mut tasks = Vec::new();
        for m in chosen_modes {
            tasks.push(PendingClauseTask {
                player_id: player_id.to_string(),
                actor_id: actor_id.to_string(),
                actor_name: actor_name.to_string(),
                source_level,
                card_type: CardType::EVENT,
                clause: m.clone(),
                resume_context: resume_context.to_string(),
                evidence_source_inst_id: None,
                post_target_ops: vec![],
                plan: None,
            });
        }
        for pc in post_clauses {
            tasks.push(PendingClauseTask {
                player_id: player_id.to_string(),
                actor_id: actor_id.to_string(),
                actor_name: actor_name.to_string(),
                source_level,
                card_type: CardType::EVENT,
                clause: pc.clone(),
                resume_context: resume_context.to_string(),
                evidence_source_inst_id: None,
                post_target_ops: vec![],
                plan: None,
            });
        }
        for t in tasks.into_iter().rev() {
            self.pending_clause_queue.insert(0, t);
        }
    }

    pub(super) fn handle_mode_select_answer(&mut self, player_id: &str, val: &Value, dec: &Value) {
        let opts = dec.get("options").cloned().unwrap_or(json!({}));
        let actor_id = opts.get("actorId").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let actor_name = opts.get("actorName").and_then(|v| v.as_str()).unwrap_or("効果").to_string();
        let source_level = opts.get("sourceLevel").and_then(|v| v.as_i64()).map(|n| n as i32);
        let resume_context = opts.get("resumeContext").and_then(|v| v.as_str()).unwrap_or("MAIN").to_string();
        let special_kind = opts.get("specialKind").and_then(|v| v.as_str()).unwrap_or("");
        let modes: Vec<String> = opts
            .get("modes")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default();

        let single_idx = val
            .get("modeIndex")
            .or_else(|| val.get("mode"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;

        match special_kind {
            "DECK_TOP_OR_BOTTOM" => {
                let target_ids: Vec<String> = opts
                    .get("targetIds")
                    .and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
                    .unwrap_or_default();
                let to_top = single_idx == 0;
                let pos_label = if to_top { "上" } else { "下" };
                let p_ids: Vec<String> = self.players.keys().cloned().collect();
                for tid in target_ids {
                    for pid in &p_ids {
                        let mut found_card = None;
                        if let Some(ps) = self.players.get_mut(pid) {
                            if let Some(pos) = ps.field.iter().position(|c| c.instance_id == tid) {
                                found_card = Some(ps.field.remove(pos));
                            }
                        }
                        if let Some(mut card) = found_card {
                            let cname = self.card_name_by_def(&card.definition_id);
                            card.zone = Zone::DECK;
                            card.face_up = false;
                            card.orientation = CardOrientation::ACTIVE;
                            if let Some(ps) = self.players.get_mut(pid) {
                                for mut sc in card.set_cards.drain(..) {
                                    sc.zone = Zone::REMOVE;
                                    sc.face_up = true;
                                    ps.remove.push(sc);
                                }
                                if to_top {
                                    ps.deck.push(card);
                                } else {
                                    ps.deck.insert(0, card);
                                }
                            }
                            self.log(format!("📚 [{}] の効果で [{}] をデッキの{}に移しました！", actor_name, cname, pos_label));
                            break;
                        }
                    }
                }
                if self.drain_pending_clause_queue() {
                    return;
                }
                self.resume_after_decision(player_id, &resume_context);
                return;
            }
            "DECLARE_CLAUSE_SELECT" => {
                if let Some(chosen_clause) = modes.get(single_idx).cloned() {
                    let def_id = opts
                        .get("definitionId")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let is_hand_event_declare = opts
                        .get("isHandEventDeclare")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let set_host_id = opts
                        .get("setHostId")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let ori = self.find_actor_orientation(
                        player_id,
                        set_host_id.as_deref().unwrap_or(&actor_id),
                    );
                    self.execute_declare_clause(
                        player_id,
                        &actor_id,
                        &def_id,
                        is_hand_event_declare,
                        set_host_id,
                        ori,
                        &chosen_clause,
                    );
                } else {
                    self.prompt_main_action(player_id);
                }
                return;
            }
            "OPTIONAL_ALL_COST" => {
                if let Ok(mut mc) = serde_json::from_value::<EventModeChoice>(opts.get("eventModeChoice").cloned().unwrap_or(json!({}))) {
                    let cost_text = opts.get("optionalCostText").and_then(|v| v.as_str()).unwrap_or("");
                    if single_idx == 0 && self.pay_optional_mode_cost(player_id, cost_text) {
                        self.enqueue_mode_clauses(player_id, &actor_id, &actor_name, source_level, &mc.options, &mc.post_clauses, &resume_context);
                        if self.drain_pending_clause_queue() {
                            return;
                        }
                    } else {
                        mc.optional_all_cost = None;
                        if self.execute_event_mode_choice_body(player_id, &actor_id, &actor_name, source_level, &mc, &resume_context) {
                            return;
                        }
                    }
                }
                self.resume_after_decision(player_id, &resume_context);
                return;
            }
            "OPTIONAL_DECK_TOP_REMOVE" => {
                if single_idx == 0 {
                    let count = opts.get("count").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                    let then_clause = opts.get("thenClause").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let threshold = opts.get("blackCutinThreshold").and_then(|v| serde_json::from_value::<(usize, String, Option<String>)>(v.clone()).ok());
                    let mut milled = vec![];
                    if let Some(ps) = self.players.get_mut(player_id) {
                        for _ in 0..count {
                            if let Some(mut d) = ps.deck.pop() {
                                d.zone = Zone::REMOVE;
                                d.face_up = true;
                                milled.push(d.definition_id.clone());
                                ps.remove.push(d);
                            }
                        }
                    }
                    // 任意リムーブ（コスト）で0枚になった瞬間に即座へリフレッシュ（docs/rules.md §リフレッシュ）
                    if !milled.is_empty() {
                        self.refresh_if_deck_emptied(player_id);
                    }
                    let mut black_cutin_cnt = 0usize;
                    for did in &milled {
                        if let Some(d) = self.catalog.get(did) {
                            if normalize_color_set(&d.colors).contains("黒") && d.description.contains("【カットイン】") {
                                black_cutin_cnt += 1;
                            }
                        }
                    }
                    self.last_removed_black_cutin_count = black_cutin_cnt;
                    self.log(format!("🗑️ [{}] 任意コスト: 山札から{}枚リムーブ（黒カットイン: {}枚）", actor_name, milled.len(), black_cutin_cnt));
                    if let Some((th, th_clause, else_cl)) = threshold {
                        if black_cutin_cnt >= th {
                            self.enqueue_mode_clauses(player_id, &actor_id, &actor_name, source_level, &[th_clause], &[], &resume_context);
                        } else if let Some(ecl) = else_cl {
                            self.enqueue_mode_clauses(player_id, &actor_id, &actor_name, source_level, &[ecl], &[], &resume_context);
                        }
                    } else if !then_clause.is_empty() {
                        self.enqueue_mode_clauses(player_id, &actor_id, &actor_name, source_level, &[then_clause], &[], &resume_context);
                    }
                }
                if self.drain_pending_clause_queue() {
                    return;
                }
                self.resume_after_decision(player_id, &resume_context);
                return;
            }
            "OPTIONAL_COMPOUND_COST" => {
                if single_idx == 0 {
                    let sleep_partner = opts.get("sleepPartner").and_then(|v| v.as_bool()).unwrap_or(false);
                    let hand_remove = opts.get("handRemove").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                    let file_top_remove = opts.get("fileTopRemove").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                    let field_remove_name = opts.get("fieldRemoveName").and_then(|v| v.as_str()).map(|s| s.to_string());
                    let then_clause = opts.get("thenClause").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let mut ok = true;
                    if let Some(ps) = self.players.get_mut(player_id) {
                        if sleep_partner {
                            if let Some(ref mut p) = ps.partner {
                                if p.zone == Zone::PARTNER && p.orientation == CardOrientation::ACTIVE {
                                    p.orientation = CardOrientation::SLEEP;
                                } else {
                                    ok = false;
                                }
                            } else {
                                ok = false;
                            }
                        }
                        if ok && hand_remove > 0 {
                            if ps.hand.len() >= hand_remove {
                                for _ in 0..hand_remove {
                                    if let Some(mut c) = ps.hand.pop() {
                                        c.zone = Zone::REMOVE;
                                        c.face_up = true;
                                        ps.remove.push(c);
                                    }
                                }
                            } else {
                                ok = false;
                            }
                        }
                        if ok && file_top_remove > 0 {
                            if ps.file.len() >= file_top_remove {
                                for _ in 0..file_top_remove {
                                    if let Some(mut f) = ps.file.pop() {
                                        f.zone = Zone::REMOVE;
                                        f.face_up = true;
                                        ps.remove.push(f);
                                    }
                                }
                            } else {
                                ok = false;
                            }
                        }
                        if ok {
                            if let Some(ref fname) = field_remove_name {
                                if let Some(pos) = ps.field.iter().position(|fc| {
                                    self.catalog.get(&fc.definition_id).map(|d| has_instance_name_match(fc, d, fname)).unwrap_or(false)
                                }) {
                                    let mut rc = ps.field.remove(pos);
                                    rc.zone = Zone::REMOVE;
                                    rc.face_up = true;
                                    ps.remove.push(rc);
                                } else {
                                    ok = false;
                                }
                            }
                        }
                    }
                    if ok && !then_clause.is_empty() {
                        self.log(format!("💸 [{}] 複合コストを支払いました", actor_name));
                        self.enqueue_mode_clauses(player_id, &actor_id, &actor_name, source_level, &[then_clause], &[], &resume_context);
                    }
                }
                if self.drain_pending_clause_queue() {
                    return;
                }
                self.resume_after_decision(player_id, &resume_context);
                return;
            }
            "OPTIONAL_FILE_TO_HAND" => {
                if single_idx == 0 {
                    let count = opts.get("count").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
                    let then_clause = opts.get("thenClause").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let mut moved = 0;
                    if let Some(ps) = self.players.get_mut(player_id) {
                        for _ in 0..count {
                            if let Some(mut f) = ps.file.pop() {
                                f.zone = Zone::HAND;
                                f.face_up = true;
                                ps.hand.push(f);
                                moved += 1;
                            }
                        }
                    }
                    if moved > 0 {
                        self.log(format!("📥 [{}] FILEから{}枚を手札に加えました", actor_name, moved));
                        if !then_clause.is_empty() {
                            self.enqueue_mode_clauses(player_id, &actor_id, &actor_name, source_level, &[then_clause], &[], &resume_context);
                        }
                    }
                }
                if self.drain_pending_clause_queue() {
                    return;
                }
                self.resume_after_decision(player_id, &resume_context);
                return;
            }
            "OPTIONAL_GIVE_OPP_EVIDENCE" => {
                if single_idx == 0 {
                    let count = opts.get("count").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
                    let opp_id = self.opponent_id_of(player_id);
                    self.gain_evidence(&opp_id, count);
                    self.log(format!("📜 [{}] 相手に証拠を{}つ与えました", actor_name, count));
                }
                if self.drain_pending_clause_queue() {
                    return;
                }
                self.resume_after_decision(player_id, &resume_context);
                return;
            }
            "INVESTIGATE_NAME_SELECT" => {
                let guessed_name = modes.get(single_idx).cloned().unwrap_or_default();
                let count = opts.get("count").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
                let then_clause = opts.get("thenClause").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let opp_id = self.opponent_id_of(player_id);
                let mut found = false;
                let mut revealed_names = Vec::new();
                let mut revealed_def_ids = Vec::new();
                if let Some(opp_ps) = self.players.get_mut(&opp_id) {
                    let mut pulled = Vec::new();
                    for _ in 0..count {
                        if let Some(mut d) = opp_ps.deck.pop() {
                            d.face_up = true;
                            if let Some(def) = self.catalog.get(&d.definition_id) {
                                if has_instance_name_match(&d, def, &guessed_name) {
                                    found = true;
                                }
                                revealed_def_ids.push(d.definition_id.clone());
                            }
                            d.face_up = false;
                            pulled.push(d);
                        }
                    }
                    for d in pulled.into_iter().rev() {
                        opp_ps.deck.insert(0, d);
                    }
                }
                let mut candidates = Vec::new();
                for (r_idx, did) in revealed_def_ids.iter().enumerate() {
                    let def_opt = self.catalog.get(did);
                    let lv = def_opt.map(|d| d.level).unwrap_or(0);
                    let cname = self.card_name_by_def(did);
                    revealed_names.push(format!("{}(Lv.{})", cname, lv));
                    candidates.push(json!({
                        "instanceId": format!("sousaku_{}", r_idx),
                        "definitionId": did,
                        "name": cname,
                        "level": lv,
                        "ap": def_opt.map(|d| d.ap).unwrap_or(0),
                        "colors": def_opt.map(|d| d.colors.clone()).unwrap_or_default(),
                        "traits": def_opt.map(|d| d.traits.clone()).unwrap_or_default(),
                        "orientation": "ACTIVE",
                        "faceUp": true,
                        "isOpponent": true,
                        "zoneLabel": "相手山札",
                    }));
                }
                self.log(format!(
                    "🔎 [{}] 〚捜査{}〛 指定名「{}」 ➔ 公開: [{}] ({})",
                    actor_name,
                    count,
                    guessed_name,
                    revealed_names.join(", "),
                    if found { "発見！" } else { "見つからず" }
                ));
                if !candidates.is_empty() {
                    let dec_id = self.generate_decision_id();
                    self.pending_decision = Some(json!({
                        "decisionId": dec_id,
                        "revision": self.revision,
                        "playerId": player_id,
                        "kind": "targetSelect",
                        "options": {
                            "actorId": actor_id,
                            "actorName": actor_name,
                            "sourceLevel": source_level,
                            "effectType": "VIEW_SOUSAKU_CARDS",
                            "targetZone": "DECK",
                            "maxSelect": 0,
                            "optional": true,
                            "title": format!("〚捜査{}〛めくったカード: {}（確認後、決定/スキップで次へ）", count, revealed_names.join(", ")),
                            "candidates": candidates,
                            "sousakuFound": found,
                            "sousakuThenClause": then_clause,
                            "resumeContext": resume_context,
                        }
                    }));
                    return;
                }
                if found && !then_clause.is_empty() {
                    self.enqueue_mode_clauses(player_id, &actor_id, &actor_name, source_level, &[then_clause], &[], &resume_context);
                }
                if self.drain_pending_clause_queue() {
                    return;
                }
                self.resume_after_decision(player_id, &resume_context);
                return;
            }
            "CHOICE_RUSH_OR_ACTIVE" => {
                let tid = opts.get("targetId").and_then(|v| v.as_str()).unwrap_or("");
                if let Some(ps) = self.players.get_mut(player_id) {
                    if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == tid) {
                        if single_idx == 0 {
                            c.has_rush = true;
                            c.rush_target = Some("CHAR".to_string());
                            self.log(format!("⚡ [{}] で対象のキャラに〚突撃［キャラ］〛を付与しました！", actor_name));
                        } else {
                            c.can_attack_active = true;
                            self.log(format!("⚡ [{}] で対象のキャラにアクティブ攻撃能力を付与しました！", actor_name));
                        }
                    }
                }
                if self.drain_pending_clause_queue() {
                    return;
                }
                self.resume_after_decision(player_id, &resume_context);
                return;
            }
            _ => {}
        }

        // 通常の単一/複数モード選択
        let post_clauses: Vec<String> = opts
            .get("postClauses")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        let max_pick = opts.get("maxPick").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
        let mut chosen_indices: Vec<usize> = if let Some(arr) = val.get("modes").and_then(|v| v.as_array()) {
            let mut v = Vec::new();
            for item in arr {
                if let Some(i) = item.as_u64().map(|n| n as usize) {
                    if i < modes.len() && !v.contains(&i) {
                        v.push(i);
                    }
                }
            }
            v
        } else if val.get("modes").is_some() {
            vec![]
        } else {
            vec![single_idx]
        };
        chosen_indices.truncate(max_pick.max(1));
        chosen_indices.sort_unstable();

        let chosen_texts: Vec<String> = chosen_indices
            .iter()
            .filter_map(|&i| modes.get(i).cloned())
            .collect();
        for txt in &chosen_texts {
            self.log(format!("💡 [{}] の選択効果: {}", actor_name, txt));
        }
        self.enqueue_mode_clauses(
            player_id,
            &actor_id,
            &actor_name,
            source_level,
            &chosen_texts,
            &post_clauses,
            &resume_context,
        );
        if self.drain_pending_clause_queue() {
            return;
        }
        self.resume_after_decision(player_id, &resume_context);
    }

    pub(super) fn handle_target_select_answer(&mut self, player_id: &str, val: &Value, dec: &Value) {
        let target_id = val.get("targetId").and_then(|v| v.as_str());
        let opts = dec.get("options").cloned().unwrap_or_else(|| json!({}));

        let actor_id = opts
            .get("actorId")
            .and_then(|v| v.as_str())
            .or_else(|| val.get("actorId").and_then(|v| v.as_str()))
            .unwrap_or("")
            .to_string();
        let effect_type = opts
            .get("effectType")
            .and_then(|v| v.as_str())
            .or_else(|| val.get("effectType").and_then(|v| v.as_str()))
            .unwrap_or("")
            .to_string();
        let target_zone = opts.get("targetZone").and_then(|v| v.as_str()).unwrap_or("FIELD").to_string();
        let source_level = opts.get("sourceLevel").and_then(|v| v.as_i64()).map(|n| n as i32);
        let resume_context = opts.get("resumeContext").and_then(|v| v.as_str()).unwrap_or("MAIN").to_string();
        let after_hint_level = opts.get("afterHintLevel").and_then(|v| v.as_i64()).map(|n| n as i32);

        let actor_name = opts
            .get("actorName")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| self.card_name_by_inst(&actor_id));
        let cand_map: HashMap<&str, &Value> = opts
            .get("candidates")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|c| c.get("instanceId").and_then(|s| s.as_str()).map(|id| (id, c)))
                    .collect()
            })
            .unwrap_or_default();
        let distinct_names = opts.get("distinctNames").and_then(|v| v.as_bool()).unwrap_or(false);
        let distinct_levels = opts.get("distinctLevels").and_then(|v| v.as_bool()).unwrap_or(false);
        let distinct_colors = opts.get("distinctColors").and_then(|v| v.as_bool()).unwrap_or(false);
        let max_total_level = opts.get("maxTotalLevel").and_then(|v| v.as_i64());

        let max_select = opts.get("maxSelect").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
        let abort_on_skip = opts.get("abortOnSkip").and_then(|v| v.as_bool()).unwrap_or(false);
        let mut selected_ids: Vec<String> = Vec::new();
        if let Some(arr) = val.get("targetIds").and_then(|v| v.as_array()) {
            for item in arr {
                if let Some(s) = item.as_str() {
                    if !selected_ids.iter().any(|x| x == s) {
                        selected_ids.push(s.to_string());
                    }
                }
            }
        } else if let Some(tid) = target_id {
            selected_ids.push(tid.to_string());
        }
        selected_ids.truncate(max_select.max(1));

        let mut used_names: HashSet<String> = HashSet::new();
        let mut used_levels: HashSet<i64> = HashSet::new();
        let mut used_colors: HashSet<String> = HashSet::new();
        let mut sum_level: i64 = 0;

        let mut valid_chosen: Vec<String> = Vec::new();
        for id in selected_ids {
            if !cand_map.is_empty() && !cand_map.contains_key(id.as_str()) {
                continue;
            }
            if let Some(&cand) = cand_map.get(id.as_str()) {
                let cnames: Vec<String> = match cand.get("cardNames").and_then(|v| v.as_array()) {
                    Some(arr) => arr
                        .iter()
                        .filter_map(|x| x.as_str().map(|s| s.to_string()))
                        .collect(),
                    None => {
                        let raw = cand.get("name").and_then(|v| v.as_str()).unwrap_or("");
                        extract_distinct_card_names(raw)
                    }
                };
                let clv = cand.get("level").and_then(|v| v.as_i64()).unwrap_or(0);
                let ccols: Vec<String> = cand
                    .get("colors")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|x| x.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();

                if distinct_names && !cnames.is_empty() && cnames.iter().any(|n| used_names.contains(n)) {
                    continue;
                }
                if distinct_levels && used_levels.contains(&clv) {
                    continue;
                }
                if distinct_colors && ccols.iter().any(|col| used_colors.contains(col)) {
                    continue;
                }
                if let Some(cap) = max_total_level {
                    if sum_level + clv > cap {
                        continue;
                    }
                }

                if distinct_names {
                    for n in cnames {
                        used_names.insert(n);
                    }
                }
                if distinct_levels {
                    used_levels.insert(clv);
                }
                if distinct_colors {
                    for col in ccols {
                        used_colors.insert(col);
                    }
                }
                sum_level += clv;
            }
            valid_chosen.push(id);
        }

        let mut deployed_list: Vec<(String, String, CardOrientation)> = Vec::new();
        let mut chosen_def_for_cond: Option<String> = None;
        let mut applied = false;

        if effect_type == "TOP_OR_BOTTOM_DECK" {
            if !valid_chosen.is_empty() {
                let dec_id = self.generate_decision_id();
                self.pending_decision = Some(json!({
                    "decisionId": dec_id,
                    "revision": self.revision,
                    "playerId": player_id,
                    "kind": "modeSelect",
                    "options": {
                        "specialKind": "DECK_TOP_OR_BOTTOM",
                        "actorId": actor_id,
                        "actorName": actor_name,
                        "sourceLevel": source_level,
                        "targetIds": valid_chosen,
                        "modes": ["デッキの上に移す", "デッキの下に移す"],
                        "resumeContext": resume_context,
                        "title": format!("[{}] 選んだカードをデッキの上か下に移してください", actor_name),
                    }
                }));
                return;
            }
            if self.drain_pending_clause_queue() {
                return;
            }
            self.resume_after_decision(player_id, &resume_context);
            return;
        } else if effect_type == "FORCE_CONTACT_PICK_ALLY" {
            let tid = opts
                .get("forcedContactTargetId")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if let Some(c_actor_id) = valid_chosen.into_iter().next() {
                let opp_id = self.opponent_id_of(player_id);
                let c_actor_name = self.card_name_by_inst(&c_actor_id);
                let target_name = self.card_name_by_inst(&tid);
                self.log(format!(
                    "⚔️ [{}] の効果で [{}] と [{}] のコンタクト（バトル）が発生！",
                    actor_name, c_actor_name, target_name
                ));
                self.contact_ap_buffs.clear();
                self.action_context = Some(json!({
                    "attackerPlayerId": player_id,
                    "defenderPlayerId": opp_id,
                    "actorId": c_actor_id,
                    "targetId": tid,
                    "attackerPass": false,
                    "defenderPass": false,
                    "cutinUsed": false,
                    "isForcedContact": true
                }));
                self.trigger_on_contact(player_id, &c_actor_id, &opp_id, &tid);
                self.proceed_to_action_resolution(None);
                return;
            }
            self.prompt_main_action(player_id);
            return;
        } else if effect_type == "VIEW_SOUSAKU_CARDS" {
            let found = opts.get("sousakuFound").and_then(|v| v.as_bool()).unwrap_or(false);
            let then_clause = opts
                .get("sousakuThenClause")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if found && !then_clause.is_empty() {
                self.enqueue_mode_clauses(
                    player_id,
                    &actor_id,
                    &actor_name,
                    source_level,
                    &[then_clause],
                    &[],
                    &resume_context,
                );
            }
            if self.drain_pending_clause_queue() {
                return;
            }
            self.resume_after_decision(player_id, &resume_context);
            return;
        } else if effect_type == "B08068_REVEAL_HAND" {
            let revealed_count = valid_chosen.len();
            if revealed_count > 0 {
                let names: Vec<String> = valid_chosen.iter().map(|id| self.card_name_by_inst(id)).collect();
                self.log(format!(
                    "👁️ [{}] のコストで手札から [{}] を公開しました（計 {} 枚）",
                    actor_name,
                    names.join("], ["),
                    revealed_count
                ));
            } else {
                self.log(format!("👁️ [{}] のコストで手札を公開しませんでした（0枚）", actor_name));
            }
            self.resolve_b08068_remove(player_id, &actor_id, revealed_count);
            return;
        } else if effect_type == "DISCARD_HAND" {
            let reveal_only = opts.get("revealOnly").and_then(|v| v.as_bool()).unwrap_or(false);
            let also_orient = opts.get("alsoOrientActor").and_then(|v| v.as_str());
            if valid_chosen.is_empty() {
                if reveal_only {
                    self.log(format!("{} は手札を公開しませんでした", player_id));
                } else {
                    self.log(format!("{} は手札をリムーブしませんでした", player_id));
                }
            } else {
                let mut discarded_defs = Vec::new();
                let mut removed_lv_sum = 0i32;
                let mut removed_ap_sum = 0i32;
                if reveal_only {
                    if let Some(ps) = self.players.get(player_id) {
                        for cid in &valid_chosen {
                            if let Some(c) = ps.hand.iter().find(|c| &c.instance_id == cid) {
                                discarded_defs.push(c.definition_id.clone());
                            }
                        }
                    }
                    if !discarded_defs.is_empty() {
                        applied = true;
                        for did in discarded_defs {
                            let cname = self.card_name_by_def(&did);
                            self.log(format!("👁️ [{}] のコスト: {} が手札の [{}] を公開しました", actor_name, player_id, cname));
                        }
                    }
                } else {
                    if let Some(ps) = self.players.get_mut(player_id) {
                        for cid in &valid_chosen {
                            if let Some(pos) = ps.hand.iter().position(|c| &c.instance_id == cid) {
                                let mut c = ps.hand.remove(pos);
                                c.zone = Zone::REMOVE;
                                c.face_up = true;
                                if let Some(d) = self.catalog.get(&c.definition_id) {
                                    removed_lv_sum += d.level;
                                    removed_ap_sum += d.ap;
                                }
                                discarded_defs.push(c.definition_id.clone());
                                ps.remove.push(c);
                            }
                        }
                    }
                    if !discarded_defs.is_empty() {
                        applied = true;
                        self.last_chosen_level = Some(removed_lv_sum);
                        self.last_removed_level_sum = removed_lv_sum;
                        self.last_removed_ap_sum = removed_ap_sum;
                        for did in discarded_defs {
                            let cname = self.card_name_by_def(&did);
                            self.log(format!("💸 [{}] の効果/コスト: {} が手札から [{}] をリムーブしました", actor_name, player_id, cname));
                        }
                    }
                }
                if applied && also_orient == Some("SLEEP") {
                    if let Some(ps) = self.players.get_mut(player_id) {
                        if let Some(ac) = ps
                            .field
                            .iter_mut()
                            .find(|c| c.instance_id == actor_id)
                            .or_else(|| ps.partner.as_mut().filter(|p| p.instance_id == actor_id))
                        {
                            ac.orientation = CardOrientation::SLEEP;
                            self.log(format!("💤 [{}] をコストでスリープさせました", actor_name));
                        }
                    }
                }
            }
        } else if effect_type == "REVIVE_SPLIT_GONIN" {
            if !valid_chosen.is_empty() {
                self.prompt_gonin_pick_step(
                    player_id,
                    &actor_id,
                    &actor_name,
                    source_level,
                    &valid_chosen,
                    &[],
                    false,
                    &resume_context,
                    after_hint_level,
                );
                return;
            }
            self.log(format!("{} は [{}] の対象を選択しませんでした（スキップ）", player_id, actor_name));
        } else if effect_type == "REVIVE_SPLIT_PICK_ACTIVE" || effect_type == "REVIVE_SPLIT_PICK_SLEEP" {
            let is_sleep_step = effect_type == "REVIVE_SPLIT_PICK_SLEEP";
            let pool_ids: Vec<String> = opts
                .get("splitPool")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
                .unwrap_or_default();
            if let Some(arr) = opts.get("splitDeployed").and_then(|v| v.as_array()) {
                for item in arr {
                    if let (Some(iid), Some(did), Some(ori_s)) = (
                        item.get("instanceId").and_then(|v| v.as_str()),
                        item.get("definitionId").and_then(|v| v.as_str()),
                        item.get("orientation").and_then(|v| v.as_str()),
                    ) {
                        let ori = if ori_s == "SLEEP" {
                            CardOrientation::SLEEP
                        } else {
                            CardOrientation::ACTIVE
                        };
                        deployed_list.push((iid.to_string(), did.to_string(), ori));
                        self.last_deployed_instance_id = Some(iid.to_string());
                    }
                }
            }
            let picked_id = valid_chosen.first().cloned();
            if let Some(ref tid) = picked_id {
                let single_action = if is_sleep_step { "REVIVE_SLEEP" } else { "REVIVE_ACTIVE" };
                self.apply_single_target_action(
                    player_id,
                    &actor_id,
                    &actor_name,
                    single_action,
                    "REMOVE",
                    tid,
                    0,
                    &opts,
                    &mut deployed_list,
                );
                applied = true;
            }
            let rem_pool: Vec<String> = pool_ids
                .into_iter()
                .filter(|id| picked_id.as_deref() != Some(id.as_str()))
                .collect();
            if !is_sleep_step && !rem_pool.is_empty() {
                self.prompt_gonin_pick_step(
                    player_id,
                    &actor_id,
                    &actor_name,
                    source_level,
                    &rem_pool,
                    &deployed_list,
                    true,
                    &resume_context,
                    after_hint_level,
                );
                return;
            }
            if is_sleep_step && !rem_pool.is_empty() {
                for tid in &rem_pool {
                    self.apply_single_target_action(
                        player_id,
                        &actor_id,
                        &actor_name,
                        "RECOVER_TO_HAND",
                        "REMOVE",
                        tid,
                        0,
                        &opts,
                        &mut deployed_list,
                    );
                }
                applied = true;
            }
        } else if !valid_chosen.is_empty() {
            if effect_type == "SLEEP" {
                self.last_slept_names.clear();
            }
            applied = true;
            self.last_chosen_added_to_hand = false;
            self.last_deployed_count = 0;
            let mut removed_lv_sum = 0i32;
            let mut removed_ap_sum = 0i32;
            let bonuses: Vec<TargetBonus> = serde_json::from_value(
                opts.get("bonuses").cloned().unwrap_or_else(|| json!([])),
            )
            .unwrap_or_default();
            let has_continuation = !bonuses.is_empty()
                || valid_chosen.len() > 1
                || opts.get("followUpTarget").filter(|v| !v.is_null()).is_some()
                || opts.get("followUpDeckSearch").filter(|v| !v.is_null()).is_some()
                || opts.get("pendingClausePlan").filter(|v| !v.is_null()).is_some()
                || opts.get("followUpOps").and_then(|v| v.as_array()).map(|a| !a.is_empty()).unwrap_or(false);
            let prev_exec = self.is_executing_clause_plan;
            if has_continuation {
                self.is_executing_clause_plan = true;
            }
            for (sel_idx, tid) in valid_chosen.iter().enumerate() {
                if chosen_def_for_cond.is_none() {
                    chosen_def_for_cond = self.find_card_def_id(tid);
                }
                if let Some(did) = self.find_card_def_id(tid) {
                    if let Some(def) = self.catalog.get(&did) {
                        removed_lv_sum += def.level;
                        removed_ap_sum += def.ap;
                    }
                }
                self.apply_single_target_action(
                    player_id,
                    &actor_id,
                    &actor_name,
                    &effect_type,
                    &target_zone,
                    tid,
                    sel_idx,
                    &opts,
                    &mut deployed_list,
                );
            }
            if effect_type == "REMOVE" {
                self.last_removed_level_sum = removed_lv_sum;
                self.last_removed_ap_sum = removed_ap_sum;
            }
            let bonus_paused = self.apply_target_bonuses(player_id, &actor_id, &actor_name, &bonuses, &valid_chosen, &resume_context);
            self.is_executing_clause_plan = prev_exec;
            if bonus_paused {
                return;
            }
        } else {
            self.last_chosen_added_to_hand = false;
            self.last_chosen_level = None;
            self.last_chosen_name = None;
            self.last_chosen_instance_id = None;
            self.last_deployed_count = 0;
            self.log(format!("{} は [{}] の対象を選択しませんでした（スキップ）", player_id, actor_name));
        }

        let follow_up_target: Option<TargetStep> = opts
            .get("followUpTarget")
            .filter(|v| !v.is_null())
            .and_then(|v| serde_json::from_value(v.clone()).ok());
        let follow_up_deck_search: Option<DeckSearchStep> = opts
            .get("followUpDeckSearch")
            .filter(|v| !v.is_null())
            .and_then(|v| serde_json::from_value(v.clone()).ok());
        let pending_clause_plan: Option<ParsedClausePlan> = opts
            .get("pendingClausePlan")
            .filter(|v| !v.is_null())
            .and_then(|v| serde_json::from_value(v.clone()).ok());

        // 後続処理を先にキューに退避しておく（任意コストゲート付き followUpTarget がある場合は pendingClausePlan をその内部へ束ねる）
        if applied || !abort_on_skip {
            let mut deferred_tasks = Vec::new();
            let ft_is_gated = follow_up_target.as_ref().map(|ft| ft.then_abort_if_skipped).unwrap_or(false);
            if ft_is_gated {
                let mut fplan = ParsedClausePlan {
                    immediate_before: vec![],
                    target_step: follow_up_target.clone(),
                    follow_up_target: None,
                    deck_search: follow_up_deck_search.clone(),
                    post_target_ops: vec![],
                    extra_clauses: vec![],
                };
                if let Some(ref pcp) = pending_clause_plan {
                    fplan.post_target_ops.extend(pcp.post_target_ops.clone());
                    fplan.extra_clauses.extend(pcp.extra_clauses.clone());
                }
                deferred_tasks.push(PendingClauseTask {
                    player_id: player_id.to_string(),
                    actor_id: actor_id.clone(),
                    actor_name: actor_name.clone(),
                    source_level,
                    card_type: CardType::EVENT,
                    clause: String::new(),
                    resume_context: resume_context.clone(),
                    evidence_source_inst_id: None,
                    post_target_ops: vec![],
                    plan: Some(fplan),
                });
            } else {
                if follow_up_deck_search.is_some() || follow_up_target.is_some() {
                    let fplan = ParsedClausePlan {
                        immediate_before: vec![],
                        target_step: follow_up_target.clone(),
                        follow_up_target: None,
                        deck_search: follow_up_deck_search.clone(),
                        post_target_ops: vec![],
                        extra_clauses: vec![],
                    };
                    deferred_tasks.push(PendingClauseTask {
                        player_id: player_id.to_string(),
                        actor_id: actor_id.clone(),
                        actor_name: actor_name.clone(),
                        source_level,
                        card_type: CardType::EVENT,
                        clause: String::new(),
                        resume_context: resume_context.clone(),
                        evidence_source_inst_id: None,
                        post_target_ops: vec![],
                        plan: Some(fplan),
                    });
                }
                if let Some(ref pcp) = pending_clause_plan {
                    deferred_tasks.push(PendingClauseTask {
                        player_id: player_id.to_string(),
                        actor_id: actor_id.clone(),
                        actor_name: actor_name.clone(),
                        source_level,
                        card_type: CardType::EVENT,
                        clause: String::new(),
                        resume_context: resume_context.clone(),
                        evidence_source_inst_id: None,
                        post_target_ops: vec![],
                        plan: Some(pcp.clone()),
                    });
                }
            }
            for t in deferred_tasks.into_iter().rev() {
                self.pending_clause_queue.insert(0, t);
            }
        }

        // 条件付き追撃 & 後続即時処理
        if applied {
            if self.pending_decision.is_some() {
                if let Ok(follow_ops) = serde_json::from_value::<Vec<ImmediateOp>>(
                    opts.get("followUpOps").cloned().unwrap_or_else(|| json!([])),
                ) {
                    if !follow_ops.is_empty() {
                        self.pending_clause_queue.insert(0, PendingClauseTask {
                            player_id: player_id.to_string(),
                            actor_id: actor_id.clone(),
                            actor_name: actor_name.clone(),
                            source_level,
                            card_type: CardType::CHARACTER,
                            clause: String::new(),
                            resume_context: resume_context.clone(),
                            evidence_source_inst_id: None,
                            post_target_ops: follow_ops,
                            plan: Some(ParsedClausePlan::default()),
                        });
                    }
                }
                return;
            }
            if let Ok(follow_ops) = serde_json::from_value::<Vec<ImmediateOp>>(
                opts.get("followUpOps").cloned().unwrap_or_else(|| json!([])),
            ) {
                if !follow_ops.is_empty()
                    && self.execute_immediate_ops_with_continuation(
                        player_id,
                        &actor_id,
                        &actor_name,
                        &follow_ops,
                        None,
                        None,
                        None,
                        None,
                        &resume_context,
                        after_hint_level,
                    )
                {
                    return;
                }
            }
            if let Some(cond_val) = opts.get("condFollowUp").filter(|v| !v.is_null()) {
                if let (Ok(branch), Ok(cops), Some(did)) = (
                    serde_json::from_value::<crate::effects::TargetBranch>(cond_val.get("branch").cloned().unwrap_or_default()),
                    serde_json::from_value::<Vec<ImmediateOp>>(cond_val.get("ops").cloned().unwrap_or_default()),
                    chosen_def_for_cond.as_ref(),
                ) {
                    if let Some(cdef) = self.catalog.get(did) {
                        if branch_matches(cdef, cdef.ap, &branch)
                            && self.execute_immediate_ops_with_continuation(
                                player_id,
                                &actor_id,
                                &actor_name,
                                &cops,
                                None,
                                None,
                                None,
                                None,
                                &resume_context,
                                after_hint_level,
                            )
                        {
                            return;
                        }
                    }
                }
            }
        }

        // 登場させたキャラの登場時効果（突撃/迅速のみを除き2体以上同時にOnPlayを持つ場合のみ発動順をプレイヤーが選択）
        if self.resolve_deployed_characters(player_id, source_level, &deployed_list, &resume_context) {
            return;
        }

        if (applied || !abort_on_skip) && self.drain_pending_clause_queue() {
            return;
        }

        if self.try_fire_pending_ally_play(&resume_context) {
            return;
        }

        if self.try_fire_pending_hint_trigger(player_id, after_hint_level) {
            return;
        }

        self.resume_after_decision(player_id, &resume_context);
    }

    fn is_simple_rush_or_swift_clause(clause: &str, card_type: CardType) -> bool {
        let plan = parse_effect_steps(clause, None, card_type);
        plan.target_step.is_none()
            && plan.follow_up_target.is_none()
            && plan.deck_search.is_none()
            && plan.post_target_ops.is_empty()
            && plan.extra_clauses.is_empty()
            && !plan.immediate_before.is_empty()
            && plan
                .immediate_before
                .iter()
                .all(|op| matches!(op, ImmediateOp::SelfGiveRush(_)))
    }

    pub fn resolve_deployed_characters(
        &mut self,
        player_id: &str,
        source_level: Option<i32>,
        deployed_list: &[(String, String, CardOrientation)],
        resume_context: &str,
    ) -> bool {
        if deployed_list.is_empty() {
            return false;
        }
        if let Some((last_id, _, _)) = deployed_list.last() {
            self.last_deployed_instance_id = Some(last_id.clone());
        }
        // 全登場キャラを先に場入場準備（MR整理 + pending_ally_play 登録）し、後から順番にOnPlay・ウォッチャーを発火させる
        for (dep_inst, dep_def, dep_ori) in deployed_list {
            self.prepare_character_enter_field(player_id, dep_inst, dep_def, *dep_ori, source_level);
        }

        let mut on_play_tasks: Vec<PendingClauseTask> = Vec::new();
        let mut non_rush_count = 0usize;
        for (dep_inst, dep_def, _) in deployed_list {
            if let Some(def) = self.catalog.get(dep_def).cloned() {
                let on_play_clauses = extract_clauses(&def.description, TriggerKind::OnPlay, def.card_type);
                let dep_name = self.card_name_by_def(dep_def);
                for cl in on_play_clauses {
                    if !Self::is_simple_rush_or_swift_clause(&cl, def.card_type) {
                        non_rush_count += 1;
                    }
                    on_play_tasks.push(PendingClauseTask {
                        player_id: player_id.to_string(),
                        actor_id: dep_inst.clone(),
                        actor_name: dep_name.clone(),
                        source_level,
                        card_type: def.card_type,
                        clause: cl,
                        resume_context: resume_context.to_string(),
                        evidence_source_inst_id: None,
                        post_target_ops: vec![],
                        plan: None,
                    });
                }
            }
        }

        if non_rush_count >= 2 && on_play_tasks.len() >= 2 {
            let triggers_json: Vec<Value> = on_play_tasks
                .iter()
                .enumerate()
                .map(|(i, t)| {
                    json!({
                        "index": i,
                        "actorId": t.actor_id,
                        "actorName": t.actor_name,
                        "clause": t.clause,
                    })
                })
                .collect();
            self.pending_trigger_order_tasks = on_play_tasks;
            let dec_id = self.generate_decision_id();
            self.pending_decision = Some(json!({
                "decisionId": dec_id,
                "revision": self.revision,
                "playerId": player_id,
                "kind": "triggerOrder",
                "options": {
                    "title": "同時に誘発した【登場時】効果のうち、先に発動する能力を選んでください",
                    "triggers": triggers_json,
                }
            }));
            return true;
        }

        if !on_play_tasks.is_empty() {
            for t in on_play_tasks.into_iter().rev() {
                self.pending_clause_queue.insert(0, t);
            }
            if self.drain_pending_clause_queue() {
                return true;
            }
        }

        self.try_fire_pending_ally_play(resume_context)
    }

    #[allow(clippy::too_many_arguments)]
    fn prompt_gonin_pick_step(
        &mut self,
        player_id: &str,
        actor_id: &str,
        actor_name: &str,
        source_level: Option<i32>,
        pool_ids: &[String],
        deployed: &[(String, String, CardOrientation)],
        pick_sleep: bool,
        resume_context: &str,
        after_hint_level: Option<i32>,
    ) {
        let candidates: Vec<Value> = pool_ids
            .iter()
            .filter_map(|iid| {
                let ps = self.players.get(player_id)?;
                let c = ps.remove.iter().find(|rc| &rc.instance_id == iid)?;
                let def = self.catalog.get(&c.definition_id)?;
                let cname = def.names.first().cloned().unwrap_or_else(|| c.definition_id.clone());
                Some(json!({
                    "instanceId": c.instance_id,
                    "definitionId": c.definition_id,
                    "name": cname,
                    "level": def.level,
                    "ap": def.ap,
                    "traits": def.traits,
                    "colors": def.colors,
                    "ownerId": player_id,
                    "orientation": "ACTIVE",
                    "zone": "REMOVE",
                    "zoneLabel": "リムーブ",
                    "faceUp": true,
                }))
            })
            .collect();
        let deployed_json: Vec<Value> = deployed
            .iter()
            .map(|(iid, did, ori)| {
                json!({
                    "instanceId": iid,
                    "definitionId": did,
                    "orientation": match ori {
                        CardOrientation::SLEEP => "SLEEP",
                        _ => "ACTIVE",
                    }
                })
            })
            .collect();
        let dec_id = self.generate_decision_id();
        let title = if pick_sleep {
            "残りから【スリープ状態】で登場させるキャラを1枚まで選択（スキップで残りを手札に加える）"
        } else {
            "選んだ中から【アクティブ状態】で登場させるキャラを1枚まで選択（スキップ可）"
        };
        self.pending_decision = Some(json!({
            "decisionId": dec_id,
            "revision": self.revision,
            "playerId": player_id,
            "kind": "targetSelect",
            "options": {
                "actorId": actor_id,
                "actorName": actor_name,
                "sourceLevel": source_level,
                "effectType": if pick_sleep { "REVIVE_SPLIT_PICK_SLEEP" } else { "REVIVE_SPLIT_PICK_ACTIVE" },
                "targetZone": "REMOVE",
                "maxSelect": 1,
                "optional": true,
                "abortOnSkip": false,
                "candidates": candidates,
                "splitPool": pool_ids,
                "splitDeployed": deployed_json,
                "resumeContext": resume_context,
                "afterHintLevel": after_hint_level,
                "title": title,
            }
        }));
    }

    /// 同時誘発した能力の発動順選択を処理する
    pub(super) fn handle_trigger_order_answer(&mut self, player_id: &str, val: &Value) {
        let mut tasks = std::mem::take(&mut self.pending_trigger_order_tasks);
        if tasks.is_empty() {
            self.prompt_main_action(player_id);
            return;
        }
        let idx = val
            .get("index")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .filter(|&i| i < tasks.len())
            .unwrap_or(0);
        let chosen = tasks.remove(idx);
        let resume_context = chosen.resume_context.clone();
        let mut ordered = vec![chosen];
        ordered.extend(tasks);
        ordered.extend(std::mem::take(&mut self.pending_clause_queue));
        self.pending_clause_queue = ordered;
        if self.drain_pending_clause_queue() {
            return;
        }
        self.resume_after_decision(player_id, &resume_context);
    }

    /// デッキサーチ・めくり選択の意思決定を処理する
    pub(super) fn handle_deck_search_answer(&mut self, player_id: &str, val: &Value, dec: &Value) {
        let opts = dec
            .get("options")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let owner_id = opts
            .get("ownerId")
            .and_then(|v| v.as_str())
            .unwrap_or(player_id)
            .to_string();
        let player_id = owner_id.as_str();

        let actor_id = opts
            .get("actorId")
            .and_then(|v| v.as_str())
            .or_else(|| val.get("actorId").and_then(|v| v.as_str()))
            .unwrap_or("");
        let actor_name = self.card_name_by_inst(actor_id);
        let pick_action = opts.get("pickAction").and_then(|v| v.as_str()).unwrap_or("TO_HAND");
        let max_select = opts.get("maxSelect").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
        let source_level = opts.get("sourceLevel").and_then(|v| v.as_i64()).map(|n| n as i32);
        let post_add_discard = opts.get("postAddDiscard").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let post_add_discard_min_picked = opts
            .get("postAddDiscardMinPicked")
            .and_then(|v| v.as_u64())
            .unwrap_or(1) as usize;
        let remainder_to_remove = opts.get("remainderToRemove").and_then(|v| v.as_bool()).unwrap_or(false);
        let shuffle_remainder = opts.get("shuffleRemainder").and_then(|v| v.as_bool()).unwrap_or(false);
        let record_removed_levels = opts.get("recordRemovedLevels").and_then(|v| v.as_bool()).unwrap_or(false);
        let post_clauses: Vec<String> = opts.get("postClauses").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
        let b09033_rem_repeats = opts.get("b09033RemRepeats").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let b09033_filter: Option<TargetFilter> = opts.get("b09033Filter").and_then(|v| serde_json::from_value(v.clone()).ok());
        let resume_context = opts
            .get("resumeContext")
            .and_then(|v| v.as_str())
            .unwrap_or("MAIN")
            .to_string();
        let after_hint_level = opts.get("afterHintLevel").and_then(|v| v.as_i64()).map(|n| n as i32);

        let mut chosen_ids: Vec<String> = vec![];
        if let Some(arr) = val.get("chosenCardIds").and_then(|v| v.as_array()) {
            for item in arr {
                if let Some(s) = item.as_str() {
                    chosen_ids.push(s.to_string());
                }
            }
        } else if let Some(single) = val.get("chosenCardId").and_then(|v| v.as_str()) {
            chosen_ids.push(single.to_string());
        }
        chosen_ids.truncate(max_select.max(1));

        let distinct_colors = opts.get("distinctColors").and_then(|v| v.as_bool()).unwrap_or(false);
        let mut revealed_instances = vec![];
        let mut selectable_map: HashMap<String, Vec<String>> = HashMap::new();
        if let Some(cards) = opts.get("revealedCards").and_then(|v| v.as_array()) {
            for c in cards {
                if let Some(iid) = c.get("instanceId").and_then(|v| v.as_str()) {
                    revealed_instances.push(iid.to_string());
                    if c.get("selectable").and_then(|v| v.as_bool()).unwrap_or(true) {
                        let cols = c
                            .get("colors")
                            .and_then(|v| v.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                                    .collect()
                            })
                            .unwrap_or_default();
                        selectable_map.insert(iid.to_string(), cols);
                    }
                }
            }
        }

        // 条件外カード・同色重複カードを選択した場合は無視
        let mut used_colors: HashSet<String> = HashSet::new();
        let mut chosen_set: HashSet<String> = HashSet::new();
        for id in chosen_ids {
            if let Some(cols) = selectable_map.get(&id) {
                if distinct_colors && cols.iter().any(|col| used_colors.contains(col)) {
                    continue;
                }
                if distinct_colors {
                    for col in cols {
                        used_colors.insert(col.clone());
                    }
                }
                chosen_set.insert(id);
            }
        }

        // B09033 繰り返しターンは FILE を上から1枚支払う
        if b09033_rem_repeats > 0 && opts.get("b09033RequireFileCost").and_then(|v| v.as_bool()).unwrap_or(false) && !chosen_set.is_empty() {
            if let Some(ps) = self.players.get_mut(player_id) {
                if let Some(mut fc) = ps.file.pop() {
                    fc.zone = Zone::REMOVE;
                    fc.face_up = true;
                    ps.remove.push(fc);
                    self.log(format!("💸 [{}] FILEを1枚リムーブして追加登場！", actor_name));
                }
            }
        }

        let mut picked_defs: Vec<(String, String)> = vec![];
        let mut remaining_cards = vec![];
        let mut removed_lv_sum = 0i32;
        let turn = self.turn;
        let mut last_chosen_def: Option<String> = None;
        let mut last_chosen_lv: Option<i32> = None;
        let mut last_chosen_inst: Option<String> = None;
        let mut deployed_count_delta = 0usize;
        let mut added_to_hand = false;
        let mut b09033_next_candidates: Option<Vec<Value>> = None;

        if let Some(ps) = self.players.get_mut(player_id) {
            for iid in &revealed_instances {
                if let Some(pos) = ps.deck.iter().position(|c| &c.instance_id == iid) {
                    let mut card = ps.deck.remove(pos);
                    if chosen_set.contains(iid) {
                        match pick_action {
                            "PLAY_ACTIVE" | "PLAY_SLEEP" => {
                                card.zone = Zone::FIELD;
                                card.orientation = if pick_action == "PLAY_SLEEP" {
                                    CardOrientation::SLEEP
                                } else {
                                    CardOrientation::ACTIVE
                                };
                                card.entered_turn = turn;
                                card.face_up = true;
                                deployed_count_delta += 1;
                                self.field_overflow_excluded_ids.insert(card.instance_id.clone());
                                if let Some(d) = self.catalog.get(&card.definition_id) {
                                    last_chosen_lv = Some(d.level);
                                }
                                last_chosen_def = Some(card.definition_id.clone());
                                last_chosen_inst = Some(card.instance_id.clone());
                                picked_defs.push((card.instance_id.clone(), card.definition_id.clone()));
                                ps.field.push(card);
                                continue;
                            }
                            _ => {
                                card.zone = Zone::HAND;
                                card.face_up = true;
                                added_to_hand = true;
                                if let Some(d) = self.catalog.get(&card.definition_id) {
                                    last_chosen_lv = Some(d.level);
                                }
                                last_chosen_def = Some(card.definition_id.clone());
                                last_chosen_inst = Some(card.instance_id.clone());
                                picked_defs.push((card.instance_id.clone(), card.definition_id.clone()));
                                ps.hand.push(card);
                                continue;
                            }
                        }
                    }
                    remaining_cards.push(card);
                }
            }

            // B09033 継続可能なら残り公開カードをデッキ上部に維持したまま再プロンプトできるようにする
            let can_b09033_continue = b09033_rem_repeats > 0
                && !picked_defs.is_empty()
                && !ps.file.is_empty()
                && !remaining_cards.is_empty();

            let mut remaining_opt = Some(remaining_cards);
            if can_b09033_continue {
                let rem_slice = remaining_opt.as_ref().unwrap();
                let rem_candidates: Vec<Value> = rem_slice
                    .iter()
                    .map(|c| {
                        let def_opt = self.catalog.get(&c.definition_id);
                        let selectable = def_opt.zip(b09033_filter.as_ref()).map(|(def, f)| {
                            f.branches.iter().any(|b| branch_matches_with_ctx(def, def.ap, b, None))
                        }).unwrap_or(false);
                        let cname = def_opt
                            .and_then(|d| d.names.first().cloned())
                            .unwrap_or_else(|| c.definition_id.clone());
                        json!({
                            "instanceId": c.instance_id,
                            "definitionId": c.definition_id,
                            "name": cname,
                            "level": def_opt.map(|d| d.level).unwrap_or(0),
                            "ap": def_opt.map(|d| d.ap).unwrap_or(0),
                            "colors": def_opt.map(|d| d.colors.clone()).unwrap_or_default(),
                            "traits": def_opt.map(|d| d.traits.clone()).unwrap_or_default(),
                            "selectable": selectable,
                        })
                    })
                    .collect();
                let any_sel = rem_candidates.iter().any(|c| c["selectable"].as_bool().unwrap_or(false));
                if any_sel {
                    if let Some(rem) = remaining_opt.take() {
                        ps.deck.extend(rem);
                    }
                    b09033_next_candidates = Some(rem_candidates);
                }
            }
            let mut removed_def_ids: Vec<String> = Vec::new();
            if let Some(mut rem) = remaining_opt.take() {
                if shuffle_remainder {
                    let mut rng = rand::thread_rng();
                    rem.shuffle(&mut rng);
                }
                for mut c in rem {
                    if remainder_to_remove {
                        if let Some(d) = self.catalog.get(&c.definition_id) {
                            removed_lv_sum += d.level;
                        }
                        removed_def_ids.push(c.definition_id.clone());
                        c.zone = Zone::REMOVE;
                        c.face_up = true;
                        ps.remove.push(c);
                    } else {
                        c.face_up = false;
                        ps.deck.insert(0, c);
                    }
                }
            }
            if !removed_def_ids.is_empty() {
                let rem_names: Vec<String> = removed_def_ids.iter().map(|did| self.card_name_by_def(did)).collect();
                self.log(format!(
                    "🗑️ [{}] の効果で残りの [{}] をリムーブエリアに移しました",
                    actor_name,
                    rem_names.join("], [")
                ));
            }
        }
        self.last_deployed_count = deployed_count_delta;
        if deployed_count_delta > 0 {
            self.self_entered_field_players_this_turn.insert(player_id.to_string());
        }
        self.last_chosen_added_to_hand = added_to_hand;
        self.last_chosen_level = last_chosen_lv;
        self.last_chosen_name = last_chosen_def.as_ref().map(|did| self.card_name_by_def(did));
        self.last_chosen_instance_id = last_chosen_inst;
        if let Some(rem_candidates) = b09033_next_candidates {
            let dec_id = self.generate_decision_id();
            self.pending_decision = Some(json!({
                "decisionId": dec_id,
                "revision": self.revision,
                "playerId": player_id,
                "kind": "deckSearch",
                "options": {
                    "actorId": actor_id,
                    "actorName": actor_name,
                    "sourceLevel": source_level,
                    "searchCount": rem_candidates.len(),
                    "maxSelect": 1,
                    "pickAction": "PLAY_ACTIVE",
                    "postAddDiscard": 0,
                    "postOps": [],
                    "remainderToRemove": false,
                    "shuffleRemainder": false,
                    "b09033RemRepeats": b09033_rem_repeats - 1,
                    "b09033RequireFileCost": true,
                    "b09033Filter": b09033_filter,
                    "afterHintLevel": after_hint_level,
                    "resumeContext": resume_context,
                    "title": format!("FILEを1枚リムーブして追加でLv6以下の〚高校生〛を1枚登場させてもよい（残り{}回）", b09033_rem_repeats),
                    "revealedCards": rem_candidates,
                    "optional": true
                }
            }));
            for (_, did) in &picked_defs {
                let cname = self.card_name_by_def(did);
                self.log(format!("✨ [{}] の効果でデッキから [{}] を登場させました", actor_name, cname));
            }
            return;
        }
        if record_removed_levels {
            self.last_removed_level_sum = removed_lv_sum;
        }

        let follow_up_target: Option<TargetStep> = opts
            .get("followUpTarget")
            .filter(|v| !v.is_null())
            .and_then(|v| serde_json::from_value(v.clone()).ok());
        let post_ops: Vec<ImmediateOp> = serde_json::from_value(
            opts.get("postOps").cloned().unwrap_or_else(|| json!([])),
        )
        .unwrap_or_default();

        for pc in post_clauses.into_iter().rev() {
            self.pending_clause_queue.insert(
                0,
                PendingClauseTask {
                    player_id: player_id.to_string(),
                    actor_id: actor_id.to_string(),
                    actor_name: actor_name.to_string(),
                    source_level,
                    card_type: CardType::EVENT,
                    clause: pc,
                    resume_context: resume_context.clone(),
                    evidence_source_inst_id: None,
                    post_target_ops: vec![],
                    plan: None,
                },
            );
        }

        let mut discarded_opened = false;
        if !picked_defs.is_empty() {
            for (_, did) in &picked_defs {
                let cname = self.card_name_by_def(did);
                if pick_action == "PLAY_ACTIVE" || pick_action == "PLAY_SLEEP" {
                    self.log(format!("✨ [{}] の効果でデッキから [{}] を登場させました", actor_name, cname));
                } else if remainder_to_remove {
                    self.log(format!("✨ [{}] の効果でデッキから [{}] を手札に加え、残りをリムーブエリアに移しました", actor_name, cname));
                } else {
                    self.log(format!("✨ [{}] の効果でデッキから [{}] を手札に加え、残りをデッキ下に移しました", actor_name, cname));
                }
            }
            if post_add_discard > 0
                && pick_action == "TO_HAND"
                && picked_defs.len() >= post_add_discard_min_picked
            {
                let mut combined = vec![ImmediateOp::DiscardHand(post_add_discard)];
                combined.extend(post_ops.clone());
                if self.execute_immediate_ops_with_continuation(
                    player_id,
                    actor_id,
                    &actor_name,
                    &combined,
                    None,
                    follow_up_target.as_ref(),
                    None,
                    None,
                    &resume_context,
                    after_hint_level,
                ) {
                    return;
                }
                discarded_opened = true;
            }
        } else if remainder_to_remove {
            self.log(format!("{} はカードを選びませんでした（残りをリムーブエリアへ）", player_id));
        } else {
            self.log(format!("{} はカードを選びませんでした（残りをデッキ下へ）", player_id));
        }

        if !discarded_opened && !post_ops.is_empty() {
            if self.execute_immediate_ops_with_continuation(
                player_id,
                actor_id,
                &actor_name,
                &post_ops,
                None,
                follow_up_target.as_ref(),
                None,
                None,
                &resume_context,
                after_hint_level,
            ) {
                return;
            }
        }

        if pick_action == "PLAY_ACTIVE" || pick_action == "PLAY_SLEEP" {
            let play_ori = if pick_action == "PLAY_SLEEP" {
                CardOrientation::SLEEP
            } else {
                CardOrientation::ACTIVE
            };
            for (idx, (dep_inst, dep_def)) in picked_defs.iter().enumerate() {
                if self.resolve_character_enter_field_with_source(
                    player_id,
                    dep_inst,
                    dep_def,
                    play_ori,
                    source_level,
                    &resume_context,
                ) {
                    for (rem_inst, rem_def) in picked_defs.iter().skip(idx + 1) {
                        self.pending_enter_field_queue.push((
                            player_id.to_string(),
                            rem_inst.clone(),
                            rem_def.clone(),
                            play_ori,
                            source_level,
                            resume_context.clone(),
                        ));
                    }
                    return;
                }
            }
        }

        if self.pending_decision.is_none() {
            if let Some(ref ft) = follow_up_target {
                if self.prompt_target_step(
                    player_id,
                    actor_id,
                    &actor_name,
                    source_level,
                    ft,
                    None,
                    &resume_context,
                    after_hint_level,
                ) {
                    return;
                }
            }
        }

        if self.drain_pending_clause_queue() {
            return;
        }

        if self.try_fire_pending_ally_play(&resume_context) {
            return;
        }

        if self.try_fire_pending_hint_trigger(player_id, after_hint_level) {
            return;
        }

        self.resume_after_decision(player_id, &resume_context);
    }

    /// 現場のキャラが上限(5体)を超えている場合、場外(リムーブエリア)に移すキャラ（登場したキャラ以外）を1体選択させる（効果リムーブではない）
    pub(super) fn check_field_overflow(&mut self, player_id: &str, resume_context: &str) -> bool {
        let overflow_pids: Vec<String> = self
            .players
            .iter()
            .filter(|(_, ps)| ps.field.len() > 5)
            .map(|(pid, _)| pid.clone())
            .collect();
        let target_pid = if overflow_pids.iter().any(|p| p == player_id) {
            player_id.to_string()
        } else if let Some(first) = overflow_pids.into_iter().next() {
            first
        } else {
            self.field_overflow_excluded_ids.clear();
            return false;
        };

        let mut candidates: Vec<Value> = if let Some(ps) = self.players.get(&target_pid) {
            ps.field
                .iter()
                .filter(|c| !self.field_overflow_excluded_ids.contains(&c.instance_id))
                .map(|c| {
                    let def = self.catalog.get(&c.definition_id);
                    json!({
                        "instanceId": c.instance_id,
                        "definitionId": c.definition_id,
                        "name": self.card_name_by_def(&c.definition_id),
                        "level": def.map(|d| d.level).unwrap_or(0),
                        "ap": self.get_effective_ap(&target_pid, c),
                        "orientation": match c.orientation {
                            CardOrientation::ACTIVE => "ACTIVE",
                            CardOrientation::SLEEP => "SLEEP",
                            CardOrientation::STUN => "STUN",
                        },
                        "isOpponent": false,
                        "zoneLabel": "現場",
                        "traits": def.map(|d| d.traits.clone()).unwrap_or_default(),
                    })
                })
                .collect()
        } else {
            return false;
        };

        // 万一すべて除外対象になった場合の安全フォールバック
        if candidates.is_empty() {
            if let Some(ps) = self.players.get(&target_pid) {
                candidates = ps
                    .field
                    .iter()
                    .map(|c| {
                        let def = self.catalog.get(&c.definition_id);
                        json!({
                            "instanceId": c.instance_id,
                            "definitionId": c.definition_id,
                            "name": self.card_name_by_def(&c.definition_id),
                            "level": def.map(|d| d.level).unwrap_or(0),
                            "ap": self.get_effective_ap(&target_pid, c),
                            "orientation": match c.orientation {
                                CardOrientation::ACTIVE => "ACTIVE",
                                CardOrientation::SLEEP => "SLEEP",
                                CardOrientation::STUN => "STUN",
                            },
                            "isOpponent": false,
                            "zoneLabel": "現場",
                            "traits": def.map(|d| d.traits.clone()).unwrap_or_default(),
                        })
                    })
                    .collect();
            }
        }

        if candidates.is_empty() {
            return false;
        }
        let dec_id = self.generate_decision_id();
        self.pending_decision = Some(json!({
            "decisionId": dec_id,
            "revision": self.revision,
            "playerId": target_pid,
            "kind": "targetSelect",
            "options": {
                "title": "場のキャラが上限(5体)を超えたため、場外(リムーブ)に移すキャラ（登場したキャラ以外）を1体選択してください",
                "actorId": "",
                "actorName": "ルール処理",
                "effectType": "FIELD_OVERFLOW_REMOVE",
                "targetZone": "FIELD",
                "maxSelect": 1,
                "optional": false,
                "candidates": candidates,
                "resumeContext": resume_context,
            }
        }));
        true
    }

    pub(super) fn resume_after_decision(&mut self, decision_player_id: &str, resume_context: &str) {
        let effective_ctx = if self.phase == MatchPhase::END_TRIGGERS {
            "END_TURN"
        } else {
            resume_context
        };
        if self.drain_pending_clause_queue() {
            return;
        }
        if self.try_fire_pending_ally_play(effective_ctx) {
            return;
        }
        if self.check_field_overflow(decision_player_id, effective_ctx) {
            return;
        }
        if effective_ctx == "MAIN" && self.try_fire_pending_hint_trigger(decision_player_id, None) {
            return;
        }
        if effective_ctx == "GUARD" {
            let pending_trigger = self.action_context.as_mut().and_then(|ctx| {
                let obj = ctx.as_object_mut()?;
                obj.remove("pendingCutinTrigger")
            });
            if let Some(pt) = pending_trigger {
                let upid = pt.get("userPid").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let did = pt.get("cutinDefId").and_then(|v| v.as_str()).map(|s| s.to_string());
                let is_disg = pt.get("isDisguise").and_then(|v| v.as_bool()).unwrap_or(false);
                if !upid.is_empty() && self.trigger_on_cutin_or_disguise_used(&upid, did.as_deref(), is_disg) {
                    return;
                }
            }
            if self.is_contact_aborted() {
                let att_pid = self
                    .action_context
                    .as_ref()
                    .and_then(|c| c.get("attackerPlayerId"))
                    .and_then(|v| v.as_str())
                    .unwrap_or(&self.turn_player_id)
                    .to_string();
                self.action_context = None;
                self.contact_ap_buffs.clear();
                self.log("⚠️ コンタクト中のキャラが現場を離れたためコンタクトを終了します");
                self.prompt_main_action(&att_pid);
                return;
            }
            let (guard_resolved, attacker_pid, defender_pid, attacker_used, defender_used, saved_guard_id) =
                if let Some(ctx) = &self.action_context {
                    (
                        ctx.get("guardResolved").and_then(|v| v.as_bool()).unwrap_or(false),
                        ctx.get("attackerPlayerId").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                        ctx.get("defenderPlayerId").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                        ctx.get("attackerUsedCutin").and_then(|v| v.as_bool()).unwrap_or(false),
                        ctx.get("defenderUsedCutin").and_then(|v| v.as_bool()).unwrap_or(false),
                        ctx.get("guardCardId").and_then(|v| v.as_str()).map(|s| s.to_string()),
                    )
                } else {
                    (false, String::new(), String::new(), false, false, None)
                };
            if guard_resolved {
                let is_att = decision_player_id == attacker_pid;
                let other_pid = if is_att { defender_pid } else { attacker_pid };
                let other_used = if is_att { defender_used } else { attacker_used };
                if !other_used && !other_pid.is_empty() {
                    self.prompt_contact_cutin(&other_pid);
                } else {
                    self.proceed_to_action_resolution(saved_guard_id.as_deref());
                }
            } else {
                self.prompt_guard_or_cutin(decision_player_id);
            }
        } else if let Some(def_pid) = effective_ctx.strip_prefix("START_GUARD:") {
            if self.is_action_target_still_valid() {
                self.prompt_guard_or_cutin(def_pid);
            } else {
                self.action_context = None;
                self.contact_ap_buffs.clear();
                self.log("⚠️ 指定されていた対象が現場を離れたためアクションを終了します");
                self.prompt_main_action(&self.turn_player_id.clone());
            }
        } else if effective_ctx == "END_TURN" {
            let next_turn = self.turn + 1;
            let cur_turn_pid = self.turn_player_id.clone();
            let next_pid = self.player_order.iter().find(|pid| *pid != &cur_turn_pid).cloned().unwrap_or(cur_turn_pid);
            self.start_turn(next_turn, &next_pid);
        } else if let Some(rest) = effective_ctx.strip_prefix("CASE_ATTACK:") {
            let (attacker_pid, actor_id_opt) = match rest.split_once(':') {
                Some((p, a)) => (p, Some(a)),
                None => (rest, None),
            };
            if let Some(aid) = actor_id_opt {
                self.trigger_on_evidence_gain(attacker_pid, Some(aid), true);
            }
            if self.pending_decision.is_none() {
                self.prompt_main_action(attacker_pid);
            }
        } else if self.action_context.is_some() && self.turn_player_id != decision_player_id {
            self.prompt_guard_or_cutin(decision_player_id);
        } else {
            self.prompt_main_action(&self.turn_player_id.clone());
        }
    }
}
