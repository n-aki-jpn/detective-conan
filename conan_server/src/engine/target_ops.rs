use super::*;

impl MatchEngine {

    pub(super) fn apply_single_target_action(
        &mut self,
        player_id: &str,
        actor_id: &str,
        actor_name: &str,
        effect_type: &str,
        target_zone: &str,
        tid: &str,
        idx_in_selection: usize,
        opts: &Value,
        deployed_list: &mut Vec<(String, String, CardOrientation)>,
    ) {
        let target_name = self.card_name_by_inst(tid);
        let opp_id = self.opponent_id_of(player_id);

        // 対象が対戦相手フィールドのキャラかつ B02067（チョーカー型変声機：ターン1無効化）を持つ場合チェック
        let is_opp_field_target = self.players.get(&opp_id).map(|ps| ps.field.iter().any(|c| c.instance_id == tid)).unwrap_or(false);
        if is_opp_field_target {
            let mut b02067_sid: Option<String> = None;
            if let Some(opp_ps) = self.players.get(&opp_id) {
                if let Some(tc) = opp_ps.field.iter().find(|c| c.instance_id == tid) {
                    for sc in &tc.set_cards {
                        if sc.face_up
                            && self
                                .catalog
                                .get(&sc.definition_id)
                                .map(|d| d.description.contains("このキャラが相手の能力や効果によって選ばれたとき、それを無効にする"))
                                .unwrap_or(false)
                            && self.can_use_clause_this_turn(&sc.instance_id, "B02067_NEGATE")
                        {
                            b02067_sid = Some(sc.instance_id.clone());
                            break;
                        }
                    }
                }
            }
            if let Some(sid) = b02067_sid {
                self.mark_clause_used_this_turn(&sid, "B02067_NEGATE");
                self.log(format!("🛡️ [チョーカー型変声機] の効果で [{}] への対象効果を無効化しました！", target_name));
                return;
            }
        }

        // B11080/コンコン/状態追跡用: 選んだ対象の詳細記録
        if let Some(did) = self.find_card_def_id(tid) {
            if let Some(def) = self.catalog.get(&did) {
                self.last_chosen_level = Some(def.level);
                self.last_chosen_name = Some(self.card_name_by_def(&did));
            }
        }
        self.last_chosen_instance_id = Some(tid.to_string());

        match effect_type {
            "SLEEP" => {
                for pid in [opp_id.as_str(), player_id] {
                    let mut protected = false;
                    let mut found_here = false;
                    if let Some(ps) = self.players.get(pid) {
                        if let Some(c) = ps.field.iter().find(|c| c.instance_id == tid) {
                            found_here = true;
                            if pid == opp_id.as_str() && self.is_protected_from_opp_sleep_or_stun(pid, c) {
                                protected = true;
                            }
                        }
                    }
                    if found_here && protected {
                        self.log(format!("🛡️ [{}] は効果耐性によりスリープされませんでした！", target_name));
                        break;
                    }
                    if let Some(ps) = self.players.get_mut(pid) {
                        if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == tid) {
                            c.orientation = CardOrientation::SLEEP;
                            if let Some(ovr) = &c.temp_name_override {
                                self.last_slept_names.push(ovr.clone());
                            }
                            if let Some(def) = self.catalog.get(&c.definition_id) {
                                for n in &def.names {
                                    self.last_slept_names.push(n.clone());
                                }
                            }
                            self.log(format!("💤 [{}] の効果で [{}] をスリープさせました", actor_name, target_name));
                            break;
                        }
                    }
                }
            }
            "STUN" => {
                for pid in [opp_id.as_str(), player_id] {
                    let mut protected = false;
                    let mut found_here = false;
                    if let Some(ps) = self.players.get(pid) {
                        if let Some(c) = ps.field.iter().find(|c| c.instance_id == tid) {
                            found_here = true;
                            if pid == opp_id.as_str() && self.is_protected_from_opp_sleep_or_stun(pid, c) {
                                protected = true;
                            }
                        }
                    }
                    if found_here && protected {
                        self.log(format!("🛡️ [{}] は効果耐性によりスタンされませんでした！", target_name));
                        break;
                    }
                    if let Some(ps) = self.players.get_mut(pid) {
                        if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == tid) {
                            c.orientation = CardOrientation::STUN;
                            self.log(format!("💫 [{}] の効果で [{}] をスタンさせました", actor_name, target_name));
                            break;
                        }
                    }
                }
            }
            "REMOVE" => {
                if target_zone == "SET_CARDS" {
                    for pid in [player_id, opp_id.as_str()] {
                        let mut removed_sc: Option<(String, String)> = None;
                        if let Some(ps) = self.players.get_mut(pid) {
                            for fc in &mut ps.field {
                                if let Some(spos) = fc.set_cards.iter().position(|sc| sc.instance_id == tid) {
                                    let mut sc = fc.set_cards.remove(spos);
                                    sc.zone = Zone::REMOVE;
                                    sc.face_up = true;
                                    removed_sc = Some((sc.instance_id.clone(), sc.definition_id.clone()));
                                    ps.remove.push(sc);
                                    break;
                                }
                            }
                        }
                        if let Some((sid, sdid)) = removed_sc {
                            self.log(format!("💥 [{}] の効果でセットされていた [{}] をリムーブしました", actor_name, target_name));
                            self.trigger_set_card_on_remove(pid, &sid, &sdid);
                            break;
                        }
                    }
                    return;
                }
                for pid in [opp_id.clone(), player_id.to_string()] {
                    if self.players.get(&pid).map(|ps| ps.field.iter().any(|c| c.instance_id == tid)).unwrap_or(false) {
                        let is_opp_eff = pid != player_id;
                        if let Some((rem_def, removed_sets)) = self.move_field_char_to_remove(&pid, tid, false, is_opp_eff) {
                            self.log(format!("💥 [{}] の効果で [{}] をリムーブしました！", actor_name, target_name));
                            self.trigger_on_remove_char(&pid, tid, &rem_def, !is_opp_eff);
                            for (sid, sdid) in removed_sets {
                                self.trigger_set_card_on_remove(&pid, &sid, &sdid);
                            }
                        } else {
                            self.log(format!("🛡️ [{}] は効果/身代わりによりリムーブを免れました！", target_name));
                        }
                        break;
                    }
                }
            }
            "BOTTOM_DECK" | "BOUNCE_TO_HAND" => {
                for pid in [opp_id.clone(), player_id.to_string()] {
                    let has_char = self.players.get(&pid).map(|ps| ps.field.iter().any(|c| c.instance_id == tid)).unwrap_or(false);
                    if !has_char {
                        continue;
                    }
                    let mut sub_remove: Option<String> = None;
                    let mut removed_sets: Vec<(String, String)> = vec![];
                    if let Some(ps) = self.players.get_mut(&pid) {
                        if let Some(idx) = ps.field.iter().position(|c| c.instance_id == tid) {
                            // B01039 check
                            if let Some(sidx) = ps.field[idx].set_cards.iter().position(|sc| {
                                sc.face_up
                                    && self
                                        .catalog
                                        .get(&sc.definition_id)
                                        .map(|d| d.description.contains("このイベントをリムーブし、そのキャラは現場から離れる代わりに現場に残る"))
                                        .unwrap_or(false)
                            }) {
                                let mut sc = ps.field[idx].set_cards.remove(sidx);
                                sc.zone = Zone::REMOVE;
                                sc.face_up = true;
                                sub_remove = Some(sc.definition_id.clone());
                                ps.remove.push(sc);
                            } else {
                                let mut card = ps.field.remove(idx);
                                for mut sc in card.set_cards.drain(..) {
                                    sc.zone = Zone::REMOVE;
                                    sc.face_up = true;
                                    removed_sets.push((sc.instance_id.clone(), sc.definition_id.clone()));
                                    ps.remove.push(sc);
                                }
                                let is_opp_turn = self.turn_player_id != pid;
                                let is_mr = Self::is_mr_def(&self.catalog, &card.definition_id);
                                if is_opp_turn && is_mr {
                                    card.zone = Zone::PARTNER;
                                    card.orientation = CardOrientation::ACTIVE;
                                    card.face_up = true;
                                    ps.extra_partners.push(card);
                                    self.log(format!(
                                        "🌟 MR能力: [{}] は相手ターン中に [{}] の効果で現場を離れたため、パートナーエリアに移動しました！",
                                        target_name, actor_name
                                    ));
                                } else if effect_type == "BOUNCE_TO_HAND" {
                                    card.zone = Zone::HAND;
                                    card.face_up = true;
                                    ps.hand.push(card);
                                    self.log(format!("🤚 [{}] の効果で [{}] を手札に戻しました！", actor_name, target_name));
                                } else {
                                    card.zone = Zone::DECK;
                                    card.face_up = false;
                                    ps.deck.insert(0, card);
                                    self.log(format!("📚 [{}] の効果で [{}] をデッキの下に移しました！", actor_name, target_name));
                                }
                            }
                        }
                    }
                    if let Some(sdef) = sub_remove {
                        self.trigger_on_remove(&pid, &sdef);
                        self.log(format!("🛡️ [{}] は身代わりにより現場に残りました！", target_name));
                    }
                    for (sid, sdid) in removed_sets {
                        self.trigger_set_card_on_remove(&pid, &sid, &sdid);
                    }
                    break;
                }
            }
            "ACTIVATE" => {
                for pid in [player_id, opp_id.as_str()] {
                    if let Some(ps) = self.players.get_mut(pid) {
                        if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == tid) {
                            if c.orientation == CardOrientation::STUN {
                                c.orientation = CardOrientation::SLEEP;
                                self.log(format!("✨ [{}] の効果でスタン状態の [{}] をスリープにしました", actor_name, target_name));
                            } else {
                                c.orientation = CardOrientation::ACTIVE;
                                self.log(format!("✨ [{}] の効果で [{}] をアクティブにしました！", actor_name, target_name));
                            }
                            break;
                        }
                    }
                }
            }
            "AP_BUFF" | "AP_DEBUFF" => {
                let ap_delta = opts
                    .get("apDelta")
                    .and_then(|v| v.as_i64())
                    .map(|n| n as i32)
                    .unwrap_or_else(|| if effect_type == "AP_DEBUFF" { -2000 } else { 1000 });
                if ap_delta != 0 {
                    for pid in [player_id, opp_id.as_str()] {
                        if let Some(ps) = self.players.get_mut(pid) {
                            if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == tid) {
                                c.temp_ap_bonus += ap_delta;
                                let sign = if ap_delta >= 0 { "+" } else { "" };
                                self.log(format!("✨ [{}] の効果で [{}] のAP {}{}", actor_name, target_name, sign, ap_delta));
                                break;
                            }
                        }
                    }
                }
            }
            "GIVE_RUSH" => {
                let r_target = opts.get("rushTarget").and_then(|v| v.as_str()).map(|s| s.to_string());
                if let Some(ps) = self.players.get_mut(player_id) {
                    if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == tid) {
                        c.has_rush = true;
                        c.rush_target = r_target.clone();
                        let label = match r_target.as_deref() {
                            Some("CHAR") => "〚突撃［キャラ］〛",
                            Some("CASE") => "〚突撃［事件］〛",
                            _ => "〚突撃〛",
                        };
                        self.log(format!("⚡ [{}] の効果で [{}] に{}を与えました！", actor_name, target_name, label));
                    }
                }
            }
            "GIVE_BULLET" => {
                if let Some(ps) = self.players.get_mut(player_id) {
                    if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == tid) {
                        c.has_bullet = true;
                        self.log(format!("💥 [{}] の効果で [{}] に〚ブレット〛を与えました！", actor_name, target_name));
                    }
                }
            }
            "GIVE_MUST_GUARD" => {
                for pid in [opp_id.as_str(), player_id] {
                    if let Some(ps) = self.players.get_mut(pid) {
                        if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == tid) {
                            c.must_guard_this_turn = true;
                            self.log(format!("🛡️ [{}] の効果で [{}] はガードできる場合、必ずガードするようになりました！", actor_name, target_name));
                            break;
                        }
                    }
                }
            }
            "GIVE_CAN_ATTACK_ACTIVE" => {
                if let Some(ps) = self.players.get_mut(player_id) {
                    if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == tid) {
                        c.can_attack_active = true;
                        self.log(format!("⚔️ [{}] の効果で [{}] はアクティブ状態のキャラにも攻撃できるようになりました！", actor_name, target_name));
                    }
                }
            }
            "SET_BASE_AP_ZERO" => {
                for pid in [opp_id.as_str(), player_id] {
                    if let Some(ps) = self.players.get_mut(pid) {
                        if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == tid) {
                            c.base_ap_override = Some(0);
                            self.log(format!("📉 [{}] の効果で [{}] の元のAPを0にしました！", actor_name, target_name));
                            break;
                        }
                    }
                }
            }
            "LEVEL_DELTA" => {
                let d_lv = opts.get("levelDelta").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                for pid in [opp_id.as_str(), player_id] {
                    if let Some(ps) = self.players.get_mut(pid) {
                        if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == tid) {
                            c.temp_level_delta += d_lv;
                            self.log(format!("📉 [{}] の効果で [{}] のレベルを {} しました", actor_name, target_name, d_lv));
                            break;
                        }
                    }
                }
            }
            "GRANT_BULLET_ON_CASE_DISCARD" => {
                if let Some(ps) = self.players.get_mut(player_id) {
                    if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == tid) {
                        c.bullet_on_case_attack_discard_this_turn = true;
                        self.log(format!("💥 [{}] の効果で [{}] に事件攻撃時の〚ブレット〛化能力を付与しました", actor_name, target_name));
                    }
                }
            }
            "REVIVE_ACTIVE" | "REVIVE_SLEEP" | "PLAY_FROM_HAND" | "REVIVE_SPLIT_GONIN" | "REVIVE_SPLIT_BAKANA" => {
                let turn = self.turn;
                if effect_type == "REVIVE_SPLIT_GONIN" && idx_in_selection >= 2 {
                    // 五人囃子 (B11017): 3枚目（残り）は手札に加える
                    if let Some(ps) = self.players.get_mut(player_id) {
                        if let Some(idx) = ps.remove.iter().position(|c| c.instance_id == tid) {
                            let mut card = ps.remove.remove(idx);
                            card.zone = Zone::HAND;
                            card.face_up = true;
                            ps.hand.push(card);
                            self.last_chosen_added_to_hand = true;
                            self.log(format!("🤲 [{}] の効果でリムーブエリアから [{}] を手札に加えました！", actor_name, target_name));
                        }
                    }
                    return;
                }
                let ori = if effect_type == "REVIVE_SLEEP"
                    || ((effect_type == "REVIVE_SPLIT_GONIN" || effect_type == "REVIVE_SPLIT_BAKANA") && idx_in_selection >= 1)
                {
                    CardOrientation::SLEEP
                } else {
                    CardOrientation::ACTIVE
                };
                // ターン内・カード名BAN（B06103 ジン等）の防御的再チェック（手札からの登場時）
                if effect_type == "PLAY_FROM_HAND" || target_zone == "HAND" {
                    let (ban_def_id, banned) = {
                        let c_opt = self
                            .players
                            .get(player_id)
                            .and_then(|ps| ps.hand.iter().find(|c| c.instance_id == tid));
                        let did = c_opt.map(|c| c.definition_id.clone()).unwrap_or_default();
                        let banned = match (c_opt, did.as_str()) {
                            (Some(c), d) => self.is_name_banned_this_turn(player_id, c, self.catalog.get(d).unwrap_or_else(|| panic!("catalog missing {d}"))),
                            _ => false,
                        };
                        (did, banned)
                    };
                    if banned {
                        self.log(format!("🚫 {} はこのターン中、〚カード名［{}］〛を使用・登場させられません", player_id, self.card_name_by_def(&ban_def_id)));
                        return;
                    }
                }
                if let Some(ps) = self.players.get_mut(player_id) {
                    {
                        let src_vec = if effect_type == "PLAY_FROM_HAND" || target_zone == "HAND" {
                            &mut ps.hand
                        } else {
                            &mut ps.remove
                        };
                        if let Some(idx) = src_vec.iter().position(|c| c.instance_id == tid) {
                            let mut card = src_vec.remove(idx);
                            let did = card.definition_id.clone();
                            let iid = card.instance_id.clone();
                            card.zone = Zone::FIELD;
                            card.orientation = ori;
                            card.entered_turn = turn;
                            card.face_up = true;
                            self.field_overflow_excluded_ids.insert(iid.clone());
                            ps.field.push(card);
                            self.last_deployed_count += 1;
                            self.self_entered_field_players_this_turn.insert(player_id.to_string());
                            deployed_list.push((iid, did, ori));
                            let zone_txt = if effect_type == "PLAY_FROM_HAND" || target_zone == "HAND" { "手札" } else { "リムーブエリア" };
                            let ori_txt = if ori == CardOrientation::SLEEP { "（スリープ状態）" } else { "" };
                            self.log(format!("✨ [{}] の効果で{}から [{}] を{}登場させました！", actor_name, zone_txt, target_name, ori_txt));
                        }
                    }
                }
            }
            "FIELD_OVERFLOW_REMOVE" => {
                let mut should_clear = false;
                if let Some(ps) = self.players.get_mut(player_id) {
                    if let Some(idx) = ps.field.iter().position(|c| c.instance_id == tid) {
                        let mut card = ps.field.remove(idx);
                        for mut sc in std::mem::take(&mut card.set_cards) {
                            sc.zone = Zone::REMOVE;
                            sc.face_up = true;
                            ps.remove.push(sc);
                        }
                        card.zone = Zone::REMOVE;
                        card.orientation = CardOrientation::ACTIVE;
                        card.face_up = true;
                        ps.remove.push(card);
                        should_clear = ps.field.len() <= 5;
                        self.log(format!("📤 ルール処理: {} の現場上限(5体)超過により [{}] を場外(リムーブエリア)に移しました（効果リムーブではないため誘発なし）", player_id, target_name));
                    }
                }
                if should_clear {
                    self.field_overflow_excluded_ids.clear();
                }
            }
            "RECOVER_TO_HAND" => {
                if let Some(ps) = self.players.get_mut(player_id) {
                    if let Some(idx) = ps.remove.iter().position(|c| c.instance_id == tid) {
                        let mut card = ps.remove.remove(idx);
                        card.zone = Zone::HAND;
                        card.face_up = true;
                        ps.hand.push(card);
                        self.last_chosen_added_to_hand = true;
                        self.log(format!("🤲 [{}] の効果でリムーブエリアから [{}] を手札に加えました！", actor_name, target_name));
                    }
                }
            }
            "DECK_TOP_FROM_REMOVE" => {
                if let Some(ps) = self.players.get_mut(player_id) {
                    if let Some(idx) = ps.remove.iter().position(|c| c.instance_id == tid) {
                        let mut card = ps.remove.remove(idx);
                        card.zone = Zone::DECK;
                        card.face_up = false;
                        ps.deck.push(card);
                        self.log(format!("📚 [{}] の効果でリムーブエリアから [{}] をデッキの上に移しました！", actor_name, target_name));
                    }
                }
            }
            "SET_THIS_EVENT" => {
                // remove actor_id (this event) from ps.remove (or ps.hand for B10018 declare) and attach to target tid
                let mut ev_card: Option<CardInstance> = None;
                let mut is_soccer_no_facedown = false;
                let mut set_desc = String::new();
                if let Some(ps) = self.players.get_mut(player_id) {
                    if let Some(rpos) = ps.remove.iter().position(|c| c.instance_id == actor_id) {
                        ev_card = Some(ps.remove.remove(rpos));
                    } else if let Some(hpos) = ps.hand.iter().position(|c| c.instance_id == actor_id) {
                        ev_card = Some(ps.hand.remove(hpos));
                    }
                    if let Some(mut ev) = ev_card {
                        ev.zone = Zone::FIELD;
                        ev.face_up = true;
                        set_desc = self.catalog.get(&ev.definition_id).map(|d| d.description.clone()).unwrap_or_default();
                        if let Some(tc) = ps.field.iter_mut().find(|c| c.instance_id == tid) {
                            let has_no_facedown = !tc.set_cards.iter().any(|s| !s.face_up);
                            let is_soccer = self
                                .catalog
                                .get(&tc.definition_id)
                                .map(|d| has_trait_match(d, "サッカー選手"))
                                .unwrap_or(false);
                            if set_desc.contains("裏向きのカードがセットされていない〚特徴［サッカー選手］〛のキャラにセットされたとき")
                                && is_soccer
                                && has_no_facedown
                            {
                                is_soccer_no_facedown = true;
                            }
                            tc.set_cards.push(ev);
                            self.log(format!("🔧 [{}] を [{}] にセットしました！", actor_name, target_name));
                        } else {
                            // フォールバックで戻す
                            ev.zone = Zone::REMOVE;
                            ps.remove.push(ev);
                        }
                    }
                }
                if is_soccer_no_facedown {
                    if let Some(ps) = self.players.get_mut(player_id) {
                        if let Some(mut top) = ps.deck.pop() {
                            top.zone = Zone::FIELD;
                            top.face_up = false;
                            if let Some(tc) = ps.field.iter_mut().find(|c| c.instance_id == tid) {
                                tc.set_cards.push(top);
                            } else {
                                top.zone = Zone::DECK;
                                ps.deck.push(top);
                            }
                        }
                    }
                    self.draw_cards(player_id, 1);
                    self.log(format!("⚽ [{}] の誘発効果で山札の上から1枚裏向きでセットし、カードを1枚引きました！", actor_name));
                }
            }
            "SET_DECK_TOP_FACE_DOWN" => {
                let n = opts.get("setDeckCount").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
                let mut attached = 0;
                if let Some(ps) = self.players.get_mut(player_id) {
                    let mut pulled = Vec::new();
                    for _ in 0..n {
                        if let Some(mut top) = ps.deck.pop() {
                            top.zone = Zone::FIELD;
                            top.face_up = false;
                            pulled.push(top);
                        }
                    }
                    if let Some(tc) = ps.field.iter_mut().find(|c| c.instance_id == tid) {
                        attached = pulled.len();
                        tc.set_cards.extend(pulled);
                    } else {
                        for mut c in pulled.into_iter().rev() {
                            c.zone = Zone::DECK;
                            ps.deck.push(c);
                        }
                    }
                }
                // 山札からキャラの下へセット（裏向き）して0枚になった瞬間に即座へリフレッシュ
                if attached > 0 {
                    self.refresh_if_deck_emptied(player_id);
                }
                if attached > 0 {
                    self.log(format!("🔧 [{}] の効果で [{}] に山札の上から{}枚裏向きでセットしました", actor_name, target_name, attached));
                }
            }
            "STACK_UNDER_SELF" => {
                let mut stacked_info: Option<(String, usize)> = None;
                if let Some(ps) = self.players.get_mut(player_id) {
                    if let Some(pos) = ps.remove.iter().position(|c| c.instance_id == tid) {
                        let mut card = ps.remove.remove(pos);
                        card.face_up = true;
                        let def_id = card.definition_id.clone();
                        if let Some(actor) = ps.field.iter_mut().find(|c| c.instance_id == actor_id) {
                            actor.set_cards.push(card);
                            if !actor.set_cards.is_empty() {
                                actor.has_rush = true;
                            }
                            stacked_info = Some((def_id, actor.set_cards.len()));
                        } else {
                            ps.remove.push(card);
                        }
                    }
                }
                if let Some((def_id, total)) = stacked_info {
                    let cname = self.card_name_by_def(&def_id);
                    self.log(format!("📚 [{}] の効果で [{}] を下に重ねました（合計{}枚）", actor_name, cname, total));
                }
            }
            "STUN_AND_GRANT_ON_REMOVE_DRAW" => {
                if let Some(ps) = self.players.get_mut(player_id) {
                    if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == tid) {
                        c.orientation = CardOrientation::STUN;
                        self.granted_on_remove_draw_until_opp_end
                            .insert(tid.to_string(), player_id.to_string());
                        self.log(format!(
                            "💫 [{}] の効果で [{}] をスタンさせ、「【現場リムーブ時】カードを1枚引く。」を与えました！",
                            actor_name, target_name
                        ));
                    }
                }
            }
            "REMOVE_SET_CARDS" => {
                for pid in [player_id, opp_id.as_str()] {
                    let mut removed_sc: Option<(String, String)> = None;
                    if let Some(ps) = self.players.get_mut(pid) {
                        for fc in &mut ps.field {
                            if let Some(spos) = fc.set_cards.iter().position(|sc| sc.instance_id == tid) {
                                let mut sc = fc.set_cards.remove(spos);
                                sc.zone = Zone::REMOVE;
                                sc.face_up = true;
                                removed_sc = Some((sc.instance_id.clone(), sc.definition_id.clone()));
                                ps.remove.push(sc);
                                break;
                            }
                        }
                    }
                    if let Some((sid, sdid)) = removed_sc {
                        self.log(format!("💥 [{}] の効果でセットされていた [{}] をリムーブしました", actor_name, target_name));
                        self.trigger_set_card_on_remove(pid, &sid, &sdid);
                        break;
                    }
                }
            }
            "FLIP_EVIDENCE_FACE_UP" => {
                if idx_in_selection == 0 {
                    self.last_cost_flipped_evidence_count = 0;
                }
                for pid in [opp_id.as_str(), player_id] {
                    if let Some(ps) = self.players.get_mut(pid) {
                        if let Some(ev) = ps.evidence.iter_mut().find(|e| e.instance_id == tid) {
                            ev.face_up = true;
                            self.last_cost_flipped_evidence_count += 1;
                            self.log(format!("🔓 [{}] の効果で証拠 [{}] を表向きにしました", actor_name, target_name));
                            break;
                        }
                    }
                }
            }
            "FLIP_EVIDENCE_FACE_DOWN" => {
                for pid in [player_id, opp_id.as_str()] {
                    if let Some(ps) = self.players.get_mut(pid) {
                        if let Some(ev) = ps.evidence.iter_mut().find(|e| e.instance_id == tid) {
                            ev.face_up = false;
                            self.log(format!("🔒 [{}] の効果で証拠を表向きの状態から裏向きにしました", actor_name));
                            break;
                        }
                    }
                }
            }
            "MOVE_EVIDENCE_TO_HAND" => {
                if let Some(ps) = self.players.get_mut(player_id) {
                    if let Some(pos) = ps.evidence.iter().position(|e| e.instance_id == tid) {
                        let mut ev = ps.evidence.remove(pos);
                        ev.zone = Zone::HAND;
                        ev.face_up = true;
                        ps.hand.push(ev);
                        self.last_chosen_added_to_hand = true;
                        self.log(format!("🤲 [{}] の効果で証拠 [{}] を手札に加えました", actor_name, target_name));
                    }
                }
            }
            "PLACE_HAND_AS_EVIDENCE" => {
                if let Some(ps) = self.players.get_mut(player_id) {
                    if let Some(pos) = ps.hand.iter().position(|c| c.instance_id == tid) {
                        let mut card = ps.hand.remove(pos);
                        card.zone = Zone::EVIDENCE;
                        card.face_up = false;
                        ps.evidence.push(card);
                        self.log(format!("🕵️ [{}] の効果で手札から1枚を裏向きで証拠として置きました", actor_name));
                    }
                }
            }
            "PLAY_EVENT_FROM_HAND" => {
                let mut ev_info: Option<(String, String)> = None;
                if let Some(ps) = self.players.get_mut(player_id) {
                    if let Some(pos) = ps.hand.iter().position(|c| c.instance_id == tid) {
                        let mut card = ps.hand.remove(pos);
                        ev_info = Some((card.instance_id.clone(), card.definition_id.clone()));
                        card.zone = Zone::REMOVE;
                        card.face_up = true;
                        ps.remove.push(card);
                    }
                }
                if let Some((eid, edid)) = ev_info {
                    let ename = self.card_name_by_def(&edid);
                    self.log(format!("📜 [{}] の効果で手札からイベント [{}] を使用！", actor_name, ename));
                    self.played_by_effect_events = true;
                    let resume_ctx = opts.get("resumeContext").and_then(|v| v.as_str()).unwrap_or("MAIN");
                    if let Some(edef) = self.catalog.get(&edid).cloned() {
                        if let Some(mc) = parse_event_mode_choice(&edef.description) {
                            let _ = self.start_event_mode_choice(player_id, &eid, &ename, Some(edef.level), &mc, resume_ctx);
                            return;
                        }
                        let clauses = extract_clauses(&edef.description, TriggerKind::OnPlay, edef.card_type);
                        for ecl in clauses.into_iter().rev() {
                            self.pending_clause_queue.insert(
                                0,
                                PendingClauseTask {
                                    player_id: player_id.to_string(),
                                    actor_id: eid.clone(),
                                    actor_name: ename.clone(),
                                    source_level: Some(edef.level),
                                    card_type: edef.card_type,
                                    clause: ecl,
                                    resume_context: resume_ctx.to_string(),
                                    evidence_source_inst_id: None,
                                    post_target_ops: vec![],
                                    plan: None,
                                },
                            );
                        }
                    }
                }
            }
            "COPY_NAME_FROM_OTHER_CHAR" => {
                // prev_chosen で選ばれていた対象（またはアクター自身）の temp_name_override をコピー元の名前に書き換え
                let source_name = self.card_name_by_inst(tid);
                let host_id = actor_id.to_string();
                if let Some(ps) = self.players.get_mut(player_id) {
                    if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == host_id) {
                        c.temp_name_override = Some(source_name.clone());
                        self.log(format!("🎭 [{}] のカード名を「{}」に書き換えました！", actor_name, source_name));
                    }
                }
            }
            "TURN_INTO_FACE_UP_EVIDENCE" => {
                let mut moved = false;
                for pid in [opp_id.as_str(), player_id] {
                    if let Some(ps) = self.players.get_mut(pid) {
                        if let Some(pos) = ps.field.iter().position(|c| c.instance_id == tid) {
                            let mut card = ps.field.remove(pos);
                            card.zone = Zone::EVIDENCE;
                            card.face_up = true;
                            card.orientation = CardOrientation::ACTIVE;
                            ps.evidence.push(card);
                            self.log(format!("📌 [{}] の効果で [{}] を持ち主の表向き証拠にしました", actor_name, target_name));
                            moved = true;
                            break;
                        }
                        for fc in &mut ps.field {
                            if let Some(spos) = fc.set_cards.iter().position(|sc| sc.instance_id == tid) {
                                let mut sc = fc.set_cards.remove(spos);
                                sc.zone = Zone::EVIDENCE;
                                sc.face_up = true;
                                ps.evidence.push(sc);
                                self.log(format!("📌 [{}] の効果でセットカード [{}] を持ち主の表向き証拠にしました", actor_name, target_name));
                                moved = true;
                                break;
                            }
                        }
                        if moved {
                            break;
                        }
                    }
                }
            }
            "GRANT_CONTACT_FORCE_ATTACK" => {
                let can_contact = self.players.get(player_id).map(|ps| ps.field.iter().any(|c| c.instance_id == actor_id)).unwrap_or(false)
                    && self.players.get(&opp_id).map(|ps| ps.field.iter().any(|c| c.instance_id == tid)).unwrap_or(false);
                if can_contact {
                    self.log(format!("⚔️ [{}] の効果で [{}] とのコンタクト（バトル）が発生！", actor_name, target_name));
                    self.contact_ap_buffs.clear();
                self.action_context = Some(json!({
                        "attackerPlayerId": player_id,
                        "defenderPlayerId": opp_id,
                        "actorId": actor_id,
                        "targetId": tid,
                        "attackerPass": false,
                        "defenderPass": false,
                        "cutinUsed": false,
                        "isForcedContact": true
                    }));
                    self.trigger_on_contact(player_id, actor_id, &opp_id, tid);
                    self.proceed_to_action_resolution(None);
                }
            }
            _ => {
                self.log(format!("✨ [{}] の効果が [{}] に適用されました", actor_name, target_name));
            }
        }
    }

    pub(super) fn apply_target_bonuses(
        &mut self,
        player_id: &str,
        actor_id: &str,
        actor_name: &str,
        bonuses: &[TargetBonus],
        selected_ids: &[String],
        resume_context: &str,
    ) -> bool {
        if bonuses.is_empty() || selected_ids.is_empty() {
            return false;
        }
        let opp_id = self.opponent_id_of(player_id);
        for bonus in bonuses {
            match bonus {
                TargetBonus::ApBuff(v) => {
                    for tid in selected_ids {
                        let tname = self.card_name_by_inst(tid);
                        for pid in [player_id, opp_id.as_str()] {
                            if let Some(ps) = self.players.get_mut(pid) {
                                if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == *tid) {
                                    c.temp_ap_bonus += *v;
                                    self.log(format!("✨ [{}] の追加効果で [{}] のAP +{}", actor_name, tname, v));
                                    break;
                                }
                            }
                        }
                    }
                }
                TargetBonus::GiveRush(rt) => {
                    for tid in selected_ids {
                        let tname = self.card_name_by_inst(tid);
                        if let Some(ps) = self.players.get_mut(player_id) {
                            if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == *tid) {
                                c.has_rush = true;
                                c.rush_target = rt.clone();
                                self.log(format!("⚡ [{}] の追加効果で [{}] に〚突撃〛を付与！", actor_name, tname));
                            }
                        }
                    }
                }
                TargetBonus::GiveCanAttackActive => {
                    for tid in selected_ids {
                        if let Some(ps) = self.players.get_mut(player_id) {
                            if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == *tid) {
                                c.can_attack_active = true;
                            }
                        }
                    }
                }
                TargetBonus::GiveBullet => {
                    for tid in selected_ids {
                        if let Some(ps) = self.players.get_mut(player_id) {
                            if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == *tid) {
                                c.has_bullet = true;
                            }
                        }
                    }
                }
                TargetBonus::DrawOnActionThisTurn => {
                    for tid in selected_ids {
                        if let Some(ps) = self.players.get_mut(player_id) {
                            if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == *tid) {
                                c.draw_on_action_this_turn = true;
                            }
                        }
                    }
                }
                TargetBonus::ContactDrawAndAp1000ThisTurn => {
                    for tid in selected_ids {
                        if let Some(ps) = self.players.get_mut(player_id) {
                            if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == *tid) {
                                c.contact_draw_and_ap1000_this_turn = true;
                            }
                        }
                    }
                }
                TargetBonus::BulletOnCaseAttackDiscardThisTurn => {
                    for tid in selected_ids {
                        if let Some(ps) = self.players.get_mut(player_id) {
                            if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == *tid) {
                                c.bullet_on_case_attack_discard_this_turn = true;
                            }
                        }
                    }
                }
                TargetBonus::SetDeckTopFaceDown(n) => {
                    for tid in selected_ids {
                        let fake_opts = json!({ "setDeckCount": n });
                        let mut dummy = vec![];
                        self.apply_single_target_action(player_id, actor_id, actor_name, "SET_DECK_TOP_FACE_DOWN", "FIELD", tid, 0, &fake_opts, &mut dummy);
                    }
                }
                TargetBonus::SetThisEvent => {
                    if let Some(tid) = selected_ids.first() {
                        let mut dummy = vec![];
                        self.apply_single_target_action(player_id, actor_id, actor_name, "SET_THIS_EVENT", "FIELD", tid, 0, &json!({}), &mut dummy);
                    }
                }
                TargetBonus::ChoiceRushOrAttackActive => {
                    if let Some(tid) = selected_ids.first() {
                        let dec_id = self.generate_decision_id();
                        self.pending_decision = Some(json!({
                            "decisionId": dec_id,
                            "revision": self.revision,
                            "playerId": player_id,
                            "kind": "modeSelect",
                            "options": {
                                "specialKind": "CHOICE_RUSH_OR_ACTIVE",
                                "actorId": actor_id,
                                "actorName": actor_name,
                                "targetId": tid,
                                "minPick": 1,
                                "maxPick": 1,
                                "modes": [
                                    "ターン終了時まで〚突撃［キャラ］〛を与える",
                                    "ターン終了時まで相手のアクティブ状態のキャラを指定してアクションできる能力を与える"
                                ],
                                "resumeContext": resume_context,
                                "title": format!("[{}] 与える能力を選択してください", actor_name)
                            }
                        }));
                        return true;
                    }
                }
                TargetBonus::GrantContactForceAttackThisTurn | TargetBonus::BonusCopyOtherCharName | TargetBonus::IfSoccerNoFaceDownSetTopAndDraw => {}
            }
        }
        false
    }

    pub(super) fn build_filtered_candidates(
        &self,
        player_id: &str,
        actor_id: Option<&str>,
        filter: &TargetFilter,
    ) -> Vec<Value> {
        let self_ps = self.players.get(player_id);
        let self_file_count = self_ps.map(|p| p.effective_file_count() as i32);
        let opp_id = self.opponent_id_of(player_id);
        let opp_cond = self.build_cond_info(&opp_id, None, CardOrientation::ACTIVE, None);
        let acting_card_id = self
            .action_context
            .as_ref()
            .and_then(|c| c.get("actorId").and_then(|v| v.as_str()));
        let self_max_lp = self_ps
            .map(|p| p.field.iter().map(|c| self.get_effective_lp(player_id, c)).max().unwrap_or(0))
            .unwrap_or(0);

        let mut candidates = Vec::new();
        let owners: Vec<(String, bool)> = match filter.owner {
            OwnerScope::SelfOnly => vec![(player_id.to_string(), false)],
            OwnerScope::OppOnly => vec![(opp_id.clone(), true)],
            OwnerScope::Both => vec![(opp_id.clone(), true), (player_id.to_string(), false)],
        };

        for (oid, is_opp) in owners {
            if oid.is_empty() {
                continue;
            }
            let Some(ps) = self.players.get(&oid) else {
                continue;
            };
            let set_card_refs: Vec<&CardInstance> = if filter.zone == TargetZone::SetCards {
                ps.field.iter().flat_map(|fc| fc.set_cards.iter()).collect()
            } else {
                vec![]
            };
            let (cards_iter, zone_label): (Vec<&CardInstance>, Option<&str>) = match filter.zone {
                TargetZone::Field => (ps.field.iter().collect(), None),
                TargetZone::Remove => (ps.remove.iter().collect(), Some("リムーブ")),
                TargetZone::Hand => (ps.hand.iter().collect(), Some("手札")),
                TargetZone::Deck => (ps.deck.iter().collect(), None),
                TargetZone::Evidence => (ps.evidence.iter().collect(), Some("証拠")),
                TargetZone::SetCards => (set_card_refs, Some("セット")),
            };
            for c in cards_iter {
                if filter.target_prev_chosen {
                    if self.last_chosen_instance_id.as_deref() != Some(c.instance_id.as_str()) {
                        continue;
                    }
                }
                if filter.exclude_prev_chosen {
                    if self.last_chosen_instance_id.as_deref() == Some(c.instance_id.as_str()) {
                        continue;
                    }
                }
                if is_opp && filter.zone == TargetZone::Field && self.is_protected_from_opp_target(&oid, c) {
                    continue;
                }
                if filter.require_in_contact {
                    if filter.zone != TargetZone::Field || !self.is_char_in_current_contact(&oid, &c.instance_id) {
                        continue;
                    }
                }
                let def_opt = self.catalog.get(&c.definition_id);
                if let Some(def) = def_opt {
                    // ターン内・カード名BAN（B06103 ジン等）: 手札からの使用・登場候補から除外
                    if filter.zone == TargetZone::Hand && self.is_name_banned_this_turn(&oid, c, def) {
                        continue;
                    }
                    let eff_lv = def.level + c.temp_level_delta;
                    if filter.min_level_from_prev {
                        if let Some(plv) = self.last_chosen_level {
                            if eff_lv < plv {
                                continue;
                            }
                        }
                    }
                    if filter.max_level_from_prev {
                        if let Some(plv) = self.last_chosen_level {
                            if eff_lv > plv {
                                continue;
                            }
                        }
                    }
                    if filter.max_level_from_removed_sum && eff_lv > self.last_removed_level_sum {
                        continue;
                    }
                    if filter.same_name_as_prev {
                        if let Some(ref pname) = self.last_chosen_name {
                            if !has_instance_name_match(c, def, pname) {
                                continue;
                            }
                        }
                    }
                    if filter.max_lp_from_self_max && def.lp > self_max_lp {
                        continue;
                    }
                }
                let eff_ap = if filter.zone == TargetZone::Field {
                    self.get_effective_ap(&oid, c)
                } else {
                    def_opt.map(|d| d.ap).unwrap_or(0)
                };
                if card_matches_filter_with_ctx(
                    &self.catalog,
                    actor_id,
                    player_id,
                    &oid,
                    c,
                    eff_ap,
                    filter,
                    if is_opp { Some(&opp_cond) } else { None },
                    acting_card_id,
                    self_file_count,
                ) {
                    let level = def_opt.map(|d| d.level + c.temp_level_delta).unwrap_or(0);
                    let traits = def_opt.map(|d| d.traits.clone()).unwrap_or_default();
                    let colors = def_opt.map(|d| d.colors.clone()).unwrap_or_default();
                    let cname = c
                        .temp_name_override
                        .clone()
                        .unwrap_or_else(|| self.card_name_by_def(&c.definition_id));
                    candidates.push(json!({
                        "instanceId": c.instance_id,
                        "definitionId": c.definition_id,
                        "name": cname,
                        "level": level,
                        "ap": eff_ap,
                        "colors": colors,
                        "traits": traits,
                        "orientation": format!("{:?}", c.orientation),
                        "faceUp": c.face_up,
                        "isOpponent": is_opp,
                        "zoneLabel": zone_label,
                    }));
                }
            }
        }
        candidates
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn prompt_deck_search_step(
        &mut self,
        player_id: &str,
        actor_id: &str,
        actor_name: &str,
        source_level: Option<i32>,
        ds: &DeckSearchStep,
        follow_up_target: Option<&TargetStep>,
        resume_context: &str,
        after_hint_level: Option<i32>,
    ) -> bool {
        let self_file_count = self.players.get(player_id).map(|p| p.effective_file_count() as i32);
        let mut revealed = vec![];
        if let Some(ps) = self.players.get_mut(player_id) {
            let deck_len = ps.deck.len();
            let actual = ds.search_count.min(deck_len);
            for i in 0..actual {
                let idx = deck_len - 1 - i;
                let card = &mut ps.deck[idx];
                card.face_up = true;
                revealed.push(card.clone());
            }
        }
        if revealed.is_empty() {
            return false;
        }
        let mut candidates = Vec::new();
        for c in &revealed {
            let selectable = self
                .catalog
                .get(&c.definition_id)
                .map(|def| {
                    ds.filter
                        .branches
                        .iter()
                        .any(|b| branch_matches_with_ctx(def, def.ap, b, self_file_count))
                })
                .unwrap_or(false);
            candidates.push(json!({
                "instanceId": c.instance_id,
                "definitionId": c.definition_id,
                "name": self.card_name_by_def(&c.definition_id),
                "level": self.catalog.get(&c.definition_id).map(|d| d.level).unwrap_or(0),
                "ap": self.catalog.get(&c.definition_id).map(|d| d.ap).unwrap_or(0),
                "colors": self.catalog.get(&c.definition_id).map(|d| d.colors.clone()).unwrap_or_default(),
                "traits": self.catalog.get(&c.definition_id).map(|d| d.traits.clone()).unwrap_or_default(),
                "selectable": selectable,
            }));
        }
        let pick_action_str = match ds.pick_action {
            DeckPickAction::ToHand => "TO_HAND",
            DeckPickAction::PlayActive => "PLAY_ACTIVE",
            DeckPickAction::PlaySleep => "PLAY_SLEEP",
        };
        let title_action = match ds.pick_action {
            DeckPickAction::ToHand => "手札に加える",
            DeckPickAction::PlayActive => "現場に登場させる",
            DeckPickAction::PlaySleep => "スリープ状態で現場に登場させる",
        };
        let max_select = ds.pick_count.max(ds.filter.count).max(1);
        let rem_label = if ds.remainder_to_remove {
            "残りはリムーブエリアへ"
        } else if ds.shuffle_remainder {
            "残りをシャッフルしてデッキ下へ"
        } else {
            "残りをデッキ下へ"
        };
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
                "searchCount": ds.search_count,
                "maxSelect": max_select,
                "distinctColors": ds.filter.distinct_colors,
                "pickAction": pick_action_str,
                "postAddDiscard": ds.post_add_discard,
                "postOps": ds.post_ops,
                "remainderToRemove": ds.remainder_to_remove,
                "shuffleRemainder": ds.shuffle_remainder,
                "recordRemovedLevels": ds.record_removed_levels,
                "postClauses": ds.post_clauses,
                "followUpTarget": follow_up_target,
                "afterHintLevel": after_hint_level,
                "resumeContext": resume_context,
                "title": format!("山札の上から{}枚めくりました。条件に合うカードを{}枚まで選び{}（{}）", ds.search_count, max_select, title_action, rem_label),
                "revealedCards": candidates,
                "optional": true
            }
        }));
        self.log(format!("🔍 [{}] の効果で山札の上から {} 枚めくりました", actor_name, ds.search_count));
        true
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn prompt_hand_discard(
        &mut self,
        player_id: &str,
        actor_id: &str,
        actor_name: &str,
        source_level: Option<i32>,
        count: usize,
        filter: Option<&TargetFilter>,
        optional: bool,
        abort_on_skip: bool,
        follow_up_ops: &[ImmediateOp],
        follow_up_target: Option<&TargetStep>,
        follow_up_deck_search: Option<&DeckSearchStep>,
        pending_clause_plan: Option<&ParsedClausePlan>,
        resume_context: &str,
        after_hint_level: Option<i32>,
    ) -> bool {
        let fallback_filter = TargetFilter {
            owner: OwnerScope::SelfOnly,
            zone: TargetZone::Hand,
            count,
            ..Default::default()
        };
        let active_filter = filter.unwrap_or(&fallback_filter);
        let candidates = self.build_filtered_candidates(player_id, Some(actor_id), active_filter);
        if candidates.is_empty() {
            if !optional {
                self.log(format!("⚠️ [{}] の効果: 捨てる対象の手札がないためスキップされました", actor_name));
            }
            return false;
        }
        let max_select = count.min(candidates.len()).max(1);
        let title = if optional {
            format!("リムーブする手札を{}枚まで選んでください（任意）", max_select)
        } else {
            format!("リムーブする手札を{}枚選んでください", max_select)
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
                "sourceLevel": source_level,
                "effectType": "DISCARD_HAND",
                "targetZone": "HAND",
                "maxSelect": max_select,
                "apDelta": 0,
                "rushTarget": null,
                "followUpOps": follow_up_ops,
                "condFollowUp": null,
                "followUpTarget": follow_up_target,
                "followUpDeckSearch": follow_up_deck_search,
                "pendingClausePlan": pending_clause_plan,
                "afterHintLevel": after_hint_level,
                "resumeContext": resume_context,
                "title": title,
                "candidates": candidates,
                "optional": optional,
                "abortOnSkip": abort_on_skip
            }
        }));
        true
    }

    pub(super) fn prompt_target_step(
        &mut self,
        player_id: &str,
        actor_id: &str,
        actor_name: &str,
        source_level: Option<i32>,
        ts: &TargetStep,
        follow_up_target: Option<&TargetStep>,
        resume_context: &str,
        after_hint_level: Option<i32>,
    ) -> bool {
        self.prompt_target_step_with_plan(
            player_id,
            actor_id,
            actor_name,
            source_level,
            ts,
            follow_up_target,
            None,
            resume_context,
            after_hint_level,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn prompt_target_step_with_plan(
        &mut self,
        player_id: &str,
        actor_id: &str,
        actor_name: &str,
        source_level: Option<i32>,
        ts: &TargetStep,
        follow_up_target: Option<&TargetStep>,
        pending_clause_plan: Option<&ParsedClausePlan>,
        resume_context: &str,
        after_hint_level: Option<i32>,
    ) -> bool {
        let candidates = self.build_filtered_candidates(player_id, Some(actor_id), &ts.filter);
        if candidates.is_empty() {
            self.log(format!("⚠️ [{}] の効果: 条件を満たす有効な対象がいないためスキップされました", actor_name));
            return false;
        }
        let target_zone_str = match ts.filter.zone {
            TargetZone::Field => "FIELD",
            TargetZone::Remove => "REMOVE",
            TargetZone::Hand => "HAND",
            TargetZone::Deck => "DECK",
            TargetZone::Evidence => "EVIDENCE",
            TargetZone::SetCards => "SET_CARDS",
        };
        let ap_delta = match ts.action {
            TargetAction::ApBuff(v) => {
                if ts.scale_by_cost_flipped_evidence {
                    v * (self.last_cost_flipped_evidence_count.max(1) as i32)
                } else {
                    v
                }
            }
            TargetAction::ApDebuff(v) => {
                if ts.scale_by_cost_flipped_evidence {
                    -v * (self.last_cost_flipped_evidence_count.max(1) as i32)
                } else {
                    -v
                }
            }
            _ => 0,
        };
        let level_delta = match ts.action {
            TargetAction::LevelDelta(d) => d,
            _ => 0,
        };
        let set_deck_count = match ts.action {
            TargetAction::SetDeckTopFaceDown(n) => n,
            _ => 1,
        };
        let rush_target = match &ts.action {
            TargetAction::GiveRush(rt) => rt.clone(),
            _ => None,
        };
        // 「そのキャラを〜」の確定追撃は無駄な再確認モーダルを出さず自動適用する
        if ts.filter.target_prev_chosen && !ts.optional && candidates.len() == 1 {
            if let Some(tid) = candidates[0].get("instanceId").and_then(|v| v.as_str()) {
                let fake_opts = json!({
                    "apDelta": ap_delta,
                    "levelDelta": level_delta,
                    "setDeckCount": set_deck_count,
                    "rushTarget": rush_target,
                    "resumeContext": resume_context,
                });
                let mut deployed_dummy = vec![];
                self.apply_single_target_action(
                    player_id,
                    actor_id,
                    actor_name,
                    ts.action.as_effect_type(),
                    target_zone_str,
                    tid,
                    0,
                    &fake_opts,
                    &mut deployed_dummy,
                );
                if self.apply_target_bonuses(player_id, actor_id, actor_name, &ts.bonuses, &[tid.to_string()], resume_context) {
                    return true;
                }
                if !ts.follow_up.is_empty()
                    && self.execute_immediate_ops_with_continuation(
                        player_id,
                        actor_id,
                        actor_name,
                        &ts.follow_up,
                        None,
                        follow_up_target,
                        None,
                        None,
                        resume_context,
                        after_hint_level,
                    )
                {
                    return true;
                }
                if let Some(ft) = follow_up_target {
                    return self.prompt_target_step(
                        player_id,
                        actor_id,
                        actor_name,
                        source_level,
                        ft,
                        None,
                        resume_context,
                        after_hint_level,
                    );
                }
                return false;
            }
        }

        let title = target_action_prompt_title(&ts.action, &ts.filter);
        let mut max_select = if ts.filter.any_count {
            candidates.len().max(1)
        } else if let Some(ref tr) = ts.filter.count_from_self_trait {
            let tr_cnt = self
                .players
                .get(player_id)
                .map(|ps| {
                    ps.field
                        .iter()
                        .filter(|c| self.catalog.get(&c.definition_id).map(|d| has_trait_match(d, tr)).unwrap_or(false))
                        .count()
                })
                .unwrap_or(0);
            if tr_cnt == 0 {
                return false;
            }
            tr_cnt.min(candidates.len())
        } else {
            match ts.action {
                TargetAction::ReviveSplitGonin => 3usize.min(candidates.len()).max(1),
                TargetAction::ReviveSplitBakana => 2usize.min(candidates.len()).max(1),
                TargetAction::RemoveSetCards(n) => n.min(candidates.len()).max(1),
                TargetAction::StunAndGrantOnRemoveDraw => 1.min(candidates.len()),
                _ => ts.filter.count.max(1).min(candidates.len()),
            }
        };
        if max_select == 0 {
            max_select = 1;
        }
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
                "effectType": ts.action.as_effect_type(),
                "targetZone": target_zone_str,
                "maxSelect": max_select,
                "distinctNames": ts.filter.distinct_names,
                "distinctLevels": ts.filter.distinct_levels,
                "distinctColors": ts.filter.distinct_colors,
                "maxTotalLevel": ts.filter.max_total_level,
                "anyCount": ts.filter.any_count,
                "apDelta": ap_delta,
                "levelDelta": level_delta,
                "setDeckCount": set_deck_count,
                "rushTarget": rush_target,
                "bonuses": ts.bonuses,
                "followUpOps": ts.follow_up,
                "condFollowUp": ts.conditional_follow_up.as_ref().map(|(br, ops)| json!({"branch": br, "ops": ops})),
                "followUpTarget": follow_up_target,
                "pendingClausePlan": pending_clause_plan,
                "afterHintLevel": after_hint_level,
                "resumeContext": resume_context,
                "title": title,
                "candidates": candidates,
                "optional": true,
                "abortOnSkip": ts.then_abort_if_skipped
            }
        }));
        true
    }
}

pub(super) fn target_action_prompt_title(action: &TargetAction, filter: &TargetFilter) -> String {
    let prefix = match filter.zone {
        TargetZone::Remove => "リムーブエリアの",
        TargetZone::Hand => "手札の",
        TargetZone::Field => match filter.owner {
            OwnerScope::OppOnly => "相手の",
            OwnerScope::SelfOnly => "自分の",
            OwnerScope::Both => "",
        },
        TargetZone::Deck => "",
        TargetZone::Evidence => match filter.owner {
            OwnerScope::OppOnly => "相手の証拠",
            _ => "証拠",
        },
        TargetZone::SetCards => "セットされている",
    };
    let max_n = filter.count.max(1);
    match action {
        TargetAction::Sleep => format!("スリープさせる{}キャラを{}体選択してください", prefix, max_n),
        TargetAction::Stun => format!("スタンさせる{}キャラを{}体選択してください", prefix, max_n),
        TargetAction::Activate => format!("アクティブにする{}キャラを{}体選択してください", prefix, max_n),
        TargetAction::Remove => format!("リムーブする{}カードを{}枚選択してください", prefix, max_n),
        TargetAction::BottomDeck => format!("デッキの下に移す{}キャラを{}体選択してください", prefix, max_n),
        TargetAction::BounceToHand => format!("手札に戻す{}カードを{}枚選択してください", prefix, max_n),
        TargetAction::ApBuff(v) => format!("AP＋{}する{}キャラを{}体選択してください", v, prefix, max_n),
        TargetAction::ApDebuff(v) => format!("AP－{}する{}キャラを{}体選択してください", v, prefix, max_n),
        TargetAction::GiveRush(_) => format!("突撃/迅速を与える{}キャラを選択してください", prefix),
        TargetAction::GiveBullet => format!("〚ブレット〛を与える{}キャラを選択してください", prefix),
        TargetAction::GiveMustGuard => format!("必ずガードさせる{}キャラを選択してください", prefix),
        TargetAction::GiveCanAttackActive => format!("アクティブ状態攻撃権を与える{}キャラを選択してください", prefix),
        TargetAction::SetBaseApZero => format!("元のAPを0にする{}キャラを選択してください", prefix),
        TargetAction::LevelDelta(d) => format!("レベル{}する{}キャラを選択してください", d, prefix),
        TargetAction::ReviveActive => format!("リムーブエリアから登場させるカードを{}枚選択してください", max_n),
        TargetAction::ReviveSleep => format!("リムーブエリアからスリープ状態で登場させるカードを{}枚選択してください", max_n),
        TargetAction::ReviveSplitGonin => "選択順に【1枚目:登場(アクティブ)】【2枚目:スリープ登場】【3枚目:手札に加える】を3枚まで選択してください".to_string(),
        TargetAction::ReviveSplitBakana => "選択順に【1枚目:登場(アクティブ)】【2枚目:スリープ登場】を合計Lv10以下で2枚まで選択してください".to_string(),
        TargetAction::RecoverToHand => format!("リムーブエリアから手札に加えるカードを{}枚選択してください", max_n),
        TargetAction::PlayFromHand => format!("手札から登場させるキャラを{}枚選択してください", max_n),
        TargetAction::PlayEventFromHand => "手札から使用するイベントカードを選択してください".to_string(),
        TargetAction::DiscardHand => format!("リムーブする手札を{}枚選択してください", max_n),
        TargetAction::SetThisEvent => format!("このイベントをセットする{}キャラを選択してください", prefix),
        TargetAction::SetDeckTopFaceDown(n) => format!("デッキの上から{}枚裏向きでセットする{}キャラを選択してください", n, prefix),
        TargetAction::FlipEvidenceFaceUp => "表向きにする証拠を選択してください".to_string(),
        TargetAction::FlipEvidenceFaceDown => "裏向きにする証拠を選択してください".to_string(),
        TargetAction::MoveEvidenceToHand => "手札に加える証拠を選択してください".to_string(),
        TargetAction::PlaceHandCardAsEvidenceFaceDown => "裏向きで証拠として置く手札を選択してください".to_string(),
        TargetAction::TurnIntoOwnerFaceUpEvidence => "表向きの証拠として送るカードを選択してください".to_string(),
        TargetAction::DeckTopFromRemove => "デッキの上に移すリムーブエリアのカードを選択してください".to_string(),
        TargetAction::CopyNameFromOtherSelfChar => "名前をコピーする自分の現場の別キャラを選択してください".to_string(),
        TargetAction::GrantContactForceAttack => "コンタクト強制効果を与える相手キャラを選択してください".to_string(),
        TargetAction::GrantBulletOnCaseAttackDiscard => "事件攻撃時ブレット化能力を与える自分のキャラを選択してください".to_string(),
        TargetAction::RemoveSetCards(n) => format!("リムーブするセットカードを{}枚選択してください", n),
        TargetAction::StunAndGrantOnRemoveDraw => "スタンさせて「【現場リムーブ時】カードを1枚引く。」を与えるキャラを1枚選択してください".to_string(),
        TargetAction::StackUnderSelf => "下に重ねるカードをリムーブエリアから選択してください".to_string(),
    }
}
