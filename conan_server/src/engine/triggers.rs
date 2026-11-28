use super::*;

impl MatchEngine {

    /// 登場時効果またはイベントカード効果を解決する（能力元レベルを引数で受け取れる）
    pub(super) fn resolve_on_play_effects(
        &mut self,
        player_id: &str,
        actor_id: &str,
        definition_id: &str,
        is_event: bool,
    ) -> bool {
        self.resolve_on_play_effects_with_source(player_id, actor_id, definition_id, is_event, None, "MAIN")
    }

    pub(crate) fn resolve_on_play_effects_with_source(
        &mut self,
        player_id: &str,
        actor_id: &str,
        definition_id: &str,
        is_event: bool,
        source_effect_level: Option<i32>,
        resume_context: &str,
    ) -> bool {
        let Some(def) = self.catalog.get(definition_id).cloned() else {
            return false;
        };
        if def.description.is_empty() {
            return false;
        }
        let card_name = self.card_name_by_def(definition_id);

        // イベントの「以下から〜選んで行う」モード選択チェック
        if is_event {
            if let Some(mc) = parse_event_mode_choice(&def.description) {
                return self.start_event_mode_choice(
                    player_id,
                    actor_id,
                    &card_name,
                    Some(def.level),
                    &mc,
                    resume_context,
                );
            }
        }

        // パッシブの〚突撃〛はカード状態に応じて動的に effective_rush_and_swift / evaluate_passive_rush_and_swift で評価するため、ここでは静的に焼き込まない

        let clauses = extract_clauses(&def.description, TriggerKind::OnPlay, def.card_type);
        if clauses.is_empty() {
            return false;
        }

        for (cidx, clause) in clauses.iter().enumerate() {
            let ori = self.find_actor_orientation(player_id, actor_id);
            let ctx = self.build_cond_info(player_id, Some(actor_id), ori, source_effect_level);
            if let Err(reason) = check_clause_conditions(clause, &ctx) {
                self.log(format!("⚠️ [{}] の登場時効果不発: {}", card_name, reason));
                continue;
            }
            let cost = parse_clause_cost(clause);
            if !self.pay_clause_cost(player_id, actor_id, &cost) {
                self.log(format!("⚠️ [{}] の登場時効果: コスト不足のため発動できませんでした", card_name));
                continue;
            }
            if self.try_open_clause_mode_select(
                player_id,
                actor_id,
                &card_name,
                Some(def.level),
                def.card_type,
                clause,
                resume_context,
            ) {
                return true;
            }
            let resolved_clause = resolve_state_branch(clause, ori);
            let plan = parse_effect_steps(&resolved_clause, None, def.card_type);
            let src_lv = Some(def.level);
            // 残りの句があればキューに退避
            let remaining_start = cidx + 1;
            let has_remaining = remaining_start < clauses.len();
            if has_remaining {
                for rem_cl in clauses[remaining_start..].iter().rev() {
                    self.pending_clause_queue.insert(
                        0,
                        PendingClauseTask {
                            player_id: player_id.to_string(),
                            actor_id: actor_id.to_string(),
                            actor_name: card_name.clone(),
                            source_level: src_lv,
                            card_type: def.card_type,
                            clause: rem_cl.clone(),
                            resume_context: resume_context.to_string(),
                            evidence_source_inst_id: None,
                            post_target_ops: vec![],
                            plan: None,
                        },
                    );
                }
            }
            if self.execute_clause_plan(player_id, actor_id, &card_name, src_lv, &plan, resume_context, None) {
                return true;
            }
            // 即時完結した場合は既にキューに入れた残り句をそのままループで処理するので一旦クリアは不要 — すでに execute_clause_plan が false ならここでキューを消化して戻す
            if has_remaining {
                return self.drain_pending_clause_queue();
            }
        }
        false
    }

    /// キャラがフィールドを離れる際の保護チェック（B01039/B05041/B11050）とリムーブ処理
    pub fn move_field_char_to_remove(
        &mut self,
        owner_pid: &str,
        char_inst_id: &str,
        is_contact_loss: bool,
        is_opponent_effect: bool,
    ) -> Option<(String, Vec<(String, String)>)> {
        let mut substituted_remove_def: Option<String> = None;
        let mut rescued = false;
        let is_opp_turn = self.turn_player_id != owner_pid;

        if let Some(ps) = self.players.get_mut(owner_pid) {
            if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == char_inst_id) {
                // B05041: 相手の能力や効果によってリムーブされない
                if is_opponent_effect {
                    let has_b05041 = c.set_cards.iter().any(|sc| {
                        sc.face_up
                            && self
                                .catalog
                                .get(&sc.definition_id)
                                .map(|d| d.description.contains("相手の能力や効果によってリムーブされず"))
                                .unwrap_or(false)
                    });
                    if has_b05041 {
                        return None;
                    }
                }
                // B11050: 相手ターン中のコンタクト時、パートナーエリアに移すことでリムーブされない
                if is_contact_loss && is_opp_turn {
                    if let Some(sidx) = c.set_cards.iter().position(|sc| {
                        sc.face_up
                            && self
                                .catalog
                                .get(&sc.definition_id)
                                .map(|d| {
                                    d.description.contains("このイベントをパートナーエリアに移してもよい")
                                        && d.description.contains("このコンタクトによってリムーブされない")
                                })
                                .unwrap_or(false)
                    }) {
                        let mut sc = c.set_cards.remove(sidx);
                        sc.zone = Zone::PARTNER;
                        ps.extra_partners.push(sc);
                        rescued = true;
                    }
                }
                // B01039: 身代わりでセットイベントをリムーブして現場に残る
                if !rescued {
                    if let Some(sidx) = c.set_cards.iter().position(|sc| {
                        sc.face_up
                            && self
                                .catalog
                                .get(&sc.definition_id)
                                .map(|d| d.description.contains("このイベントをリムーブし、そのキャラは現場から離れる代わりに現場に残る"))
                                .unwrap_or(false)
                    }) {
                        let mut sc = c.set_cards.remove(sidx);
                        sc.zone = Zone::REMOVE;
                        sc.face_up = true;
                        substituted_remove_def = Some(sc.definition_id.clone());
                        ps.remove.push(sc);
                        rescued = true;
                    }
                }
            }
        }
        if let Some(sdef) = substituted_remove_def {
            self.trigger_on_remove(owner_pid, &sdef);
        }
        if rescued {
            return None;
        }

        if let Some(c_snapshot) = self
            .players
            .get(owner_pid)
            .and_then(|ps| ps.field.iter().find(|c| c.instance_id == char_inst_id))
            .cloned()
        {
            let eff_ap = self.get_effective_ap(owner_pid, &c_snapshot);
            let eff_lv = self
                .catalog
                .get(&c_snapshot.definition_id)
                .map(|d| d.level + c_snapshot.temp_level_delta)
                .unwrap_or(0);
            self.last_removed_char_state = Some((
                char_inst_id.to_string(),
                c_snapshot.orientation,
                eff_lv,
                eff_ap,
            ));
        }

        let mut removed_sets: Vec<(String, String)> = vec![];
        let mut removed_def: Option<String> = None;
        let mut transferred_set_info: Option<(String, String, bool)> = None;
        if let Some(ps) = self.players.get_mut(owner_pid) {
            if let Some(idx) = ps.field.iter().position(|c| c.instance_id == char_inst_id) {
                let mut card = ps.field.remove(idx);
                Self::reset_turn_card_modifiers(&mut card);
                removed_def = Some(card.definition_id.clone());
                let mut transfer_kaito: Option<CardInstance> = None;
                for mut sc in card.set_cards.drain(..) {
                    let is_b02052 = is_opp_turn
                        && sc.face_up
                        && self
                            .catalog
                            .get(&sc.definition_id)
                            .map(|d| d.description.contains("代わりに自分の現場にいる〚特徴［怪盗］〛のキャラ1枚にセットしてもよい"))
                            .unwrap_or(false);
                    if is_b02052 && transfer_kaito.is_none() {
                        transfer_kaito = Some(sc);
                        continue;
                    }
                    sc.zone = Zone::REMOVE;
                    sc.face_up = true;
                    removed_sets.push((sc.instance_id.clone(), sc.definition_id.clone()));
                    ps.remove.push(sc);
                }
                if let Some(kcard) = transfer_kaito {
                    if let Some(other_kaito) = ps.field.iter_mut().find(|fc| {
                        self.catalog
                            .get(&fc.definition_id)
                            .map(|d| has_trait_match(d, "怪盗"))
                            .unwrap_or(false)
                    }) {
                        transferred_set_info = Some((
                            other_kaito.instance_id.clone(),
                            kcard.definition_id.clone(),
                            kcard.face_up,
                        ));
                        other_kaito.set_cards.push(kcard);
                    } else {
                        let mut kcard = kcard;
                        kcard.zone = Zone::REMOVE;
                        kcard.face_up = true;
                        removed_sets.push((kcard.instance_id.clone(), kcard.definition_id.clone()));
                        ps.remove.push(kcard);
                    }
                }
                let is_mr = Self::is_mr_def(&self.catalog, &card.definition_id);
                if is_opp_turn && is_mr {
                    card.zone = Zone::PARTNER;
                    card.orientation = CardOrientation::ACTIVE;
                    card.face_up = true;
                    ps.extra_partners.push(card);
                } else {
                    card.zone = Zone::REMOVE;
                    card.face_up = true;
                    ps.remove.push(card);
                }
            }
        }
        if let Some((oid, kdef, kfu)) = transferred_set_info {
            self.trigger_on_card_set(owner_pid, &oid, &[(kdef, kfu)]);
        }
        if let Some(ref d) = removed_def {
            if is_opp_turn && self.is_mr_card(d) {
                let cname = self.card_name_by_def(d);
                self.log(format!(
                    "🌟 MR能力: [{}] は相手ターン中に現場を離れたため、パートナーエリアに移動しました！",
                    cname
                ));
            }
        }
        removed_def.map(|d| (d, removed_sets))
    }

    pub(super) fn trigger_set_card_on_remove(&mut self, owner_id: &str, set_inst_id: &str, set_def_id: &str) {
        let Some(def) = self.catalog.get(set_def_id).cloned() else {
            return;
        };
        let is_opp_turn = self.turn_player_id != owner_id;
        let card_name = self.card_name_by_def(set_def_id);
        for line in def.description.lines().map(|l| l.trim()).filter(|l| !l.is_empty()) {
            let clean = strip_parens(line);
            if clean.contains("キャラにセットされていたこのイベントがリムーブエリアに置かれたとき")
                || (clean.contains("このキャラが現場から離れたとき") && clean.contains("を持つ"))
            {
                let mut inner = clean.as_str();
                if let (Some(a), Some(b)) = (clean.find('「'), clean.rfind('」')) {
                    if b > a {
                        inner = &clean[a + '「'.len_utf8()..b];
                    }
                }
                if (clean.contains("【相手ターン中】") || inner.contains("【相手ターン中】")) && !is_opp_turn {
                    continue;
                }
                let ctx = self.build_cond_info(owner_id, Some(set_inst_id), CardOrientation::ACTIVE, None);
                if check_clause_conditions(inner, &ctx).is_err() {
                    continue;
                }
                self.log(format!("💥 セットされていた [{}] の誘発効果が発動！", card_name));
                let plan = parse_effect_steps(inner, None, def.card_type);
                let _ = self.execute_clause_plan(owner_id, set_inst_id, &card_name, Some(def.level), &plan, "MAIN", None);
            }
        }
    }

    pub(super) fn extract_set_card_granted_clauses(&self, desc: &str, trigger: TriggerKind) -> Vec<String> {
        let mut out = Vec::new();
        for line in desc.lines().map(|l| l.trim()).filter(|l| !l.is_empty()) {
            let clean = strip_parens(line);
            if !(clean.contains("このイベントがセットされている") && clean.contains("を持つ")) {
                continue;
            }
            let mut rest = clean.as_str();
            while let Some(a) = rest.find('「') {
                let after = &rest[a + '「'.len_utf8()..];
                // find matching '」'
                if let Some(b) = after.find('」') {
                    let inner = after[..b].trim();
                    let inner_clauses = extract_clauses(inner, trigger, CardType::CHARACTER);
                    for ic in inner_clauses {
                        if !out.contains(&ic) {
                            out.push(ic);
                        }
                    }
                    rest = &after[b + '」'.len_utf8()..];
                } else {
                    break;
                }
            }
        }
        out
    }

    pub(super) fn check_and_pay_set_card_cost(&mut self, player_id: &str, host_id: &str, clause: &str, dry_run: bool) -> bool {
        let clean = strip_parens(clause);
        let Some((left, _)) = clean.split_once('：') else {
            return true;
        };
        if !left.contains("このキャラにセットされている") {
            return true;
        }
        let target_name = extract_bracketed_value(left, "カード名［", "］");
        if dry_run {
            return self.players.get(player_id).and_then(|ps| {
                ps.field.iter().find(|c| c.instance_id == host_id).map(|c| {
                    c.set_cards.iter().any(|sc| {
                        target_name.as_ref().map(|tn| {
                            self.catalog.get(&sc.definition_id).map(|d| has_instance_name_match(sc, d, tn)).unwrap_or(false)
                        }).unwrap_or(true)
                    })
                })
            }).unwrap_or(false);
        }
        let mut removed_sc: Option<(String, String)> = None;
        if let Some(ps) = self.players.get_mut(player_id) {
            if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == host_id) {
                if let Some(pos) = c.set_cards.iter().position(|sc| {
                    target_name.as_ref().map(|tn| {
                        self.catalog.get(&sc.definition_id).map(|d| has_instance_name_match(sc, d, tn)).unwrap_or(false)
                    }).unwrap_or(true)
                }) {
                    let mut sc = c.set_cards.remove(pos);
                    sc.zone = Zone::REMOVE;
                    sc.face_up = true;
                    removed_sc = Some((sc.instance_id.clone(), sc.definition_id.clone()));
                    ps.remove.push(sc);
                }
            }
        }
        if let Some((sid, sdid)) = removed_sc {
            let sname = self.card_name_by_def(&sdid);
            self.log(format!("💸 コスト支払い: セットされていた [{}] をリムーブしました", sname));
            self.trigger_set_card_on_remove(player_id, &sid, &sdid);
            true
        } else {
            false
        }
    }

    /// 現場リムーブ時トリガーを解決する
    pub fn trigger_on_remove(&mut self, owner_id: &str, definition_id: &str) {
        self.trigger_on_remove_char(owner_id, "", definition_id, false);
    }

    /// モーダルUIが既に開いている場合はキューへ退避し、そうでなければ即座に句プランを実行する
    pub(super) fn execute_or_enqueue_plan(
        &mut self,
        player_id: &str,
        actor_id: &str,
        actor_name: &str,
        source_level: Option<i32>,
        card_type: CardType,
        plan: ParsedClausePlan,
    ) {
        let is_main_action_pending = self
            .pending_decision
            .as_ref()
            .and_then(|pd| pd.get("kind").and_then(|k| k.as_str()))
            == Some("mainAction");
        if (self.pending_decision.is_some() && !is_main_action_pending)
            || self.is_executing_clause_plan
        {
            self.pending_clause_queue.push(PendingClauseTask {
                player_id: player_id.to_string(),
                actor_id: actor_id.to_string(),
                actor_name: actor_name.to_string(),
                source_level,
                card_type,
                clause: String::new(),
                resume_context: "MAIN".to_string(),
                evidence_source_inst_id: None,
                post_target_ops: vec![],
                plan: Some(plan),
            });
        } else {
            let saved_main = if is_main_action_pending {
                self.pending_decision.take()
            } else {
                None
            };
            let _ = self.execute_clause_plan(
                player_id,
                actor_id,
                actor_name,
                source_level,
                &plan,
                "MAIN",
                None,
            );
            if self.pending_decision.is_none() {
                if let Some(m) = saved_main {
                    self.pending_decision = Some(m);
                }
            }
        }
    }

    /// キャラにカードがセットされたときの誘発効果（「〜がセットされるたび」「〜がセットされたとき」）を解決する
    pub(super) fn trigger_on_card_set(
        &mut self,
        owner_pid: &str,
        target_cid: &str,
        added_cards: &[(String, bool)],
    ) {
        if added_cards.is_empty() {
            return;
        }
        let Some(target_def_id) = self
            .players
            .get(owner_pid)
            .and_then(|ps| ps.field.iter().find(|c| c.instance_id == target_cid))
            .map(|c| c.definition_id.clone())
        else {
            return;
        };
        let Some(def) = self.catalog.get(&target_def_id).cloned() else {
            return;
        };
        let clauses = extract_clauses(&def.description, TriggerKind::OnCardSet, def.card_type);
        if clauses.is_empty() {
            return;
        }
        let target_name = self.card_name_by_def(&def.definition_id);

        for clause in clauses {
            let clean_clause = strip_parens(&clause);
            let req_trait: Option<String> = clean_clause
                .find("がセット")
                .and_then(|pos| {
                    let prefix = &clean_clause[..pos];
                    prefix.find("〚特徴［").and_then(|s| {
                        let rest = &prefix[s + "〚特徴［".len()..];
                        rest.find("］〛").map(|e| rest[..e].to_string())
                    })
                });
            let is_per_card = clean_clause.contains("がセットされるたび");

            for (set_def_id, face_up) in added_cards {
                if let Some(ref tr) = req_trait {
                    if !*face_up {
                        continue;
                    }
                    let matched = self
                        .catalog
                        .get(set_def_id)
                        .map(|d| has_trait_match(d, tr))
                        .unwrap_or(false);
                    if !matched {
                        continue;
                    }
                }

                let Some((ori, entered_turn, has_rush)) = self
                    .players
                    .get(owner_pid)
                    .and_then(|ps| ps.field.iter().find(|c| c.instance_id == target_cid))
                    .map(|c| (c.orientation, c.entered_turn, c.has_rush))
                else {
                    break;
                };

                let ctx = self.build_cond_info(owner_pid, Some(target_cid), ori, Some(def.level));
                if let Err(reason) = check_clause_conditions(&clause, &ctx) {
                    self.log(format!(
                        "⚠️ [{}] のセット時効果不発: {}",
                        target_name, reason
                    ));
                    continue;
                }
                if !self.can_use_clause_this_turn(target_cid, &clause) {
                    break;
                }
                self.mark_clause_used_this_turn(target_cid, &clause);

                if clean_clause.contains("このキャラをアクティブにするか、")
                    && clean_clause.contains("〚突撃〛")
                {
                    if ori == CardOrientation::ACTIVE {
                        if let Some(ps) = self.players.get_mut(owner_pid) {
                            if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == target_cid) {
                                c.has_rush = true;
                            }
                        }
                        self.log(format!(
                            "✨ [{}] のセット時効果でターン終了時まで〚突撃〛を得ました！",
                            target_name
                        ));
                    } else if has_rush || entered_turn < self.turn {
                        let mut was_stun = false;
                        if let Some(ps) = self.players.get_mut(owner_pid) {
                            if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == target_cid) {
                                if c.orientation == CardOrientation::STUN {
                                    c.orientation = CardOrientation::SLEEP;
                                    was_stun = true;
                                } else {
                                    c.orientation = CardOrientation::ACTIVE;
                                }
                            }
                        }
                        if was_stun {
                            self.log(format!(
                                "✨ [{}] のセット時効果でスタン状態の自身をスリープにしました！",
                                target_name
                            ));
                        } else {
                            self.log(format!(
                                "✨ [{}] のセット時効果で自身をアクティブにしました！",
                                target_name
                            ));
                        }
                    } else {
                        let mc = EventModeChoice {
                            min_pick: 1,
                            max_pick: 1,
                            distinct_colors_limit: false,
                            pre_clauses: vec![],
                            mode_gate_condition: None,
                            options: vec![
                                "このキャラをアクティブにする。".to_string(),
                                "ターン終了時までこのキャラは〚突撃〛を持つ。".to_string(),
                            ],
                            all_condition: None,
                            optional_all_cost: None,
                            post_clauses: vec![],
                        };
                        let plan = ParsedClausePlan {
                            immediate_before: vec![ImmediateOp::EventModeChoice(mc)],
                            ..Default::default()
                        };
                        self.execute_or_enqueue_plan(
                            owner_pid,
                            target_cid,
                            &target_name,
                            Some(def.level),
                            def.card_type,
                            plan,
                        );
                    }
                } else {
                    let resolved = resolve_state_branch(&clause, ori);
                    let plan = parse_effect_steps(&resolved, Some(def.level), def.card_type);
                    self.execute_or_enqueue_plan(
                        owner_pid,
                        target_cid,
                        &target_name,
                        Some(def.level),
                        def.card_type,
                        plan,
                    );
                }

                if !is_per_card {
                    break;
                }
            }
        }
    }

    /// 現場リムーブ時トリガーを解決する（インスタンスID・自分の能力/効果によるリムーブかを指定）
    pub fn trigger_on_remove_char(
        &mut self,
        owner_id: &str,
        instance_id: &str,
        definition_id: &str,
        by_own_ability: bool,
    ) {
        let Some(removed_def) = self.catalog.get(definition_id).cloned() else {
            return;
        };
        let card_name = self.card_name_by_def(definition_id);
        let (rem_ori, rem_lv, rem_ap) = match self.last_removed_char_state.take() {
            Some((id, o, lv, ap)) if id == instance_id || instance_id.is_empty() => (o, lv, ap),
            _ => (CardOrientation::ACTIVE, removed_def.level, removed_def.ap),
        };

        // 1) リムーブされたキャラ自身の【現場リムーブ時】能力（および自己能力リムーブ時効果）
        let self_clauses = extract_clauses(&removed_def.description, TriggerKind::OnRemove, removed_def.card_type);
        for clause in self_clauses {
            if clause.contains("自分の能力や効果によってリムーブされたとき") && !by_own_ability {
                continue;
            }
            if clause.contains("このターン中にこのキャラがアクションしていた場合")
                && (instance_id.is_empty() || !self.acted_this_turn_ids.contains(instance_id))
            {
                continue;
            }
            let ctx = self.build_cond_info(owner_id, None, CardOrientation::ACTIVE, None);
            if check_clause_conditions(&clause, &ctx).is_err() {
                continue;
            }
            self.log(format!("💥 [{}] の【現場リムーブ時】能力が誘発しました！", card_name));
            let cost = parse_clause_cost(&clause);
            if !self.pay_clause_cost(owner_id, instance_id, &cost) {
                continue;
            }
            let plan = parse_effect_steps(&clause, None, removed_def.card_type);
            self.execute_or_enqueue_plan(
                owner_id,
                instance_id,
                &card_name,
                Some(removed_def.level),
                removed_def.card_type,
                plan,
            );
        }
        if !instance_id.is_empty() && self.granted_on_remove_draw_until_opp_end.remove(instance_id).is_some() {
            self.draw_cards(owner_id, 1);
            self.log(format!("🎴 [{}] に付与された【現場リムーブ時】効果でカードを1枚引きました！", card_name));
        }

        // 2) 現場・パートナー等の常駐ウォッチャーによる「キャラがリムーブされたとき」誘発（B05106 ジン＆ウォッカ等）
        let opp_id = self.opponent_id_of(owner_id);
        let turn_pid = self.turn_player_id.clone();
        let watch_order: Vec<(String, bool)> = if turn_pid == owner_id {
            vec![(owner_id.to_string(), true), (opp_id, false)]
        } else {
            vec![(opp_id, false), (owner_id.to_string(), true)]
        };

        for (watch_pid, owner_is_watcher) in watch_order {
            if watch_pid.is_empty() {
                continue;
            }
            let mut watchers: Vec<(String, String, CardOrientation, bool, bool)> = self
                .players
                .get(&watch_pid)
                .map(|ps| {
                    let mut list: Vec<(String, String, CardOrientation, bool, bool)> = ps
                        .field
                        .iter()
                        .map(|c| (c.instance_id.clone(), c.definition_id.clone(), c.orientation, false, false))
                        .collect();
                    if let Some(ref p) = ps.partner {
                        if p.zone == Zone::PARTNER {
                            list.push((p.instance_id.clone(), p.definition_id.clone(), p.orientation, true, false));
                        }
                    }
                    for ep in &ps.extra_partners {
                        list.push((ep.instance_id.clone(), ep.definition_id.clone(), ep.orientation, true, false));
                    }
                    list
                })
                .unwrap_or_default();

            if owner_is_watcher && !instance_id.is_empty() {
                watchers.push((
                    instance_id.to_string(),
                    definition_id.to_string(),
                    rem_ori,
                    false,
                    true,
                ));
            }

            for (watcher_id, wdef_id, ori, in_partner, is_removed_self) in watchers {
                let Some(wdef) = self.catalog.get(&wdef_id).cloned() else {
                    continue;
                };
                let watcher_name = self.card_name_by_def(&wdef_id);
                for clause in extract_clauses(&wdef.description, TriggerKind::OnCharRemoved, wdef.card_type) {
                    if in_partner && wdef.card_type != CardType::PARTNER && !allows_partner_area(&clause) {
                        continue;
                    }
                    if is_removed_self
                        && !clause.contains("このキャラか")
                        && !clause.contains("スリープ状態のこのキャラ")
                    {
                        continue;
                    }
                    if !check_char_removed_match(
                        &clause,
                        owner_is_watcher,
                        &watcher_id,
                        instance_id,
                        &removed_def,
                        rem_ap,
                        rem_lv,
                        rem_ori,
                        by_own_ability,
                    ) {
                        continue;
                    }
                    let ctx = self.build_cond_info(&watch_pid, Some(&watcher_id), ori, None);
                    if check_clause_conditions(&clause, &ctx).is_err() {
                        continue;
                    }
                    if !self.can_use_clause_this_turn(&watcher_id, &clause) {
                        continue;
                    }
                    let cost = parse_clause_cost(&clause);
                    if !self.pay_clause_cost(&watch_pid, &watcher_id, &cost) {
                        continue;
                    }
                    self.mark_clause_used_this_turn(&watcher_id, &clause);
                    self.log(format!("✨ [{}] の誘発効果（キャラリムーブ時）が発動！", watcher_name));
                    let plan = parse_effect_steps(&clause, None, wdef.card_type);
                    self.execute_or_enqueue_plan(
                        &watch_pid,
                        &watcher_id,
                        &watcher_name,
                        Some(wdef.level),
                        wdef.card_type,
                        plan,
                    );
                }
            }
        }
    }

    /// ターン終了時トリガーを解決する
    pub fn trigger_on_turn_end(&mut self, current_turn_pid: &str) {
        // 「自分か相手のターン終了時」もあるため自分・相手両方を走査し、フィールド・パートナー・セットカードも評価する
        struct EndTask {
            pid: String,
            actor_id: String,
            host_id: String,
            def_id: String,
            ori: CardOrientation,
            clause: String,
        }
        let mut effects_to_trigger: Vec<EndTask> = vec![];
        let opp_pid = self
            .player_order
            .iter()
            .find(|p| *p != current_turn_pid)
            .cloned()
            .unwrap_or_default();

        for pid in [current_turn_pid.to_string(), opp_pid] {
            if pid.is_empty() {
                continue;
            }
            if let Some(ps) = self.players.get(&pid) {
                for c in &ps.field {
                    if let Some(c_def) = self.catalog.get(&c.definition_id) {
                        let mut end_clauses = extract_clauses(&c_def.description, TriggerKind::OnTurnEnd, c_def.card_type);
                        if let Some(granted_list) = self.granted_abilities_this_turn.get(&c.instance_id) {
                            for g in granted_list {
                                end_clauses.extend(extract_clauses(g, TriggerKind::OnTurnEnd, CardType::CHARACTER));
                            }
                        }
                        for clause in end_clauses {
                            let is_opp_end = clause.contains("相手のターン終了時") || clause.contains("自分か相手のターン終了時");
                            let is_self_end = clause.contains("自分のターン終了時") || clause.contains("自分か相手のターン終了時") || (!clause.contains("相手の"));
                            if (pid == current_turn_pid && is_self_end) || (pid != current_turn_pid && is_opp_end) {
                                effects_to_trigger.push(EndTask {
                                    pid: pid.clone(),
                                    actor_id: c.instance_id.clone(),
                                    host_id: c.instance_id.clone(),
                                    def_id: c.definition_id.clone(),
                                    ori: c.orientation,
                                    clause,
                                });
                            }
                        }
                    }
                    for sc in &c.set_cards {
                        if !sc.face_up {
                            continue;
                        }
                        if let Some(sc_def) = self.catalog.get(&sc.definition_id) {
                            let mut clauses = extract_clauses(&sc_def.description, TriggerKind::OnTurnEnd, sc_def.card_type);
                            for inner in self.extract_set_card_granted_clauses(&sc_def.description, TriggerKind::OnTurnEnd) {
                                if !clauses.contains(&inner) {
                                    clauses.push(inner);
                                }
                            }
                            for clause in clauses {
                                let is_opp_end = clause.contains("相手のターン終了時") || clause.contains("自分か相手のターン終了時");
                                let is_self_end = clause.contains("自分のターン終了時") || clause.contains("自分か相手のターン終了時") || (!clause.contains("相手の"));
                                if (pid == current_turn_pid && is_self_end) || (pid != current_turn_pid && is_opp_end) {
                                    effects_to_trigger.push(EndTask {
                                        pid: pid.clone(),
                                        actor_id: sc.instance_id.clone(),
                                        host_id: c.instance_id.clone(),
                                        def_id: sc.definition_id.clone(),
                                        ori: c.orientation,
                                        clause,
                                    });
                                }
                            }
                        }
                    }
                }
                if let Some(ref p) = ps.partner {
                    if let Some(p_def) = self.catalog.get(&p.definition_id) {
                        for clause in extract_clauses(&p_def.description, TriggerKind::OnTurnEnd, p_def.card_type) {
                            if pid == current_turn_pid || clause.contains("相手のターン終了時") || clause.contains("自分か相手のターン終了時") {
                                effects_to_trigger.push(EndTask {
                                    pid: pid.clone(),
                                    actor_id: p.instance_id.clone(),
                                    host_id: p.instance_id.clone(),
                                    def_id: p.definition_id.clone(),
                                    ori: p.orientation,
                                    clause,
                                });
                            }
                        }
                    }
                }
                for ep in &ps.extra_partners {
                    if let Some(ep_def) = self.catalog.get(&ep.definition_id) {
                        for clause in extract_clauses(&ep_def.description, TriggerKind::OnTurnEnd, ep_def.card_type) {
                            if ep_def.card_type != CardType::PARTNER && !allows_partner_area(&clause) {
                                continue;
                            }
                            if pid == current_turn_pid || clause.contains("相手のターン終了時") || clause.contains("自分か相手のターン終了時") {
                                effects_to_trigger.push(EndTask {
                                    pid: pid.clone(),
                                    actor_id: ep.instance_id.clone(),
                                    host_id: ep.instance_id.clone(),
                                    def_id: ep.definition_id.clone(),
                                    ori: ep.orientation,
                                    clause,
                                });
                            }
                        }
                    }
                }
            }
        }

        for (tidx, task) in effects_to_trigger.iter().enumerate() {
            let Some(def) = self.catalog.get(&task.def_id).cloned() else {
                continue;
            };
            let card_name = self.card_name_by_def(&task.def_id);
            let ctx = self.build_cond_info(&task.pid, Some(&task.host_id), task.ori, None);
            if check_clause_conditions(&task.clause, &ctx).is_err() {
                continue;
            }
            let resolved = resolve_state_branch(&task.clause, task.ori);
            let plan = parse_effect_steps(&resolved, None, def.card_type);
            // 後続ターン終了時タスクをキューに積んでおきUI停止でも順次発火させる
            for rem in effects_to_trigger[tidx + 1..].iter().rev() {
                let rname = self.card_name_by_def(&rem.def_id);
                let rlv = self.catalog.get(&rem.def_id).map(|d| d.level);
                let rtype = self.catalog.get(&rem.def_id).map(|d| d.card_type).unwrap_or(CardType::CHARACTER);
                self.pending_clause_queue.insert(
                    0,
                    PendingClauseTask {
                        player_id: rem.pid.clone(),
                        actor_id: rem.actor_id.clone(),
                        actor_name: rname,
                        source_level: rlv,
                        card_type: rtype,
                        clause: rem.clause.clone(),
                        resume_context: "END_TURN".to_string(),
                        evidence_source_inst_id: None,
                        post_target_ops: vec![],
                        plan: None,
                    },
                );
            }
            if self.execute_clause_plan(&task.pid, &task.actor_id, &card_name, Some(def.level), &plan, "END_TURN", None) {
                return;
            }
            if self.drain_pending_clause_queue() {
                return;
            }
            break;
        }
    }

    /// アクション・推理宣言時トリガーを解決する
    pub fn trigger_on_action(
        &mut self,
        player_id: &str,
        actor_id: &str,
        kind: ActionEventKind,
        target_id: Option<&str>,
        resume_context: &str,
    ) -> bool {
        let actor_card_opt = self.players.get(player_id).and_then(|ps| {
            ps.field
                .iter()
                .find(|c| c.instance_id == actor_id)
                .or_else(|| ps.partner.as_ref().filter(|p| p.instance_id == actor_id))
                .cloned()
        });
        let Some(actor_card) = actor_card_opt else {
            return false;
        };
        let Some(actor_def) = self.catalog.get(&actor_card.definition_id).cloned() else {
            return false;
        };
        let actor_ap = self.get_effective_ap(player_id, &actor_card);
        let actor_name = self.card_name_by_def(&actor_card.definition_id);

        if actor_card.draw_on_action_this_turn {
            self.draw_cards(player_id, 1);
            self.log(format!("✨ [{}] の付与効果でカードを1枚引きました", actor_name));
        }

        if kind == ActionEventKind::AttackCase && actor_card.bullet_on_case_attack_discard_this_turn {
            let has_hand = self.players.get(player_id).map(|p| !p.hand.is_empty()).unwrap_or(false);
            if has_hand {
                self.last_chosen_instance_id = Some(actor_id.to_string());
                let follow_ts = TargetStep {
                    action: TargetAction::GiveBullet,
                    filter: TargetFilter {
                        owner: OwnerScope::SelfOnly,
                        target_prev_chosen: true,
                        ..Default::default()
                    },
                    bonuses: vec![],
                    follow_up: vec![],
                    conditional_follow_up: None,
                    optional: false,
                    then_abort_if_skipped: false,
                    scale_by_cost_flipped_evidence: false,
                };
                if self.prompt_hand_discard(
                    player_id,
                    actor_id,
                    &actor_name,
                    Some(actor_def.level),
                    1,
                    None,
                    true,
                    true,
                    &[],
                    Some(&follow_ts),
                    None,
                    None,
                    resume_context,
                    None,
                ) {
                    return true;
                }
            }
        }

        let opp_pid = self.opponent_id_of(player_id);
        let target_info = target_id.and_then(|tid| {
            self.players.get(&opp_pid).and_then(|ops| {
                ops.field.iter().find(|c| c.instance_id == tid).and_then(|tc| {
                    self.catalog
                        .get(&tc.definition_id)
                        .cloned()
                        .map(|tdef| (tdef, self.get_effective_ap(&opp_pid, tc)))
                })
            })
        });

        let watch_sides: Vec<(String, bool)> = vec![(player_id.to_string(), true), (opp_pid.clone(), false)];
        let mut opened_ui = false;
        for (watch_pid, owner_is_acting) in watch_sides {
            let watchers: Vec<(String, String, CardOrientation, bool)> = self
                .players
                .get(&watch_pid)
                .map(|ps| {
                    let mut list: Vec<(String, String, CardOrientation, bool)> = ps
                        .field
                        .iter()
                        .map(|c| (c.instance_id.clone(), c.definition_id.clone(), c.orientation, false))
                        .collect();
                    if let Some(ref p) = ps.partner {
                        if p.zone == Zone::PARTNER {
                            list.push((p.instance_id.clone(), p.definition_id.clone(), p.orientation, true));
                        }
                    }
                    for ep in &ps.extra_partners {
                        list.push((ep.instance_id.clone(), ep.definition_id.clone(), ep.orientation, true));
                    }
                    if let Some(ref cs) = ps.r#case {
                        list.push((cs.instance_id.clone(), cs.definition_id.clone(), cs.orientation, false));
                    }
                    list
                })
                .unwrap_or_default();

            for (watcher_id, def_id, ori, in_partner) in watchers {
                let Some(def) = self.catalog.get(&def_id).cloned() else {
                    continue;
                };
                let watcher_name = self.card_name_by_def(&def_id);
                for clause in extract_clauses(&def.description, TriggerKind::OnAction, def.card_type) {
                    if in_partner && def.card_type != CardType::PARTNER && !allows_partner_area(&clause) {
                        continue;
                    }
                    let watcher_under_len = self
                        .players
                        .get(&watch_pid)
                        .and_then(|ps| ps.field.iter().find(|c| c.instance_id == watcher_id))
                        .map(|c| c.set_cards.len())
                        .unwrap_or(0);
                    if !check_action_trigger_match(
                        &clause,
                        kind,
                        owner_is_acting,
                        &watcher_id,
                        watcher_under_len,
                        actor_id,
                        &actor_def,
                        actor_ap,
                        target_id,
                        target_info.as_ref().map(|(d, _)| d),
                        target_info.as_ref().map(|(_, ap)| *ap),
                    ) {
                        continue;
                    }
                    let ctx = self.build_cond_info(&watch_pid, Some(&watcher_id), ori, None);
                    if check_clause_conditions(&clause, &ctx).is_err() {
                        continue;
                    }
                    if !self.can_use_clause_this_turn(&watcher_id, &clause) {
                        continue;
                    }
                    let cost = parse_clause_cost(&clause);
                    if !self.pay_clause_cost(&watch_pid, &watcher_id, &cost) {
                        continue;
                    }
                    self.mark_clause_used_this_turn(&watcher_id, &clause);
                    let kind_label = match kind {
                        ActionEventKind::Deduce => "推理時",
                        ActionEventKind::AttackCase => "アクション［事件］時",
                        ActionEventKind::AttackChar => "アクション時",
                    };
                    self.log(format!("✨ [{}] の誘発効果（{}）が発動！", watcher_name, kind_label));

                    let plan = parse_effect_steps(&clause, None, def.card_type);
                    if watcher_id != actor_id && clause.contains("そのキャラをAP") && !clause.contains("このキャラをAP") {
                        for op in &plan.immediate_before {
                            match op {
                                ImmediateOp::SelfApBuff(amt) => {
                                    if let Some(ps) = self.players.get_mut(player_id) {
                                        if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == actor_id) {
                                            c.temp_ap_bonus += *amt;
                                        }
                                    }
                                    self.log(format!("✨ [{}] の効果: [{}] のAP +{}", watcher_name, actor_name, amt));
                                }
                                other => {
                                    self.execute_immediate_ops(&watch_pid, &watcher_id, &watcher_name, &[other.clone()], None);
                                }
                            }
                        }
                        continue;
                    }
                    if opened_ui || self.pending_decision.is_some() {
                        self.pending_clause_queue.push(PendingClauseTask {
                            player_id: watch_pid.clone(),
                            actor_id: watcher_id.clone(),
                            actor_name: watcher_name.clone(),
                            source_level: Some(def.level),
                            card_type: def.card_type,
                            clause: String::new(),
                            resume_context: resume_context.to_string(),
                            evidence_source_inst_id: None,
                            post_target_ops: vec![],
                            plan: Some(plan),
                        });
                    } else if self.execute_clause_plan(
                        &watch_pid,
                        &watcher_id,
                        &watcher_name,
                        Some(def.level),
                        &plan,
                        resume_context,
                        None,
                    ) {
                        opened_ui = true;
                    }
                }
            }
        }
        opened_ui
    }

    /// コンタクト突入時トリガーを解決する
    pub fn trigger_on_contact(&mut self, attacker_pid: &str, actor_id: &str, defender_pid: &str, def_cid: &str) {
        // 攻撃側「このキャラがコンタクトしたとき」と付与効果
        if let Some(a_card) = self.players.get(attacker_pid).and_then(|p| p.field.iter().find(|c| c.instance_id == actor_id)).cloned() {
            let name = self.card_name_by_def(&a_card.definition_id);
            if a_card.contact_draw_and_ap1000_this_turn {
                self.draw_cards(attacker_pid, 1);
                self.log(format!("✨ [{}] の付与効果（コンタクト時）でカードを1枚引き、このコンタクト中AP＋1000！", name));
            }
            if let Some(def) = self.catalog.get(&a_card.definition_id).cloned() {
                let ori = a_card.orientation;
                let mut contact_clauses = extract_clauses(&def.description, TriggerKind::OnContact, def.card_type);
                if let Some(granted_list) = self.granted_abilities_this_turn.get(actor_id) {
                    for g in granted_list {
                        contact_clauses.extend(extract_clauses(g, TriggerKind::OnContact, CardType::CHARACTER));
                    }
                }
                for clause in contact_clauses {
                    if !clause.contains("コンタクトしたとき") {
                        continue;
                    }
                    let ctx = self.build_cond_info(attacker_pid, Some(actor_id), ori, None);
                    if check_clause_conditions(&clause, &ctx).is_ok() {
                        let plan = parse_effect_steps(&clause, None, def.card_type);
                        self.execute_immediate_ops(attacker_pid, actor_id, &name, &plan.immediate_before, None);
                    }
                }
            }
        }

        // 防御側「このキャラを指定してアクションしたとき」 / 「コンタクトされたとき」と付与効果
        if let Some(d_card) = self.players.get(defender_pid).and_then(|p| p.field.iter().find(|c| c.instance_id == def_cid)).cloned() {
            let name = self.card_name_by_def(&d_card.definition_id);
            if d_card.contact_draw_and_ap1000_this_turn {
                self.draw_cards(defender_pid, 1);
                self.log(format!("✨ [{}] の付与効果（コンタクト時）でカードを1枚引き、このコンタクト中AP＋1000！", name));
            }
            if let Some(def) = self.catalog.get(&d_card.definition_id).cloned() {
                let ori = d_card.orientation;
                for line in def.description.lines() {
                    if !(line.contains("このキャラを指定してアクションしたとき") || line.contains("コンタクトされたとき")) {
                        continue;
                    }
                    let ctx = self.build_cond_info(defender_pid, Some(def_cid), ori, None);
                    if check_clause_conditions(line, &ctx).is_ok() {
                        let plan = parse_effect_steps(line, None, def.card_type);
                        self.execute_immediate_ops(defender_pid, def_cid, &name, &plan.immediate_before, None);
                    }
                }
            }
        }

        // 味方の現場・パートナーエリアからの「自分の現場にいるレベルN以上のキャラがコンタクトしたとき、そのコンタクト中、そのキャラをAP＋Xする」
        for (owner_pid, contact_cid) in [(attacker_pid, actor_id), (defender_pid, def_cid)] {
            let contact_lv = self.players.get(owner_pid)
                .and_then(|p| p.field.iter().find(|c| c.instance_id == contact_cid))
                .map(|c| self.catalog.get(&c.definition_id).map(|d| d.level).unwrap_or(0) + c.temp_level_delta)
                .unwrap_or(0);
            let watchers: Vec<(String, String, CardOrientation, bool)> = self.players.get(owner_pid).map(|ps| {
                let mut v: Vec<_> = ps.field.iter().map(|c| (c.instance_id.clone(), c.definition_id.clone(), c.orientation, false)).collect();
                for ep in &ps.extra_partners {
                    v.push((ep.instance_id.clone(), ep.definition_id.clone(), ep.orientation, true));
                }
                v
            }).unwrap_or_default();

            for (wid, wdef_id, ori, in_partner) in watchers {
                let Some(wdef) = self.catalog.get(&wdef_id).cloned() else {
                    continue;
                };
                let wname = self.card_name_by_def(&wdef_id);
                for clause in extract_clauses(&wdef.description, TriggerKind::OnContact, wdef.card_type) {
                    if in_partner && !allows_partner_area(&clause) {
                        continue;
                    }
                    if !clause.contains("自分の現場にいる") || !clause.contains("コンタクトしたとき") {
                        continue;
                    }
                    if let Some(min_lv) = crate::effects::extract_number_before(&clause, "レベル", "以上のキャラがコンタクトしたとき") {
                        if contact_lv < min_lv {
                            continue;
                        }
                    }
                    let ctx = self.build_cond_info(owner_pid, Some(&wid), ori, None);
                    if check_clause_conditions(&clause, &ctx).is_err() {
                        continue;
                    }
                    if let Some(ap_buff) = crate::effects::parse_signed_ap(&clause, true) {
                        self.log(format!("✨ [{}] の効果（コンタクト時）でコンタクト中のキャラをAP＋{}！", wname, ap_buff));
                    }
                }
            }
        }
    }

    /// コンタクトによる相手キャラリムーブ時のトリガーを解決する
    pub fn trigger_on_contact_win(&mut self, attacker_pid: &str, actor_id: &str, actor_def_id: &str) {
        let actor_ap = self.players.get(attacker_pid)
            .and_then(|ps| ps.field.iter().find(|c| c.instance_id == actor_id))
            .map(|c| self.get_effective_ap_with_contact(attacker_pid, c, true))
            .unwrap_or(0);
        let actor_is_kyogoku = self.catalog.get(actor_def_id)
            .map(|d| d.names.iter().any(|n| n == "京極真"))
            .unwrap_or(false);

        let field_cards: Vec<(String, String, CardOrientation)> = self
            .players
            .get(attacker_pid)
            .map(|ps| {
                ps.field
                    .iter()
                    .map(|c| (c.instance_id.clone(), c.definition_id.clone(), c.orientation))
                    .collect()
            })
            .unwrap_or_default();

        for (watcher_id, wdef_id, ori) in field_cards {
            let Some(def) = self.catalog.get(&wdef_id).cloned() else {
                continue;
            };
            let mut clauses = extract_clauses(&def.description, TriggerKind::OnContactWin, def.card_type);
            if let Some(granted_list) = self.granted_abilities_this_turn.get(&watcher_id) {
                for g in granted_list {
                    clauses.extend(extract_clauses(g, TriggerKind::OnContactWin, CardType::CHARACTER));
                }
            }
            if clauses.is_empty() {
                continue;
            }
            let card_name = self.card_name_by_def(&wdef_id);

            for clause in clauses {
                if !self.can_use_clause_this_turn(&watcher_id, &clause) {
                    continue;
                }
                let is_self_only = clause.contains("このキャラとのコンタクト") || !clause.contains("自分の現場にいる");
                if is_self_only && watcher_id != actor_id {
                    continue;
                }
                if clause.contains("〚カード名［京極真］〛以外の") && actor_is_kyogoku {
                    continue;
                }
                if let Some(min_ap) = crate::effects::extract_number_before(&clause, "AP", "以上のキャラとのコンタクト") {
                    if actor_ap < min_ap {
                        continue;
                    }
                }
                let ctx = self.build_cond_info(attacker_pid, Some(&watcher_id), ori, None);
                if check_clause_conditions(&clause, &ctx).is_err() {
                    continue;
                }
                let cost = parse_clause_cost(&clause);
                if !self.pay_clause_cost(attacker_pid, &watcher_id, &cost) {
                    continue;
                }
                self.mark_clause_used_this_turn(&watcher_id, &clause);
                let plan = parse_effect_steps(&clause, None, def.card_type);
                self.execute_or_enqueue_plan(
                    attacker_pid,
                    &watcher_id,
                    &card_name,
                    Some(def.level),
                    def.card_type,
                    plan,
                );
            }
        }
    }

    /// アクション［事件］や証拠獲得時の誘発効果（D04007 メアリー等）を解決する
    pub fn trigger_on_evidence_gain(&mut self, player_id: &str, actor_id: Option<&str>, by_case_action: bool) {
        let field_cards: Vec<(String, String, CardOrientation)> = self
            .players
            .get(player_id)
            .map(|ps| {
                ps.field
                    .iter()
                    .map(|c| (c.instance_id.clone(), c.definition_id.clone(), c.orientation))
                    .collect()
            })
            .unwrap_or_default();

        for (watcher_id, def_id, ori) in field_cards {
            let Some(def) = self.catalog.get(&def_id).cloned() else {
                continue;
            };
            let watcher_name = self.card_name_by_def(&def_id);
            for clause in extract_clauses(&def.description, TriggerKind::OnEvidenceGain, def.card_type) {
                let is_self_case_trigger = clause.contains("このキャラのアクション［事件］によって");
                let is_ally_case_trigger = clause.contains("自分の現場にいるキャラのアクション［事件］によって");
                let is_any_gain_trigger = clause.contains("自分が証拠を得たとき");

                let matched = if is_self_case_trigger {
                    by_case_action && actor_id == Some(watcher_id.as_str())
                } else if is_ally_case_trigger {
                    by_case_action
                } else if is_any_gain_trigger {
                    true
                } else {
                    by_case_action
                };
                if !matched {
                    continue;
                }

                let ctx = self.build_cond_info(player_id, Some(&watcher_id), ori, None);
                if check_clause_conditions(&clause, &ctx).is_err() {
                    continue;
                }
                if !self.can_use_clause_this_turn(&watcher_id, &clause) {
                    continue;
                }
                let cost = parse_clause_cost(&clause);
                if !self.pay_clause_cost(player_id, &watcher_id, &cost) {
                    continue;
                }
                self.mark_clause_used_this_turn(&watcher_id, &clause);
                self.log(format!("✨ [{}] の誘発効果（証拠獲得時）が発動！", watcher_name));

                let plan = parse_effect_steps(&clause, None, def.card_type);
                self.execute_or_enqueue_plan(
                    player_id,
                    &watcher_id,
                    &watcher_name,
                    Some(def.level),
                    def.card_type,
                    plan,
                );
            }
        }
    }

    /// 変装時トリガー（【変装時】および独立した【登場時】）を解決する
    pub fn trigger_on_disguise(&mut self, player_id: &str, actor_id: &str, definition_id: &str) -> bool {
        self.trigger_on_disguise_with_ctx(player_id, actor_id, definition_id, "MAIN")
    }

    pub fn trigger_on_disguise_with_ctx(
        &mut self,
        player_id: &str,
        actor_id: &str,
        definition_id: &str,
        resume_context: &str,
    ) -> bool {
        let Some(def) = self.catalog.get(definition_id).cloned() else {
            return false;
        };
        let card_name = self.card_name_by_def(definition_id);
        let ori = self.find_actor_orientation(player_id, actor_id);

        // 1) 【変装時】句の解決
        for clause in extract_clauses(&def.description, TriggerKind::OnDisguise, def.card_type) {
            let ctx = self.build_cond_info(player_id, Some(actor_id), ori, None);
            if check_clause_conditions(&clause, &ctx).is_err() {
                continue;
            }
            let cost = parse_clause_cost(&clause);
            if !self.pay_clause_cost(player_id, actor_id, &cost) {
                continue;
            }
            let plan = parse_effect_steps(&clause, None, def.card_type);
            if self.execute_clause_plan(player_id, actor_id, &card_name, Some(def.level), &plan, resume_context, None) {
                return true;
            }
        }

        // 2) 別行の【登場時】句があれば解決（B02038など。「【登場時】【変装時】」1行兼用は二重発動させない）
        for clause in extract_clauses(&def.description, TriggerKind::OnPlay, def.card_type) {
            if clause.contains("【変装時】") {
                continue;
            }
            let ctx = self.build_cond_info(player_id, Some(actor_id), ori, None);
            if check_clause_conditions(&clause, &ctx).is_err() {
                continue;
            }
            let cost = parse_clause_cost(&clause);
            if !self.pay_clause_cost(player_id, actor_id, &cost) {
                continue;
            }
            let plan = parse_effect_steps(&clause, None, def.card_type);
            if self.execute_clause_plan(player_id, actor_id, &card_name, Some(def.level), &plan, resume_context, None) {
                return true;
            }
        }

        false
    }

    /// ネクストヒントで手札を使用したときのトリガー（例: B01005 / B05005）
    pub fn trigger_on_hint_use(&mut self, player_id: &str, hint_card_level: i32) -> bool {
        self.trigger_on_hint_use_with_colors(player_id, hint_card_level, &[])
    }

    pub fn trigger_on_hint_use_with_colors(
        &mut self,
        player_id: &str,
        hint_card_level: i32,
        hint_colors: &[String],
    ) -> bool {
        let actors: Vec<(String, String, CardOrientation, bool)> = {
            let Some(ps) = self.players.get(player_id) else {
                return false;
            };
            let mut list = vec![];
            for c in &ps.field {
                list.push((c.instance_id.clone(), c.definition_id.clone(), c.orientation, false));
            }
            if let Some(ref p) = ps.partner {
                if p.zone == Zone::PARTNER {
                    list.push((p.instance_id.clone(), p.definition_id.clone(), p.orientation, true));
                }
            }
            for ep in &ps.extra_partners {
                list.push((ep.instance_id.clone(), ep.definition_id.clone(), ep.orientation, true));
            }
            if let Some(ref cs) = ps.r#case {
                list.push((cs.instance_id.clone(), cs.definition_id.clone(), cs.orientation, false));
            }
            list
        };

        for (actor_id, def_id, ori, in_partner) in actors {
            let Some(def) = self.catalog.get(&def_id).cloned() else {
                continue;
            };
            let card_name = self.card_name_by_def(&def_id);
            for clause in extract_clauses(&def.description, TriggerKind::OnHintUse, def.card_type) {
                if in_partner && def.card_type != CardType::PARTNER && !allows_partner_area(&clause) {
                    continue;
                }
                if !check_hint_use_match(&clause, hint_colors) {
                    continue;
                }
                let ctx = self.build_cond_info(player_id, Some(&actor_id), ori, None);
                if check_clause_conditions(&clause, &ctx).is_err() {
                    continue;
                }
                if !self.can_use_clause_this_turn(&actor_id, &clause) {
                    continue;
                }
                let cost = parse_clause_cost(&clause);
                if !self.pay_clause_cost(player_id, &actor_id, &cost) {
                    continue;
                }
                self.mark_clause_used_this_turn(&actor_id, &clause);
                self.log(format!("💡 [{}] の誘発効果（ネクストヒント使用時）が発動！", card_name));
                let plan = parse_effect_steps(&clause, Some(hint_card_level), def.card_type);
                if self.execute_clause_plan(player_id, &actor_id, &card_name, Some(def.level), &plan, "MAIN", None) {
                    return true;
                }
            }
        }
        false
    }

    /// キャラ登場時の常駐ウォッチャートリガー（`【疾風】`・「〜が登場したとき」）
    pub fn trigger_on_ally_play(
        &mut self,
        playing_pid: &str,
        played_inst_id: &str,
        played_def_id: &str,
        played_ori: CardOrientation,
        played_by_effect_level: Option<i32>,
        is_first_play_this_turn: bool,
        resume_context: &str,
    ) -> bool {
        let Some(mut played_def) = self.catalog.get(played_def_id).cloned() else {
            return false;
        };
        if let Some(inst) = self
            .players
            .get(playing_pid)
            .and_then(|ps| ps.field.iter().find(|c| c.instance_id == played_inst_id))
        {
            played_def.level += inst.temp_level_delta;
        }
        let played_ap = self
            .players
            .get(playing_pid)
            .and_then(|ps| ps.field.iter().find(|c| c.instance_id == played_inst_id))
            .map(|c| self.get_effective_ap(playing_pid, c))
            .unwrap_or(played_def.ap);

        let opp_pid = self.opponent_id_of(playing_pid);
        let watch_sides: Vec<(String, bool)> = vec![(playing_pid.to_string(), true), (opp_pid, false)];
        let mut opened_ui = false;

        for (watch_pid, owner_is_playing) in watch_sides {
            let watchers: Vec<(String, String, CardOrientation, bool)> = self
                .players
                .get(&watch_pid)
                .map(|ps| {
                    let mut list: Vec<(String, String, CardOrientation, bool)> = ps
                        .field
                        .iter()
                        .map(|c| (c.instance_id.clone(), c.definition_id.clone(), c.orientation, false))
                        .collect();
                    if let Some(ref p) = ps.partner {
                        if p.zone == Zone::PARTNER {
                            list.push((p.instance_id.clone(), p.definition_id.clone(), p.orientation, true));
                        }
                    }
                    for ep in &ps.extra_partners {
                        list.push((ep.instance_id.clone(), ep.definition_id.clone(), ep.orientation, true));
                    }
                    if let Some(ref cs) = ps.r#case {
                        list.push((cs.instance_id.clone(), cs.definition_id.clone(), cs.orientation, false));
                    }
                    list
                })
                .unwrap_or_default();

            for (watcher_id, def_id, ori, in_partner) in watchers {
                let Some(def) = self.catalog.get(&def_id).cloned() else {
                    continue;
                };
                let watcher_name = self.card_name_by_def(&def_id);
                for clause in extract_clauses(&def.description, TriggerKind::OnAllyPlay, def.card_type) {
                    if in_partner && def.card_type != CardType::PARTNER && !allows_partner_area(&clause) {
                        continue;
                    }
                    if !check_ally_play_match(
                        &clause,
                        owner_is_playing,
                        &watcher_id,
                        played_inst_id,
                        &played_def,
                        played_ap,
                        played_ori,
                        played_by_effect_level,
                        is_first_play_this_turn,
                    ) {
                        continue;
                    }
                    let ctx = self.build_cond_info(&watch_pid, Some(&watcher_id), ori, played_by_effect_level);
                    if check_clause_conditions(&clause, &ctx).is_err() {
                        continue;
                    }
                    if !self.can_use_clause_this_turn(&watcher_id, &clause) {
                        continue;
                    }
                    let cost = parse_clause_cost(&clause);
                    if !self.pay_clause_cost(&watch_pid, &watcher_id, &cost) {
                        continue;
                    }
                    self.mark_clause_used_this_turn(&watcher_id, &clause);
                    self.log(format!("✨ [{}] の誘発効果（キャラ登場時）が発動！", watcher_name));
                    let plan = parse_effect_steps(&clause, Some(played_def.level), def.card_type);
                    if opened_ui || self.pending_decision.is_some() {
                        self.pending_clause_queue.push(PendingClauseTask {
                            player_id: watch_pid.clone(),
                            actor_id: watcher_id.clone(),
                            actor_name: watcher_name.clone(),
                            source_level: Some(def.level),
                            card_type: def.card_type,
                            clause: String::new(),
                            resume_context: resume_context.to_string(),
                            evidence_source_inst_id: None,
                            post_target_ops: vec![],
                            plan: Some(plan),
                        });
                    } else if self.execute_clause_plan(
                        &watch_pid,
                        &watcher_id,
                        &watcher_name,
                        Some(def.level),
                        &plan,
                        resume_context,
                        None,
                    ) {
                        opened_ui = true;
                    }
                }
            }
        }
        opened_ui
    }

    pub(super) fn prepare_character_enter_field(
        &mut self,
        player_id: &str,
        actor_id: &str,
        definition_id: &str,
        orientation: CardOrientation,
        source_effect_level: Option<i32>,
    ) {
        self.field_overflow_excluded_ids.insert(actor_id.to_string());
        // MRルール: 自分の現場に新たなMRが登場する場合、すでに現場かパートナーエリアにいるMRをリムーブする
        if self.is_mr_card(definition_id) {
            let mut old_field_mrs: Vec<(String, String)> = Vec::new();
            let mut old_partner_mrs: Vec<String> = Vec::new();
            if let Some(ps) = self.players.get_mut(player_id) {
                for fc in &ps.field {
                    if fc.instance_id != actor_id && Self::is_mr_def(&self.catalog, &fc.definition_id) {
                        old_field_mrs.push((fc.instance_id.clone(), fc.definition_id.clone()));
                    }
                }
                let mut i = 0;
                while i < ps.extra_partners.len() {
                    if Self::is_mr_def(&self.catalog, &ps.extra_partners[i].definition_id) {
                        let mut ep = ps.extra_partners.remove(i);
                        Self::reset_turn_card_modifiers(&mut ep);
                        old_partner_mrs.push(ep.definition_id.clone());
                        ep.zone = Zone::REMOVE;
                        ep.face_up = true;
                        ps.remove.push(ep);
                    } else {
                        i += 1;
                    }
                }
            }
            for p_def in old_partner_mrs {
                let cname = self.card_name_by_def(&p_def);
                self.log(format!(
                    "🔄 新たなMR登場により、パートナーエリアの [{}] をリムーブしました",
                    cname
                ));
            }
            for (f_inst, _) in old_field_mrs {
                if let Some((rdef, rsets)) = self.move_field_char_to_remove(player_id, &f_inst, false, false) {
                    let cname = self.card_name_by_def(&rdef);
                    self.log(format!(
                        "🔄 新たなMR登場により、現場の [{}] をリムーブしました",
                        cname
                    ));
                    self.trigger_on_remove_char(player_id, &f_inst, &rdef, true);
                    for (sid, sdid) in rsets {
                        self.trigger_set_card_on_remove(player_id, &sid, &sdid);
                    }
                }
            }
        }

        self.self_entered_field_players_this_turn.insert(player_id.to_string());
        let entry = self
            .turn_field_plays_by_player
            .entry(player_id.to_string())
            .or_insert(0);
        *entry += 1;
        let is_first = *entry == 1;

        self.pending_ally_play.push((
            player_id.to_string(),
            actor_id.to_string(),
            definition_id.to_string(),
            orientation,
            source_effect_level,
            is_first,
        ));
    }

    pub fn resolve_character_enter_field_with_source(
        &mut self,
        player_id: &str,
        actor_id: &str,
        definition_id: &str,
        orientation: CardOrientation,
        source_effect_level: Option<i32>,
        resume_context: &str,
    ) -> bool {
        self.prepare_character_enter_field(
            player_id,
            actor_id,
            definition_id,
            orientation,
            source_effect_level,
        );

        if self.resolve_on_play_effects_with_source(
            player_id,
            actor_id,
            definition_id,
            false,
            source_effect_level,
            resume_context,
        ) {
            return true;
        }

        self.try_fire_pending_ally_play(resume_context)
    }

    pub(super) fn try_fire_pending_ally_play(&mut self, resume_context: &str) -> bool {
        // 複数のキャラが連続で登場した場合（五人囃子など）、登場順にすべての「キャラ登場時」誘発を処理する
        while !self.pending_decision.is_some() && !self.pending_ally_play.is_empty() {
            let (pid, inst_id, def_id, ori, src_lv, is_first) = self.pending_ally_play.remove(0);
            self.debug_log(format!("[DEBUG][ALLY_PLAY] fire trigger for entered {}({}) remaining_queue={}", def_id, inst_id, self.pending_ally_play.len()));
            if self.trigger_on_ally_play(&pid, &inst_id, &def_id, ori, src_lv, is_first, resume_context) {
                return true;
            }
        }
        false
    }

    pub(super) fn try_fire_pending_hint_trigger(&mut self, player_id: &str, after_hint_level: Option<i32>) -> bool {
        if self.pending_decision.is_some() {
            return false;
        }
        if let Some((pid, hlv, hcols)) = self.pending_hint_trigger.take() {
            return self.trigger_on_hint_use_with_colors(&pid, hlv, &hcols);
        }
        if let Some(hlv) = after_hint_level {
            return self.trigger_on_hint_use_with_colors(player_id, hlv, &[]);
        }
        false
    }

    pub(super) fn is_hirameki_blocked_for(&self, victim_id: &str) -> bool {
        if self.action_hirameki_blocked_player.as_deref() == Some(victim_id) {
            return true;
        }
        let opp_id = self.opponent_id_of(victim_id);
        if let Some(opp_ps) = self.players.get(&opp_id) {
            for c in opp_ps.field.iter().chain(opp_ps.partner.iter()).chain(opp_ps.extra_partners.iter()) {
                if let Some(def) = self.catalog.get(&c.definition_id) {
                    if def.description.contains("相手は【ヒラメキ】を発動できない") {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// 証拠をリムーブし、該当カードの【ヒラメキ】を発動する
    /// 戻り値: (opened_pending_ui, opponent_blocked_evidence_gain)
    pub(crate) fn remove_evidence_and_trigger_hirameki(
        &mut self,
        victim_id: &str,
        count: usize,
        resume_context: &str,
    ) -> (bool, bool) {
        let mut removed_evs: Vec<(String, String)> = vec![];
        if let Some(ps) = self.players.get_mut(victim_id) {
            for _ in 0..count {
                if let Some(mut ev) = ps.evidence.pop() {
                    let eid = ev.instance_id.clone();
                    let did = ev.definition_id.clone();
                    ev.zone = Zone::REMOVE;
                    ev.face_up = true;
                    ps.remove.push(ev);
                    removed_evs.push((eid, did));
                }
            }
        }

        if self.is_hirameki_blocked_for(victim_id) {
            if !removed_evs.is_empty() {
                self.log(format!("🚫 相手の能力により {} の【ヒラメキ】は発動しません", victim_id));
            }
            return (false, false);
        }

        let mut opened_ui = false;
        let mut block_opp_gain = false;
        let any_removed = !removed_evs.is_empty();
        for (eid, did) in removed_evs {
            let Some(def) = self.catalog.get(&did).cloned() else {
                continue;
            };
            let clauses = extract_clauses(&def.description, TriggerKind::Hirameki, def.card_type);
            if clauses.is_empty() {
                continue;
            }
            let card_name = self.card_name_by_def(&did);
            for clause in clauses {
                let ctx = self.build_cond_info(victim_id, Some(&eid), CardOrientation::ACTIVE, None);
                if check_clause_conditions(&clause, &ctx).is_err() {
                    continue;
                }
                if clause.contains("相手はこのアクションによって証拠を得られない") {
                    block_opp_gain = true;
                    self.log(format!("💡 [{}] の【ヒラメキ】発動！相手はこのアクションで証拠を得られません", card_name));
                    continue;
                }
                if clause.contains("以下から1つ選んで行う") {
                    let modes: Vec<String> = def
                        .description
                        .lines()
                        .map(|l| l.trim())
                        .skip_while(|l| *l != clause.trim())
                        .skip(1)
                        .take_while(|l| l.starts_with('・'))
                        .map(|l| l.trim_start_matches('・').trim().to_string())
                        .collect();
                    if !modes.is_empty() {
                        let dec_id = self.generate_decision_id();
                        self.pending_decision = Some(json!({
                            "decisionId": dec_id,
                            "revision": self.revision,
                            "playerId": victim_id,
                            "kind": "modeSelect",
                            "options": {
                                "actorId": eid,
                                "actorName": card_name,
                                "sourceLevel": def.level,
                                "cardType": format!("{:?}", def.card_type),
                                "modes": modes,
                                "resumeContext": resume_context,
                                "title": format!("[{}] の【ヒラメキ】効果を選択してください", card_name)
                            }
                        }));
                        opened_ui = true;
                        break;
                    }
                }
                let cost = parse_clause_cost(&clause);
                if !self.pay_clause_cost(victim_id, &eid, &cost) {
                    continue;
                }
                self.log(format!("💡 {} の証拠 [{}] の【ヒラメキ】が発動！", victim_id, card_name));
                let plan = parse_effect_steps(&clause, None, def.card_type);
                if self.execute_clause_plan(
                    victim_id,
                    &eid,
                    &card_name,
                    Some(def.level),
                    &plan,
                    resume_context,
                    Some(&eid),
                ) {
                    opened_ui = true;
                    break;
                }
            }
            if opened_ui {
                break;
            }
        }
        // RegisterNextOppEvidenceRemovedTrigger: 「次に相手の証拠がリムーブされたとき...」
        if any_removed {
            let mut remaining_trigs = Vec::new();
            let mut fired_trigs = Vec::new();
            for trig in self.next_opp_evidence_remove_triggers.drain(..) {
                if trig.0 != victim_id {
                    fired_trigs.push(trig);
                } else {
                    remaining_trigs.push(trig);
                }
            }
            self.next_opp_evidence_remove_triggers = remaining_trigs;
            for (opid, aid, aname, tclause) in fired_trigs {
                self.pending_clause_queue.insert(
                    0,
                    PendingClauseTask {
                        player_id: opid,
                        actor_id: aid,
                        actor_name: aname,
                        source_level: None,
                        card_type: CardType::EVENT,
                        clause: tclause,
                        resume_context: resume_context.to_string(),
                        evidence_source_inst_id: None,
                        post_target_ops: vec![],
                        plan: None,
                    },
                );
            }
            if !opened_ui && self.pending_decision.is_none() && self.drain_pending_clause_queue() {
                opened_ui = true;
            }
        }
        (opened_ui, block_opp_gain)
    }
}
