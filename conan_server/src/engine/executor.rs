use super::*;

impl MatchEngine {

    pub(super) fn drain_pending_clause_queue(&mut self) -> bool {
        while !self.pending_clause_queue.is_empty() {
            if self.phase == MatchPhase::FINISHED || self.pending_decision.is_some() {
                return true;
            }
            let task = self.pending_clause_queue.remove(0);
            if let Some(plan) = task.plan {
                if self.execute_clause_plan(
                    &task.player_id,
                    &task.actor_id,
                    &task.actor_name,
                    task.source_level,
                    &plan,
                    &task.resume_context,
                    task.evidence_source_inst_id.as_deref(),
                ) {
                    return true;
                }
                if !task.post_target_ops.is_empty()
                    && self.execute_immediate_ops_with_continuation(
                        &task.player_id,
                        &task.actor_id,
                        &task.actor_name,
                        &task.post_target_ops,
                        task.evidence_source_inst_id.as_deref(),
                        None,
                        None,
                        None,
                        &task.resume_context,
                        None,
                    )
                {
                    return true;
                }
                continue;
            }
            let ori = self.find_actor_orientation(&task.player_id, &task.actor_id);
            let ctx = self.build_cond_info(&task.player_id, Some(&task.actor_id), ori, task.source_level);
            if let Err(reason) = check_clause_conditions(&task.clause, &ctx) {
                self.log(format!("⚠️ [{}] の追加効果条件未達: {}", task.actor_name, reason));
                continue;
            }
            let resolved = resolve_state_branch(&task.clause, ori);
            let plan = parse_effect_steps(&resolved, None, task.card_type);
            if self.execute_clause_plan(
                &task.player_id,
                &task.actor_id,
                &task.actor_name,
                task.source_level,
                &plan,
                &task.resume_context,
                task.evidence_source_inst_id.as_deref(),
            ) {
                return true;
            }
        }
        while !self.pending_enter_field_queue.is_empty() {
            if self.phase == MatchPhase::FINISHED || self.pending_decision.is_some() {
                return true;
            }
            let (pid, dep_inst, dep_def, play_ori, src_lv, ctx) = self.pending_enter_field_queue.remove(0);
            if self.resolve_character_enter_field_with_source(
                &pid,
                &dep_inst,
                &dep_def,
                play_ori,
                src_lv,
                &ctx,
            ) {
                return true;
            }
        }
        false
    }

    pub(super) fn clause_usage_key(instance_id: &str, clause: &str) -> String {
        format!("{}::{}", instance_id, clause)
    }

    pub(super) fn can_use_clause_this_turn(&self, instance_id: &str, clause: &str) -> bool {
        let Some(limit) = clause_turn_limit(clause) else {
            return true;
        };
        let key = Self::clause_usage_key(instance_id, clause);
        let used = self.turn_clause_usage.get(&key).copied().unwrap_or(0);
        used < limit
    }

    pub(super) fn mark_clause_used_this_turn(&mut self, instance_id: &str, clause: &str) {
        if clause_turn_limit(clause).is_some() {
            let key = Self::clause_usage_key(instance_id, clause);
            *self.turn_clause_usage.entry(key).or_insert(0) += 1;
        }
    }

    pub(super) fn can_pay_clause_cost(&self, ps: &PlayerState, actor_id: &str, cost: &crate::effects::ClauseCost) -> bool {
        if cost.requires_sleep {
            let active = ps
                .field
                .iter()
                .find(|c| c.instance_id == actor_id)
                .or_else(|| ps.partner.as_ref().filter(|p| p.instance_id == actor_id))
                .map(|c| c.orientation == CardOrientation::ACTIVE)
                .unwrap_or(false);
            if !active {
                return false;
            }
        }
        if cost.hand_remove > 0 {
            if let Some(ref hf) = cost.hand_filter {
                let fc = ps.effective_file_count() as i32;
                let matched = ps
                    .hand
                    .iter()
                    .filter(|c| {
                        card_matches_filter_with_ctx(
                            &self.catalog,
                            None,
                            &ps.player_id,
                            &ps.player_id,
                            c,
                            self.catalog.get(&c.definition_id).map(|d| d.ap).unwrap_or(0),
                            hf,
                            None,
                            None,
                            Some(fc),
                        )
                    })
                    .count();
                if matched < cost.hand_remove {
                    return false;
                }
            } else if ps.hand.len() < cost.hand_remove {
                return false;
            }
        }
        if cost.evidence_remove > 0 && ps.evidence.len() < cost.evidence_remove {
            return false;
        }
        if cost.evidence_flip_up > 0 && ps.evidence.iter().filter(|e| !e.face_up).count() < cost.evidence_flip_up {
            return false;
        }
        if cost.file_remove > 0 && ps.file.len() < cost.file_remove {
            return false;
        }
        if cost.deck_mill > 0 && ps.deck.len() < cost.deck_mill {
            return false;
        }
        if cost.field_sleep > 0 {
            let fc = ps.effective_file_count() as i32;
            let matched = if let Some(ref fsf) = cost.field_sleep_filter {
                ps.field
                    .iter()
                    .filter(|c| {
                        card_matches_filter_with_ctx(
                            &self.catalog,
                            Some(actor_id),
                            &ps.player_id,
                            &ps.player_id,
                            c,
                            self.catalog.get(&c.definition_id).map(|d| d.ap).unwrap_or(0),
                            fsf,
                            None,
                            None,
                            Some(fc),
                        )
                    })
                    .count()
            } else {
                ps.field
                    .iter()
                    .filter(|c| c.orientation == CardOrientation::ACTIVE)
                    .count()
            };
            if matched < cost.field_sleep {
                return false;
            }
        }
        true
    }

    pub(super) fn pay_clause_cost(&mut self, player_id: &str, actor_id: &str, cost: &crate::effects::ClauseCost) -> bool {
        self.pay_clause_cost_internal(player_id, actor_id, cost, true)
    }

    pub(super) fn pay_clause_cost_internal(
        &mut self,
        player_id: &str,
        actor_id: &str,
        cost: &crate::effects::ClauseCost,
        pay_hand_auto: bool,
    ) -> bool {
        let can_pay = self
            .players
            .get(player_id)
            .map(|ps| self.can_pay_clause_cost(ps, actor_id, cost))
            .unwrap_or(false);
        if !can_pay {
            return false;
        }

        let mut removed_hand = vec![];
        let mut flipped = 0usize;
        let mut removed_file = vec![];
        let mut milled = vec![];

        let mut cost_milled_deck = false;
        if let Some(ps) = self.players.get_mut(player_id) {
            if cost.requires_sleep {
                if let Some(ref mut p) = ps.partner {
                    if p.instance_id == actor_id {
                        p.orientation = CardOrientation::SLEEP;
                    }
                }
                if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == actor_id) {
                    c.orientation = CardOrientation::SLEEP;
                }
            }
            if pay_hand_auto {
                for _ in 0..cost.hand_remove {
                    if let Some(mut c) = ps.hand.pop() {
                        c.zone = Zone::REMOVE;
                        removed_hand.push(c.definition_id.clone());
                        ps.remove.push(c);
                    }
                }
            }
            for ev in ps.evidence.iter_mut() {
                if flipped >= cost.evidence_flip_up {
                    break;
                }
                if !ev.face_up {
                    ev.face_up = true;
                    flipped += 1;
                }
            }
            for _ in 0..cost.file_remove {
                if let Some(mut f) = ps.file.pop() {
                    f.zone = Zone::REMOVE;
                    removed_file.push(f.definition_id.clone());
                    ps.remove.push(f);
                }
            }
            for _ in 0..cost.deck_mill {
                if let Some(mut d) = ps.deck.pop() {
                    d.zone = Zone::REMOVE;
                    d.face_up = true;
                    milled.push(d.definition_id.clone());
                    ps.remove.push(d);
                }
            }
            // コストで山札からリムーブした（残りが0枚か判定し、借用解除後に即リフレッシュ）
            if cost.deck_mill > 0 {
                cost_milled_deck = true;
            }
            if cost.sacrifice_self && !actor_id.is_empty() {
                if let Some(idx) = ps.field.iter().position(|c| c.instance_id == actor_id) {
                    let mut c = ps.field.remove(idx);
                    c.zone = Zone::REMOVE;
                    ps.remove.push(c);
                }
            }
            if cost.self_to_deck_bottom && !actor_id.is_empty() {
                if let Some(idx) = ps.field.iter().position(|c| c.instance_id == actor_id) {
                    let mut c = ps.field.remove(idx);
                    c.zone = Zone::DECK;
                    c.orientation = CardOrientation::ACTIVE;
                    c.face_up = false;
                    ps.deck.insert(0, c);
                }
            }
            if cost.self_to_partner_area && !actor_id.is_empty() {
                if let Some(idx) = ps.field.iter().position(|c| c.instance_id == actor_id) {
                    let mut c = ps.field.remove(idx);
                    c.zone = Zone::PARTNER;
                    ps.extra_partners.push(c);
                }
            }
        }

        // コストで山札からリムーブして0枚になった瞬間に即座へリフレッシュ（docs/rules.md §リフレッシュ）
        if cost_milled_deck {
            self.refresh_if_deck_emptied(player_id);
        }

        for _ in 0..cost.evidence_remove {
            let _ = self.remove_evidence_and_trigger_hirameki(player_id, 1, "MAIN");
        }
        for did in removed_hand {
            if let Some(d) = self.catalog.get(&did) {
                self.last_chosen_level = Some(d.level);
                self.last_removed_level_sum = d.level;
            }
            let n = self.card_name_by_def(&did);
            self.log(format!("💸 コスト支払い: {} の手札から [{}] をリムーブしました", player_id, n));
        }
        if flipped > 0 {
            self.log(format!("🔓 コスト支払い: {} は裏向きの証拠を{}つ表向きにしました", player_id, flipped));
        }
        for did in removed_file {
            let n = self.card_name_by_def(&did);
            self.log(format!("💸 コスト支払い: {} のFILEから [{}] をリムーブしました", player_id, n));
        }
        if !milled.is_empty() {
            let names: Vec<String> = milled.iter().map(|did| self.card_name_by_def(did)).collect();
            self.log(format!(
                "💸 コスト支払い: {} はデッキの上から {} 枚（[{}]）をリムーブしました",
                player_id,
                milled.len(),
                names.join("], [")
            ));
        }
        true
    }

    pub(super) fn execute_immediate_ops(
        &mut self,
        player_id: &str,
        actor_id: &str,
        actor_name: &str,
        ops: &[ImmediateOp],
        evidence_source_inst_id: Option<&str>,
    ) {
        let _ = self.execute_immediate_ops_with_continuation(
            player_id,
            actor_id,
            actor_name,
            ops,
            evidence_source_inst_id,
            None,
            None,
            None,
            "MAIN",
            None,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn execute_immediate_ops_with_continuation(
        &mut self,
        player_id: &str,
        actor_id: &str,
        actor_name: &str,
        ops: &[ImmediateOp],
        evidence_source_inst_id: Option<&str>,
        follow_up_target: Option<&TargetStep>,
        follow_up_deck_search: Option<&DeckSearchStep>,
        pending_clause_plan: Option<&ParsedClausePlan>,
        resume_context: &str,
        after_hint_level: Option<i32>,
    ) -> bool {
        for (idx, op) in ops.iter().enumerate() {
            match op {
                ImmediateOp::Draw(n) => {
                    self.draw_cards(player_id, *n);
                    self.log(format!("✨ [{}] の効果でカードを {} 枚引きました", actor_name, n));
                }
                ImmediateOp::DiscardHandIfHandAtLeast { discard_count, min_hand } => {
                    let hand_len = self.players.get(player_id).map(|ps| ps.hand.len()).unwrap_or(0);
                    if hand_len >= *min_hand {
                        let remaining = &ops[idx + 1..];
                        if self.prompt_hand_discard(
                            player_id,
                            actor_id,
                            actor_name,
                            None,
                            *discard_count,
                            None,
                            false,
                            false,
                            remaining,
                            follow_up_target,
                            follow_up_deck_search,
                            pending_clause_plan,
                            resume_context,
                            after_hint_level,
                        ) {
                            return true;
                        }
                    }
                }
                ImmediateOp::DiscardHand(n) => {
                    let remaining = &ops[idx + 1..];
                    if self.prompt_hand_discard(
                        player_id,
                        actor_id,
                        actor_name,
                        None,
                        *n,
                        None,
                        false,
                        false,
                        remaining,
                        follow_up_target,
                        follow_up_deck_search,
                        pending_clause_plan,
                        resume_context,
                        after_hint_level,
                    ) {
                        return true;
                    }
                }
                ImmediateOp::DiscardFilteredHand { count, filter } => {
                    let remaining = &ops[idx + 1..];
                    if self.prompt_hand_discard(
                        player_id,
                        actor_id,
                        actor_name,
                        None,
                        *count,
                        Some(filter),
                        false,
                        false,
                        remaining,
                        follow_up_target,
                        follow_up_deck_search,
                        pending_clause_plan,
                        resume_context,
                        after_hint_level,
                    ) {
                        return true;
                    }
                }
                ImmediateOp::OptionalDiscardHand { count, filter } => {
                    let remaining = &ops[idx + 1..];
                    if self.prompt_hand_discard(
                        player_id,
                        actor_id,
                        actor_name,
                        None,
                        *count,
                        filter.as_ref(),
                        true,
                        true,
                        remaining,
                        follow_up_target,
                        follow_up_deck_search,
                        pending_clause_plan,
                        resume_context,
                        after_hint_level,
                    ) {
                        return true;
                    }
                    // 任意手札リムーブ用の候補が0枚だった場合、後続効果は発動しない
                    return false;
                }
                ImmediateOp::GainEvidence(n) => {
                    self.gain_evidence(player_id, *n);
                    self.log(format!("✨ [{}] の効果で証拠を {} つ獲得しました", actor_name, n));
                }
                ImmediateOp::FlipEvidenceUp(n) => {
                    let mut count = 0;
                    if let Some(ps) = self.players.get_mut(player_id) {
                        for ev in ps.evidence.iter_mut() {
                            if count >= *n {
                                break;
                            }
                            if !ev.face_up {
                                ev.face_up = true;
                                count += 1;
                            }
                        }
                    }
                    if count > 0 {
                        self.log(format!("🔓 [{}] の効果で裏向きの証拠を {} つ表向きにしました", actor_name, count));
                    }
                }
                ImmediateOp::FlipEvidenceDown(n) => {
                    let mut count = 0;
                    if let Some(ps) = self.players.get_mut(player_id) {
                        for ev in ps.evidence.iter_mut() {
                            if count >= *n {
                                break;
                            }
                            if ev.face_up {
                                ev.face_up = false;
                                count += 1;
                            }
                        }
                    }
                    if count > 0 {
                        self.log(format!("🔒 [{}] の効果で表向きの証拠を {} つ裏向きにしました", actor_name, count));
                    }
                }
                ImmediateOp::OppRemoveEvidence(n) => {
                    let opp_id = self.opponent_id_of(player_id);
                    for _ in 0..*n {
                        let (opened_ui, _) = self.remove_evidence_and_trigger_hirameki(&opp_id, 1, resume_context);
                        if opened_ui {
                            return true;
                        }
                    }
                }
                ImmediateOp::SelfRemoveEvidence { count, trigger_hirameki } => {
                    for _ in 0..*count {
                        if *trigger_hirameki {
                            let (opened_ui, _) = self.remove_evidence_and_trigger_hirameki(player_id, 1, resume_context);
                            if opened_ui {
                                return true;
                            }
                        } else if let Some(ps) = self.players.get_mut(player_id) {
                            if let Some(mut ev) = ps.evidence.pop() {
                                ev.zone = Zone::REMOVE;
                                ps.remove.push(ev);
                                self.log(format!("⚠️ [{}] の効果: 証拠を1つリムーブしました", actor_name));
                            }
                        }
                    }
                }
                ImmediateOp::SelfApBuff(amt) => {
                    let contact_cid = self.get_contact_char_id_for_player(player_id);
                    if let Some(ps) = self.players.get_mut(player_id) {
                        let target_pos = ps
                            .field
                            .iter()
                            .position(|c| c.instance_id == actor_id)
                            .or_else(|| {
                                contact_cid
                                    .as_deref()
                                    .and_then(|cid| ps.field.iter().position(|c| c.instance_id == cid))
                            });
                        if let Some(pos) = target_pos {
                            let actor_on_field = ps.field[pos].instance_id == actor_id;
                            if actor_on_field {
                                ps.field[pos].temp_ap_bonus += *amt;
                            } else {
                                // 手札から使用したカットイン等：コンタクト終了時まで有効
                                let inst = ps.field[pos].instance_id.clone();
                                *self.contact_ap_buffs.entry(inst).or_insert(0) += *amt;
                            }
                            let target_name = self
                                .catalog
                                .get(&ps.field[pos].definition_id)
                                .and_then(|d| d.names.first().cloned())
                                .unwrap_or_else(|| actor_name.to_string());
                            self.log(format!("✨ [{}] の効果で [{}] のAP +{}", actor_name, target_name, amt));
                        }
                    }
                }
                ImmediateOp::SelfLpBuff(amt) => {
                    if let Some(ps) = self.players.get_mut(player_id) {
                        if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == actor_id) {
                            c.temp_lp_bonus += *amt;
                            self.log(format!("✨ [{}] の効果で自身のLP +{}", actor_name, amt));
                        }
                    }
                }
                ImmediateOp::SetSelfFieldBaseLp {
                    from_lp,
                    level,
                    trait_name,
                    to_lp,
                } => {
                    let mut updated_names: Vec<String> = Vec::new();
                    if let Some(ps) = self.players.get_mut(player_id) {
                        for c in ps.field.iter_mut() {
                            if let Some(def) = self.catalog.get(&c.definition_id) {
                                if def.lp == *from_lp
                                    && def.level == *level
                                    && def.traits.iter().any(|t| t == trait_name)
                                {
                                    c.temp_lp_bonus += *to_lp - def.lp;
                                    updated_names.push(
                                        def.names.first().cloned().unwrap_or_else(|| def.definition_id.clone()),
                                    );
                                }
                            }
                        }
                    }
                    for target_name in updated_names {
                        self.log(format!(
                            "✨ [{}] の効果で自分の現場の [{}] のLPを {} にしました",
                            actor_name, target_name, to_lp
                        ));
                    }
                }
                ImmediateOp::SelfLevelDelta(amt) => {
                    if let Some(ps) = self.players.get_mut(player_id) {
                        if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == actor_id) {
                            c.temp_level_delta += *amt;
                            let sign = if *amt >= 0 { "+" } else { "" };
                            self.log(format!("📉 [{}] の効果で [{}] のレベルを {}{} しました", actor_name, actor_name, sign, amt));
                        }
                    }
                }
                ImmediateOp::SetDeckTopFaceDownOnSelf(n) => {
                    let mut added: Vec<(String, bool)> = Vec::new();
                    if let Some(ps) = self.players.get_mut(player_id) {
                        let mut popped = Vec::new();
                        for _ in 0..*n {
                            if let Some(mut c) = ps.deck.pop() {
                                c.zone = Zone::FIELD;
                                c.face_up = false;
                                added.push((c.definition_id.clone(), false));
                                popped.push(c);
                            }
                        }
                        if !popped.is_empty() {
                            if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == actor_id) {
                                c.set_cards.extend(popped);
                            } else {
                                added.clear();
                                for mut c in popped.into_iter().rev() {
                                    c.zone = Zone::DECK;
                                    ps.deck.push(c);
                                }
                            }
                        }
                    }
                    if !added.is_empty() {
                        self.refresh_if_deck_emptied(player_id);
                        self.log(format!("🔧 [{}] の効果でデッキの上から {}枚を裏向きで自身にセットしました", actor_name, added.len()));
                        self.trigger_on_card_set(player_id, actor_id, &added);
                    }
                }
                ImmediateOp::SelfActivate => {
                    let mut was_stun = false;
                    let mut changed = false;
                    if let Some(ps) = self.players.get_mut(player_id) {
                        if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == actor_id) {
                            changed = true;
                            if c.orientation == CardOrientation::STUN {
                                c.orientation = CardOrientation::SLEEP;
                                was_stun = true;
                            } else {
                                c.orientation = CardOrientation::ACTIVE;
                            }
                        }
                    }
                    if changed {
                        if was_stun {
                            self.log(format!("✨ スタン状態の [{}] をスリープにしました", actor_name));
                        } else {
                            self.log(format!("✨ [{}] をアクティブにしました", actor_name));
                        }
                    }
                }
                ImmediateOp::SelfGiveRush(r_target) => {
                    if let Some(ps) = self.players.get_mut(player_id) {
                        if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == actor_id) {
                            c.has_rush = true;
                            c.rush_target = r_target.clone();
                            let label = match r_target.as_deref() {
                                Some("CHAR") => "〚突撃［キャラ］〛",
                                Some("CASE") => "〚突撃［事件］〛",
                                _ => "〚突撃〛",
                            };
                            self.log(format!("⚡ [{}] に{}を付与しました", actor_name, label));
                        }
                    }
                }
                ImmediateOp::MillDeck(n) => {
                    if let Some(ps) = self.players.get_mut(player_id) {
                        for _ in 0..*n {
                            if let Some(mut d) = ps.deck.pop() {
                                d.zone = Zone::REMOVE;
                                d.face_up = true;
                                ps.remove.push(d);
                            }
                        }
                    }
                    self.log(format!("🗑️ [{}] の効果で山札から {} 枚をリムーブしました", actor_name, n));
                    // 強制リムーブで山札が0枚になった瞬間に即座へリフレッシュ（docs/rules.md §リフレッシュ）
                    self.refresh_if_deck_emptied(player_id);
                }
                ImmediateOp::MillDeckByLastChosenLevel => {
                    let n = self.last_chosen_level.unwrap_or(0).max(0) as usize;
                    if let Some(ps) = self.players.get_mut(player_id) {
                        for _ in 0..n {
                            if let Some(mut d) = ps.deck.pop() {
                                d.zone = Zone::REMOVE;
                                d.face_up = true;
                                ps.remove.push(d);
                            }
                        }
                    }
                    self.log(format!("🗑️ [{}] の効果で選択カードと同じレベルの {} 枚を山札からリムーブしました", actor_name, n));
                    self.refresh_if_deck_emptied(player_id);
                }
                ImmediateOp::OppMillDeck(n) => {
                    let opp_id = self.opponent_id_of(player_id);
                    let mut milled = 0;
                    if let Some(ps) = self.players.get_mut(&opp_id) {
                        for _ in 0..*n {
                            if let Some(mut d) = ps.deck.pop() {
                                d.zone = Zone::REMOVE;
                                d.face_up = true;
                                ps.remove.push(d);
                                milled += 1;
                            }
                        }
                    }
                    self.log(format!("🗑️ [{}] の効果で {} の山札から {} 枚をリムーブしました", actor_name, opp_id, milled));
                    self.refresh_if_deck_emptied(&opp_id);
                }
                ImmediateOp::OppDiscardHand(n) => {
                    let opp_id = self.opponent_id_of(player_id);
                    if self.prompt_hand_discard(
                        &opp_id,
                        actor_id,
                        actor_name,
                        None,
                        *n,
                        None,
                        false,
                        false,
                        &[],
                        None,
                        None,
                        None,
                        resume_context,
                        None,
                    ) {
                        return true;
                    }
                }
                ImmediateOp::OppFlipEvidenceUp(n) => {
                    let opp_id = self.opponent_id_of(player_id);
                    let mut count = 0;
                    if let Some(ps) = self.players.get_mut(&opp_id) {
                        for ev in ps.evidence.iter_mut() {
                            if count >= *n {
                                break;
                            }
                            if !ev.face_up {
                                ev.face_up = true;
                                count += 1;
                            }
                        }
                    }
                    if count > 0 {
                        self.log(format!("🔓 [{}] の効果で {} の裏向きの証拠を {} つ表向きにしました", actor_name, opp_id, count));
                    }
                }
                ImmediateOp::ReviveSelfFromRemove { sleep } => {
                    let turn = self.turn;
                    let mut revived = false;
                    if let Some(ps) = self.players.get_mut(player_id) {
                        if let Some(pos) = ps.remove.iter().position(|c| c.instance_id == actor_id) {
                            let mut card = ps.remove.remove(pos);
                            card.zone = Zone::FIELD;
                            card.orientation = if *sleep { CardOrientation::SLEEP } else { CardOrientation::ACTIVE };
                            card.entered_turn = turn;
                            card.face_up = true;
                            self.field_overflow_excluded_ids.insert(card.instance_id.clone());
                            ps.field.push(card);
                            revived = true;
                        }
                    }
                    if revived {
                        self.last_deployed_count = 1;
                        let state_str = if *sleep { "スリープ状態で" } else { "" };
                        self.log(format!("⚡ [{}] が自身の効果でリムーブエリアから{}現場に登場！", actor_name, state_str));
                    }
                }
                ImmediateOp::PlaySelfFromHand { sleep } => {
                    let turn = self.turn;
                    let mut played = false;
                    if let Some(ps) = self.players.get_mut(player_id) {
                        if let Some(pos) = ps.hand.iter().position(|c| c.instance_id == actor_id) {
                            let mut card = ps.hand.remove(pos);
                            card.zone = Zone::FIELD;
                            card.orientation = if *sleep { CardOrientation::SLEEP } else { CardOrientation::ACTIVE };
                            card.entered_turn = turn;
                            card.face_up = true;
                            self.field_overflow_excluded_ids.insert(card.instance_id.clone());
                            ps.field.push(card);
                            played = true;
                        }
                    }
                    if played {
                        self.last_deployed_count = 1;
                        let state_str = if *sleep { "スリープ状態で" } else { "" };
                        self.log(format!("⚡ [{}] が自身の効果で手札から{}現場に登場！", actor_name, state_str));
                    }
                }
                ImmediateOp::MoveSelfFromEvidenceToFieldSleep => {
                    if let Some(eid) = evidence_source_inst_id {
                        let turn = self.turn;
                        if let Some(ps) = self.players.get_mut(player_id) {
                            if let Some(pos) = ps.remove.iter().position(|c| c.instance_id == eid) {
                                let mut card = ps.remove.remove(pos);
                                card.zone = Zone::FIELD;
                                card.orientation = CardOrientation::SLEEP;
                                card.entered_turn = turn;
                                card.face_up = true;
                                self.field_overflow_excluded_ids.insert(card.instance_id.clone());
                                ps.field.push(card);
                                self.log(format!("💡 【ヒラメキ】[{}] が証拠から現場へスリープ状態で登場！", actor_name));
                            }
                        }
                    }
                }
                ImmediateOp::MoveSelfFromEvidenceToHand => {
                    if let Some(eid) = evidence_source_inst_id {
                        if let Some(ps) = self.players.get_mut(player_id) {
                            if let Some(pos) = ps.remove.iter().position(|c| c.instance_id == eid) {
                                let mut card = ps.remove.remove(pos);
                                card.zone = Zone::HAND;
                                card.face_up = true;
                                ps.hand.push(card);
                                self.log(format!("💡 【ヒラメキ】[{}] が証拠から手札に加わりました！", actor_name));
                            }
                        }
                    }
                }
                ImmediateOp::MoveSelfFromEvidenceToPartner => {
                    if let Some(eid) = evidence_source_inst_id {
                        if let Some(ps) = self.players.get_mut(player_id) {
                            if let Some(pos) = ps.remove.iter().position(|c| c.instance_id == eid) {
                                let mut card = ps.remove.remove(pos);
                                card.zone = Zone::PARTNER;
                                card.orientation = CardOrientation::ACTIVE;
                                card.face_up = true;
                                if ps.partner.is_none() {
                                    ps.partner = Some(card);
                                } else {
                                    ps.extra_partners.push(card);
                                }
                                self.log(format!("💡 【ヒラメキ】[{}] が証拠からパートナーエリアに移りました！", actor_name));
                            }
                        }
                    }
                }
                ImmediateOp::BlockOppDisguiseThisTurn => {
                    let opp_id = self.opponent_id_of(player_id);
                    self.turn_disguise_blocked_players.insert(opp_id);
                    self.log(format!("🚫 [{}] の【ヒラメキ】発動！このターン中、相手は【変装】を使用できません", actor_name));
                }
                ImmediateOp::TriggerFieldHirameki(target_name) => {
                    let found = self.players.get(player_id).and_then(|ps| {
                        ps.field.iter().find_map(|c| {
                            let def = self.catalog.get(&c.definition_id)?;
                            if def.names.iter().any(|n| n.contains(target_name.as_str())) {
                                let clauses = extract_clauses(&def.description, TriggerKind::Hirameki, def.card_type);
                                if !clauses.is_empty() {
                                    return Some((
                                        c.instance_id.clone(),
                                        self.card_name_by_def(&c.definition_id),
                                        def.level,
                                        def.card_type,
                                        clauses,
                                    ));
                                }
                            }
                            None
                        })
                    });
                    if let Some((cid, cname, clevel, ctype, clauses)) = found {
                        for clause in clauses {
                            self.log(format!("💡 [{}] の効果で [{}] の【ヒラメキ】を発動！", actor_name, cname));
                            let plan = parse_effect_steps(&clause, None, ctype);
                            if self.execute_clause_plan(player_id, &cid, &cname, Some(clevel), &plan, resume_context, Some(&cid)) {
                                return true;
                            }
                        }
                    }
                }
                ImmediateOp::TriggerEvidenceHiramekiByTrait(target_trait) => {
                    let found = self.players.get(player_id).and_then(|ps| {
                        ps.evidence.iter().rev().find_map(|ev| {
                            if !ev.face_up {
                                return None;
                            }
                            let def = self.catalog.get(&ev.definition_id)?;
                            if def.traits.iter().any(|t| t.contains(target_trait.as_str())) {
                                let clauses = extract_clauses(&def.description, TriggerKind::Hirameki, def.card_type);
                                if !clauses.is_empty() {
                                    return Some((
                                        ev.instance_id.clone(),
                                        self.card_name_by_def(&ev.definition_id),
                                        def.level,
                                        def.card_type,
                                        clauses,
                                    ));
                                }
                            }
                            None
                        })
                    });
                    if let Some((eid, ename, elevel, etype, clauses)) = found {
                        for clause in clauses {
                            self.log(format!("💡 [{}] の効果で表向きの証拠 [{}] の【ヒラメキ】を発動！", actor_name, ename));
                            let plan = parse_effect_steps(&clause, None, etype);
                            if self.execute_clause_plan(player_id, &eid, &ename, Some(elevel), &plan, resume_context, Some(&eid)) {
                                return true;
                            }
                        }
                    }
                }
                ImmediateOp::SolveCaseVictory => {
                    self.log(format!("🎉 [{}] の効果により {} が事件を解決し勝利しました！", actor_name, player_id));
                    self.finish_match(player_id, "CASE_RESOLVED");
                }
                ImmediateOp::OppDiscardRandom(n) => {
                    let opp_id = self.opponent_id_of(player_id);
                    let mut discarded = vec![];
                    if let Some(ops_ps) = self.players.get_mut(&opp_id) {
                        let mut rng = rand::thread_rng();
                        for _ in 0..*n {
                            if ops_ps.hand.is_empty() {
                                break;
                            }
                            let ridx = rng.gen_range(0..ops_ps.hand.len());
                            let mut c = ops_ps.hand.remove(ridx);
                            c.zone = Zone::REMOVE;
                            c.face_up = true;
                            discarded.push(c.definition_id.clone());
                            ops_ps.remove.push(c);
                        }
                    }
                    for did in discarded {
                        let cname = self.card_name_by_def(&did);
                        self.log(format!("🎲 [{}] の効果で {} の手札からランダムに [{}] をリムーブしました", actor_name, opp_id, cname));
                    }
                }
                ImmediateOp::RevealOpponentHand => {
                    let opp_id = self.opponent_id_of(player_id);
                    let mut candidates = Vec::new();
                    let mut names = Vec::new();
                    if let Some(ops_ps) = self.players.get(&opp_id) {
                        for c in &ops_ps.hand {
                            let def_opt = self.catalog.get(&c.definition_id);
                            let cname = self.card_name_by_def(&c.definition_id);
                            names.push(cname.clone());
                            candidates.push(json!({
                                "instanceId": c.instance_id,
                                "definitionId": c.definition_id,
                                "name": cname,
                                "level": def_opt.map(|d| d.level).unwrap_or(0),
                                "ap": def_opt.map(|d| d.ap).unwrap_or(0),
                                "colors": def_opt.map(|d| d.colors.clone()).unwrap_or_default(),
                                "traits": def_opt.map(|d| d.traits.clone()).unwrap_or_default(),
                                "orientation": format!("{:?}", c.orientation),
                                "faceUp": true,
                                "isOpponent": true,
                                "zoneLabel": "相手手札",
                            }));
                        }
                    }
                    if candidates.is_empty() {
                        self.log(format!("👁️ [{}] の効果で {} の手札を公開しましたが、手札は0枚でした", actor_name, opp_id));
                    } else {
                        self.log(format!("👁️ [{}] の効果で {} の手札が公開されました: [{}]", actor_name, opp_id, names.join("], [")));
                        let remaining = &ops[idx + 1..];
                        let dec_id = self.generate_decision_id();
                        self.pending_decision = Some(json!({
                            "decisionId": dec_id,
                            "revision": self.revision,
                            "playerId": player_id,
                            "kind": "targetSelect",
                            "options": {
                                "actorId": actor_id,
                                "actorName": actor_name,
                                "sourceLevel": None::<i32>,
                                "effectType": "VIEW_OPPONENT_HAND",
                                "targetZone": "HAND",
                                "maxSelect": 0,
                                "optional": true,
                                "title": "相手の手札が公開されました（確認後、決定/スキップで次へ進みます）",
                                "candidates": candidates,
                                "followUpOps": remaining,
                                "followUpTarget": follow_up_target,
                                "followUpDeckSearch": follow_up_deck_search,
                                "pendingClausePlan": pending_clause_plan,
                                "afterHintLevel": after_hint_level,
                                "resumeContext": resume_context,
                            }
                        }));
                        return true;
                    }
                }
                ImmediateOp::BlockHintThisTurn => {
                    self.turn_hint_blocked_players.insert(player_id.to_string());
                    self.log(format!("🚫 [{}] の効果: このターン中、{} は【ネクストヒント】を使用できません", actor_name, player_id));
                }
                ImmediateOp::BlockOppCutinThisTurn => {
                    let opp_id = self.opponent_id_of(player_id);
                    self.turn_cutin_blocked_players.insert(opp_id.clone());
                    self.log(format!("🚫 [{}] の効果: このターン中、{} は【カットイン】を使用できません", actor_name, opp_id));
                }
                ImmediateOp::BlockSelfEventsThisTurn => {
                    self.turn_event_blocked_players.insert(player_id.to_string());
                    self.log(format!("🚫 [{}] の効果: このターン中、{} はイベントを使用できません", actor_name, player_id));
                }
                ImmediateOp::BanNameThisTurn(ref ban_name) => {
                    self.turn_name_bans.entry(player_id.to_string()).or_default().push(ban_name.clone());
                    self.log(format!(
                        "🚫 [{}] の効果: このターン中、自分は〚カード名［{}］〛を使用できず、登場させることができません",
                        actor_name, ban_name
                    ));
                }
                ImmediateOp::DiscardHandTo(target_cnt) => {
                    let cur_len = self.players.get(player_id).map(|p| p.hand.len()).unwrap_or(0);
                    if cur_len > *target_cnt {
                        let need_discard = cur_len - *target_cnt;
                        let remaining = &ops[idx + 1..];
                        if self.prompt_hand_discard(
                            player_id,
                            actor_id,
                            actor_name,
                            None,
                            need_discard,
                            None,
                            false,
                            false,
                            remaining,
                            follow_up_target,
                            follow_up_deck_search,
                            pending_clause_plan,
                            resume_context,
                            after_hint_level,
                        ) {
                            return true;
                        }
                    }
                }
                ImmediateOp::DiscardAllHand => {
                    let mut defs = vec![];
                    let mut lv_sum = 0i32;
                    if let Some(ps) = self.players.get_mut(player_id) {
                        while let Some(mut c) = ps.hand.pop() {
                            c.zone = Zone::REMOVE;
                            c.face_up = true;
                            if let Some(d) = self.catalog.get(&c.definition_id) {
                                lv_sum += d.level;
                            }
                            defs.push(c.definition_id.clone());
                            ps.remove.push(c);
                        }
                    }
                    self.last_removed_level_sum = lv_sum;
                    if !defs.is_empty() {
                        self.log(format!("🗑️ [{}] の効果で {} の手札 {} 枚をすべてリムーブしました", actor_name, player_id, defs.len()));
                    }
                }
                ImmediateOp::FileTopToHand(n) => {
                    let mut moved = 0usize;
                    if let Some(ps) = self.players.get_mut(player_id) {
                        for _ in 0..*n {
                            if let Some(mut fc) = ps.file.pop() {
                                fc.zone = Zone::HAND;
                                fc.face_up = true;
                                ps.hand.push(fc);
                                moved += 1;
                            }
                        }
                    }
                    if moved > 0 {
                        self.log(format!("📥 [{}] の効果でFILEから {} 枚を手札に加えました", actor_name, moved));
                    }
                }
                ImmediateOp::OptionalFileTopToHand { count, then_clause } => {
                    let has_file = self.players.get(player_id).map(|p| p.file.len() >= *count).unwrap_or(false);
                    if has_file {
                        self.enqueue_continuation_from_ops(player_id, actor_id, actor_name, &ops[idx + 1..], evidence_source_inst_id, follow_up_target, follow_up_deck_search, pending_clause_plan, resume_context);
                        let remaining = &ops[idx + 1..];
                        let dec_id = self.generate_decision_id();
                        self.pending_decision = Some(json!({
                            "decisionId": dec_id,
                            "revision": self.revision,
                            "playerId": player_id,
                            "kind": "modeSelect",
                            "options": {
                                "specialKind": "OPTIONAL_FILE_TO_HAND",
                                "actorId": actor_id,
                                "actorName": actor_name,
                                "count": count,
                                "thenClause": then_clause,
                                "minPick": 1,
                                "maxPick": 1,
                                "modes": [
                                    format!("FILEの上から{}枚を手札に加える", count),
                                    "行わない"
                                ],
                                "resumeContext": resume_context,
                                "title": format!("[{}] FILEの上から{}枚を手札に加えますか？", actor_name, count)
                            }
                        }));
                        return true;
                    }
                }
                ImmediateOp::FileTopRemove(n) => {
                    let mut moved = 0usize;
                    if let Some(ps) = self.players.get_mut(player_id) {
                        for _ in 0..*n {
                            if let Some(mut fc) = ps.file.pop() {
                                fc.zone = Zone::REMOVE;
                                fc.face_up = true;
                                ps.remove.push(fc);
                                moved += 1;
                            }
                        }
                    }
                    if moved > 0 {
                        self.log(format!("🗑️ [{}] の効果でFILEの上から {} 枚をリムーブしました", actor_name, moved));
                    }
                }
                ImmediateOp::DeckTopRemovePerFieldChar(per_cnt) => {
                    let field_cnt = self.players.get(player_id).map(|p| p.field.len()).unwrap_or(0);
                    let mill_total = field_cnt * (*per_cnt);
                    if mill_total > 0 {
                        if let Some(ps) = self.players.get_mut(player_id) {
                            for _ in 0..mill_total {
                                if let Some(mut d) = ps.deck.pop() {
                                    d.zone = Zone::REMOVE;
                                    d.face_up = true;
                                    ps.remove.push(d);
                                }
                            }
                        }
                        self.log(format!("🗑️ [{}] の効果で現場キャラ {} 体につき山札から合計 {} 枚リムーブしました", actor_name, field_cnt, mill_total));
                        // 現場キャラ数に応じた強制リムーブで0枚になった瞬間に即座へリフレッシュ
                        self.refresh_if_deck_emptied(player_id);
                    }
                }
                ImmediateOp::OppTopEvidenceRemove(n) => {
                    let opp_id = self.opponent_id_of(player_id);
                    for _ in 0..*n {
                        let (opened_ui, _) = self.remove_evidence_and_trigger_hirameki(&opp_id, 1, resume_context);
                        if opened_ui {
                            self.enqueue_continuation_from_ops(player_id, actor_id, actor_name, &ops[idx + 1..], evidence_source_inst_id, follow_up_target, follow_up_deck_search, pending_clause_plan, resume_context);
                            return true;
                        }
                    }
                }
                ImmediateOp::GiveOppEvidence(n) => {
                    let opp_id = self.opponent_id_of(player_id);
                    self.gain_evidence(&opp_id, *n);
                    self.log(format!("📜 [{}] の効果で {} に証拠を {} つ与えました", actor_name, opp_id, n));
                }
                ImmediateOp::OptionalGiveOppEvidence(n) => {
                    self.enqueue_continuation_from_ops(player_id, actor_id, actor_name, &ops[idx + 1..], evidence_source_inst_id, follow_up_target, follow_up_deck_search, pending_clause_plan, resume_context);
                    let dec_id = self.generate_decision_id();
                    self.pending_decision = Some(json!({
                        "decisionId": dec_id,
                        "revision": self.revision,
                        "playerId": player_id,
                        "kind": "modeSelect",
                        "options": {
                            "specialKind": "OPTIONAL_GIVE_OPP_EVIDENCE",
                            "actorId": actor_id,
                            "actorName": actor_name,
                            "count": n,
                            "minPick": 1,
                            "maxPick": 1,
                            "modes": [
                                format!("相手に証拠を{}つ与える", n),
                                "与えない"
                            ],
                            "resumeContext": resume_context,
                            "title": format!("[{}] 相手に証拠を{}つ与えますか？", actor_name, n)
                        }
                    }));
                    return true;
                }
                ImmediateOp::GainThisAsFaceUpEvidence => {
                    if let Some(ps) = self.players.get_mut(player_id) {
                        if let Some(pos) = ps.remove.iter().position(|c| c.instance_id == actor_id) {
                            let mut card = ps.remove.remove(pos);
                            card.zone = Zone::EVIDENCE;
                            card.face_up = true;
                            ps.evidence.push(card);
                            self.log(format!("✨ [{}] を表向きの証拠として得ました！", actor_name));
                        }
                    }
                }
                ImmediateOp::MoveThisToPartner => {
                    if let Some(ps) = self.players.get_mut(player_id) {
                        if let Some(pos) = ps.remove.iter().position(|c| c.instance_id == actor_id) {
                            let mut card = ps.remove.remove(pos);
                            card.zone = Zone::PARTNER;
                            card.face_up = true;
                            card.orientation = CardOrientation::ACTIVE;
                            ps.extra_partners.push(card);
                            self.log(format!("🤝 [{}] をパートナーエリアに移しました！", actor_name));
                        }
                    }
                }
                ImmediateOp::MassOppSleepAll => {
                    let opp_id = self.opponent_id_of(player_id);
                    let mut cnt = 0;
                    if let Some(opp_ps) = self.players.get_mut(&opp_id) {
                        for c in &mut opp_ps.field {
                            let protected = c.set_cards.iter().any(|sc| {
                                sc.face_up && self.catalog.get(&sc.definition_id).map(|d| d.description.contains("リムーブされず、スリープされず、スタンされない")).unwrap_or(false)
                            });
                            if !protected {
                                c.orientation = CardOrientation::SLEEP;
                                cnt += 1;
                            }
                        }
                    }
                    self.log(format!("💤 [{}] の効果で相手の現場キャラ {} 体をスリープさせました", actor_name, cnt));
                }
                ImmediateOp::MassAllSleepToStun => {
                    let mut cnt = 0;
                    for pid in self.player_order.clone() {
                        if let Some(ps) = self.players.get_mut(&pid) {
                            for c in &mut ps.field {
                                if c.orientation == CardOrientation::SLEEP {
                                    let protected = pid != player_id && c.set_cards.iter().any(|sc| {
                                        sc.face_up && self.catalog.get(&sc.definition_id).map(|d| d.description.contains("リムーブされず、スリープされず、スタンされない")).unwrap_or(false)
                                    });
                                    if !protected {
                                        c.orientation = CardOrientation::STUN;
                                        cnt += 1;
                                    }
                                }
                            }
                        }
                    }
                    self.log(format!("💫 [{}] の効果でスリープ状態のキャラ {} 体をスタンさせました", actor_name, cnt));
                }
                ImmediateOp::MassAllRemove => {
                    let mut to_remove: Vec<(String, String)> = vec![];
                    for pid in &self.player_order {
                        if let Some(ps) = self.players.get(pid) {
                            for c in &ps.field {
                                to_remove.push((pid.clone(), c.instance_id.clone()));
                            }
                        }
                    }
                    for (oid, cid) in to_remove {
                        let is_opp_eff = oid != player_id;
                        if let Some((rdef, rsets)) = self.move_field_char_to_remove(&oid, &cid, false, is_opp_eff) {
                            self.trigger_on_remove_char(&oid, &cid, &rdef, !is_opp_eff);
                            for (sid, sdid) in rsets {
                                self.trigger_set_card_on_remove(&oid, &sid, &sdid);
                            }
                        }
                    }
                    self.log(format!("💥 [{}] の効果で現場のキャラすべてをリムーブしました！", actor_name));
                }
                ImmediateOp::MassContactRemove => {
                    let mut to_remove: Vec<(String, String)> = vec![];
                    for pid in self.player_order.clone() {
                        if let Some(cid) = self.get_contact_char_id_for_player(&pid) {
                            if self.players.get(&pid).map(|ps| ps.field.iter().any(|c| c.instance_id == cid)).unwrap_or(false) {
                                to_remove.push((pid, cid));
                            }
                        }
                    }
                    for (oid, cid) in to_remove {
                        let is_opp_eff = oid != player_id;
                        if let Some((rdef, rsets)) = self.move_field_char_to_remove(&oid, &cid, false, is_opp_eff) {
                            self.trigger_on_remove_char(&oid, &cid, &rdef, !is_opp_eff);
                            for (sid, sdid) in rsets {
                                self.trigger_set_card_on_remove(&oid, &sid, &sdid);
                            }
                        }
                    }
                    self.log(format!("💥 [{}] の効果でコンタクト中のキャラすべてをリムーブしました！", actor_name));
                }
                ImmediateOp::OptionalSelfOrientAndDiscardHand { target_orientation, count, filter, reveal_only } => {
                    let actor_active = self.players.get(player_id).map(|ps| {
                        ps.field
                            .iter()
                            .find(|c| c.instance_id == actor_id)
                            .or_else(|| ps.partner.as_ref().filter(|p| p.instance_id == actor_id))
                            .map(|c| c.orientation == CardOrientation::ACTIVE)
                            .unwrap_or(false)
                    }).unwrap_or(false);
                    if !actor_active {
                        return false;
                    }
                    let fallback_filter = TargetFilter {
                        owner: OwnerScope::SelfOnly,
                        zone: TargetZone::Hand,
                        count: *count,
                        ..Default::default()
                    };
                    let active_filter = filter.as_ref().unwrap_or(&fallback_filter);
                    let candidates = self.build_filtered_candidates(player_id, Some(actor_id), active_filter);
                    if candidates.len() < *count {
                        return false;
                    }
                    let remaining = &ops[idx + 1..];
                    let max_select = (*count).min(candidates.len()).max(1);
                    let title = if *reveal_only {
                        format!("公開する手札を{}枚まで選んでください（任意・このキャラをスリープ）", max_select)
                    } else {
                        format!("リムーブする手札を{}枚まで選んでください（任意・このキャラをスリープ）", max_select)
                    };
                    let dec_id = self.generate_decision_id();
                    self.pending_decision = Some(json!({
                        "decisionId": dec_id,
                        "revision": self.revision,
                        "playerId": player_id,
                        "kind": "targetSelect",
                        "options": {
                            "actorId": actor_id,
                            "actorName": actor_name,
                            "sourceLevel": None::<i32>,
                            "effectType": "DISCARD_HAND",
                            "targetZone": "HAND",
                            "maxSelect": max_select,
                            "revealOnly": reveal_only,
                            "alsoOrientActor": format!("{:?}", target_orientation),
                            "apDelta": 0,
                            "rushTarget": null,
                            "followUpOps": remaining,
                            "condFollowUp": null,
                            "followUpTarget": follow_up_target,
                            "followUpDeckSearch": follow_up_deck_search,
                            "pendingClausePlan": pending_clause_plan,
                            "afterHintLevel": after_hint_level,
                            "resumeContext": resume_context,
                            "title": title,
                            "candidates": candidates,
                            "optional": true,
                            "abortOnSkip": true
                        }
                    }));
                    return true;
                }
                ImmediateOp::CutinBuffByRemovedLevel(per_lv) => {
                    let buff = self.last_removed_level_sum * *per_lv;
                    if buff != 0 {
                        if let Some(cid) = self.get_contact_char_id_for_player(player_id) {
                            if let Some(ps) = self.players.get_mut(player_id) {
                                if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == cid) {
                                    *self.contact_ap_buffs.entry(c.instance_id.clone()).or_insert(0) += buff;
                                    self.log(format!("⚡ [{}] の効果でコンタクト中キャラのAP +{}", actor_name, buff));
                                }
                            }
                        }
                    }
                }
                ImmediateOp::CutinBuffByRemovedApThousand(per_k) => {
                    let buff = (self.last_removed_ap_sum / 1000) * *per_k;
                    if buff != 0 {
                        if let Some(cid) = self.get_contact_char_id_for_player(player_id) {
                            if let Some(ps) = self.players.get_mut(player_id) {
                                if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == cid) {
                                    *self.contact_ap_buffs.entry(c.instance_id.clone()).or_insert(0) += buff;
                                    self.log(format!("⚡ [{}] の効果でコンタクト中キャラのAP +{}", actor_name, buff));
                                }
                            }
                        }
                    }
                }
                ImmediateOp::MassSelfTraitApBuff { trait_name, buff } => {
                    let mut cnt = 0;
                    if let Some(ps) = self.players.get_mut(player_id) {
                        for c in &mut ps.field {
                            if let Some(def) = self.catalog.get(&c.definition_id) {
                                if has_trait_match(def, trait_name) {
                                    c.temp_ap_bonus += *buff;
                                    cnt += 1;
                                }
                            }
                        }
                    }
                    self.log(format!("✨ [{}] の効果で〚特徴［{}］〛のキャラ {} 体をAP +{}", actor_name, trait_name, cnt, buff));
                }
                ImmediateOp::DrawPerOppStun { if_zero_clause } => {
                    let opp_id = self.opponent_id_of(player_id);
                    let stun_cnt = self.players.get(&opp_id).map(|p| p.field.iter().filter(|c| c.orientation == CardOrientation::STUN).count()).unwrap_or(0);
                    if stun_cnt > 0 {
                        self.draw_cards(player_id, stun_cnt);
                        self.log(format!("✨ [{}] の効果で相手のスタン状態 {} 体につきカードを {} 枚引きました", actor_name, stun_cnt, stun_cnt));
                    } else if let Some(zc) = if_zero_clause {
                        self.enqueue_mode_clauses(player_id, actor_id, actor_name, None, &[zc.clone()], &[], resume_context);
                    }
                }
                ImmediateOp::InvestigateAndCheckName { count, then_clause } => {
                    let opp_id = self.opponent_id_of(player_id);
                    let mut name_options: Vec<String> = Vec::new();
                    if let Some(opp_ps) = self.players.get(&opp_id) {
                        for c in opp_ps.deck.iter().chain(opp_ps.field.iter()).chain(opp_ps.remove.iter()) {
                            let n = self.card_name_by_def(&c.definition_id);
                            if !name_options.contains(&n) && name_options.len() < 8 {
                                name_options.push(n);
                            }
                        }
                    }
                    for default_n in ["江戸川コナン", "毛利蘭", "工藤新一", "灰原哀", "服部平次", "怪盗キッド", "安室透", "赤井秀一"] {
                        if !name_options.iter().any(|x| x == default_n) && name_options.len() < 12 {
                            name_options.push(default_n.to_string());
                        }
                    }
                    self.enqueue_continuation_from_ops(player_id, actor_id, actor_name, &ops[idx + 1..], evidence_source_inst_id, follow_up_target, follow_up_deck_search, pending_clause_plan, resume_context);
                    let dec_id = self.generate_decision_id();
                    self.pending_decision = Some(json!({
                        "decisionId": dec_id,
                        "revision": self.revision,
                        "playerId": player_id,
                        "kind": "modeSelect",
                        "options": {
                            "specialKind": "INVESTIGATE_NAME_SELECT",
                            "actorId": actor_id,
                            "actorName": actor_name,
                            "count": count,
                            "thenClause": then_clause,
                            "minPick": 1,
                            "maxPick": 1,
                            "modes": name_options,
                            "resumeContext": resume_context,
                            "title": format!("[{}] 〚捜査{}〛で指定するカード名を選んでください", actor_name, count)
                        }
                    }));
                    return true;
                }
                ImmediateOp::RegisterNextOppEvidenceRemovedTrigger(body) => {
                    self.next_opp_evidence_remove_triggers.push((
                        player_id.to_string(),
                        actor_id.to_string(),
                        actor_name.to_string(),
                        body.clone(),
                    ));
                    self.log(format!("📌 [{}] 次回相手証拠リムーブ時トリガーを登録: {}", actor_name, body));
                }
                ImmediateOp::ConditionalClause { condition, then_clause, else_clause } => {
                    let ori = self.find_actor_orientation(player_id, actor_id);
                    let cond_ctx = self.build_cond_info(player_id, Some(actor_id), ori, None);
                    let mut ok = crate::effects::eval_cond_phrase(condition, &cond_ctx);
                    if let Some(h_idx) = condition.find("自分の手札が") {
                        let after_h = &condition[h_idx + "自分の手札が".len()..];
                        if let Some(m_idx) = after_h.find("枚以上ある") {
                            if let Ok(min_hand) = after_h[..m_idx].trim().parse::<usize>() {
                                let hand_len = self.players.get(player_id).map(|ps| ps.hand.len()).unwrap_or(0);
                                ok = ok && hand_len >= min_hand;
                            }
                        }
                    }
                    let chosen = if ok { Some(then_clause.clone()) } else { else_clause.clone() };
                    if let Some(cl) = chosen {
                        if !cl.is_empty() {
                            self.enqueue_mode_clauses(player_id, actor_id, actor_name, None, &[cl], &[], resume_context);
                        }
                    }
                }
                ImmediateOp::RevealDeckUntil { filter, add_to_hand, if_found_clause } => {
                    let mut revealed = Vec::new();
                    let mut matched_card: Option<CardInstance> = None;
                    let self_fc = self.players.get(player_id).map(|p| p.effective_file_count() as i32);
                    if let Some(ps) = self.players.get_mut(player_id) {
                        while let Some(c) = ps.deck.pop() {
                            let is_match = self.catalog.get(&c.definition_id).map(|def| {
                                filter.branches.iter().any(|b| branch_matches_with_ctx(def, def.ap, b, self_fc))
                            }).unwrap_or(false);
                            if is_match {
                                matched_card = Some(c);
                                break;
                            }
                            revealed.push(c);
                        }
                        if let Some(mut mc) = matched_card.take() {
                            let mname = self.catalog.get(&mc.definition_id)
                                .and_then(|d| d.names.first().cloned())
                                .unwrap_or_else(|| mc.definition_id.clone());
                            if let Some(d) = self.catalog.get(&mc.definition_id) {
                                self.last_chosen_level = Some(d.level);
                                self.last_chosen_name = Some(mname.clone());
                            }
                            self.last_chosen_instance_id = Some(mc.instance_id.clone());
                            if *add_to_hand {
                                mc.zone = Zone::HAND;
                                mc.face_up = true;
                                ps.hand.push(mc);
                                self.last_chosen_added_to_hand = true;
                            } else {
                                mc.face_up = false;
                                revealed.push(mc);
                            }
                            matched_card = Some(CardInstance { instance_id: mname, ..Default::default() });
                        }
                        let mut rng = rand::thread_rng();
                        revealed.shuffle(&mut rng);
                        for mut rc in revealed {
                            rc.face_up = false;
                            ps.deck.insert(0, rc);
                        }
                    }
                    if let Some(marker) = matched_card {
                        self.log(format!("🔍 [{}] めくり効果で [{}] を公開し手札に加えました", actor_name, marker.instance_id));
                        if let Some(fc) = if_found_clause {
                            self.enqueue_mode_clauses(player_id, actor_id, actor_name, None, &[fc.clone()], &[], resume_context);
                        }
                    } else {
                        self.log(format!("🔍 [{}] めくり効果: 条件に合うカードが見つかりませんでした", actor_name));
                    }
                }
                ImmediateOp::B09033RevealDeploy { look_count, filter, max_repeats } => {
                    let self_fc = self.players.get(player_id).map(|p| p.effective_file_count() as i32);
                    let mut revealed = vec![];
                    if let Some(ps) = self.players.get_mut(player_id) {
                        let dlen = ps.deck.len();
                        let actual = (*look_count).min(dlen);
                        for i in 0..actual {
                            let card = &mut ps.deck[dlen - 1 - i];
                            card.face_up = true;
                            revealed.push(card.clone());
                        }
                    }
                    if !revealed.is_empty() {
                        self.enqueue_continuation_from_ops(player_id, actor_id, actor_name, &ops[idx + 1..], evidence_source_inst_id, follow_up_target, follow_up_deck_search, pending_clause_plan, resume_context);
                        let candidates: Vec<Value> = revealed
                            .iter()
                            .map(|c| {
                                let selectable = self.catalog.get(&c.definition_id).map(|def| {
                                    filter.branches.iter().any(|b| branch_matches_with_ctx(def, def.ap, b, self_fc))
                                }).unwrap_or(false);
                                json!({
                                    "instanceId": c.instance_id,
                                    "definitionId": c.definition_id,
                                    "name": self.card_name_by_def(&c.definition_id),
                                    "level": self.catalog.get(&c.definition_id).map(|d| d.level).unwrap_or(0),
                                    "ap": self.catalog.get(&c.definition_id).map(|d| d.ap).unwrap_or(0),
                                    "colors": self.catalog.get(&c.definition_id).map(|d| d.colors.clone()).unwrap_or_default(),
                                    "traits": self.catalog.get(&c.definition_id).map(|d| d.traits.clone()).unwrap_or_default(),
                                    "selectable": selectable,
                                })
                            })
                            .collect();
                        let remaining = &ops[idx + 1..];
                        let dec_id = self.generate_decision_id();
                        self.pending_decision = Some(json!({
                            "decisionId": dec_id,
                            "revision": self.revision,
                            "playerId": player_id,
                            "kind": "deckSearch",
                            "options": {
                                "actorId": actor_id,
                                "actorName": actor_name,
                                "searchCount": look_count,
                                "maxSelect": 1,
                                "pickAction": "PLAY_ACTIVE",
                                "postAddDiscard": 0,
                                "postOps": [],
                                "remainderToRemove": false,
                                "b09033RemRepeats": max_repeats,
                                "b09033RequireFileCost": false,
                                "b09033Filter": filter,
                                "resumeContext": resume_context,
                                "title": format!("山札の上から{}枚公開。Lv6以下の〚高校生〛を1枚まで登場させます", look_count),
                                "revealedCards": candidates,
                                "optional": true
                            }
                        }));
                        self.log(format!("🔍 [{}] の効果で山札の上から {} 枚公開しました", actor_name, look_count));
                        return true;
                    }
                }
                ImmediateOp::OptionalDeckTopRemove { count, then_clause, black_cutin_threshold } => {
                    let has_deck = self.players.get(player_id).map(|p| p.deck.len() >= *count).unwrap_or(false);
                    if has_deck {
                        self.enqueue_continuation_from_ops(player_id, actor_id, actor_name, &ops[idx + 1..], evidence_source_inst_id, follow_up_target, follow_up_deck_search, pending_clause_plan, resume_context);
                        let remaining = &ops[idx + 1..];
                        let dec_id = self.generate_decision_id();
                        self.pending_decision = Some(json!({
                            "decisionId": dec_id,
                            "revision": self.revision,
                            "playerId": player_id,
                            "kind": "modeSelect",
                            "options": {
                                "specialKind": "OPTIONAL_DECK_TOP_REMOVE",
                                "actorId": actor_id,
                                "actorName": actor_name,
                                "count": count,
                                "thenClause": then_clause,
                                "blackCutinThreshold": black_cutin_threshold,
                                "minPick": 1,
                                "maxPick": 1,
                                "modes": [
                                    format!("山札の上から{}枚リムーブする", count),
                                    "リムーブしない"
                                ],
                                "resumeContext": resume_context,
                                "title": format!("[{}] 山札の上から{}枚リムーブしますか？", actor_name, count)
                            }
                        }));
                        return true;
                    }
                }
                ImmediateOp::OptionalCompoundCost { sleep_partner, hand_remove, file_top_remove, field_remove_name, then_clause } => {
                    let can_pay = self.players.get(player_id).map(|ps| {
                        let p_ok = !*sleep_partner || ps.partner.as_ref().map(|p| p.zone == Zone::PARTNER && p.orientation == CardOrientation::ACTIVE).unwrap_or(false);
                        let h_ok = ps.hand.len() >= *hand_remove;
                        let f_ok = ps.file.len() >= *file_top_remove;
                        let c_ok = field_remove_name.as_ref().map(|fnm| {
                            ps.field.iter().any(|fc| self.catalog.get(&fc.definition_id).map(|d| has_instance_name_match(fc, d, fnm)).unwrap_or(false))
                        }).unwrap_or(true);
                        p_ok && h_ok && f_ok && c_ok
                    }).unwrap_or(false);
                    if can_pay {
                        self.enqueue_continuation_from_ops(player_id, actor_id, actor_name, &ops[idx + 1..], evidence_source_inst_id, follow_up_target, follow_up_deck_search, pending_clause_plan, resume_context);
                        let remaining = &ops[idx + 1..];
                        let dec_id = self.generate_decision_id();
                        self.pending_decision = Some(json!({
                            "decisionId": dec_id,
                            "revision": self.revision,
                            "playerId": player_id,
                            "kind": "modeSelect",
                            "options": {
                                "specialKind": "OPTIONAL_COMPOUND_COST",
                                "actorId": actor_id,
                                "actorName": actor_name,
                                "sleepPartner": sleep_partner,
                                "handRemove": hand_remove,
                                "fileTopRemove": file_top_remove,
                                "fieldRemoveName": field_remove_name,
                                "thenClause": then_clause,
                                "minPick": 1,
                                "maxPick": 1,
                                "modes": [
                                    "コストを支払って効果を発動する",
                                    "支払わない"
                                ],
                                "resumeContext": resume_context,
                                "title": format!("[{}] 追加コストを支払って発動しますか？", actor_name)
                            }
                        }));
                        return true;
                    }
                }
                ImmediateOp::ConditionalLpRemoveOpp => {
                    // 高LP比較系フォールバック: 相手キャラ1体以下リムーブと同等扱いしないよう最低限安全にスキップ
                }
                ImmediateOp::EventModeChoice(mc) => {
                    self.enqueue_continuation_from_ops(player_id, actor_id, actor_name, &ops[idx + 1..], evidence_source_inst_id, follow_up_target, follow_up_deck_search, pending_clause_plan, resume_context);
                    if self.start_event_mode_choice(player_id, actor_id, actor_name, None, mc, resume_context) {
                        return true;
                    }
                }
            }
        }
        false
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn enqueue_continuation_from_ops(
        &mut self,
        player_id: &str,
        actor_id: &str,
        actor_name: &str,
        rem_ops: &[ImmediateOp],
        evidence_source_inst_id: Option<&str>,
        follow_up_target: Option<&TargetStep>,
        follow_up_deck_search: Option<&DeckSearchStep>,
        pending_clause_plan: Option<&ParsedClausePlan>,
        resume_context: &str,
    ) {
        let mut to_insert: Vec<PendingClauseTask> = Vec::new();
        if !rem_ops.is_empty() || follow_up_deck_search.is_some() || follow_up_target.is_some() {
            to_insert.push(PendingClauseTask {
                player_id: player_id.to_string(),
                actor_id: actor_id.to_string(),
                actor_name: actor_name.to_string(),
                source_level: None,
                card_type: CardType::EVENT,
                clause: String::new(),
                resume_context: resume_context.to_string(),
                evidence_source_inst_id: evidence_source_inst_id.map(|s| s.to_string()),
                post_target_ops: vec![],
                plan: Some(ParsedClausePlan {
                    immediate_before: rem_ops.to_vec(),
                    target_step: follow_up_target.cloned(),
                    follow_up_target: None,
                    deck_search: follow_up_deck_search.cloned(),
                    post_target_ops: vec![],
                    extra_clauses: vec![],
                }),
            });
        }
        if let Some(pcp) = pending_clause_plan {
            to_insert.push(PendingClauseTask {
                player_id: player_id.to_string(),
                actor_id: actor_id.to_string(),
                actor_name: actor_name.to_string(),
                source_level: None,
                card_type: CardType::EVENT,
                clause: String::new(),
                resume_context: resume_context.to_string(),
                evidence_source_inst_id: evidence_source_inst_id.map(|s| s.to_string()),
                post_target_ops: vec![],
                plan: Some(pcp.clone()),
            });
        }
        for t in to_insert.into_iter().rev() {
            self.pending_clause_queue.insert(0, t);
        }
    }

    pub fn execute_clause_plan(
        &mut self,
        player_id: &str,
        actor_id: &str,
        actor_name: &str,
        source_level: Option<i32>,
        plan: &ParsedClausePlan,
        resume_context: &str,
        evidence_source_inst_id: Option<&str>,
    ) -> bool {
        let prev_executing = self.is_executing_clause_plan;
        self.is_executing_clause_plan = true;
        let res = self.execute_clause_plan_inner(
            player_id,
            actor_id,
            actor_name,
            source_level,
            plan,
            resume_context,
            evidence_source_inst_id,
        );
        self.is_executing_clause_plan = prev_executing;
        res
    }

    fn execute_clause_plan_inner(
        &mut self,
        player_id: &str,
        actor_id: &str,
        actor_name: &str,
        source_level: Option<i32>,
        plan: &ParsedClausePlan,
        resume_context: &str,
        evidence_source_inst_id: Option<&str>,
    ) -> bool {
        let queue_len_before = self.pending_clause_queue.len();
        let has_opt_hand_gate = plan.immediate_before.iter().any(|op| {
            matches!(
                op,
                ImmediateOp::OptionalDiscardHand { .. }
                    | ImmediateOp::OptionalSelfOrientAndDiscardHand { .. }
            )
        });
        let has_opt_target_gate = plan
            .target_step
            .as_ref()
            .map(|ts| ts.then_abort_if_skipped)
            .unwrap_or(false)
            || plan
                .follow_up_target
                .as_ref()
                .map(|ft| ft.then_abort_if_skipped)
                .unwrap_or(false);
        let has_opt_cost_gate = has_opt_hand_gate || has_opt_target_gate;

        // 複合任意コストを含むゲートがある場合、全コスト要素が支払可能か事前チェックする
        if has_opt_cost_gate {
            for op in &plan.immediate_before {
                match op {
                    ImmediateOp::OptionalDiscardHand { count, filter } => {
                        let fallback_filter = TargetFilter {
                            owner: OwnerScope::SelfOnly,
                            zone: TargetZone::Hand,
                            count: *count,
                            ..Default::default()
                        };
                        let active_f = filter.as_ref().unwrap_or(&fallback_filter);
                        if self.build_filtered_candidates(player_id, Some(actor_id), active_f).len() < *count {
                            self.log(format!("⚠️ [{}] の任意コストを支払えないため発動しません", actor_name));
                            return false;
                        }
                    }
                    ImmediateOp::OptionalSelfOrientAndDiscardHand { count, filter, .. } => {
                        let actor_active = self.players.get(player_id).map(|ps| {
                            ps.field
                                .iter()
                                .find(|c| c.instance_id == actor_id)
                                .or_else(|| ps.partner.as_ref().filter(|p| p.instance_id == actor_id))
                                .map(|c| c.orientation == CardOrientation::ACTIVE)
                                .unwrap_or(false)
                        }).unwrap_or(false);
                        let fallback_filter = TargetFilter {
                            owner: OwnerScope::SelfOnly,
                            zone: TargetZone::Hand,
                            count: *count,
                            ..Default::default()
                        };
                        let active_f = filter.as_ref().unwrap_or(&fallback_filter);
                        if !actor_active || self.build_filtered_candidates(player_id, Some(actor_id), active_f).len() < *count {
                            self.log(format!("⚠️ [{}] の任意コストを支払えないため発動しません", actor_name));
                            return false;
                        }
                    }
                    _ => {}
                }
            }
            if let Some(ref ts) = plan.target_step {
                if ts.then_abort_if_skipped {
                    let min_needed = if ts.filter.up_to { 1 } else { ts.filter.count.max(1) };
                    if self.build_filtered_candidates(player_id, Some(actor_id), &ts.filter).len() < min_needed {
                        self.log(format!("⚠️ [{}] の任意コスト対象がいないため発動しません", actor_name));
                        return false;
                    }
                    // Case B (例: B07036 / B09022): 1段目がキャラ操作コストで2段目(extra_clauses[0])が手札コストの場合も事前確認
                    if let Some(first_extra) = plan.extra_clauses.first() {
                        if first_extra.contains("てもよい。そうした場合、") {
                            let sub_plan = parse_effect_steps(first_extra, source_level, CardType::CHARACTER);
                            for op in &sub_plan.immediate_before {
                                if let ImmediateOp::OptionalDiscardHand { count, filter } = op {
                                    let fallback_filter = TargetFilter {
                                        owner: OwnerScope::SelfOnly,
                                        zone: TargetZone::Hand,
                                        count: *count,
                                        ..Default::default()
                                    };
                                    let active_f = filter.as_ref().unwrap_or(&fallback_filter);
                                    if self.build_filtered_candidates(player_id, Some(actor_id), active_f).len() < *count {
                                        self.log(format!("⚠️ [{}] の任意コスト（手札）が不足しているため発動しません", actor_name));
                                        return false;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // post_target_ops / extra_clauses は任意コストゲートがある場合は先にキューへ積まず、成功後の継続プランに預ける
        let mut deferred: Vec<PendingClauseTask> = Vec::new();
        if !plan.post_target_ops.is_empty() {
            deferred.push(PendingClauseTask {
                player_id: player_id.to_string(),
                actor_id: actor_id.to_string(),
                actor_name: actor_name.to_string(),
                source_level,
                card_type: CardType::EVENT,
                clause: String::new(),
                resume_context: resume_context.to_string(),
                evidence_source_inst_id: evidence_source_inst_id.map(|s| s.to_string()),
                post_target_ops: vec![],
                plan: Some(ParsedClausePlan {
                    immediate_before: plan.post_target_ops.clone(),
                    ..Default::default()
                }),
            });
        }
        for ecl in &plan.extra_clauses {
            deferred.push(PendingClauseTask {
                player_id: player_id.to_string(),
                actor_id: actor_id.to_string(),
                actor_name: actor_name.to_string(),
                source_level,
                card_type: CardType::EVENT,
                clause: ecl.clone(),
                resume_context: resume_context.to_string(),
                evidence_source_inst_id: evidence_source_inst_id.map(|s| s.to_string()),
                post_target_ops: vec![],
                plan: None,
            });
        }
        let has_deferred = !deferred.is_empty();
        if !has_opt_cost_gate {
            for t in deferred.into_iter().rev() {
                self.pending_clause_queue.insert(0, t);
            }
        }

        if !plan.immediate_before.is_empty() {
            let cont_plan = if has_opt_cost_gate
                || plan.target_step.is_some()
                || plan.follow_up_target.is_some()
            {
                let cp = ParsedClausePlan {
                    immediate_before: vec![],
                    target_step: plan.target_step.clone(),
                    follow_up_target: plan.follow_up_target.clone(),
                    deck_search: if has_opt_cost_gate { plan.deck_search.clone() } else { None },
                    post_target_ops: if has_opt_cost_gate { plan.post_target_ops.clone() } else { vec![] },
                    extra_clauses: if has_opt_cost_gate { plan.extra_clauses.clone() } else { vec![] },
                };
                if cp.target_step.is_some()
                    || cp.follow_up_target.is_some()
                    || cp.deck_search.is_some()
                    || !cp.post_target_ops.is_empty()
                    || !cp.extra_clauses.is_empty()
                {
                    Some(cp)
                } else {
                    None
                }
            } else {
                None
            };
            if self.execute_immediate_ops_with_continuation(
                player_id,
                actor_id,
                actor_name,
                &plan.immediate_before,
                evidence_source_inst_id,
                None,
                if has_opt_cost_gate { None } else { plan.deck_search.as_ref() },
                cont_plan.as_ref(),
                resume_context,
                None,
            ) {
                return true;
            }
            if self.phase == MatchPhase::FINISHED {
                return true;
            }
            // OptionalDiscardHand / OptionalSelfOrientAndDiscardHand は返り値 false でもスキップ扱いのため後続へ進まない
            if has_opt_hand_gate {
                return false;
            }
        }

        if let Some(ref ds) = plan.deck_search {
            if self.prompt_deck_search_step(
                player_id,
                actor_id,
                actor_name,
                source_level,
                ds,
                plan.target_step.as_ref(),
                resume_context,
                None,
            ) {
                return true;
            }
        }

        if let Some(ref ts) = plan.target_step {
            let after_target_plan = if has_opt_cost_gate
                && (!plan.post_target_ops.is_empty() || !plan.extra_clauses.is_empty())
            {
                Some(ParsedClausePlan {
                    immediate_before: vec![],
                    target_step: None,
                    follow_up_target: None,
                    deck_search: None,
                    post_target_ops: plan.post_target_ops.clone(),
                    extra_clauses: plan.extra_clauses.clone(),
                })
            } else {
                None
            };
            if self.prompt_target_step_with_plan(
                player_id,
                actor_id,
                actor_name,
                source_level,
                ts,
                plan.follow_up_target.as_deref(),
                after_target_plan.as_ref(),
                resume_context,
                None,
            ) {
                return true;
            }
            if ts.then_abort_if_skipped {
                return false;
            }
            let mut skipped_follow_ops = ts.follow_up.clone();
            skipped_follow_ops.extend(plan.post_target_ops.clone());
            if !skipped_follow_ops.is_empty() {
                if self.execute_immediate_ops_with_continuation(
                    player_id,
                    actor_id,
                    actor_name,
                    &skipped_follow_ops,
                    None,
                    plan.follow_up_target.as_deref(),
                    None,
                    after_target_plan.as_ref(),
                    resume_context,
                    None,
                ) {
                    return true;
                }
            } else if let Some(ref ft) = plan.follow_up_target {
                if self.prompt_target_step_with_plan(
                    player_id,
                    actor_id,
                    actor_name,
                    source_level,
                    ft,
                    None,
                    after_target_plan.as_ref(),
                    resume_context,
                    None,
                ) {
                    return true;
                }
            }
            if !plan.extra_clauses.is_empty() {
                self.enqueue_mode_clauses(
                    player_id,
                    actor_id,
                    actor_name,
                    source_level,
                    &plan.extra_clauses,
                    &[],
                    resume_context,
                );
                if self.drain_pending_clause_queue() {
                    return true;
                }
            }
        }

        if (has_deferred && !has_opt_cost_gate) || self.pending_clause_queue.len() > queue_len_before {
            return self.drain_pending_clause_queue();
        }
        false
    }
}
