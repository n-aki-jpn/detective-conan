use crate::catalog::CardCatalog;
use crate::effects::{
    branch_matches, card_matches_filter, check_clause_conditions, check_disguise_conditions,
    compute_passive_ap_bonus, extract_clauses, parse_branches_from_phrase, parse_clause_cost,
    parse_effect_steps, parse_immediate_ops, resolve_state_branch, DeckPickAction, ImmediateOp,
    OwnerScope, ParsedClausePlan, PlayerCondInfo, TargetAction, TargetFilter, TargetZone,
    TriggerKind,
};
use crate::models::{
    CardInstance, CardOrientation, CardType, CasePhase, DeckDefinition, MatchPhase, Zone,
};
use rand::seq::SliceRandom;
use rand::Rng;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct PlayerState {
    pub player_id: String,
    pub deck: Vec<CardInstance>,
    pub hand: Vec<CardInstance>,
    pub field: Vec<CardInstance>,
    pub partner: Option<CardInstance>,
    pub r#case: Option<CardInstance>,
    pub file: Vec<CardInstance>,
    pub evidence: Vec<CardInstance>,
    pub remove: Vec<CardInstance>,

    pub case_phase: CasePhase,
    pub is_first_player: bool,
    pub normal_hand_played_this_turn: bool,
    pub hint_played_this_turn: bool,
}

impl PlayerState {
    pub fn new(player_id: String) -> Self {
        Self {
            player_id,
            deck: vec![],
            hand: vec![],
            field: vec![],
            partner: None,
            r#case: None,
            file: vec![],
            evidence: vec![],
            remove: vec![],
            case_phase: CasePhase::INTRO,
            is_first_player: false,
            normal_hand_played_this_turn: false,
            hint_played_this_turn: false,
        }
    }

    pub fn effective_file_count(&self) -> usize {
        let mut count = self.file.len();
        if let Some(ref p) = self.partner {
            if p.zone == Zone::FILE {
                count += 1;
            }
        }
        count
    }
}

#[derive(Debug, Clone)]
pub struct MatchEngine {
    pub catalog: CardCatalog,
    pub players: HashMap<String, PlayerState>,
    pub player_order: Vec<String>,
    pub phase: MatchPhase,
    pub turn: i32,
    pub turn_player_id: String,
    pub revision: i32,
    pub next_instance_id: i32,
    pub next_decision_id: i32,
    pub pending_decision: Option<Value>,
    pub winner_id: Option<String>,
    pub finish_reason: Option<String>,
    pub action_context: Option<Value>,
    pub logs: Vec<String>,
}

fn normalize_color_set(raw_colors: &[String]) -> HashSet<String> {
    let mut set = HashSet::new();
    let valid_chars = ['青', '緑', '白', '赤', '黄', '黒'];
    for s in raw_colors {
        for ch in s.chars() {
            if valid_chars.contains(&ch) {
                set.insert(ch.to_string());
            }
        }
    }
    set
}

fn parse_draw_count(desc: &str) -> usize {
    if desc.contains("カードを2枚引く") || desc.contains("2枚引く") { return 2; }
    if desc.contains("カードを3枚引く") || desc.contains("3枚引く") { return 3; }
    if desc.contains("カードを1枚引く") || desc.contains("1枚引く") || desc.contains("カードを引く") { return 1; }
    0
}


impl MatchEngine {
    pub fn new(catalog: CardCatalog) -> Self {
        Self {
            catalog,
            players: HashMap::new(),
            player_order: vec![],
            phase: MatchPhase::WAITING,
            turn: 0,
            turn_player_id: String::new(),
            revision: 0,
            next_instance_id: 1,
            next_decision_id: 1,
            pending_decision: None,
            winner_id: None,
            finish_reason: None,
            action_context: None,
            logs: vec![],
        }
    }

    pub fn log(&mut self, msg: impl Into<String>) {
        let msg_str = msg.into();
        self.logs.push(msg_str);
        if self.logs.len() > 500 {
            self.logs.drain(0..100);
        }
    }

    pub fn card_name_by_def(&self, def_id: &str) -> String {
        self.catalog.get(def_id)
            .and_then(|d| d.names.first().cloned())
            .unwrap_or_else(|| def_id.to_string())
    }

    pub fn find_card_def_id(&self, instance_id: &str) -> Option<String> {
        for ps in self.players.values() {
            if let Some(ref p) = ps.partner {
                if p.instance_id == instance_id { return Some(p.definition_id.clone()); }
            }
            if let Some(ref c) = ps.r#case {
                if c.instance_id == instance_id { return Some(c.definition_id.clone()); }
            }
            for c in &ps.field {
                if c.instance_id == instance_id { return Some(c.definition_id.clone()); }
            }
            for c in &ps.hand {
                if c.instance_id == instance_id { return Some(c.definition_id.clone()); }
            }
            for c in &ps.file {
                if c.instance_id == instance_id { return Some(c.definition_id.clone()); }
            }
            for c in &ps.evidence {
                if c.instance_id == instance_id { return Some(c.definition_id.clone()); }
            }
            for c in &ps.remove {
                if c.instance_id == instance_id { return Some(c.definition_id.clone()); }
            }
        }
        None
    }

    pub fn card_name_by_inst(&self, inst_id: &str) -> String {
        if let Some(def_id) = self.find_card_def_id(inst_id) {
            self.card_name_by_def(&def_id)
        } else {
            inst_id.to_string()
        }
    }

    fn generate_instance_id(&mut self) -> String {
        let id = format!("inst_{}", self.next_instance_id);
        self.next_instance_id += 1;
        id
    }

    pub fn generate_decision_id(&mut self) -> String {
        let id = format!("dec_{}", self.next_decision_id);
        self.next_decision_id += 1;
        id
    }

    pub fn add_player(&mut self, player_id: &str, deck_def: &DeckDefinition) {
        let mut ps = PlayerState::new(player_id.to_string());

        let p_inst = CardInstance {
            instance_id: self.generate_instance_id(),
            definition_id: deck_def.partner_id.clone(),
            owner_id: player_id.to_string(),
            zone: Zone::PARTNER,
            orientation: CardOrientation::ACTIVE,
            entered_turn: -1,
            face_up: false,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
        };
        ps.partner = Some(p_inst);

        let c_inst = CardInstance {
            instance_id: self.generate_instance_id(),
            definition_id: deck_def.case_id.clone(),
            owner_id: player_id.to_string(),
            zone: Zone::CASE,
            orientation: CardOrientation::ACTIVE,
            entered_turn: -1,
            face_up: false,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
        };
        ps.r#case = Some(c_inst);

        for item in &deck_def.main_deck {
            for _ in 0..item.count {
                let inst = CardInstance {
                    instance_id: self.generate_instance_id(),
                    definition_id: item.definition_id.clone(),
                    owner_id: player_id.to_string(),
                    zone: Zone::DECK,
                    orientation: CardOrientation::ACTIVE,
                    entered_turn: -1,
                    face_up: false,
                    temp_ap_bonus: 0,
                    has_rush: false,
                    rush_target: None,
                    used_declare_this_turn: false,
                };
                ps.deck.push(inst);
            }
        }

        self.players.insert(player_id.to_string(), ps);
        self.player_order.push(player_id.to_string());
    }

    pub fn start_match(&mut self) {
        if self.player_order.len() != 2 {
            return;
        }

        let mut rng = rand::thread_rng();
        if rng.gen_bool(0.5) {
            self.player_order.swap(0, 1);
        }

        let p1_id = self.player_order[0].clone();
        let p2_id = self.player_order[1].clone();
        self.players.get_mut(&p1_id).unwrap().is_first_player = true;
        self.players.get_mut(&p2_id).unwrap().is_first_player = false;

        self.log(format!("⚔️ 対戦開始！ 先攻: {}, 後攻: {}", p1_id, p2_id));

        // Shuffle and deal 5 cards
        for ps in self.players.values_mut() {
            ps.deck.shuffle(&mut rng);
            for _ in 0..5 {
                if let Some(mut card) = ps.deck.pop() {
                    card.zone = Zone::HAND;
                    card.face_up = true;
                    ps.hand.push(card);
                }
            }
        }

        self.phase = MatchPhase::MULLIGAN_FIRST;
        self.prompt_mulligan(&p1_id);
    }

    fn prompt_mulligan(&mut self, player_id: &str) {
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
            _ => return Err(format!("Unknown decision kind: {}", kind)),
        }

        Ok(())
    }

    fn handle_mulligan_answer(&mut self, player_id: &str, val: &Value) {
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

    fn start_turn(&mut self, turn: i32, turn_player_id: &str) {
        self.turn = turn;
        self.turn_player_id = turn_player_id.to_string();

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

            // AUTO_ACTIVATE & Reset turn bonuses
            for c in &mut ps.field {
                c.temp_ap_bonus = 0;
                c.has_rush = false;
                c.rush_target = None;
                c.used_declare_this_turn = false;
                if c.orientation == CardOrientation::STUN {
                    c.orientation = CardOrientation::SLEEP;
                } else {
                    c.orientation = CardOrientation::ACTIVE;
                }
            }
            if let Some(ref mut p) = ps.partner {
                p.temp_ap_bonus = 0;
                p.has_rush = false;
                p.rush_target = None;
                p.used_declare_this_turn = false;
                if p.zone == Zone::PARTNER {
                    if p.orientation == CardOrientation::STUN {
                        p.orientation = CardOrientation::SLEEP;
                    } else {
                        p.orientation = CardOrientation::ACTIVE;
                    }
                }
            }
            if let Some(ref mut cs) = ps.r#case {
                cs.used_declare_this_turn = false;
            }

            is_first_player = ps.is_first_player;
        }

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

        self.check_resolution_transition(turn_player_id);
        if self.phase == MatchPhase::FINISHED {
            return;
        }

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
                    let mut removed_def = None;
                    if let Some(ps) = self.players.get_mut(player_id) {
                        if let Some(mut card) = ps.hand.pop() {
                            removed_def = Some(card.definition_id.clone());
                            card.zone = Zone::REMOVE;
                            ps.remove.push(card);
                        }
                    }
                    if let Some(rdid) = removed_def {
                        let rname = self.card_name_by_def(&rdid);
                        self.log(format!("📜 事件 [{}] の解決編移行時効果: {} は手札から [{}] をリムーブしました", case_name, player_id, rname));
                    }
                }
            }
        }
    }

    fn draw_cards(&mut self, player_id: &str, count: usize) {
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
        }
    }

    fn file_card_from_deck(&mut self, player_id: &str) {
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
        }
    }

    fn trigger_refresh(&mut self, player_id: &str) {
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

    pub fn prompt_main_action(&mut self, player_id: &str) {
        if self.phase == MatchPhase::FINISHED {
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

        // 2. Hand Play (キャラは1ターン1回・現場5枚未満、イベントはいつでも使用可能)
        {
            let can_play_char = !ps.normal_hand_played_this_turn && !ps.hint_played_this_turn && ps.field.len() < 5;
            let mut playable_cards = vec![];
            for c in &ps.hand {
                if let Some(c_def) = self.catalog.get(&c.definition_id) {
                    let card_colors = normalize_color_set(&c_def.colors);
                    let color_ok = card_colors.is_empty() || case_colors.is_empty() || card_colors.is_subset(&case_colors);
                    if c_def.level <= effective_file && color_ok {
                        if c_def.card_type == CardType::EVENT {
                            playable_cards.push(c.instance_id.clone());
                        } else if can_play_char {
                            playable_cards.push(c.instance_id.clone());
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
        if !ps.file.is_empty() {
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
            let can_act = c.orientation == CardOrientation::ACTIVE && (c.entered_turn != self.turn || c.has_rush);
            if can_act {
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
        for opp_c in &opp_ps.field {
            if opp_c.orientation == CardOrientation::SLEEP || opp_c.orientation == CardOrientation::STUN {
                char_target_ids.push(opp_c.instance_id.clone());
            }
        }
        let mut case_target_id: Option<String> = None;
        if !opp_ps.evidence.is_empty() {
            if let Some(ref opp_case) = opp_ps.r#case {
                case_target_id = Some(opp_case.instance_id.clone());
            }
        }

        let mut all_target_ids = char_target_ids.clone();
        if let Some(ref cid) = case_target_id {
            all_target_ids.push(cid.clone());
        }

        let mut action_actors = vec![];
        let mut actor_target_map: HashMap<String, Vec<String>> = HashMap::new();
        for c in &ps.field {
            let can_act = c.orientation == CardOrientation::ACTIVE && (c.entered_turn != self.turn || c.has_rush);
            if !can_act {
                continue;
            }
            let valid_targets = if c.entered_turn == self.turn && c.has_rush {
                match c.rush_target.as_deref() {
                    Some("CHAR") => char_target_ids.clone(),
                    Some("CASE") => case_target_id.iter().cloned().collect(),
                    _ => all_target_ids.clone(),
                }
            } else {
                all_target_ids.clone()
            };
            if !valid_targets.is_empty() {
                action_actors.push(c.instance_id.clone());
                actor_target_map.insert(c.instance_id.clone(), valid_targets);
            }
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
        let can_declare_clause = |actor_id: &str, actor_ori: CardOrientation, used_turn: bool, def: &crate::models::CardDefinition| -> bool {
            let clauses = extract_clauses(&def.description, TriggerKind::Declare, def.card_type);
            if clauses.is_empty() {
                return false;
            }
            let ctx = self.build_cond_info(player_id, Some(actor_id), actor_ori, None);
            clauses.iter().any(|clause| {
                if check_clause_conditions(clause, &ctx).is_err() {
                    return false;
                }
                if clause.contains("【ターン1】") && used_turn {
                    return false;
                }
                let cost = parse_clause_cost(clause);
                self.can_pay_clause_cost(ps, actor_id, &cost)
            })
        };

        let mut declare_actors = vec![];
        for c in &ps.field {
            if let Some(c_def) = self.catalog.get(&c.definition_id) {
                if can_declare_clause(&c.instance_id, c.orientation, c.used_declare_this_turn, c_def) {
                    declare_actors.push(json!({
                        "actorId": c.instance_id.clone(),
                        "name": self.card_name_by_def(&c.definition_id),
                        "desc": c_def.description.clone(),
                    }));
                }
            }
        }
        if let Some(ref p) = ps.partner {
            if p.zone == Zone::PARTNER {
                if let Some(p_def) = self.catalog.get(&p.definition_id) {
                    if can_declare_clause(&p.instance_id, p.orientation, p.used_declare_this_turn, p_def) {
                        declare_actors.push(json!({
                            "actorId": p.instance_id.clone(),
                            "name": self.card_name_by_def(&p.definition_id),
                            "desc": p_def.description.clone(),
                        }));
                    }
                }
            }
        }
        if let Some(ref cs) = ps.r#case {
            if let Some(c_def) = self.catalog.get(&cs.definition_id) {
                if can_declare_clause(&cs.instance_id, cs.orientation, cs.used_declare_this_turn, c_def) {
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
            let effective_file = ps.effective_file_count() as i32;
            let case_colors = case_def.map(|d| normalize_color_set(&d.colors)).unwrap_or_default();
            let cond_ctx = self.build_cond_info(player_id, None, CardOrientation::ACTIVE, None);
            let mut disguise_card_ids = vec![];
            for c in &ps.hand {
                if let Some(c_def) = self.catalog.get(&c.definition_id) {
                    if c_def.card_type == CardType::CHARACTER
                        && (c_def.description.contains("【変装】") || c_def.description.contains("【変装時】"))
                    {
                        let disguise_clauses = extract_clauses(&c_def.description, TriggerKind::DisguiseCond, c_def.card_type);
                        let cond_ok = disguise_clauses.is_empty()
                            || disguise_clauses.iter().any(|cl| check_disguise_conditions(cl, &cond_ctx).is_ok());
                        let card_colors = normalize_color_set(&c_def.colors);
                        let color_ok = card_colors.is_empty() || case_colors.is_empty() || card_colors.is_subset(&case_colors);
                        if cond_ok && c_def.level <= effective_file && color_ok {
                            disguise_card_ids.push(c.instance_id.clone());
                        }
                    }
                }
            }
            if !disguise_card_ids.is_empty() {
                let target_field_ids: Vec<String> = ps.field.iter().map(|c| c.instance_id.clone()).collect();
                actions.push(json!({
                    "type": "DISGUISE",
                    "cardIds": disguise_card_ids,
                    "targetIds": target_field_ids
                }));
            }
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

    fn handle_main_action(&mut self, player_id: &str, val: &Value) {
        let action_type = val.get("type").and_then(|v| v.as_str()).unwrap_or("");

        match action_type {
            "END_TURN" => {
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
                        } else {
                            card.zone = Zone::FIELD;
                            card.orientation = CardOrientation::ACTIVE;
                            card.entered_turn = turn;
                            ps.field.push(card);
                            ps.normal_hand_played_this_turn = true;
                        }
                    }
                }
                if let (Some(def_id), Some(inst_id)) = (played_def_id, played_inst_id) {
                    let card_name = self.card_name_by_def(&def_id);
                    let lvl = self.catalog.get(&def_id).map(|d| d.level).unwrap_or(0);
                    if is_event {
                        self.log(format!("📜 {} がイベント [{}] (Lv.{}) を使用しました", player_id, card_name, lvl));
                    } else {
                        self.log(format!("🃏 {} が [{}] (Lv.{}) を登場させました", player_id, card_name, lvl));
                    }
                    let need_target = self.resolve_on_play_effects(player_id, &inst_id, &def_id, is_event);
                    if need_target {
                        return;
                    }
                }
                self.prompt_main_action(player_id);
            }
            "HINT_TAKE" => {
                let mut playable_cards = vec![];
                if let Some(ps) = self.players.get_mut(player_id) {
                    if let Some(mut top_file) = ps.file.pop() {
                        top_file.zone = Zone::HAND;
                        top_file.face_up = true;
                        ps.hand.push(top_file);
                        ps.hint_played_this_turn = true;
                    }

                    let effective_file = ps.effective_file_count() as i32;
                    let case_def = ps.r#case.as_ref().and_then(|c| self.catalog.get(&c.definition_id));
                    let case_colors = case_def.map(|d| normalize_color_set(&d.colors)).unwrap_or_default();

                    for c in &ps.hand {
                        if let Some(c_def) = self.catalog.get(&c.definition_id) {
                            let card_colors = normalize_color_set(&c_def.colors);
                            let color_ok = card_colors.is_empty() || case_colors.is_empty() || card_colors.is_subset(&case_colors);
                            let slot_ok = c_def.card_type == CardType::EVENT || ps.field.len() < 5;
                            if c_def.level <= effective_file && color_ok && slot_ok {
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
                if self.phase == MatchPhase::FINISHED {
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
                let mut lp = 0;
                if let Some(ps) = self.players.get_mut(player_id) {
                    if let Some(ref mut p) = ps.partner {
                        if p.instance_id == actor_id {
                            p.orientation = CardOrientation::SLEEP;
                            if let Some(c_def) = self.catalog.get(&p.definition_id) {
                                lp = c_def.lp;
                            }
                        }
                    }
                    if lp == 0 {
                        if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == actor_id) {
                            c.orientation = CardOrientation::SLEEP;
                            if let Some(c_def) = self.catalog.get(&c.definition_id) {
                                lp = c_def.lp;
                            }
                        }
                    }
                }

                let actor_name = self.card_name_by_inst(&actor_id);
                if lp > 0 {
                    self.gain_evidence(player_id, lp as usize);
                    self.log(format!("🔎 {} の [{}] が 推理 を行いました（証拠 +{}）", player_id, actor_name, lp));
                } else {
                    self.log(format!("🔎 {} の [{}] が 推理 を行いました", player_id, actor_name));
                }
                self.prompt_main_action(player_id);
            }
            "ACTION" => {
                let actor_id = val.get("actorId").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let target_id = val.get("targetId").and_then(|v| v.as_str()).unwrap_or("").to_string();

                let opp_pid = self.player_order.iter().find(|p| *p != player_id).unwrap().clone();

                // 突撃の攻撃対象制限チェック
                let rush_violation = {
                    let ps = self.players.get(player_id);
                    let opp_ps = self.players.get(&opp_pid);
                    if let (Some(ps), Some(opp_ps)) = (ps, opp_ps) {
                        if let Some(actor_c) = ps.field.iter().find(|c| c.instance_id == actor_id) {
                            if actor_c.entered_turn == self.turn && actor_c.has_rush {
                                let is_case_target = opp_ps.r#case.as_ref().map(|c| c.instance_id == target_id).unwrap_or(false);
                                match actor_c.rush_target.as_deref() {
                                    Some("CHAR") if is_case_target => Some("〚突撃［キャラ］〛は事件を攻撃できません"),
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

                // アクション時トリガーの解決
                self.trigger_on_action(player_id, &actor_id);

                self.action_context = Some(json!({
                    "actorId": actor_id,
                    "targetId": target_id,
                    "attackerPlayerId": player_id,
                    "defenderPlayerId": opp_pid
                }));

                self.prompt_guard_or_cutin(&opp_pid);
            }
            "DECLARE" => {
                let actor_id = val.get("actorId").and_then(|v| v.as_str()).unwrap_or("").to_string();
                self.handle_declare_action(player_id, &actor_id);
            }
            "DISGUISE" => {
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
                    self.log(format!("🎭 {} が [{}] と入れ替えて [{}] を【変装】登場させました！", player_id, old_name, new_name));
                    let need_ui = self.trigger_on_disguise(player_id, &inst_id, &def_id);
                    if need_ui {
                        return;
                    }
                }
                self.prompt_main_action(player_id);
            }
            _ => {
                self.prompt_main_action(player_id);
            }
        }
    }

    fn handle_declare_action(&mut self, player_id: &str, actor_id: &str) {
        let def_id_opt = self.players.get(player_id).and_then(|ps| {
            if let Some(ref p) = ps.partner {
                if p.instance_id == actor_id {
                    return Some(p.definition_id.clone());
                }
            }
            if let Some(ref cs) = ps.r#case {
                if cs.instance_id == actor_id {
                    return Some(cs.definition_id.clone());
                }
            }
            ps.field.iter().find(|c| c.instance_id == actor_id).map(|c| c.definition_id.clone())
        });

        let Some(def_id) = def_id_opt else {
            self.prompt_main_action(player_id);
            return;
        };
        let Some(def) = self.catalog.get(&def_id).cloned() else {
            self.prompt_main_action(player_id);
            return;
        };

        let card_name = self.card_name_by_def(&def_id);
        let ori = self.find_actor_orientation(player_id, actor_id);
        let clauses = extract_clauses(&def.description, TriggerKind::Declare, def.card_type);
        let ctx = self.build_cond_info(player_id, Some(actor_id), ori, None);

        // 実行可能な宣言句を選出（複数宣言がある場合は条件・コストを満たす最初の有効句を優先）
        let mut chosen_clause: Option<String> = None;
        for cl in &clauses {
            if check_clause_conditions(cl, &ctx).is_err() {
                continue;
            }
            let cost = parse_clause_cost(cl);
            let can_pay = self
                .players
                .get(player_id)
                .map(|ps| self.can_pay_clause_cost(ps, actor_id, &cost))
                .unwrap_or(false);
            if can_pay {
                chosen_clause = Some(cl.clone());
                break;
            }
        }

        let Some(clause) = chosen_clause else {
            self.log(format!("⚠️ [{}] の【宣言】条件またはコストを満たしていません", card_name));
            self.prompt_main_action(player_id);
            return;
        };

        let cost = parse_clause_cost(&clause);
        if !self.pay_clause_cost(player_id, actor_id, &cost) {
            self.prompt_main_action(player_id);
            return;
        }

        if let Some(ps) = self.players.get_mut(player_id) {
            if let Some(ref mut p) = ps.partner {
                if p.instance_id == actor_id {
                    p.used_declare_this_turn = true;
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
        }

        self.log(format!("⚡ {} が [{}] の【宣言】を発動！", player_id, card_name));
        let resolved_clause = resolve_state_branch(&clause, ori);
        let plan = parse_effect_steps(&resolved_clause, None, def.card_type);
        if self.execute_clause_plan(player_id, actor_id, &card_name, Some(def.level), &plan, "MAIN", None) {
            return;
        }
        self.prompt_main_action(player_id);
    }

    fn handle_target_select_answer(&mut self, player_id: &str, val: &Value, dec: &Value) {
        let target_id = val.get("targetId").and_then(|v| v.as_str());
        let opts = dec
            .get("options")
            .cloned()
            .unwrap_or_else(|| json!({}));

        let actor_id = opts
            .get("actorId")
            .and_then(|v| v.as_str())
            .or_else(|| val.get("actorId").and_then(|v| v.as_str()))
            .unwrap_or("");
        let effect_type = opts
            .get("effectType")
            .and_then(|v| v.as_str())
            .or_else(|| val.get("effectType").and_then(|v| v.as_str()))
            .unwrap_or("");
        let target_zone = opts.get("targetZone").and_then(|v| v.as_str()).unwrap_or("FIELD");
        let source_level = opts.get("sourceLevel").and_then(|v| v.as_i64()).map(|n| n as i32);
        let resume_context = opts
            .get("resumeContext")
            .and_then(|v| v.as_str())
            .unwrap_or("MAIN")
            .to_string();
        let after_hint_level = opts.get("afterHintLevel").and_then(|v| v.as_i64()).map(|n| n as i32);

        let actor_name = self.card_name_by_inst(actor_id);
        let valid_candidate_ids: HashSet<String> = opts
            .get("candidates")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|c| c.get("instanceId").and_then(|s| s.as_str()).map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let mut deployed_on_play: Option<(String, String)> = None;
        let mut chosen_def_for_cond: Option<String> = None;
        let mut applied = false;

        if let Some(tid) = target_id {
            if !valid_candidate_ids.is_empty() && !valid_candidate_ids.contains(tid) {
                self.log(format!("⚠️ 無効な対象 ({}) が選択されたため拒否しました", tid));
                // 再プロンプトせず現行フェーズに復帰
            } else {
                let target_name = self.card_name_by_inst(tid);
                let opp_id = self.opponent_id_of(player_id);
                applied = true;

                match effect_type {
                    "SLEEP" => {
                        for pid in [opp_id.as_str(), player_id] {
                            if let Some(ps) = self.players.get_mut(pid) {
                                if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == tid) {
                                    c.orientation = CardOrientation::SLEEP;
                                    self.log(format!("💤 [{}] の効果で [{}] をスリープさせました", actor_name, target_name));
                                    break;
                                }
                            }
                        }
                    }
                    "STUN" => {
                        for pid in [opp_id.as_str(), player_id] {
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
                        let mut removed_info: Option<(String, String)> = None;
                        for pid in [opp_id.clone(), player_id.to_string()] {
                            if let Some(ps) = self.players.get_mut(&pid) {
                                if let Some(idx) = ps.field.iter().position(|c| c.instance_id == tid) {
                                    let mut card = ps.field.remove(idx);
                                    let rdef = card.definition_id.clone();
                                    card.zone = Zone::REMOVE;
                                    ps.remove.push(card);
                                    removed_info = Some((pid, rdef));
                                    break;
                                }
                            }
                        }
                        if let Some((owner_pid, rem_def)) = removed_info {
                            self.log(format!("💥 [{}] の効果で [{}] をリムーブしました！", actor_name, target_name));
                            self.trigger_on_remove(&owner_pid, &rem_def);
                        }
                    }
                    "BOTTOM_DECK" => {
                        for pid in [opp_id.clone(), player_id.to_string()] {
                            if let Some(ps) = self.players.get_mut(&pid) {
                                if let Some(idx) = ps.field.iter().position(|c| c.instance_id == tid) {
                                    let mut card = ps.field.remove(idx);
                                    card.zone = Zone::DECK;
                                    card.face_up = false;
                                    ps.deck.insert(0, card);
                                    self.log(format!("📚 [{}] の効果で [{}] をデッキの下に移しました！", actor_name, target_name));
                                    break;
                                }
                            }
                        }
                    }
                    "ACTIVATE" => {
                        for pid in [player_id, opp_id.as_str()] {
                            if let Some(ps) = self.players.get_mut(pid) {
                                if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == tid) {
                                    c.orientation = CardOrientation::ACTIVE;
                                    self.log(format!("✨ [{}] の効果で [{}] をアクティブにしました！", actor_name, target_name));
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
                    "GIVE_RUSH" => {
                        let r_target = opts
                            .get("rushTarget")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
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
                    "REVIVE_ACTIVE" | "REVIVE_SLEEP" | "PLAY_FROM_HAND" => {
                        let ori = if effect_type == "REVIVE_SLEEP" {
                            CardOrientation::SLEEP
                        } else {
                            CardOrientation::ACTIVE
                        };
                        let turn = self.turn;
                        if let Some(ps) = self.players.get_mut(player_id) {
                            if ps.field.len() < 5 {
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
                                    ps.field.push(card);
                                    chosen_def_for_cond = Some(did.clone());
                                    deployed_on_play = Some((iid, did));
                                    let zone_txt = if effect_type == "PLAY_FROM_HAND" || target_zone == "HAND" {
                                        "手札"
                                    } else {
                                        "リムーブエリア"
                                    };
                                    self.log(format!("✨ [{}] の効果で{}から [{}] を登場させました！", actor_name, zone_txt, target_name));
                                }
                            }
                        }
                    }
                    "RECOVER_TO_HAND" => {
                        if let Some(ps) = self.players.get_mut(player_id) {
                            if let Some(idx) = ps.remove.iter().position(|c| c.instance_id == tid) {
                                let mut card = ps.remove.remove(idx);
                                chosen_def_for_cond = Some(card.definition_id.clone());
                                card.zone = Zone::HAND;
                                card.face_up = true;
                                ps.hand.push(card);
                                self.log(format!("🤲 [{}] の効果でリムーブエリアから [{}] を手札に加えました！", actor_name, target_name));
                            }
                        }
                    }
                    _ => {
                        self.log(format!("✨ [{}] の効果が [{}] に適用されました", actor_name, target_name));
                    }
                }
            }
        } else {
            self.log(format!("{} は [{}] の対象を選択しませんでした（スキップ）", player_id, actor_name));
        }

        // 条件付き追撃 & 後続即時処理（「〜リムーブし、カードを1枚引く」「〇〇を登場させた場合〜」）
        if applied {
            if let Ok(follow_ops) = serde_json::from_value::<Vec<ImmediateOp>>(
                opts.get("followUpOps").cloned().unwrap_or_else(|| json!([])),
            ) {
                if !follow_ops.is_empty() {
                    self.execute_immediate_ops(player_id, actor_id, &actor_name, &follow_ops, None);
                }
            }
            if let Some(cond_val) = opts.get("condFollowUp").filter(|v| !v.is_null()) {
                if let (Ok(branch), Ok(cops), Some(did)) = (
                    serde_json::from_value::<crate::effects::TargetBranch>(cond_val.get("branch").cloned().unwrap_or_default()),
                    serde_json::from_value::<Vec<ImmediateOp>>(cond_val.get("ops").cloned().unwrap_or_default()),
                    chosen_def_for_cond.as_ref(),
                ) {
                    if let Some(cdef) = self.catalog.get(did) {
                        if branch_matches(cdef, cdef.ap, &branch) {
                            self.execute_immediate_ops(player_id, actor_id, &actor_name, &cops, None);
                        }
                    }
                }
            }
        }

        // 登場させたキャラの登場時効果（チェーン発火）
        if let Some((dep_inst, dep_def)) = deployed_on_play {
            if self.resolve_on_play_effects_with_source(player_id, &dep_inst, &dep_def, false, source_level) {
                return;
            }
        }

        if let Some(hlv) = after_hint_level {
            if self.trigger_on_hint_use(player_id, hlv) {
                return;
            }
        }

        self.resume_after_decision(player_id, &resume_context);
    }

    /// デッキサーチ・めくり選択の意思決定を処理する
    fn handle_deck_search_answer(&mut self, player_id: &str, val: &Value, dec: &Value) {
        let opts = dec
            .get("options")
            .cloned()
            .unwrap_or_else(|| json!({}));

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

        let mut revealed_instances = vec![];
        let mut selectable_ids = HashSet::new();
        if let Some(cards) = opts.get("revealedCards").and_then(|v| v.as_array()) {
            for c in cards {
                if let Some(iid) = c.get("instanceId").and_then(|v| v.as_str()) {
                    revealed_instances.push(iid.to_string());
                    if c.get("selectable").and_then(|v| v.as_bool()).unwrap_or(true) {
                        selectable_ids.insert(iid.to_string());
                    }
                }
            }
        }

        // 条件外カードを選択した場合は無視
        let chosen_set: HashSet<String> = chosen_ids
            .into_iter()
            .filter(|id| selectable_ids.contains(id))
            .collect();

        let mut picked_defs: Vec<(String, String)> = vec![];
        let turn = self.turn;
        if let Some(ps) = self.players.get_mut(player_id) {
            let mut remaining_cards = vec![];
            for iid in &revealed_instances {
                if let Some(pos) = ps.deck.iter().position(|c| &c.instance_id == iid) {
                    let mut card = ps.deck.remove(pos);
                    if chosen_set.contains(iid) {
                        match pick_action {
                            "PLAY_ACTIVE" | "PLAY_SLEEP" => {
                                if ps.field.len() < 5 {
                                    card.zone = Zone::FIELD;
                                    card.orientation = if pick_action == "PLAY_SLEEP" {
                                        CardOrientation::SLEEP
                                    } else {
                                        CardOrientation::ACTIVE
                                    };
                                    card.entered_turn = turn;
                                    card.face_up = true;
                                    picked_defs.push((card.instance_id.clone(), card.definition_id.clone()));
                                    ps.field.push(card);
                                    continue;
                                }
                            }
                            _ => {
                                card.zone = Zone::HAND;
                                card.face_up = true;
                                picked_defs.push((card.instance_id.clone(), card.definition_id.clone()));
                                ps.hand.push(card);
                                continue;
                            }
                        }
                    }
                    card.face_up = false;
                    remaining_cards.push(card);
                }
            }
            // 残りのカードをデッキの下に移す
            for c in remaining_cards {
                ps.deck.insert(0, c);
            }
        }

        if !picked_defs.is_empty() {
            for (_, did) in &picked_defs {
                let cname = self.card_name_by_def(did);
                if pick_action == "PLAY_ACTIVE" || pick_action == "PLAY_SLEEP" {
                    self.log(format!("✨ [{}] の効果でデッキから [{}] を登場させました", actor_name, cname));
                } else {
                    self.log(format!("✨ [{}] の効果でデッキから [{}] を手札に加え、残りをデッキ下に移しました", actor_name, cname));
                }
            }
            if post_add_discard > 0 && pick_action == "TO_HAND" {
                self.execute_immediate_ops(
                    player_id,
                    actor_id,
                    &actor_name,
                    &[ImmediateOp::DiscardHand(post_add_discard)],
                    None,
                );
            }
        } else {
            self.log(format!("{} はカードを選びませんでした（残りをデッキ下へ）", player_id));
        }

        if let Ok(post_ops) = serde_json::from_value::<Vec<ImmediateOp>>(
            opts.get("postOps").cloned().unwrap_or_else(|| json!([])),
        ) {
            if !post_ops.is_empty() {
                self.execute_immediate_ops(player_id, actor_id, &actor_name, &post_ops, None);
            }
        }

        if pick_action == "PLAY_ACTIVE" || pick_action == "PLAY_SLEEP" {
            for (dep_inst, dep_def) in picked_defs {
                if self.resolve_on_play_effects_with_source(player_id, &dep_inst, &dep_def, false, source_level) {
                    return;
                }
            }
        }

        if let Some(hlv) = after_hint_level {
            if self.trigger_on_hint_use(player_id, hlv) {
                return;
            }
        }

        self.resume_after_decision(player_id, &resume_context);
    }

    fn resume_after_decision(&mut self, decision_player_id: &str, resume_context: &str) {
        if resume_context == "GUARD" {
            self.prompt_guard_or_cutin(decision_player_id);
        } else if resume_context == "END_TURN" {
            let next_turn = self.turn + 1;
            let cur_turn_pid = self.turn_player_id.clone();
            let next_pid = self.player_order.iter().find(|pid| *pid != &cur_turn_pid).cloned().unwrap_or(cur_turn_pid);
            self.start_turn(next_turn, &next_pid);
        } else if let Some(attacker_pid) = resume_context.strip_prefix("CASE_ATTACK:") {
            self.prompt_main_action(attacker_pid);
        } else if self.action_context.is_some() && self.turn_player_id != decision_player_id {
            self.prompt_guard_or_cutin(decision_player_id);
        } else {
            self.prompt_main_action(&self.turn_player_id.clone());
        }
    }

    /// ガードまたはカットインの意思決定をプロンプト
    fn prompt_guard_or_cutin(&mut self, defender_pid: &str) {
        let opp_ps = match self.players.get(defender_pid) {
            Some(p) => p,
            None => {
                self.proceed_to_action_resolution(None);
                return;
            }
        };

        // ガード可能カード（フィールドのACTIVE状態キャラ）
        let guards: Vec<String> = opp_ps.field.iter()
            .filter(|c| c.orientation == CardOrientation::ACTIVE)
            .map(|c| c.instance_id.clone())
            .collect();

        // カットイン可能カード（手札の【カットイン】持ちカードで発動条件を満たすもの）
        let cond_ctx = self.build_cond_info(defender_pid, None, CardOrientation::ACTIVE, None);
        let cutins: Vec<Value> = opp_ps.hand.iter()
            .filter_map(|c| {
                let def = self.catalog.get(&c.definition_id)?;
                let clauses = extract_clauses(&def.description, TriggerKind::Cutin, def.card_type);
                let any_ok = !clauses.is_empty()
                    && clauses.iter().any(|cl| check_clause_conditions(cl, &cond_ctx).is_ok());
                if any_ok {
                    Some(json!({
                        "instanceId": c.instance_id,
                        "definitionId": c.definition_id,
                        "name": self.card_name_by_def(&c.definition_id),
                        "desc": def.description.clone(),
                    }))
                } else {
                    None
                }
            })
            .collect();

        if guards.is_empty() && cutins.is_empty() {
            // ガードもカットインもない場合は即アクション解決
            self.proceed_to_action_resolution(None);
            return;
        }

        let dec_id = self.generate_decision_id();
        self.pending_decision = Some(json!({
            "decisionId": dec_id,
            "revision": self.revision,
            "playerId": defender_pid,
            "kind": "guard",
            "options": {
                "guardCardIds": guards,
                "cutinCardIds": cutins
            }
        }));
    }

    fn handle_hint_play_answer(&mut self, player_id: &str, val: &Value) {
        let mut played_def_id = None;
        let mut played_inst_id = None;
        let mut is_event = false;
        if let Some(card_id) = val.get("cardId").and_then(|v| v.as_str()) {
            let turn = self.turn;
            if let Some(ps) = self.players.get_mut(player_id) {
                if let Some(idx) = ps.hand.iter().position(|c| c.instance_id == card_id) {
                    let def_id_peek = ps.hand[idx].definition_id.clone();
                    let card_type = self.catalog.get(&def_id_peek).map(|d| d.card_type).unwrap_or(CardType::CHARACTER);
                    if card_type == CardType::EVENT || ps.field.len() < 5 {
                        let mut card = ps.hand.remove(idx);
                        played_def_id = Some(card.definition_id.clone());
                        played_inst_id = Some(card.instance_id.clone());
                        card.face_up = true;
                        if card_type == CardType::EVENT {
                            is_event = true;
                            card.zone = Zone::REMOVE;
                            ps.remove.push(card);
                        } else {
                            card.zone = Zone::FIELD;
                            card.orientation = CardOrientation::ACTIVE;
                            card.entered_turn = turn;
                            ps.field.push(card);
                        }
                    }
                }
            }
        }
        if let (Some(def_id), Some(inst_id)) = (played_def_id, played_inst_id) {
            let card_name = self.card_name_by_def(&def_id);
            let card_lv = self.catalog.get(&def_id).map(|d| d.level).unwrap_or(0);
            if is_event {
                self.log(format!("📜 {} がイベント [{}] を使用しました", player_id, card_name));
            } else {
                self.log(format!("🃏 {} が [{}] を登場させました", player_id, card_name));
            }
            let need_target = self.resolve_on_play_effects(player_id, &inst_id, &def_id, is_event);
            if need_target {
                if let Some(ref mut pd) = self.pending_decision {
                    if let Some(opts) = pd.get_mut("options").and_then(|v| v.as_object_mut()) {
                        opts.insert("afterHintLevel".to_string(), json!(card_lv));
                    }
                }
                return;
            }
            if self.trigger_on_hint_use(player_id, card_lv) {
                return;
            }
        } else {
            self.log(format!("{} はヒントからの登場をスキップしました", player_id));
        }
        self.prompt_main_action(player_id);
    }

    fn handle_guard_answer(&mut self, player_id: &str, val: &Value) {
        // カットイン使用かチェック
        if let Some(cutin_instance_id) = val.get("cutinCardId").and_then(|v| v.as_str()) {
            let cutin_instance_id = cutin_instance_id.to_string();
            let cutin_def_id = {
                let ps = self.players.get_mut(player_id).unwrap();
                if let Some(idx) = ps.hand.iter().position(|c| c.instance_id == cutin_instance_id) {
                    let mut card = ps.hand.remove(idx);
                    let def_id = card.definition_id.clone();
                    card.zone = Zone::REMOVE;
                    ps.remove.push(card);
                    Some(def_id)
                } else {
                    None
                }
            };

            if let Some(def_id) = cutin_def_id {
                let cutin_name = self.card_name_by_def(&def_id);
                self.log(format!("⚡ {} が [{}] の【カットイン】を発動！", player_id, cutin_name));
                let needs_target = self.resolve_cutin_effects(player_id, &cutin_instance_id, &def_id);
                if needs_target {
                    return;
                }
                // カットイン後も再度ガードできる
                let defender_pid = player_id.to_string();
                self.prompt_guard_or_cutin(&defender_pid);
                return;
            }
        }

        // 通常ガード処理
        let guard_id = val.get("guardCardId").and_then(|v| v.as_str()).map(|s| s.to_string());
        if let Some(ref gid) = guard_id {
            let guard_name = self.card_name_by_inst(gid);
            self.log(format!("🛡️ {} が [{}] でガードを宣言！", player_id, guard_name));
        } else {
            self.log(format!("{} はガードを行いませんでした（スルー）", player_id));
        }
        self.proceed_to_action_resolution(guard_id.as_deref());
    }

    fn handle_cutin_answer(&mut self, player_id: &str, val: &Value) {
        self.handle_guard_answer(player_id, val);
    }

    fn proceed_to_action_resolution(&mut self, guard_id: Option<&str>) {
        let ctx = match self.action_context.take() {
            Some(c) => c,
            None => return,
        };

        let attacker_pid = ctx["attackerPlayerId"].as_str().unwrap().to_string();
        let defender_pid = ctx["defenderPlayerId"].as_str().unwrap().to_string();
        let actor_id = ctx["actorId"].as_str().unwrap().to_string();
        let target_id = ctx["targetId"].as_str().unwrap().to_string();

        let is_case_action;
        let mut target_field_card_id = None;

        if let Some(gid) = guard_id {
            if let Some(defender_ps) = self.players.get_mut(&defender_pid) {
                if let Some(g_card) = defender_ps.field.iter_mut().find(|c| c.instance_id == gid) {
                    g_card.orientation = CardOrientation::SLEEP;
                }
            }
            is_case_action = false;
            target_field_card_id = Some(gid.to_string());
        } else {
            let defender_ps = self.players.get(&defender_pid).unwrap();
            if let Some(ref c) = defender_ps.r#case {
                if target_id == c.instance_id {
                    is_case_action = true;
                } else {
                    is_case_action = false;
                    target_field_card_id = Some(target_id.clone());
                }
            } else {
                is_case_action = false;
                target_field_card_id = Some(target_id.clone());
            }
        }

        if is_case_action {
            let has_evidence = self.players.get(&defender_pid).map(|p| !p.evidence.is_empty()).unwrap_or(false);
            if has_evidence {
                let resume_tag = format!("CASE_ATTACK:{}", attacker_pid);
                let (opened_hirameki_ui, block_gain) =
                    self.remove_evidence_and_trigger_hirameki(&defender_pid, 1, &resume_tag);
                if block_gain {
                    self.log(format!("💥 事件への攻撃ヒット！ {} の証拠を1枚削りました（ヒラメキにより {} は証拠を獲得できません）", defender_pid, attacker_pid));
                } else {
                    self.gain_evidence(&attacker_pid, 1);
                    self.log(format!("💥 事件への攻撃ヒット！ {} の証拠を1枚削り、{} が証拠を1つ獲得！", defender_pid, attacker_pid));
                }
                if opened_hirameki_ui {
                    return;
                }
            } else {
                self.log(format!("💥 事件への攻撃！ しかし {} の証拠はすでに0でした", defender_pid));
            }
        } else if let Some(def_cid) = target_field_card_id {
            // コンタクト時トリガーの解決（バトルAP比較前）
            self.trigger_on_contact(&attacker_pid, &actor_id, &defender_pid, &def_cid);

            let actor_def_id = self.players.get(&attacker_pid)
                .and_then(|p| p.field.iter().find(|c| c.instance_id == actor_id).map(|c| c.definition_id.clone()));
            let def_def_id = self.players.get(&defender_pid)
                .and_then(|p| p.field.iter().find(|c| c.instance_id == def_cid).map(|c| c.definition_id.clone()));

            let actor_ap = self.players.get(&attacker_pid)
                .and_then(|p| p.field.iter().find(|c| c.instance_id == actor_id))
                .map(|c| self.get_effective_ap(&attacker_pid, c))
                .unwrap_or(0);
            let def_ap = self.players.get(&defender_pid)
                .and_then(|p| p.field.iter().find(|c| c.instance_id == def_cid))
                .map(|c| self.get_effective_ap(&defender_pid, c))
                .unwrap_or(0);

            if let (Some(a_def_id), Some(d_def_id)) = (actor_def_id, def_def_id) {
                let actor_name = self.card_name_by_def(&a_def_id);
                let def_name = self.card_name_by_def(&d_def_id);

                if actor_ap >= def_ap {
                    let defender_ps = self.players.get_mut(&defender_pid).unwrap();
                    if let Some(idx) = defender_ps.field.iter().position(|c| c.instance_id == def_cid) {
                        let mut removed_c = defender_ps.field.remove(idx);
                        removed_c.zone = Zone::REMOVE;
                        defender_ps.remove.push(removed_c);
                    }
                    self.log(format!("💥 バトル決着: [{}] (AP{}) vs [{}] (AP{}) ➔ {} の [{}] をリムーブ！", actor_name, actor_ap, def_name, def_ap, defender_pid, def_name));
                    // 現場リムーブ時トリガーの解決
                    self.trigger_on_remove(&defender_pid, &d_def_id);
                    // コンタクト勝利時トリガーの解決
                    self.trigger_on_contact_win(&attacker_pid, &actor_id, &a_def_id);
                } else {
                    self.log(format!("🛡️ バトル決着: [{}] (AP{}) vs [{}] (AP{}) ➔ AP不足のためリムーブできず（守備成功）", actor_name, actor_ap, def_name, def_ap));
                }
            }
        }

        self.prompt_main_action(&attacker_pid);
    }

    /// 登場時効果またはイベントカード効果を解決する（能力元レベルを引数で受け取れる）
    fn resolve_on_play_effects(
        &mut self,
        player_id: &str,
        actor_id: &str,
        definition_id: &str,
        is_event: bool,
    ) -> bool {
        self.resolve_on_play_effects_with_source(player_id, actor_id, definition_id, is_event, None)
    }

    fn resolve_on_play_effects_with_source(
        &mut self,
        player_id: &str,
        actor_id: &str,
        definition_id: &str,
        is_event: bool,
        source_effect_level: Option<i32>,
    ) -> bool {
        let Some(def) = self.catalog.get(definition_id).cloned() else {
            return false;
        };
        if def.description.is_empty() {
            return false;
        }
        let card_name = self.card_name_by_def(definition_id);

        // パッシブの〚突撃〛（「【登場時】」や「【宣言】」と混同せず常時句のみ確認）
        if !is_event {
            let passive_clauses = extract_clauses(&def.description, TriggerKind::Passive, def.card_type);
            for clause in passive_clauses {
                let ori = self.find_actor_orientation(player_id, actor_id);
                let ctx = self.build_cond_info(player_id, Some(actor_id), ori, source_effect_level);
                if check_clause_conditions(&clause, &ctx).is_ok() {
                    let clean = crate::effects::strip_parens(&clause);
                    if !clean.contains("与え") && (clean.contains("このキャラは〚突撃") || clean.contains("このキャラは〚迅速")) {
                        let ops = parse_immediate_ops(&clean);
                        let rush_ops: Vec<ImmediateOp> = ops
                            .into_iter()
                            .filter(|op| matches!(op, ImmediateOp::SelfGiveRush(_)))
                            .collect();
                        if !rush_ops.is_empty() {
                            self.execute_immediate_ops(player_id, actor_id, &card_name, &rush_ops, None);
                        }
                    }
                }
            }
        }

        let clauses = extract_clauses(&def.description, TriggerKind::OnPlay, def.card_type);
        if clauses.is_empty() {
            return false;
        }

        for clause in clauses {
            let ori = self.find_actor_orientation(player_id, actor_id);
            let ctx = self.build_cond_info(player_id, Some(actor_id), ori, source_effect_level);
            if let Err(reason) = check_clause_conditions(&clause, &ctx) {
                self.log(format!("⚠️ [{}] の登場時効果不発: {}", card_name, reason));
                continue;
            }
            let cost = parse_clause_cost(&clause);
            if !self.pay_clause_cost(player_id, actor_id, &cost) {
                self.log(format!("⚠️ [{}] の登場時効果: コスト不足のため発動できませんでした", card_name));
                continue;
            }
            let resolved_clause = resolve_state_branch(&clause, ori);
            let plan = parse_effect_steps(&resolved_clause, None, def.card_type);
            let src_lv = Some(def.level);
            if self.execute_clause_plan(player_id, actor_id, &card_name, src_lv, &plan, "MAIN", None) {
                return true;
            }
        }
        false
    }

    /// カットイン効果を解決する（対象選択が必要な場合は true を返す）
    fn resolve_cutin_effects(&mut self, player_id: &str, cutin_instance_id: &str, definition_id: &str) -> bool {
        let Some(def) = self.catalog.get(definition_id).cloned() else {
            return false;
        };
        let clauses = extract_clauses(&def.description, TriggerKind::Cutin, def.card_type);
        if clauses.is_empty() {
            return false;
        }
        let card_name = self.card_name_by_def(definition_id);

        for clause in clauses {
            let ctx = self.build_cond_info(player_id, Some(cutin_instance_id), CardOrientation::ACTIVE, None);
            if let Err(reason) = check_clause_conditions(&clause, &ctx) {
                self.log(format!("⚠️ [{}] のカットイン条件未達: {}", card_name, reason));
                continue;
            }
            let cost = parse_clause_cost(&clause);
            if !self.pay_clause_cost(player_id, cutin_instance_id, &cost) {
                self.log(format!("⚠️ [{}] のカットインコスト不足", card_name));
                continue;
            }
            // 「自分の現場に空きがある場合、スリープ状態で登場させる」系カットイン
            let clean = crate::effects::strip_parens(&clause);
            if clean.contains("自分の現場に空きがある場合、スリープ状態で登場させる") {
                if let Some(ps) = self.players.get_mut(player_id) {
                    if ps.field.len() < 5 {
                        if let Some(idx) = ps.remove.iter().position(|c| c.instance_id == cutin_instance_id) {
                            let mut c = ps.remove.remove(idx);
                            c.zone = Zone::FIELD;
                            c.orientation = CardOrientation::SLEEP;
                            c.entered_turn = self.turn;
                            c.face_up = true;
                            ps.field.push(c);
                            self.log(format!("⚡ [{}] のカットイン効果で自身の現場にスリープ登場！", card_name));
                            continue;
                        }
                    }
                }
            }
            let plan = parse_effect_steps(&clause, None, def.card_type);
            if self.execute_clause_plan(player_id, cutin_instance_id, &card_name, Some(def.level), &plan, "GUARD", None) {
                return true;
            }
        }
        false
    }

    /// フィールド上のキャラの実効APを計算する（ベースAP + 一時バフ + 常時パッシブ）
    pub fn get_effective_ap(&self, owner_id: &str, card: &CardInstance) -> i32 {
        let base_ap = self.catalog.get(&card.definition_id).map(|d| d.ap).unwrap_or(0);
        let mut total = base_ap + card.temp_ap_bonus;

        if card.zone != Zone::FIELD {
            return total;
        }

        if let Some(def) = self.catalog.get(&card.definition_id) {
            let passive_clauses = extract_clauses(&def.description, TriggerKind::Passive, def.card_type);
            if !passive_clauses.is_empty() {
                let ctx = self.build_cond_info(owner_id, Some(&card.instance_id), card.orientation, None);
                for clause in passive_clauses {
                    total += compute_passive_ap_bonus(&clause, &ctx);
                }
            }
        }

        total
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

    /// 現場リムーブ時トリガーを解決する
    pub fn trigger_on_remove(&mut self, owner_id: &str, definition_id: &str) {
        let Some(def) = self.catalog.get(definition_id).cloned() else {
            return;
        };
        let clauses = extract_clauses(&def.description, TriggerKind::OnRemove, def.card_type);
        if clauses.is_empty() {
            return;
        }
        let card_name = self.card_name_by_def(definition_id);

        for clause in clauses {
            let ctx = self.build_cond_info(owner_id, None, CardOrientation::ACTIVE, None);
            if check_clause_conditions(&clause, &ctx).is_err() {
                continue;
            }
            self.log(format!("💥 [{}] の【現場リムーブ時】能力が誘発しました！", card_name));
            let cost = parse_clause_cost(&clause);
            if !self.pay_clause_cost(owner_id, "", &cost) {
                continue;
            }
            let plan = parse_effect_steps(&clause, None, def.card_type);
            let _ = self.execute_clause_plan(owner_id, "", &card_name, Some(def.level), &plan, "MAIN", None);
        }
    }

    /// ターン終了時トリガーを解決する
    pub fn trigger_on_turn_end(&mut self, current_turn_pid: &str) {
        // 「自分か相手のターン終了時」もあるため自分・相手両方を走査できる構成だが、
        // 主対象として現在ターンプレイヤーと相手の現場/パートナーを評価する
        let mut effects_to_trigger: Vec<(String, String, String, CardOrientation)> = vec![];
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
                        for clause in extract_clauses(&c_def.description, TriggerKind::OnTurnEnd, c_def.card_type) {
                            let is_opp_end = clause.contains("相手のターン終了時") || clause.contains("自分か相手のターン終了時");
                            let is_self_end = clause.contains("自分のターン終了時") || clause.contains("自分か相手のターン終了時") || (!clause.contains("相手の"));
                            if (pid == current_turn_pid && is_self_end) || (pid != current_turn_pid && is_opp_end) {
                                effects_to_trigger.push((pid.clone(), c.instance_id.clone(), c.definition_id.clone(), c.orientation));
                            }
                        }
                    }
                }
                if let Some(ref p) = ps.partner {
                    if let Some(p_def) = self.catalog.get(&p.definition_id) {
                        for clause in extract_clauses(&p_def.description, TriggerKind::OnTurnEnd, p_def.card_type) {
                            if pid == current_turn_pid || clause.contains("相手のターン終了時") || clause.contains("自分か相手のターン終了時") {
                                effects_to_trigger.push((pid.clone(), p.instance_id.clone(), p.definition_id.clone(), p.orientation));
                            }
                        }
                    }
                }
            }
        }

        for (pid, actor_id, def_id, ori) in effects_to_trigger {
            let Some(def) = self.catalog.get(&def_id).cloned() else {
                continue;
            };
            let card_name = self.card_name_by_def(&def_id);
            for clause in extract_clauses(&def.description, TriggerKind::OnTurnEnd, def.card_type) {
                let ctx = self.build_cond_info(&pid, Some(&actor_id), ori, None);
                if check_clause_conditions(&clause, &ctx).is_err() {
                    continue;
                }
                let resolved = resolve_state_branch(&clause, ori);
                let plan = parse_effect_steps(&resolved, None, def.card_type);
                if self.execute_clause_plan(&pid, &actor_id, &card_name, Some(def.level), &plan, "END_TURN", None) {
                    return;
                }
            }
        }
    }

    /// アクション宣言時トリガーを解決する（「このキャラがアクションしたとき」「〚...〛のキャラがアクションしたとき」）
    pub fn trigger_on_action(&mut self, player_id: &str, actor_id: &str) {
        let Some(actor_def_id) = self.find_card_def_id(actor_id) else {
            return;
        };
        let Some(actor_def) = self.catalog.get(&actor_def_id).cloned() else {
            return;
        };
        let actor_name = self.card_name_by_def(&actor_def_id);

        // 1. 攻撃キャラ自身の「このキャラがアクションしたとき」
        let actor_ori = self.find_actor_orientation(player_id, actor_id);
        for clause in extract_clauses(&actor_def.description, TriggerKind::OnAction, actor_def.card_type) {
            if !clause.contains("このキャラがアクションしたとき")
                && !clause.contains("このキャラが推理かアクションしたとき")
            {
                continue;
            }
            let ctx = self.build_cond_info(player_id, Some(actor_id), actor_ori, None);
            if check_clause_conditions(&clause, &ctx).is_err() {
                continue;
            }
            let plan = parse_effect_steps(&clause, None, actor_def.card_type);
            self.execute_immediate_ops(player_id, actor_id, &actor_name, &plan.immediate_before, None);
        }

        // 2. 自分の場のキャラの「〇〇がアクションしたとき」
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
            for clause in extract_clauses(&def.description, TriggerKind::OnAction, def.card_type) {
                if !clause.contains("キャラがアクションしたとき") {
                    continue;
                }
                let ctx = self.build_cond_info(player_id, Some(&watcher_id), ori, None);
                if check_clause_conditions(&clause, &ctx).is_err() {
                    continue;
                }
                if let Some(end_idx) = clause.find("がアクションしたとき") {
                    let trigger_subject = &clause[..end_idx];
                    let branches = parse_branches_from_phrase(trigger_subject, CardType::CHARACTER);
                    if !branches.is_empty() && !branches.iter().any(|b| branch_matches(&actor_def, actor_def.ap, b)) {
                        continue;
                    }
                }
                let plan = parse_effect_steps(&clause, None, def.card_type);
                for op in &plan.immediate_before {
                    match op {
                        ImmediateOp::SelfApBuff(amt) => {
                            if let Some(ps) = self.players.get_mut(player_id) {
                                if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == actor_id) {
                                    c.temp_ap_bonus += *amt;
                                }
                            }
                            self.log(format!("✨ [{}] の誘発効果（アクション一致）: [{}] のAP +{}", watcher_name, actor_name, amt));
                        }
                        other => {
                            self.execute_immediate_ops(player_id, &watcher_id, &watcher_name, &[other.clone()], None);
                        }
                    }
                }
            }
        }
    }

    /// コンタクト突入時トリガーを解決する
    pub fn trigger_on_contact(&mut self, attacker_pid: &str, actor_id: &str, defender_pid: &str, def_cid: &str) {
        // 攻撃側「このキャラがコンタクトしたとき」
        if let Some(a_def_id) = self.find_card_def_id(actor_id) {
            if let Some(def) = self.catalog.get(&a_def_id).cloned() {
                let ori = self.find_actor_orientation(attacker_pid, actor_id);
                let name = self.card_name_by_def(&a_def_id);
                for clause in extract_clauses(&def.description, TriggerKind::OnContact, def.card_type) {
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

        // 防御側「このキャラを指定してアクションしたとき」 / 「コンタクトされたとき」
        if let Some(d_def_id) = self.find_card_def_id(def_cid) {
            if let Some(def) = self.catalog.get(&d_def_id).cloned() {
                let ori = self.find_actor_orientation(defender_pid, def_cid);
                let name = self.card_name_by_def(&d_def_id);
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
    }

    /// コンタクトによる相手キャラリムーブ時のトリガーを解決する
    pub fn trigger_on_contact_win(&mut self, attacker_pid: &str, actor_id: &str, actor_def_id: &str) {
        let Some(def) = self.catalog.get(actor_def_id).cloned() else {
            return;
        };
        let clauses = extract_clauses(&def.description, TriggerKind::OnContactWin, def.card_type);
        if clauses.is_empty() {
            return;
        }
        let card_name = self.card_name_by_def(actor_def_id);
        let ori = self.find_actor_orientation(attacker_pid, actor_id);

        for clause in clauses {
            let ctx = self.build_cond_info(attacker_pid, Some(actor_id), ori, None);
            if check_clause_conditions(&clause, &ctx).is_err() {
                continue;
            }
            let cost = parse_clause_cost(&clause);
            if !self.pay_clause_cost(attacker_pid, actor_id, &cost) {
                continue;
            }
            let plan = parse_effect_steps(&clause, None, def.card_type);
            let _ = self.execute_clause_plan(attacker_pid, actor_id, &card_name, Some(def.level), &plan, "MAIN", None);
        }
    }

    /// 変装時トリガー（【変装時】および独立した【登場時】）を解決する
    pub fn trigger_on_disguise(&mut self, player_id: &str, actor_id: &str, definition_id: &str) -> bool {
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
            if self.execute_clause_plan(player_id, actor_id, &card_name, Some(def.level), &plan, "MAIN", None) {
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
            if self.execute_clause_plan(player_id, actor_id, &card_name, Some(def.level), &plan, "MAIN", None) {
                return true;
            }
        }

        false
    }

    /// ネクストヒントで手札を使用したときのトリガー（例: B01005）
    pub fn trigger_on_hint_use(&mut self, player_id: &str, hint_card_level: i32) -> bool {
        let actors: Vec<(String, String, CardOrientation)> = {
            let Some(ps) = self.players.get(player_id) else {
                return false;
            };
            let mut list = vec![];
            for c in &ps.field {
                list.push((c.instance_id.clone(), c.definition_id.clone(), c.orientation));
            }
            if let Some(ref p) = ps.partner {
                list.push((p.instance_id.clone(), p.definition_id.clone(), p.orientation));
            }
            list
        };

        for (actor_id, def_id, ori) in actors {
            let Some(def) = self.catalog.get(&def_id).cloned() else {
                continue;
            };
            let card_name = self.card_name_by_def(&def_id);
            for clause in extract_clauses(&def.description, TriggerKind::OnHintUse, def.card_type) {
                let ctx = self.build_cond_info(player_id, Some(&actor_id), ori, None);
                if check_clause_conditions(&clause, &ctx).is_err() {
                    continue;
                }
                if clause.contains("【ターン1】") {
                    let already_used = self
                        .players
                        .get(player_id)
                        .map(|ps| {
                            ps.partner.as_ref().map(|p| p.instance_id == actor_id && p.used_declare_this_turn).unwrap_or(false)
                                || ps.field.iter().any(|c| c.instance_id == actor_id && c.used_declare_this_turn)
                        })
                        .unwrap_or(false);
                    if already_used {
                        continue;
                    }
                    if let Some(ps) = self.players.get_mut(player_id) {
                        if let Some(ref mut p) = ps.partner {
                            if p.instance_id == actor_id {
                                p.used_declare_this_turn = true;
                            }
                        }
                        if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == actor_id) {
                            c.used_declare_this_turn = true;
                        }
                    }
                }
                let plan = parse_effect_steps(&clause, Some(hint_card_level), def.card_type);
                if self.execute_clause_plan(player_id, &actor_id, &card_name, Some(def.level), &plan, "MAIN", None) {
                    return true;
                }
            }
        }
        false
    }

    // ─── Unified Effect Execution Helpers ────────────────────────────────────

    fn opponent_id_of(&self, player_id: &str) -> String {
        self.player_order
            .iter()
            .find(|p| *p != player_id)
            .cloned()
            .unwrap_or_default()
    }

    fn find_actor_orientation(&self, player_id: &str, actor_id: &str) -> CardOrientation {
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

    fn build_cond_info<'a>(
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

        PlayerCondInfo {
            actor_id,
            actor_orientation,
            is_my_turn: self.turn_player_id == player_id,
            case_phase: ps.map(|p| p.case_phase).unwrap_or(CasePhase::INTRO),
            file_count: ps.map(|p| p.effective_file_count()).unwrap_or(0),
            self_evidence_count: ps.map(|p| p.evidence.len()).unwrap_or(0),
            opp_evidence_count: opp_ps.map(|p| p.evidence.len()).unwrap_or(0),
            opp_field_count: opp_ps.map(|p| p.field.len()).unwrap_or(0),
            case_level,
            partner_def,
            case_def,
            field_cards,
            played_by_effect_level,
        }
    }

    fn can_pay_clause_cost(&self, ps: &PlayerState, actor_id: &str, cost: &crate::effects::ClauseCost) -> bool {
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
        if cost.hand_remove > 0 && ps.hand.len() < cost.hand_remove {
            return false;
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
        true
    }

    fn pay_clause_cost(&mut self, player_id: &str, actor_id: &str, cost: &crate::effects::ClauseCost) -> bool {
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
            for _ in 0..cost.hand_remove {
                if let Some(mut c) = ps.hand.pop() {
                    c.zone = Zone::REMOVE;
                    removed_hand.push(c.definition_id.clone());
                    ps.remove.push(c);
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
            if cost.sacrifice_self && !actor_id.is_empty() {
                if let Some(idx) = ps.field.iter().position(|c| c.instance_id == actor_id) {
                    let mut c = ps.field.remove(idx);
                    c.zone = Zone::REMOVE;
                    ps.remove.push(c);
                }
            }
        }

        for _ in 0..cost.evidence_remove {
            let _ = self.remove_evidence_and_trigger_hirameki(player_id, 1, "MAIN");
        }
        for did in removed_hand {
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
            self.log(format!("💸 コスト支払い: {} はデッキの上から {} 枚をリムーブしました", player_id, milled.len()));
        }
        true
    }

    fn execute_immediate_ops(
        &mut self,
        player_id: &str,
        actor_id: &str,
        actor_name: &str,
        ops: &[ImmediateOp],
        evidence_source_inst_id: Option<&str>,
    ) {
        for op in ops {
            match op {
                ImmediateOp::Draw(n) => {
                    self.draw_cards(player_id, *n);
                    self.log(format!("✨ [{}] の効果でカードを {} 枚引きました", actor_name, n));
                }
                ImmediateOp::DiscardHand(n) => {
                    let mut removed = vec![];
                    if let Some(ps) = self.players.get_mut(player_id) {
                        for _ in 0..*n {
                            if let Some(mut c) = ps.hand.pop() {
                                c.zone = Zone::REMOVE;
                                removed.push(c.definition_id.clone());
                                ps.remove.push(c);
                            }
                        }
                    }
                    for did in removed {
                        let cname = self.card_name_by_def(&did);
                        self.log(format!("💸 [{}] の効果: {} の手札から [{}] をリムーブしました", actor_name, player_id, cname));
                    }
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
                        let _ = self.remove_evidence_and_trigger_hirameki(&opp_id, 1, "MAIN");
                    }
                }
                ImmediateOp::SelfRemoveEvidence { count, trigger_hirameki } => {
                    for _ in 0..*count {
                        if *trigger_hirameki {
                            let _ = self.remove_evidence_and_trigger_hirameki(player_id, 1, "MAIN");
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
                    if let Some(ps) = self.players.get_mut(player_id) {
                        if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == actor_id) {
                            c.temp_ap_bonus += *amt;
                            self.log(format!("✨ [{}] の効果で自身のAP +{}", actor_name, amt));
                        }
                    }
                }
                ImmediateOp::SelfActivate => {
                    if let Some(ps) = self.players.get_mut(player_id) {
                        if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == actor_id) {
                            c.orientation = CardOrientation::ACTIVE;
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
                }
                ImmediateOp::MoveSelfFromEvidenceToFieldSleep => {
                    if let Some(eid) = evidence_source_inst_id {
                        let turn = self.turn;
                        if let Some(ps) = self.players.get_mut(player_id) {
                            if ps.field.len() < 5 {
                                if let Some(pos) = ps.remove.iter().position(|c| c.instance_id == eid) {
                                    let mut card = ps.remove.remove(pos);
                                    card.zone = Zone::FIELD;
                                    card.orientation = CardOrientation::SLEEP;
                                    card.entered_turn = turn;
                                    card.face_up = true;
                                    ps.field.push(card);
                                    self.log(format!("💡 【ヒラメキ】[{}] が証拠から現場へスリープ状態で登場！", actor_name));
                                }
                            }
                        }
                    }
                }
                ImmediateOp::SolveCaseVictory => {
                    self.log(format!("🎉 [{}] の効果により {} が事件を解決し勝利しました！", actor_name, player_id));
                    self.finish_match(player_id, "CASE_RESOLVED");
                }
            }
        }
    }

    fn build_filtered_candidates(
        &self,
        player_id: &str,
        actor_id: Option<&str>,
        filter: &TargetFilter,
    ) -> Vec<Value> {
        let opp_id = self.opponent_id_of(player_id);
        let opp_cond = self.build_cond_info(&opp_id, None, CardOrientation::ACTIVE, None);
        let acting_card_id = self
            .action_context
            .as_ref()
            .and_then(|c| c.get("actorId").and_then(|v| v.as_str()));

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
            let (cards, zone_label): (&Vec<CardInstance>, Option<&str>) = match filter.zone {
                TargetZone::Field => (&ps.field, None),
                TargetZone::Remove => (&ps.remove, Some("リムーブ")),
                TargetZone::Hand => (&ps.hand, Some("手札")),
                TargetZone::Deck => (&ps.deck, None),
            };
            for c in cards {
                let eff_ap = if filter.zone == TargetZone::Field {
                    self.get_effective_ap(&oid, c)
                } else {
                    self.catalog.get(&c.definition_id).map(|d| d.ap).unwrap_or(0)
                };
                if card_matches_filter(
                    &self.catalog,
                    actor_id,
                    player_id,
                    &oid,
                    c,
                    eff_ap,
                    filter,
                    if is_opp { Some(&opp_cond) } else { None },
                    acting_card_id,
                ) {
                    let level = self.catalog.get(&c.definition_id).map(|d| d.level).unwrap_or(0);
                    let traits = self.catalog.get(&c.definition_id).map(|d| d.traits.clone()).unwrap_or_default();
                    candidates.push(json!({
                        "instanceId": c.instance_id,
                        "definitionId": c.definition_id,
                        "name": self.card_name_by_def(&c.definition_id),
                        "level": level,
                        "ap": eff_ap,
                        "traits": traits,
                        "orientation": format!("{:?}", c.orientation),
                        "isOpponent": is_opp,
                        "zoneLabel": zone_label,
                    }));
                }
            }
        }
        candidates
    }

    fn execute_clause_plan(
        &mut self,
        player_id: &str,
        actor_id: &str,
        actor_name: &str,
        source_level: Option<i32>,
        plan: &ParsedClausePlan,
        resume_context: &str,
        evidence_source_inst_id: Option<&str>,
    ) -> bool {
        if !plan.immediate_before.is_empty() {
            self.execute_immediate_ops(
                player_id,
                actor_id,
                actor_name,
                &plan.immediate_before,
                evidence_source_inst_id,
            );
            if self.phase == MatchPhase::FINISHED {
                return true;
            }
        }

        if let Some(ref ts) = plan.target_step {
            let candidates = self.build_filtered_candidates(player_id, Some(actor_id), &ts.filter);
            if candidates.is_empty() {
                self.log(format!("⚠️ [{}] の効果: 条件を満たす有効な対象がいないためスキップされました", actor_name));
                return false;
            }
            let title = target_action_prompt_title(&ts.action, &ts.filter);
            let ap_delta = match ts.action {
                TargetAction::ApBuff(v) => v,
                TargetAction::ApDebuff(v) => -v,
                _ => 0,
            };
            let rush_target = match &ts.action {
                TargetAction::GiveRush(rt) => rt.clone(),
                _ => None,
            };
            let target_zone_str = match ts.filter.zone {
                TargetZone::Field => "FIELD",
                TargetZone::Remove => "REMOVE",
                TargetZone::Hand => "HAND",
                TargetZone::Deck => "DECK",
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
                    "effectType": ts.action.as_effect_type(),
                    "targetZone": target_zone_str,
                    "apDelta": ap_delta,
                    "rushTarget": rush_target,
                    "followUpOps": ts.follow_up,
                    "condFollowUp": ts.conditional_follow_up.as_ref().map(|(br, ops)| json!({"branch": br, "ops": ops})),
                    "resumeContext": resume_context,
                    "title": title,
                    "candidates": candidates,
                    "optional": true
                }
            }));
            return true;
        }

        if let Some(ref ds) = plan.deck_search {
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
            if !revealed.is_empty() {
                let mut candidates = Vec::new();
                for c in &revealed {
                    let selectable = self
                        .catalog
                        .get(&c.definition_id)
                        .map(|def| ds.filter.branches.iter().any(|b| branch_matches(def, def.ap, b)))
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
                        "maxSelect": ds.filter.count,
                        "pickAction": pick_action_str,
                        "postAddDiscard": ds.post_add_discard,
                        "postOps": ds.post_ops,
                        "resumeContext": resume_context,
                        "title": format!("山札の上から{}枚めくりました。条件に合うカードを{}まで選び{}（残りはデッキ下へ）", ds.search_count, ds.filter.count, title_action),
                        "revealedCards": candidates,
                        "optional": true
                    }
                }));
                self.log(format!("🔍 [{}] の効果で山札の上から {} 枚めくりました", actor_name, ds.search_count));
                return true;
            }
        }

        false
    }

    /// 証拠をリムーブし、該当カードの【ヒラメキ】を発動する
    /// 戻り値: (opened_pending_ui, opponent_blocked_evidence_gain)
    fn remove_evidence_and_trigger_hirameki(
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

        let mut opened_ui = false;
        let mut block_opp_gain = false;
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
        (opened_ui, block_opp_gain)
    }
}

fn target_action_prompt_title(action: &TargetAction, filter: &TargetFilter) -> String {
    let prefix = match filter.zone {
        TargetZone::Remove => "リムーブエリアの",
        TargetZone::Hand => "手札の",
        TargetZone::Field => match filter.owner {
            OwnerScope::OppOnly => "相手の",
            OwnerScope::SelfOnly => "自分の",
            OwnerScope::Both => "",
        },
        TargetZone::Deck => "",
    };
    match action {
        TargetAction::Sleep => format!("スリープさせる{}キャラを1体選択してください", prefix),
        TargetAction::Stun => format!("スタンさせる{}キャラを1体選択してください", prefix),
        TargetAction::Activate => format!("アクティブにする{}キャラを1体選択してください", prefix),
        TargetAction::Remove => format!("リムーブする{}キャラを1体選択してください", prefix),
        TargetAction::BottomDeck => format!("デッキの下に移す{}キャラを1体選択してください", prefix),
        TargetAction::ApBuff(v) => format!("AP＋{}する{}キャラを1体選択してください", v, prefix),
        TargetAction::ApDebuff(v) => format!("AP－{}する{}キャラを1体選択してください", v, prefix),
        TargetAction::GiveRush(_) => format!("突撃/迅速を与える{}キャラを1体選択してください", prefix),
        TargetAction::ReviveActive => "リムーブエリアから登場させるカードを選択してください".to_string(),
        TargetAction::ReviveSleep => "リムーブエリアからスリープ状態で登場させるカードを選択してください".to_string(),
        TargetAction::RecoverToHand => "リムーブエリアから手札に加えるカードを選択してください".to_string(),
        TargetAction::PlayFromHand => "手札から登場させるキャラを選択してください".to_string(),
    }
}
