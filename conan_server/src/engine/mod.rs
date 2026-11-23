#![allow(unused_imports)]
mod actions;
mod combat;
mod decisions;
mod executor;
mod stats;
mod target_ops;
mod triggers;
mod turn;

use crate::catalog::CardCatalog;
use crate::effects::{
    allows_partner_area, branch_matches, branch_matches_with_ctx, card_effective_level,
    card_is_untargetable_by_opponent, card_matches_filter_with_ctx, check_action_trigger_match,
    check_ally_play_match, check_char_removed_match, check_clause_conditions,
    check_disguise_conditions,
    check_event_play_conditions, check_hint_use_match, clause_turn_limit,
    compute_passive_ap_for_target, compute_passive_lp_bonus, eval_cond_phrase,
    evaluate_passive_rush_and_swift,
    extract_bracketed_value, extract_clauses, extract_distinct_card_names,
    has_instance_name_match, has_trait_match,
    merge_rush_capabilities, parse_clause_cost, parse_effect_steps, parse_event_mode_choice,
    resolve_state_branch, strip_parens, ActionEventKind, DeckPickAction,
    DeckSearchStep, EventModeChoice, ImmediateOp, OwnerScope, ParsedClausePlan,
    PassiveEvalContext, PlayerCondInfo, TargetAction, TargetBonus, TargetFilter, TargetStep,
    TargetZone, TriggerKind,
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
    pub extra_partners: Vec<CardInstance>,

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
            extra_partners: vec![],
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
pub struct PendingClauseTask {
    pub player_id: String,
    pub actor_id: String,
    pub actor_name: String,
    pub source_level: Option<i32>,
    pub card_type: CardType,
    pub clause: String,
    pub resume_context: String,
    pub evidence_source_inst_id: Option<String>,
    pub post_target_ops: Vec<ImmediateOp>,
    pub plan: Option<ParsedClausePlan>,
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
    /// コンタクト中のみ有効なAPバフ（カットイン等）。コンタクト終了時にクリアされる
    pub contact_ap_buffs: HashMap<String, i32>,
    pub contact_removed_actors_this_turn: HashSet<String>,
    pub turn_clause_usage: HashMap<String, usize>,
    pub turn_field_plays_by_player: HashMap<String, usize>,
    pub turn_disguise_blocked_players: HashSet<String>,
    pub turn_hint_blocked_players: HashSet<String>,
    pub turn_cutin_blocked_players: HashSet<String>,
    pub turn_event_blocked_players: HashSet<String>,
    /// 「このターン中、自分は〚カード名［X］〛を使用できず、登場させられない」によるターン内BAN（player_id -> カード名一覧）
    pub turn_name_bans: HashMap<String, Vec<String>>,
    pub is_executing_clause_plan: bool,
    pub refreshed_players: HashSet<String>,
    pub self_entered_field_players_this_turn: HashSet<String>,
    pub acted_this_turn_ids: HashSet<String>,
    pub granted_on_remove_draw_until_opp_end: HashMap<String, String>,
    pub granted_abilities_this_turn: HashMap<String, Vec<String>>,
    pub played_by_effect_events: bool,
    pub last_deployed_count: usize,
    pub last_removed_black_cutin_count: usize,
    pub last_chosen_added_to_hand: bool,
    pub last_chosen_level: Option<i32>,
    pub last_chosen_name: Option<String>,
    pub last_chosen_instance_id: Option<String>,
    pub last_deployed_instance_id: Option<String>,
    pub last_cost_flipped_evidence_count: usize,
    pub last_removed_level_sum: i32,
    pub last_removed_ap_sum: i32,
    pub last_removed_char_state: Option<(String, CardOrientation, i32, i32)>,
    pub last_disguise_replaced_name: Option<String>,
    pub last_slept_names: Vec<String>,
    pub next_opp_evidence_remove_triggers: Vec<(String, String, String, String)>,
    pub action_hirameki_blocked_player: Option<String>,
    pub pending_hint_trigger: Option<(String, i32, Vec<String>)>,
    pub pending_ally_play: Vec<(String, String, String, CardOrientation, Option<i32>, bool)>,
    pub pending_clause_queue: Vec<PendingClauseTask>,
    pub pending_enter_field_queue: Vec<(String, String, String, CardOrientation, Option<i32>, String)>,
    pub pending_trigger_order_tasks: Vec<PendingClauseTask>,
    pub field_overflow_excluded_ids: HashSet<String>,
    pub logs: Vec<String>,
    pub pending_file_logs: Vec<String>,
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
            contact_ap_buffs: HashMap::new(),
            contact_removed_actors_this_turn: HashSet::new(),
            turn_clause_usage: HashMap::new(),
            turn_field_plays_by_player: HashMap::new(),
            turn_disguise_blocked_players: HashSet::new(),
            turn_hint_blocked_players: HashSet::new(),
            turn_cutin_blocked_players: HashSet::new(),
            turn_event_blocked_players: HashSet::new(),
            turn_name_bans: HashMap::new(),
            is_executing_clause_plan: false,
            refreshed_players: HashSet::new(),
            self_entered_field_players_this_turn: HashSet::new(),
            acted_this_turn_ids: HashSet::new(),
            granted_on_remove_draw_until_opp_end: HashMap::new(),
            granted_abilities_this_turn: HashMap::new(),
            played_by_effect_events: false,
            last_deployed_count: 0,
            last_removed_black_cutin_count: 0,
            last_chosen_added_to_hand: false,
            last_chosen_level: None,
            last_chosen_name: None,
            last_chosen_instance_id: None,
            last_deployed_instance_id: None,
            last_cost_flipped_evidence_count: 0,
            last_removed_level_sum: 0,
            last_removed_ap_sum: 0,
            last_removed_char_state: None,
            last_disguise_replaced_name: None,
            last_slept_names: vec![],
            next_opp_evidence_remove_triggers: vec![],
            action_hirameki_blocked_player: None,
            pending_hint_trigger: None,
            pending_ally_play: Vec::new(),
            pending_clause_queue: vec![],
            pending_enter_field_queue: vec![],
            pending_trigger_order_tasks: vec![],
            field_overflow_excluded_ids: HashSet::new(),
            logs: vec![],
            pending_file_logs: vec![],
        }
    }

    pub fn find_card_def_id_in_set(&self, ps: &PlayerState, instance_id: &str) -> Option<String> {
        for c in &ps.extra_partners {
            if c.instance_id == instance_id { return Some(c.definition_id.clone()); }
        }
        for c in &ps.field {
            for sc in &c.set_cards {
                if sc.instance_id == instance_id { return Some(sc.definition_id.clone()); }
            }
        }
        None
    }

    /// UIには出さず、ファイルログ（logs/all_rooms.log）にのみ記録するデバッグログ
    pub fn debug_log(&mut self, msg: impl Into<String>) {
        self.pending_file_logs.push(msg.into());
    }

    pub fn log(&mut self, msg: impl Into<String>) {
        let msg_str = msg.into();
        self.pending_file_logs.push(msg_str.clone());
        self.logs.push(msg_str);
        if self.logs.len() > 500 {
            self.logs.drain(0..100);
        }
    }

    pub fn card_name_by_def(&self, def_id: &str) -> String {
        self.catalog
            .get(def_id)
            .and_then(|d| d.names.first().cloned())
            .map(|name| format!("{}({})", name, def_id))
            .unwrap_or_else(|| def_id.to_string())
    }

    /// 指定されたカード定義IDがMR（レベル9キャラ / パートナーエリア能力持ち）か判定する
    pub fn is_mr_card(&self, def_id: &str) -> bool {
        Self::is_mr_def(&self.catalog, def_id)
    }

    pub fn is_mr_def(catalog: &CardCatalog, def_id: &str) -> bool {
        catalog
            .get(def_id)
            .map(|d| {
                d.card_type == CardType::CHARACTER
                    && (d.level >= 9 || d.description.contains("パートナーエリアでも"))
            })
            .unwrap_or(false)
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
            if let Some(d) = self.find_card_def_id_in_set(ps, instance_id) {
                return Some(d);
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

    pub(self) fn generate_instance_id(&mut self) -> String {
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
            ..Default::default()
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
            ..Default::default()
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
                    ..Default::default()
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
}
