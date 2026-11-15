use crate::catalog::CardCatalog;
use crate::models::{CardDefinition, CardInstance, CardOrientation, CardType, CasePhase};
use serde::{Deserialize, Serialize};

/// 能力・効果のトリガー種別
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerKind {
    OnPlay,
    Declare,
    Cutin,
    Hirameki,
    DisguiseCond,
    OnDisguise,
    OnRemove,
    OnTurnEnd,
    OnAction,
    OnContact,
    OnContactWin,
    OnHintUse,
    OnAllyPlay,
    Passive,
}

/// ターゲット所有者のスコープ
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OwnerScope {
    SelfOnly,
    OppOnly,
    Both,
}

/// ターゲット探索対象ゾーン
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TargetZone {
    Field,
    Remove,
    Hand,
    Deck,
    Evidence,
    SetCards,
}

/// ターゲットに対して実行するアクション
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TargetAction {
    Sleep,
    Stun,
    Activate,
    Remove,
    BottomDeck,
    ApBuff(i32),
    ApDebuff(i32),
    GiveRush(Option<String>),
    ReviveActive,
    ReviveSleep,
    RecoverToHand,
    PlayFromHand,
    DiscardHand,
    BounceToHand,
    SetThisEvent,
    SetDeckTopFaceDown(usize),
    GiveBullet,
    GiveCanAttackActive,
    SetBaseApZero,
    LevelDelta(i32),
    ReviveSplitGonin,
    ReviveSplitBakana,
    FlipEvidenceFaceUp,
    FlipEvidenceFaceDown,
    MoveEvidenceToHand,
    PlaceHandCardAsEvidenceFaceDown,
    TurnIntoOwnerFaceUpEvidence,
    PlayEventFromHand,
    DeckTopFromRemove,
    CopyNameFromOtherSelfChar,
    GrantContactForceAttack,
    GrantBulletOnCaseAttackDiscard,
    RemoveSetCards(usize),
    StunAndGrantOnRemoveDraw,
    StackUnderSelf,
}

impl TargetAction {
    /// クライアント側UI・回答互換のための effectType 文字列
    pub fn as_effect_type(&self) -> &'static str {
        match self {
            TargetAction::Sleep => "SLEEP",
            TargetAction::Stun => "STUN",
            TargetAction::Activate => "ACTIVATE",
            TargetAction::Remove => "REMOVE",
            TargetAction::BottomDeck => "BOTTOM_DECK",
            TargetAction::ApBuff(_) => "AP_BUFF",
            TargetAction::ApDebuff(_) => "AP_DEBUFF",
            TargetAction::GiveRush(_) => "GIVE_RUSH",
            TargetAction::ReviveActive => "REVIVE_ACTIVE",
            TargetAction::ReviveSleep => "REVIVE_SLEEP",
            TargetAction::RecoverToHand => "RECOVER_TO_HAND",
            TargetAction::PlayFromHand => "PLAY_FROM_HAND",
            TargetAction::DiscardHand => "DISCARD_HAND",
            TargetAction::BounceToHand => "BOUNCE_TO_HAND",
            TargetAction::SetThisEvent => "SET_THIS_EVENT",
            TargetAction::SetDeckTopFaceDown(_) => "SET_DECK_TOP_FACE_DOWN",
            TargetAction::GiveBullet => "GIVE_BULLET",
            TargetAction::GiveCanAttackActive => "GIVE_CAN_ATTACK_ACTIVE",
            TargetAction::SetBaseApZero => "SET_BASE_AP_ZERO",
            TargetAction::LevelDelta(_) => "LEVEL_DELTA",
            TargetAction::ReviveSplitGonin => "REVIVE_SPLIT_GONIN",
            TargetAction::ReviveSplitBakana => "REVIVE_SPLIT_BAKANA",
            TargetAction::FlipEvidenceFaceUp => "FLIP_EVIDENCE_FACE_UP",
            TargetAction::FlipEvidenceFaceDown => "FLIP_EVIDENCE_FACE_DOWN",
            TargetAction::MoveEvidenceToHand => "MOVE_EVIDENCE_TO_HAND",
            TargetAction::PlaceHandCardAsEvidenceFaceDown => "PLACE_HAND_AS_EVIDENCE",
            TargetAction::TurnIntoOwnerFaceUpEvidence => "TURN_INTO_FACE_UP_EVIDENCE",
            TargetAction::PlayEventFromHand => "PLAY_EVENT_FROM_HAND",
            TargetAction::DeckTopFromRemove => "DECK_TOP_FROM_REMOVE",
            TargetAction::CopyNameFromOtherSelfChar => "COPY_NAME_FROM_OTHER_CHAR",
            TargetAction::GrantContactForceAttack => "GRANT_CONTACT_FORCE_ATTACK",
            TargetAction::GrantBulletOnCaseAttackDiscard => "GRANT_BULLET_ON_CASE_DISCARD",
            TargetAction::RemoveSetCards(_) => "REMOVE_SET_CARDS",
            TargetAction::StunAndGrantOnRemoveDraw => "STUN_AND_GRANT_ON_REMOVE_DRAW",
            TargetAction::StackUnderSelf => "STACK_UNDER_SELF",
        }
    }

    /// 味方バフ・回復・展開系か（所有者省略時のデフォルト判定に使用）
    pub fn is_beneficial(&self) -> bool {
        matches!(
            self,
            TargetAction::Activate
                | TargetAction::ApBuff(_)
                | TargetAction::GiveRush(_)
                | TargetAction::ReviveActive
                | TargetAction::ReviveSleep
                | TargetAction::RecoverToHand
                | TargetAction::PlayFromHand
                | TargetAction::DiscardHand
                | TargetAction::SetThisEvent
                | TargetAction::SetDeckTopFaceDown(_)
                | TargetAction::GiveBullet
                | TargetAction::GiveCanAttackActive
                | TargetAction::ReviveSplitGonin
                | TargetAction::ReviveSplitBakana
                | TargetAction::FlipEvidenceFaceDown
                | TargetAction::MoveEvidenceToHand
                | TargetAction::PlaceHandCardAsEvidenceFaceDown
                | TargetAction::PlayEventFromHand
                | TargetAction::DeckTopFromRemove
                | TargetAction::CopyNameFromOtherSelfChar
                | TargetAction::GrantContactForceAttack
                | TargetAction::GrantBulletOnCaseAttackDiscard
                | TargetAction::RemoveSetCards(_)
                | TargetAction::StunAndGrantOnRemoveDraw
        )
    }
}

/// 選択ターゲットに追加で付与されるボーナス効果
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TargetBonus {
    ApBuff(i32),
    GiveRush(Option<String>),
    GiveCanAttackActive,
    GiveBullet,
    DrawOnActionThisTurn,
    ContactDrawAndAp1000ThisTurn,
    BulletOnCaseAttackDiscardThisTurn,
    GrantContactForceAttackThisTurn,
    SetDeckTopFaceDown(usize),
    SetThisEvent,
    ChoiceRushOrAttackActive,
    BonusCopyOtherCharName,
    IfSoccerNoFaceDownSetTopAndDraw,
}

/// 選択肢イベント（以下から〜選んで行う）の解析構造
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventModeChoice {
    pub min_pick: usize,
    pub max_pick: usize,
    pub distinct_colors_limit: bool,
    pub pre_clauses: Vec<String>,
    pub mode_gate_condition: Option<String>,
    pub options: Vec<String>,
    pub all_condition: Option<String>,
    pub optional_all_cost: Option<String>,
    pub post_clauses: Vec<String>,
}

/// デッキ公開後の処理種別
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeckPickAction {
    ToHand,
    PlayActive,
    PlaySleep,
}

/// ターゲット条件の1ブランチ（OR 結合された候補条件の1つ）
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetBranch {
    pub trait_req: Option<String>,
    pub name_req: Option<String>,
    pub exclude_name: Option<String>,
    pub exclude_trait: Option<String>,
    pub require_keyword: Option<String>,
    pub colors: Vec<String>,
    pub card_types: Vec<CardType>,
    pub max_level: Option<i32>,
    #[serde(default)]
    pub max_level_from_file: bool,
    pub min_level: Option<i32>,
    pub max_ap: Option<i32>,
    pub min_ap: Option<i32>,
    pub exact_lp: Option<i32>,
    pub max_lp: Option<i32>,
    pub min_lp: Option<i32>,
}

/// 統一ターゲットフィルタ
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetFilter {
    pub owner: OwnerScope,
    pub zone: TargetZone,
    pub exclude_self: bool,
    pub acting_only: bool,
    pub orientations: Vec<CardOrientation>,
    pub count: usize,
    pub branches: Vec<TargetBranch>,
    #[serde(default)]
    pub up_to: bool,
    #[serde(default)]
    pub any_count: bool,
    #[serde(default)]
    pub distinct_names: bool,
    #[serde(default)]
    pub distinct_levels: bool,
    #[serde(default)]
    pub distinct_colors: bool,
    #[serde(default)]
    pub max_total_level: Option<i32>,
    #[serde(default)]
    pub min_level_from_prev: bool,
    #[serde(default)]
    pub max_level_from_prev: bool,
    #[serde(default)]
    pub max_level_from_removed_sum: bool,
    #[serde(default)]
    pub same_name_as_prev: bool,
    #[serde(default)]
    pub exclude_prev_chosen: bool,
    #[serde(default)]
    pub target_prev_chosen: bool,
    #[serde(default)]
    pub require_no_other_ability: bool,
    #[serde(default)]
    pub card_type_filter: Option<CardType>,
    #[serde(default)]
    pub any_card_type: bool,
    #[serde(default)]
    pub face_up_only: bool,
    #[serde(default)]
    pub face_down_only: bool,
    #[serde(default)]
    pub count_from_self_trait: Option<String>,
    #[serde(default)]
    pub max_lp_from_self_max: bool,
    #[serde(default)]
    pub require_in_contact: bool,
    #[serde(default)]
    pub self_only: bool,
}

impl Default for TargetFilter {
    fn default() -> Self {
        Self {
            owner: OwnerScope::Both,
            zone: TargetZone::Field,
            exclude_self: false,
            acting_only: false,
            orientations: Vec::new(),
            count: 1,
            branches: vec![TargetBranch::default()],
            up_to: false,
            any_count: false,
            distinct_names: false,
            distinct_levels: false,
            distinct_colors: false,
            max_total_level: None,
            min_level_from_prev: false,
            max_level_from_prev: false,
            max_level_from_removed_sum: false,
            same_name_as_prev: false,
            exclude_prev_chosen: false,
            target_prev_chosen: false,
            require_no_other_ability: false,
            card_type_filter: None,
            any_card_type: false,
            face_up_only: false,
            face_down_only: false,
            count_from_self_trait: None,
            max_lp_from_self_max: false,
            require_in_contact: false,
            self_only: false,
        }
    }
}

/// コスト仕様（：の左側、または明示的な事前コスト）
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClauseCost {
    pub requires_sleep: bool,
    pub hand_remove: usize,
    pub hand_filter: Option<TargetFilter>,
    pub evidence_remove: usize,
    pub evidence_flip_up: usize,
    pub evidence_flip_up_any: bool,
    pub file_remove: usize,
    pub deck_mill: usize,
    pub sacrifice_self: bool,
    pub self_to_deck_bottom: bool,
}

/// 即時処理オペレーション
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImmediateOp {
    Draw(usize),
    DiscardHand(usize),
    DiscardFilteredHand { count: usize, filter: TargetFilter },
    OptionalDiscardHand { count: usize, filter: Option<TargetFilter> },
    GainEvidence(usize),
    FlipEvidenceUp(usize),
    FlipEvidenceDown(usize),
    OppRemoveEvidence(usize),
    SelfRemoveEvidence { count: usize, trigger_hirameki: bool },
    SelfApBuff(i32),
    SelfActivate,
    SelfGiveRush(Option<String>),
    MillDeck(usize),
    MillDeckByLastChosenLevel,
    OppMillDeck(usize),
    OppDiscardHand(usize),
    OppDiscardRandom(usize),
    OppFlipEvidenceUp(usize),
    MoveSelfFromEvidenceToFieldSleep,
    MoveSelfFromEvidenceToHand,
    MoveSelfFromEvidenceToPartner,
    BlockOppDisguiseThisTurn,
    BlockOppCutinThisTurn,
    BlockHintThisTurn,
    BlockSelfEventsThisTurn,
    TriggerFieldHirameki(String),
    TriggerEvidenceHiramekiByTrait(String),
    SolveCaseVictory,
    DiscardHandTo(usize),
    DiscardAllHand,
    FileTopToHand(usize),
    OptionalFileTopToHand { count: usize, then_clause: String },
    FileTopRemove(usize),
    DeckTopRemovePerFieldChar(usize),
    OppTopEvidenceRemove(usize),
    GiveOppEvidence(usize),
    OptionalGiveOppEvidence(usize),
    GainThisAsFaceUpEvidence,
    MoveThisToPartner,
    MassOppSleepAll,
    MassAllSleepToStun,
    MassAllRemove,
    MassContactRemove,
    OptionalSelfOrientAndDiscardHand {
        target_orientation: CardOrientation,
        count: usize,
        filter: Option<TargetFilter>,
        reveal_only: bool,
    },
    CutinBuffByRemovedLevel(i32),
    CutinBuffByRemovedApThousand(i32),
    MassSelfTraitApBuff { trait_name: String, buff: i32 },
    DrawPerOppStun { if_zero_clause: Option<String> },
    InvestigateAndCheckName { count: usize, then_clause: String },
    RegisterNextOppEvidenceRemovedTrigger(String),
    ConditionalClause { condition: String, then_clause: String, else_clause: Option<String> },
    RevealDeckUntil { filter: TargetFilter, add_to_hand: bool, if_found_clause: Option<String> },
    B09033RevealDeploy { look_count: usize, filter: TargetFilter, max_repeats: usize },
    OptionalDeckTopRemove {
        count: usize,
        then_clause: String,
        black_cutin_threshold: Option<(usize, String, Option<String>)>,
    },
    OptionalCompoundCost {
        sleep_partner: bool,
        hand_remove: usize,
        file_top_remove: usize,
        field_remove_name: Option<String>,
        then_clause: String,
    },
    ConditionalLpRemoveOpp,
    EventModeChoice(EventModeChoice),
    ReviveSelfFromRemove { sleep: bool },
}

/// 対象選択ステップ
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetStep {
    pub action: TargetAction,
    pub filter: TargetFilter,
    #[serde(default)]
    pub bonuses: Vec<TargetBonus>,
    pub follow_up: Vec<ImmediateOp>,
    pub conditional_follow_up: Option<(TargetBranch, Vec<ImmediateOp>)>,
    #[serde(default)]
    pub optional: bool,
    #[serde(default)]
    pub then_abort_if_skipped: bool,
    #[serde(default)]
    pub scale_by_cost_flipped_evidence: bool,
}

/// デッキサーチステップ
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeckSearchStep {
    pub search_count: usize,
    pub pick_action: DeckPickAction,
    pub filter: TargetFilter,
    pub post_add_discard: usize,
    pub post_ops: Vec<ImmediateOp>,
    pub remainder_to_remove: bool,
    #[serde(default = "default_one_usize")]
    pub pick_count: usize,
    #[serde(default)]
    pub shuffle_remainder: bool,
    #[serde(default)]
    pub record_removed_levels: bool,
    #[serde(default)]
    pub post_clauses: Vec<String>,
}

fn default_one_usize() -> usize {
    1
}

/// 1つの能力句（clause）の解析結果
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ParsedClausePlan {
    pub immediate_before: Vec<ImmediateOp>,
    pub target_step: Option<TargetStep>,
    pub follow_up_target: Option<Box<TargetStep>>,
    pub deck_search: Option<DeckSearchStep>,
    #[serde(default)]
    pub post_target_ops: Vec<ImmediateOp>,
    #[serde(default)]
    pub extra_clauses: Vec<String>,
}

/// 条件判定に必要なプレイヤー状態ビュー
pub struct PlayerCondInfo<'a> {
    pub actor_id: Option<&'a str>,
    pub actor_orientation: CardOrientation,
    pub is_my_turn: bool,
    pub case_phase: CasePhase,
    pub file_count: usize,
    pub self_evidence_count: usize,
    pub opp_evidence_count: usize,
    pub self_field_count: usize,
    pub opp_field_count: usize,
    pub case_level: i32,
    pub partner_def: Option<&'a CardDefinition>,
    pub case_def: Option<&'a CardDefinition>,
    pub field_cards: Vec<(&'a CardInstance, &'a CardDefinition)>,
    pub played_by_effect_level: Option<i32>,
    pub self_remove_cards: Vec<(&'a CardInstance, &'a CardDefinition)>,
    pub self_partner_cards: Vec<(&'a CardInstance, &'a CardDefinition)>,
    pub self_entered_field_this_turn: bool,
    pub opp_refreshed: bool,
    pub played_by_effect: bool,
    pub last_deployed_count: usize,
    pub last_removed_black_cutin_count: usize,
    pub last_chosen_added_to_hand: bool,
    pub last_chosen_level: Option<i32>,
    pub last_chosen_name: Option<String>,
    pub last_removed_level_sum: i32,
    pub last_disguise_replaced_name: Option<String>,
    pub last_slept_names: Vec<String>,
    pub actor_effective_ap: Option<i32>,
}

/// 括弧書きの注釈 `（...）` を除去する（効果本体・ターゲット判定の誤爆防止）
pub fn strip_parens(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut depth = 0usize;
    for ch in input.chars() {
        if ch == '（' || ch == '(' {
            depth += 1;
        } else if ch == '）' || ch == ')' {
            depth = depth.saturating_sub(1);
        } else if depth == 0 {
            out.push(ch);
        }
    }
    out
}

/// 説明文を改行単位の能力句（clause）に分解し、指定トリガーに該当するものだけを返す
pub fn extract_clauses(description: &str, trigger: TriggerKind, card_type: CardType) -> Vec<String> {
    let lines: Vec<String> = description
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace("<br>", "\n")
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    lines
        .into_iter()
        .filter(|line| line_matches_trigger(line, trigger, card_type))
        .collect()
}

fn line_matches_trigger(line: &str, trigger: TriggerKind, card_type: CardType) -> bool {
    let clean = strip_parens(line);
    match trigger {
        TriggerKind::OnPlay => {
            if card_type == CardType::EVENT {
                let t = clean.trim_start();
                // イベント使用時：ヒラメキ・カットイン・相手ターン中効果・セット中継続効果・宣言等は除外
                !t.starts_with("【ヒラメキ】")
                    && !t.starts_with("【カットイン】")
                    && !t.starts_with("【相手ターン中】")
                    && !t.starts_with("【現場リムーブ時】")
                    && !t.starts_with("・")
                    && !t.contains("【宣言】")
                    && !t.starts_with("このイベントがセットされている")
                    && !t.starts_with("このイベントが、")
                    && !t.starts_with("自分か相手のターン終了時、")
                    && !(t.starts_with("【解決編】【自分ターン中】")
                        && t.contains("手札にあるこのイベントはレベル"))
            } else {
                line.contains("【登場時】")
            }
        }
        TriggerKind::Declare => {
            extract_leading_bracket_tags(line).iter().any(|t| *t == "宣言")
        }
        TriggerKind::Cutin => line.trim_start().starts_with("【カットイン】"),
        TriggerKind::Hirameki => line.trim_start().starts_with("【ヒラメキ】"),
        TriggerKind::DisguiseCond => line.contains("【変装】"),
        TriggerKind::OnDisguise => line.contains("【変装時】"),
        TriggerKind::OnRemove => {
            line.contains("【現場リムーブ時】")
                || clean.contains("自分の現場にいるこのキャラが自分の能力や効果によってリムーブされたとき")
        }
        TriggerKind::OnTurnEnd => {
            // 「ターン終了時まで〜」という継続期間表記はトリガーではない
            clean.contains("ターン終了時、")
                || clean.contains("自分のターン終了時、")
                || clean.contains("相手のターン終了時、")
        }
        TriggerKind::OnAction => {
            !clean.starts_with("このイベントがセットされている")
                && !clean.contains("アクション［事件］によって証拠を得たとき")
                && !clean.contains("アクションによって証拠を得たとき")
                && !clean.contains("ガードされたとき")
                && !clean.contains("ガードしたとき")
                && (clean.contains("アクションしたとき")
                    || clean.contains("アクション［キャラ］したとき")
                    || clean.contains("アクション［事件］したとき")
                    || clean.contains("推理したとき")
                    || clean.contains("推理かアクションしたとき"))
        }
        TriggerKind::OnContact => {
            clean.contains("コンタクトしたとき") || clean.contains("コンタクトされたとき")
        }
        TriggerKind::OnContactWin => {
            clean.contains("コンタクトで勝利したとき")
                || clean.contains("コンタクトによってリムーブされたとき")
        }
        TriggerKind::OnHintUse => {
            clean.contains("ネクストヒントで") && clean.contains("を使用したとき")
        }
        TriggerKind::OnAllyPlay => {
            !line.contains("【登場時】")
                && !line.contains("【現場リムーブ時】")
                && !clean.starts_with("このイベントがセットされている")
                && (line.contains("【疾風】") || clean.contains("登場したとき"))
        }
        TriggerKind::Passive => {
            !line.contains("【登場時】")
                && !line.contains("【宣言】")
                && !line.contains("【カットイン】")
                && !line.contains("【ヒラメキ】")
                && !line.contains("【変装】")
                && !line.contains("【変装時】")
                && !line.contains("【現場リムーブ時】")
                && !line.contains("【疾風】")
                && !clean.starts_with("このイベントがセットされている")
                && !clean.contains("とき、")
                && !clean.contains("したとき")
                && !clean.contains("されたとき")
                && !clean.contains("ターン終了時、")
                && !clean.contains("自分のターン終了時、")
                && !clean.contains("相手のターン終了時、")
        }
    }
}

/// 「このキャラがアクティブ状態の場合、〜。スリープ状態かスタン状態の場合、〜。」等の状態分岐を解決する
pub fn resolve_state_branch(clause: &str, actor_orientation: CardOrientation) -> String {
    let clean = strip_parens(clause);
    if !clean.contains("アクティブ状態の場合、") && !clean.contains("スリープ状態かスタン状態の場合、")
    {
        return clause.to_string();
    }

    let body = strip_leading_tags(&clean);
    let sentences: Vec<&str> = body
        .split('。')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    let mut selected: Vec<String> = Vec::new();
    for s in sentences {
        if s.starts_with("この能力は") || s.starts_with("この効果は") {
            continue;
        }
        if let Some(rest) = s.strip_prefix("このキャラがアクティブ状態の場合、") {
            if actor_orientation == CardOrientation::ACTIVE {
                selected.push(rest.to_string());
            }
        } else if let Some(rest) = s.strip_prefix("アクティブ状態の場合、") {
            if actor_orientation == CardOrientation::ACTIVE {
                selected.push(rest.to_string());
            }
        } else if let Some(rest) = s.strip_prefix("スリープ状態かスタン状態の場合、") {
            if actor_orientation != CardOrientation::ACTIVE {
                selected.push(rest.to_string());
            }
        } else if let Some(rest) = s.strip_prefix("このキャラがスリープ状態かスタン状態の場合、")
        {
            if actor_orientation != CardOrientation::ACTIVE {
                selected.push(rest.to_string());
            }
        } else if let Some(rest) = s.strip_prefix("スリープ状態の場合、") {
            if actor_orientation == CardOrientation::SLEEP {
                selected.push(rest.to_string());
            }
        } else {
            selected.push(s.to_string());
        }
    }

    if selected.is_empty() {
        String::new()
    } else {
        format!("{}。", selected.join("。"))
    }
}

/// 先頭の `【...】` タグ群をスキップした本文を返す
pub fn strip_leading_tags(text: &str) -> &str {
    let mut rest = text.trim();
    while rest.starts_with('【') {
        if let Some(end) = rest.find('】') {
            rest = rest[end + '】'.len_utf8()..].trim_start();
        } else {
            break;
        }
    }
    rest
}

/// 文中からすべての `【...】` タグ内容を抽出する
fn extract_bracket_tags(text: &str) -> Vec<&str> {
    let mut tags = Vec::new();
    let mut s = text;
    while let Some(start) = s.find('【') {
        let after = &s[start + '【'.len_utf8()..];
        if let Some(end) = after.find('】') {
            tags.push(&after[..end]);
            s = &after[end + '】'.len_utf8()..];
        } else {
            break;
        }
    }
    tags
}

const COLORS: &[&str] = &["青", "緑", "白", "赤", "黄", "黒"];

fn has_name_match(def: &CardDefinition, query: &str) -> bool {
    let q = strip_parens(query);
    def.names.iter().any(|n| {
        n == query
            || strip_parens(n) == q
            || n.contains(&format!("（{}）", query))
    })
}

pub fn has_trait_match(def: &CardDefinition, query: &str) -> bool {
    def.traits.iter().any(|t| t == query)
}

/// `【絆〇〇】相手の能力や効果によって選ばれない。` による選択不可判定
pub fn card_is_untargetable_by_opponent(def: &CardDefinition, ctx: &PlayerCondInfo) -> bool {
    for line in extract_clauses(&def.description, TriggerKind::Passive, def.card_type) {
        let clean = strip_parens(&line);
        if clean.contains("相手の能力や効果によって選ばれない")
            && check_clause_conditions(&line, ctx).is_ok()
        {
            return true;
        }
    }
    false
}

/// インスタンス（一時的なカード名書き換えを含む）のカード名一致判定
pub fn has_instance_name_match(inst: &CardInstance, def: &CardDefinition, query: &str) -> bool {
    if let Some(ref ov) = inst.temp_name_override {
        let q = strip_parens(query);
        return ov == query || strip_parens(ov) == q;
    }
    has_name_match(def, query)
}

/// イベントカード自体の使用可否条件チェック（「〜場合に使用できる」および先頭【条件】タグ）
pub fn check_event_play_conditions(description: &str, ctx: &PlayerCondInfo) -> Result<(), String> {
    let clean_all = strip_parens(description);
    for line in clean_all
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
    {
        for sent in line.split('。').map(str::trim).filter(|s| !s.is_empty()) {
            let s = strip_leading_tags(sent);
            if s.starts_with("このイベントは") && s.contains("場合に使用できる") {
                let cond_body = s
                    .trim_start_matches("このイベントは、")
                    .trim_start_matches("このイベントは")
                    .split("場合に使用できる")
                    .next()
                    .unwrap_or("");
                if !eval_cond_phrase(cond_body, ctx) {
                    return Err(format!("使用条件未達: {}", sent));
                }
            }
        }
    }

    for clause in extract_clauses(description, TriggerKind::OnPlay, CardType::EVENT) {
        for tag in extract_leading_bracket_tags(&clause) {
            check_single_bracket_tag(tag, ctx)?;
        }
    }
    Ok(())
}

/// 手札にあるカードの実効レベル計算（B10033等のレベル－N能力を考慮）
pub fn card_effective_level(def: &CardDefinition, ctx: &PlayerCondInfo) -> i32 {
    let mut lv = def.level;
    let clean = strip_parens(&def.description);
    for line in clean.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if line.contains("手札にあるこの") && line.contains("レベル－") {
            let tags_ok = extract_leading_bracket_tags(line)
                .iter()
                .all(|t| check_single_bracket_tag(t, ctx).is_ok());
            if !tags_ok {
                continue;
            }
            let body = strip_leading_tags(line);
            let cond_ok = if let Some(idx) = body.find("場合、") {
                eval_cond_phrase(&body[..idx], ctx)
            } else {
                true
            };
            if cond_ok {
                if let Some(delta) = extract_number_before(body, "レベル－", "され")
                    .or_else(|| extract_number_before(body, "レベル－", "する"))
                {
                    lv = (lv - delta).max(0);
                }
            }
        }
    }
    lv
}

fn extract_leading_bracket_tags(text: &str) -> Vec<&str> {
    let mut tags = Vec::new();
    let mut rest = text.trim();
    while rest.starts_with('【') {
        if let Some(end) = rest.find('】') {
            tags.push(&rest['【'.len_utf8()..end]);
            rest = rest[end + '】'.len_utf8()..].trim_start();
        } else {
            break;
        }
    }
    tags
}

fn check_single_bracket_tag(tag: &str, ctx: &PlayerCondInfo) -> Result<(), String> {
    if tag == "解決編" && ctx.case_phase != CasePhase::RESOLUTION {
        return Err("事件が解決編になっていません".to_string());
    }
    if tag == "自分ターン中" && !ctx.is_my_turn {
        return Err("自分のターン中ではありません".to_string());
    }
    if tag == "相手ターン中" && ctx.is_my_turn {
        return Err("相手のターン中ではありません".to_string());
    }

    if let Some(col_spec) = tag.strip_prefix("パートナー") {
        let mut matched = false;
        if let Some(pdef) = ctx.partner_def {
            matched = COLORS
                .iter()
                .any(|c| col_spec.contains(c) && pdef.colors.iter().any(|pc| pc == c));
        }
        if !matched {
            return Err(format!("条件未達: 【{}】", tag));
        }
    }

    if let Some(num_str) = tag.strip_prefix("FILE") {
        if let Ok(req) = num_str.parse::<usize>() {
            if ctx.file_count < req {
                return Err(format!("FILEが{}枚未満です（【FILE{}】）", req, req));
            }
        }
    }

    if tag == "事件編" && ctx.case_phase != CasePhase::INTRO {
        return Err("事件編ではないため発動できません".to_string());
    }

    if let Some(case_req) = tag.strip_prefix("事件") {
        if tag != "事件解決" && tag != "事件編" {
            check_case_tag(case_req, ctx.case_def)?;
        }
    }

    if let Some(bond_name) = tag.strip_prefix("絆") {
        let has_bond = check_bond(bond_name, ctx);
        if !has_bond {
            return Err(format!("絆条件未達: 【{}】", tag));
        }
    }
    Ok(())
}

/// 汎用条件文（`〜場合`）の評価
pub fn eval_cond_phrase(cond: &str, ctx: &PlayerCondInfo) -> bool {
    let clean = strip_parens(cond);
    let c = clean.trim();
    if c.is_empty() {
        return true;
    }

    // 複合「Aで、B」条件（「自分の事件が解決編で、自分の現場に...」等）
    if c.contains("自分の事件が解決編で、") {
        if ctx.case_phase != CasePhase::RESOLUTION {
            return false;
        }
        let rest = c
            .split_once("自分の事件が解決編で、")
            .map(|(_, r)| r)
            .unwrap_or("");
        if !rest.is_empty() && !eval_cond_phrase(rest, ctx) {
            return false;
        }
        return true;
    }

    if (c.contains("自分の事件が解決編") || c == "解決編") && ctx.case_phase != CasePhase::RESOLUTION {
        return false;
    }
    if c.contains("この能力や効果") || c.contains("能力や効果によって使用されていた") {
        return ctx.played_by_effect;
    }
    if c.contains("痕跡［発見済み］") {
        return ctx.opp_refreshed;
    }
    if c.contains("痕跡［未発見］") {
        return !ctx.opp_refreshed;
    }
    if c.contains("カードを手札に加えた") || c.contains("キャラを手札に加えた") {
        if !ctx.last_chosen_added_to_hand {
            return false;
        }
        if let Some(max_lv) = extract_number_before(c, "レベル", "以下の") {
            if !ctx.last_chosen_level.map_or(false, |lv| lv <= max_lv) {
                return false;
            }
        }
        if let Some(min_lv) = extract_number_before(c, "レベル", "以上の") {
            if !ctx.last_chosen_level.map_or(false, |lv| lv >= min_lv) {
                return false;
            }
        }
        if c.contains("以外のカードを手札に加えた") || c.contains("以外のキャラを手札に加えた") {
            let excluded_names: Vec<String> = parse_branches_from_phrase(c, CardType::CHARACTER)
                .into_iter()
                .filter_map(|b| b.name_req.or(b.exclude_name))
                .collect();
            if !excluded_names.is_empty() {
                return ctx.last_chosen_name.as_deref().map_or(false, |nm| {
                    !excluded_names.iter().any(|ex| nm.contains(ex.as_str()))
                });
            }
        }
        return true;
    }
    if c.contains("セットした") {
        return ctx.last_chosen_added_to_hand || ctx.last_chosen_name.is_some();
    }
    if let Some(n) = extract_number_before(c, "キャラが", "枚登場した") {
        return ctx.last_deployed_count >= n as usize;
    }
    if c.contains("登場させた") {
        if ctx.last_deployed_count == 0 {
            return false;
        }
        if c.contains('〚') {
            let branches = parse_branches_from_phrase(c, CardType::CHARACTER);
            let target_field = ctx
                .last_chosen_name
                .as_deref()
                .and_then(|ln| {
                    ctx.field_cards
                        .iter()
                        .rev()
                        .find(|(inst, def)| has_instance_name_match(inst, def, ln))
                })
                .or_else(|| ctx.field_cards.last());
            return target_field.map_or(false, |(inst, def)| {
                branches
                    .iter()
                    .any(|b| branch_matches(def, def.ap + inst.temp_ap_bonus, b))
            });
        }
        return true;
    }
    if c.contains("この効果によって【カットイン】を持つ【黒】のカードが") || c.contains("枚以上リムーブされた") {
        if let Some(n) = extract_number_before(c, "カードが", "枚以上リムーブ")
            .or_else(|| c.split("枚以上リムーブ").next().and_then(|s| s.chars().rev().take_while(|ch| ch.is_ascii_digit()).collect::<String>().chars().rev().collect::<String>().parse::<i32>().ok()))
        {
            return ctx.last_removed_black_cutin_count >= n as usize;
        }
    }
    if c.contains("このターン中、自分の現場にキャラが登場していない") {
        return !ctx.self_entered_field_this_turn;
    }
    if let Some(max_f) = extract_number_before(c, "FILEエリアにあるカードが", "枚以下") {
        return ctx.file_count <= max_f as usize;
    }
    if c.contains("と入れ替わった") {
        if let Some(target_nm) = extract_bracketed_value(c, "カード名［", "］") {
            let norm_target = target_nm.replace(' ', "").replace('　', "");
            return ctx.last_disguise_replaced_name.as_deref().map_or(false, |rn| {
                rn.split('|').any(|part| {
                    let norm_part = part.trim().replace(' ', "").replace('　', "");
                    norm_part == norm_target || part.contains(target_nm)
                })
            });
        }
    }
    if c.contains("この効果によって") && c.contains("をスリープさせた") {
        let branches = parse_branches_from_phrase(c, CardType::CHARACTER);
        let req_names: Vec<String> = branches.into_iter().filter_map(|b| b.name_req).collect();
        if req_names.is_empty() {
            return !ctx.last_slept_names.is_empty();
        }
        return req_names.iter().any(|rn| {
            let norm_req = rn.replace(' ', "").replace('　', "");
            ctx.last_slept_names.iter().any(|sn| {
                let norm_sn = sn.replace(' ', "").replace('　', "");
                norm_sn == norm_req || sn.contains(rn.as_str())
            })
        });
    }
    if c.contains("以外の色を持たない") && c.contains("自分の事件") {
        if let Some(&allowed) = COLORS.iter().find(|&&col| c.contains(&format!("【{}】", col))) {
            return ctx.case_def.map_or(false, |cd| {
                !cd.colors.is_empty() && cd.colors.iter().all(|dc| dc == allowed)
            });
        }
    }
    if c.contains("このキャラにカードがセットされている") {
        return ctx
            .actor_id
            .and_then(|aid| ctx.field_cards.iter().find(|(inst, _)| inst.instance_id == aid))
            .map_or(false, |(inst, _)| !inst.set_cards.is_empty());
    }
    if c.contains("裏向きでセットされているカードが合わせて") {
        if let Some(req_n) = extract_number_before(c, "合わせて", "枚以上") {
            let total_fd: usize = ctx
                .field_cards
                .iter()
                .map(|(inst, _)| inst.set_cards.iter().filter(|sc| !sc.face_up).count())
                .sum();
            return total_fd >= req_n as usize;
        }
    }
    if let Some(req_ap) = extract_number_before(c, "このキャラがAP", "以上") {
        if let Some(ap) = ctx.actor_effective_ap {
            return ap >= req_ap;
        }
        return ctx
            .actor_id
            .and_then(|aid| ctx.field_cards.iter().find(|(inst, _)| inst.instance_id == aid))
            .map_or(false, |(inst, def)| {
                inst.base_ap_override.unwrap_or(def.ap) + inst.temp_ap_bonus >= req_ap
            });
    }
    if c.contains("自分の証拠の数が相手の証拠の数以下") {
        return ctx.self_evidence_count <= ctx.opp_evidence_count;
    }
    if let Some(diff) = extract_number_before(c, "相手の証拠が自分の証拠より", "つ以上多い") {
        return ctx.opp_evidence_count >= ctx.self_evidence_count + diff as usize;
    }
    if c.contains("自分の現場にいるキャラが相手の現場にいるキャラより少ない") {
        return ctx.self_field_count < ctx.opp_field_count;
    }
    if c.contains("相手の現場にキャラがいない") {
        return ctx.opp_field_count == 0;
    }
    if c.contains("自分の現場にキャラが1枚もいない") || c.contains("自分の現場にキャラがいない") {
        return ctx.self_field_count == 0;
    }
    if let Some(min_f) = extract_number_before(c, "自分の現場にキャラが", "枚以上いる") {
        return ctx.self_field_count >= min_f as usize;
    }
    if c.contains("自分の現場にスタン状態のキャラがいる") {
        return ctx
            .field_cards
            .iter()
            .any(|(inst, _)| inst.orientation == CardOrientation::STUN);
    }

    // 「自分のリムーブエリアに...がN枚以上ある」
    if c.contains("リムーブエリアに") {
        if let Some(req_n) = extract_number_before(c, "が", "枚以上")
            .or_else(|| extract_number_before(c, "カードが", "枚以上"))
        {
            let scope_end = c.find("が").unwrap_or(c.len());
            let scope = &c[..scope_end];
            let branches = parse_branches_from_phrase(scope, CardType::CHARACTER);
            let cnt = ctx
                .self_remove_cards
                .iter()
                .filter(|(inst, def)| {
                    if scope.contains("カードが") && !scope.contains("〚") && !scope.contains("キャラ") {
                        if let Some(col) = extract_color_filters(scope).first() {
                            return def.colors.iter().any(|dc| dc == col);
                        }
                        return true;
                    }
                    branches
                        .iter()
                        .any(|b| branch_matches(def, def.ap + inst.temp_ap_bonus, b))
                })
                .count();
            return cnt >= req_n as usize;
        }
    }

    // 「自分の現場（かパートナーエリア）にA（とB...）がいる」
    if c.contains("自分の現場") {
        let include_partner = c.contains("パートナーエリア");
        let after = c
            .find("自分の現場かパートナーエリアに")
            .map(|i| &c[i + "自分の現場かパートナーエリアに".len()..])
            .or_else(|| c.find("自分の現場に").map(|i| &c[i + "自分の現場に".len()..]))
            .unwrap_or(c);
        let req_slice = after
            .split("がいる")
            .next()
            .unwrap_or(after);

        let granted_traits: Vec<(&str, &str)> = ctx
            .field_cards
            .iter()
            .flat_map(|(_, fdef)| {
                fdef.description.lines().filter_map(|line| {
                    let t = line.trim();
                    if t.contains("自分の現場にいる") && t.contains("は〚特徴［") && t.contains("を持つ") {
                        if let (Some(nm), Some(tr)) = (
                            extract_bracketed_value(t, "カード名［", "］"),
                            extract_bracketed_value(t, "特徴［", "］"),
                        ) {
                            return Some((nm, tr));
                        }
                    }
                    None
                })
            })
            .collect();

        let matches_field_branches = |inst: &CardInstance, def: &CardDefinition, branches: &[TargetBranch]| -> bool {
            let mut eff_def = def.clone();
            eff_def.level = def.level + inst.temp_level_delta;
            for (nm, tr) in &granted_traits {
                if has_instance_name_match(inst, def, nm) && !eff_def.traits.iter().any(|t| t == tr) {
                    eff_def.traits.push((*tr).to_string());
                }
            }
            branches.iter().any(|b| {
                if let Some(ref nm) = b.name_req {
                    if has_instance_name_match(inst, def, nm) {
                        let mut b_no_name = b.clone();
                        b_no_name.name_req = None;
                        return branch_matches(&eff_def, eff_def.ap + inst.temp_ap_bonus, &b_no_name);
                    }
                    return false;
                }
                branch_matches(&eff_def, eff_def.ap + inst.temp_ap_bonus, b)
            })
        };

        // 「〜のキャラがN枚以上」
        if let Some(req_n) = extract_number_before(after, "が", "枚以上") {
            let distinct_names = req_slice.contains("それぞれカード名の異なる");
            let exclude_self = req_slice.contains("このキャラ以外の");
            let branches = parse_branches_from_phrase(req_slice, CardType::CHARACTER);
            let mut seen_names: Vec<String> = Vec::new();
            let mut cnt = 0usize;
            for (inst, def) in &ctx.field_cards {
                if exclude_self && ctx.actor_id == Some(inst.instance_id.as_str()) {
                    continue;
                }
                if matches_field_branches(inst, def, &branches) {
                    if distinct_names {
                        let cname = inst
                            .temp_name_override
                            .as_deref()
                            .or_else(|| def.names.first().map(|s| s.as_str()))
                            .unwrap_or(&def.definition_id)
                            .to_string();
                        if seen_names.iter().any(|s| s == &cname) {
                            continue;
                        }
                        seen_names.push(cname);
                    }
                    cnt += 1;
                }
            }
            return cnt >= req_n as usize;
        }

        // 「AとB（とCとD）」のAND結合チェック
        if req_slice.contains("〛と〚") {
            let and_parts: Vec<&str> = req_slice.split("と").collect();
            let mut last_was_name = true;
            return and_parts.iter().all(|part| {
                if let Some(nm) = extract_bracketed_value(part, "カード名［", "］") {
                    last_was_name = true;
                    ctx.field_cards
                        .iter()
                        .any(|(inst, def)| has_instance_name_match(inst, def, nm))
                } else if let Some(tr) = extract_bracketed_value(part, "特徴［", "］") {
                    last_was_name = false;
                    ctx.field_cards.iter().any(|(inst, def)| {
                        has_trait_match(def, tr)
                            || granted_traits
                                .iter()
                                .any(|(gnm, gtr)| *gtr == tr && has_instance_name_match(inst, def, gnm))
                    })
                } else if let Some(short) = extract_bracketed_value(part, "〚［", "］") {
                    if last_was_name {
                        ctx.field_cards
                            .iter()
                            .any(|(inst, def)| has_instance_name_match(inst, def, short))
                    } else {
                        ctx.field_cards.iter().any(|(inst, def)| {
                            has_trait_match(def, short)
                                || granted_traits
                                    .iter()
                                    .any(|(gnm, gtr)| *gtr == short && has_instance_name_match(inst, def, gnm))
                        })
                    }
                } else {
                    true
                }
            });
        }

        let need_sleep = req_slice.contains("スリープ状態の");
        let need_active = req_slice.contains("アクティブ状態の");
        let need_stun = req_slice.contains("スタン状態の");
        let exclude_self = req_slice.contains("このキャラ以外の");
        let branches = parse_branches_from_phrase(req_slice, CardType::CHARACTER);

        let mut pool: Vec<(&CardInstance, &CardDefinition)> = ctx.field_cards.clone();
        if include_partner {
            pool.extend(ctx.self_partner_cards.iter().copied());
        }
        return pool.iter().any(|(inst, def)| {
            if exclude_self && ctx.actor_id == Some(inst.instance_id.as_str()) {
                return false;
            }
            if need_sleep && inst.orientation != CardOrientation::SLEEP {
                return false;
            }
            if need_active && inst.orientation != CardOrientation::ACTIVE {
                return false;
            }
            if need_stun && inst.orientation != CardOrientation::STUN {
                return false;
            }
            matches_field_branches(inst, def, &branches)
        });
    }

    true
}

/// 単一能力句の発動条件チェック
pub fn check_clause_conditions(clause: &str, ctx: &PlayerCondInfo) -> Result<(), String> {
    let tags = extract_leading_bracket_tags(clause);
    let clean = strip_parens(clause);

    for tag in &tags {
        check_single_bracket_tag(tag, ctx)?;
    }

    let body = strip_leading_tags(&clean);
    let sentence_count = body
        .split('。')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .count();

    if (sentence_count <= 1
        || clean.contains("場合に宣言できる")
        || clean.contains("場合にのみ宣言できる"))
        && clean.contains("自分の現場に")
    {
        if let Some(req_slice) = extract_field_presence_condition(&clean) {
            let cond_str = format!("自分の現場に{}がいる", req_slice);
            if !eval_cond_phrase(&cond_str, ctx) {
                return Err("自分の現場の条件キャラが不足しています".to_string());
            }
        }
    }

    if sentence_count <= 1 && clean.contains("相手の現場にキャラがいない場合") && ctx.opp_field_count > 0 {
        return Err("相手の現場にキャラがいるため使用できません".to_string());
    }

    if let Some(req_ev) = extract_number_before(&clean, "自分の証拠が", "つ以上") {
        if ctx.self_evidence_count < req_ev as usize {
            return Err(format!("自分の証拠が{}つ未満です", req_ev));
        }
    }

    if clean.contains("自分の証拠が事件レベルの数以上ある場合")
        && (ctx.self_evidence_count as i32) < ctx.case_level
    {
        return Err("証拠が事件レベル未満です".to_string());
    }

    // 単独の「このキャラがスリープ状態かスタン状態の場合、...」（分岐ではない単一文）
    if !clean.contains("アクティブ状態の場合、") {
        if clean.contains("このキャラがスリープ状態かスタン状態の場合")
            && ctx.actor_orientation == CardOrientation::ACTIVE
        {
            return Err("アクティブ状態のため条件を満たしません".to_string());
        }
    } else if clean.contains("このキャラがアクティブ状態の場合、")
        && !clean.contains("スリープ状態かスタン状態の場合")
        && ctx.actor_orientation != CardOrientation::ACTIVE
    {
        return Err("アクティブ状態ではないため条件を満たしません".to_string());
    }

    if let Some(min_src_lv) = extract_number_before(&clean, "レベル", "以上のキャラの能力や") {
        match ctx.played_by_effect_level {
            Some(lv) if lv >= min_src_lv => {}
            _ => return Err("能力や効果によって登場していないため発動しません".to_string()),
        }
    } else if clean.contains("能力や効果によって登場した場合") && ctx.played_by_effect_level.is_none()
    {
        return Err("能力や効果によって登場していないため発動しません".to_string());
    }

    Ok(())
}

fn check_case_tag(case_req: &str, case_def: Option<&CardDefinition>) -> Result<(), String> {
    let Some(cdef) = case_def else {
        return Err("事件カードがありません".to_string());
    };
    let normalized = case_req.replace("or", "＆").replace("か", "＆");
    let parts: Vec<&str> = normalized
        .split('＆')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if !parts.is_empty() && parts.iter().all(|p| COLORS.contains(p)) {
        let ok = if case_req.contains("or") || case_req.contains("か") {
            parts.iter().any(|c| cdef.colors.iter().any(|cc| cc == c))
        } else {
            parts.iter().all(|c| cdef.colors.iter().any(|cc| cc == c))
        };
        if !ok {
            return Err(format!("事件色が【事件{}】を満たしていません", case_req));
        }
        return Ok(());
    }
    let theme_ok = cdef.names.iter().any(|n| n.contains(case_req))
        || cdef.traits.iter().any(|t| t.contains(case_req));
    if !theme_ok {
        return Err(format!("事件が【事件{}】を満たしていません", case_req));
    }
    Ok(())
}

fn check_bond(bond_name: &str, ctx: &PlayerCondInfo) -> bool {
    if bond_name.is_empty() {
        return true;
    }
    if let Some(pdef) = ctx.partner_def {
        if has_name_match(pdef, bond_name) {
            return true;
        }
    }
    ctx.field_cards
        .iter()
        .any(|(inst, def)| ctx.actor_id != Some(inst.instance_id.as_str()) && has_instance_name_match(inst, def, bond_name))
}

fn extract_field_presence_condition(clean: &str) -> Option<&str> {
    let start = clean.rfind("自分の現場に")? + "自分の現場に".len();
    let tail = &clean[start..];
    let end = tail
        .find("がいる場合")
        .or_else(|| tail.find("がいるなら"))
        .or_else(|| tail.find("が登場した場合"))
        ?;
    Some(&tail[..end])
}

/// イベントの選択型効果（「以下から〜選んで行う」）を解析する
pub fn parse_event_mode_choice(description: &str) -> Option<EventModeChoice> {
    let normalized = description.replace("\r\n", "\n").replace('\r', "\n");
    let mut active_lines = Vec::new();
    for line in normalized.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let cl = strip_parens(line);
        let t = cl.trim_start();
        if t.starts_with("【ヒラメキ】")
            || t.starts_with("【カットイン】")
            || t.starts_with("【相手ターン中】")
        {
            break;
        }
        active_lines.push(line.to_string());
    }

    let full = active_lines.join("\n");
    let pos = full.find("以下から")?;

    let mut pre_clauses = Vec::new();
    let mut mode_gate_condition = None;
    let before = full[..pos].trim();
    if !before.is_empty() {
        let stripped = strip_leading_tags(before);
        let parts: Vec<&str> = stripped.split('。').map(str::trim).filter(|s| !s.is_empty()).collect();
        for (i, s) in parts.iter().enumerate() {
            let cs = strip_parens(s);
            if i + 1 == parts.len() && (cs.ends_with("場合、") || cs.ends_with("場合")) {
                mode_gate_condition = Some(cs.trim_end_matches('、').to_string());
            } else {
                pre_clauses.push(format!("{}。", s));
            }
        }
    }

    let after = &full[pos..];
    let header_end = after.find('\n').unwrap_or(after.len());
    let header_raw = &after[..header_end];
    let header_clean = strip_parens(header_raw);

    let mut distinct_colors_limit = false;
    let up_to_n = extract_count_around(&header_clean, "から", "つまで選んで");
    let exact_n = extract_count_around(&header_clean, "から", "つ選んで");
    let (min_pick, mut max_pick) = if header_clean.contains("持つ色の数まで") {
        distinct_colors_limit = true;
        (0, 6)
    } else if up_to_n > 0 {
        (1, up_to_n)
    } else if exact_n > 0 {
        (exact_n, exact_n)
    } else {
        (1, 1)
    };

    let mut all_condition = None;
    let mut optional_all_cost = None;
    if let Some(rep_idx) = header_clean.find("代わりに") {
        let before_rep = header_clean[..rep_idx]
            .split('。')
            .last()
            .unwrap_or("")
            .trim()
            .trim_end_matches('、');
        if let Some(so_idx) = header_raw.find("てもよい。そうした場合") {
            let cost_raw = header_raw[..so_idx]
                .split('。')
                .last()
                .unwrap_or("")
                .trim();
            optional_all_cost = Some(format!("{}てもよい", cost_raw));
        } else if !before_rep.is_empty() {
            all_condition = Some(before_rep.to_string());
        }
    }

    let mut options = Vec::new();
    let mut post_clauses = Vec::new();
    if header_end < after.len() {
        for line in after[header_end..].lines().map(str::trim).filter(|l| !l.is_empty()) {
            if let Some(rest) = line.strip_prefix('・') {
                options.push(rest.trim().to_string());
            } else if !options.is_empty() {
                post_clauses.push(line.to_string());
            }
        }
    }
    if options.is_empty() {
        return None;
    }
    if distinct_colors_limit {
        max_pick = options.len();
    }

    Some(EventModeChoice {
        min_pick,
        max_pick,
        distinct_colors_limit,
        pre_clauses,
        mode_gate_condition,
        options,
        all_condition,
        optional_all_cost,
        post_clauses,
    })
}

/// `【変装】` 句の条件チェック
pub fn check_disguise_conditions(clause: &str, ctx: &PlayerCondInfo) -> Result<(), String> {
    check_clause_conditions(clause, ctx)
}

/// 能力句のコスト（`：` の左側など）を解析する
pub fn parse_clause_cost(clause: &str) -> ClauseCost {
    let mut cost = ClauseCost::default();
    let clean = strip_parens(clause);

    if let Some(colon_idx) = clean.find('：') {
        let prefix = &clean[..colon_idx];
        cost.requires_sleep = prefix.contains("【スリープ】");
        let (hc, hf) = extract_hand_remove_spec(prefix);
        cost.hand_remove = hc;
        cost.hand_filter = hf;
        cost.evidence_remove = extract_count_around(prefix, "証拠を", "つリムーブ");
        cost.evidence_flip_up = extract_count_around(prefix, "裏向きの証拠を", "つ表向き");
        if prefix.contains("裏向きの証拠を1つ以上表向き") {
            cost.evidence_flip_up = cost.evidence_flip_up.max(1);
            cost.evidence_flip_up_any = true;
        }
        cost.file_remove = extract_count_around(prefix, "FILEを", "枚リムーブ")
            .max(extract_count_around(prefix, "FILEエリアのカードを", "枚リムーブ"))
            .max(extract_count_around(prefix, "FILEエリアにあるカードを上から", "枚リムーブ"));
        cost.deck_mill = extract_count_around(prefix, "デッキのカードを上から", "枚リムーブ");
        // 「このキャラをリムーブする」だけでなく「リムーブエリアに移す」も自己犠牲コストとして認識
        cost.sacrifice_self = prefix.contains("このキャラをリムーブする")
            || prefix.contains("このキャラをリムーブエリアに移")
            || prefix.contains("〚リムーブエリアに移す〛")
            || prefix.contains("〚リムーブエリアに移し");
        cost.self_to_deck_bottom = prefix.contains("〚デッキの下に移す〛")
            || prefix.contains("〚デッキの下に移し")
            || prefix.contains("このキャラをデッキの下に移す")
            || prefix.contains("このキャラをデッキの下に移し");
    } else if let Some(opt_idx) = clean.find("てもよい。そうした場合、") {
        let opt_prefix = &clean[..opt_idx];
        let seg = opt_prefix.rsplit('。').next().unwrap_or(opt_prefix);
        cost.evidence_remove = extract_count_around(seg, "証拠を", "つリムーブ");
        cost.evidence_flip_up = extract_count_around(seg, "裏向きの証拠を", "つ表向き");
        cost.file_remove = extract_count_around(seg, "FILEを", "枚リムーブ")
            .max(extract_count_around(seg, "FILEエリアのカードを", "枚リムーブ"));
    }

    cost
}

/// `"手札をN枚リムーブ"` / `"手札から<条件>をN枚リムーブ"` を解析する
pub fn extract_hand_remove_spec(text: &str) -> (usize, Option<TargetFilter>) {
    let simple = extract_count_around(text, "手札を", "枚リムーブ");
    if simple > 0 {
        return (simple, None);
    }
    if let Some(start) = text.find("手札から") {
        let rest = &text[start..];
        if let Some(rem_idx) = rest.find("リムーブ") {
            let scope = &rest[..rem_idx];
            let n = extract_count_around(scope, "を", "枚");
            if n > 0 {
                let mut f = parse_target_filter(scope, TargetZone::Hand, true, None);
                f.owner = OwnerScope::SelfOnly;
                f.zone = TargetZone::Hand;
                f.count = n;
                return (n, Some(f));
            }
        }
    }
    (0, None)
}

/// 能力句の解決プラン（事前処理・対象選択・デッキサーチ・事後効果）を解析する
pub fn parse_effect_steps(
    clause: &str,
    dynamic_level: Option<i32>,
    source_card_type: CardType,
) -> ParsedClausePlan {
    let mut plan = ParsedClausePlan::default();
    let clean = strip_parens(clause);

    // B03006 / B06005 / B06006 / B08003 等: リムーブエリア等からキャラを選び、このキャラの下に重ねる
    if clean.contains("選び、このキャラの下に重ねる") {
        if let Some((tp, _)) = clean.split_once("選び、このキャラの下に重ねる") {
            let zone = if tp.contains("リムーブエリア") {
                TargetZone::Remove
            } else {
                TargetZone::Field
            };
            let mut filter = parse_target_filter(tp, zone, true, dynamic_level);
            filter.owner = OwnerScope::SelfOnly;
            plan.target_step = Some(TargetStep {
                filter,
                action: TargetAction::StackUnderSelf,
                bonuses: vec![],
                follow_up: vec![],
                conditional_follow_up: None,
                optional: clean.contains("まで選び"),
                then_abort_if_skipped: false,
                scale_by_cost_flipped_evidence: false,
            });
            return plan;
        }
    }

    // B11082 ジン＆ベルモット: コストで現場のLv7以下黒をスタンさせ、「【現場リムーブ時】カードを1枚引く。」を付与
    if clean.contains("コストによってスタンさせたキャラに、相手のターン終了時まで「【現場リムーブ時】カードを1枚引く。」を与える") {
        let mut filter = TargetFilter::default();
        filter.owner = OwnerScope::SelfOnly;
        filter.zone = TargetZone::Field;
        filter.count = 1;
        filter.orientations = vec![CardOrientation::ACTIVE, CardOrientation::SLEEP];
        let mut br = TargetBranch::default();
        br.max_level = Some(7);
        br.colors = vec!["黒".to_string()];
        filter.branches = vec![br];
        plan.target_step = Some(TargetStep {
            filter,
            action: TargetAction::StunAndGrantOnRemoveDraw,
            bonuses: vec![],
            follow_up: vec![],
            conditional_follow_up: None,
            optional: false,
            then_abort_if_skipped: false,
            scale_by_cost_flipped_evidence: false,
        });
        return plan;
    }

    // `：` がある場合、コスト側を除いた右辺だけを効果本体とする
    let after_colon_raw = if let Some(idx) = clean.find('：') {
        &clean[idx + '：'.len_utf8()..]
    } else {
        strip_leading_tags(&clean)
    };

    // トリガー文言（「〜したとき、」やタイミング句等）が先頭にある場合、効果対象のフィルタ解析を汚染しないよう除去する
    let after_colon_stage1 = if !clean.contains('：') {
        let first_sent = after_colon_raw.split('。').next().unwrap_or(after_colon_raw);
        if let Some(tok_idx) = first_sent.find("とき、") {
            if !first_sent[..tok_idx].contains('「')
                && !first_sent[..tok_idx].contains("次に相手の証拠がリムーブされた")
            {
                after_colon_raw[tok_idx + "とき、".len()..].trim_start()
            } else {
                after_colon_raw
            }
        } else {
            after_colon_raw
        }
    } else {
        after_colon_raw
    };
    let mut after_colon = after_colon_stage1;
    for timing_prefix in [
        "自分か相手のターン終了時、",
        "自分のターン終了時、",
        "相手のターン終了時、",
        "ターン終了時、",
        "自分のターンのメインフェイズ開始時、",
        "相手のターンのメインフェイズ開始時、",
        "メインフェイズ開始時、",
        "このキャラのアクション終了時、",
    ] {
        if let Some(stripped) = after_colon.strip_prefix(timing_prefix) {
            after_colon = stripped.trim_start();
            break;
        }
    }

    // 1) 「このイベントは〜場合に使用できる。」を除外し、「〜てもよい。そうした場合、〜」を前段と分割しないよう保護する
    let effect_body_owned = strip_playable_restriction_sentences(after_colon);
    let effect_body = effect_body_owned.trim();

    if effect_body.is_empty() {
        return plan;
    }

    // 2) 「〜てもよい。そうした場合、〜」が文頭から始まる場合は任意コスト＋後続として処理する
    if let Some(opt_plan) = try_parse_leading_optional_cost_clause(effect_body, dynamic_level, source_card_type) {
        return opt_plan;
    }

    // もし2文目以降に「〜てもよい。そうした場合、」がある場合は、そこまでの文を先に解析し残りを extra_clauses に積む
    let (primary_chunk, deferred_opt_tail) = split_before_later_optional_cost(effect_body);

    // 「AP7000以上のキャラを1枚までと、AP5000以下のキャラを1枚まで選び、リムーブする」（B04027）等を2文へ正規化
    let expanded_chunk = expand_multi_target_and_sentences(primary_chunk);

    let sentences: Vec<&str> = expanded_chunk
        .split('。')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    let mut seen_target_or_search = false;
    let mut skip_indices: Vec<usize> = Vec::new();
    for i in 0..sentences.len() {
        if skip_indices.contains(&i) {
            continue;
        }
        let sentence = sentences[i];
        if sentence.starts_with("この能力は")
            || sentence.starts_with("この効果は")
            || sentence.starts_with("残りを")
            || sentence.starts_with("その後、公開したカードの残りを")
            || (sentence.starts_with("このイベントは") && sentence.contains("場合に使用できる"))
        {
            continue;
        }

        if sentence.contains("デッキのカードを上から") && sentence.contains("リムーブしてもよい") {
            let cnt = extract_count_around(sentence, "上から", "枚リムーブ").max(1);
            push_op_or_extra(
                &mut plan,
                seen_target_or_search,
                ImmediateOp::OptionalDeckTopRemove {
                    count: cnt,
                    then_clause: String::new(),
                    black_cutin_threshold: None,
                },
            );
            seen_target_or_search = true;
            continue;
        }

        // B09033: 4枚公開＆FILEコスト3回まで追加登場
        if sentence.contains("デッキのカードを上から")
            && sentence.contains("枚公開し")
            && sentences.get(i + 1).is_some_and(|n| n.contains("回まで行ってよい"))
        {
            let look_count = extract_count_around(sentence, "上から", "枚公開").max(4);
            let sub = sentence
                .split("その中から")
                .nth(1)
                .and_then(|x| x.split("枚まで").next())
                .unwrap_or("レベル6以下の〚特徴［高校生］〛のキャラ");
            let mut f = parse_target_filter(sub, TargetZone::Deck, true, dynamic_level);
            f.owner = OwnerScope::SelfOnly;
            let max_repeats = sentences
                .get(i + 1)
                .map(|n| extract_count_around(n, "を", "回まで"))
                .unwrap_or(3)
                .max(1);
            let op = ImmediateOp::B09033RevealDeploy {
                look_count,
                filter: f,
                max_repeats,
            };
            push_op_or_extra(&mut plan, seen_target_or_search, op);
            skip_indices.push(i + 1);
            continue;
        }

        // 「自分のデッキのカードを上から〜が出るまで1枚ずつ公開し...」（B03062, B05042, B08060, D11019, PR195）
        if sentence.contains("デッキのカードを上から") && sentence.contains("が出るまで1枚ずつ公開") {
            if let Some(op) = parse_reveal_deck_until(sentence, &sentences, i, &mut skip_indices, dynamic_level) {
                if seen_target_or_search || plan.deck_search.is_some() {
                    plan.extra_clauses.push(format!("{}。", sentence));
                    for si in &skip_indices {
                        let _ = si;
                    }
                    // skip_indices の後続は既に if_found に吸収済み
                } else {
                    plan.immediate_before.push(op);
                    seen_target_or_search = true;
                }
                continue;
            }
        }

        // 捜査（「カード名を1つ指定し、〚捜査N〛...」 + 発見時効果）（B01095）
        if sentence.contains("捜査") {
            let inv_n = extract_count_around(sentence, "捜査", "〛").max(1);
            let mut then_clause = String::new();
            if let Some(next_s) = sentences.get(i + 1) {
                if next_s.contains("指定したカード名のカードが発見された場合、") {
                    if let Some((_, body)) = next_s.split_once("発見された場合、") {
                        then_clause = format!("{}。", body.trim());
                        skip_indices.push(i + 1);
                    }
                }
            }
            let op = ImmediateOp::InvestigateAndCheckName {
                count: inv_n,
                then_clause,
            };
            push_op_or_extra(&mut plan, seen_target_or_search, op);
            continue;
        }

        // 0) 特殊：自分の証拠から場に出るヒラメキ
        if sentence == "このキャラをスリープ状態で登場させる" {
            plan.immediate_before
                .push(ImmediateOp::MoveSelfFromEvidenceToFieldSleep);
            continue;
        }
        // リムーブエリアから自身を登場させる（ウォッカ等）
        if sentence.contains("このキャラをリムーブエリアからスリープ状態で登場") {
            plan.immediate_before
                .push(ImmediateOp::ReviveSelfFromRemove { sleep: true });
            continue;
        }
        // 事件解決勝利
        if sentence.contains("ゲームに勝利する") {
            plan.immediate_before.push(ImmediateOp::SolveCaseVictory);
            continue;
        }
        // B01058: 「このターン中、次に相手の証拠がリムーブされたとき、...」
        if sentence.starts_with("このターン中、次に相手の証拠がリムーブされたとき、") {
            let body = sentence
                .trim_start_matches("このターン中、次に相手の証拠がリムーブされたとき、")
                .trim();
            let op = ImmediateOp::RegisterNextOppEvidenceRemovedTrigger(format!("{}。", body));
            push_op_or_extra(&mut plan, seen_target_or_search, op);
            continue;
        }

        // B03103 / B03104 の「S_base。〜場合、代わりに〜」を事前結合する
        if let Some(next_s) = sentences.get(i + 1) {
            if let Some((cond, alt_body)) = next_s.split_once("場合、代わりに") {
                if !alt_body.contains("2つとも") && !alt_body.contains("3つとも") {
                    let alt_norm = if alt_body.contains("そのキャラを") {
                        replace_sonochara_with_base(sentence, alt_body)
                    } else {
                        alt_body.trim().to_string()
                    };
                    let cond_op = ImmediateOp::ConditionalClause {
                        condition: cond.trim().to_string(),
                        then_clause: format!("{}。", alt_norm),
                        else_clause: Some(format!("{}。", sentence)),
                    };
                    if seen_target_or_search || !plan.extra_clauses.is_empty() {
                        plan.extra_clauses.push(format!("{}場合、{}。{}でない場合、{}。", cond.trim(), alt_norm, cond.trim(), sentence));
                    } else {
                        plan.immediate_before.push(cond_op);
                        seen_target_or_search = true;
                    }
                    skip_indices.push(i + 1);
                    continue;
                }
            }
        }

        // B11017 五人囃子（3枚まで選ぶ。その中から1枚まで登場させ、1枚までスリープ状態で登場させ、残りを手札に加える）
        // & B04084 バカな作戦（2枚まで選ぶ。その中から1枚を登場させ、残りをスリープ状態で登場させる）
        // & B08043 （相手の現場にいるキャラを1枚まで選ぶ。そのキャラが自分の現場にいるLPがもっとも高いキャラのLP以下のLPの場合、リムーブする）
        if (sentence.ends_with("選ぶ") || sentence.ends_with("選ぶ。"))
            && sentences.get(i + 1).is_some()
        {
            let next_s = sentences[i + 1];
            if let Some(combined_step) = try_combine_pick_and_split(sentence, next_s, dynamic_level, source_card_type) {
                skip_indices.push(i + 1);
                assign_or_queue_target_step(&mut plan, combined_step, format!("{}。{}。", sentence, next_s), &mut seen_target_or_search);
                continue;
            }
        }

        if plan.deck_search.as_ref().is_some_and(|ds| {
            (ds.post_add_discard > 0
                && sentence.starts_with("カードを手札に加えた場合、手札を")
                && sentence.ends_with("枚リムーブする"))
                || ds.post_clauses.iter().any(|pc| pc.trim_end_matches('。') == sentence.trim_end_matches('。'))
        }) {
            continue;
        }

        // 条件付き文（「...場合、〜」 / B02084「セットした場合、...」）
        if !seen_target_or_search || plan.target_step.is_some() || plan.deck_search.is_some() {
            if let Some(cond_op) = try_parse_conditional_sentence(sentence, &sentences, i, &mut skip_indices) {
                // 既存のキャラ追撃（「〜を登場させた場合、...」が直前 target_step に載せられる単発即時処理）は従来通り保持
                if let ImmediateOp::ConditionalClause { ref condition, ref then_clause, ref else_clause } = cond_op {
                    if else_clause.is_none()
                        && seen_target_or_search
                        && plan.target_step.as_ref().is_some_and(|t| t.conditional_follow_up.is_none())
                        && condition.ends_with("を登場させた")
                    {
                        if let Some((cb, c_ops)) = parse_conditional_follow_up(sentence) {
                            plan.target_step.as_mut().unwrap().conditional_follow_up = Some((cb, c_ops));
                            continue;
                        }
                    }
                    let _ = then_clause;
                }
                push_op_or_extra(&mut plan, seen_target_or_search, cond_op);
                continue;
            }
        }

        // B02083: 「相手の現場にいるスタン状態のキャラ1枚につき、カードを1枚引く。（＋次文：カードを1枚も引かなかった場合、...）」
        if sentence.contains("相手の現場にいるスタン状態のキャラ1枚につき、カードを") && sentence.contains("引く") {
            let mut if_zero = None;
            if let Some(next_s) = sentences.get(i + 1) {
                if let Some(rest) = next_s.strip_prefix("カードを1枚も引かなかった場合、") {
                    if_zero = Some(format!("{}。", rest.trim()));
                    skip_indices.push(i + 1);
                }
            }
            push_op_or_extra(&mut plan, seen_target_or_search, ImmediateOp::DrawPerOppStun { if_zero_clause: if_zero });
            continue;
        }

        // 1) デッキサーチ（「自分のデッキのカードを上からN枚見る...」を含む）
        if parse_deck_look_count(expanded_chunk.as_str()).is_some()
            && (sentence.contains("デッキのカードを上から")
                || sentence.contains("その中から")
                || (sentence.starts_with("カードを手札に加えた場合、手札を")
                    && sentence.ends_with("枚リムーブする")))
        {
            if plan.deck_search.is_none() {
                if let Some(search_n) = parse_deck_look_count(sentence)
                    .or_else(|| parse_deck_look_count(expanded_chunk.as_str()))
                {
                    let look_from = sentences[i..].join("。");
                    let search_scope = extract_deck_search_scope(&look_from);
                    plan.deck_search =
                        Some(parse_deck_search_step(search_n, search_scope, dynamic_level));
                    seen_target_or_search = true;
                }
            }
            continue;
        }

        // 2) 条件付き追撃文（例：「〚カード名［江戸川コナン］〛を登場させた場合、カードを1枚引く」）
        if seen_target_or_search {
            if let Some(tstep) = plan.target_step.as_mut() {
                if tstep.conditional_follow_up.is_none() {
                    if let Some((cond_branch, ops)) = parse_conditional_follow_up(sentence) {
                        tstep.conditional_follow_up = Some((cond_branch, ops));
                        continue;
                    }
                }
            }
        }

        // B08029: 「〜を与えるか、〜を与える。」 inline 2択
        if sentence.contains("を与えるか、") && (sentence.contains("与える") || sentence.contains("アクションできる")) {
            if let Some((left, right)) = sentence.split_once("を与えるか、") {
                let opt1 = format!("{}を与える。", left.trim());
                let opt2 = format!("{}。", right.trim().trim_end_matches('。'));
                let mc = EventModeChoice {
                    min_pick: 1,
                    max_pick: 1,
                    distinct_colors_limit: false,
                    pre_clauses: Vec::new(),
                    mode_gate_condition: None,
                    options: vec![opt1, opt2],
                    all_condition: None,
                    optional_all_cost: None,
                    post_clauses: Vec::new(),
                };
                if seen_target_or_search {
                    plan.post_target_ops.push(ImmediateOp::EventModeChoice(mc));
                } else {
                    plan.immediate_before.push(ImmediateOp::EventModeChoice(mc));
                    seen_target_or_search = true;
                }
                continue;
            }
        }

        // 3) 対象選択を伴う文（「〜選び、...」「手札から〜登場させる」「このイベントを〜にセットする」等）
        if is_target_selection_sentence(sentence) {
            let mut target_sent = sentence;
            // 「カードを1枚引き、相手の現場にいるキャラを1枚まで選び...」等の前置即時処理を分離
            for sep in ["引き、", "リムーブし、", "手札に移し、", "AP＋1000、"] {
                if let Some(sidx) = sentence.find(sep) {
                    if sentence[sidx..].contains("選び、") || sentence[sidx..].contains("手札から") {
                        let cut = sidx + sep.len() - "、".len();
                        let prefix = sentence[..cut].trim();
                        let pre_ops = parse_immediate_ops(prefix);
                        if !pre_ops.is_empty() {
                            push_op_or_extra(&mut plan, seen_target_or_search, pre_ops.into_iter().next().unwrap());
                            target_sent = sentence[sidx + sep.len()..].trim();
                            break;
                        }
                    }
                }
            }
            // 「カードを1枚引き、手札から〜登場させる」の前置即時処理を抽出
            if !target_sent.contains("選び") && !target_sent.contains("このイベントを") {
                if let Some(hidx) = target_sent.find("手札から") {
                    let prefix = target_sent[..hidx].trim();
                    if !prefix.is_empty() {
                        let pre_ops = parse_immediate_ops(prefix);
                        if !pre_ops.is_empty() {
                            if plan.deck_search.is_none() && plan.target_step.is_none() && plan.follow_up_target.is_none() {
                                plan.immediate_before.extend(pre_ops);
                            } else if let Some(ds) = plan.deck_search.as_mut() {
                                if plan.target_step.is_none() {
                                    ds.post_ops.extend(pre_ops);
                                } else {
                                    // follow_up の前に挟む必要がある場合はそのままキューに積む
                                    plan.extra_clauses.push(format!("{}。", prefix));
                                }
                            } else {
                                plan.extra_clauses.push(format!("{}。", target_sent));
                                seen_target_or_search = true;
                                continue;
                            }
                        }
                    }
                }
            }

            if let Some(tstep) = parse_target_step(target_sent, dynamic_level, source_card_type) {
                assign_or_queue_target_step(&mut plan, tstep, format!("{}。", target_sent), &mut seen_target_or_search);
                continue;
            }
        }

        // 4) 即時オペレーション（事前または対象解決後）
        let mut ops = parse_immediate_ops(sentence);
        if clause.contains("【ヒラメキ】は発動しない") {
            for op in &mut ops {
                if let ImmediateOp::SelfRemoveEvidence {
                    trigger_hirameki, ..
                } = op
                {
                    *trigger_hirameki = false;
                }
            }
        }
        if !plan.extra_clauses.is_empty() {
            plan.extra_clauses.push(format!("{}。", sentence));
        } else if seen_target_or_search {
            if let Some(ft) = plan.follow_up_target.as_mut() {
                ft.follow_up.extend(ops);
            } else if let Some(tstep) = plan.target_step.as_mut() {
                tstep.follow_up.extend(ops);
            } else if let Some(ds) = plan.deck_search.as_mut() {
                ds.post_ops.extend(ops);
            } else {
                plan.post_target_ops.extend(ops);
            }
        } else {
            plan.immediate_before.extend(ops);
        }
    }

    // fallback: 1文に「〜を見る。その中から〜」がまとまっていたが上で捕捉済み以外の場合
    if plan.deck_search.is_none() && effect_body.contains("デッキのカードを上から") && !effect_body.contains("1枚ずつ公開") {
        if let Some(search_n) = parse_deck_look_count(effect_body) {
            let search_scope = extract_deck_search_scope(effect_body);
            plan.deck_search = Some(parse_deck_search_step(search_n, search_scope, dynamic_level));
        }
    }

    if let Some(tail) = deferred_opt_tail {
        if !tail.is_empty() {
            plan.extra_clauses.push(tail.to_string());
        }
    }

    if !clause.contains("【ヒラメキ】") {
        let conv = |op: &mut ImmediateOp| {
            if matches!(op, ImmediateOp::MoveSelfFromEvidenceToPartner) {
                *op = ImmediateOp::MoveThisToPartner;
            }
        };
        plan.immediate_before.iter_mut().for_each(conv);
        plan.post_target_ops.iter_mut().for_each(conv);
        if let Some(t) = plan.target_step.as_mut() {
            t.follow_up.iter_mut().for_each(conv);
        }
        if let Some(t) = plan.follow_up_target.as_mut() {
            t.follow_up.iter_mut().for_each(conv);
        }
        if let Some(d) = plan.deck_search.as_mut() {
            d.post_ops.iter_mut().for_each(conv);
        }
    }

    plan
}

fn push_op_or_extra(plan: &mut ParsedClausePlan, seen_target_or_search: bool, op: ImmediateOp) {
    if seen_target_or_search {
        if let Some(ft) = plan.follow_up_target.as_mut() {
            ft.follow_up.push(op);
        } else if let Some(tstep) = plan.target_step.as_mut() {
            tstep.follow_up.push(op);
        } else if let Some(ds) = plan.deck_search.as_mut() {
            ds.post_ops.push(op);
        } else {
            plan.post_target_ops.push(op);
        }
    } else {
        plan.immediate_before.push(op);
    }
}

fn assign_or_queue_target_step(
    plan: &mut ParsedClausePlan,
    tstep: TargetStep,
    raw_sentence: String,
    seen_target_or_search: &mut bool,
) {
    if plan.extra_clauses.is_empty() && plan.target_step.is_none() && plan.deck_search.is_none() {
        plan.target_step = Some(tstep);
    } else if plan.extra_clauses.is_empty() && plan.follow_up_target.is_none() && plan.deck_search.is_none() {
        plan.follow_up_target = Some(Box::new(tstep));
    } else {
        // デッキサーチ後の対象選択や3段目以降は実行順を厳密に保つため extra_clauses へ積む
        plan.extra_clauses.push(raw_sentence);
    }
    *seen_target_or_search = true;
}

fn strip_playable_restriction_sentences(text: &str) -> String {
    let mut out = Vec::new();
    for part in text.split('。') {
        let p = part.trim();
        if p.is_empty() {
            continue;
        }
        if p.starts_with("このイベントは") && p.contains("場合に使用できる") {
            continue;
        }
        out.push(p);
    }
    if out.is_empty() {
        String::new()
    } else {
        format!("{}。", out.join("。"))
    }
}

fn split_before_later_optional_cost(text: &str) -> (&str, Option<&str>) {
    if let Some(idx) = text.find("てもよい。そうした場合、") {
        let before = &text[..idx];
        if let Some(dot) = before.rfind('。') {
            let end = dot + '。'.len_utf8();
            return (&text[..end], Some(text[end..].trim()));
        }
    }
    (text, None)
}

fn expand_multi_target_and_sentences(chunk: &str) -> String {
    // B04027: 「AP7000以上のキャラを1枚までと、AP5000以下のキャラを1枚まで選び、リムーブする」
    let mut out = Vec::new();
    for s in chunk.split('。').map(str::trim).filter(|s| !s.is_empty()) {
        if let Some(eidx) = s.find("選び、") {
            let tphrase = &s[..eidx];
            let aphrase = &s[eidx + "選び、".len()..];
            if !tphrase.contains('「') && tphrase.contains("と、") {
                if let Some((left, right)) = tphrase.split_once("と、") {
                    if (left.contains("枚") || left.contains("つ"))
                        && (right.contains("枚") || right.contains("つ"))
                        && !right.contains("同じ色を持たない")
                    {
                        out.push(format!("{}選び、{}", left.trim(), aphrase.trim()));
                        out.push(format!("{}選び、{}", right.trim(), aphrase.trim()));
                        continue;
                    }
                }
            }
        }
        out.push(s.to_string());
    }
    if out.is_empty() {
        String::new()
    } else {
        format!("{}。", out.join("。"))
    }
}

fn replace_sonochara_with_base(base_sentence: &str, alt_body: &str) -> String {
    if let Some((base_target, _)) = base_sentence.split_once("選び、") {
        alt_body.replace("そのキャラ", &format!("{}選び", base_target))
    } else {
        alt_body.to_string()
    }
}

fn parse_optional_cost_target_step(
    seg: &str,
    dynamic_level: Option<i32>,
    source_card_type: CardType,
) -> Option<TargetStep> {
    let normalized_cost = seg
        .replace("スリープさせ", "スリープさせる")
        .replace("スタンさせ", "スタンさせる")
        .replace("デッキの下に移し", "デッキの下に移す")
        .replace("手札に移し", "手札に移す")
        .replace("手札に加え", "手札に加える")
        .replace("リムーブし", "リムーブする")
        .replace("リムーブさ", "リムーブする");
    let normalized_cost = if normalized_cost.ends_with("リムーブ") {
        format!("{}する", normalized_cost)
    } else if normalized_cost.ends_with("デッキの下に移") || normalized_cost.ends_with("手札に移") {
        format!("{}す", normalized_cost)
    } else if normalized_cost.ends_with("手札に加え") {
        format!("{}る", normalized_cost)
    } else {
        normalized_cost
    };
    let cost_src = if normalized_cost.contains("選び") || normalized_cost.contains("手札から") {
        format!("{}。", normalized_cost.trim_end_matches('。'))
    } else {
        let mut inserted = None;
        for verb in ["スリープ", "スタン", "デッキの下に移", "手札に移", "手札に加え", "リムーブ"] {
            if let Some(pos) = normalized_cost.find(verb) {
                inserted = Some(format!(
                    "{}選び、{}。",
                    &normalized_cost[..pos],
                    normalized_cost[pos..].trim_end_matches('。')
                ));
                break;
            }
        }
        inserted.unwrap_or_else(|| format!("{}。", normalized_cost.trim_end_matches('。')))
    };

    let mut cost_step = parse_target_step(&cost_src, dynamic_level, source_card_type)?;
    cost_step.optional = true;
    cost_step.then_abort_if_skipped = true;
    if seg.starts_with("このキャラを") {
        cost_step.filter.self_only = true;
        cost_step.filter.owner = OwnerScope::SelfOnly;
    }
    if cost_step.action == TargetAction::Sleep && cost_step.filter.orientations.is_empty() {
        cost_step.filter.orientations = vec![CardOrientation::ACTIVE];
    } else if cost_step.action == TargetAction::Stun && cost_step.filter.orientations.is_empty() {
        cost_step.filter.orientations = vec![CardOrientation::ACTIVE, CardOrientation::SLEEP];
    }
    Some(cost_step)
}

fn try_parse_leading_optional_cost_clause(
    effect_body: &str,
    dynamic_level: Option<i32>,
    source_card_type: CardType,
) -> Option<ParsedClausePlan> {
    let idx = effect_body.find("てもよい。そうした場合、")?;
    let opt_prefix = &effect_body[..idx];
    // もし先頭文より後ろの文に付いている場合（dotあり）や前提条件（〜場合、）がある場合は別ルートに任せる
    if opt_prefix.contains('。') || opt_prefix.contains("場合、") {
        return None;
    }
    let seg = opt_prefix.trim();
    if seg.contains("相手") {
        return None;
    }

    let then_rest = effect_body[idx + "てもよい。そうした場合、".len()..].trim();
    let mut plan = ParsedClausePlan::default();

    // 1) B09019 / B09105 等の複合コスト（パートナーをスリープ / 手札リムーブ / FILEリムーブ / 特定キャラリムーブ）
    if seg.contains("パートナーをスリープ")
        || (seg.contains("FILEエリア") && seg.contains("リムーブ") && seg.contains("、"))
    {
        let sleep_partner = seg.contains("パートナーをスリープ");
        let (hand_remove, _) = extract_hand_remove_spec(seg);
        let file_top_remove = extract_count_around(seg, "FILEエリアにあるカードを上から", "枚リムーブ")
            .max(extract_count_around(seg, "FILEエリアのカードを上から", "枚リムーブ"));
        let field_remove_name = if seg.contains("自分の現場にいる") && seg.contains("リムーブ") {
            extract_bracketed_value(seg, "カード名［", "］").map(|s| s.to_string())
        } else {
            None
        };
        plan.immediate_before.push(ImmediateOp::OptionalCompoundCost {
            sleep_partner,
            hand_remove,
            file_top_remove,
            field_remove_name,
            then_clause: then_rest.to_string(),
        });
        return Some(plan);
    }

    // 1.5) 自己スリープ + 手札リムーブ/公開コスト（B04049 / B08064 等）
    if seg.starts_with("このキャラをスリープさせ、") {
        let after_self = seg["このキャラをスリープさせ、".len()..].trim();
        let reveal_only = after_self.contains("公開");
        let norm_hand = after_self.replace("公開", "リムーブ");
        let (hc, hf) = extract_hand_remove_spec(&norm_hand);
        if hc > 0 {
            plan.immediate_before.push(ImmediateOp::OptionalSelfOrientAndDiscardHand {
                target_orientation: CardOrientation::SLEEP,
                count: hc,
                filter: hf,
                reveal_only,
            });
            let rest_plan = parse_effect_steps(then_rest, dynamic_level, source_card_type);
            plan.immediate_before.extend(rest_plan.immediate_before);
            plan.target_step = rest_plan.target_step;
            plan.follow_up_target = rest_plan.follow_up_target;
            plan.deck_search = rest_plan.deck_search;
            plan.post_target_ops = rest_plan.post_target_ops;
            plan.extra_clauses = rest_plan.extra_clauses;
            return Some(plan);
        }
    }

    // 1.6) 手札リムーブ + 現場キャラ操作の複合任意コスト（B07094, PR307, B07036, B09022）
    if let Some((p0_raw, p1_raw)) = seg.split_once('、') {
        let p0 = p0_raw.trim();
        let p1 = p1_raw.trim();
        let (hc0, hf0) = extract_hand_remove_spec(p0);
        let (hc1, _) = extract_hand_remove_spec(p1);
        if hc0 > 0 && !p0.contains("現場") {
            if let Some(cost_step) = parse_optional_cost_target_step(p1, dynamic_level, source_card_type) {
                plan.immediate_before.push(ImmediateOp::OptionalDiscardHand {
                    count: hc0,
                    filter: hf0,
                });
                plan.target_step = Some(cost_step);
                if !then_rest.is_empty() {
                    plan.extra_clauses.push(then_rest.to_string());
                }
                return Some(plan);
            }
        }
        if hc1 > 0 && !p1.contains("現場") {
            if let Some(cost_step) = parse_optional_cost_target_step(p0, dynamic_level, source_card_type) {
                plan.target_step = Some(cost_step);
                if !then_rest.is_empty() {
                    plan.extra_clauses.push(format!("{}てもよい。そうした場合、{}", p1, then_rest));
                }
                return Some(plan);
            }
        }
    }

    // 2) デッキ上からN枚リムーブしてもよい（B05061, B10096, PR178）
    if seg.contains("デッキのカードを上から") && seg.contains("リムーブ") {
        let count = extract_count_around(seg, "上から", "枚リムーブ").max(1);
        plan.immediate_before.push(ImmediateOp::OptionalDeckTopRemove {
            count,
            then_clause: then_rest.to_string(),
            black_cutin_threshold: None,
        });
        return Some(plan);
    }

    // 3) FILEエリア上からN枚手札に加えてもよい（B06082）
    if seg.contains("FILEエリアにあるカードを上から") && seg.contains("手札に加え") {
        let count = extract_count_around(seg, "上から", "枚手札").max(1);
        plan.immediate_before.push(ImmediateOp::OptionalFileTopToHand {
            count,
            then_clause: then_rest.to_string(),
        });
        return Some(plan);
    }

    // 4) 手札リムーブコスト（B04084 2枚、その他1枚等）
    let (hc, hf) = extract_hand_remove_spec(seg);
    if hc > 0 {
        plan.immediate_before.push(ImmediateOp::OptionalDiscardHand {
            count: hc,
            filter: hf,
        });
        let rest_plan = parse_effect_steps(then_rest, dynamic_level, source_card_type);
        plan.immediate_before.extend(rest_plan.immediate_before);
        plan.target_step = rest_plan.target_step;
        plan.follow_up_target = rest_plan.follow_up_target;
        plan.deck_search = rest_plan.deck_search;
        plan.post_target_ops = rest_plan.post_target_ops;
        plan.extra_clauses = rest_plan.extra_clauses;
        return Some(plan);
    }

    // 5) ターゲット選択を伴う任意コスト（「〜キャラを1枚スリープさせてもよい」「〜セットされているカードを合わせて2枚リムーブしてもよい」等）
    if let Some(cost_step) = parse_optional_cost_target_step(seg, dynamic_level, source_card_type) {
        plan.target_step = Some(cost_step);
        if !then_rest.is_empty() {
            plan.extra_clauses.push(then_rest.to_string());
        }
        return Some(plan);
    }

    None
}

fn parse_reveal_deck_until(
    sentence: &str,
    sentences: &[&str],
    idx: usize,
    skip_indices: &mut Vec<usize>,
    dynamic_level: Option<i32>,
) -> Option<ImmediateOp> {
    let start = sentence.find("デッキのカードを上から")? + "デッキのカードを上から".len();
    let rest = &sentence[start..];
    let end = rest.find("が出るまで")?;
    let criteria = rest[..end].trim();
    let add_to_hand = sentence.contains("手札に加える");

    let mut f = parse_target_filter(criteria, TargetZone::Deck, true, dynamic_level);
    f.owner = OwnerScope::SelfOnly;
    f.zone = TargetZone::Deck;
    if criteria == "イベント" {
        f.card_type_filter = Some(CardType::EVENT);
        for b in &mut f.branches {
            b.card_types = vec![CardType::EVENT];
        }
    } else if criteria.contains("カード") && !criteria.contains("カード名") {
        f.any_card_type = true;
    }

    let mut if_found_clause = None;
    for j in (idx + 1)..sentences.len() {
        let s = sentences[j];
        if s.starts_with("残りの公開したカードを") {
            skip_indices.push(j);
            continue;
        }
        if s.starts_with("カードを手札に加えた場合、") {
            if_found_clause = Some(format!("{}。", s.trim_start_matches("カードを手札に加えた場合、").trim()));
            skip_indices.push(j);
            break;
        }
        break;
    }

    Some(ImmediateOp::RevealDeckUntil {
        filter: f,
        add_to_hand,
        if_found_clause,
    })
}

fn try_combine_pick_and_split(
    pick_sent: &str,
    next_s: &str,
    dynamic_level: Option<i32>,
    _source_card_type: CardType,
) -> Option<TargetStep> {
    let (target_phrase, action) = if next_s.contains("1枚まで登場させ、1枚までスリープ状態で登場させ、残りを手札に加える") {
        let tp = pick_sent.trim_end_matches('。').trim_end_matches("選ぶ").trim();
        (tp, TargetAction::ReviveSplitGonin)
    } else if next_s.contains("その中から1枚を登場させ、残りをスリープ状態で登場させる") {
        let tp = pick_sent.trim_end_matches('。').trim_end_matches("選ぶ").trim();
        (tp, TargetAction::ReviveSplitBakana)
    } else if next_s.contains("もっとも高いキャラのLP以下のLPの場合、リムーブする") {
        let tp = pick_sent.trim_end_matches('。').trim_end_matches("選ぶ").trim();
        let mut f = parse_target_filter(tp, TargetZone::Field, false, dynamic_level);
        f.max_lp_from_self_max = true;
        f.up_to = pick_sent.contains("まで");
        return Some(TargetStep {
            action: TargetAction::Remove,
            filter: f,
            bonuses: Vec::new(),
            follow_up: Vec::new(),
            conditional_follow_up: None,
            optional: false,
            then_abort_if_skipped: false,
            scale_by_cost_flipped_evidence: false,
        });
    } else {
        return None;
    };

    let zone = if target_phrase.contains("リムーブエリア") {
        TargetZone::Remove
    } else {
        TargetZone::Field
    };
    let mut filter = parse_target_filter(target_phrase, zone, true, dynamic_level);
    filter.up_to = true;
    Some(TargetStep {
        action,
        filter,
        bonuses: Vec::new(),
        follow_up: Vec::new(),
        conditional_follow_up: None,
        optional: false,
        then_abort_if_skipped: false,
        scale_by_cost_flipped_evidence: false,
    })
}

fn try_parse_conditional_sentence(
    sentence: &str,
    sentences: &[&str],
    idx: usize,
    skip_indices: &mut Vec<usize>,
) -> Option<ImmediateOp> {
    if sentence.contains("アクティブ状態の場合、")
        || sentence.contains("スリープ状態かスタン状態の場合、")
        || sentence.contains("その【ヒラメキ】の効果を発動")
    {
        return None;
    }
    let (cond_raw, then_raw) = sentence.split_once("場合、")?;
    let cond = cond_raw.trim();
    if cond.contains("カードのレベル以下") {
        // B04063 のような修飾句ではなく条件文のみ捕捉
        return None;
    }
    if !(cond.contains("自分の")
        || cond.contains("相手の")
        || cond.contains("この")
        || cond.contains("痕跡")
        || cond.contains("セットした")
        || cond.contains("カードを手札に加えた")
        || cond.contains("登場させた")
        || cond.contains("解決編"))
    {
        return None;
    }

    let mut then_clause = format!("{}。", then_raw.trim());
    // B05103 等の「...場合、手札を1枚リムーブしてもよい。そうした場合、〜」を丸ごと条件内に格納
    if then_raw.ends_with("てもよい") {
        if let Some(next_s) = sentences.get(idx + 1) {
            if next_s.starts_with("そうした場合、") {
                then_clause = format!("{}{}。", then_clause, next_s.trim());
                skip_indices.push(idx + 1);
            }
        }
    }

    let mut else_clause = None;
    // B09106: 直後の「〚痕跡［未発見］〛の場合、...」を else として結合（個別でも動くが連結が明確）
    if cond.contains("痕跡［発見済み］") {
        if let Some(next_s) = sentences.get(idx + 1) {
            if let Some((ec, et)) = next_s.split_once("場合、") {
                if ec.contains("痕跡［未発見］") {
                    else_clause = Some(format!("{}。", et.trim()));
                    skip_indices.push(idx + 1);
                }
            }
        }
    }

    Some(ImmediateOp::ConditionalClause {
        condition: cond.to_string(),
        then_clause,
        else_clause,
    })
}

fn extract_deck_search_scope(effect_body: &str) -> &str {
    let rest_from_look = effect_body
        .find("デッキのカードを上から")
        .map(|i| &effect_body[i..])
        .unwrap_or(effect_body);

    // 「残りを〜移す。」の文末までを切り出し、後続の「手札から〜登場させる。」が混入しないようにする
    let mut end_idx = rest_from_look.len();
    if let Some(rem_idx) = rest_from_look.find("残りを") {
        let after_rem = &rest_from_look[rem_idx..];
        if let Some(dot_rel) = after_rem.find('。') {
            let after_dot_idx = rem_idx + dot_rel + '。'.len_utf8();
            end_idx = after_dot_idx;
            // 直後の文が「〜手札に加えた場合、」「〜登場させた場合、」ならそれも範囲に含める
            let tail = rest_from_look[after_dot_idx..].trim_start();
            if tail.starts_with("カードを手札に加えた場合、手札を")
                || tail.contains("手札に加えた場合、")
                || tail.contains("登場させた場合、")
            {
                if let Some(dot2) = tail.find('。') {
                    let offset = rest_from_look.len() - tail.len();
                    end_idx = offset + dot2 + '。'.len_utf8();
                } else {
                    end_idx = rest_from_look.len();
                }
            }
        }
    } else if let Some(hidx) = rest_from_look.find("手札から") {
        end_idx = hidx;
    }

    &rest_from_look[..end_idx]
}

fn is_target_selection_sentence(s: &str) -> bool {
    if s.contains("証拠を")
        && (s.contains("表向き") || s.contains("裏向き"))
        && !s.contains("リムーブ")
        && !s.contains("手札に加え")
        && !s.contains("移す")
    {
        return false;
    }
    if s.contains("選び") || s.ends_with("選ぶ") {
        return true;
    }
    if s.contains("手札から")
        && (s.contains("登場させ") || s.contains("使用する") || s.contains("証拠として得る"))
    {
        return true;
    }
    if s.contains("このイベントを") && s.contains("にセットする") {
        return true;
    }
    if (s.contains("そのキャラ") || s.contains("登場させたキャラ"))
        && (s.contains("AP＋")
            || s.contains("突撃")
            || s.contains("ブレット")
            || s.contains("セットする")
            || s.contains("書き換え")
            || s.contains("アクティブ状態のキャラを指定してアクションできる")
            || s.contains("レベル－"))
    {
        return true;
    }
    if s.contains("を") && s.contains("にセットする") && !s.contains("すべての") {
        return true;
    }
    false
}

fn parse_deck_look_count(text: &str) -> Option<usize> {
    let c1 = extract_count_around(text, "上から", "枚見");
    if c1 > 0 {
        return Some(c1);
    }
    let c2 = extract_count_around(text, "上から", "枚公開");
    if c2 > 0 {
        return Some(c2);
    }
    None
}

fn parse_deck_search_step(
    search_count: usize,
    text: &str,
    dynamic_level: Option<i32>,
) -> DeckSearchStep {
    let pick_action = if text.contains("スリープ状態で登場") {
        DeckPickAction::PlaySleep
    } else if text.contains("登場させ") {
        DeckPickAction::PlayActive
    } else {
        DeckPickAction::ToHand
    };

    let target_slice = if let Some(idx) = text.find("その中から") {
        let sub = &text[idx + "その中から".len()..];
        if let Some(end) = sub.find("枚まで") {
            &sub[..end]
        } else if let Some(end) = sub.find("公開して") {
            &sub[..end]
        } else {
            sub
        }
    } else {
        text
    };

    let mut filter = parse_target_filter(target_slice, TargetZone::Deck, true, dynamic_level);
    filter.owner = OwnerScope::SelfOnly;

    let mut count = extract_count_around(text, "を", "枚まで公開");
    if count == 0 {
        count = extract_count_around(text, "を", "枚まで手札");
    }
    if count == 0 {
        count = extract_count_around(text, "を", "枚まで登場");
    }
    if count == 0 {
        count = extract_count_around(text, "を", "枚手札");
    }
    // B03042: 「〚特徴［探偵］〛のキャラを1枚までと、そのキャラと同じ色を持たない〚特徴［探偵］〛のキャラを1枚まで公開して手札に加え」
    if text.contains("と、") && text.contains("同じ色を持たない") {
        count = count.max(2);
        filter.distinct_colors = true;
    }
    filter.count = count.max(1);
    filter.up_to = true;

    let mut post_add_discard = 0;
    let mut post_clauses = Vec::new();
    for s in text.split('。').map(str::trim).filter(|s| !s.is_empty()) {
        if s.starts_with("カードを手札に加えた場合、手札を") && s.ends_with("枚リムーブする") {
            post_add_discard = extract_count_around(s, "手札を", "枚リムーブ");
        } else if (s.contains("手札に加えた場合、") || s.contains("登場させた場合、"))
            && !s.contains("デッキのカードを上から")
        {
            post_clauses.push(format!("{}。", s));
        }
    }
    let remainder_to_remove = text.contains("残りをリムーブ");
    let shuffle_remainder = text.contains("シャッフル");
    let record_removed_levels = remainder_to_remove;

    DeckSearchStep {
        search_count,
        pick_action,
        filter,
        post_add_discard,
        post_ops: Vec::new(),
        remainder_to_remove,
        pick_count: count.max(1),
        shuffle_remainder,
        record_removed_levels,
        post_clauses,
    }
}

fn parse_conditional_follow_up(sentence: &str) -> Option<(TargetBranch, Vec<ImmediateOp>)> {
    let idx = sentence.find("登場させた場合、")?;
    let cond_part = &sentence[..idx];
    let ops_part = &sentence[idx + "登場させた場合、".len()..];
    let branches = parse_branches_from_phrase(cond_part, CardType::CHARACTER);
    let cond_branch = branches.into_iter().next()?;
    let ops = parse_immediate_ops(ops_part);
    if ops.is_empty() {
        return None;
    }
    Some((cond_branch, ops))
}

fn parse_target_step(
    sentence: &str,
    dynamic_level: Option<i32>,
    _source_card_type: CardType,
) -> Option<TargetStep> {
    // 特殊0: 「このイベントを〜1枚にセットする」「このイベントをそのキャラにセットする」
    if sentence.contains("このイベントを") && sentence.contains("にセットする") {
        let after = sentence.split_once("このイベントを").map(|x| x.1)?;
        let target_phrase = after.split("にセットする").next()?.trim();
        let mut filter = if target_phrase == "そのキャラ" {
            TargetFilter {
                target_prev_chosen: true,
                owner: OwnerScope::SelfOnly,
                ..Default::default()
            }
        } else {
            let mut f = parse_target_filter(target_phrase, TargetZone::Field, true, dynamic_level);
            f.owner = OwnerScope::SelfOnly;
            f
        };
        filter.count = 1;
        return Some(TargetStep {
            action: TargetAction::SetThisEvent,
            filter,
            bonuses: Vec::new(),
            follow_up: Vec::new(),
            conditional_follow_up: None,
            optional: false,
            then_abort_if_skipped: false,
            scale_by_cost_flipped_evidence: false,
        });
    }

    // 特殊1: 「ターン終了時までそのキャラのカード名を、自分の現場にいるそのキャラ以外のレベル8以下のキャラ1枚のカード名に書き換えてもよい」（B09052）
    if sentence.contains("そのキャラのカード名を、") && sentence.contains("カード名に書き換えてもよい") {
        let scope = sentence
            .split_once("カード名を、")
            .map(|x| x.1)
            .unwrap_or(sentence);
        let target_phrase = scope.split("のカード名に書き換").next().unwrap_or(scope);
        let mut filter = parse_target_filter(target_phrase, TargetZone::Field, true, dynamic_level);
        filter.owner = OwnerScope::SelfOnly;
        filter.exclude_prev_chosen = true;
        filter.up_to = true;
        return Some(TargetStep {
            action: TargetAction::CopyNameFromOtherSelfChar,
            filter,
            bonuses: Vec::new(),
            follow_up: Vec::new(),
            conditional_follow_up: None,
            optional: true,
            then_abort_if_skipped: false,
            scale_by_cost_flipped_evidence: false,
        });
    }

    // 特殊2: 「〜そのキャラをAP＋3000し、〚突撃...〛を与え、自分のデッキのカードを上から1枚裏向きでそのキャラにセットする」（B07058 / B03133 / D09025 / D11019）
    if !sentence.contains("選び")
        && !sentence.contains("手札から")
        && (sentence.contains("そのキャラに") || sentence.contains("そのキャラを"))
    {
        let bonuses = extract_bonuses_from_action(sentence);
        if let Some(primary) = parse_target_action(sentence) {
            let mut f = TargetFilter {
                target_prev_chosen: true,
                owner: OwnerScope::SelfOnly,
                ..Default::default()
            };
            let _ = &mut f;
            return Some(TargetStep {
                action: primary,
                filter: f,
                bonuses,
                follow_up: Vec::new(),
                conditional_follow_up: None,
                optional: sentence.contains("てもよい"),
                then_abort_if_skipped: false,
                scale_by_cost_flipped_evidence: false,
            });
        }
    }

    // 選択対象フレーズとアクションフレーズに分割
    let (target_phrase, action_phrase) = if let Some(idx) = sentence.find("選び、") {
        (&sentence[..idx], &sentence[idx + "選び、".len()..])
    } else if let Some(idx) = sentence.find("選び") {
        (&sentence[..idx], &sentence[idx + "選び".len()..])
    } else if let Some(idx) = sentence.find("手札から") {
        let rest = &sentence[idx..];
        if let Some(didx) = rest.find("登場させ") {
            (&rest[..didx], &rest[didx..])
        } else if let Some(didx) = rest.find("使用") {
            (&rest[..didx], &rest[didx..])
        } else if let Some(didx) = rest.find("裏向きで証拠として得る") {
            (&rest[..didx], &rest[didx..])
        } else {
            return None;
        }
    } else {
        return None;
    };

    // action_phrase を先頭のアクション節と後続効果（「〜し、カードを1枚引く」等）に分離
    let (primary_action_text, trailing_text) = split_action_and_follow_up(action_phrase);
    let mut bonuses = extract_bonuses_from_action(primary_action_text);
    let mut action = parse_target_action(primary_action_text)?;

    // 「自分の現場にいる〚特徴［警察］〛のキャラを1枚まで選び、自分のデッキのカードを上から1枚裏向きでセットする」（B10033）
    let zone = if target_phrase.contains("セットされているカード") {
        TargetZone::SetCards
    } else if target_phrase.contains("証拠") {
        TargetZone::Evidence
    } else if target_phrase.contains("リムーブエリア") {
        TargetZone::Remove
    } else if target_phrase.contains("手札から") || target_phrase.contains("手札にある") {
        TargetZone::Hand
    } else {
        TargetZone::Field
    };

    if zone == TargetZone::Hand {
        if matches!(action, TargetAction::ReviveActive | TargetAction::ReviveSleep) {
            action = TargetAction::PlayFromHand;
            // B08092 / PR291 / B03024 等で「スリープ状態で登場」を保持するため bonus に記載するか action 維持
            // 現状 PlayFromHand で `ReviveSleep` 文言は parse_target_step の follow_up 等で区分、または target_phrase/action_phrase 判定
        }
    }
    if zone == TargetZone::Evidence && matches!(action, TargetAction::RecoverToHand) {
        action = TargetAction::MoveEvidenceToHand;
    }
    if zone == TargetZone::SetCards {
        let cnt = extract_count_around(target_phrase, "合わせて", "枚")
            .max(extract_count_around(target_phrase, "を", "枚"))
            .max(1);
        action = TargetAction::RemoveSetCards(cnt);
    }

    let mut filter = parse_target_filter(
        target_phrase,
        zone,
        action.is_beneficial(),
        dynamic_level,
    );
    if sentence.contains("アクション中のキャラ") {
        filter.acting_only = true;
    }
    if action_phrase.contains("スリープ状態で登場") && zone == TargetZone::Hand {
        // engine.rs が action_phrase の スリープ状態 を判別できるようにする、または ReviveSleep を Hand で使う
        // Wait: 既存の `handle_target_select_answer` は `PLAY_FROM_HAND` が Active 固定な場合があるので、`PLAY_FROM_HAND` 用 sleep か判定する
        // 既存互換のため `ReviveSleep` と同等に判定する follow_up? いいえ、`TargetStep.filter` の `orientations` には使えないので、
        // `TargetBonus` ではなく既存の `ReviveSleep` vs `PlayFromHand` を整理！
        // 実際 `PlayFromHand` 時にスリープ状態で登場は `PR291` `B03024` `B08092` のみ！
        // よってここでは `PLAY_FROM_HAND` を `ReviveSleep` とは区別しつつ、もし action_phrase に「スリープ状態で登場」があれば
        // `TargetBonus` は使わず `TargetAction::ReviveSleep` (zone==Hand の時は手札からスリープ登場) として扱うのが超シンプル！
        action = TargetAction::ReviveSleep;
    }
    // すでに primary action が与えた効果自身は bonuses から重複除外する
    bonuses.retain(|b| match (&action, b) {
        (TargetAction::ApBuff(v), TargetBonus::ApBuff(bv)) => v != bv,
        (TargetAction::GiveRush(r), TargetBonus::GiveRush(br)) => r != br,
        (TargetAction::GiveBullet, TargetBonus::GiveBullet) => false,
        (TargetAction::GiveCanAttackActive, TargetBonus::GiveCanAttackActive) => false,
        (TargetAction::SetDeckTopFaceDown(n), TargetBonus::SetDeckTopFaceDown(bn)) => n != bn,
        (TargetAction::GrantContactForceAttack, TargetBonus::GrantContactForceAttackThisTurn) => false,
        (TargetAction::GrantBulletOnCaseAttackDiscard, TargetBonus::BulletOnCaseAttackDiscardThisTurn) => false,
        _ => true,
    });

    let follow_up = if let Some(t) = trailing_text {
        parse_immediate_ops(t)
    } else {
        Vec::new()
    };

    if matches!(action, TargetAction::ApBuff(_))
        && !sentence.contains("相手の")
        && filter.owner == OwnerScope::OppOnly
    {
        filter.owner = OwnerScope::SelfOnly;
    }

    Some(TargetStep {
        action,
        filter,
        bonuses,
        follow_up,
        conditional_follow_up: None,
        optional: sentence.contains("てもよい"),
        then_abort_if_skipped: false,
        scale_by_cost_flipped_evidence: sentence.contains("コストで表向きにした証拠1つにつき"),
    })
}

fn split_action_and_follow_up(action_phrase: &str) -> (&str, Option<&str>) {
    // 複合ターゲットボーナス（「〜AP＋2000し、〚突撃［キャラ］〛を与える」や「〜〚突撃［事件］〛と「...引く。」を与える」「〜AP＋3000し、〜セットする」）は分割せず primary 内に残す
    for sep in [
        "リムーブし、",
        "スリープさせ、",
        "スタンさせ、",
        "アクティブにし、",
        "デッキの下に移し、",
        "登場させ、",
        "手札に加え、",
        "手札に移し、",
        "裏向きにし、",
        "表向きにし、",
    ] {
        if let Some(idx) = action_phrase.find(sep) {
            let cut = idx + sep.len() - "、".len();
            return (&action_phrase[..cut], Some(&action_phrase[idx + sep.len()..]));
        }
    }
    if let Some(idx) = action_phrase.find("する。") {
        let cut = idx + "する".len();
        return (&action_phrase[..cut], Some(&action_phrase[idx + "する。".len()..]));
    }
    // 「〜レベル－1する。カードを1枚引き...」（別文・または「し、カードを」系）
    if let Some(idx) = action_phrase.find("レベル－") {
        if let Some(suru) = action_phrase[idx..].find("する。") {
            let cut = idx + suru + "する".len();
            return (&action_phrase[..cut], Some(&action_phrase[cut + "。".len()..]));
        }
    }
    if let Some(idx) = action_phrase.find("与え、カードを") {
        let cut = idx + "与え".len();
        return (&action_phrase[..cut], Some(&action_phrase[idx + "与え、".len()..]));
    }
    (action_phrase, None)
}

fn extract_bonuses_from_action(text: &str) -> Vec<TargetBonus> {
    let mut out = Vec::new();
    if let Some(ap) = parse_signed_ap(text, true) {
        if !text.contains("コンタクト中、このキャラをAP＋") && !text.contains("コンタクト中、そのキャラをAP＋") {
            out.push(TargetBonus::ApBuff(ap));
        }
    }
    if text.contains("〚突撃［レベル4以下のキャラ］〛") {
        out.push(TargetBonus::GiveRush(Some("CHAR_LV4".to_string())));
    } else if text.contains("〚突撃［事件］〛") {
        out.push(TargetBonus::GiveRush(Some("CASE".to_string())));
    } else if text.contains("〚突撃［キャラ］〛") {
        out.push(TargetBonus::GiveRush(Some("CHAR".to_string())));
    } else if text.contains("〚迅速〛") {
        out.push(TargetBonus::GiveRush(Some("SWIFT".to_string())));
    } else if text.contains("〚突撃〛") {
        out.push(TargetBonus::GiveRush(None));
    }
    if text.contains("〚ブレット〛") && !text.contains("そうした場合") {
        out.push(TargetBonus::GiveBullet);
    }
    if text.contains("アクティブ状態のキャラを指定してアクションできる") {
        out.push(TargetBonus::GiveCanAttackActive);
    }
    if text.contains("このキャラがアクションしたとき、カードを1枚引く") {
        out.push(TargetBonus::DrawOnActionThisTurn);
    }
    if text.contains("このキャラがコンタクトしたとき、カードを1枚引き、そのコンタクト中、そのキャラをAP＋1000する") {
        out.push(TargetBonus::ContactDrawAndAp1000ThisTurn);
    }
    if text.contains("このキャラがアクション［事件］したとき、手札を1枚リムーブしてもよい。そうした場合、ターン終了時までこのキャラは〚ブレット〛") {
        out.push(TargetBonus::BulletOnCaseAttackDiscardThisTurn);
    }
    if text.contains("相手の現場にいるキャラを1枚まで選び、このキャラとのコンタクトを発生させる") {
        out.push(TargetBonus::GrantContactForceAttackThisTurn);
    }
    if text.contains("デッキのカードを上から") && text.contains("枚裏向きで") && text.contains("セットする") {
        let n = extract_count_around(text, "上から", "枚裏向き").max(1);
        out.push(TargetBonus::SetDeckTopFaceDown(n));
    }
    out
}

fn parse_target_action(text: &str) -> Option<TargetAction> {
    // 「相手はそのカードを〜証拠として得る」と「手札から裏向きで証拠として得る」を区別
    if text.contains("相手はそのカードを表向きのまま証拠として得る")
        || text.contains("持ち主はそのカードを表向きで証拠として得る")
    {
        return Some(TargetAction::TurnIntoOwnerFaceUpEvidence);
    }
    if text.contains("証拠として得る") && !text.contains("相手はそのカード") && !text.contains("持ち主はそのカード") {
        return Some(TargetAction::PlaceHandCardAsEvidenceFaceDown);
    }
    if text.contains("このキャラとのコンタクトを発生させる") {
        return Some(TargetAction::GrantContactForceAttack);
    }
    if text.contains("デッキの上か下に移す") {
        return Some(TargetAction::BottomDeck);
    }
    if text.contains("使用する") && text.contains("イベント") {
        return Some(TargetAction::PlayEventFromHand);
    }
    if text.contains("表向きにする") || text.contains("表向きにし") {
        return Some(TargetAction::FlipEvidenceFaceUp);
    }
    if text.contains("裏向きにする") || text.contains("裏向きにし") {
        return Some(TargetAction::FlipEvidenceFaceDown);
    }
    if text.contains("手札に移す") || text.contains("手札に移し") {
        return Some(TargetAction::BounceToHand);
    }
    if text.contains("デッキの上に移") {
        return Some(TargetAction::DeckTopFromRemove);
    }
    if text.contains("手札に加える") || text.contains("手札に加え") {
        return Some(TargetAction::RecoverToHand);
    }
    if text.contains("裏向きで") && text.contains("セットする") && !text.contains("AP＋") {
        let n = extract_count_around(text, "上から", "枚裏向き").max(1);
        return Some(TargetAction::SetDeckTopFaceDown(n));
    }
    if text.contains("元のAPを0にする") {
        return Some(TargetAction::SetBaseApZero);
    }
    if let Some(lv_d) = extract_number_before(text, "レベル－", "する") {
        return Some(TargetAction::LevelDelta(-lv_d));
    }
    if text.contains("スリープ状態で登場") {
        return Some(TargetAction::ReviveSleep);
    }
    if text.contains("登場させ") {
        return Some(TargetAction::ReviveActive);
    }
    if text.contains("デッキの下に移") {
        return Some(TargetAction::BottomDeck);
    }
    if text.contains("リムーブ") {
        return Some(TargetAction::Remove);
    }
    if text.contains("スタン") {
        return Some(TargetAction::Stun);
    }
    if text.contains("スリープ") {
        return Some(TargetAction::Sleep);
    }
    if text.contains("アクティブに") {
        return Some(TargetAction::Activate);
    }
    if let Some(val) = parse_signed_ap(text, true) {
        return Some(TargetAction::ApBuff(val));
    }
    if let Some(val) = parse_signed_ap(text, false) {
        return Some(TargetAction::ApDebuff(val));
    }
    if text.contains("このキャラがコンタクトしたとき、カードを1枚引き、そのコンタクト中") {
        return Some(TargetAction::ApBuff(0));
    }
    if text.contains("アクション［事件］したとき、手札を1枚リムーブしてもよい") {
        return Some(TargetAction::GrantBulletOnCaseAttackDiscard);
    }
    if text.contains("〚ブレット〛") {
        return Some(TargetAction::GiveBullet);
    }
    if text.contains("〚突撃［レベル4以下のキャラ］〛") {
        return Some(TargetAction::GiveRush(Some("CHAR_LV4".to_string())));
    }
    if text.contains("〚突撃［事件］〛") {
        return Some(TargetAction::GiveRush(Some("CASE".to_string())));
    }
    if text.contains("〚突撃［キャラ］〛") {
        return Some(TargetAction::GiveRush(Some("CHAR".to_string())));
    }
    if text.contains("〚迅速〛") {
        return Some(TargetAction::GiveRush(Some("SWIFT".to_string())));
    }
    if text.contains("〚突撃〛") {
        return Some(TargetAction::GiveRush(None));
    }
    if text.contains("アクティブ状態のキャラを指定してアクションできる") {
        return Some(TargetAction::GiveCanAttackActive);
    }
    None
}

/// `ターゲットフレーズ`（例: `"このキャラ以外の〚特徴［毛利探偵事務所］〛のキャラ"` や `"相手のレベル5以下の【青】のキャラ"`）からフィルタを構築
pub fn parse_target_filter(
    raw_phrase: &str,
    zone: TargetZone,
    beneficial: bool,
    dynamic_level: Option<i32>,
) -> TargetFilter {
    // ターゲット自身の記述部分だけを取り出す（「...場合、」「...とき、」など直前の前提文を落とす）
    let phrase_pre = isolate_target_noun_phrase(raw_phrase);

    // B03063: 「自分の現場にいる〚特徴［空手家］〛のキャラと同じ数まで相手の現場にいるキャラ」
    let mut count_from_self_trait = None;
    let phrase: &str = if let Some(cidx) = phrase_pre.find("と同じ数まで") {
        count_from_self_trait = extract_bracketed_value(&phrase_pre[..cidx], "特徴［", "］")
            .map(|s| s.to_string());
        phrase_pre[cidx + "と同じ数まで".len()..].trim_start()
    } else {
        phrase_pre
    };

    let exclude_self = phrase.contains("このキャラ以外の");
    let require_in_contact = phrase.contains("コンタクト中の");
    let self_only = phrase.starts_with("このキャラ") && !exclude_self;

    let owner = if zone == TargetZone::Hand || zone == TargetZone::Deck {
        OwnerScope::SelfOnly
    } else if self_only {
        OwnerScope::SelfOnly
    } else if phrase.contains("自分か相手の") || phrase.contains("どちらかの") {
        OwnerScope::Both
    } else if phrase.contains("相手の") {
        OwnerScope::OppOnly
    } else if phrase.contains("自分の") || zone == TargetZone::Remove || zone == TargetZone::SetCards {
        if phrase.contains("相手の") {
            OwnerScope::OppOnly
        } else {
            OwnerScope::SelfOnly
        }
    } else if require_in_contact {
        OwnerScope::Both
    } else if beneficial {
        OwnerScope::SelfOnly
    } else {
        OwnerScope::OppOnly
    };
    let exclude_prev_chosen = phrase.contains("そのキャラ以外の");
    let acting_only = phrase.contains("アクション中の");
    let target_prev_chosen = phrase == "そのキャラ" || phrase == "登場させたキャラ";

    let mut orientations = Vec::new();
    if phrase.contains("スリープ状態かスタン状態") || phrase.contains("アクティブ状態ではない")
    {
        orientations.push(CardOrientation::SLEEP);
        orientations.push(CardOrientation::STUN);
    } else if phrase.contains("アクティブ状態かスリープ状態") {
        orientations.push(CardOrientation::ACTIVE);
        orientations.push(CardOrientation::SLEEP);
    } else if phrase.contains("スリープ状態の") {
        orientations.push(CardOrientation::SLEEP);
    } else if phrase.contains("スタン状態の") {
        orientations.push(CardOrientation::STUN);
    } else if phrase.contains("アクティブ状態の") {
        orientations.push(CardOrientation::ACTIVE);
    }

    let any_count = phrase.contains("を好きな数");
    let up_to = any_count || phrase.contains("まで") || count_from_self_trait.is_some();
    let count = if any_count {
        99
    } else if zone == TargetZone::SetCards {
        extract_count_around(phrase, "合わせて", "枚")
            .max(extract_count_around(phrase, "を", "枚"))
            .max(1)
    } else {
        extract_count_around(phrase, "を", "枚")
            .max(extract_count_around(phrase, "を", "つ"))
            .max(1)
    };

    let distinct_names = phrase.contains("それぞれカード名の異なる");
    let distinct_levels = phrase.contains("それぞれレベルの異なる");
    let distinct_colors = phrase.contains("同じ色を持たない");
    let max_total_level = extract_number_before(phrase, "レベルの合計が", "以下");
    let min_level_from_prev = phrase.contains("そのカードのレベル以上");
    let max_level_from_prev = phrase.contains("そのキャラのレベル以下");
    let max_level_from_removed_sum =
        phrase.contains("移したカードのレベルの合計以下")
            || phrase.contains("リムーブしたカードのレベルの合計以下");
    let same_name_as_prev = phrase.contains("登場させたキャラと同じカード名");
    let require_no_other_ability =
        phrase.contains("【カットイン】と【ヒラメキ】以外の元の能力を持たない")
            || phrase.contains("元の能力を持たない");
    let face_up_only = phrase.contains("表向きの");
    let face_down_only = phrase.contains("裏向きの") || phrase.contains("裏向きで");

    let card_type_filter = if phrase.contains("イベント") && !phrase.contains("キャラ") {
        Some(CardType::EVENT)
    } else if zone == TargetZone::Evidence || zone == TargetZone::SetCards {
        None
    } else if phrase.contains("カード") && !phrase.contains("カード名") {
        None
    } else {
        None
    };
    let any_card_type =
        zone == TargetZone::Evidence
            || zone == TargetZone::SetCards
            || (phrase.contains("カード") && !phrase.contains("カード名") && !phrase.contains("イベント") && !phrase.contains("キャラ"));

    let default_card_type = if phrase.contains("イベント") && !phrase.contains("キャラ") {
        CardType::EVENT
    } else {
        CardType::CHARACTER
    };

    let mut branches = parse_branches_from_phrase(phrase, default_card_type);
    if phrase.contains("そのカードのレベル以下") {
        if let Some(dlv) = dynamic_level {
            for b in &mut branches {
                b.max_level = Some(b.max_level.map_or(dlv, |prev| prev.min(dlv)));
            }
        }
    }

    TargetFilter {
        owner,
        zone,
        exclude_self,
        acting_only,
        orientations,
        count,
        branches,
        up_to,
        any_count,
        distinct_names,
        distinct_levels,
        distinct_colors,
        max_total_level,
        min_level_from_prev,
        max_level_from_prev,
        max_level_from_removed_sum,
        same_name_as_prev,
        exclude_prev_chosen,
        target_prev_chosen,
        require_no_other_ability,
        card_type_filter,
        any_card_type,
        face_up_only,
        face_down_only,
        count_from_self_trait,
        max_lp_from_self_max: false,
        require_in_contact,
        self_only,
    }
}

/// 接続詞・前提句の末尾（「、」「：」「場合、」等）を落として対象名詞句を抽出する
fn isolate_target_noun_phrase(raw: &str) -> &str {
    // まず最後の `、` の後ろを確認（「〜とき、このキャラ以外の...」）
    if let Some(idx) = raw.rfind("とき、") {
        return &raw[idx + "とき、".len()..];
    }
    if let Some(idx) = raw.rfind("場合、") {
        return &raw[idx + "場合、".len()..];
    }
    if let Some(idx) = raw.rfind("そうした場合、") {
        return &raw[idx + "そうした場合、".len()..];
    }
    raw
}

/// OR (`か`) で繋がった特徴/名前/レベルの複数ブランチを解析する
pub fn parse_branches_from_phrase(phrase: &str, default_card_type: CardType) -> Vec<TargetBranch> {
    // 「〜のイベントかキャラ」「【カットイン】を持つ【緑】のカード」のような共通修飾
    let trailing_types = infer_card_types(phrase, default_card_type);
    let global_keyword = if phrase.contains("【カットイン】と【ヒラメキ】以外") {
        None
    } else if phrase.contains("【カットイン】を持つ") {
        Some("【カットイン】".to_string())
    } else if phrase.contains("【ヒラメキ】を持つ") {
        Some("【ヒラメキ】".to_string())
    } else if phrase.contains("【現場リムーブ時】を持つ") {
        Some("【現場リムーブ時】".to_string())
    } else if phrase.contains("【変装】を持つ") {
        Some("【変装】".to_string())
    } else if phrase.contains("【疾風】を持つ") {
        Some("【疾風】".to_string())
    } else {
        None
    };

    // 全体を先に単一解析してグローバル上限（レベル/AP/色）を取得しておく
    let global_base = parse_single_branch(phrase, trailing_types.clone(), global_keyword.clone());

    // `〛か` や `］か` で繋がれた複合OR条件があるかチェック
    let or_parts: Vec<&str> = split_or_segments(phrase);
    if or_parts.len() <= 1 {
        return vec![global_base];
    }

    let mut branches = Vec::new();
    let mut last_was_name = global_base.name_req.is_some() || global_base.exclude_name.is_some();
    let mut last_was_trait = global_base.trait_req.is_some() || global_base.exclude_trait.is_some();
    for (idx, part) in or_parts.iter().enumerate() {
        let mut b = parse_single_branch(part, trailing_types.clone(), global_keyword.clone());
        if b.name_req.is_some() || b.exclude_name.is_some() {
            last_was_name = true;
            last_was_trait = false;
        } else if b.trait_req.is_some() || b.exclude_trait.is_some() {
            last_was_trait = true;
            last_was_name = false;
        } else if let Some(short) = extract_bracketed_value(part, "〚［", "］") {
            if last_was_name {
                b.name_req = Some(short.to_string());
            } else if last_was_trait {
                b.trait_req = Some(short.to_string());
            } else {
                b.name_req = Some(short.to_string());
            }
        }
        // 先頭にだけ書かれた「レベル5以下の」や末尾の「【青】の」を他のブランチにも継承
        if idx > 0 && b.max_level.is_none() {
            b.max_level = branches.first().and_then(|f: &TargetBranch| f.max_level);
        }
        if idx > 0 && b.min_level.is_none() {
            b.min_level = branches.first().and_then(|f: &TargetBranch| f.min_level);
        }
        if global_base.max_level_from_file {
            b.max_level_from_file = true;
        }
        if b.colors.is_empty() && !global_base.colors.is_empty() {
            // 「【青】か【白】」のときのみ個別色を優先、単一色修飾が末尾なら共有
            if or_parts.iter().all(|p| extract_color_filters(p).is_empty())
                || (idx + 1 < or_parts.len() && extract_color_filters(part).is_empty())
            {
                b.colors = global_base.colors.clone();
            }
        }
        branches.push(b);
    }
    branches
}

fn split_or_segments(phrase: &str) -> Vec<&str> {
    // 〚...〛か〚...〛 や 特徴［A］か特徴［B］ の `か` のみを分割点にする（「自分の現場か...」等には誤爆しない）
    let mut split_points = Vec::new();
    for pat in ["〛か", "］か"] {
        let mut search_from = 0;
        while let Some(pos) = phrase[search_from..].find(pat) {
            let abs = search_from + pos;
            let close_len = pat.len() - "か".len();
            let ka_pos = abs + close_len;
            split_points.push(ka_pos);
            search_from = abs + pat.len();
        }
    }
    if split_points.is_empty() {
        return vec![phrase];
    }
    split_points.sort_unstable();
    split_points.dedup();
    let mut out = Vec::new();
    let mut prev = 0;
    for sp in split_points {
        out.push(&phrase[prev..sp]);
        prev = sp + "か".len();
    }
    out.push(&phrase[prev..]);
    out
}

fn infer_card_types(phrase: &str, default_card_type: CardType) -> Vec<CardType> {
    if phrase.contains("カード") && !phrase.contains("カード名") {
        // 「【緑】のカードを1枚」-> キャラ＆イベント両方
        return vec![CardType::CHARACTER, CardType::EVENT];
    }
    let mut types = Vec::new();
    if phrase.contains("キャラ") {
        types.push(CardType::CHARACTER);
    }
    if phrase.contains("イベント") {
        types.push(CardType::EVENT);
    }
    if types.is_empty() {
        types.push(default_card_type);
    }
    types
}

fn extract_color_filters(phrase: &str) -> Vec<String> {
    let mut cols = Vec::new();
    for col in COLORS {
        let tag = format!("【{}】", col);
        if phrase.contains(&tag) {
            cols.push((*col).to_string());
        }
    }
    cols
}

fn parse_single_branch(
    raw_part: &str,
    card_types: Vec<CardType>,
    require_keyword: Option<String>,
) -> TargetBranch {
    let part_owned = raw_part.replace("レべル", "レベル");
    let part = part_owned.as_str();
    let mut b = TargetBranch {
        card_types,
        require_keyword,
        colors: extract_color_filters(part),
        ..Default::default()
    };

    // 「〚カード名［X］〛以外の」 / 「〚特徴［Y］〛以外の」チェック
    if let Some(name) = extract_bracketed_value(part, "カード名［", "］") {
        let check_after = format!("カード名［{}］", name);
        if let Some(pos) = part.find(&check_after) {
            let tail = &part[pos + check_after.len()..];
            if tail.starts_with("以外の") || tail.starts_with("〛以外の") {
                b.exclude_name = Some(name.to_string());
            } else {
                b.name_req = Some(name.to_string());
            }
        }
    }
    if let Some(tr) = extract_bracketed_value(part, "特徴［", "］") {
        let check_after = format!("特徴［{}］", tr);
        if let Some(pos) = part.find(&check_after) {
            let tail = &part[pos + check_after.len()..];
            if tail.starts_with("以外の") || tail.starts_with("〛以外の") {
                b.exclude_trait = Some(tr.to_string());
            } else {
                b.trait_req = Some(tr.to_string());
            }
        }
    }

    b.max_level = extract_number_before(part, "レベル", "以下")
        .or_else(|| extract_number_before(part, "コスト", "以下"));
    b.max_level_from_file = part.contains("FILEエリアの枚数以下");
    b.min_level = extract_number_before(part, "レベル", "以上")
        .or_else(|| extract_number_before(part, "コスト", "以上"));
    if b.max_level.is_none() && b.min_level.is_none() {
        if let Some(exact_lv) = extract_number_before(part, "レベル", "の")
            .or_else(|| extract_number_before(part, "レベル", "が出"))
        {
            b.max_level = Some(exact_lv);
            b.min_level = Some(exact_lv);
        }
    }
    b.max_ap = extract_number_before(part, "AP", "以下");
    b.min_ap = extract_number_before(part, "AP", "以上");
    b.max_lp = extract_number_before(part, "LP", "以下");
    b.min_lp = extract_number_before(part, "LP", "以上");
    b.exact_lp = extract_number_before(part, "LP", "の");

    b
}

/// 統一ターゲット一致判定：UI提示・自動探索・サーバー回答検証のすべてで同一関数を使う
pub fn card_matches_filter(
    catalog: &CardCatalog,
    actor_id: Option<&str>,
    owner_player_id: &str,
    card_owner_id: &str,
    card: &CardInstance,
    effective_ap: i32,
    filter: &TargetFilter,
    opp_cond_info: Option<&PlayerCondInfo>,
    acting_card_id: Option<&str>,
) -> bool {
    card_matches_filter_with_ctx(
        catalog,
        actor_id,
        owner_player_id,
        card_owner_id,
        card,
        effective_ap,
        filter,
        opp_cond_info,
        acting_card_id,
        None,
    )
}

pub fn has_other_ability_besides_cutin_and_hirameki(def: &CardDefinition) -> bool {
    let norm = def.description.replace("\r\n", "\n").replace('\r', "\n");
    for line in norm.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if !line.starts_with("【カットイン】") && !line.starts_with("【ヒラメキ】") {
            return true;
        }
    }
    false
}

pub fn card_matches_filter_with_ctx(
    catalog: &CardCatalog,
    actor_id: Option<&str>,
    owner_player_id: &str,
    card_owner_id: &str,
    card: &CardInstance,
    effective_ap: i32,
    filter: &TargetFilter,
    opp_cond_info: Option<&PlayerCondInfo>,
    acting_card_id: Option<&str>,
    self_file_count: Option<i32>,
) -> bool {
    match filter.owner {
        OwnerScope::SelfOnly if card_owner_id != owner_player_id => return false,
        OwnerScope::OppOnly if card_owner_id == owner_player_id => return false,
        OwnerScope::Both => {}
        _ => {}
    }
    if filter.exclude_self && actor_id == Some(card.instance_id.as_str()) {
        return false;
    }
    if filter.self_only && actor_id != Some(card.instance_id.as_str()) {
        return false;
    }
    if filter.acting_only {
        if let Some(aid) = acting_card_id {
            if card.instance_id != aid {
                return false;
            }
        }
    }
    if filter.face_up_only && !card.face_up {
        return false;
    }
    if filter.face_down_only && card.face_up {
        return false;
    }
    if !filter.orientations.is_empty() && !filter.orientations.contains(&card.orientation) {
        return false;
    }
    let Some(def) = catalog.get(&card.definition_id) else {
        return false;
    };
    if let Some(ct) = filter.card_type_filter {
        if def.card_type != ct {
            return false;
        }
    }
    if filter.require_no_other_ability && has_other_ability_besides_cutin_and_hirameki(def) {
        return false;
    }
    if card_owner_id != owner_player_id {
        if let Some(opp_ctx) = opp_cond_info {
            if card_is_untargetable_by_opponent(def, opp_ctx) {
                return false;
            }
        }
    }
    filter.branches.iter().any(|b| {
        if card.temp_name_override.is_some() {
            if let Some(ref nm) = b.name_req {
                if !has_instance_name_match(card, def, nm) {
                    return false;
                }
                let mut b_copy = b.clone();
                b_copy.name_req = None;
                return branch_matches_with_ctx(def, effective_ap, &b_copy, self_file_count);
            }
            if let Some(ref ex_nm) = b.exclude_name {
                if has_instance_name_match(card, def, ex_nm) {
                    return false;
                }
            }
        }
        branch_matches_with_ctx(def, effective_ap, b, self_file_count)
    })
}

/// `CardDefinition` + 有効APが 1 つの `TargetBranch` に一致するか判定
pub fn branch_matches(def: &CardDefinition, effective_ap: i32, b: &TargetBranch) -> bool {
    branch_matches_with_ctx(def, effective_ap, b, None)
}

/// FILE枚数など動的コンテキスト付きで `TargetBranch` 一致判定を行う
pub fn branch_matches_with_ctx(
    def: &CardDefinition,
    effective_ap: i32,
    b: &TargetBranch,
    self_file_count: Option<i32>,
) -> bool {
    if !b.card_types.is_empty() && !b.card_types.contains(&def.card_type) {
        return false;
    }
    if !b.colors.is_empty() && !b.colors.iter().any(|c| def.colors.iter().any(|dc| dc == c)) {
        return false;
    }
    if let Some(ref req_kw) = b.require_keyword {
        if !def.description.contains(req_kw.as_str()) {
            return false;
        }
    }
    if let Some(ref tr) = b.trait_req {
        if !has_trait_match(def, tr) {
            return false;
        }
    }
    if let Some(ref ex_tr) = b.exclude_trait {
        if has_trait_match(def, ex_tr) {
            return false;
        }
    }
    if let Some(ref nm) = b.name_req {
        if !has_name_match(def, nm) {
            return false;
        }
    }
    if let Some(ref ex_nm) = b.exclude_name {
        if has_name_match(def, ex_nm) {
            return false;
        }
    }
    if let Some(max_lv) = b.max_level {
        if def.level > max_lv {
            return false;
        }
    }
    if b.max_level_from_file {
        if let Some(fc) = self_file_count {
            if def.level > fc {
                return false;
            }
        }
    }
    if let Some(min_lv) = b.min_level {
        if def.level < min_lv {
            return false;
        }
    }
    if let Some(max_ap) = b.max_ap {
        if effective_ap > max_ap {
            return false;
        }
    }
    if let Some(min_ap) = b.min_ap {
        if effective_ap < min_ap {
            return false;
        }
    }
    if let Some(max_lp) = b.max_lp {
        if def.lp > max_lp {
            return false;
        }
    }
    if let Some(min_lp) = b.min_lp {
        if def.lp < min_lp {
            return false;
        }
    }
    if let Some(eq_lp) = b.exact_lp {
        if def.lp != eq_lp {
            return false;
        }
    }
    true
}

/// 対象選択を伴わない即時処理文を `Vec<ImmediateOp>` へ変換する
/// 「カードを2枚引き、手札を2枚リムーブする」のように順序も保持する
pub fn parse_immediate_ops(sentence: &str) -> Vec<ImmediateOp> {
    let mut ops = Vec::new();

    // 全文レベルの全体効果・制限チェック
    if sentence.contains("相手の現場にいるすべてのキャラをスリープさせる") {
        ops.push(ImmediateOp::MassOppSleepAll);
        return ops;
    }
    if sentence.contains("スリープ状態のすべてのキャラをスタンさせる") {
        ops.push(ImmediateOp::MassAllSleepToStun);
        return ops;
    }
    if sentence.contains("コンタクト中のすべてのキャラをリムーブする") {
        ops.push(ImmediateOp::MassContactRemove);
        return ops;
    }
    if sentence.contains("すべてのキャラをリムーブする") {
        ops.push(ImmediateOp::MassAllRemove);
    }
    if sentence.contains("すべてのキャラをターン終了時までAP＋") {
        if let Some(tr) = extract_bracketed_value(sentence, "特徴［", "］") {
            if let Some(ap) = parse_signed_ap(sentence, true) {
                ops.push(ImmediateOp::MassSelfTraitApBuff {
                    trait_name: tr.to_string(),
                    buff: ap,
                });
                return ops;
            }
        }
    }
    if sentence.contains("このカードを表向きのまま証拠として得る") {
        ops.push(ImmediateOp::GainThisAsFaceUpEvidence);
        return ops;
    }
    if sentence.contains("ネクストヒントできない") {
        ops.push(ImmediateOp::BlockHintThisTurn);
        return ops;
    }
    if sentence.contains("自分はイベントを使用できない") {
        ops.push(ImmediateOp::BlockSelfEventsThisTurn);
        return ops;
    }
    if sentence.contains("相手に証拠を1つ与えてもよい") {
        ops.push(ImmediateOp::OptionalGiveOppEvidence(1));
        return ops;
    }
    if sentence.contains("キャラ1枚につき、自分のデッキのカードを上から") && sentence.contains("リムーブ") {
        let n = extract_count_around(sentence, "上から", "枚リムーブ").max(2);
        ops.push(ImmediateOp::DeckTopRemovePerFieldChar(n));
        return ops;
    }

    for part in sentence.split('、') {
        let p = part.trim();
        if p.is_empty() {
            continue;
        }
        let draw = extract_count_around(p, "カードを", "枚引く")
            .max(extract_count_around(p, "を", "枚引"));
        if draw > 0 && p.contains('引') {
            ops.push(ImmediateOp::Draw(draw));
        }

        if p.contains("手札が") && p.contains("枚になるまで手札をリムーブ") {
            let keep = extract_count_around(p, "手札が", "枚になるまで");
            ops.push(ImmediateOp::DiscardHandTo(keep));
        } else if p.contains("手札をすべてリムーブ") {
            ops.push(ImmediateOp::DiscardAllHand);
        } else if !p.contains("相手") {
            let (discard, hand_filter) = extract_hand_remove_spec(p);
            if discard > 0 {
                if let Some(hf) = hand_filter {
                    ops.push(ImmediateOp::DiscardFilteredHand {
                        count: discard,
                        filter: hf,
                    });
                } else {
                    ops.push(ImmediateOp::DiscardHand(discard));
                }
            }
        }

        if p.contains("FILEエリアにあるカードを上から") && (p.contains("手札に加え") || p.contains("手札に加える")) {
            let n = extract_count_around(p, "上から", "枚手札").max(1);
            ops.push(ImmediateOp::FileTopToHand(n));
        } else if p.contains("FILEエリアにあるカードを上から") && p.contains("リムーブ") {
            let n = extract_count_around(p, "上から", "枚リムーブ").max(1);
            ops.push(ImmediateOp::FileTopRemove(n));
        }

        if p.contains("相手の証拠を上から") && p.contains("リムーブ") {
            let n = extract_count_around(p, "上から", "つリムーブ").max(1);
            ops.push(ImmediateOp::OppTopEvidenceRemove(n));
        } else if p.contains("相手の証拠") && p.contains("リムーブ") {
            let n = extract_count_around(p, "相手の証拠を", "つリムーブ")
                .max(extract_count_around(p, "上から", "つリムーブ"))
                .max(1);
            ops.push(ImmediateOp::OppRemoveEvidence(n));
        } else if p.contains("証拠") && p.contains("リムーブ") && !p.contains("コスト") {
            let n = extract_count_around(p, "上から", "つリムーブ")
                .max(extract_count_around(p, "証拠を", "つリムーブ"))
                .max(1);
            ops.push(ImmediateOp::SelfRemoveEvidence {
                count: n,
                trigger_hirameki: !sentence.contains("【ヒラメキ】は発動しない"),
            });
        } else if p.contains("自分の裏向きの証拠") && sentence.contains("表向き") {
            let n = extract_count_around(p, "証拠を", "つ").max(1);
            ops.push(ImmediateOp::FlipEvidenceUp(n));
        } else if !p.contains("相手") && p.contains("表向きの証拠") && sentence.contains("裏向き") {
            let n = extract_count_around(p, "証拠を", "つ").max(1);
            ops.push(ImmediateOp::FlipEvidenceDown(n));
        } else if p.contains("相手の") && p.contains("証拠") && sentence.contains("表向き") {
            let n = extract_count_around(p, "証拠を", "つ").max(1);
            ops.push(ImmediateOp::OppFlipEvidenceUp(n));
        } else if !p.contains("相手に") && p.contains("証拠を") && (p.contains("得る") || p.contains("増やす")) {
            let n = extract_count_around(p, "証拠を", "つ").max(1);
            ops.push(ImmediateOp::GainEvidence(n));
        }

        if p.contains("このカードを手札に加える") {
            ops.push(ImmediateOp::MoveSelfFromEvidenceToHand);
        }
        if p.contains("このカードをパートナーエリアに移す") {
            ops.push(ImmediateOp::MoveSelfFromEvidenceToPartner);
        }
        if p.contains("相手は手札を") && p.contains("ランダムにリムーブ") {
            let n = extract_count_around(p, "手札を", "枚ランダム").max(1);
            ops.push(ImmediateOp::OppDiscardRandom(n));
        } else if p.contains("相手は手札を") && p.contains("リムーブ") {
            let n = extract_count_around(p, "手札を", "枚リムーブ").max(1);
            ops.push(ImmediateOp::OppDiscardHand(n));
        }
        if sentence.contains("相手は") && p.contains("【カットイン】") && sentence.contains("使用できない") {
            ops.push(ImmediateOp::BlockOppCutinThisTurn);
        }
        if (p.contains("【変装】") && (p.contains("使用できない") || sentence.contains("使用できない")))
            || p.contains("相手は【変装】を使用できない")
        {
            ops.push(ImmediateOp::BlockOppDisguiseThisTurn);
        }
        if sentence.contains("その【ヒラメキ】の効果を発動") {
            if sentence.contains("松田陣平") {
                ops.push(ImmediateOp::TriggerFieldHirameki("松田陣平".to_string()));
            } else if sentence.contains("YAIBA") {
                ops.push(ImmediateOp::TriggerEvidenceHiramekiByTrait("YAIBA".to_string()));
            }
        }

        if p.contains("相手のデッキのカードを上から") && p.contains("リムーブ") {
            let n = extract_count_around(p, "上から", "枚リムーブ");
            if n > 0 {
                ops.push(ImmediateOp::OppMillDeck(n));
            }
        } else if p.contains("デッキのカードを上から") && p.contains("リムーブ") {
            if p.contains("そのカードのレベルと同じ枚数リムーブ") {
                ops.push(ImmediateOp::MillDeckByLastChosenLevel);
            } else {
                let n = extract_count_around(p, "上から", "枚リムーブ");
                if n > 0 {
                    ops.push(ImmediateOp::MillDeck(n));
                }
            }
        }

        if let Some(buff) = parse_signed_ap(p, true) {
            ops.push(ImmediateOp::SelfApBuff(buff));
        }

        if p.contains("このキャラをアクティブに") {
            ops.push(ImmediateOp::SelfActivate);
        }

        if !p.contains("与える") && (p.contains("このキャラ") || sentence.contains("このキャラ")) {
            if p.contains("〚突撃［レベル4以下のキャラ］〛") {
                ops.push(ImmediateOp::SelfGiveRush(Some("CHAR_LV4".to_string())));
            } else if p.contains("〚突撃［事件］〛") {
                ops.push(ImmediateOp::SelfGiveRush(Some("CASE".to_string())));
            } else if p.contains("〚突撃［キャラ］〛") {
                ops.push(ImmediateOp::SelfGiveRush(Some("CHAR".to_string())));
            } else if p.contains("〚迅速〛") {
                ops.push(ImmediateOp::SelfGiveRush(Some("SWIFT".to_string())));
            } else if p.contains("〚突撃〛") {
                ops.push(ImmediateOp::SelfGiveRush(None));
            }
        }
    }
    ops
}

/// アクション系トリガー種別
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionEventKind {
    Deduce,
    AttackChar,
    AttackCase,
}

/// パッシブ効果（自己バフ・全体オーラ・LP修正）評価コンテキスト
pub struct PassiveEvalContext<'a> {
    pub source_instance_id: &'a str,
    #[allow(dead_code)]
    pub source_def: &'a CardDefinition,
    pub source_in_partner_area: bool,
    pub source_cond: &'a PlayerCondInfo<'a>,
    pub target_instance_id: &'a str,
    pub target_def: &'a CardDefinition,
    pub target_base_ap: i32,
    pub target_is_friendly_to_source: bool,
    pub opp_field_cards: &'a [(&'a CardInstance, &'a CardDefinition)],
    pub self_hand_count: usize,
    pub self_faceup_evidence_count: usize,
}

/// `【ターン1】` / `【ターン2】` などのターン回数制限を返す
pub fn clause_turn_limit(clause: &str) -> Option<usize> {
    for tag in extract_bracket_tags(clause) {
        if let Some(rest) = tag.strip_prefix("ターン") {
            if let Ok(n) = rest.parse::<usize>() {
                return Some(n);
            }
        }
    }
    None
}

/// パートナーエリアでも宣言・発動・有効になる能力か
pub fn allows_partner_area(clause: &str) -> bool {
    clause.contains("パートナーエリアでも")
}

/// 突撃・迅速能力の合算（突撃[事件] + 突撃[キャラ] -> 無制限突撃、迅速 -> 迅速＋突撃 等）
pub fn merge_rush_capabilities(
    cur_rush: &mut bool,
    cur_target: &mut Option<String>,
    cur_swift: &mut bool,
    new_target: Option<&str>,
) {
    match new_target {
        Some("SWIFT") => {
            *cur_rush = true;
            *cur_swift = true;
            *cur_target = None;
        }
        None => {
            *cur_rush = true;
            *cur_target = None;
        }
        Some("CHAR") => {
            if !*cur_rush {
                *cur_rush = true;
                *cur_target = Some("CHAR".to_string());
            } else if matches!(cur_target.as_deref(), Some("CASE") | Some("CHAR_LV4+CASE")) {
                *cur_target = None;
            } else if cur_target.as_deref() == Some("CHAR_LV4") {
                *cur_target = Some("CHAR".to_string());
            }
        }
        Some("CHAR_LV4") => {
            if !*cur_rush {
                *cur_rush = true;
                *cur_target = Some("CHAR_LV4".to_string());
            } else if cur_target.as_deref() == Some("CASE") {
                *cur_target = Some("CHAR_LV4+CASE".to_string());
            }
        }
        Some("CASE") => {
            if !*cur_rush {
                *cur_rush = true;
                *cur_target = Some("CASE".to_string());
            } else if cur_target.as_deref() == Some("CHAR") {
                *cur_target = None;
            } else if cur_target.as_deref() == Some("CHAR_LV4") {
                *cur_target = Some("CHAR_LV4+CASE".to_string());
            }
        }
        Some(other) => {
            if !*cur_rush {
                *cur_rush = true;
                *cur_target = Some(other.to_string());
            }
        }
    }
}

fn parse_rush_keyword_token(text: &str) -> Option<(bool, Option<&'static str>)> {
    if text.contains("〚突撃［レベル4以下のキャラ］〛") || text.contains("【突撃［レベル4以下のキャラ］】") {
        return Some((false, Some("CHAR_LV4")));
    }
    if text.contains("〚突撃［事件］〛") || text.contains("【突撃［事件］】") {
        return Some((false, Some("CASE")));
    }
    if text.contains("〚突撃［キャラ］〛") || text.contains("【突撃［キャラ］】") {
        return Some((false, Some("CHAR")));
    }
    if text.contains("〚迅速〛") || text.contains("【迅速】") {
        return Some((true, Some("SWIFT")));
    }
    if text.contains("〚突撃〛") || text.contains("【突撃】") {
        return Some((false, None));
    }
    None
}

/// カード自身の常時パッシブ効果・セットカード・動的バフから実効的な (突撃可否, 突撃制限, 迅速可否) を評価する
pub fn evaluate_passive_rush_and_swift(
    card: &CardInstance,
    def: &CardDefinition,
    ctx: &PlayerCondInfo,
) -> (bool, Option<String>, bool) {
    let mut has_rush = card.has_rush;
    let mut rush_target = card.rush_target.clone();
    let mut has_swift = rush_target.as_deref() == Some("SWIFT");
    if has_swift {
        rush_target = None;
    }

    if card.has_rush {
        if let Some(rt) = card.rush_target.as_deref() {
            let mut r = false;
            let mut t = None;
            let mut s = false;
            merge_rush_capabilities(&mut r, &mut t, &mut s, Some(rt));
            has_rush = r;
            rush_target = t;
            has_swift = s;
        }
    }

    let has_underneath_header = def.description.contains("このキャラの下に重なっているカードの数につき");

    for raw_line in def.description.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let clean = strip_parens(trimmed);
        let clean_trimmed = clean.trim();
        if clean_trimmed.is_empty() {
            continue;
        }

        // トリガー句・宣言句・引用付与句・時限付与文はパッシブ突撃として扱わない
        let leading_tags = extract_leading_bracket_tags(clean_trimmed);
        if leading_tags.iter().any(|t| {
            matches!(
                *t,
                "登場時" | "変装時" | "宣言" | "ヒラメキ" | "カットイン" | "現場リムーブ時" | "ターン終了時" | "アクション終了時" | "変装"
            )
        }) {
            continue;
        }
        if clean_trimmed.contains('：')
            || clean_trimmed.contains("とき、")
            || clean_trimmed.contains('「')
            || clean_trimmed.contains("を与える")
            || clean_trimmed.contains("を与え")
            || clean_trimmed.contains("ターン終了時まで")
            || clean_trimmed.contains("このキャラ以外の")
        {
            continue;
        }

        // 前提タグ条件の確認（「自分のターン中」「解決編」「パートナー黒」「FILE5」「1枚以上」等）
        if check_clause_conditions(trimmed, ctx).is_err() {
            continue;
        }
        let mut under_tag_ok = true;
        for tag in &leading_tags {
            if let Some(num_s) = tag.strip_suffix("枚以上") {
                if let Ok(req_under) = num_s.parse::<usize>() {
                    if (has_underneath_header && !ctx.is_my_turn) || card.set_cards.len() < req_under {
                        under_tag_ok = false;
                        break;
                    }
                }
            }
        }
        if !under_tag_ok {
            continue;
        }

        let body = strip_leading_tags(clean_trimmed).trim();
        if body.is_empty() {
            continue;
        }

        // 1) 条件付きパッシブ「〜場合、このキャラは〚突撃/迅速〛...を持つ」
        if let Some(case_idx) = body.find("場合、") {
            let cond_part = body[..case_idx].trim();
            let then_part = body[case_idx + "場合、".len()..].trim();
            if then_part.contains("このキャラは") {
                if let Some((_, tok)) = parse_rush_keyword_token(then_part) {
                    if eval_cond_phrase(cond_part, ctx) {
                        merge_rush_capabilities(&mut has_rush, &mut rush_target, &mut has_swift, tok);
                    }
                }
            }
            continue;
        }

        // 2) 常時（タグ条件のみ、または無条件）キーワード行：「〚突撃〛」「AP＋1000、〚突撃［キャラ］〛」「このキャラは〚突撃〛を持つ」
        if let Some((_, tok)) = parse_rush_keyword_token(body) {
            merge_rush_capabilities(&mut has_rush, &mut rush_target, &mut has_swift, tok);
        }
    }

    (has_rush, rush_target, has_swift)
}

/// ネクストヒント使用トリガー条件（例: `【青】のカードを使用したとき`）に一致するか判定する
pub fn check_hint_use_match(clause: &str, hint_colors: &[String]) -> bool {
    let clean = strip_parens(clause);
    let body = strip_leading_tags(&clean);
    let trig = body.split("とき、").next().unwrap_or(body);
    let trig_with_toki = format!("{}とき", trig);
    if !trig_with_toki.contains("ネクストヒントで") || !trig_with_toki.contains("を使用したとき") {
        return false;
    }
    let req_colors = extract_color_filters(trig);
    if !req_colors.is_empty() {
        return hint_colors
            .iter()
            .any(|hc| req_colors.iter().any(|rc| rc == hc));
    }
    true
}

/// アクション・推理時トリガー条件に一致するか判定する
#[allow(clippy::too_many_arguments)]
pub fn check_action_trigger_match(
    clause: &str,
    kind: ActionEventKind,
    owner_is_acting: bool,
    self_instance_id: &str,
    actor_instance_id: &str,
    actor_def: &CardDefinition,
    actor_ap: i32,
    target_instance_id: Option<&str>,
    target_def: Option<&CardDefinition>,
    target_ap: Option<i32>,
) -> bool {
    let clean = strip_parens(clause);
    let body = strip_leading_tags(&clean);
    let Some((trig, _)) = body.split_once("とき、") else {
        return false;
    };
    let trig_with_toki = format!("{}とき", trig);

    let kind_ok = if trig_with_toki.contains("アクション［事件］したとき") {
        kind == ActionEventKind::AttackCase
    } else if trig_with_toki.contains("アクション［キャラ］したとき") {
        kind == ActionEventKind::AttackChar
    } else if trig_with_toki.contains("推理したとき") {
        kind == ActionEventKind::Deduce
    } else if trig_with_toki.contains("推理かアクションしたとき") {
        true
    } else if trig_with_toki.contains("アクションしたとき") {
        matches!(kind, ActionEventKind::AttackChar | ActionEventKind::AttackCase)
    } else {
        false
    };
    if !kind_ok {
        return false;
    }

    if let Some(diff) = extract_number_before(trig, "APが", "以上低い") {
        if kind != ActionEventKind::AttackChar {
            return false;
        }
        let Some(tap) = target_ap else {
            return false;
        };
        if tap > actor_ap - diff {
            return false;
        }
    }

    if trig.starts_with("相手の現場にいる") || trig.contains("相手の現場にいるキャラが") {
        if owner_is_acting {
            return false;
        }
        if trig.contains("このキャラを指定して") {
            return kind == ActionEventKind::AttackChar
                && target_instance_id == Some(self_instance_id);
        }
        if let Some(spec_idx) = trig.find("を指定して") {
            if kind != ActionEventKind::AttackChar {
                return false;
            }
            let (Some(tdef), Some(tap)) = (target_def, target_ap) else {
                return false;
            };
            let phrase_slice = if let Some(s) = trig[..spec_idx].find("自分の現場にいる") {
                &trig[s + "自分の現場にいる".len()..spec_idx]
            } else {
                &trig[..spec_idx]
            };
            let branches = parse_branches_from_phrase(phrase_slice, CardType::CHARACTER);
            return branches.iter().any(|b| branch_matches(tdef, tap, b));
        }
        return true;
    }

    if !owner_is_acting {
        return false;
    }

    if trig.starts_with("このキャラが") || trig.starts_with("このキャラの") {
        return actor_instance_id == self_instance_id;
    }

    if let Some(pos) = trig.find("自分の現場にいる") {
        let rest = &trig[pos + "自分の現場にいる".len()..];
        let cut = ["がアクション", "が推理", "が、"]
            .iter()
            .filter_map(|pat| rest.find(pat))
            .min()
            .unwrap_or(rest.len());
        let actor_phrase = rest[..cut].trim();
        let branches = parse_branches_from_phrase(actor_phrase, CardType::CHARACTER);
        return branches.iter().any(|b| branch_matches(actor_def, actor_ap, b));
    }

    actor_instance_id == self_instance_id
}

/// キャラ登場トリガー（`【疾風】`・味方/相手登場時）に一致するか判定する
#[allow(clippy::too_many_arguments)]
pub fn check_ally_play_match(
    clause: &str,
    owner_is_playing: bool,
    self_instance_id: &str,
    played_instance_id: &str,
    played_def: &CardDefinition,
    played_ap: i32,
    played_orientation: CardOrientation,
    played_by_effect_level: Option<i32>,
    is_first_play_this_turn: bool,
) -> bool {
    if clause.contains("【疾風】") {
        return owner_is_playing
            && is_first_play_this_turn
            && self_instance_id == played_instance_id;
    }

    let clean = strip_parens(clause);
    let body = strip_leading_tags(&clean);
    let Some(idx) = body.find("登場したとき") else {
        return false;
    };
    let trig = body[..idx]
        .trim()
        .trim_end_matches(['が', 'に', 'で'])
        .trim();

    if trig.contains("能力や効果によって") && played_by_effect_level.is_none() {
        return false;
    }

    let opp_play = trig.contains("相手の現場に");
    if opp_play {
        if owner_is_playing {
            return false;
        }
    } else if !owner_is_playing {
        return false;
    }

    if trig.contains("スリープ状態で") && played_orientation != CardOrientation::SLEEP {
        return false;
    }
    if trig.contains("このキャラ以外の") && self_instance_id == played_instance_id {
        return false;
    }

    if trig.contains("元の能力を持たないキャラ") {
        let has_other_ability = played_def
            .description
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .any(|l| !l.contains("【カットイン】") && !l.contains("【ヒラメキ】"));
        if has_other_ability {
            return false;
        }
    }

    if trig == "このキャラ" || trig == "このキャラがスリープ状態で" {
        return self_instance_id == played_instance_id;
    }
    if trig.starts_with("このキャラか") && self_instance_id == played_instance_id {
        return true;
    }

    let subj = trig
        .replace("自分の現場に", "")
        .replace("相手の現場に", "")
        .replace("このキャラ以外の", "")
        .replace("このキャラか", "")
        .replace("能力や効果によって", "");
    let subj = subj.trim();

    if subj.contains("【疾風】を持つ") && !played_def.description.contains("【疾風】") {
        return false;
    }

    let branches = parse_branches_from_phrase(subj, CardType::CHARACTER);
    branches.iter().any(|b| branch_matches(played_def, played_ap, b))
}

fn check_passive_extra_conditions(body: &str, pctx: &PassiveEvalContext) -> bool {
    let cond_part = if let Some((before, _)) = body.split_once("場合、") {
        before
    } else {
        return true;
    };

    if let Some(req_opp) = extract_number_before(cond_part, "相手の現場にキャラが", "枚以上") {
        if (pctx.opp_field_cards.len() as i32) < req_opp {
            return false;
        }
    }
    if cond_part.contains("自分の現場にこのキャラ以外のキャラがいない") {
        if pctx
            .source_cond
            .field_cards
            .iter()
            .any(|(inst, _)| inst.instance_id != pctx.source_instance_id)
        {
            return false;
        }
    }
    if let Some(max_h) = extract_number_before(cond_part, "自分の手札が", "枚以下") {
        if (pctx.self_hand_count as i32) > max_h {
            return false;
        }
    }
    if let Some(max_ev) = extract_number_before(cond_part, "自分の証拠が", "つ以下") {
        if (pctx.source_cond.self_evidence_count as i32) > max_ev {
            return false;
        }
    }
    if let Some(sum_ev) = extract_number_before(cond_part, "お互いの証拠が合わせて", "つ以上") {
        let total = pctx.source_cond.self_evidence_count + pctx.source_cond.opp_evidence_count;
        if (total as i32) < sum_ev {
            return false;
        }
    }
    if cond_part.contains("自分の事件が【青】以外の色を持つ") {
        let has_non_blue = pctx
            .source_cond
            .case_def
            .map(|c| c.colors.iter().any(|col| col != "青"))
            .unwrap_or(false);
        if !has_non_blue {
            return false;
        }
    }
    if let Some(start) = cond_part.find("自分の現場にいるすべてのキャラが") {
        let phrase = &cond_part[start + "自分の現場にいるすべてのキャラが".len()..];
        let branches = parse_branches_from_phrase(phrase, CardType::CHARACTER);
        if pctx.source_cond.field_cards.is_empty()
            || !pctx.source_cond.field_cards.iter().all(|(inst, def)| {
                branches
                    .iter()
                    .any(|b| branch_matches(def, def.ap + inst.temp_ap_bonus, b))
            })
        {
            return false;
        }
    } else if let Some(start) = cond_part.find("自分か相手の現場に") {
        let rest = &cond_part[start + "自分か相手の現場に".len()..];
        let end = rest.find("がいる").unwrap_or(rest.len());
        let branches = parse_branches_from_phrase(&rest[..end], CardType::CHARACTER);
        let in_self = pctx.source_cond.field_cards.iter().any(|(inst, def)| {
            branches
                .iter()
                .any(|b| branch_matches(def, def.ap + inst.temp_ap_bonus, b))
        });
        let in_opp = pctx.opp_field_cards.iter().any(|(inst, def)| {
            branches
                .iter()
                .any(|b| branch_matches(def, def.ap + inst.temp_ap_bonus, b))
        });
        if !in_self && !in_opp {
            return false;
        }
    } else if let Some(start) = cond_part.find("自分の現場に") {
        let rest = &cond_part[start + "自分の現場に".len()..];
        if let Some(req_cnt) = extract_number_before(rest, "が", "枚以上") {
            let exclude_self = rest.contains("このキャラ以外の");
            let phrase = rest
                .split("がいる")
                .next()
                .unwrap_or(rest)
                .replace("このキャラ以外の", "");
            let branches = parse_branches_from_phrase(&phrase, CardType::CHARACTER);
            let cnt = pctx
                .source_cond
                .field_cards
                .iter()
                .filter(|(inst, def)| {
                    if exclude_self && inst.instance_id == pctx.source_instance_id {
                        return false;
                    }
                    branches
                        .iter()
                        .any(|b| branch_matches(def, def.ap + inst.temp_ap_bonus, b))
                })
                .count();
            if (cnt as i32) < req_cnt {
                return false;
            }
        }
    }

    true
}

fn compute_passive_multiplier(body: &str, pctx: &PassiveEvalContext) -> Option<i32> {
    let Some((scale_part, _)) = body.split_once("につき、") else {
        return Some(1);
    };
    if scale_part.contains("自分の表向きの証拠1つ") {
        return Some(pctx.self_faceup_evidence_count as i32);
    }
    if scale_part.contains("相手の現場にいるキャラ1枚") {
        return Some(pctx.opp_field_cards.len() as i32);
    }
    if let Some(start) = scale_part.find("自分の現場にいる") {
        let rest = &scale_part[start + "自分の現場にいる".len()..];
        let exclude_self = rest.contains("このキャラ以外の");
        let phrase = rest.replace("このキャラ以外の", "").replace("1枚", "");
        let branches = parse_branches_from_phrase(&phrase, CardType::CHARACTER);
        let cnt = pctx
            .source_cond
            .field_cards
            .iter()
            .filter(|(inst, def)| {
                if exclude_self && inst.instance_id == pctx.source_instance_id {
                    return false;
                }
                branches
                    .iter()
                    .any(|b| branch_matches(def, def.ap + inst.temp_ap_bonus, b))
            })
            .count();
        return Some(cnt as i32);
    }
    None
}

fn extract_passive_effect_part(body: &str) -> &str {
    if let Some((_, after)) = body.split_once("場合、") {
        after
    } else if let Some((_, after)) = body.split_once("につき、") {
        after
    } else {
        body
    }
}

/// パッシブ（自己バフ・味方/相手オーラ）のAP補正を対象カードごとに計算する
pub fn compute_passive_ap_for_target(clause: &str, pctx: &PassiveEvalContext) -> i32 {
    if pctx.source_in_partner_area && !allows_partner_area(clause) {
        return 0;
    }
    if clause.contains("カードがセットされている")
        || clause.contains("セットされているカード")
        || clause.contains("下に重なっているカード")
    {
        return 0;
    }
    let clean = strip_parens(clause);
    if clean.contains("元の能力を無効") || clean.contains("選ばれない") || clean.contains("ミスリード") {
        return 0;
    }

    let delta = if let Some(b) = parse_signed_ap(&clean, true) {
        b
    } else if let Some(d) = parse_signed_ap(&clean, false) {
        -d
    } else {
        return 0;
    };

    if check_clause_conditions(clause, pctx.source_cond).is_err() {
        return 0;
    }
    let body = strip_leading_tags(&clean);
    if !check_passive_extra_conditions(body, pctx) {
        return 0;
    }
    let Some(mult) = compute_passive_multiplier(body, pctx) else {
        return 0;
    };
    if mult == 0 {
        return 0;
    }

    let eff = extract_passive_effect_part(body);
    if eff.contains("相手の現場にいる") {
        if pctx.target_is_friendly_to_source {
            return 0;
        }
        let s = eff.find("相手の現場にいる").unwrap() + "相手の現場にいる".len();
        let end = eff[s..].find("をAP").map(|i| s + i).unwrap_or(eff.len());
        let phrase = eff[s..end].trim();
        if phrase != "キャラ" && phrase != "すべてのキャラ" {
            let branches = parse_branches_from_phrase(phrase, CardType::CHARACTER);
            if !branches
                .iter()
                .any(|b| branch_matches(pctx.target_def, pctx.target_base_ap, b))
            {
                return 0;
            }
        }
        return delta * mult;
    }

    if let Some(idx) = eff.find("自分の現場にいる") {
        if !pctx.target_is_friendly_to_source {
            return 0;
        }
        let s = idx + "自分の現場にいる".len();
        let end = eff[s..].find("をAP").map(|i| s + i).unwrap_or(eff.len());
        let raw_phrase = &eff[s..end];
        if raw_phrase.contains("このキャラ以外の")
            && pctx.target_instance_id == pctx.source_instance_id
        {
            return 0;
        }
        let phrase = raw_phrase.replace("このキャラ以外の", "").trim().to_string();
        if phrase != "キャラ" && phrase != "すべてのキャラ" && !phrase.is_empty() {
            let branches = parse_branches_from_phrase(&phrase, CardType::CHARACTER);
            if !branches
                .iter()
                .any(|b| branch_matches(pctx.target_def, pctx.target_base_ap, b))
            {
                return 0;
            }
        }
        return delta * mult;
    }

    // 自己限定パッシブ（例: `AP＋1000`, `このキャラをAP＋2000する`）
    if !pctx.target_is_friendly_to_source || pctx.target_instance_id != pctx.source_instance_id {
        return 0;
    }
    delta * mult
}

/// パッシブ（自己）のLP補正を計算する
pub fn compute_passive_lp_bonus(clause: &str, pctx: &PassiveEvalContext) -> i32 {
    if pctx.source_in_partner_area && !allows_partner_area(clause) {
        return 0;
    }
    if pctx.target_instance_id != pctx.source_instance_id || !pctx.target_is_friendly_to_source {
        return 0;
    }
    let clean = strip_parens(clause);
    if clean.contains("元の能力を無効") || clean.contains("ミスリード") {
        return 0;
    }
    let delta = if let Some(b) = parse_signed_lp(&clean, true) {
        b
    } else if let Some(d) = parse_signed_lp(&clean, false) {
        -d
    } else {
        return 0;
    };

    if check_clause_conditions(clause, pctx.source_cond).is_err() {
        return 0;
    }
    let body = strip_leading_tags(&clean);
    if !check_passive_extra_conditions(body, pctx) {
        return 0;
    }
    delta
}

/// パッシブ（常時/自分ターン中/相手ターン中）のAP補正を計算する（自己バフ専用）
#[allow(dead_code)]
pub fn compute_passive_ap_bonus(clause: &str, ctx: &PlayerCondInfo) -> i32 {
    if check_clause_conditions(clause, ctx).is_err() {
        return 0;
    }
    let clean = strip_parens(clause);
    if clean.contains("このキャラ以外の") || clean.contains("相手の現場にいる") {
        return 0;
    }
    if let Some(buff) = parse_signed_ap(&clean, true) {
        return buff;
    }
    0
}

// ─── Small String Parsing Helpers (No Regex Dependency) ──────────────────────

pub fn extract_count_around(text: &str, before: &str, after: &str) -> usize {
    extract_number_before(text, before, after)
        .filter(|&n| n > 0)
        .map(|n| n as usize)
        .unwrap_or(0)
}

pub fn extract_number_before(text: &str, prefix: &str, suffix: &str) -> Option<i32> {
    let mut search = 0;
    while let Some(pos) = text[search..].find(prefix) {
        let abs = search + pos + prefix.len();
        let tail = &text[abs..];
        let digits: String = tail
            .chars()
            .take_while(|c| c.is_ascii_digit() || matches!(c, '０'..='９'))
            .map(normalize_digit)
            .collect();
        if !digits.is_empty() {
            let num_bytes: usize = tail
                .chars()
                .take(digits.len())
                .map(|c| c.len_utf8())
                .sum();
            let after_num = &tail[num_bytes..];
            if suffix.is_empty() || after_num.starts_with(suffix) {
                if let Ok(val) = digits.parse::<i32>() {
                    return Some(val);
                }
            }
        }
        search = abs;
    }
    None
}

fn normalize_digit(ch: char) -> char {
    match ch {
        '０'..='９' => char::from_u32(('0' as u32) + (ch as u32 - '０' as u32)).unwrap_or(ch),
        _ => ch,
    }
}

pub fn parse_signed_ap(text: &str, positive: bool) -> Option<i32> {
    let markers: &[&str] = if positive {
        &["AP＋", "AP+"]
    } else {
        &["AP－", "AP-", "APー"]
    };
    for m in markers {
        if let Some(val) = extract_number_before(text, m, "") {
            return Some(if positive { val } else { val });
        }
    }
    None
}

pub fn parse_signed_lp(text: &str, positive: bool) -> Option<i32> {
    let markers: &[&str] = if positive {
        &["LP＋", "LP+"]
    } else {
        &["LP－", "LP-", "LPー"]
    };
    for m in markers {
        if let Some(val) = extract_number_before(text, m, "") {
            return Some(val);
        }
    }
    None
}

pub fn extract_bracketed_value<'a>(text: &'a str, start_pat: &str, end_pat: &str) -> Option<&'a str> {
    let idx = text.find(start_pat)?;
    let rest = &text[idx + start_pat.len()..];
    let end = rest.find(end_pat)?;
    Some(&rest[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_def(id: &str, name: &str, traits: &[&str], colors: &[&str], level: i32, ap: i32, desc: &str) -> CardDefinition {
        CardDefinition {
            definition_id: id.to_string(),
            canonical_id: id.to_string(),
            card_type: CardType::CHARACTER,
            colors: colors.iter().map(|s| s.to_string()).collect(),
            level,
            ap,
            lp: 1,
            names: vec![name.to_string()],
            description: desc.to_string(),
            traits: traits.iter().map(|s| s.to_string()).collect(),
            case_levels: None,
            main_thumb: None,
            main_path: None,
        }
    }

    #[test]
    fn test_clause_isolation_b04005() {
        let desc = "【登場時】カードを2枚引き、手札を2枚リムーブする。\n【宣言】【ターン1】このキャラ以外の〚特徴［毛利探偵事務所］〛のキャラを1枚まで選び、ターン終了時までAP＋1000する。";
        let on_play = extract_clauses(desc, TriggerKind::OnPlay, CardType::CHARACTER);
        assert_eq!(on_play.len(), 1);
        let plan = parse_effect_steps(&on_play[0], None, CardType::CHARACTER);
        assert_eq!(
            plan.immediate_before,
            vec![ImmediateOp::Draw(2), ImmediateOp::DiscardHand(2)]
        );
        assert!(plan.target_step.is_none());

        let decl = extract_clauses(desc, TriggerKind::Declare, CardType::CHARACTER);
        assert_eq!(decl.len(), 1);
        let decl_plan = parse_effect_steps(&decl[0], None, CardType::CHARACTER);
        assert!(decl_plan.immediate_before.is_empty());
        let ts = decl_plan.target_step.unwrap();
        assert_eq!(ts.action, TargetAction::ApBuff(1000));
        assert!(ts.filter.exclude_self);
        assert_eq!(ts.filter.branches[0].trait_req.as_deref(), Some("毛利探偵事務所"));
    }

    #[test]
    fn test_b05006_state_branching_and_followup() {
        let desc = "【パートナー青】【宣言】【ターン1】このキャラがアクティブ状態の場合、ターン終了時までこのキャラは〚突撃〛（登場したターンからすぐにアクションできる）を持つ。スリープ状態かスタン状態の場合、AP8000以下のキャラを1枚まで選び、リムーブし、カードを1枚引く。この能力は自分の現場に〚カード名［妃英理］〛かこのキャラ以外の〚特徴［毛利探偵事務所］〛のキャラがいる場合に宣言できる。";
        let active_branch = resolve_state_branch(desc, CardOrientation::ACTIVE);
        let active_plan = parse_effect_steps(&active_branch, None, CardType::CHARACTER);
        assert_eq!(active_plan.immediate_before, vec![ImmediateOp::SelfGiveRush(None)]);
        assert!(active_plan.target_step.is_none());

        let sleep_branch = resolve_state_branch(desc, CardOrientation::SLEEP);
        let sleep_plan = parse_effect_steps(&sleep_branch, None, CardType::CHARACTER);
        let ts = sleep_plan.target_step.unwrap();
        assert_eq!(ts.action, TargetAction::Remove);
        assert_eq!(ts.filter.owner, OwnerScope::OppOnly);
        assert_eq!(ts.filter.branches[0].max_ap, Some(8000));
        assert_eq!(ts.follow_up, vec![ImmediateOp::Draw(1)]);
    }

    #[test]
    fn test_or_branches_filter() {
        let f = parse_target_filter(
            "自分のリムーブエリアにあるレベル5以下の〚カード名［妃英理］〛かレベル5以下の〚特徴［毛利探偵事務所］〛のキャラ",
            TargetZone::Remove,
            true,
            None,
        );
        assert_eq!(f.owner, OwnerScope::SelfOnly);
        assert_eq!(f.zone, TargetZone::Remove);
        assert_eq!(f.branches.len(), 2);

        let eri = make_def("C1", "妃英理", &["弁護士"], &["青"], 5, 5000, "");
        let ran = make_def("C2", "毛利蘭", &["毛利探偵事務所"], &["青"], 4, 4000, "");
        let gin = make_def("C3", "ジン", &["黒ずくめの組織"], &["黒"], 5, 6000, "");
        let cogoro_hi = make_def("C4", "毛利小五郎", &["毛利探偵事務所"], &["青"], 7, 7000, "");

        assert!(f.branches.iter().any(|b| branch_matches(&eri, eri.ap, b)));
        assert!(f.branches.iter().any(|b| branch_matches(&ran, ran.ap, b)));
        assert!(!f.branches.iter().any(|b| branch_matches(&gin, gin.ap, b)));
        assert!(!f.branches.iter().any(|b| branch_matches(&cogoro_hi, cogoro_hi.ap, b)));
    }

    #[test]
    fn test_full_catalog_effect_audit() {
        let path = if std::path::Path::new("data/dummy/cards.json").exists() {
            "data/dummy/cards.json"
        } else {
            "../data/dummy/cards.json"
        };
        let catalog = CardCatalog::load_from_json(path).expect("catalog load failed");

        let mut seen_canon = std::collections::HashSet::new();
        let mut unparsed_selects = Vec::new();
        let mut lost_prefixes = Vec::new();

        for def in catalog.definitions.values() {
            if !seen_canon.insert(def.canonical_id.clone()) {
                continue;
            }
            let clean = strip_parens(&def.description);
            for line in clean.lines().map(str::trim).filter(|l| !l.is_empty()) {
                let after_colon = line.split_once('：').map(|x| x.1).unwrap_or_else(|| strip_leading_tags(line));
                for sent in after_colon.split('。').map(str::trim).filter(|s| !s.is_empty()) {
                    if let Some((tp, ap)) = sent.split_once("選び、") {
                        if parse_target_action(ap).is_none()
                            && !sent.contains("このキャラの下に重ねる")
                            && !sent.contains("コストによってスタンさせたキャラに")
                        {
                            unparsed_selects.push(format!("{} ({}): {}", def.definition_id, def.names.first().unwrap_or(&def.definition_id), sent));
                        }
                        if tp.contains("引き、") || tp.contains("リムーブし、") {
                            lost_prefixes.push(format!("{} ({}): {}", def.definition_id, def.names.first().unwrap_or(&def.definition_id), sent));
                        }
                    }
                }
            }
        }

        println!("🔍 カタログ自動監査: 未対応の『選び、』アクション = {}件, 複合前置文 = {}件", unparsed_selects.len(), lost_prefixes.len());
        for item in &unparsed_selects {
            println!("  [未対応アクション] {}", item);
        }
        for item in &lost_prefixes {
            println!("  [複合前置文] {}", item);
        }
    }
}
