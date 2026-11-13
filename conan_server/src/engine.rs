use crate::catalog::CardCatalog;
use crate::models::{
    CardInstance, CardOrientation, CasePhase, DeckDefinition, MatchPhase, Zone,
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

fn parse_evidence_gain(desc: &str) -> usize {
    if desc.contains("証拠を3つ得る") || desc.contains("証拠を3枚得る") { return 3; }
    if desc.contains("証拠を2つ得る") || desc.contains("証拠を2枚得る") { return 2; }
    if desc.contains("証拠を1つ得る") || desc.contains("証拠を1枚得る") || desc.contains("証拠を得る") { return 1; }
    0
}

fn parse_ap_bonus(desc: &str) -> i32 {
    if desc.contains("AP＋3000") || desc.contains("AP+3000") { return 3000; }
    if desc.contains("AP＋2000") || desc.contains("AP+2000") { return 2000; }
    if desc.contains("AP＋1000") || desc.contains("AP+1000") { return 1000; }
    0
}

fn parse_hand_remove_cost(desc: &str) -> usize {
    if desc.contains("手札を2枚リムーブ") || desc.contains("手札から特徴［少年探偵団］のキャラを2枚リムーブ") {
        return 2;
    }
    if desc.contains("手札を1枚リムーブ") || desc.contains("手札からキャラを1枚リムーブ")
        || desc.contains("手札から特徴［") && desc.contains("リムーブする")
        || desc.contains("手札からカード名［") && desc.contains("リムーブする")
        || desc.contains("手札から【カットイン】") && desc.contains("リムーブする")
    {
        return 1;
    }
    0
}

fn parse_deck_search_count(desc: &str) -> usize {
    if desc.contains("デッキのカードを上から6枚見る") { return 6; }
    if desc.contains("デッキのカードを上から5枚見る") { return 5; }
    if desc.contains("デッキのカードを上から4枚見る") { return 4; }
    if desc.contains("デッキのカードを上から3枚見る") { return 3; }
    if desc.contains("デッキのカードを上から2枚見る") { return 2; }
    if desc.contains("デッキのカードを上から1枚見る") { return 1; }
    0
}

fn parse_rush_target(desc: &str) -> Option<String> {
    if desc.contains("突撃［キャラ］") {
        Some("CHAR".to_string())
    } else if desc.contains("突撃［事件］") {
        Some("CASE".to_string())
    } else {
        None
    }
}

fn extract_trait_filters(text: &str) -> Vec<String> {
    let mut traits = Vec::new();
    let mut rest = text;
    while let Some(idx) = rest.find("特徴［") {
        let after = &rest[idx + "特徴［".len()..];
        if let Some(end_idx) = after.find('］') {
            let t = &after[..end_idx];
            if !t.is_empty() && !traits.contains(&t.to_string()) {
                traits.push(t.to_string());
            }
            rest = &after[end_idx + '］'.len_utf8()..];
        } else {
            break;
        }
    }
    traits
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

    fn generate_decision_id(&mut self) -> String {
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
            "targetSelect" => self.handle_target_select_answer(player_id, &val),
            "deckSearch" => self.handle_deck_search_answer(player_id, &val),
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

        self.phase = MatchPhase::MAIN;
        self.prompt_main_action(turn_player_id);
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

        // 2. Hand Play
        if !ps.normal_hand_played_this_turn && !ps.hint_played_this_turn {
            let mut playable_cards = vec![];
            for c in &ps.hand {
                if let Some(c_def) = self.catalog.get(&c.definition_id) {
                    let card_colors = normalize_color_set(&c_def.colors);
                    let color_ok = card_colors.is_empty() || case_colors.is_empty() || card_colors.is_subset(&case_colors);
                    if c_def.level <= effective_file && color_ok {
                        playable_cards.push(c.instance_id.clone());
                    }
                }
            }
            if !playable_cards.is_empty() && ps.field.len() < 5 {
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
        let mut declare_actors = vec![];
        for c in &ps.field {
            if let Some(c_def) = self.catalog.get(&c.definition_id) {
                if c_def.description.contains("【宣言】") {
                    // 条件チェック: パートナー色・FILE数
                    if !self.check_effect_conditions(player_id, &c_def.description) {
                        continue;
                    }
                    // コストチェック: 手札リムーブコスト
                    let hand_cost = parse_hand_remove_cost(&c_def.description);
                    if hand_cost > 0 && ps.hand.len() < hand_cost {
                        continue;
                    }
                    let is_turn_1 = c_def.description.contains("【ターン1】");
                    if is_turn_1 && c.used_declare_this_turn {
                        continue;
                    }
                    let needs_sleep = c_def.description.contains("【スリープ】");
                    if needs_sleep && c.orientation != CardOrientation::ACTIVE {
                        continue;
                    }
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
                    if p_def.description.contains("【宣言】") {
                        let cond_ok = self.check_effect_conditions(player_id, &p_def.description);
                        let hand_cost = parse_hand_remove_cost(&p_def.description);
                        let hand_ok = hand_cost == 0 || ps.hand.len() >= hand_cost;
                        let is_turn_1 = p_def.description.contains("【ターン1】");
                        let needs_sleep = p_def.description.contains("【スリープ】");
                        let ok = cond_ok && hand_ok && (!is_turn_1 || !p.used_declare_this_turn) && (!needs_sleep || p.orientation == CardOrientation::ACTIVE);
                        if ok {
                            declare_actors.push(json!({
                                "actorId": p.instance_id.clone(),
                                "name": self.card_name_by_def(&p.definition_id),
                                "desc": p_def.description.clone(),
                            }));
                        }
                    }
                }
            }
        }
        if !declare_actors.is_empty() {
            actions.push(json!({
                "type": "DECLARE",
                "actors": declare_actors
            }));
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
                if self.phase == MatchPhase::FINISHED {
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
                if let Some(ps) = self.players.get_mut(player_id) {
                    if let Some(idx) = ps.hand.iter().position(|c| c.instance_id == card_id) {
                        let mut card = ps.hand.remove(idx);
                        played_def_id = Some(card.definition_id.clone());
                        played_inst_id = Some(card.instance_id.clone());
                        card.zone = Zone::FIELD;
                        card.orientation = CardOrientation::ACTIVE;
                        card.entered_turn = turn;
                        card.face_up = true;
                        ps.field.push(card);
                        ps.normal_hand_played_this_turn = true;
                    }
                }
                if let (Some(def_id), Some(inst_id)) = (played_def_id, played_inst_id) {
                    let card_name = self.card_name_by_def(&def_id);
                    let lvl = self.catalog.get(&def_id).map(|d| d.level).unwrap_or(0);
                    self.log(format!("🃏 {} が [{}] (Lv.{}) を登場させました", player_id, card_name, lvl));
                    let need_target = self.resolve_on_play_effects(player_id, &inst_id, &def_id);
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

                    if ps.field.len() < 5 {
                        for c in &ps.hand {
                            if let Some(c_def) = self.catalog.get(&c.definition_id) {
                                let card_colors = normalize_color_set(&c_def.colors);
                                let color_ok = card_colors.is_empty() || case_colors.is_empty() || card_colors.is_subset(&case_colors);
                                if c_def.level <= effective_file && color_ok {
                                    playable_cards.push(c.instance_id.clone());
                                }
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
                let mut became_resolution = false;
                if let Some(ps) = self.players.get_mut(player_id) {
                    if let Some(ref mut p) = ps.partner {
                        if p.zone == Zone::PARTNER {
                            p.orientation = CardOrientation::SLEEP;
                            p.zone = Zone::FILE;
                        }
                    }
                    if ps.effective_file_count() >= 7 && ps.case_phase != CasePhase::RESOLUTION {
                        ps.case_phase = CasePhase::RESOLUTION;
                        became_resolution = true;
                    }
                }
                self.log(format!("🤝 {} が パートナーのアシスト を宣言（パートナーをFILEへ移動）", player_id));
                if became_resolution {
                    self.log(format!("✨ {} のFILEが7枚以上になり、事件が【解決編】に突入しました！", player_id));
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
            ps.field.iter().find(|c| c.instance_id == actor_id).map(|c| c.definition_id.clone())
        });

        let def_id = match def_id_opt {
            Some(id) => id,
            None => {
                self.prompt_main_action(player_id);
                return;
            }
        };

        let desc = match self.catalog.get(&def_id) {
            Some(d) => d.description.clone(),
            None => {
                self.prompt_main_action(player_id);
                return;
            }
        };

        let card_name = self.card_name_by_def(&def_id);

        // 1. コスト支払い
        let hand_cost = parse_hand_remove_cost(&desc);
        let mut removed_cost_names = vec![];
        if let Some(ps) = self.players.get_mut(player_id) {
            if desc.contains("【スリープ】") {
                if let Some(ref mut p) = ps.partner {
                    if p.instance_id == actor_id {
                        p.orientation = CardOrientation::SLEEP;
                    }
                }
                if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == actor_id) {
                    c.orientation = CardOrientation::SLEEP;
                }
            }
            if let Some(ref mut p) = ps.partner {
                if p.instance_id == actor_id {
                    p.used_declare_this_turn = true;
                }
            }
            if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == actor_id) {
                c.used_declare_this_turn = true;
            }
            // 手札コストの支払い
            for _ in 0..hand_cost {
                if let Some(mut removed_c) = ps.hand.pop() {
                    removed_c.zone = Zone::REMOVE;
                    let did = removed_c.definition_id.clone();
                    ps.remove.push(removed_c);
                    removed_cost_names.push(did);
                }
            }
        }
        for did in removed_cost_names {
            let cost_name = self.card_name_by_def(&did);
            self.log(format!("💸 コスト支払い: {} の手札から [{}] をリムーブしました", player_id, cost_name));
        }

        self.log(format!("⚡ {} が [{}] の【宣言】を発動！", player_id, card_name));

        // 2. 効果内容の判定
        let opp_id = self.player_order.iter().find(|p| *p != player_id).cloned().unwrap_or_default();

        let is_ap_buff = desc.contains("AP＋") || desc.contains("AP+");
        let is_give_rush = desc.contains("突撃") && (desc.contains("与え") || desc.contains("持つ"));
        let is_activate = desc.contains("アクティブにする");
        let is_sleep_opp = desc.contains("スリープさせる") && (desc.contains("相手") || desc.contains("キャラを1枚") || desc.contains("キャラ1枚"));
        let is_stun_opp = desc.contains("スタンさせる") && (desc.contains("相手") || desc.contains("キャラを1枚") || desc.contains("キャラ1枚"));
        let is_remove_opp = desc.contains("リムーブする") && (desc.contains("相手") || desc.contains("キャラを1枚") || desc.contains("キャラ1枚"));

        let mut candidates: Vec<Value> = vec![];
        let mut effect_type = "";
        let mut title = "";

        if is_sleep_opp {
            effect_type = "SLEEP";
            title = "スリープさせる相手キャラを1体選択してください";
            if let Some(opp_ps) = self.players.get(&opp_id) {
                for c in &opp_ps.field {
                    if c.orientation == CardOrientation::ACTIVE {
                        candidates.push(json!({
                            "instanceId": c.instance_id,
                            "name": self.card_name_by_def(&c.definition_id),
                            "level": self.catalog.get(&c.definition_id).map(|d| d.level).unwrap_or(0),
                            "ap": self.catalog.get(&c.definition_id).map(|d| d.ap).unwrap_or(0) + c.temp_ap_bonus,
                            "orientation": format!("{:?}", c.orientation),
                            "isOpponent": true
                        }));
                    }
                }
            }
        } else if is_stun_opp {
            effect_type = "STUN";
            title = "スタンさせる相手キャラを1体選択してください";
            if let Some(opp_ps) = self.players.get(&opp_id) {
                for c in &opp_ps.field {
                    candidates.push(json!({
                        "instanceId": c.instance_id,
                        "name": self.card_name_by_def(&c.definition_id),
                        "level": self.catalog.get(&c.definition_id).map(|d| d.level).unwrap_or(0),
                        "ap": self.catalog.get(&c.definition_id).map(|d| d.ap).unwrap_or(0) + c.temp_ap_bonus,
                        "orientation": format!("{:?}", c.orientation),
                        "isOpponent": true
                    }));
                }
            }
        } else if is_remove_opp {
            effect_type = "REMOVE";
            title = "リムーブする相手キャラを1体選択してください";
            if let Some(opp_ps) = self.players.get(&opp_id) {
                for c in &opp_ps.field {
                    candidates.push(json!({
                        "instanceId": c.instance_id,
                        "name": self.card_name_by_def(&c.definition_id),
                        "level": self.catalog.get(&c.definition_id).map(|d| d.level).unwrap_or(0),
                        "ap": self.catalog.get(&c.definition_id).map(|d| d.ap).unwrap_or(0) + c.temp_ap_bonus,
                        "orientation": format!("{:?}", c.orientation),
                        "isOpponent": true
                    }));
                }
            }
        } else if is_activate {
            effect_type = "ACTIVATE";
            title = "アクティブにする味方キャラを1体選択してください";
            if let Some(ps) = self.players.get(player_id) {
                for c in &ps.field {
                    if c.orientation == CardOrientation::SLEEP {
                        candidates.push(json!({
                            "instanceId": c.instance_id,
                            "name": self.card_name_by_def(&c.definition_id),
                            "level": self.catalog.get(&c.definition_id).map(|d| d.level).unwrap_or(0),
                            "ap": self.catalog.get(&c.definition_id).map(|d| d.ap).unwrap_or(0) + c.temp_ap_bonus,
                            "orientation": format!("{:?}", c.orientation),
                            "isOpponent": false
                        }));
                    }
                }
            }
        } else if is_ap_buff && desc.contains("選び") {
            effect_type = "AP_BUFF";
            title = "APを強化する味方キャラを1体選択してください";
            if let Some(ps) = self.players.get(player_id) {
                for c in &ps.field {
                    candidates.push(json!({
                        "instanceId": c.instance_id,
                        "name": self.card_name_by_def(&c.definition_id),
                        "level": self.catalog.get(&c.definition_id).map(|d| d.level).unwrap_or(0),
                        "ap": self.catalog.get(&c.definition_id).map(|d| d.ap).unwrap_or(0) + c.temp_ap_bonus,
                        "orientation": format!("{:?}", c.orientation),
                        "isOpponent": false
                    }));
                }
            }
        } else if is_give_rush && desc.contains("選び") {
            effect_type = "GIVE_RUSH";
            title = "突撃を与える味方キャラを1体選択してください";
            if let Some(ps) = self.players.get(player_id) {
                for c in &ps.field {
                    if !c.has_rush {
                        candidates.push(json!({
                            "instanceId": c.instance_id,
                            "name": self.card_name_by_def(&c.definition_id),
                            "level": self.catalog.get(&c.definition_id).map(|d| d.level).unwrap_or(0),
                            "ap": self.catalog.get(&c.definition_id).map(|d| d.ap).unwrap_or(0) + c.temp_ap_bonus,
                            "orientation": format!("{:?}", c.orientation),
                            "isOpponent": false
                        }));
                    }
                }
            }
        }

        if !effect_type.is_empty() {
            if candidates.is_empty() {
                self.log("⚠️ 対象となる有効なカードが存在しないため、効果は適用されませんでした");
                self.prompt_main_action(player_id);
                return;
            }

            let dec_id = self.generate_decision_id();
            self.pending_decision = Some(json!({
                "decisionId": dec_id,
                "revision": self.revision,
                "playerId": player_id,
                "kind": "targetSelect",
                "options": {
                    "actorId": actor_id,
                    "actorName": card_name,
                    "effectType": effect_type,
                    "title": title,
                    "candidates": candidates,
                    "optional": true
                }
            }));
            return;
        }

        // デッキサーチ・めくり効果
        let search_count = parse_deck_search_count(&desc);
        if search_count > 0 {
            let mut revealed = vec![];
            if let Some(ps) = self.players.get_mut(player_id) {
                let deck_len = ps.deck.len();
                let actual_count = search_count.min(deck_len);
                for i in 0..actual_count {
                    let idx = deck_len - 1 - i;
                    let card = &mut ps.deck[idx];
                    card.face_up = true;
                    revealed.push(card.clone());
                }
            }

            if !revealed.is_empty() {
                let mut candidates: Vec<Value> = vec![];
                for c in &revealed {
                    candidates.push(json!({
                        "instanceId": c.instance_id,
                        "definitionId": c.definition_id,
                        "name": self.card_name_by_def(&c.definition_id),
                        "level": self.catalog.get(&c.definition_id).map(|d| d.level).unwrap_or(0),
                        "ap": self.catalog.get(&c.definition_id).map(|d| d.ap).unwrap_or(0),
                        "colors": self.catalog.get(&c.definition_id).map(|d| d.colors.clone()).unwrap_or_default(),
                    }));
                }

                let dec_id = self.generate_decision_id();
                self.pending_decision = Some(json!({
                    "decisionId": dec_id,
                    "revision": self.revision,
                    "playerId": player_id,
                    "kind": "deckSearch",
                    "options": {
                        "actorId": actor_id,
                        "actorName": card_name,
                        "searchCount": search_count,
                        "title": format!("山札の上から{}枚めくりました。手札に加えるカードを1枚選んでください（残りはデッキ下へ）", search_count),
                        "revealedCards": candidates,
                        "optional": true
                    }
                }));
                self.log(format!("🔍 [{}] の【宣言】で山札の上から {} 枚めくりました", card_name, search_count));
                return;
            }
        }

        self.resolve_declare_effect_immediate(player_id, actor_id, &desc);
        self.prompt_main_action(player_id);
    }

    fn resolve_declare_effect_immediate(&mut self, player_id: &str, actor_id: &str, desc: &str) {
        let card_name = self.card_name_by_inst(actor_id);

        let draw_count = parse_draw_count(desc);
        if draw_count > 0 {
            self.draw_cards(player_id, draw_count);
            self.log(format!("✨ [{}] の【宣言】でカードを {} 枚引きました", card_name, draw_count));
        }

        let ev_count = parse_evidence_gain(desc);
        if ev_count > 0 {
            self.gain_evidence(player_id, ev_count);
            self.log(format!("✨ [{}] の【宣言】で証拠を {} つ獲得しました", card_name, ev_count));
        }

        let ap_bonus = parse_ap_bonus(desc);
        if ap_bonus > 0 && !desc.contains("選び") {
            if let Some(ps) = self.players.get_mut(player_id) {
                if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == actor_id) {
                    c.temp_ap_bonus += ap_bonus;
                    self.log(format!("✨ [{}] の【宣言】で自身のAP +{}", card_name, ap_bonus));
                }
            }
        }

        if desc.contains("突撃") && !desc.contains("与え") {
            let r_target = parse_rush_target(desc);
            if let Some(ps) = self.players.get_mut(player_id) {
                if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == actor_id) {
                    c.has_rush = true;
                    c.rush_target = r_target.clone();
                    let label = match r_target.as_deref() {
                        Some("CHAR") => "〚突撃［キャラ］〛",
                        Some("CASE") => "〚突撃［事件］〛",
                        _ => "〚突撃〛",
                    };
                    self.log(format!("⚡ [{}] の【宣言】で自身に{}を付与しました", card_name, label));
                }
            }
        }
    }

    fn handle_target_select_answer(&mut self, player_id: &str, val: &Value) {
        let target_id = val.get("targetId").and_then(|v| v.as_str());
        let actor_id = val.get("actorId").and_then(|v| v.as_str()).unwrap_or("");
        let effect_type = val.get("effectType").and_then(|v| v.as_str()).unwrap_or("");

        let actor_name = self.card_name_by_inst(actor_id);

        if let Some(tid) = target_id {
            let target_name = self.card_name_by_inst(tid);
            let opp_id = self.player_order.iter().find(|p| *p != player_id).cloned().unwrap_or_default();

            match effect_type {
                "SLEEP" => {
                    if let Some(opp_ps) = self.players.get_mut(&opp_id) {
                        if let Some(c) = opp_ps.field.iter_mut().find(|c| c.instance_id == tid) {
                            c.orientation = CardOrientation::SLEEP;
                            self.log(format!("💤 [{}] の効果で相手の [{}] をスリープさせました", actor_name, target_name));
                        }
                    }
                }
                "STUN" => {
                    if let Some(opp_ps) = self.players.get_mut(&opp_id) {
                        if let Some(c) = opp_ps.field.iter_mut().find(|c| c.instance_id == tid) {
                            c.orientation = CardOrientation::STUN;
                            self.log(format!("💫 [{}] の効果で相手の [{}] をスタンさせました", actor_name, target_name));
                        }
                    }
                }
                "REMOVE" => {
                    let mut removed_def_id = None;
                    if let Some(opp_ps) = self.players.get_mut(&opp_id) {
                        if let Some(idx) = opp_ps.field.iter().position(|c| c.instance_id == tid) {
                            let mut card = opp_ps.field.remove(idx);
                            removed_def_id = Some(card.definition_id.clone());
                            card.zone = Zone::REMOVE;
                            opp_ps.remove.push(card);
                        }
                    }
                    if let Some(rem_def) = removed_def_id {
                        self.log(format!("💥 [{}] の効果で相手の [{}] をリムーブしました！", actor_name, target_name));
                        // 現場リムーブ時トリガーの解決
                        self.trigger_on_remove(&opp_id, &rem_def);
                    }
                }
                "ACTIVATE" => {
                    if let Some(ps) = self.players.get_mut(player_id) {
                        if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == tid) {
                            c.orientation = CardOrientation::ACTIVE;
                            self.log(format!("✨ [{}] の効果で味方の [{}] をアクティブにしました！", actor_name, target_name));
                        }
                    }
                }
                "AP_BUFF" => {
                    let def_id = self.find_card_def_id(actor_id).unwrap_or_default();
                    let desc = self.catalog.get(&def_id).map(|d| d.description.clone()).unwrap_or_default();
                    let bonus = parse_ap_bonus(&desc);
                    let actual_bonus = if bonus > 0 { bonus } else { 1000 };
                    if let Some(ps) = self.players.get_mut(player_id) {
                        if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == tid) {
                            c.temp_ap_bonus += actual_bonus;
                            self.log(format!("✨ [{}] の効果で [{}] のAP +{}", actor_name, target_name, actual_bonus));
                        }
                    }
                }
                "GIVE_RUSH" => {
                    let def_id = self.find_card_def_id(actor_id).unwrap_or_default();
                    let desc = self.catalog.get(&def_id).map(|d| d.description.clone()).unwrap_or_default();
                    let r_target = parse_rush_target(&desc);
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
                _ => {
                    self.log(format!("✨ [{}] の効果が [{}] に適用されました", actor_name, target_name));
                }
            }
        } else {
            self.log(format!("{} は [{}] の対象を選択しませんでした（スキップ）", player_id, actor_name));
        }

        self.prompt_main_action(player_id);
    }

    /// デッキサーチ・めくり選択の意思決定を処理する
    fn handle_deck_search_answer(&mut self, player_id: &str, val: &Value) {
        let chosen_instance_id = val.get("chosenCardId").and_then(|v| v.as_str());
        let actor_id = val.get("actorId").and_then(|v| v.as_str()).unwrap_or("");
        let actor_name = self.card_name_by_inst(actor_id);

        let mut revealed_instances = vec![];
        if let Some(pd) = &self.pending_decision {
            if let Some(opts) = pd.get("options") {
                if let Some(cards) = opts.get("revealedCards").and_then(|v| v.as_array()) {
                    for c in cards {
                        if let Some(iid) = c.get("instanceId").and_then(|v| v.as_str()) {
                            revealed_instances.push(iid.to_string());
                        }
                    }
                }
            }
        }

        let mut added_to_hand = false;
        let mut added_def_id = None;

        if let Some(ps) = self.players.get_mut(player_id) {
            let mut remaining_cards = vec![];
            for iid in &revealed_instances {
                if let Some(pos) = ps.deck.iter().position(|c| &c.instance_id == iid) {
                    let mut card = ps.deck.remove(pos);
                    if let Some(chosen_id) = chosen_instance_id {
                        if chosen_id == iid {
                            card.zone = Zone::HAND;
                            card.face_up = true;
                            added_def_id = Some(card.definition_id.clone());
                            ps.hand.push(card);
                            added_to_hand = true;
                            continue;
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

        if added_to_hand {
            if let Some(did) = added_def_id {
                let card_name = self.card_name_by_def(&did);
                self.log(format!("✨ [{}] の効果でデッキから [{}] を手札に加え、残りをデッキ下に移しました", actor_name, card_name));
            }
        } else {
            self.log(format!("{} はカードを手札に加えませんでした（スキップ、残りをデッキ下へ）", player_id));
        }

        // 手札に加えた場合、手札を1枚リムーブする効果の処理
        let def_id = self.find_card_def_id(actor_id).unwrap_or_default();
        let desc = self.catalog.get(&def_id).map(|d| d.description.clone()).unwrap_or_default();
        if added_to_hand && desc.contains("カードを手札に加えた場合、手札を1枚リムーブする") {
            let mut removed_name = None;
            if let Some(ps) = self.players.get_mut(player_id) {
                if let Some(mut removed_c) = ps.hand.pop() {
                    removed_c.zone = Zone::REMOVE;
                    let rdid = removed_c.definition_id.clone();
                    ps.remove.push(removed_c);
                    removed_name = Some(rdid);
                }
            }
            if let Some(rdid) = removed_name {
                let rem_name = self.card_name_by_def(&rdid);
                self.log(format!("💸 [{}] の効果（手札加えた後の代償）: 手札から [{}] をリムーブしました", actor_name, rem_name));
            }
        }

        self.prompt_main_action(player_id);
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

        // カットイン可能カード（手札の【カットイン】持ちカード）
        let cutins: Vec<Value> = opp_ps.hand.iter()
            .filter_map(|c| {
                let def = self.catalog.get(&c.definition_id)?;
                if def.description.contains("【カットイン】") {
                    Some(json!({
                        "instanceId": c.instance_id,
                        "definitionId": c.definition_id,
                        "name": if def.description.len() > 0 { 
                            self.catalog.get(&c.definition_id).map(|d| d.description.clone()).unwrap_or_default()
                        } else { String::new() }
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
        if let Some(card_id) = val.get("cardId").and_then(|v| v.as_str()) {
            let turn = self.turn;
            if let Some(ps) = self.players.get_mut(player_id) {
                if ps.field.len() < 5 {
                    if let Some(idx) = ps.hand.iter().position(|c| c.instance_id == card_id) {
                        let mut card = ps.hand.remove(idx);
                        played_def_id = Some(card.definition_id.clone());
                        played_inst_id = Some(card.instance_id.clone());
                        card.zone = Zone::FIELD;
                        card.orientation = CardOrientation::ACTIVE;
                        card.entered_turn = turn;
                        card.face_up = true;
                        ps.field.push(card);
                    }
                }
            }
        }
        if let (Some(def_id), Some(inst_id)) = (played_def_id, played_inst_id) {
            let card_name = self.card_name_by_def(&def_id);
            self.log(format!("🃏 {} がヒントから [{}] を登場させました", player_id, card_name));
            let need_target = self.resolve_on_play_effects(player_id, &inst_id, &def_id);
            if need_target {
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
                self.resolve_cutin_effects(player_id, &def_id);
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
                let defender_ps = self.players.get_mut(&defender_pid).unwrap();
                if let Some(mut ev_card) = defender_ps.evidence.pop() {
                    ev_card.zone = Zone::REMOVE;
                    defender_ps.remove.push(ev_card);
                }
                self.gain_evidence(&attacker_pid, 1);
                self.log(format!("💥 事件への攻撃ヒット！ {} の証拠を1枚削り、{} が証拠を1つ獲得！", defender_pid, attacker_pid));
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

            let actor_bonus = self.players.get(&attacker_pid)
                .and_then(|p| p.field.iter().find(|c| c.instance_id == actor_id).map(|c| c.temp_ap_bonus))
                .unwrap_or(0);
            let def_bonus = self.players.get(&defender_pid)
                .and_then(|p| p.field.iter().find(|c| c.instance_id == def_cid).map(|c| c.temp_ap_bonus))
                .unwrap_or(0);

            if let (Some(a_def_id), Some(d_def_id)) = (actor_def_id, def_def_id) {
                let actor_ap = self.catalog.get(&a_def_id).map(|d| d.ap).unwrap_or(0) + actor_bonus;
                let def_ap = self.catalog.get(&d_def_id).map(|d| d.ap).unwrap_or(0) + def_bonus;

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

    /// 登場時効果を解決する（対象選択が必要な場合は true を返し、pending_decision をセットする）
    fn resolve_on_play_effects(&mut self, player_id: &str, actor_id: &str, definition_id: &str) -> bool {
        let desc = match self.catalog.get(definition_id) {
            Some(d) => d.description.clone(),
            None => return false,
        };

        if desc.is_empty() {
            return false;
        }

        let card_name = self.card_name_by_def(definition_id);

        // 突撃: 登場したターンからすぐにアクション可能
        if desc.contains("突撃") && !desc.contains("与え") && !desc.contains("コンタクトによってリムーブされたとき") {
            let r_target = parse_rush_target(&desc);
            if let Some(ps) = self.players.get_mut(player_id) {
                if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == actor_id) {
                    c.has_rush = true;
                    c.rush_target = r_target.clone();
                    let label = match r_target.as_deref() {
                        Some("CHAR") => "〚突撃［キャラ］〛（キャラ限定）",
                        Some("CASE") => "〚突撃［事件］〛（事件限定）",
                        _ => "〚突撃〛",
                    };
                    self.log(format!("⚡ {} [{}] は登場ターンから攻撃可能です！", label, card_name));
                }
            }
        }

        // 【登場】【登場時】トリガーの効果
        let is_on_play = desc.contains("【登場】") || desc.contains("【登場時】")
            || desc.contains("登場させたとき") || desc.contains("登場したとき");

        if !is_on_play {
            return false;
        }

        // 条件チェック: パートナー色・FILE数
        if !self.check_effect_conditions(player_id, &desc) {
            self.log(format!("⚠️ [{}] の登場時効果: 発動条件（パートナー色/FILE数）を満たしていないため不発となりました", card_name));
            return false;
        }

        // コストチェック: 手札リムーブコスト
        let hand_cost = parse_hand_remove_cost(&desc);
        if hand_cost > 0 {
            let has_enough_hand = self.players.get(player_id).map(|ps| ps.hand.len() >= hand_cost).unwrap_or(false);
            if !has_enough_hand {
                self.log(format!("⚠️ [{}] の登場時効果: 手札コスト（{}枚）が不足しているため発動できませんでした", card_name, hand_cost));
                return false;
            }
            // 手札コスト支払い
            let mut removed_cost_names = vec![];
            if let Some(ps) = self.players.get_mut(player_id) {
                for _ in 0..hand_cost {
                    if let Some(mut removed_c) = ps.hand.pop() {
                        removed_c.zone = Zone::REMOVE;
                        let did = removed_c.definition_id.clone();
                        ps.remove.push(removed_c);
                        removed_cost_names.push(did);
                    }
                }
            }
            for did in removed_cost_names {
                let cost_name = self.card_name_by_def(&did);
                self.log(format!("💸 コスト支払い: {} の手札から [{}] をリムーブしました", player_id, cost_name));
            }
        }

        // 1. 即時解決効果（ドロー、証拠獲得、自身のAPバフ）
        let draw_count = parse_draw_count(&desc);
        if draw_count > 0 {
            self.draw_cards(player_id, draw_count);
            self.log(format!("✨ [{}] の効果でカードを {} 枚引きました", card_name, draw_count));
        }

        let ev_count = parse_evidence_gain(&desc);
        if ev_count > 0 {
            self.gain_evidence(player_id, ev_count);
            self.log(format!("✨ [{}] の効果で証拠を {} つ獲得しました", card_name, ev_count));
        }

        let ap_bonus = parse_ap_bonus(&desc);
        if ap_bonus > 0 && !desc.contains("選び") {
            if let Some(ps) = self.players.get_mut(player_id) {
                if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == actor_id) {
                    c.temp_ap_bonus += ap_bonus;
                    self.log(format!("✨ [{}] の効果で自身のAP +{}", card_name, ap_bonus));
                }
            }
        }

        // 2. 対象選択が必要な効果（スリープ、スタン、リムーブ、味方APバフなど）
        let opp_id = self.player_order.iter().find(|p| *p != player_id).cloned().unwrap_or_default();
        let is_sleep_opp = desc.contains("スリープさせる") && (desc.contains("相手") || desc.contains("キャラを1枚") || desc.contains("キャラ1枚"));
        let is_stun_opp = desc.contains("スタンさせる") && (desc.contains("相手") || desc.contains("キャラを1枚") || desc.contains("キャラ1枚"));
        let is_remove_opp = desc.contains("リムーブする") && (desc.contains("相手") || desc.contains("キャラを1枚") || desc.contains("キャラ1枚") || desc.contains("デッキの下に移す"));
        let is_ap_buff_target = (desc.contains("AP＋") || desc.contains("AP+")) && desc.contains("選び");

        let mut candidates: Vec<Value> = vec![];
        let mut effect_type = "";
        let mut title = "";

        if is_sleep_opp {
            effect_type = "SLEEP";
            title = "登場時効果: スリープさせる相手キャラを1体選択してください";
            if let Some(opp_ps) = self.players.get(&opp_id) {
                for c in &opp_ps.field {
                    if c.orientation == CardOrientation::ACTIVE {
                        candidates.push(json!({
                            "instanceId": c.instance_id,
                            "name": self.card_name_by_def(&c.definition_id),
                            "level": self.catalog.get(&c.definition_id).map(|d| d.level).unwrap_or(0),
                            "ap": self.catalog.get(&c.definition_id).map(|d| d.ap).unwrap_or(0) + c.temp_ap_bonus,
                            "orientation": format!("{:?}", c.orientation),
                            "isOpponent": true
                        }));
                    }
                }
            }
        } else if is_stun_opp {
            effect_type = "STUN";
            title = "登場時効果: スタンさせる相手キャラを1体選択してください";
            if let Some(opp_ps) = self.players.get(&opp_id) {
                for c in &opp_ps.field {
                    candidates.push(json!({
                        "instanceId": c.instance_id,
                        "name": self.card_name_by_def(&c.definition_id),
                        "level": self.catalog.get(&c.definition_id).map(|d| d.level).unwrap_or(0),
                        "ap": self.catalog.get(&c.definition_id).map(|d| d.ap).unwrap_or(0) + c.temp_ap_bonus,
                        "orientation": format!("{:?}", c.orientation),
                        "isOpponent": true
                    }));
                }
            }
        } else if is_remove_opp {
            effect_type = "REMOVE";
            title = "登場時効果: リムーブ/除去する相手キャラを1体選択してください";
            if let Some(opp_ps) = self.players.get(&opp_id) {
                for c in &opp_ps.field {
                    candidates.push(json!({
                        "instanceId": c.instance_id,
                        "name": self.card_name_by_def(&c.definition_id),
                        "level": self.catalog.get(&c.definition_id).map(|d| d.level).unwrap_or(0),
                        "ap": self.catalog.get(&c.definition_id).map(|d| d.ap).unwrap_or(0) + c.temp_ap_bonus,
                        "orientation": format!("{:?}", c.orientation),
                        "isOpponent": true
                    }));
                }
            }
        } else if is_ap_buff_target {
            effect_type = "AP_BUFF";
            title = "登場時効果: APを強化するキャラを1体選択してください";
            if let Some(ps) = self.players.get(player_id) {
                for c in &ps.field {
                    candidates.push(json!({
                        "instanceId": c.instance_id,
                        "name": self.card_name_by_def(&c.definition_id),
                        "level": self.catalog.get(&c.definition_id).map(|d| d.level).unwrap_or(0),
                        "ap": self.catalog.get(&c.definition_id).map(|d| d.ap).unwrap_or(0) + c.temp_ap_bonus,
                        "orientation": format!("{:?}", c.orientation),
                        "isOpponent": false
                    }));
                }
            }
        }

        if !effect_type.is_empty() {
            if candidates.is_empty() {
                self.log(format!("⚠️ [{}] の登場時効果: 有効な対象が存在しないためスキップされました", card_name));
                return false;
            }

            let dec_id = self.generate_decision_id();
            self.pending_decision = Some(json!({
                "decisionId": dec_id,
                "revision": self.revision,
                "playerId": player_id,
                "kind": "targetSelect",
                "options": {
                    "actorId": actor_id,
                    "actorName": card_name,
                    "effectType": effect_type,
                    "title": title,
                    "candidates": candidates,
                    "optional": true
                }
            }));
            return true;
        }

        // 3. デッキサーチ・めくり効果（「自分のデッキのカードを上からX枚見る」）
        let search_count = parse_deck_search_count(&desc);
        if search_count > 0 {
            let mut revealed = vec![];
            if let Some(ps) = self.players.get_mut(player_id) {
                let deck_len = ps.deck.len();
                let actual_count = search_count.min(deck_len);
                for i in 0..actual_count {
                    let idx = deck_len - 1 - i;
                    let card = &mut ps.deck[idx];
                    card.face_up = true;
                    revealed.push(card.clone());
                }
            }

            if !revealed.is_empty() {
                let mut candidates: Vec<Value> = vec![];
                for c in &revealed {
                    candidates.push(json!({
                        "instanceId": c.instance_id,
                        "definitionId": c.definition_id,
                        "name": self.card_name_by_def(&c.definition_id),
                        "level": self.catalog.get(&c.definition_id).map(|d| d.level).unwrap_or(0),
                        "ap": self.catalog.get(&c.definition_id).map(|d| d.ap).unwrap_or(0),
                        "colors": self.catalog.get(&c.definition_id).map(|d| d.colors.clone()).unwrap_or_default(),
                    }));
                }

                let dec_id = self.generate_decision_id();
                self.pending_decision = Some(json!({
                    "decisionId": dec_id,
                    "revision": self.revision,
                    "playerId": player_id,
                    "kind": "deckSearch",
                    "options": {
                        "actorId": actor_id,
                        "actorName": card_name,
                        "searchCount": search_count,
                        "title": format!("山札の上から{}枚めくりました。手札に加えるカードを1枚選んでください（残りはデッキ下へ）", search_count),
                        "revealedCards": candidates,
                        "optional": true
                    }
                }));
                self.log(format!("🔍 [{}] の効果で山札の上から {} 枚めくりました", card_name, search_count));
                return true;
            }
        }

        false
    }

    /// カットイン効果を解決する
    fn resolve_cutin_effects(&mut self, player_id: &str, definition_id: &str) {
        let desc = match self.catalog.get(definition_id) {
            Some(d) => d.description.clone(),
            None => return,
        };

        if desc.is_empty() || !desc.contains("【カットイン】") {
            return;
        }

        let card_name = self.card_name_by_def(definition_id);

        // カットイン後のドロー
        let draw_count = parse_draw_count(&desc);
        if draw_count > 0 {
            self.draw_cards(player_id, draw_count);
            self.log(format!("⚡ [{}] のカットイン効果: カードを {} 枚引きました", card_name, draw_count));
        }

        // カットイン後の証拠獲得
        let ev_count = parse_evidence_gain(&desc);
        if ev_count > 0 {
            self.gain_evidence(player_id, ev_count);
            self.log(format!("⚡ [{}] のカットイン効果: 証拠を {} つ獲得しました", card_name, ev_count));
        }

        // カットイン: 相手の現場キャラをスリープ
        if desc.contains("スリープさせる") {
            let opp_id = self.player_order.iter().find(|p| *p != player_id).cloned().unwrap_or_default();
            let mut target_name = None;
            if let Some(ctx) = &self.action_context {
                let actor_id = ctx.get("actorId").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if let Some(opp_ps) = self.players.get_mut(&opp_id) {
                    if let Some(c) = opp_ps.field.iter_mut().find(|c| c.instance_id == actor_id) {
                        c.orientation = CardOrientation::SLEEP;
                        let tid = c.definition_id.clone();
                        target_name = Some(self.card_name_by_def(&tid));
                    }
                }
            } else {
                let opp_id2 = opp_id.clone();
                if let Some(opp_ps) = self.players.get_mut(&opp_id2) {
                    for c in opp_ps.field.iter_mut() {
                        if c.orientation == CardOrientation::ACTIVE {
                            c.orientation = CardOrientation::SLEEP;
                            let tid = c.definition_id.clone();
                            target_name = Some(self.card_name_by_def(&tid));
                            break;
                        }
                    }
                }
            }
            if let Some(tname) = target_name {
                self.log(format!("⚡ [{}] のカットイン効果: [{}] をスリープさせました", card_name, tname));
            }
        }

        // カットイン: 相手の現場キャラをスタン
        if desc.contains("スタンさせる") {
            let opp_id = self.player_order.iter().find(|p| *p != player_id).cloned().unwrap_or_default();
            let mut target_name = None;
            if let Some(opp_ps) = self.players.get_mut(&opp_id) {
                for c in opp_ps.field.iter_mut() {
                    if c.orientation == CardOrientation::ACTIVE || c.orientation == CardOrientation::SLEEP {
                        c.orientation = CardOrientation::STUN;
                        let tid = c.definition_id.clone();
                        target_name = Some(self.card_name_by_def(&tid));
                        break;
                    }
                }
            }
            if let Some(tname) = target_name {
                self.log(format!("⚡ [{}] のカットイン効果: [{}] をスタンさせました", card_name, tname));
            }
        }

        // カットイン: APバフ（自分のキャラに）
        let ap_bonus = parse_ap_bonus(&desc);
        if ap_bonus > 0 {
            let mut buffed_def_id = None;
            if let Some(ps) = self.players.get_mut(player_id) {
                for c in ps.field.iter_mut() {
                    if c.orientation == CardOrientation::ACTIVE {
                        c.temp_ap_bonus += ap_bonus;
                        buffed_def_id = Some(c.definition_id.clone());
                        break;
                    }
                }
            }
            if let Some(did) = buffed_def_id {
                let my_card_name = self.card_name_by_def(&did);
                self.log(format!("⚡ [{}] のカットイン効果: [{}] のAP +{}", card_name, my_card_name, ap_bonus));
            }
        }

        // カットイン: リムーブ効果（相手の証拠をリムーブ）
        if desc.contains("証拠をリムーブ") || desc.contains("証拠を1つリムーブ") {
            let opp_id = self.player_order.iter().find(|p| *p != player_id).cloned().unwrap_or_default();
            if let Some(opp_ps) = self.players.get_mut(&opp_id) {
                if let Some(mut ev) = opp_ps.evidence.pop() {
                    ev.zone = Zone::REMOVE;
                    opp_ps.remove.push(ev);
                    self.log(format!("⚡ [{}] のカットイン効果: 相手の証拠を1枚リムーブしました！", card_name));
                }
            }
        }
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

    /// 効果テキストの発動条件（【パートナー色】および【FILE X】）を満たしているか判定する
    pub fn check_effect_conditions(&self, player_id: &str, desc: &str) -> bool {
        // 1. パートナー色条件チェック: 【パートナー青】, 【パートナー緑】, 【パートナー白】, 【パートナー赤】, 【パートナー黄】, 【パートナー黒】
        let partner_colors = self.get_player_partner_colors(player_id);
        for color in &["青", "緑", "白", "赤", "黄", "黒"] {
            let tag = format!("【パートナー{}】", color);
            if desc.contains(&tag) && !partner_colors.contains(*color) {
                return false;
            }
        }

        // 2. FILE条件チェック: 【FILE5】, 【FILE6】, 【FILE7】, 【FILE8】等
        if let Some(ps) = self.players.get(player_id) {
            let current_file = ps.effective_file_count();
            for req in 1..=10 {
                let tag = format!("【FILE{}】", req);
                if desc.contains(&tag) && current_file < req {
                    return false;
                }
            }
        }

        // 3. 【相手ターン中】条件チェック
        if desc.contains("【相手ターン中】") {
            let is_turn_player = self.turn_player_id == player_id;
            if is_turn_player {
                return false;
            }
        }

        // 4. 【自分ターン中】条件チェック
        if desc.contains("【自分ターン中】") {
            let is_turn_player = self.turn_player_id == player_id;
            if !is_turn_player {
                return false;
            }
        }

        // 5. 【特徴［〇〇］】条件チェック（自分の現場に該当特徴のキャラがいる場合）
        if desc.contains("自分の現場に") && desc.contains("特徴［") && (desc.contains("がいる場合") || desc.contains("がいるなら")) {
            let required_traits = extract_trait_filters(desc);
            if !required_traits.is_empty() {
                if let Some(ps) = self.players.get(player_id) {
                    let has_matching = ps.field.iter().any(|c| {
                        self.catalog.get(&c.definition_id).map(|def| {
                            required_traits.iter().any(|rt| def.traits.iter().any(|t| t == rt))
                        }).unwrap_or(false)
                    });
                    if !has_matching {
                        return false;
                    }
                }
            }
        }

        true
    }

    /// 現場リムーブ時トリガーを解決する
    pub fn trigger_on_remove(&mut self, owner_id: &str, definition_id: &str) {
        let desc = match self.catalog.get(definition_id) {
            Some(d) => d.description.clone(),
            None => return,
        };

        let is_on_remove = desc.contains("【現場リムーブ時】")
            || desc.contains("リムーブされたとき")
            || desc.contains("現場から離れたとき");

        if !is_on_remove {
            return;
        }

        // 条件判定（【相手ターン中】、【パートナー色】、【FILE】等）
        if !self.check_effect_conditions(owner_id, &desc) {
            return;
        }

        let card_name = self.card_name_by_def(definition_id);
        self.log(format!("💥 [{}] の【現場リムーブ時】能力が誘発しました！", card_name));

        // ドロー効果
        let draw_count = parse_draw_count(&desc);
        if draw_count > 0 {
            self.draw_cards(owner_id, draw_count);
            self.log(format!("✨ [{}] の効果でカードを {} 枚引きました", card_name, draw_count));
        }

        // 証拠獲得
        let ev_count = parse_evidence_gain(&desc);
        if ev_count > 0 {
            self.gain_evidence(owner_id, ev_count);
            self.log(format!("✨ [{}] の効果で証拠を {} つ獲得しました", card_name, ev_count));
        }

        // 相手キャラのスリープ
        if desc.contains("スリープさせる") {
            let opp_id = self.player_order.iter().find(|p| *p != owner_id).cloned().unwrap_or_default();
            let mut target_name = None;
            if let Some(opp_ps) = self.players.get_mut(&opp_id) {
                for c in opp_ps.field.iter_mut() {
                    if c.orientation == CardOrientation::ACTIVE {
                        c.orientation = CardOrientation::SLEEP;
                        target_name = Some(c.definition_id.clone());
                        break;
                    }
                }
            }
            if let Some(tid) = target_name {
                let tname = self.card_name_by_def(&tid);
                self.log(format!("💤 [{}] の効果で相手の [{}] をスリープさせました", card_name, tname));
            }
        }

        // 相手キャラのスタン
        if desc.contains("スタンさせる") {
            let opp_id = self.player_order.iter().find(|p| *p != owner_id).cloned().unwrap_or_default();
            let mut target_name = None;
            if let Some(opp_ps) = self.players.get_mut(&opp_id) {
                for c in opp_ps.field.iter_mut() {
                    if c.orientation == CardOrientation::ACTIVE || c.orientation == CardOrientation::SLEEP {
                        c.orientation = CardOrientation::STUN;
                        target_name = Some(c.definition_id.clone());
                        break;
                    }
                }
            }
            if let Some(tid) = target_name {
                let tname = self.card_name_by_def(&tid);
                self.log(format!("💫 [{}] の効果で相手の [{}] をスタンさせました", card_name, tname));
            }
        }

        // 手札からキャラ登場（例: 「手札からレベル6以下の〜登場させる」）
        if desc.contains("手札から") && desc.contains("登場させる") {
            let mut played_c = None;
            if let Some(ps) = self.players.get_mut(owner_id) {
                if ps.field.len() < 5 {
                    if let Some(idx) = ps.hand.iter().position(|c| c.zone == Zone::HAND) {
                        let mut card = ps.hand.remove(idx);
                        card.zone = Zone::FIELD;
                        card.orientation = CardOrientation::SLEEP;
                        card.face_up = true;
                        played_c = Some(card.definition_id.clone());
                        ps.field.push(card);
                    }
                }
            }
            if let Some(tid) = played_c {
                let tname = self.card_name_by_def(&tid);
                self.log(format!("🃏 [{}] の効果で手札から [{}] を登場させました！", card_name, tname));
            }
        }

        // リムーブエリアから蘇生（例: 「リムーブエリアにある〜スリープ状態で登場させる」）
        if desc.contains("リムーブエリアにある") && desc.contains("登場させる") {
            let mut revived_c = None;
            if let Some(ps) = self.players.get_mut(owner_id) {
                if ps.field.len() < 5 {
                    if let Some(idx) = ps.remove.iter().position(|c| c.zone == Zone::REMOVE) {
                        let mut card = ps.remove.remove(idx);
                        card.zone = Zone::FIELD;
                        card.orientation = CardOrientation::SLEEP;
                        card.face_up = true;
                        revived_c = Some(card.definition_id.clone());
                        ps.field.push(card);
                    }
                }
            }
            if let Some(tid) = revived_c {
                let tname = self.card_name_by_def(&tid);
                self.log(format!("✨ [{}] の効果でリムーブエリアから [{}] を登場させました！", card_name, tname));
            }
        }
    }

    /// ターン終了時トリガーを解決する
    pub fn trigger_on_turn_end(&mut self, current_turn_pid: &str) {
        // 現場にいるキャラ・パートナーのターン終了時効果をチェック
        let mut effects_to_trigger = vec![];
        if let Some(ps) = self.players.get(current_turn_pid) {
            for c in &ps.field {
                if let Some(c_def) = self.catalog.get(&c.definition_id) {
                    if c_def.description.contains("ターン終了時") {
                        effects_to_trigger.push((current_turn_pid.to_string(), c.definition_id.clone(), c_def.description.clone()));
                    }
                }
            }
            if let Some(ref p) = ps.partner {
                if let Some(p_def) = self.catalog.get(&p.definition_id) {
                    if p_def.description.contains("ターン終了時") {
                        effects_to_trigger.push((current_turn_pid.to_string(), p.definition_id.clone(), p_def.description.clone()));
                    }
                }
            }
        }

        for (pid, def_id, desc) in effects_to_trigger {
            if !self.check_effect_conditions(&pid, &desc) {
                continue;
            }

            let card_name = self.card_name_by_def(&def_id);

            // ドロー効果
            let draw_count = parse_draw_count(&desc);
            if draw_count > 0 {
                self.draw_cards(&pid, draw_count);
                self.log(format!("✨ [{}] のターン終了時効果: カードを {} 枚引きました", card_name, draw_count));
            }

            // 手札リムーブ効果（例: 「手札を1枚リムーブする」）
            let hand_cost = parse_hand_remove_cost(&desc);
            if hand_cost > 0 {
                let mut removed_cost_names = vec![];
                if let Some(ps) = self.players.get_mut(&pid) {
                    for _ in 0..hand_cost {
                        if let Some(mut removed_c) = ps.hand.pop() {
                            removed_c.zone = Zone::REMOVE;
                            let did = removed_c.definition_id.clone();
                            ps.remove.push(removed_c);
                            removed_cost_names.push(did);
                        }
                    }
                }
                for did in removed_cost_names {
                    let cost_name = self.card_name_by_def(&did);
                    self.log(format!("💸 [{}] のターン終了時効果: 手札から [{}] をリムーブしました", card_name, cost_name));
                }
            }

            // 証拠リムーブ（例: 「自分の証拠を上から1つリムーブする」）
            if desc.contains("証拠を上から1つリムーブ") || desc.contains("証拠を1つリムーブ") {
                if let Some(ps) = self.players.get_mut(&pid) {
                    if let Some(mut ev) = ps.evidence.pop() {
                        ev.zone = Zone::REMOVE;
                        ps.remove.push(ev);
                        self.log(format!("⚠️ [{}] のターン終了時効果: 証拠を1つリムーブしました", card_name));
                    }
                }
            }
        }
    }

    /// アクション宣言時トリガーを解決する（「このキャラがアクションしたとき」「〚特徴［〇〇］〛のキャラがアクションしたとき」）
    pub fn trigger_on_action(&mut self, player_id: &str, actor_id: &str) {
        let actor_def_id = match self.find_card_def_id(actor_id) {
            Some(d) => d,
            None => return,
        };
        let actor_traits = self.catalog.get(&actor_def_id).map(|d| d.traits.clone()).unwrap_or_default();
        let actor_name = self.card_name_by_def(&actor_def_id);

        // 1. 攻撃キャラ自身の「このキャラがアクションしたとき」
        if let Some(actor_desc) = self.catalog.get(&actor_def_id).map(|d| d.description.clone()) {
            if actor_desc.contains("このキャラがアクションしたとき") && self.check_effect_conditions(player_id, &actor_desc) {
                let draw_n = parse_draw_count(&actor_desc);
                if draw_n > 0 {
                    self.draw_cards(player_id, draw_n);
                    self.log(format!("🎴 [{}] のアクション時効果: カードを{}枚引きました", actor_name, draw_n));
                }
                let ap_bonus = parse_ap_bonus(&actor_desc);
                if ap_bonus > 0 {
                    if let Some(ps) = self.players.get_mut(player_id) {
                        if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == actor_id) {
                            c.temp_ap_bonus += ap_bonus;
                        }
                    }
                    self.log(format!("✨ [{}] のアクション時効果: AP +{}", actor_name, ap_bonus));
                }
            }
        }

        // 2. 自分の場の他キャラの「〚特徴［〇〇］〛のキャラがアクションしたとき」
        let field_cards: Vec<(String, String)> = self.players.get(player_id)
            .map(|ps| ps.field.iter().map(|c| (c.instance_id.clone(), c.definition_id.clone())).collect())
            .unwrap_or_default();

        for (_cid, def_id) in field_cards {
            let desc = match self.catalog.get(&def_id) {
                Some(d) => d.description.clone(),
                None => continue,
            };
            if !desc.contains("のキャラがアクションしたとき") {
                continue;
            }
            if !self.check_effect_conditions(player_id, &desc) {
                continue;
            }
            let req_traits = extract_trait_filters(&desc);
            if !req_traits.is_empty() && !req_traits.iter().any(|rt| actor_traits.iter().any(|t| t == rt)) {
                continue;
            }
            let watcher_name = self.card_name_by_def(&def_id);
            let draw_n = parse_draw_count(&desc);
            if draw_n > 0 {
                self.draw_cards(player_id, draw_n);
                self.log(format!("🎴 [{}] の誘発効果（特徴一致アクション）: カードを{}枚引きました", watcher_name, draw_n));
            }
            let ap_bonus = parse_ap_bonus(&desc);
            if ap_bonus > 0 {
                if let Some(ps) = self.players.get_mut(player_id) {
                    if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == actor_id) {
                        c.temp_ap_bonus += ap_bonus;
                    }
                }
                self.log(format!("✨ [{}] の誘発効果（特徴一致アクション）: [{}] のAP +{}", watcher_name, actor_name, ap_bonus));
            }
        }
    }

    /// コンタクト突入時トリガーを解決する
    pub fn trigger_on_contact(&mut self, attacker_pid: &str, actor_id: &str, defender_pid: &str, def_cid: &str) {
        // 攻撃側「このキャラがコンタクトしたとき」
        if let Some(a_def_id) = self.find_card_def_id(actor_id) {
            let desc = self.catalog.get(&a_def_id).map(|d| d.description.clone()).unwrap_or_default();
            if desc.contains("このキャラがコンタクトしたとき") && self.check_effect_conditions(attacker_pid, &desc) {
                let ap_bonus = parse_ap_bonus(&desc);
                if ap_bonus > 0 {
                    if let Some(ps) = self.players.get_mut(attacker_pid) {
                        if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == actor_id) {
                            c.temp_ap_bonus += ap_bonus;
                        }
                    }
                    let name = self.card_name_by_def(&a_def_id);
                    self.log(format!("⚔️ [{}] のコンタクト時効果: AP +{}", name, ap_bonus));
                }
            }
        }

        // 防御側「このキャラを指定してアクションしたとき」
        if let Some(d_def_id) = self.find_card_def_id(def_cid) {
            let desc = self.catalog.get(&d_def_id).map(|d| d.description.clone()).unwrap_or_default();
            if desc.contains("このキャラを指定してアクションしたとき") && self.check_effect_conditions(defender_pid, &desc) {
                let ap_bonus = parse_ap_bonus(&desc);
                if ap_bonus > 0 {
                    if let Some(ps) = self.players.get_mut(defender_pid) {
                        if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == def_cid) {
                            c.temp_ap_bonus += ap_bonus;
                        }
                    }
                    let name = self.card_name_by_def(&d_def_id);
                    self.log(format!("🛡️ [{}] の被コンタクト時効果: AP +{}", name, ap_bonus));
                }
            }
        }
    }

    /// コンタクトによる相手キャラリムーブ時のトリガーを解決する
    pub fn trigger_on_contact_win(&mut self, attacker_pid: &str, actor_id: &str, actor_def_id: &str) {
        let desc = match self.catalog.get(actor_def_id) {
            Some(d) => d.description.clone(),
            None => return,
        };
        if !desc.contains("コンタクトによってリムーブされたとき") {
            return;
        }
        if !self.check_effect_conditions(attacker_pid, &desc) {
            return;
        }

        let card_name = self.card_name_by_def(actor_def_id);

        // 1. ドロー
        let draw_n = parse_draw_count(&desc);
        if draw_n > 0 {
            self.draw_cards(attacker_pid, draw_n);
            self.log(format!("🎴 [{}] のコンタクト勝利時効果: カードを{}枚引きました", card_name, draw_n));
        }

        // 2. 証拠獲得（手札リムーブ任意コスト付きの場合も含む）
        let ev_n = parse_evidence_gain(&desc);
        if ev_n > 0 {
            let hand_cost = parse_hand_remove_cost(&desc);
            if hand_cost > 0 {
                let can_pay = self.players.get(attacker_pid).map(|p| p.hand.len() >= hand_cost).unwrap_or(false);
                if can_pay {
                    if let Some(ps) = self.players.get_mut(attacker_pid) {
                        for _ in 0..hand_cost {
                            if let Some(mut c) = ps.hand.pop() {
                                c.zone = Zone::REMOVE;
                                ps.remove.push(c);
                            }
                        }
                    }
                    self.gain_evidence(attacker_pid, ev_n);
                    self.log(format!("🔍 [{}] のコンタクト勝利時効果: 手札を{}枚リムーブし証拠を{}つ獲得！", card_name, hand_cost, ev_n));
                }
            } else {
                self.gain_evidence(attacker_pid, ev_n);
                self.log(format!("🔍 [{}] のコンタクト勝利時効果: 証拠を{}つ獲得！", card_name, ev_n));
            }
        }

        // 3. 突撃付与
        if desc.contains("突撃") && (desc.contains("持つ") || desc.contains("得る")) {
            let r_target = parse_rush_target(&desc);
            if let Some(ps) = self.players.get_mut(attacker_pid) {
                if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == actor_id) {
                    c.has_rush = true;
                    c.rush_target = r_target;
                }
            }
            self.log(format!("⚡ [{}] のコンタクト勝利時効果: ターン終了時まで〚突撃〛を獲得！", card_name));
        }
    }
}
