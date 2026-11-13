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

        // Opponent gains 1 evidence
        self.gain_evidence(&other_id, 1);
    }

    pub fn finish_match(&mut self, winner_id: &str, reason: &str) {
        self.phase = MatchPhase::FINISHED;
        self.winner_id = Some(winner_id.to_string());
        self.finish_reason = Some(reason.to_string());
        self.pending_decision = None;
    }

    fn prompt_main_action(&mut self, player_id: &str) {
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
        let case_colors: HashSet<String> = case_def.map(|d| d.colors.iter().cloned().collect()).unwrap_or_default();

        let mut actions: Vec<Value> = vec![];

        // 1. End turn
        actions.push(json!({"type": "END_TURN"}));

        // 2. Hand Play
        if !ps.normal_hand_played_this_turn && !ps.hint_played_this_turn {
            let mut playable_cards = vec![];
            for c in &ps.hand {
                if let Some(c_def) = self.catalog.get(&c.definition_id) {
                    let card_colors: HashSet<String> = c_def.colors.iter().cloned().collect();
                    if c_def.level <= effective_file && card_colors.is_subset(&case_colors) {
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
            if c.orientation == CardOrientation::ACTIVE && c.entered_turn != self.turn {
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
        let mut action_actors = vec![];
        for c in &ps.field {
            if c.orientation == CardOrientation::ACTIVE && c.entered_turn != self.turn {
                action_actors.push(c.instance_id.clone());
            }
        }

        let mut target_ids = vec![];
        for opp_c in &opp_ps.field {
            if opp_c.orientation == CardOrientation::SLEEP || opp_c.orientation == CardOrientation::STUN {
                target_ids.push(opp_c.instance_id.clone());
            }
        }
        if !opp_ps.evidence.is_empty() {
            if let Some(ref opp_case) = opp_ps.r#case {
                target_ids.push(opp_case.instance_id.clone());
            }
        }

        if !action_actors.is_empty() && !target_ids.is_empty() {
            actions.push(json!({
                "type": "ACTION",
                "actorIds": action_actors,
                "targetIds": target_ids
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
                let next_turn = self.turn + 1;
                let next_pid = self.player_order.iter().find(|pid| *pid != player_id).unwrap().clone();
                self.start_turn(next_turn, &next_pid);
            }
            "HAND_PLAY" => {
                let card_id = val.get("cardId").and_then(|v| v.as_str()).unwrap_or("");
                let turn = self.turn;
                if let Some(ps) = self.players.get_mut(player_id) {
                    if let Some(idx) = ps.hand.iter().position(|c| c.instance_id == card_id) {
                        let mut card = ps.hand.remove(idx);
                        card.zone = Zone::FIELD;
                        card.orientation = CardOrientation::ACTIVE;
                        card.entered_turn = turn;
                        card.face_up = true;
                        ps.field.push(card);
                        ps.normal_hand_played_this_turn = true;
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
                    let case_colors: HashSet<String> = case_def.map(|d| d.colors.iter().cloned().collect()).unwrap_or_default();

                    if ps.field.len() < 5 {
                        for c in &ps.hand {
                            if let Some(c_def) = self.catalog.get(&c.definition_id) {
                                let card_colors: HashSet<String> = c_def.colors.iter().cloned().collect();
                                if c_def.level <= effective_file && card_colors.is_subset(&case_colors) {
                                    playable_cards.push(c.instance_id.clone());
                                }
                            }
                        }
                    }
                }

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
                    if ps.effective_file_count() >= 7 {
                        ps.case_phase = CasePhase::RESOLUTION;
                    }
                }
                self.prompt_main_action(player_id);
            }
            "RESOLVE_CASE" => {
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

                if lp > 0 {
                    self.gain_evidence(player_id, lp as usize);
                }
                self.prompt_main_action(player_id);
            }
            "ACTION" => {
                let actor_id = val.get("actorId").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let target_id = val.get("targetId").and_then(|v| v.as_str()).unwrap_or("").to_string();

                if let Some(ps) = self.players.get_mut(player_id) {
                    if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == actor_id) {
                        c.orientation = CardOrientation::SLEEP;
                    }
                }

                let opp_pid = self.player_order.iter().find(|p| *p != player_id).unwrap().clone();
                let opp_ps = self.players.get(&opp_pid).unwrap();

                self.action_context = Some(json!({
                    "actorId": actor_id,
                    "targetId": target_id,
                    "attackerPlayerId": player_id,
                    "defenderPlayerId": opp_pid
                }));

                let guards: Vec<String> = opp_ps.field.iter()
                    .filter(|c| c.orientation == CardOrientation::ACTIVE)
                    .map(|c| c.instance_id.clone())
                    .collect();

                if !guards.is_empty() {
                    let dec_id = self.generate_decision_id();
                    self.pending_decision = Some(json!({
                        "decisionId": dec_id,
                        "revision": self.revision,
                        "playerId": opp_pid,
                        "kind": "guard",
                        "options": {
                            "guardCardIds": guards
                        }
                    }));
                } else {
                    self.proceed_to_action_resolution(None);
                }
            }
            _ => {
                self.prompt_main_action(player_id);
            }
        }
    }

    fn handle_hint_play_answer(&mut self, player_id: &str, val: &Value) {
        if let Some(card_id) = val.get("cardId").and_then(|v| v.as_str()) {
            let turn = self.turn;
            if let Some(ps) = self.players.get_mut(player_id) {
                if ps.field.len() < 5 {
                    if let Some(idx) = ps.hand.iter().position(|c| c.instance_id == card_id) {
                        let mut card = ps.hand.remove(idx);
                        card.zone = Zone::FIELD;
                        card.orientation = CardOrientation::ACTIVE;
                        card.entered_turn = turn;
                        card.face_up = true;
                        ps.field.push(card);
                    }
                }
            }
        }
        self.prompt_main_action(player_id);
    }

    fn handle_guard_answer(&mut self, _player_id: &str, val: &Value) {
        let guard_id = val.get("guardCardId").and_then(|v| v.as_str()).map(|s| s.to_string());
        self.proceed_to_action_resolution(guard_id.as_deref());
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
            }
        } else if let Some(def_cid) = target_field_card_id {
            let actor_def_id = self.players.get(&attacker_pid)
                .and_then(|p| p.field.iter().find(|c| c.instance_id == actor_id).map(|c| c.definition_id.clone()));
            let def_def_id = self.players.get(&defender_pid)
                .and_then(|p| p.field.iter().find(|c| c.instance_id == def_cid).map(|c| c.definition_id.clone()));

            if let (Some(a_def_id), Some(d_def_id)) = (actor_def_id, def_def_id) {
                let actor_ap = self.catalog.get(&a_def_id).map(|d| d.ap).unwrap_or(0);
                let def_ap = self.catalog.get(&d_def_id).map(|d| d.ap).unwrap_or(0);

                if actor_ap >= def_ap {
                    let defender_ps = self.players.get_mut(&defender_pid).unwrap();
                    if let Some(idx) = defender_ps.field.iter().position(|c| c.instance_id == def_cid) {
                        let mut removed_c = defender_ps.field.remove(idx);
                        removed_c.zone = Zone::REMOVE;
                        defender_ps.remove.push(removed_c);
                    }
                }
            }
        }

        self.prompt_main_action(&attacker_pid);
    }
}
