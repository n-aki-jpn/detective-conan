use super::*;

impl MatchEngine {

    pub(super) fn prompt_mulligan(&mut self, player_id: &str) {
        let ps = match self.players.get(player_id) {
            Some(p) => p,
            None => return,
        };
        let options: Vec<String> = ps.hand.iter().map(|c| c.instance_id.clone()).collect();
        let dec_id = self.generate_decision_id();
        self.pending_decision = Some(json!({
            "decisionId": dec_id,
            "revision": self.revision,
            "playerId": player_id,
            "kind": "mulligan",
            "options": {
                "handCardIds": options
            }
        }));
    }

    pub(super) fn handle_mulligan_answer(&mut self, player_id: &str, val: &Value) {
        let cards_to_replace: Vec<String> = val
            .get("cardIds")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|s| s.as_str().map(|str| str.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        if let Some(ps) = self.players.get_mut(player_id) {
            if !cards_to_replace.is_empty() {
                let mut kept = vec![];
                let mut returned = vec![];
                for c in ps.hand.drain(..) {
                    if cards_to_replace.contains(&c.instance_id) {
                        returned.push(c);
                    } else {
                        kept.push(c);
                    }
                }

                let replace_count = returned.len();
                for mut c in returned {
                    c.zone = Zone::DECK;
                    ps.deck.push(c);
                }

                let mut rng = rand::thread_rng();
                ps.deck.shuffle(&mut rng);

                for _ in 0..replace_count {
                    if let Some(mut draw_c) = ps.deck.pop() {
                        draw_c.zone = Zone::HAND;
                        draw_c.face_up = true;
                        kept.push(draw_c);
                    }
                }
                ps.hand = kept;
                self.log(format!("🃏 {} が手札 {} 枚を引き直しました", player_id, replace_count));
            } else {
                self.log(format!("🃏 {} は初期手札をキープしました", player_id));
            }
        }

        if self.phase == MatchPhase::MULLIGAN_FIRST {
            self.phase = MatchPhase::MULLIGAN_SECOND;
            let next_pid = self.player_order[1].clone();
            self.prompt_mulligan(&next_pid);
        } else if self.phase == MatchPhase::MULLIGAN_SECOND {
            self.phase = MatchPhase::REVEAL;
            for p in self.players.values_mut() {
                if let Some(ref mut pt) = p.partner {
                    pt.face_up = true;
                }
                if let Some(ref mut cs) = p.r#case {
                    cs.face_up = true;
                }
            }
            let first_player = self.player_order[0].clone();
            self.start_turn(1, &first_player);
        }
    }

    pub(super) fn reset_turn_card_modifiers(c: &mut CardInstance) {
        c.temp_ap_bonus = 0;
        c.temp_lp_bonus = 0;
        c.has_rush = false;
        c.rush_target = None;
        c.used_declare_this_turn = false;
        c.can_attack_active = false;
        c.must_guard_this_turn = false;
        c.has_bullet = false;
        c.base_ap_override = None;
        c.temp_level_delta = 0;
        c.draw_on_action_this_turn = false;
        c.contact_draw_and_ap1000_this_turn = false;
        c.bullet_on_case_attack_discard_this_turn = false;
        c.temp_name_override = None;
        for sc in &mut c.set_cards {
            sc.used_declare_this_turn = false;
        }
    }

    pub(super) fn start_turn(&mut self, turn: i32, turn_player_id: &str) {
        self.turn = turn;
        self.turn_player_id = turn_player_id.to_string();
        self.turn_clause_usage.clear();
        self.turn_field_plays_by_player.clear();
        self.turn_disguise_blocked_players.clear();
        self.turn_hint_blocked_players.clear();
        self.turn_cutin_blocked_players.clear();
        self.turn_event_blocked_players.clear();
        self.turn_name_bans.clear();
        self.self_entered_field_players_this_turn.clear();
        self.action_hirameki_blocked_player = None;
        self.pending_hint_trigger = None;
        self.pending_ally_play.clear();

        for ps in self.players.values_mut() {
            for c in &mut ps.field {
                Self::reset_turn_card_modifiers(c);
            }
            if let Some(ref mut p) = ps.partner {
                Self::reset_turn_card_modifiers(p);
            }
            for ep in &mut ps.extra_partners {
                Self::reset_turn_card_modifiers(ep);
            }
            if let Some(ref mut cs) = ps.r#case {
                cs.used_declare_this_turn = false;
            }
        }

        let is_first_player;
        {
            let ps = self.players.get_mut(turn_player_id).unwrap();
            ps.normal_hand_played_this_turn = false;
            ps.hint_played_this_turn = false;

            // AUTO_RESTORE_PARTNER
            if let Some(ref mut p) = ps.partner {
                if p.zone == Zone::FILE {
                    p.zone = Zone::PARTNER;
                    p.orientation = CardOrientation::ACTIVE;
                }
            }

            // AUTO_ACTIVATE
            for c in &mut ps.field {
                if c.orientation == CardOrientation::STUN {
                    c.orientation = CardOrientation::SLEEP;
                } else {
                    c.orientation = CardOrientation::ACTIVE;
                }
            }
            if let Some(ref mut p) = ps.partner {
                if p.zone == Zone::PARTNER {
                    if p.orientation == CardOrientation::STUN {
                        p.orientation = CardOrientation::SLEEP;
                    } else {
                        p.orientation = CardOrientation::ACTIVE;
                    }
                }
            }

            is_first_player = ps.is_first_player;
        }

        self.acted_this_turn_ids.clear();
        self.log(format!("━━━━━━━━━━━━━━━━━━━━"));
        self.log(format!("【ターン {}】{} のターン開始", turn, turn_player_id));

        // AUTO_DRAW 1 card
        self.draw_cards(turn_player_id, 1);
        if self.phase == MatchPhase::FINISHED {
            return;
        }

        // AUTO_FILE
        let file_count = if turn == 1 && is_first_player { 1 } else { 2 };
        for _ in 0..file_count {
            self.file_card_from_deck(turn_player_id);
            if self.phase == MatchPhase::FINISHED {
                return;
            }
        }
        self.log(format!("📥 {} はカードを1枚引き、FILEに {} 枚置きました", turn_player_id, file_count));

        self.phase = MatchPhase::MAIN;
        self.prompt_main_action(turn_player_id);
    }

    /// FILEが7枚以上になった際の【解決編】移行と、事件カードの移行時効果を処理する
    pub fn check_resolution_transition(&mut self, player_id: &str) {
        let mut became_resolution = false;
        let mut case_def_id = None;
        if let Some(ps) = self.players.get_mut(player_id) {
            if ps.effective_file_count() >= 7 && ps.case_phase != CasePhase::RESOLUTION {
                ps.case_phase = CasePhase::RESOLUTION;
                became_resolution = true;
                case_def_id = ps.r#case.as_ref().map(|c| c.definition_id.clone());
            }
        }

        if !became_resolution {
            return;
        }

        self.log(format!("✨ {} のFILEが7枚以上になり、事件が【解決編】に突入しました！", player_id));

        if let Some(def_id) = case_def_id {
            let desc = self.catalog.get(&def_id).map(|d| d.description.clone()).unwrap_or_default();
            let case_name = self.card_name_by_def(&def_id);
            if desc.contains("解決編になったとき") || desc.contains("解決編に移行したとき") {
                // 相手はカードを1枚引く
                if desc.contains("相手はカードを") && desc.contains("引く") {
                    let opp_id = self.player_order.iter().find(|p| *p != player_id).cloned().unwrap_or_default();
                    let draw_n = parse_draw_count(&desc).max(1);
                    self.draw_cards(&opp_id, draw_n);
                    self.log(format!("📜 事件 [{}] の解決編移行時効果: {} はカードを{}枚引きました", case_name, opp_id, draw_n));
                }
                // 自分は手札を1枚リムーブする
                if desc.contains("自分は手札を") && desc.contains("リムーブ") {
                    let case_inst_id = self.players.get(player_id)
                        .and_then(|p| p.r#case.as_ref())
                        .map(|c| c.instance_id.clone())
                        .unwrap_or_default();
                    self.prompt_hand_discard(
                        player_id,
                        &case_inst_id,
                        &case_name,
                        None,
                        1,
                        None,
                        false,
                        false,
                        &[],
                        None,
                        None,
                        None,
                        "MAIN",
                        None,
                    );
                }
            }
        }
    }

    pub(super) fn draw_cards(&mut self, player_id: &str, count: usize) {
        for _ in 0..count {
            let deck_empty = self.players.get(player_id).map(|p| p.deck.is_empty()).unwrap_or(true);
            if deck_empty {
                self.trigger_refresh(player_id);
                if self.phase == MatchPhase::FINISHED {
                    return;
                }
            }
            let ps = self.players.get_mut(player_id).unwrap();
            if let Some(mut card) = ps.deck.pop() {
                card.zone = Zone::HAND;
                card.face_up = true;
                ps.hand.push(card);
            }
            // 山札が0枚になった瞬間に即座へリフレッシュ（ルール手册 p.20 / F §2.3、残数は続行）
            self.refresh_if_deck_emptied(player_id);
            if self.phase == MatchPhase::FINISHED {
                return;
            }
        }
    }

    pub(super) fn file_card_from_deck(&mut self, player_id: &str) {
        let deck_empty = self.players.get(player_id).map(|p| p.deck.is_empty()).unwrap_or(true);
        if deck_empty {
            self.trigger_refresh(player_id);
            if self.phase == MatchPhase::FINISHED {
                return;
            }
        }
        let ps = self.players.get_mut(player_id).unwrap();
        if let Some(mut card) = ps.deck.pop() {
            card.zone = Zone::FILE;
            ps.file.push(card);
        }
        // 山札が0枚になった瞬間に即座へリフレッシュ
        self.refresh_if_deck_emptied(player_id);
    }

    pub fn gain_evidence(&mut self, player_id: &str, count: usize) {
        for _ in 0..count {
            let deck_empty = self.players.get(player_id).map(|p| p.deck.is_empty()).unwrap_or(true);
            if deck_empty {
                self.trigger_refresh(player_id);
                if self.phase == MatchPhase::FINISHED {
                    return;
                }
            }
            let ps = self.players.get_mut(player_id).unwrap();
            if let Some(mut card) = ps.deck.pop() {
                card.zone = Zone::EVIDENCE;
                ps.evidence.push(card);
            }
            // 山札が0枚になった瞬間に即座へリフレッシュ（ルール手册 p.20 / F §2.3、残数は続行）
            self.refresh_if_deck_emptied(player_id);
            if self.phase == MatchPhase::FINISHED {
                return;
            }
        }
    }

    /// 1枚のカードが山札から「実際に離脱」した直後に呼び、山札が0枚になった瞬間かどうかを見る。
    /// 「見る／公開」だけで山札に戻す処理（捜査・めくり）では呼ばない（docs/rules.md §リフレッシュ p.20）。
    /// 0枚になったら即座へリフレッシュ（remove0なら敗北）。呼び出し側の残枚数処理はそのまま続行する。
    pub(super) fn refresh_if_deck_emptied(&mut self, player_id: &str) {
        if self.phase == MatchPhase::FINISHED {
            return;
        }
        let deck_empty = self.players.get(player_id).map(|p| p.deck.is_empty()).unwrap_or(false);
        if deck_empty {
            self.trigger_refresh(player_id);
        }
    }

    pub(super) fn trigger_refresh(&mut self, player_id: &str) {
        let remove_empty = self.players.get(player_id).map(|p| p.remove.is_empty()).unwrap_or(true);
        if remove_empty {
            let other_id = self.player_order.iter().find(|pid| *pid != player_id).cloned().unwrap_or_default();
            self.finish_match(&other_id, "REFRESH_EMPTY_REMOVE");
            return;
        }

        let other_id = self.player_order.iter().find(|pid| *pid != player_id).cloned().unwrap_or_default();

        let ps = self.players.get_mut(player_id).unwrap();
        while let Some(mut c) = ps.remove.pop() {
            c.zone = Zone::DECK;
            ps.deck.push(c);
        }
        let mut rng = rand::thread_rng();
        ps.deck.shuffle(&mut rng);

        self.refreshed_players.insert(player_id.to_string());
        self.log(format!("🔄 {} の山札が無くなりリフレッシュ発生！（相手の証拠+1）", player_id));

        // Opponent gains 1 evidence
        self.gain_evidence(&other_id, 1);
    }

    pub fn finish_match(&mut self, winner_id: &str, reason: &str) {
        self.phase = MatchPhase::FINISHED;
        self.winner_id = Some(winner_id.to_string());
        self.finish_reason = Some(reason.to_string());
        self.pending_decision = None;

        let reason_str = match reason {
            "CASE_RESOLVED" => "事件解決",
            "REFRESH_EMPTY_REMOVE" => "リフレッシュ枯渇",
            _ => reason,
        };
        self.log(format!("🏆 【対戦終了】勝者: {} ({})", winner_id, reason_str));
    }
}
