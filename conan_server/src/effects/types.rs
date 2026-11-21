use super::*;

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
    OnCharRemoved,
    OnTurnEnd,
    OnAction,
    OnEvidenceGain,
    OnContact,
    OnContactWin,
    OnHintUse,
    OnAllyPlay,
    OnCardSet,
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
    GiveMustGuard,
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
    LpBuff(i32),
    SetBaseLpZero,
    MoveToPartnerArea,
    MoveFaceDownSetToTarget,
    SetCardOnTargetChar,
    SetCardOnSelf,
    StackRemoveUnderAllyBlue,
    TransferStackedUnderTarget(usize),
    StackSelfUnderTarget,
    NegateBaseAbilities,
    CopyNameFromRevealed,
    TriggerTargetHirameki,
    GrantBondAsAny,
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
            TargetAction::GiveMustGuard => "GIVE_MUST_GUARD",
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
            TargetAction::LpBuff(_) => "LP_BUFF",
            TargetAction::SetBaseLpZero => "SET_BASE_LP_ZERO",
            TargetAction::MoveToPartnerArea => "MOVE_TO_PARTNER_AREA",
            TargetAction::MoveFaceDownSetToTarget => "MOVE_FACE_DOWN_SET_TO_TARGET",
            TargetAction::SetCardOnTargetChar => "SET_CARD_ON_TARGET_CHAR",
            TargetAction::SetCardOnSelf => "SET_CARD_ON_SELF",
            TargetAction::StackRemoveUnderAllyBlue => "STACK_REMOVE_UNDER_ALLY_BLUE",
            TargetAction::TransferStackedUnderTarget(_) => "TRANSFER_STACKED_UNDER_TARGET",
            TargetAction::StackSelfUnderTarget => "STACK_SELF_UNDER_TARGET",
            TargetAction::NegateBaseAbilities => "NEGATE_BASE_ABILITIES",
            TargetAction::CopyNameFromRevealed => "COPY_NAME_FROM_REVEALED",
            TargetAction::TriggerTargetHirameki => "TRIGGER_TARGET_HIRAMEKI",
            TargetAction::GrantBondAsAny => "GRANT_BOND_AS_ANY",
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
    #[serde(default)]
    pub max_ap_from_actor: bool,
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
    pub field_sleep: usize,
    pub field_sleep_filter: Option<TargetFilter>,
    pub evidence_remove: usize,
    pub evidence_flip_up: usize,
    pub evidence_flip_up_any: bool,
    pub file_remove: usize,
    pub deck_mill: usize,
    pub sacrifice_self: bool,
    pub self_to_deck_bottom: bool,
    pub self_to_partner_area: bool,
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
    SelfLpBuff(i32),
    SetSelfFieldBaseLp {
        from_lp: i32,
        level: i32,
        trait_name: String,
        to_lp: i32,
    },
    SelfLevelDelta(i32),
    SelfActivate,
    SelfGiveRush(Option<String>),
    SetDeckTopFaceDownOnSelf(usize),
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
    PlaySelfFromHand { sleep: bool },
    BanNameThisTurn(String),
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

pub fn default_one_usize() -> usize {
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
#[derive(Clone)]
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
    pub self_ap: i32,
    pub self_contact_removed_opp_this_turn: bool,
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

