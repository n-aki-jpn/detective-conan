use super::*;

impl MatchEngine {

    /// フィールド上のキャラの実効APを計算する（ベースAP + 一時バフ + パッシブ自己/オーラ）
    pub fn get_effective_ap(&self, owner_id: &str, card: &CardInstance) -> i32 {
        let in_contact = self.is_card_in_active_contact(owner_id, &card.instance_id);
        self.get_effective_ap_with_contact(owner_id, card, in_contact)
    }

    pub(super) fn is_card_in_active_contact(&self, owner_id: &str, inst_id: &str) -> bool {
        let Some(ctx) = self.action_context.as_ref() else {
            return false;
        };
        let guard_resolved = ctx.get("guardResolved").and_then(|v| v.as_bool()).unwrap_or(false);
        if !guard_resolved {
            return false;
        }
        let att_pid = ctx.get("attackerPlayerId").and_then(|v| v.as_str()).unwrap_or("");
        let def_pid = ctx.get("defenderPlayerId").and_then(|v| v.as_str()).unwrap_or("");
        let actor_id = ctx.get("actorId").and_then(|v| v.as_str()).unwrap_or("");
        let guard_id = ctx.get("guardCardId").and_then(|v| v.as_str());
        let target_id = ctx.get("targetId").and_then(|v| v.as_str()).unwrap_or("");
        if owner_id == att_pid && inst_id == actor_id {
            return true;
        }
        if owner_id == def_pid {
            if let Some(gid) = guard_id {
                return inst_id == gid;
            }
            return inst_id == target_id;
        }
        false
    }

    pub fn get_effective_ap_with_contact(&self, owner_id: &str, card: &CardInstance, in_contact: bool) -> i32 {
        let contact_bonus = self.contact_ap_buffs.get(&card.instance_id).copied().unwrap_or(0);
        let Some(target_def) = self.catalog.get(&card.definition_id) else {
            return card.base_ap_override.unwrap_or(0) + card.temp_ap_bonus + contact_bonus;
        };
        let base_ap = card.base_ap_override.unwrap_or(target_def.ap);
        let mut total = base_ap + card.temp_ap_bonus + contact_bonus;

        if card.zone != Zone::FIELD {
            return total;
        }

        if in_contact {
            if let Some(ps) = self.players.get(owner_id) {
                let card_lv = target_def.level + card.temp_level_delta;
                for (src, in_partner) in ps.field.iter().map(|c| (c, false)).chain(ps.extra_partners.iter().map(|c| (c, true))) {
                    if let Some(sdef) = self.catalog.get(&src.definition_id) {
                        for line in sdef.description.lines() {
                            if in_partner && !line.contains("パートナーエリアでも発動する") {
                                continue;
                            }
                            if line.contains("自分の現場にいる") && line.contains("コンタクトしたとき") {
                                if let Some(min_lv) = crate::effects::extract_number_before(line, "レベル", "以上のキャラがコンタクトしたとき") {
                                    if card_lv >= min_lv {
                                        if let Some(ap_buff) = crate::effects::parse_signed_ap(line, true) {
                                            total += ap_buff;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // セットカードからのAPバフ（自分のターン中+2000、コンタクト中+2000等）
        for sc in &card.set_cards {
            if !sc.face_up {
                continue;
            }
            if let Some(sc_def) = self.catalog.get(&sc.definition_id) {
                let d = &sc_def.description;
                if d.contains("このキャラは自分のターン中、AP＋") && self.turn_player_id == owner_id {
                    total += crate::effects::parse_signed_ap(d, true).unwrap_or(0);
                }
                if in_contact && d.contains("コンタクト中、このキャラをAP＋") {
                    if d.contains("〚特徴［少年探偵団］〛のキャラは") {
                        if has_trait_match(target_def, "少年探偵団") {
                            total += crate::effects::parse_signed_ap(d, true).unwrap_or(2000);
                        }
                    } else {
                        total += crate::effects::parse_signed_ap(d, true).unwrap_or(2000);
                    }
                }
            }
        }
        if in_contact && card.contact_draw_and_ap1000_this_turn {
            total += 1000;
        }

        let target_base_ap = base_ap + card.temp_ap_bonus + contact_bonus;
        for src_pid in &self.player_order {
            let Some(src_ps) = self.players.get(src_pid) else {
                continue;
            };
            let opp_pid = self.opponent_id_of(src_pid);
            let opp_field_cards: Vec<(&CardInstance, &crate::models::CardDefinition)> = self
                .players
                .get(&opp_pid)
                .map(|ops| {
                    ops.field
                        .iter()
                        .filter_map(|inst| self.catalog.get(&inst.definition_id).map(|d| (inst, d)))
                        .collect()
                })
                .unwrap_or_default();
            let self_faceup_ev = src_ps.evidence.iter().filter(|e| e.face_up).count();
            let self_hand_cnt = src_ps.hand.len();
            let friendly = src_pid == owner_id;

            let mut sources: Vec<(&CardInstance, bool)> = src_ps.field.iter().map(|c| (c, false)).collect();
            if let Some(ref p) = src_ps.partner {
                if p.zone == Zone::PARTNER {
                    sources.push((p, true));
                }
            }
            for ep in &src_ps.extra_partners {
                sources.push((ep, true));
            }
            if let Some(ref cs) = src_ps.r#case {
                sources.push((cs, false));
            }

            for (src_inst, in_partner) in sources {
                let Some(src_def) = self.catalog.get(&src_inst.definition_id) else {
                    continue;
                };
                let passive_clauses = extract_clauses(&src_def.description, TriggerKind::Passive, src_def.card_type);
                if passive_clauses.is_empty() {
                    continue;
                }
                let src_cond = self.build_cond_info(src_pid, Some(&src_inst.instance_id), src_inst.orientation, None);
                let pctx = PassiveEvalContext {
                    source_instance_id: &src_inst.instance_id,
                    source_def: src_def,
                    source_in_partner_area: in_partner && src_def.card_type != CardType::PARTNER,
                    source_cond: &src_cond,
                    target_instance_id: &card.instance_id,
                    target_def,
                    target_base_ap,
                    target_is_friendly_to_source: friendly,
                    opp_field_cards: &opp_field_cards,
                    self_hand_count: self_hand_cnt,
                    self_faceup_evidence_count: self_faceup_ev,
                };
                for clause in passive_clauses {
                    total += compute_passive_ap_for_target(&clause, &pctx);
                }
            }
        }

        total.max(0)
    }

    /// フィールド上のキャラ・パートナーの実効LPを計算する（ベースLP + 常時パッシブ）
    pub fn get_effective_lp(&self, owner_id: &str, card: &CardInstance) -> i32 {
        let Some(target_def) = self.catalog.get(&card.definition_id) else {
            return 0;
        };
        let mut total = target_def.lp + card.temp_lp_bonus;
        if card.zone != Zone::FIELD && card.zone != Zone::PARTNER {
            return total;
        }
        let Some(ps) = self.players.get(owner_id) else {
            return total;
        };
        let passive_clauses = extract_clauses(&target_def.description, TriggerKind::Passive, target_def.card_type);
        if passive_clauses.is_empty() {
            return total;
        }
        let opp_pid = self.opponent_id_of(owner_id);
        let opp_field_cards: Vec<(&CardInstance, &crate::models::CardDefinition)> = self
            .players
            .get(&opp_pid)
            .map(|ops| {
                ops.field
                    .iter()
                    .filter_map(|inst| self.catalog.get(&inst.definition_id).map(|d| (inst, d)))
                    .collect()
            })
            .unwrap_or_default();
        let src_cond = self.build_cond_info(owner_id, Some(&card.instance_id), card.orientation, None);
        let pctx = PassiveEvalContext {
            source_instance_id: &card.instance_id,
            source_def: target_def,
            source_in_partner_area: card.zone == Zone::PARTNER && target_def.card_type != CardType::PARTNER,
            source_cond: &src_cond,
            target_instance_id: &card.instance_id,
            target_def,
            target_base_ap: target_def.ap + card.temp_ap_bonus + self.contact_ap_buffs.get(&card.instance_id).copied().unwrap_or(0),
            target_is_friendly_to_source: true,
            opp_field_cards: &opp_field_cards,
            self_hand_count: ps.hand.len(),
            self_faceup_evidence_count: ps.evidence.iter().filter(|e| e.face_up).count(),
        };
        for clause in passive_clauses {
            total += compute_passive_lp_bonus(&clause, &pctx);
        }
        total.max(0)
    }

    /// プレイヤーのパートナー色を取得する
    pub fn get_player_partner_colors(&self, player_id: &str) -> HashSet<String> {
        let mut set = HashSet::new();
        if let Some(ps) = self.players.get(player_id) {
            if let Some(ref p) = ps.partner {
                if let Some(p_def) = self.catalog.get(&p.definition_id) {
                    for color in &p_def.colors {
                        set.insert(color.clone());
                    }
                }
            }
        }
        set
    }

    /// 効果テキストの発動条件を満たしているか判定する
    pub fn check_effect_conditions(&self, player_id: &str, clause: &str) -> bool {
        let ctx = self.build_cond_info(player_id, None, CardOrientation::ACTIVE, None);
        check_clause_conditions(clause, &ctx).is_ok()
    }

    /// ターン内・カード名BAN（B06103 ジン等）: そのプレイヤーが指定カードを使用・登場させられるか
    pub(super) fn is_name_banned_this_turn(
        &self,
        player_id: &str,
        inst: &CardInstance,
        def: &crate::models::CardDefinition,
    ) -> bool {
        let Some(bans) = self.turn_name_bans.get(player_id) else {
            return false;
        };
        bans.iter().any(|b| has_instance_name_match(inst, def, b))
    }

    // ─── Unified Effect Execution Helpers ────────────────────────────────────

    pub(super) fn opponent_id_of(&self, player_id: &str) -> String {
        self.player_order
            .iter()
            .find(|p| *p != player_id)
            .cloned()
            .unwrap_or_default()
    }

    pub(super) fn find_actor_orientation(&self, player_id: &str, actor_id: &str) -> CardOrientation {
        self.players
            .get(player_id)
            .and_then(|ps| {
                ps.field
                    .iter()
                    .find(|c| c.instance_id == actor_id)
                    .or_else(|| ps.partner.as_ref().filter(|p| p.instance_id == actor_id))
                    .map(|c| c.orientation)
            })
            .unwrap_or(CardOrientation::ACTIVE)
    }

    pub fn build_cond_info<'a>(
        &'a self,
        player_id: &str,
        actor_id: Option<&'a str>,
        actor_orientation: CardOrientation,
        played_by_effect_level: Option<i32>,
    ) -> PlayerCondInfo<'a> {
        let ps = self.players.get(player_id);
        let opp_id = self.opponent_id_of(player_id);
        let opp_ps = self.players.get(&opp_id);

        let partner_def = ps
            .and_then(|p| p.partner.as_ref())
            .and_then(|c| self.catalog.get(&c.definition_id));
        let case_def = ps
            .and_then(|p| p.r#case.as_ref())
            .and_then(|c| self.catalog.get(&c.definition_id));
        let case_level = ps
            .and_then(|p| {
                case_def.and_then(|cd| {
                    cd.case_levels.as_ref().and_then(|cl| {
                        let key = if p.is_first_player { "first" } else { "second" };
                        cl.get(key).copied()
                    })
                })
            })
            .unwrap_or(0);

        let field_cards: Vec<(&CardInstance, &crate::models::CardDefinition)> = ps
            .map(|p| {
                p.field
                    .iter()
                    .filter_map(|inst| self.catalog.get(&inst.definition_id).map(|def| (inst, def)))
                    .collect()
            })
            .unwrap_or_default();

        let self_remove_cards: Vec<(&CardInstance, &crate::models::CardDefinition)> = ps
            .map(|p| {
                p.remove
                    .iter()
                    .filter_map(|inst| self.catalog.get(&inst.definition_id).map(|def| (inst, def)))
                    .collect()
            })
            .unwrap_or_default();

        let mut self_partner_cards: Vec<(&CardInstance, &crate::models::CardDefinition)> = Vec::new();
        if let Some(p) = ps {
            if let Some(ref pt) = p.partner {
                if pt.zone == Zone::PARTNER {
                    if let Some(d) = self.catalog.get(&pt.definition_id) {
                        self_partner_cards.push((pt, d));
                    }
                }
            }
            for ep in &p.extra_partners {
                if let Some(d) = self.catalog.get(&ep.definition_id) {
                    self_partner_cards.push((ep, d));
                }
            }
        }

        PlayerCondInfo {
            actor_id,
            actor_orientation,
            is_my_turn: self.turn_player_id == player_id,
            case_phase: ps.map(|p| p.case_phase).unwrap_or(CasePhase::INTRO),
            file_count: ps.map(|p| p.effective_file_count()).unwrap_or(0),
            self_evidence_count: ps.map(|p| p.evidence.len()).unwrap_or(0),
            opp_evidence_count: opp_ps.map(|p| p.evidence.len()).unwrap_or(0),
            self_field_count: ps.map(|p| p.field.len()).unwrap_or(0),
            opp_field_count: opp_ps.map(|p| p.field.len()).unwrap_or(0),
            case_level,
            partner_def,
            case_def,
            field_cards,
            played_by_effect_level,
            self_ap: actor_id
                .and_then(|aid| ps.and_then(|p| p.field.iter().find(|c| c.instance_id == aid)))
                .map(|c| {
                    let base = c.base_ap_override.unwrap_or_else(|| {
                        self.catalog.get(&c.definition_id).map(|d| d.ap).unwrap_or(0)
                    });
                    base + c.temp_ap_bonus + self.contact_ap_buffs.get(&c.instance_id).copied().unwrap_or(0)
                })
                .unwrap_or(0),
            self_contact_removed_opp_this_turn: actor_id
                .map(|aid| self.contact_removed_actors_this_turn.contains(aid))
                .unwrap_or(false),
            self_remove_cards,
            self_partner_cards,
            self_entered_field_this_turn: self.self_entered_field_players_this_turn.contains(player_id),
            opp_refreshed: self.refreshed_players.contains(&opp_id),
            played_by_effect: self.played_by_effect_events,
            last_deployed_count: self.last_deployed_count,
            last_removed_black_cutin_count: self.last_removed_black_cutin_count,
            last_chosen_added_to_hand: self.last_chosen_added_to_hand,
            last_chosen_level: self.last_chosen_level,
            last_chosen_name: self.last_chosen_name.clone(),
            last_removed_level_sum: self.last_removed_level_sum,
            last_disguise_replaced_name: self.last_disguise_replaced_name.clone(),
            last_slept_names: self.last_slept_names.clone(),
            actor_effective_ap: None,
        }
    }

    pub fn effective_rush_and_swift(&self, c: &CardInstance) -> (bool, Option<String>, bool) {
        let mut has_rush = false;
        let mut rush_target: Option<String> = None;
        let mut has_swift = false;

        if let Some(def) = self.catalog.get(&c.definition_id) {
            let mut ctx = self.build_cond_info(&c.owner_id, Some(&c.instance_id), c.orientation, None);
            ctx.actor_effective_ap = Some(self.get_effective_ap(&c.owner_id, c));
            let (r, rt, sw) = evaluate_passive_rush_and_swift(c, def, &ctx);
            has_rush = r;
            rush_target = rt;
            has_swift = sw;
        } else if c.has_rush {
            merge_rush_capabilities(&mut has_rush, &mut rush_target, &mut has_swift, c.rush_target.as_deref());
        }

        for sc in &c.set_cards {
            if !sc.face_up {
                continue;
            }
            if let Some(sdef) = self.catalog.get(&sc.definition_id) {
                let d = &sdef.description;
                if d.contains("〚突撃［レベル4以下のキャラ］〛") || d.contains("【突撃［レベル4以下のキャラ］】") {
                    merge_rush_capabilities(&mut has_rush, &mut rush_target, &mut has_swift, Some("CHAR_LV4"));
                } else if d.contains("〚突撃［キャラ］〛") || d.contains("【突撃［キャラ］】") {
                    merge_rush_capabilities(&mut has_rush, &mut rush_target, &mut has_swift, Some("CHAR"));
                } else if d.contains("〚突撃［事件］〛") || d.contains("【突撃［事件］】") {
                    merge_rush_capabilities(&mut has_rush, &mut rush_target, &mut has_swift, Some("CASE"));
                } else if d.contains("〚迅速〛") || d.contains("【迅速】") {
                    merge_rush_capabilities(&mut has_rush, &mut rush_target, &mut has_swift, Some("SWIFT"));
                } else if d.contains("〚突撃〛") || d.contains("【突撃】") {
                    merge_rush_capabilities(&mut has_rush, &mut rush_target, &mut has_swift, None);
                }
            }
        }
        (has_rush, rush_target, has_swift)
    }

    pub fn effective_swift(&self, c: &CardInstance) -> bool {
        self.effective_rush_and_swift(c).2
    }

    pub fn effective_rush(&self, c: &CardInstance) -> (bool, Option<String>) {
        let (r, rt, _) = self.effective_rush_and_swift(c);
        (r, rt)
    }

    pub fn effective_can_attack_active(&self, c: &CardInstance) -> bool {
        if c.can_attack_active {
            return true;
        }
        for sc in &c.set_cards {
            if !sc.face_up {
                continue;
            }
            if let Some(sdef) = self.catalog.get(&sc.definition_id) {
                if sdef.description.contains("相手の現場にいるアクティブ状態のキャラを指定してアクションできる") {
                    return true;
                }
            }
        }
        false
    }

    pub fn effective_bullet(&self, player_id: &str, actor_id: &str) -> bool {
        let Some(ps) = self.players.get(player_id) else {
            return false;
        };
        let Some(c) = ps.field.iter().find(|c| c.instance_id == actor_id) else {
            return false;
        };
        if c.has_bullet {
            return true;
        }
        for sc in &c.set_cards {
            if !sc.face_up {
                continue;
            }
            if let Some(sdef) = self.catalog.get(&sc.definition_id) {
                if sdef.description.contains("〚ブレット〛を持つ") {
                    return true;
                }
            }
        }
        false
    }

    pub(super) fn attacker_forces_guard(&self, player_id: &str, actor_id: &str) -> bool {
        let Some(ps) = self.players.get(player_id) else {
            return false;
        };
        let Some(c) = ps.field.iter().find(|c| c.instance_id == actor_id) else {
            return false;
        };
        for sc in &c.set_cards {
            if !sc.face_up {
                continue;
            }
            if let Some(sdef) = self.catalog.get(&sc.definition_id) {
                if sdef.description.contains("相手はガードできる場合、必ずガードする") {
                    return true;
                }
            }
        }
        false
    }

    pub(super) fn is_protected_from_opp_sleep_or_stun(&self, _owner_id: &str, c: &CardInstance) -> bool {
        for sc in &c.set_cards {
            if !sc.face_up {
                continue;
            }
            if let Some(sdef) = self.catalog.get(&sc.definition_id) {
                if sdef.description.contains("相手の能力や効果によってリムーブされず、スリープされず、スタンされない") {
                    return true;
                }
            }
        }
        false
    }

    pub(super) fn is_protected_from_opp_target(&self, owner_id: &str, c: &CardInstance) -> bool {
        if let Some(def) = self.catalog.get(&c.definition_id) {
            let cond_ctx = self.build_cond_info(owner_id, Some(&c.instance_id), c.orientation, None);
            if card_is_untargetable_by_opponent(def, &cond_ctx) {
                return true;
            }
            // B08017: 江戸川コナンがスリープでセットありなら、味方の灰原哀は相手に選ばれない
            if has_instance_name_match(c, def, "灰原哀") {
                if let Some(ps) = self.players.get(owner_id) {
                    let protected = ps.field.iter().any(|fc| {
                        fc.orientation == CardOrientation::SLEEP
                            && fc.set_cards.iter().any(|sc| {
                                sc.face_up
                                    && self
                                        .catalog
                                        .get(&sc.definition_id)
                                        .map(|sd| sd.description.contains("〚カード名［灰原哀］〛は相手の能力や効果によって選ばれない"))
                                        .unwrap_or(false)
                            })
                    });
                    if protected {
                        return true;
                    }
                }
            }
        }
        false
    }
}
