use crate::catalog::CardCatalog;
use crate::effects::{
    allows_partner_area, branch_matches, branch_matches_with_ctx, card_effective_level,
    card_is_untargetable_by_opponent, card_matches_filter_with_ctx, check_action_trigger_match,
    check_ally_play_match, check_clause_conditions, check_disguise_conditions,
    check_event_play_conditions, check_hint_use_match, clause_turn_limit,
    compute_passive_ap_for_target, compute_passive_lp_bonus, eval_cond_phrase,
    evaluate_passive_rush_and_swift,
    extract_bracketed_value, extract_clauses, has_instance_name_match, has_trait_match,
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
    pub turn_clause_usage: HashMap<String, usize>,
    pub turn_field_plays_by_player: HashMap<String, usize>,
    pub turn_disguise_blocked_players: HashSet<String>,
    pub turn_hint_blocked_players: HashSet<String>,
    pub turn_cutin_blocked_players: HashSet<String>,
    pub turn_event_blocked_players: HashSet<String>,
    pub refreshed_players: HashSet<String>,
    pub self_entered_field_players_this_turn: HashSet<String>,
    pub acted_this_turn_ids: HashSet<String>,
    pub granted_on_remove_draw_until_opp_end: HashMap<String, String>,
    pub played_by_effect_events: bool,
    pub last_deployed_count: usize,
    pub last_removed_black_cutin_count: usize,
    pub last_chosen_added_to_hand: bool,
    pub last_chosen_level: Option<i32>,
    pub last_chosen_name: Option<String>,
    pub last_chosen_instance_id: Option<String>,
    pub last_cost_flipped_evidence_count: usize,
    pub last_removed_level_sum: i32,
    pub last_removed_ap_sum: i32,
    pub last_disguise_replaced_name: Option<String>,
    pub last_slept_names: Vec<String>,
    pub next_opp_evidence_remove_triggers: Vec<(String, String, String, String)>,
    pub action_hirameki_blocked_player: Option<String>,
    pub pending_hint_trigger: Option<(String, i32, Vec<String>)>,
    pub pending_ally_play: Vec<(String, String, String, CardOrientation, Option<i32>, bool)>,
    pub pending_clause_queue: Vec<PendingClauseTask>,
    pub pending_enter_field_queue: Vec<(String, String, String, CardOrientation, Option<i32>, String)>,
    pub pending_trigger_order_tasks: Vec<PendingClauseTask>,
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
            turn_clause_usage: HashMap::new(),
            turn_field_plays_by_player: HashMap::new(),
            turn_disguise_blocked_players: HashSet::new(),
            turn_hint_blocked_players: HashSet::new(),
            turn_cutin_blocked_players: HashSet::new(),
            turn_event_blocked_players: HashSet::new(),
            refreshed_players: HashSet::new(),
            self_entered_field_players_this_turn: HashSet::new(),
            acted_this_turn_ids: HashSet::new(),
            granted_on_remove_draw_until_opp_end: HashMap::new(),
            played_by_effect_events: false,
            last_deployed_count: 0,
            last_removed_black_cutin_count: 0,
            last_chosen_added_to_hand: false,
            last_chosen_level: None,
            last_chosen_name: None,
            last_chosen_instance_id: None,
            last_cost_flipped_evidence_count: 0,
            last_removed_level_sum: 0,
            last_removed_ap_sum: 0,
            last_disguise_replaced_name: None,
            last_slept_names: vec![],
            next_opp_evidence_remove_triggers: vec![],
            action_hirameki_blocked_player: None,
            pending_hint_trigger: None,
            pending_ally_play: Vec::new(),
            pending_clause_queue: vec![],
            pending_enter_field_queue: vec![],
            pending_trigger_order_tasks: vec![],
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
            "modeSelect" => self.handle_mode_select_answer(player_id, &val, &dec),
            "triggerOrder" => self.handle_trigger_order_answer(player_id, &val),
            _ => return Err(format!("Unknown decision kind: {}", kind)),
        }

        Ok(())
    }

    fn drain_pending_clause_queue(&mut self) -> bool {
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

    fn start_event_mode_choice(
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

    fn execute_event_mode_choice_body(
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

    fn can_pay_optional_mode_cost(&self, player_id: &str, opt_cost_text: &str) -> bool {
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

    fn pay_optional_mode_cost(&mut self, player_id: &str, opt_cost_text: &str) -> bool {
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

    fn enqueue_mode_clauses(
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

    fn handle_mode_select_answer(&mut self, player_id: &str, val: &Value, dec: &Value) {
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
                for did in revealed_def_ids {
                    revealed_names.push(self.card_name_by_def(&did));
                }
                self.log(format!(
                    "🔎 [{}] 〚捜査{}〛 指定名「{}」 ➔ 公開: [{}] ({})",
                    actor_name,
                    count,
                    guessed_name,
                    revealed_names.join(", "),
                    if found { "発見！" } else { "見つからず" }
                ));
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

    fn reset_turn_card_modifiers(c: &mut CardInstance) {
        c.temp_ap_bonus = 0;
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

    fn start_turn(&mut self, turn: i32, turn_player_id: &str) {
        self.turn = turn;
        self.turn_player_id = turn_player_id.to_string();
        self.turn_clause_usage.clear();
        self.turn_field_plays_by_player.clear();
        self.turn_disguise_blocked_players.clear();
        self.turn_hint_blocked_players.clear();
        self.turn_cutin_blocked_players.clear();
        self.turn_event_blocked_players.clear();
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

        // 2. Hand Play (キャラ・イベント合わせて通常手札使用は1ターン1回、キャラは現場5枚未満)
        {
            let cond_ctx = self.build_cond_info(player_id, None, CardOrientation::ACTIVE, None);
            let can_normal_play = !ps.normal_hand_played_this_turn && !ps.hint_played_this_turn;
            let can_play_char = can_normal_play;
            let event_blocked = self.turn_event_blocked_players.contains(player_id);
            let mut playable_cards = vec![];
            if can_normal_play {
                for c in &ps.hand {
                    if let Some(c_def) = self.catalog.get(&c.definition_id) {
                        let card_colors = normalize_color_set(&c_def.colors);
                        let color_ok = card_colors.is_empty() || case_colors.is_empty() || card_colors.is_subset(&case_colors);
                        let eff_lv = if c_def.card_type == CardType::EVENT {
                            card_effective_level(c_def, &cond_ctx)
                        } else {
                            c_def.level
                        };
                        if eff_lv <= effective_file && color_ok {
                            if c_def.card_type == CardType::EVENT {
                                if !event_blocked && check_event_play_conditions(&c_def.description, &cond_ctx).is_ok() {
                                    playable_cards.push(c.instance_id.clone());
                                }
                            } else if can_play_char {
                                playable_cards.push(c.instance_id.clone());
                            }
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
        if !ps.file.is_empty() && !self.turn_hint_blocked_players.contains(player_id) {
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
            // 突撃（has_rush）は登場ターンの「アクション」のみ可能であり、「迅速（SWIFT）」がある場合のみ登場ターンに推理も可能
            let can_deduce = c.orientation == CardOrientation::ACTIVE
                && (c.entered_turn != self.turn || self.effective_swift(c));
            if can_deduce {
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
        let mut active_opp_char_ids = vec![];
        for opp_c in &opp_ps.field {
            if opp_c.orientation == CardOrientation::SLEEP || opp_c.orientation == CardOrientation::STUN {
                char_target_ids.push(opp_c.instance_id.clone());
            } else if opp_c.orientation == CardOrientation::ACTIVE {
                active_opp_char_ids.push(opp_c.instance_id.clone());
            }
        }
        let mut case_target_id: Option<String> = None;
        if !opp_ps.evidence.is_empty() {
            if let Some(ref opp_case) = opp_ps.r#case {
                case_target_id = Some(opp_case.instance_id.clone());
            }
        }

        let mut all_target_ids = char_target_ids.clone();
        for aid in &active_opp_char_ids {
            all_target_ids.push(aid.clone());
        }
        if let Some(ref cid) = case_target_id {
            all_target_ids.push(cid.clone());
        }

        let mut action_actors = vec![];
        let mut dbg_lines: Vec<String> = Vec::new();
        let mut actor_target_map: HashMap<String, Vec<String>> = HashMap::new();
        for c in &ps.field {
            let (has_rush, rush_target) = self.effective_rush(c);
            let can_act = c.orientation == CardOrientation::ACTIVE && (c.entered_turn != self.turn || has_rush);
            if c.entered_turn == self.turn || !c.set_cards.is_empty() {
                dbg_lines.push(format!(
                    "[DEBUG][ACTION_CHECK] turn={} player={} card={}({}) ori={:?} entered={} set_cards={} has_rush={} rush_target={:?} can_act={} case_target={:?} opp_sleep_chars={}",
                    self.turn,
                    player_id,
                    c.definition_id,
                    c.instance_id,
                    c.orientation,
                    c.entered_turn,
                    c.set_cards.len(),
                    has_rush,
                    rush_target,
                    can_act,
                    case_target_id,
                    char_target_ids.len(),
                ));
            }
            if !can_act {
                continue;
            }
            let can_atk_active = self.effective_can_attack_active(c);
            let mut valid_char_targets = char_target_ids.clone();
            if can_atk_active {
                valid_char_targets.extend(active_opp_char_ids.iter().cloned());
            }
            let valid_targets = if c.entered_turn == self.turn && has_rush {
                match rush_target.as_deref() {
                    Some("CHAR") => valid_char_targets,
                    Some("CHAR_LV4") => valid_char_targets
                        .into_iter()
                        .filter(|tid| {
                            opp_ps
                                .field
                                .iter()
                                .find(|oc| &oc.instance_id == tid)
                                .and_then(|oc| {
                                    self.catalog
                                        .get(&oc.definition_id)
                                        .map(|od| (od.level + oc.temp_level_delta) <= 4)
                                })
                                .unwrap_or(false)
                        })
                        .collect(),
                    Some("CHAR_LV4+CASE") => {
                        let mut v: Vec<String> = valid_char_targets
                            .into_iter()
                            .filter(|tid| {
                                opp_ps
                                    .field
                                    .iter()
                                    .find(|oc| &oc.instance_id == tid)
                                    .and_then(|oc| {
                                        self.catalog
                                            .get(&oc.definition_id)
                                            .map(|od| (od.level + oc.temp_level_delta) <= 4)
                                    })
                                    .unwrap_or(false)
                            })
                            .collect();
                        if let Some(ref cid) = case_target_id {
                            v.push(cid.clone());
                        }
                        v
                    }
                    Some("CASE") => case_target_id.iter().cloned().collect(),
                    _ => {
                        let mut v = valid_char_targets;
                        if let Some(ref cid) = case_target_id {
                            v.push(cid.clone());
                        }
                        v
                    }
                }
            } else {
                let mut v = valid_char_targets;
                if let Some(ref cid) = case_target_id {
                    v.push(cid.clone());
                }
                v
            };
            if !valid_targets.is_empty() {
                action_actors.push(c.instance_id.clone());
                actor_target_map.insert(c.instance_id.clone(), valid_targets);
            } else if c.entered_turn == self.turn || !c.set_cards.is_empty() {
                dbg_lines.push(format!(
                    "[DEBUG][ACTION_SKIP] turn={} player={} card={}({}) valid_targets is empty",
                    self.turn, player_id, c.definition_id, c.instance_id
                ));
            }
        }
        for l in dbg_lines {
            self.pending_file_logs.push(l);
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
        let can_declare_clause = |actor_id: &str, actor_ori: CardOrientation, used_turn: bool, in_partner_area: bool, in_hand: bool, def: &crate::models::CardDefinition| -> bool {
            let clauses = extract_clauses(&def.description, TriggerKind::Declare, def.card_type);
            if clauses.is_empty() {
                return false;
            }
            let ctx = self.build_cond_info(player_id, Some(actor_id), actor_ori, None);
            let single_clause_legacy_used = used_turn && clauses.len() == 1;
            clauses.iter().any(|clause| {
                if !in_hand && clause.contains("この能力はこのカードが手札にあり") {
                    return false;
                }
                if in_partner_area && def.card_type != CardType::PARTNER && !allows_partner_area(clause) {
                    return false;
                }
                if check_clause_conditions(clause, &ctx).is_err() {
                    return false;
                }
                if single_clause_legacy_used || !self.can_use_clause_this_turn(actor_id, clause) {
                    return false;
                }
                let cost = parse_clause_cost(clause);
                self.can_pay_clause_cost(ps, actor_id, &cost)
            })
        };

        let mut declare_actors = vec![];
        for c in &ps.field {
            if let Some(c_def) = self.catalog.get(&c.definition_id) {
                if can_declare_clause(&c.instance_id, c.orientation, c.used_declare_this_turn, false, false, c_def) {
                    declare_actors.push(json!({
                        "actorId": c.instance_id.clone(),
                        "name": self.card_name_by_def(&c.definition_id),
                        "desc": c_def.description.clone(),
                    }));
                }
            }
            for sc in &c.set_cards {
                if !sc.face_up {
                    continue;
                }
                if let Some(sc_def) = self.catalog.get(&sc.definition_id) {
                    if can_declare_clause(&sc.instance_id, c.orientation, sc.used_declare_this_turn, false, false, sc_def) {
                        declare_actors.push(json!({
                            "actorId": sc.instance_id.clone(),
                            "name": format!("🔧 {}", self.card_name_by_def(&sc.definition_id)),
                            "desc": sc_def.description.clone(),
                        }));
                    }
                }
            }
        }
        if let Some(ref p) = ps.partner {
            if p.zone == Zone::PARTNER {
                if let Some(p_def) = self.catalog.get(&p.definition_id) {
                    if can_declare_clause(&p.instance_id, p.orientation, p.used_declare_this_turn, true, false, p_def) {
                        declare_actors.push(json!({
                            "actorId": p.instance_id.clone(),
                            "name": self.card_name_by_def(&p.definition_id),
                            "desc": p_def.description.clone(),
                        }));
                    }
                }
            }
        }
        for ep in &ps.extra_partners {
            if let Some(ep_def) = self.catalog.get(&ep.definition_id) {
                if can_declare_clause(&ep.instance_id, ep.orientation, ep.used_declare_this_turn, true, false, ep_def) {
                    declare_actors.push(json!({
                        "actorId": ep.instance_id.clone(),
                        "name": self.card_name_by_def(&ep.definition_id),
                        "desc": ep_def.description.clone(),
                    }));
                }
            }
        }
        for hc in &ps.hand {
            if let Some(h_def) = self.catalog.get(&hc.definition_id) {
                if (h_def.card_type == CardType::EVENT
                    || (h_def.card_type == CardType::CHARACTER
                        && h_def.description.contains("この能力はこのカードが手札にあり")))
                    && can_declare_clause(&hc.instance_id, hc.orientation, hc.used_declare_this_turn, false, true, h_def)
                {
                    declare_actors.push(json!({
                        "actorId": hc.instance_id.clone(),
                        "name": format!("📜(手札) {}", self.card_name_by_def(&hc.definition_id)),
                        "desc": h_def.description.clone(),
                    }));
                }
            }
        }
        if let Some(ref cs) = ps.r#case {
            if let Some(c_def) = self.catalog.get(&cs.definition_id) {
                if can_declare_clause(&cs.instance_id, cs.orientation, cs.used_declare_this_turn, false, false, c_def) {
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
                            ps.normal_hand_played_this_turn = true;
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
                        self.played_by_effect_events = false;
                        self.log(format!("📜 {} がイベント [{}] (Lv.{}) を使用しました", player_id, card_name, lvl));
                        if self.resolve_on_play_effects(player_id, &inst_id, &def_id, true) {
                            return;
                        }
                    } else {
                        self.log(format!("🃏 {} が [{}] (Lv.{}) を登場させました", player_id, card_name, lvl));
                        if self.resolve_character_enter_field_with_source(player_id, &inst_id, &def_id, CardOrientation::ACTIVE, None, "MAIN") {
                            return;
                        }
                    }
                }
                self.prompt_main_action(player_id);
            }
            "HINT_TAKE" => {
                if self.turn_hint_blocked_players.contains(player_id) {
                    self.log(format!("🚫 {} はこのターン【ネクストヒント】を使用できません", player_id));
                    self.prompt_main_action(player_id);
                    return;
                }
                let mut playable_cards = vec![];
                let event_blocked = self.turn_event_blocked_players.contains(player_id);
                if let Some(ps) = self.players.get_mut(player_id) {
                    if let Some(mut top_file) = ps.file.pop() {
                        top_file.zone = Zone::HAND;
                        top_file.face_up = true;
                        ps.hand.push(top_file);
                        ps.hint_played_this_turn = true;
                    }
                }
                let cond_ctx = self.build_cond_info(player_id, None, CardOrientation::ACTIVE, None);
                if let Some(ps) = self.players.get(player_id) {
                    let effective_file = ps.effective_file_count() as i32;
                    let case_def = ps.r#case.as_ref().and_then(|c| self.catalog.get(&c.definition_id));
                    let case_colors = case_def.map(|d| normalize_color_set(&d.colors)).unwrap_or_default();

                    for c in &ps.hand {
                        if let Some(c_def) = self.catalog.get(&c.definition_id) {
                            let card_colors = normalize_color_set(&c_def.colors);
                            let color_ok = card_colors.is_empty() || case_colors.is_empty() || card_colors.is_subset(&case_colors);
                            let slot_ok = true;
                            let eff_lv = if c_def.card_type == CardType::EVENT {
                                card_effective_level(c_def, &cond_ctx)
                            } else {
                                c_def.level
                            };
                            let ev_ok = c_def.card_type != CardType::EVENT
                                || (!event_blocked && check_event_play_conditions(&c_def.description, &cond_ctx).is_ok());
                            if eff_lv <= effective_file && color_ok && slot_ok && ev_ok {
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
                if self.phase == MatchPhase::FINISHED || self.pending_decision.is_some() {
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
                // サーバー側でも登場ターン推理不可・アクティブ状態の検証を行う（〚迅速〛は例外）
                let deduce_valid = self.players.get(player_id).map(|ps| {
                    if let Some(ref p) = ps.partner {
                        if p.instance_id == actor_id {
                            return p.orientation == CardOrientation::ACTIVE;
                        }
                    }
                    if let Some(c) = ps.field.iter().find(|c| c.instance_id == actor_id) {
                        return c.orientation == CardOrientation::ACTIVE
                            && (c.entered_turn != self.turn || self.effective_swift(c));
                    }
                    false
                }).unwrap_or(false);
                if !deduce_valid {
                    self.log("⚠️ そのキャラは現在推理できません（登場ターンに突撃のみで推理不可、またはアクティブ状態ではありません）");
                    self.prompt_main_action(player_id);
                    return;
                }
                let mut actor_card: Option<CardInstance> = None;
                if let Some(ps) = self.players.get_mut(player_id) {
                    if let Some(ref mut p) = ps.partner {
                        if p.instance_id == actor_id {
                            p.orientation = CardOrientation::SLEEP;
                            actor_card = Some(p.clone());
                        }
                    }
                    if actor_card.is_none() {
                        if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == actor_id) {
                            c.orientation = CardOrientation::SLEEP;
                            actor_card = Some(c.clone());
                        }
                    }
                }
                let lp = actor_card
                    .as_ref()
                    .map(|c| self.get_effective_lp(player_id, c))
                    .unwrap_or(0);

                let actor_name = self.card_name_by_inst(&actor_id);
                if lp > 0 {
                    self.gain_evidence(player_id, lp as usize);
                    self.log(format!("🔎 {} の [{}] が 推理 を行いました（証拠 +{}）", player_id, actor_name, lp));
                } else {
                    self.log(format!("🔎 {} の [{}] が 推理 を行いました", player_id, actor_name));
                }
                if self.trigger_on_action(player_id, &actor_id, ActionEventKind::Deduce, None, "MAIN") {
                    return;
                }
                self.prompt_main_action(player_id);
            }
            "ACTION" => {
                let actor_id = val.get("actorId").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let target_id = val.get("targetId").and_then(|v| v.as_str()).unwrap_or("").to_string();

                let opp_pid = self.player_order.iter().find(|p| *p != player_id).unwrap().clone();

                let is_case_target = self
                    .players
                    .get(&opp_pid)
                    .and_then(|opp_ps| opp_ps.r#case.as_ref())
                    .map(|c| c.instance_id == target_id)
                    .unwrap_or(false);

                // 突撃・登場ターン制限・攻撃対象制限チェック
                let rush_violation = {
                    let ps = self.players.get(player_id);
                    let target_eff_level = self
                        .players
                        .get(&opp_pid)
                        .and_then(|ops| ops.field.iter().find(|oc| oc.instance_id == target_id))
                        .and_then(|oc| {
                            self.catalog
                                .get(&oc.definition_id)
                                .map(|od| od.level + oc.temp_level_delta)
                        })
                        .unwrap_or(99);
                    if let Some(ps) = ps {
                        if let Some(actor_c) = ps.field.iter().find(|c| c.instance_id == actor_id) {
                            let (has_rush, rush_target) = self.effective_rush(actor_c);
                            if actor_c.orientation != CardOrientation::ACTIVE {
                                Some("アクティブ状態ではないためアクションできません")
                            } else if actor_c.entered_turn == self.turn && !has_rush {
                                Some("登場したターンは突撃がないためアクションできません")
                            } else if actor_c.entered_turn == self.turn && has_rush {
                                match rush_target.as_deref() {
                                    Some("CHAR") if is_case_target => Some("〚突撃［キャラ］〛は事件を攻撃できません"),
                                    Some("CHAR_LV4") if is_case_target || target_eff_level > 4 => {
                                        Some("〚突撃［レベル4以下のキャラ］〛はレベル4以下のキャラ以外を攻撃できません")
                                    }
                                    Some("CHAR_LV4+CASE") if !is_case_target && target_eff_level > 4 => {
                                        Some("レベル5以上のキャラを攻撃できません")
                                    }
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

                self.contact_ap_buffs.clear();
                self.action_context = Some(json!({
                    "actorId": actor_id,
                    "targetId": target_id,
                    "attackerPlayerId": player_id,
                    "defenderPlayerId": opp_pid
                }));

                // アクション時トリガーの解決
                self.acted_this_turn_ids.insert(actor_id.clone());
                let action_kind = if is_case_target {
                    ActionEventKind::AttackCase
                } else {
                    ActionEventKind::AttackChar
                };
                let start_guard_ctx = format!("START_GUARD:{}", opp_pid);
                if self.trigger_on_action(player_id, &actor_id, action_kind, Some(&target_id), &start_guard_ctx) {
                    return;
                }

                if !self.is_action_target_still_valid() {
                    self.action_context = None;
                self.contact_ap_buffs.clear();
                    self.log("⚠️ 指定されていた対象が現場を離れたためアクションを終了します");
                    self.prompt_main_action(player_id);
                    return;
                }

                self.prompt_guard_or_cutin(&opp_pid);
            }
            "DECLARE" => {
                let actor_id = val.get("actorId").and_then(|v| v.as_str()).unwrap_or("").to_string();
                self.handle_declare_action(player_id, &actor_id);
            }
            "DISGUISE" => {
                if self.turn_disguise_blocked_players.contains(player_id) {
                    self.log(format!("🚫 {} はこのターン【変装】を使用できません", player_id));
                    self.prompt_main_action(player_id);
                    return;
                }
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
                    self.last_disguise_replaced_name = self
                        .catalog
                        .get(&old_def_id)
                        .map(|d| d.names.join("|"))
                        .or_else(|| Some(old_name.clone()));
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
        let mut is_set_card = false;
        let mut set_host_id: Option<String> = None;
        let mut is_hand_event_declare = false;

        let info_opt = self.players.get(player_id).and_then(|ps| {
            if let Some(ref p) = ps.partner {
                if p.instance_id == actor_id {
                    return Some((p.definition_id.clone(), p.zone == Zone::PARTNER, p.used_declare_this_turn));
                }
            }
            for ep in &ps.extra_partners {
                if ep.instance_id == actor_id {
                    return Some((ep.definition_id.clone(), true, ep.used_declare_this_turn));
                }
            }
            if let Some(ref cs) = ps.r#case {
                if cs.instance_id == actor_id {
                    return Some((cs.definition_id.clone(), false, cs.used_declare_this_turn));
                }
            }
            if let Some(c) = ps.field.iter().find(|c| c.instance_id == actor_id) {
                return Some((c.definition_id.clone(), false, c.used_declare_this_turn));
            }
            for c in &ps.field {
                if let Some(sc) = c.set_cards.iter().find(|s| s.instance_id == actor_id) {
                    is_set_card = true;
                    set_host_id = Some(c.instance_id.clone());
                    return Some((sc.definition_id.clone(), false, sc.used_declare_this_turn));
                }
            }
            if let Some(hc) = ps.hand.iter().find(|h| h.instance_id == actor_id) {
                is_hand_event_declare = true;
                return Some((hc.definition_id.clone(), false, hc.used_declare_this_turn));
            }
            None
        });

        let Some((def_id, in_partner_area, legacy_used)) = info_opt else {
            self.prompt_main_action(player_id);
            return;
        };
        let Some(def) = self.catalog.get(&def_id).cloned() else {
            self.prompt_main_action(player_id);
            return;
        };

        let card_name = self.card_name_by_def(&def_id);
        let ori = self.find_actor_orientation(player_id, set_host_id.as_deref().unwrap_or(actor_id));
        let mut clauses = extract_clauses(&def.description, TriggerKind::Declare, def.card_type);
        if is_set_card {
            for inner in self.extract_set_card_granted_clauses(&def.description, TriggerKind::Declare) {
                if !clauses.contains(&inner) {
                    clauses.push(inner);
                }
            }
        }
        let single_clause_legacy_used = legacy_used && clauses.len() == 1;

        // 先に条件判定（immutable borrow を完了）
        let mut cond_ok_clauses: Vec<String> = Vec::new();
        {
            let ctx = self.build_cond_info(player_id, Some(actor_id), ori, None);
            for cl in &clauses {
                if in_partner_area && def.card_type != CardType::PARTNER && !allows_partner_area(cl) {
                    continue;
                }
                if check_clause_conditions(cl, &ctx).is_err() {
                    continue;
                }
                cond_ok_clauses.push(cl.clone());
            }
        }

        // 実行可能な宣言句を選出（複数宣言がある場合はターン制限・コストを満たす最初の有効句を優先）
        let mut chosen_clause: Option<String> = None;
        for cl in &cond_ok_clauses {
            if single_clause_legacy_used || !self.can_use_clause_this_turn(actor_id, cl) {
                continue;
            }
            let cost = parse_clause_cost(cl);
            let cost_actor = set_host_id.as_deref().unwrap_or(actor_id);
            let can_pay_set = self.check_and_pay_set_card_cost(player_id, cost_actor, cl, true);
            let can_pay = can_pay_set
                && self
                    .players
                    .get(player_id)
                    .map(|ps| self.can_pay_clause_cost(ps, cost_actor, &cost))
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
        let pay_auto = cost.hand_remove == 0 && cost.evidence_flip_up == 0;
        self.last_cost_flipped_evidence_count = 0;
        let cost_actor = set_host_id.clone().unwrap_or_else(|| actor_id.to_string());
        // 証拠を表向きにするコストは後段のインタラクティブ選択（枚数分）で支払うため、ここでは自動で表向きにしない
        let face_down_for_cost = self
            .players
            .get(player_id)
            .map(|ps| ps.evidence.iter().filter(|e| !e.face_up).count())
            .unwrap_or(0);
        if cost.evidence_flip_up > 0 && face_down_for_cost < cost.evidence_flip_up {
            self.log(format!("⚠️ [{}] の【宣言】コストを満たしていません（裏向きの証拠が不足）", card_name));
            self.prompt_main_action(player_id);
            return;
        }
        let mut auto_cost = cost.clone();
        auto_cost.evidence_flip_up = 0;
        if !self.check_and_pay_set_card_cost(player_id, &cost_actor, &clause, false)
            || !self.pay_clause_cost_internal(player_id, &cost_actor, &auto_cost, pay_auto)
        {
            self.prompt_main_action(player_id);
            return;
        }

        // B10018 hand declare: keep hand card available as actor for SetThisEvent
        let _ = is_hand_event_declare;

        self.mark_clause_used_this_turn(actor_id, &clause);
        if let Some(ps) = self.players.get_mut(player_id) {
            if let Some(ref mut p) = ps.partner {
                if p.instance_id == actor_id {
                    p.used_declare_this_turn = true;
                }
            }
            for ep in &mut ps.extra_partners {
                if ep.instance_id == actor_id {
                    ep.used_declare_this_turn = true;
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
            for c in &mut ps.field {
                if let Some(sc) = c.set_cards.iter_mut().find(|s| s.instance_id == actor_id) {
                    sc.used_declare_this_turn = true;
                }
            }
        }

        self.log(format!("⚡ {} が [{}] の【宣言】を発動！", player_id, card_name));
        let resolved_clause = resolve_state_branch(&clause, ori);
        let plan = parse_effect_steps(&resolved_clause, None, def.card_type);
        if cost.evidence_flip_up > 0 {
            let face_down_count = self
                .players
                .get(player_id)
                .map(|ps| ps.evidence.iter().filter(|e| !e.face_up).count())
                .unwrap_or(0);
            if face_down_count >= cost.evidence_flip_up {
                let max_flip = if cost.evidence_flip_up_any {
                    face_down_count
                } else {
                    cost.evidence_flip_up
                };
                let flip_step = crate::effects::TargetStep {
                    action: TargetAction::FlipEvidenceFaceUp,
                    filter: crate::effects::TargetFilter {
                        owner: crate::effects::OwnerScope::SelfOnly,
                        zone: crate::effects::TargetZone::Evidence,
                        count: max_flip,
                        up_to: cost.evidence_flip_up_any,
                        face_down_only: true,
                        ..Default::default()
                    },
                    bonuses: Vec::new(),
                    follow_up: Vec::new(),
                    conditional_follow_up: None,
                    optional: false,
                    then_abort_if_skipped: false,
                    scale_by_cost_flipped_evidence: false,
                };
                if self.prompt_target_step_with_plan(
                    player_id,
                    actor_id,
                    &card_name,
                    Some(def.level),
                    &flip_step,
                    None,
                    Some(&plan),
                    "MAIN",
                    None,
                ) {
                    return;
                }
            }
        }
        if cost.hand_remove > 0 {
            if self.prompt_hand_discard(
                player_id,
                actor_id,
                &card_name,
                Some(def.level),
                cost.hand_remove,
                cost.hand_filter.as_ref(),
                false,
                false,
                &[],
                None,
                None,
                Some(&plan),
                "MAIN",
                None,
            ) {
                return;
            }
        }
        if self.execute_clause_plan(player_id, actor_id, &card_name, Some(def.level), &plan, "MAIN", None) {
            return;
        }
        self.prompt_main_action(player_id);
    }

    fn apply_single_target_action(
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
                                if effect_type == "BOUNCE_TO_HAND" {
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
                if let Some(ps) = self.players.get_mut(player_id) {
                    if let Some(idx) = ps.field.iter().position(|c| c.instance_id == tid) {
                        let mut card = ps.field.remove(idx);
                        card.zone = Zone::REMOVE;
                        card.orientation = CardOrientation::ACTIVE;
                        card.face_up = true;
                        ps.remove.push(card);
                        self.log(format!("📤 ルール処理: {} の現場上限(5体)超過により [{}] を場外(リムーブエリア)に移しました（効果リムーブではないため誘発なし）", player_id, target_name));
                    }
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

    fn apply_target_bonuses(
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

    fn handle_target_select_answer(&mut self, player_id: &str, val: &Value, dec: &Value) {
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
        let valid_candidate_ids: HashSet<String> = opts
            .get("candidates")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|c| c.get("instanceId").and_then(|s| s.as_str()).map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

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

        let valid_chosen: Vec<String> = selected_ids
            .into_iter()
            .filter(|id| valid_candidate_ids.is_empty() || valid_candidate_ids.contains(id))
            .collect();

        let mut deployed_list: Vec<(String, String, CardOrientation)> = Vec::new();
        let mut chosen_def_for_cond: Option<String> = None;
        let mut applied = false;

        if effect_type == "DISCARD_HAND" {
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
        } else if !valid_chosen.is_empty() {
            if effect_type == "SLEEP" {
                self.last_slept_names.clear();
            }
            applied = true;
            self.last_chosen_added_to_hand = false;
            self.last_deployed_count = 0;
            let mut removed_lv_sum = 0i32;
            let mut removed_ap_sum = 0i32;
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
            let bonuses: Vec<TargetBonus> = serde_json::from_value(
                opts.get("bonuses").cloned().unwrap_or_else(|| json!([])),
            )
            .unwrap_or_default();
            if self.apply_target_bonuses(player_id, &actor_id, &actor_name, &bonuses, &valid_chosen, &resume_context) {
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

        // 登場させたキャラの登場時効果（複数体が同時にOnPlayを持つ場合は発動順をプレイヤーが選択）
        if deployed_list.len() >= 2 {
            let mut on_play_tasks: Vec<PendingClauseTask> = Vec::new();
            for (dep_inst, dep_def, _) in &deployed_list {
                if let Some(def) = self.catalog.get(dep_def).cloned() {
                    let on_play_clauses = extract_clauses(&def.description, TriggerKind::OnPlay, def.card_type);
                    let dep_name = self.card_name_by_def(dep_def);
                    for cl in on_play_clauses {
                        on_play_tasks.push(PendingClauseTask {
                            player_id: player_id.to_string(),
                            actor_id: dep_inst.clone(),
                            actor_name: dep_name.clone(),
                            source_level,
                            card_type: def.card_type,
                            clause: cl,
                            resume_context: resume_context.clone(),
                            evidence_source_inst_id: None,
                            post_target_ops: vec![],
                            plan: None,
                        });
                    }
                }
            }
            if on_play_tasks.len() >= 2 {
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
                self.pending_decision = Some(json!({
                    "decisionId": format!("dec_{}", self.revision + 1),
                    "playerId": player_id,
                    "kind": "triggerOrder",
                    "options": {
                        "title": "同時に誘発した【登場時】効果のうち、先に発動する能力を選んでください",
                        "triggers": triggers_json,
                    }
                }));
                return;
            }
        }

        for (didx, (dep_inst, dep_def, dep_ori)) in deployed_list.iter().enumerate() {
            if self.resolve_character_enter_field_with_source(
                player_id,
                dep_inst,
                dep_def,
                *dep_ori,
                source_level,
                &resume_context,
            ) {
                let mut rem_tasks: Vec<PendingClauseTask> = Vec::new();
                for (rem_inst, rem_def, _) in &deployed_list[didx + 1..] {
                    if let Some(def) = self.catalog.get(rem_def).cloned() {
                        let on_play_clauses = extract_clauses(&def.description, TriggerKind::OnPlay, def.card_type);
                        let rem_name = self.card_name_by_def(rem_def);
                        for cl in on_play_clauses {
                            rem_tasks.push(PendingClauseTask {
                                player_id: player_id.to_string(),
                                actor_id: rem_inst.clone(),
                                actor_name: rem_name.clone(),
                                source_level,
                                card_type: def.card_type,
                                clause: cl,
                                resume_context: resume_context.clone(),
                                evidence_source_inst_id: None,
                                post_target_ops: vec![],
                                plan: None,
                            });
                        }
                    }
                }
                if !rem_tasks.is_empty() {
                    rem_tasks.extend(std::mem::take(&mut self.pending_clause_queue));
                    self.pending_clause_queue = rem_tasks;
                }
                return;
            }
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

    /// 同時誘発した能力の発動順選択を処理する
    fn handle_trigger_order_answer(&mut self, player_id: &str, val: &Value) {
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
                                if ps.field.len() < 5 {
                                    card.zone = Zone::FIELD;
                                    card.orientation = if pick_action == "PLAY_SLEEP" {
                                        CardOrientation::SLEEP
                                    } else {
                                        CardOrientation::ACTIVE
                                    };
                                    card.entered_turn = turn;
                                    card.face_up = true;
                                    deployed_count_delta += 1;
                                    if let Some(d) = self.catalog.get(&card.definition_id) {
                                        last_chosen_lv = Some(d.level);
                                    }
                                    last_chosen_def = Some(card.definition_id.clone());
                                    last_chosen_inst = Some(card.instance_id.clone());
                                    picked_defs.push((card.instance_id.clone(), card.definition_id.clone()));
                                    ps.field.push(card);
                                    continue;
                                }
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
                && ps.field.len() < 5
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
                        c.zone = Zone::REMOVE;
                        c.face_up = true;
                        ps.remove.push(c);
                    } else {
                        c.face_up = false;
                        ps.deck.insert(0, c);
                    }
                }
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
            if post_add_discard > 0 && pick_action == "TO_HAND" {
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

    /// 現場のキャラが上限(5体)を超えている場合、場外(リムーブエリア)に移すキャラを1体選択させる（効果リムーブではない）
    fn check_field_overflow(&mut self, player_id: &str, resume_context: &str) -> bool {
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
            return false;
        };
        let candidates: Vec<Value> = if let Some(ps) = self.players.get(&target_pid) {
            ps.field
                .iter()
                .map(|c| {
                    let def = self.catalog.get(&c.definition_id);
                    json!({
                        "instanceId": c.instance_id,
                        "definitionId": c.definition_id,
                        "name": self.card_name_by_def(&c.definition_id),
                        "level": def.map(|d| d.level).unwrap_or(0),
                        "ap": def.map(|d| d.ap).unwrap_or(0),
                        "traits": def.map(|d| d.traits.clone()).unwrap_or_default(),
                    })
                })
                .collect()
        } else {
            return false;
        };
        if candidates.is_empty() {
            return false;
        }
        self.pending_decision = Some(json!({
            "decisionId": format!("dec_{}", self.revision + 1),
            "playerId": target_pid,
            "kind": "targetSelect",
            "options": {
                "title": "場のキャラが上限(5体)を超えたため、場外に移すキャラを1体選択してください",
                "actorId": "",
                "actorName": "ルール処理",
                "effectType": "FIELD_OVERFLOW_REMOVE",
                "zone": "FIELD",
                "maxCount": 1,
                "optional": false,
                "candidates": candidates,
                "resumeContext": resume_context,
            }
        }));
        true
    }

    fn resume_after_decision(&mut self, decision_player_id: &str, resume_context: &str) {
        if self.drain_pending_clause_queue() {
            return;
        }
        if self.try_fire_pending_ally_play(resume_context) {
            return;
        }
        if self.check_field_overflow(decision_player_id, resume_context) {
            return;
        }
        if resume_context == "MAIN" && self.try_fire_pending_hint_trigger(decision_player_id, None) {
            return;
        }
        if resume_context == "GUARD" {
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
        } else if let Some(def_pid) = resume_context.strip_prefix("START_GUARD:") {
            if self.is_action_target_still_valid() {
                self.prompt_guard_or_cutin(def_pid);
            } else {
                self.action_context = None;
                self.contact_ap_buffs.clear();
                self.log("⚠️ 指定されていた対象が現場を離れたためアクションを終了します");
                self.prompt_main_action(&self.turn_player_id.clone());
            }
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

        // ガード可能カード（フィールドのACTIVE状態キャラ、攻撃側がブレットならガード不可）
        let cutin_or_disguise_used = self.action_context.as_ref().and_then(|c| c.get("defenderUsedCutin")).and_then(|v| v.as_bool()).unwrap_or(false)
            || self.action_context.as_ref().and_then(|c| c.get("cutinOrDisguiseUsed")).and_then(|v| v.as_bool()).unwrap_or(false);
        let contact_target_id = self.action_context.as_ref().and_then(|c| c.get("targetId")).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let attacker_pid = self.action_context.as_ref().and_then(|c| c.get("attackerPlayerId")).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let attacker_actor_id = self.action_context.as_ref().and_then(|c| c.get("actorId")).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let attacker_has_bullet = self.effective_bullet(&attacker_pid, &attacker_actor_id);
        let attacker_forces_guard = self.attacker_forces_guard(&attacker_pid, &attacker_actor_id);
        let is_case_attack = opp_ps.r#case.as_ref().map(|c| c.instance_id == contact_target_id).unwrap_or(false);

        let mut guards: Vec<String> = if attacker_has_bullet {
            vec![]
        } else {
            opp_ps.field.iter()
                .filter(|c| c.orientation == CardOrientation::ACTIVE)
                .map(|c| c.instance_id.clone())
                .collect()
        };
        // 「このキャラはガードできる場合、必ずガードする」を持つキャラがいる場合、そのキャラが必ずガードする
        let forced_guards: Vec<String> = opp_ps.field.iter()
            .filter(|c| c.must_guard_this_turn && guards.contains(&c.instance_id))
            .map(|c| c.instance_id.clone())
            .collect();
        let char_forces_guard = !forced_guards.is_empty();
        if char_forces_guard {
            guards = forced_guards;
        }
        let must_guard = (attacker_forces_guard || char_forces_guard) && !guards.is_empty();

        let (cutins, disguises, disguise_target_id) = if !cutin_or_disguise_used && !is_case_attack {
            self.collect_cutin_and_disguise_options(defender_pid, &contact_target_id)
        } else {
            (vec![], vec![], None)
        };

        if guards.is_empty() {
            if is_case_attack {
                self.proceed_to_action_resolution(None);
                return;
            }
            // 防衛側にガード可能なキャラがいない場合は、カットインフェーズへ（APの低い方から）
            if let Some(ctx) = self.action_context.as_mut() {
                ctx["guardResolved"] = json!(true);
                ctx["consecutivePasses"] = json!(0);
            }
            self.start_cutin_phase();
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
                "cutinCardIds": [],
                "disguiseOptions": [],
                "disguiseTargetId": null,
                "mustGuard": must_guard
            }
        }));
    }

    /// カットインフェーズを開始し、コンタクト中キャラの現在APが低い方のプレイヤーから確認する
    /// （APが同じ場合は攻撃側＝ターンプレイヤーから）
    fn start_cutin_phase(&mut self) {
        let (attacker_pid, defender_pid, actor_id, target_id, guard_id) = {
            let ctx = match self.action_context.as_ref() {
                Some(c) => c,
                None => return,
            };
            (
                ctx.get("attackerPlayerId").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                ctx.get("defenderPlayerId").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                ctx.get("actorId").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                ctx.get("targetId").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                ctx.get("guardCardId").and_then(|v| v.as_str()).map(|s| s.to_string()),
            )
        };

        let def_char_id = guard_id.clone().unwrap_or(target_id);
        let att_ap = self.players.get(&attacker_pid)
            .and_then(|ps| ps.field.iter().find(|c| c.instance_id == actor_id))
            .map(|c| self.get_effective_ap(&attacker_pid, c))
            .unwrap_or(0);
        let def_ap = self.players.get(&defender_pid)
            .and_then(|ps| ps.field.iter().find(|c| c.instance_id == def_char_id))
            .map(|c| self.get_effective_ap(&defender_pid, c))
            .unwrap_or(0);

        let first_pid = if def_ap < att_ap && !defender_pid.is_empty() {
            defender_pid
        } else if !attacker_pid.is_empty() {
            attacker_pid
        } else {
            defender_pid
        };

        if first_pid.is_empty() {
            self.proceed_to_action_resolution(guard_id.as_deref());
        } else {
            self.prompt_contact_cutin(&first_pid);
        }
    }

    fn collect_cutin_and_disguise_options(&self, pid: &str, contact_char_id: &str) -> (Vec<Value>, Vec<Value>, Option<String>) {
        let ps = match self.players.get(pid) {
            Some(p) => p,
            None => return (vec![], vec![], None),
        };
        let cond_ctx = self.build_cond_info(pid, None, CardOrientation::ACTIVE, None);
        let cutins: Vec<Value> = if self.turn_cutin_blocked_players.contains(pid) {
            vec![]
        } else {
            ps.hand.iter()
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
                .collect()
        };

        let mut disguises: Vec<Value> = vec![];
        let mut disguise_target_id: Option<String> = None;
        if !self.turn_disguise_blocked_players.contains(pid) && ps.field.iter().any(|c| c.instance_id == contact_char_id) {
            disguise_target_id = Some(contact_char_id.to_string());
            let effective_file = ps.effective_file_count() as i32;
            let case_def = ps.r#case.as_ref().and_then(|c| self.catalog.get(&c.definition_id));
            let case_colors = case_def.map(|d| normalize_color_set(&d.colors)).unwrap_or_default();
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
                            disguises.push(json!({
                                "instanceId": c.instance_id,
                                "definitionId": c.definition_id,
                                "name": self.card_name_by_def(&c.definition_id),
                                "desc": c_def.description.clone(),
                            }));
                        }
                    }
                }
            }
        }
        (cutins, disguises, disguise_target_id)
    }

    fn prompt_contact_cutin(&mut self, pid: &str) {
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
        let (attacker_pid, defender_pid, actor_id, target_id, guard_id, attacker_used, defender_used, consecutive_passes) = {
            let ctx = match self.action_context.as_ref() {
                Some(c) => c,
                None => return,
            };
            (
                ctx.get("attackerPlayerId").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                ctx.get("defenderPlayerId").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                ctx.get("actorId").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                ctx.get("targetId").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                ctx.get("guardCardId").and_then(|v| v.as_str()).map(|s| s.to_string()),
                ctx.get("attackerUsedCutin").and_then(|v| v.as_bool()).unwrap_or(false),
                ctx.get("defenderUsedCutin").and_then(|v| v.as_bool()).unwrap_or(false),
                ctx.get("consecutivePasses").and_then(|v| v.as_u64()).unwrap_or(0),
            )
        };

        let is_attacker = pid == attacker_pid;
        let already_used = if is_attacker { attacker_used } else { defender_used };
        let other_pid = if is_attacker { defender_pid.clone() } else { attacker_pid.clone() };
        let other_used = if is_attacker { defender_used } else { attacker_used };

        let contact_char_id = if is_attacker {
            actor_id
        } else {
            guard_id.clone().unwrap_or(target_id)
        };

        let (cutins, disguises, disguise_target_id) = if !already_used {
            self.collect_cutin_and_disguise_options(pid, &contact_char_id)
        } else {
            (vec![], vec![], None)
        };

        if cutins.is_empty() && disguises.is_empty() {
            let next_passes = consecutive_passes + 1;
            if next_passes >= 2 || other_used || other_pid.is_empty() {
                self.proceed_to_action_resolution(guard_id.as_deref());
            } else {
                if let Some(ctx) = self.action_context.as_mut() {
                    ctx["consecutivePasses"] = json!(next_passes);
                }
                self.prompt_contact_cutin(&other_pid);
            }
            return;
        }

        let dec_id = self.generate_decision_id();
        self.pending_decision = Some(json!({
            "decisionId": dec_id,
            "revision": self.revision,
            "playerId": pid,
            "kind": "guard",
            "options": {
                "guardCardIds": [],
                "cutinCardIds": cutins,
                "disguiseOptions": disguises,
                "disguiseTargetId": disguise_target_id
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
                    if true {
                        let mut card = ps.hand.remove(idx);
                        played_def_id = Some(card.definition_id.clone());
                        played_inst_id = Some(card.instance_id.clone());
                        card.face_up = true;
                        if card_type == CardType::EVENT {
                            is_event = true;
                            card.zone = Zone::REMOVE;
                            ps.remove.push(card);
                            ps.normal_hand_played_this_turn = true;
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
            let (card_lv, card_colors) = self
                .catalog
                .get(&def_id)
                .map(|d| (d.level, d.colors.clone()))
                .unwrap_or((0, vec![]));
            self.pending_hint_trigger = Some((player_id.to_string(), card_lv, card_colors.clone()));
            let need_target = if is_event {
                self.log(format!("📜 {} がイベント [{}] を使用しました", player_id, card_name));
                self.resolve_on_play_effects(player_id, &inst_id, &def_id, true)
            } else {
                self.log(format!("🃏 {} が [{}] を登場させました", player_id, card_name));
                self.resolve_character_enter_field_with_source(
                    player_id,
                    &inst_id,
                    &def_id,
                    CardOrientation::ACTIVE,
                    None,
                    "MAIN",
                )
            };
            if need_target {
                if let Some(ref mut pd) = self.pending_decision {
                    if let Some(opts) = pd.get_mut("options").and_then(|v| v.as_object_mut()) {
                        opts.insert("afterHintLevel".to_string(), json!(card_lv));
                    }
                }
                return;
            }
            self.pending_hint_trigger = None;
            if self.trigger_on_hint_use_with_colors(player_id, card_lv, &card_colors) {
                return;
            }
        } else {
            self.log(format!("{} はヒントからの登場をスキップしました", player_id));
        }
        self.prompt_main_action(player_id);
    }

    fn handle_guard_answer(&mut self, player_id: &str, val: &Value) {
        let (guard_resolved, attacker_pid, defender_pid, saved_guard_id, attacker_used, defender_used, consecutive_passes) = {
            let ctx = self.action_context.as_ref();
            (
                ctx.and_then(|c| c.get("guardResolved")).and_then(|v| v.as_bool()).unwrap_or(false),
                ctx.and_then(|c| c.get("attackerPlayerId")).and_then(|v| v.as_str()).unwrap_or("").to_string(),
                ctx.and_then(|c| c.get("defenderPlayerId")).and_then(|v| v.as_str()).unwrap_or("").to_string(),
                ctx.and_then(|c| c.get("guardCardId")).and_then(|v| v.as_str()).map(|s| s.to_string()),
                ctx.and_then(|c| c.get("attackerUsedCutin")).and_then(|v| v.as_bool()).unwrap_or(false),
                ctx.and_then(|c| c.get("defenderUsedCutin")).and_then(|v| v.as_bool()).unwrap_or(false),
                ctx.and_then(|c| c.get("consecutivePasses")).and_then(|v| v.as_u64()).unwrap_or(0),
            )
        };
        let is_attacker = player_id == attacker_pid;
        let player_used = if is_attacker { attacker_used } else { defender_used };
        let other_pid = if is_attacker { defender_pid.clone() } else { attacker_pid.clone() };
        let other_used = if is_attacker { defender_used } else { attacker_used };

        // 変装使用かチェック（コンタクト時にカットインと変装を含めて合計1枚まで）
        if let Some(disguise_instance_id) = val.get("disguiseCardId").and_then(|v| v.as_str()) {
            if !player_used {
                let default_target = if is_attacker {
                    self.action_context.as_ref().and_then(|c| c.get("actorId")).and_then(|v| v.as_str()).unwrap_or("").to_string()
                } else {
                    saved_guard_id.clone().unwrap_or_else(|| {
                        self.action_context.as_ref().and_then(|c| c.get("targetId")).and_then(|v| v.as_str()).unwrap_or("").to_string()
                    })
                };
                let target_id = val.get("targetId").and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or(default_target);
                let turn = self.turn;
                let mut disguised_def_id = None;
                let mut disguised_inst_id = None;
                let mut replaced_def_id = None;

                if let Some(ps) = self.players.get_mut(player_id) {
                    if let (Some(h_idx), Some(f_idx)) = (
                        ps.hand.iter().position(|c| c.instance_id == disguise_instance_id),
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
                    if let Some(ctx) = self.action_context.as_mut() {
                        if is_attacker {
                            ctx["attackerUsedCutin"] = json!(true);
                            ctx["actorId"] = json!(inst_id.clone());
                        } else {
                            ctx["defenderUsedCutin"] = json!(true);
                            ctx["cutinOrDisguiseUsed"] = json!(true);
                            if saved_guard_id.is_some() {
                                ctx["guardCardId"] = json!(inst_id.clone());
                            } else {
                                ctx["targetId"] = json!(inst_id.clone());
                            }
                        }
                        ctx["consecutivePasses"] = json!(0);
                    }
                    let new_name = self.card_name_by_def(&def_id);
                    let old_name = self.card_name_by_def(&old_def_id);
                    self.last_disguise_replaced_name = self
                        .catalog
                        .get(&old_def_id)
                        .map(|d| d.names.join("|"))
                        .or_else(|| Some(old_name.clone()));
                    self.log(format!("🎭 {} が [{}] と入れ替えて [{}] を【変装】登場させました！", player_id, old_name, new_name));
                    let need_ui = self.trigger_on_disguise_with_ctx(player_id, &inst_id, &def_id, "GUARD");
                    if need_ui {
                        if let Some(ctx) = self.action_context.as_mut() {
                            ctx["pendingCutinTrigger"] = json!({
                                "userPid": player_id,
                                "cutinDefId": def_id,
                                "isDisguise": true
                            });
                        }
                        return;
                    }
                    if self.trigger_on_cutin_or_disguise_used(player_id, Some(&def_id), true) {
                        return;
                    }
                    if self.is_contact_aborted() {
                        self.action_context = None;
                self.contact_ap_buffs.clear();
                        self.log("⚠️ コンタクト中のキャラが現場を離れたためコンタクトを終了します");
                        self.prompt_main_action(&attacker_pid);
                        return;
                    }
                    if !guard_resolved {
                        self.prompt_guard_or_cutin(&defender_pid);
                    } else if !other_used && !other_pid.is_empty() {
                        self.prompt_contact_cutin(&other_pid);
                    } else {
                        let gid = self.action_context.as_ref().and_then(|c| c.get("guardCardId")).and_then(|v| v.as_str()).map(|s| s.to_string());
                        self.proceed_to_action_resolution(gid.as_deref());
                    }
                    return;
                }
            }
        }

        // カットイン使用かチェック（ガードされていない事件アタック時は使用不可、コンタクト時に各プレイヤー1回まで）
        if let Some(cutin_instance_id) = val.get("cutinCardId").and_then(|v| v.as_str()) {
            let contact_target_id = self.action_context.as_ref().and_then(|c| c.get("targetId")).and_then(|v| v.as_str()).unwrap_or("");
            let has_guard = self.action_context.as_ref().and_then(|c| c.get("guardCardId")).and_then(|v| v.as_str()).is_some()
                || val.get("guardCardId").and_then(|v| v.as_str()).is_some();
            let is_case_attack = self.players.get(&defender_pid)
                .and_then(|ps| ps.r#case.as_ref())
                .map(|c| c.instance_id == contact_target_id)
                .unwrap_or(false);
            let is_unblocked_case_attack = is_case_attack && !has_guard;
            if !player_used && !is_unblocked_case_attack {
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
                    if let Some(ctx) = self.action_context.as_mut() {
                        if is_attacker {
                            ctx["attackerUsedCutin"] = json!(true);
                        } else {
                            ctx["defenderUsedCutin"] = json!(true);
                            ctx["cutinOrDisguiseUsed"] = json!(true);
                        }
                        ctx["consecutivePasses"] = json!(0);
                    }
                    let cutin_name = self.card_name_by_def(&def_id);
                    self.log(format!("⚡ {} が [{}] の【カットイン】を発動！", player_id, cutin_name));
                    let needs_target = self.resolve_cutin_effects(player_id, &cutin_instance_id, &def_id);
                    if needs_target {
                        if let Some(ctx) = self.action_context.as_mut() {
                            ctx["pendingCutinTrigger"] = json!({
                                "userPid": player_id,
                                "cutinDefId": def_id,
                                "isDisguise": false
                            });
                        }
                        return;
                    }
                    if self.trigger_on_cutin_or_disguise_used(player_id, Some(&def_id), false) {
                        return;
                    }
                    if self.is_contact_aborted() {
                        self.action_context = None;
                self.contact_ap_buffs.clear();
                        self.log("⚠️ コンタクト中のキャラが現場を離れたためコンタクトを終了します");
                        self.prompt_main_action(&attacker_pid);
                        return;
                    }
                    if !guard_resolved {
                        self.prompt_guard_or_cutin(&defender_pid);
                    } else if !other_used && !other_pid.is_empty() {
                        self.prompt_contact_cutin(&other_pid);
                    } else {
                        let gid = self.action_context.as_ref().and_then(|c| c.get("guardCardId")).and_then(|v| v.as_str()).map(|s| s.to_string());
                        self.proceed_to_action_resolution(gid.as_deref());
                    }
                    return;
                }
            }
        }

        if !guard_resolved {
            // ガード宣言またはスルー
            let attacker_actor_id = self.action_context.as_ref().and_then(|c| c.get("actorId")).and_then(|v| v.as_str()).unwrap_or("").to_string();
            let mut guard_id = if self.effective_bullet(&attacker_pid, &attacker_actor_id) {
                None
            } else {
                val.get("guardCardId").and_then(|v| v.as_str()).map(|s| s.to_string())
            };
            if guard_id.is_none() && self.attacker_forces_guard(&attacker_pid, &attacker_actor_id) {
                if let Some(forced_gid) = self.players.get(&defender_pid).and_then(|ps| {
                    ps.field.iter().find(|c| c.orientation == CardOrientation::ACTIVE).map(|c| c.instance_id.clone())
                }) {
                    guard_id = Some(forced_gid);
                }
            }
            if let Some(ref gid) = guard_id {
                let guard_name = self.card_name_by_inst(gid);
                self.log(format!("🛡️ {} が [{}] でガードを宣言！", player_id, guard_name));
            } else {
                self.log(format!("{} はガードを行いませんでした（スルー）", player_id));
            }

            let contact_target_id = self.action_context.as_ref().and_then(|c| c.get("targetId")).and_then(|v| v.as_str()).unwrap_or("");
            let is_case_attack = self.players.get(&defender_pid)
                .and_then(|ps| ps.r#case.as_ref())
                .map(|c| c.instance_id == contact_target_id)
                .unwrap_or(false);

            if is_case_attack && guard_id.is_none() {
                self.proceed_to_action_resolution(None);
                return;
            }

            if let Some(ctx) = self.action_context.as_mut() {
                ctx["guardResolved"] = json!(true);
                ctx["guardCardId"] = json!(guard_id);
                ctx["consecutivePasses"] = json!(0);
            }
            self.start_cutin_phase();
            return;
        }

        // コンタクト中のカットイン/変装ウィンドウでのスルー
        let next_passes = consecutive_passes + 1;
        if next_passes >= 2 || other_used || other_pid.is_empty() {
            self.proceed_to_action_resolution(saved_guard_id.as_deref());
        } else {
            if let Some(ctx) = self.action_context.as_mut() {
                ctx["consecutivePasses"] = json!(next_passes);
            }
            self.prompt_contact_cutin(&other_pid);
        }
    }

    fn handle_cutin_answer(&mut self, player_id: &str, val: &Value) {
        self.handle_guard_answer(player_id, val);
    }

    fn proceed_to_action_resolution(&mut self, guard_id: Option<&str>) {
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
            if !self.drain_pending_clause_queue() {
                self.prompt_main_action(&att_pid);
            }
            return;
        }
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
            // B03080 (BANG): このキャラがアクション［事件］したとき、ターン1回アクティブにする
            let mut b03080_target: Option<(String, String)> = None;
            if let Some(att_ps) = self.players.get(&attacker_pid) {
                if let Some(att_c) = att_ps.field.iter().find(|c| c.instance_id == actor_id) {
                    for sc in &att_c.set_cards {
                        if let Some(sdef) = self.catalog.get(&sc.definition_id) {
                            if sdef.description.contains("アクション［事件］したとき、このキャラをアクティブにする")
                                && self.can_use_clause_this_turn(&sc.instance_id, &sdef.description)
                            {
                                b03080_target = Some((sc.instance_id.clone(), sdef.description.clone()));
                                break;
                            }
                        }
                    }
                }
            }
            if let Some((sid, sdesc)) = b03080_target {
                if let Some(att_ps) = self.players.get_mut(&attacker_pid) {
                    if let Some(att_c) = att_ps.field.iter_mut().find(|c| c.instance_id == actor_id) {
                        att_c.orientation = CardOrientation::ACTIVE;
                    }
                }
                self.mark_clause_used_this_turn(&sid, &sdesc);
                let aname = self.card_name_by_inst(&actor_id);
                self.log(format!("💥 [BANG] の効果で [{}] をアクティブにしました！", aname));
            }

            let blocks_opp_hirameki = self.players.get(&attacker_pid)
                .and_then(|p| p.field.iter().find(|c| c.instance_id == actor_id))
                .and_then(|c| self.catalog.get(&c.definition_id))
                .map(|d| d.description.contains("アクション［事件］したとき、アクション終了時まで相手の【ヒラメキ】は発動しない"))
                .unwrap_or(false);
            if blocks_opp_hirameki {
                self.action_hirameki_blocked_player = Some(defender_pid.clone());
            }
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
                self.action_hirameki_blocked_player = None;
                if opened_hirameki_ui {
                    return;
                }
            } else {
                self.action_hirameki_blocked_player = None;
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
                .map(|c| self.get_effective_ap_with_contact(&attacker_pid, c, true))
                .unwrap_or(0);
            let def_ap = self.players.get(&defender_pid)
                .and_then(|p| p.field.iter().find(|c| c.instance_id == def_cid))
                .map(|c| self.get_effective_ap_with_contact(&defender_pid, c, true))
                .unwrap_or(0);

            if let (Some(a_def_id), Some(d_def_id)) = (actor_def_id, def_def_id) {
                let actor_name = self.card_name_by_def(&a_def_id);
                let def_name = self.card_name_by_def(&d_def_id);

                if actor_ap >= def_ap {
                    if let Some((rem_def, removed_sets)) = self.move_field_char_to_remove(&defender_pid, &def_cid, true, false) {
                        self.log(format!("💥 バトル決着: [{}] (AP{}) vs [{}] (AP{}) ➔ {} の [{}] をリムーブ！", actor_name, actor_ap, def_name, def_ap, defender_pid, def_name));
                        self.trigger_on_remove(&defender_pid, &rem_def);
                        for (sid, sdid) in removed_sets {
                            self.trigger_set_card_on_remove(&defender_pid, &sid, &sdid);
                        }
                        // コンタクト勝利時トリガーの解決
                        self.trigger_on_contact_win(&attacker_pid, &actor_id, &a_def_id);
                    } else {
                        self.log(format!("🛡️ [{}] は身代わり/効果によりコンタクトでのリムーブを免れました！", def_name));
                    }
                } else {
                    self.log(format!("🛡️ バトル決着: [{}] (AP{}) vs [{}] (AP{}) ➔ AP不足のためリムーブできず（守備成功）", actor_name, actor_ap, def_name, def_ap));
                }
            }
        }

        // コンタクト終了：カットイン等によるコンタクト中のAP増減はここで切れる
        if !self.contact_ap_buffs.is_empty() {
            let buffs = format!("{:?}", self.contact_ap_buffs);
            self.debug_log(format!("[DEBUG][CONTACT_END] contact_ap_buffs expired: {}", buffs));
        }
        self.contact_ap_buffs.clear();
        if !self.drain_pending_clause_queue() {
            self.prompt_main_action(&attacker_pid);
        }
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

        // イベントの「以下から〜選んで行う」モード選択チェック
        if is_event {
            if let Some(mc) = parse_event_mode_choice(&def.description) {
                let resume_ctx = "MAIN";
                return self.start_event_mode_choice(
                    player_id,
                    actor_id,
                    &card_name,
                    Some(def.level),
                    &mc,
                    resume_ctx,
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
                            resume_context: "MAIN".to_string(),
                            evidence_source_inst_id: None,
                            post_target_ops: vec![],
                            plan: None,
                        },
                    );
                }
            }
            if self.execute_clause_plan(player_id, actor_id, &card_name, src_lv, &plan, "MAIN", None) {
                return true;
            }
            // 即時完結した場合は既にキューに入れた残り句をそのままループで処理するので一旦クリアは不要 — すでに execute_clause_plan が false ならここでキューを消化して戻す
            if has_remaining {
                return self.drain_pending_clause_queue();
            }
        }
        false
    }

    fn contact_char_matches_req(&self, player_id: &str, req_phrase: &str) -> bool {
        let Some(cid) = self.get_contact_char_id_for_player(player_id) else {
            return false;
        };
        let Some(ps) = self.players.get(player_id) else {
            return false;
        };
        let Some(cc) = ps.field.iter().find(|c| c.instance_id == cid) else {
            return false;
        };
        let Some(cdef) = self.catalog.get(&cc.definition_id) else {
            return false;
        };
        let eff_ap = self.get_effective_ap(player_id, cc);
        let mut eff_def = cdef.clone();
        eff_def.level = cdef.level + cc.temp_level_delta;
        let branches = crate::effects::parse_branches_from_phrase(req_phrase, CardType::CHARACTER);
        branches.iter().any(|b| {
            if let Some(ref nm) = b.name_req {
                if has_instance_name_match(cc, &eff_def, nm) {
                    let mut b_no_name = b.clone();
                    b_no_name.name_req = None;
                    return branch_matches(&eff_def, eff_ap, &b_no_name);
                }
                return false;
            }
            branch_matches(&eff_def, eff_ap, b)
        })
    }

    fn apply_ap_buff_to_contact_char(&mut self, player_id: &str, buff_source_name: &str, amt: i32) {
        if let Some(cid) = self.get_contact_char_id_for_player(player_id) {
            if let Some(ps) = self.players.get_mut(player_id) {
                if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == cid) {
                    *self.contact_ap_buffs.entry(c.instance_id.clone()).or_insert(0) += amt;
                    let target_name = self
                        .catalog
                        .get(&c.definition_id)
                        .and_then(|d| d.names.first().cloned())
                        .unwrap_or_default();
                    self.log(format!("⚡ [{}] の効果で [{}] のAP +{}", buff_source_name, target_name, amt));
                }
            }
        }
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
            let cond_check = {
                let ctx = self.build_cond_info(player_id, Some(cutin_instance_id), CardOrientation::ACTIVE, None);
                check_clause_conditions(&clause, &ctx)
            };
            if let Err(reason) = cond_check {
                self.log(format!("⚠️ [{}] のカットイン条件未達: {}", card_name, reason));
                continue;
            }
            let cost = parse_clause_cost(&clause);
            if !self.pay_clause_cost(player_id, cutin_instance_id, &cost) {
                self.log(format!("⚠️ [{}] のカットインコスト不足", card_name));
                continue;
            }
            let clean = crate::effects::strip_parens(&clause);
            let body = crate::effects::strip_leading_tags(&clean).trim();

            // 1) 「自分の現場に空きがある場合、スリープ状態で登場させる」系カットイン
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

            // 2) B10091: 「〜に【カットイン】した場合、このキャラをリムーブエリアから登場させる」
            if body.contains("に【カットイン】した場合、このキャラをリムーブエリアから登場させる") {
                if let Some((req_target, _)) = body.split_once("に【カットイン】した場合、") {
                    if self.contact_char_matches_req(player_id, req_target.trim()) {
                        let turn = self.turn;
                        let mut deployed = false;
                        if let Some(ps) = self.players.get_mut(player_id) {
                            if ps.field.len() < 5 {
                                if let Some(idx) = ps.remove.iter().position(|c| c.instance_id == cutin_instance_id) {
                                    let mut c = ps.remove.remove(idx);
                                    c.zone = Zone::FIELD;
                                    c.orientation = CardOrientation::ACTIVE;
                                    c.entered_turn = turn;
                                    c.face_up = true;
                                    ps.field.push(c);
                                    deployed = true;
                                }
                            }
                        }
                        if deployed {
                            self.self_entered_field_players_this_turn.insert(player_id.to_string());
                            self.log(format!("⚡ [{}] のカットイン効果でリムーブエリアから現場に登場！", card_name));
                            if self.resolve_character_enter_field_with_source(
                                player_id,
                                cutin_instance_id,
                                definition_id,
                                CardOrientation::ACTIVE,
                                Some(def.level),
                                "GUARD",
                            ) {
                                return true;
                            }
                        }
                    }
                }
                continue;
            }

            // 3) B03039: 「AP＋1000、相手の現場にいるキャラに裏向きでセットされているカードを1枚選び、リムーブしてもよい。そうした場合、代わりにAP＋3000」
            if clean.contains("代わりにAP＋") && clean.contains("裏向きでセットされているカード") {
                let base_ap = crate::effects::parse_signed_ap(
                    clean.split("相手の現場").next().unwrap_or(&clean),
                    true,
                )
                .unwrap_or(1000);
                let alt_ap = clean
                    .split("代わりに")
                    .nth(1)
                    .and_then(|s| crate::effects::parse_signed_ap(s, true))
                    .unwrap_or(3000);
                self.apply_ap_buff_to_contact_char(player_id, &card_name, base_ap);
                let bonus_diff = alt_ap - base_ap;
                let mut set_f = TargetFilter {
                    owner: OwnerScope::OppOnly,
                    zone: TargetZone::SetCards,
                    face_down_only: true,
                    count: 1,
                    up_to: true,
                    any_card_type: true,
                    branches: vec![crate::effects::TargetBranch::default()],
                    ..Default::default()
                };
                set_f.face_down_only = true;
                let ts = TargetStep {
                    action: TargetAction::RemoveSetCards(1),
                    filter: set_f,
                    bonuses: vec![],
                    follow_up: if bonus_diff != 0 {
                        vec![ImmediateOp::SelfApBuff(bonus_diff)]
                    } else {
                        vec![]
                    },
                    conditional_follow_up: None,
                    optional: true,
                    then_abort_if_skipped: false,
                    scale_by_cost_flipped_evidence: false,
                };
                if self.prompt_target_step(player_id, cutin_instance_id, &card_name, Some(def.level), &ts, None, "GUARD", None) {
                    return true;
                }
                continue;
            }

            // 4) B05040 / B08055: 手札を1枚リムーブしてレベル/APに応じたAPバフ
            if clean.contains("リムーブしたカードのレベル1につき、AP＋") {
                let per_lv = clean
                    .split("レベル1につき、")
                    .nth(1)
                    .and_then(|s| crate::effects::parse_signed_ap(s, true))
                    .unwrap_or(1000);
                let sub_plan = ParsedClausePlan {
                    immediate_before: vec![
                        ImmediateOp::OptionalDiscardHand { count: 1, filter: None },
                        ImmediateOp::CutinBuffByRemovedLevel(per_lv),
                    ],
                    ..Default::default()
                };
                if self.execute_clause_plan(player_id, cutin_instance_id, &card_name, Some(def.level), &sub_plan, "GUARD", None) {
                    return true;
                }
                continue;
            }
            if clean.contains("リムーブしたキャラのAP1000につき、AP＋") {
                let per_k = clean
                    .split("AP1000につき、")
                    .nth(1)
                    .and_then(|s| crate::effects::parse_signed_ap(s, true))
                    .unwrap_or(1000);
                let char_f = TargetFilter {
                    owner: OwnerScope::SelfOnly,
                    zone: TargetZone::Hand,
                    count: 1,
                    card_type_filter: Some(CardType::CHARACTER),
                    branches: vec![crate::effects::TargetBranch {
                        card_types: vec![CardType::CHARACTER],
                        ..Default::default()
                    }],
                    ..Default::default()
                };
                let sub_plan = ParsedClausePlan {
                    immediate_before: vec![
                        ImmediateOp::OptionalDiscardHand { count: 1, filter: Some(char_f) },
                        ImmediateOp::CutinBuffByRemovedApThousand(per_k),
                    ],
                    ..Default::default()
                };
                if self.execute_clause_plan(player_id, cutin_instance_id, &card_name, Some(def.level), &sub_plan, "GUARD", None) {
                    return true;
                }
                continue;
            }

            // 5) 条件による代替AP（「AP＋1000、Xの場合、代わりにAP＋3000」等）
            if body.contains("代わりにAP＋") {
                if let Some((before_repl, after_repl)) = body.split_once("代わりに") {
                    let alt_ap = crate::effects::parse_signed_ap(after_repl, true).unwrap_or(3000);
                    let base_ap = before_repl
                        .split('、')
                        .next()
                        .and_then(|s| crate::effects::parse_signed_ap(s, true))
                        .unwrap_or(1000);
                    let cond_raw = before_repl
                        .split_once('、')
                        .map(|(_, r)| r.trim())
                        .unwrap_or("")
                        .trim_end_matches("場合、")
                        .trim_end_matches("場合");
                    let cond_met = if let Some((treq, _)) = cond_raw.split_once("に【カットイン】") {
                        self.contact_char_matches_req(player_id, treq.trim())
                    } else if let Some(max_h) = crate::effects::extract_number_before(cond_raw, "手札が", "枚以下") {
                        self.players.get(player_id).map(|ps| ps.hand.len() <= max_h as usize).unwrap_or(false)
                    } else {
                        let ctx2 = self.build_cond_info(player_id, Some(cutin_instance_id), CardOrientation::ACTIVE, None);
                        crate::effects::eval_cond_phrase(cond_raw, &ctx2)
                    };
                    let final_ap = if cond_met { alt_ap } else { base_ap };
                    self.apply_ap_buff_to_contact_char(player_id, &card_name, final_ap);
                    if after_repl.contains("カードを") && after_repl.contains("引く") {
                        let d = parse_draw_count(after_repl).max(1);
                        if cond_met {
                            self.draw_cards(player_id, d);
                            self.log(format!("⚡ [{}] のカットイン追加効果でカードを{}枚引きました", card_name, d));
                        }
                    }
                    continue;
                }
            }

            // 6) 現場・リムーブ枚数によるスケールAP（「自分の現場/リムーブにある〜1枚につき、AP＋N」）
            if clean.contains("1枚につき、AP＋") {
                let per_card = crate::effects::parse_signed_ap(&clean, true).unwrap_or(1000);
                if let Some((scope_part, _)) = body.split_once("1枚につき、") {
                    let branches = crate::effects::parse_branches_from_phrase(scope_part, CardType::CHARACTER);
                    let cnt = if scope_part.contains("リムーブエリア") {
                        self.players.get(player_id).map(|ps| {
                            ps.remove.iter().filter(|rc| {
                                self.catalog.get(&rc.definition_id).map(|rdef| {
                                    branches.iter().any(|b| branch_matches(rdef, rdef.ap, b))
                                }).unwrap_or(false)
                            }).count()
                        }).unwrap_or(0)
                    } else {
                        self.players.get(player_id).map(|ps| {
                            ps.field.iter().filter(|fc| {
                                let fap = self.get_effective_ap(player_id, fc);
                                self.catalog.get(&fc.definition_id).map(|fdef| {
                                    branches.iter().any(|b| branch_matches(fdef, fap, b))
                                }).unwrap_or(false)
                            }).count()
                        }).unwrap_or(0)
                    };
                    let total_buff = (cnt as i32) * per_card;
                    if total_buff != 0 {
                        self.apply_ap_buff_to_contact_char(player_id, &card_name, total_buff);
                    }
                    continue;
                }
            }

            // 7) B10086 等: 「AP＋1000、〜に【カットイン】した場合 / 自分の事件が〜場合、カードを1枚引く」
            if body.contains("場合、カードを")
                && (body.contains("に【カットイン】した場合、カードを")
                    || body.contains("以外の色を持たない場合、カードを"))
            {
                if let Some((before_c, after_c)) = body.split_once("場合、") {
                    if let Some((ap_part, cond_part)) = before_c.split_once('、') {
                        let cond_met = if let Some((treq, _)) = cond_part.split_once("に【カットイン】") {
                            self.contact_char_matches_req(player_id, treq.trim())
                        } else {
                            let ctx2 = self.build_cond_info(player_id, Some(cutin_instance_id), CardOrientation::ACTIVE, None);
                            crate::effects::eval_cond_phrase(cond_part, &ctx2)
                        };
                        if let Some(ap) = crate::effects::parse_signed_ap(ap_part, true) {
                            self.apply_ap_buff_to_contact_char(player_id, &card_name, ap);
                        }
                        if cond_met {
                            let d = parse_draw_count(after_c).max(1);
                            self.draw_cards(player_id, d);
                            self.log(format!("⚡ [{}] のカットイン効果でカードを{}枚引きました", card_name, d));
                        }
                        continue;
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

    fn trigger_on_cutin_or_disguise_used(
        &mut self,
        user_pid: &str,
        cutin_def_id: Option<&str>,
        is_disguise: bool,
    ) -> bool {
        let used_def = cutin_def_id.and_then(|did| self.catalog.get(did)).cloned();
        let pids = self.player_order.clone();
        for watch_pid in pids {
            let is_user = watch_pid == user_pid;
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

            for (watcher_id, wdef_id, ori, in_partner) in watchers {
                let Some(wdef) = self.catalog.get(&wdef_id).cloned() else {
                    continue;
                };
                let watcher_name = self.card_name_by_def(&wdef_id);
                let norm_desc = wdef.description.replace("\r\n", "\n").replace('\r', "\n");
                for raw_line in norm_desc.lines().map(str::trim).filter(|l| !l.is_empty()) {
                    let clean = strip_parens(raw_line);
                    if !clean.contains("【カットイン】を使用したとき") && !clean.contains("【カットイン】か【変装】を使用したとき") {
                        continue;
                    }
                    if clean.starts_with("【カットイン】") || clean.contains("「") {
                        continue;
                    }
                    if in_partner && wdef.card_type != CardType::PARTNER && !allows_partner_area(raw_line) {
                        continue;
                    }
                    let ctx = self.build_cond_info(&watch_pid, Some(&watcher_id), ori, None);
                    if check_clause_conditions(raw_line, &ctx).is_err() {
                        continue;
                    }
                    if !self.can_use_clause_this_turn(&watcher_id, raw_line) {
                        continue;
                    }
                    let expects_opp = clean.contains("相手が【カットイン】");
                    if expects_opp {
                        if is_user {
                            continue;
                        }
                        if is_disguise && !clean.contains("か【変装】") {
                            continue;
                        }
                    } else {
                        if !is_user || is_disguise {
                            continue;
                        }
                    }
                    if clean.contains("このキャラのコンタクト中に") && !self.is_char_in_current_contact(&watch_pid, &watcher_id) {
                        continue;
                    }
                    let contact_cid = self.get_contact_char_id_for_player(&watch_pid);
                    if clean.contains("自分の現場にいる") && clean.contains("キャラのコンタクト中に") {
                        let Some(ref ccid) = contact_cid else {
                            continue;
                        };
                        if let Some(tr) = extract_bracketed_value(&clean, "特徴［", "］") {
                            let tr_ok = self
                                .players
                                .get(&watch_pid)
                                .and_then(|ps| ps.field.iter().find(|c| c.instance_id == *ccid))
                                .and_then(|c| self.catalog.get(&c.definition_id))
                                .map(|cd| has_trait_match(cd, tr))
                                .unwrap_or(false);
                            if !tr_ok {
                                continue;
                            }
                        }
                    }
                    if let Some((_, req_cutin_part)) = clean.split_once("自分が").and_then(|(_, r)| r.split_once("の【カットイン】を使用したとき")) {
                        if req_cutin_part.contains("〚") {
                            let Some(ref cd) = used_def else {
                                continue;
                            };
                            let branches = crate::effects::parse_branches_from_phrase(req_cutin_part, CardType::CHARACTER);
                            if !branches.iter().any(|b| branch_matches(cd, cd.ap, b)) {
                                continue;
                            }
                        }
                    }

                    let cost = parse_clause_cost(raw_line);
                    if !self.pay_clause_cost(&watch_pid, &watcher_id, &cost) {
                        continue;
                    }
                    self.mark_clause_used_this_turn(&watcher_id, raw_line);
                    self.log(format!("⚡ [{}] の誘発効果（カットイン使用時）が発動！", watcher_name));

                    let body = clean.split("を使用したとき、").nth(1).unwrap_or(&clean);
                    if body.contains("そのキャラをAP＋") {
                        if let Some(ap) = crate::effects::parse_signed_ap(body, true) {
                            self.apply_ap_buff_to_contact_char(&watch_pid, &watcher_name, ap);
                        }
                        continue;
                    }
                    if body.contains("このキャラをAP＋") {
                        if let Some(ap) = crate::effects::parse_signed_ap(body, true) {
                            if let Some(ps) = self.players.get_mut(&watch_pid) {
                                if let Some(wc) = ps.field.iter_mut().find(|c| c.instance_id == watcher_id) {
                                    wc.temp_ap_bonus += ap;
                                    self.log(format!("⚡ [{}] の効果で自身のAP +{}", watcher_name, ap));
                                }
                            }
                        }
                        continue;
                    }
                    if body.contains("このキャラをアクティブにし") {
                        if let Some(ps) = self.players.get_mut(&watch_pid) {
                            if let Some(wc) = ps.field.iter_mut().find(|c| c.instance_id == watcher_id) {
                                wc.orientation = CardOrientation::ACTIVE;
                                self.log(format!("✨ [{}] をアクティブにしました！", watcher_name));
                            }
                        }
                        continue;
                    }
                    if body.contains("カードを") && body.contains("引く") && !body.contains("選び") {
                        let d = parse_draw_count(body).max(1);
                        self.draw_cards(&watch_pid, d);
                        self.log(format!("✨ [{}] の効果でカードを{}枚引きました", watcher_name, d));
                        continue;
                    }

                    let plan = parse_effect_steps(raw_line, None, wdef.card_type);
                    if self.execute_clause_plan(&watch_pid, &watcher_id, &watcher_name, Some(wdef.level), &plan, "GUARD", None) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// フィールド上のキャラの実効APを計算する（ベースAP + 一時バフ + パッシブ自己/オーラ）
    pub fn get_effective_ap(&self, owner_id: &str, card: &CardInstance) -> i32 {
        let in_contact = self.is_card_in_active_contact(owner_id, &card.instance_id);
        self.get_effective_ap_with_contact(owner_id, card, in_contact)
    }

    fn is_card_in_active_contact(&self, owner_id: &str, inst_id: &str) -> bool {
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
        let mut total = target_def.lp;
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

    /// キャラがフィールドを離れる際の保護チェック（B01039/B05041/B11050）とリムーブ処理
    fn move_field_char_to_remove(
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

        let mut removed_sets: Vec<(String, String)> = vec![];
        let mut removed_def: Option<String> = None;
        if let Some(ps) = self.players.get_mut(owner_pid) {
            if let Some(idx) = ps.field.iter().position(|c| c.instance_id == char_inst_id) {
                let mut card = ps.field.remove(idx);
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
                        other_kaito.set_cards.push(kcard);
                    } else {
                        let mut kcard = kcard;
                        kcard.zone = Zone::REMOVE;
                        kcard.face_up = true;
                        removed_sets.push((kcard.instance_id.clone(), kcard.definition_id.clone()));
                        ps.remove.push(kcard);
                    }
                }
                card.zone = Zone::REMOVE;
                card.face_up = true;
                ps.remove.push(card);
            }
        }
        removed_def.map(|d| (d, removed_sets))
    }

    fn trigger_set_card_on_remove(&mut self, owner_id: &str, set_inst_id: &str, set_def_id: &str) {
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

    fn extract_set_card_granted_clauses(&self, desc: &str, trigger: TriggerKind) -> Vec<String> {
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

    fn check_and_pay_set_card_cost(&mut self, player_id: &str, host_id: &str, clause: &str, dry_run: bool) -> bool {
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

    /// 現場リムーブ時トリガーを解決する（インスタンスID・自分の能力/効果によるリムーブかを指定）
    pub fn trigger_on_remove_char(
        &mut self,
        owner_id: &str,
        instance_id: &str,
        definition_id: &str,
        by_own_ability: bool,
    ) {
        let Some(def) = self.catalog.get(definition_id).cloned() else {
            return;
        };
        let clauses = extract_clauses(&def.description, TriggerKind::OnRemove, def.card_type);
        if clauses.is_empty() {
            return;
        }
        let card_name = self.card_name_by_def(definition_id);

        for clause in clauses {
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
            let plan = parse_effect_steps(&clause, None, def.card_type);
            let _ = self.execute_clause_plan(owner_id, instance_id, &card_name, Some(def.level), &plan, "MAIN", None);
        }
        if !instance_id.is_empty() && self.granted_on_remove_draw_until_opp_end.remove(instance_id).is_some() {
            self.draw_cards(owner_id, 1);
            self.log(format!("🎴 [{}] に付与された【現場リムーブ時】効果でカードを1枚引きました！", card_name));
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
                        for clause in extract_clauses(&c_def.description, TriggerKind::OnTurnEnd, c_def.card_type) {
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
                    if !check_action_trigger_match(
                        &clause,
                        kind,
                        owner_is_acting,
                        &watcher_id,
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
                    if self.execute_clause_plan(
                        &watch_pid,
                        &watcher_id,
                        &watcher_name,
                        Some(def.level),
                        &plan,
                        resume_context,
                        None,
                    ) {
                        return true;
                    }
                }
            }
        }
        false
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
        let Some(played_def) = self.catalog.get(played_def_id).cloned() else {
            return false;
        };
        let played_ap = self
            .players
            .get(playing_pid)
            .and_then(|ps| ps.field.iter().find(|c| c.instance_id == played_inst_id))
            .map(|c| self.get_effective_ap(playing_pid, c))
            .unwrap_or(played_def.ap);

        let opp_pid = self.opponent_id_of(playing_pid);
        let watch_sides: Vec<(String, bool)> = vec![(playing_pid.to_string(), true), (opp_pid, false)];

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
                    if self.execute_clause_plan(
                        &watch_pid,
                        &watcher_id,
                        &watcher_name,
                        Some(def.level),
                        &plan,
                        resume_context,
                        None,
                    ) {
                        return true;
                    }
                }
            }
        }
        false
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

        if self.resolve_on_play_effects_with_source(
            player_id,
            actor_id,
            definition_id,
            false,
            source_effect_level,
        ) {
            return true;
        }

        self.try_fire_pending_ally_play(resume_context)
    }

    fn try_fire_pending_ally_play(&mut self, resume_context: &str) -> bool {
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

    fn try_fire_pending_hint_trigger(&mut self, player_id: &str, after_hint_level: Option<i32>) -> bool {
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

    fn is_action_target_still_valid(&self) -> bool {
        let Some(ctx) = self.action_context.as_ref() else {
            return false;
        };
        let target_id = ctx.get("targetId").and_then(|v| v.as_str()).unwrap_or("");
        let def_pid = ctx.get("defenderPlayerId").and_then(|v| v.as_str()).unwrap_or("");
        let Some(def_ps) = self.players.get(def_pid) else {
            return false;
        };
        if def_ps.r#case.as_ref().map(|c| c.instance_id == target_id).unwrap_or(false) {
            return true;
        }
        def_ps.field.iter().any(|c| c.instance_id == target_id)
    }

    fn clause_usage_key(instance_id: &str, clause: &str) -> String {
        format!("{}::{}", instance_id, clause)
    }

    fn can_use_clause_this_turn(&self, instance_id: &str, clause: &str) -> bool {
        let Some(limit) = clause_turn_limit(clause) else {
            return true;
        };
        let key = Self::clause_usage_key(instance_id, clause);
        let used = self.turn_clause_usage.get(&key).copied().unwrap_or(0);
        used < limit
    }

    fn mark_clause_used_this_turn(&mut self, instance_id: &str, clause: &str) {
        if clause_turn_limit(clause).is_some() {
            let key = Self::clause_usage_key(instance_id, clause);
            *self.turn_clause_usage.entry(key).or_insert(0) += 1;
        }
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
        true
    }

    fn pay_clause_cost(&mut self, player_id: &str, actor_id: &str, cost: &crate::effects::ClauseCost) -> bool {
        self.pay_clause_cost_internal(player_id, actor_id, cost, true)
    }

    fn pay_clause_cost_internal(
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
                        if ps.field.len() < 5 {
                            if let Some(pos) = ps.remove.iter().position(|c| c.instance_id == actor_id) {
                                let mut card = ps.remove.remove(pos);
                                card.zone = Zone::FIELD;
                                card.orientation = if *sleep { CardOrientation::SLEEP } else { CardOrientation::ACTIVE };
                                card.entered_turn = turn;
                                card.face_up = true;
                                ps.field.push(card);
                                revived = true;
                            }
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
                        if ps.field.len() < 5 {
                            if let Some(pos) = ps.hand.iter().position(|c| c.instance_id == actor_id) {
                                let mut card = ps.hand.remove(pos);
                                card.zone = Zone::FIELD;
                                card.orientation = if *sleep { CardOrientation::SLEEP } else { CardOrientation::ACTIVE };
                                card.entered_turn = turn;
                                card.face_up = true;
                                ps.field.push(card);
                                played = true;
                            }
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
                    let ok = crate::effects::eval_cond_phrase(condition, &cond_ctx);
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
    fn enqueue_continuation_from_ops(
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

    fn build_filtered_candidates(
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
        }

        if (has_deferred && !has_opt_cost_gate) || self.pending_clause_queue.len() > queue_len_before {
            return self.drain_pending_clause_queue();
        }
        false
    }

    #[allow(clippy::too_many_arguments)]
    fn prompt_deck_search_step(
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
    fn prompt_hand_discard(
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

    fn prompt_target_step(
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
    fn prompt_target_step_with_plan(
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

    pub fn get_contact_char_id_for_player(&self, player_id: &str) -> Option<String> {
        let ctx = self.action_context.as_ref()?;
        let att_pid = ctx.get("attackerPlayerId").and_then(|v| v.as_str()).unwrap_or("");
        let def_pid = ctx.get("defenderPlayerId").and_then(|v| v.as_str()).unwrap_or("");
        let actor_id = ctx.get("actorId").and_then(|v| v.as_str()).unwrap_or("");
        let target_id = ctx.get("targetId").and_then(|v| v.as_str()).unwrap_or("");
        let guard_id = ctx.get("guardCardId").and_then(|v| v.as_str());
        if player_id == att_pid && !actor_id.is_empty() {
            return Some(actor_id.to_string());
        }
        if player_id == def_pid {
            if let Some(gid) = guard_id {
                if !gid.is_empty() {
                    return Some(gid.to_string());
                }
            }
            if !target_id.is_empty() {
                let is_case = self
                    .players
                    .get(def_pid)
                    .and_then(|ps| ps.r#case.as_ref())
                    .map(|cs| cs.instance_id == target_id)
                    .unwrap_or(false);
                if !is_case {
                    return Some(target_id.to_string());
                }
            }
        }
        None
    }

    pub fn is_char_in_current_contact(&self, owner_id: &str, inst_id: &str) -> bool {
        self.get_contact_char_id_for_player(owner_id).as_deref() == Some(inst_id)
    }

    pub fn is_contact_aborted(&self) -> bool {
        let Some(ctx) = self.action_context.as_ref() else {
            return true;
        };
        let att_pid = ctx.get("attackerPlayerId").and_then(|v| v.as_str()).unwrap_or("");
        let def_pid = ctx.get("defenderPlayerId").and_then(|v| v.as_str()).unwrap_or("");
        let actor_id = ctx.get("actorId").and_then(|v| v.as_str()).unwrap_or("");
        let target_id = ctx.get("targetId").and_then(|v| v.as_str()).unwrap_or("");
        let guard_id = ctx.get("guardCardId").and_then(|v| v.as_str());

        let attacker_alive = self
            .players
            .get(att_pid)
            .map(|ps| ps.field.iter().any(|c| c.instance_id == actor_id))
            .unwrap_or(false);
        if !attacker_alive {
            return true;
        }
        if let Some(gid) = guard_id {
            if !gid.is_empty() {
                let guard_alive = self
                    .players
                    .get(def_pid)
                    .map(|ps| ps.field.iter().any(|c| c.instance_id == gid))
                    .unwrap_or(false);
                return !guard_alive;
            }
        }
        let is_case_target = self
            .players
            .get(def_pid)
            .and_then(|ps| ps.r#case.as_ref())
            .map(|cs| cs.instance_id == target_id)
            .unwrap_or(false);
        if is_case_target {
            return false;
        }
        let target_alive = self
            .players
            .get(def_pid)
            .map(|ps| ps.field.iter().any(|c| c.instance_id == target_id))
            .unwrap_or(false);
        !target_alive
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

    fn attacker_forces_guard(&self, player_id: &str, actor_id: &str) -> bool {
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

    fn is_protected_from_opp_sleep_or_stun(&self, _owner_id: &str, c: &CardInstance) -> bool {
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

    fn is_protected_from_opp_target(&self, owner_id: &str, c: &CardInstance) -> bool {
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

    fn is_hirameki_blocked_for(&self, victim_id: &str) -> bool {
        if self.action_hirameki_blocked_player.as_deref() == Some(victim_id) {
            return true;
        }
        let opp_id = self.opponent_id_of(victim_id);
        if let Some(opp_ps) = self.players.get(&opp_id) {
            for c in opp_ps.field.iter().chain(opp_ps.partner.iter()) {
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
