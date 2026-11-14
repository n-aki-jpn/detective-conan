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
    Passive,
}

/// ターゲット所有者のスコープ
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerScope {
    SelfOnly,
    OppOnly,
    Both,
}

/// ターゲット探索対象ゾーン
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetZone {
    Field,
    Remove,
    Hand,
    Deck,
}

/// ターゲットに対して実行するアクション
#[derive(Debug, Clone, PartialEq, Eq)]
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
        )
    }
}

/// デッキ公開後の処理種別
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    pub min_level: Option<i32>,
    pub max_ap: Option<i32>,
    pub min_ap: Option<i32>,
    pub exact_lp: Option<i32>,
    pub max_lp: Option<i32>,
    pub min_lp: Option<i32>,
}

/// 統一ターゲットフィルタ
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetFilter {
    pub owner: OwnerScope,
    pub zone: TargetZone,
    pub exclude_self: bool,
    pub acting_only: bool,
    pub orientations: Vec<CardOrientation>,
    pub count: usize,
    pub branches: Vec<TargetBranch>,
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
        }
    }
}

/// コスト仕様（：の左側、または明示的な事前コスト）
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClauseCost {
    pub requires_sleep: bool,
    pub hand_remove: usize,
    pub evidence_remove: usize,
    pub evidence_flip_up: usize,
    pub file_remove: usize,
    pub deck_mill: usize,
    pub sacrifice_self: bool,
}

/// 即時処理オペレーション
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImmediateOp {
    Draw(usize),
    DiscardHand(usize),
    GainEvidence(usize),
    FlipEvidenceUp(usize),
    FlipEvidenceDown(usize),
    OppRemoveEvidence(usize),
    SelfRemoveEvidence { count: usize, trigger_hirameki: bool },
    SelfApBuff(i32),
    SelfActivate,
    SelfGiveRush(Option<String>),
    MillDeck(usize),
    MoveSelfFromEvidenceToFieldSleep,
    SolveCaseVictory,
}

/// 対象選択ステップ
#[derive(Debug, Clone)]
pub struct TargetStep {
    pub action: TargetAction,
    pub filter: TargetFilter,
    pub follow_up: Vec<ImmediateOp>,
    pub conditional_follow_up: Option<(TargetBranch, Vec<ImmediateOp>)>,
}

/// デッキサーチステップ
#[derive(Debug, Clone)]
pub struct DeckSearchStep {
    pub search_count: usize,
    pub pick_action: DeckPickAction,
    pub filter: TargetFilter,
    pub post_add_discard: usize,
    pub post_ops: Vec<ImmediateOp>,
}

/// 1つの能力句（clause）の解析結果
#[derive(Debug, Clone, Default)]
pub struct ParsedClausePlan {
    pub immediate_before: Vec<ImmediateOp>,
    pub target_step: Option<TargetStep>,
    pub deck_search: Option<DeckSearchStep>,
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
    pub opp_field_count: usize,
    pub case_level: i32,
    pub partner_def: Option<&'a CardDefinition>,
    pub case_def: Option<&'a CardDefinition>,
    pub field_cards: Vec<(&'a CardInstance, &'a CardDefinition)>,
    pub played_by_effect_level: Option<i32>,
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
                // イベント使用時：ヒラメキ・セット中効果・現場リムーブ時などは除外
                !line.contains("【ヒラメキ】")
                    && !line.contains("【現場リムーブ時】")
                    && !clean.starts_with("このイベントがセットされている")
                    && !clean.contains("がアクションしたとき")
            } else {
                line.contains("【登場時】")
            }
        }
        TriggerKind::Declare => line.contains("【宣言】"),
        TriggerKind::Cutin => line.contains("【カットイン】"),
        TriggerKind::Hirameki => line.contains("【ヒラメキ】"),
        TriggerKind::DisguiseCond => line.contains("【変装】"),
        TriggerKind::OnDisguise => line.contains("【変装時】"),
        TriggerKind::OnRemove => line.contains("【現場リムーブ時】"),
        TriggerKind::OnTurnEnd => {
            // 「ターン終了時まで〜」という継続期間表記はトリガーではない
            clean.contains("ターン終了時、")
                || clean.contains("自分のターン終了時")
                || clean.contains("相手のターン終了時")
        }
        TriggerKind::OnAction => {
            clean.contains("アクションしたとき") || clean.contains("推理かアクションしたとき")
        }
        TriggerKind::OnContact => {
            clean.contains("コンタクトしたとき") || clean.contains("コンタクトされたとき")
        }
        TriggerKind::OnContactWin => {
            clean.contains("コンタクトで勝利したとき")
                || clean.contains("コンタクトによってリムーブされたとき")
        }
        TriggerKind::OnHintUse => clean.contains("ネクストヒントで手札を使用したとき"),
        TriggerKind::Passive => {
            !line.contains("【登場時】")
                && !line.contains("【宣言】")
                && !line.contains("【カットイン】")
                && !line.contains("【ヒラメキ】")
                && !line.contains("【変装時】")
                && !line.contains("【現場リムーブ時】")
                && !clean.contains("アクションしたとき")
                && !clean.contains("コンタクトしたとき")
                && !clean.contains("コンタクトされたとき")
                && !clean.contains("ターン終了時、")
                && !clean.contains("自分のターン終了時")
                && !clean.contains("相手のターン終了時")
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
fn strip_leading_tags(text: &str) -> &str {
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

fn has_trait_match(def: &CardDefinition, query: &str) -> bool {
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

/// 単一能力句の発動条件チェック
pub fn check_clause_conditions(clause: &str, ctx: &PlayerCondInfo) -> Result<(), String> {
    let tags = extract_bracket_tags(clause);
    let clean = strip_parens(clause);

    for tag in &tags {
        if *tag == "解決編" && ctx.case_phase != CasePhase::RESOLUTION {
            return Err("事件が解決編になっていません".to_string());
        }
        if *tag == "自分ターン中" && !ctx.is_my_turn {
            return Err("自分のターン中ではありません".to_string());
        }
        if *tag == "相手ターン中" && ctx.is_my_turn {
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

        if let Some(case_req) = tag.strip_prefix("事件") {
            if *tag != "事件解決" && *tag != "事件編" {
                check_case_tag(case_req, ctx.case_def)?;
            }
        }

        if let Some(bond_name) = tag.strip_prefix("絆") {
            let has_bond = check_bond(bond_name, ctx);
            if !has_bond {
                return Err(format!("絆条件未達: 【{}】", tag));
            }
        }
    }

    if clean.contains("自分の現場に") {
        if let Some(req_slice) = extract_field_presence_condition(&clean) {
            let exclude_self = req_slice.contains("このキャラ以外の");
            let branches = parse_branches_from_phrase(req_slice, CardType::CHARACTER);
            let satisfied = ctx.field_cards.iter().any(|(inst, def)| {
                if exclude_self && ctx.actor_id == Some(inst.instance_id.as_str()) {
                    return false;
                }
                branches
                    .iter()
                    .any(|b| branch_matches(def, def.ap + inst.temp_ap_bonus, b))
            });
            if !satisfied {
                return Err("自分の現場の条件キャラが不足しています".to_string());
            }
        }
    }

    if clean.contains("相手の現場にキャラがいない場合") && ctx.opp_field_count > 0 {
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
    let req_colors: Vec<&str> = COLORS.iter().copied().filter(|c| case_req.contains(c)).collect();
    if !req_colors.is_empty() {
        let ok = if case_req.contains("or") || case_req.contains("か") {
            req_colors.iter().any(|c| cdef.colors.iter().any(|cc| cc == c))
        } else {
            req_colors.iter().all(|c| cdef.colors.iter().any(|cc| cc == c))
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
        .any(|(inst, def)| ctx.actor_id != Some(inst.instance_id.as_str()) && has_name_match(def, bond_name))
}

fn extract_field_presence_condition(clean: &str) -> Option<&str> {
    let start = clean.find("自分の現場に")? + "自分の現場に".len();
    let tail = &clean[start..];
    let end = tail
        .find("がいる場合")
        .or_else(|| tail.find("がいるなら"))
        .or_else(|| tail.find("が登場した"))
        ?;
    Some(&tail[..end])
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
        cost.hand_remove = extract_count_around(prefix, "手札を", "枚リムーブ");
        cost.evidence_remove = extract_count_around(prefix, "証拠を", "つリムーブ");
        cost.evidence_flip_up = extract_count_around(prefix, "裏向きの証拠を", "つ表向き");
        cost.file_remove = extract_count_around(prefix, "FILEを", "枚リムーブ")
            .max(extract_count_around(prefix, "FILEエリアのカードを", "枚リムーブ"));
        cost.deck_mill = extract_count_around(prefix, "デッキのカードを上から", "枚リムーブ");
        cost.sacrifice_self = prefix.contains("このキャラをリムーブする");
    } else if let Some(opt_idx) = clean.find("てもよい。そうした場合、") {
        let opt_prefix = &clean[..opt_idx];
        let seg = opt_prefix.rsplit('。').next().unwrap_or(opt_prefix);
        cost.hand_remove = extract_count_around(seg, "手札を", "枚リムーブ");
        cost.evidence_remove = extract_count_around(seg, "証拠を", "つリムーブ");
        cost.evidence_flip_up = extract_count_around(seg, "裏向きの証拠を", "つ表向き");
        cost.file_remove = extract_count_around(seg, "FILEを", "枚リムーブ");
    }

    cost
}

/// 能力句の解決プラン（事前処理・対象選択・デッキサーチ・事後効果）を解析する
pub fn parse_effect_steps(
    clause: &str,
    dynamic_level: Option<i32>,
    source_card_type: CardType,
) -> ParsedClausePlan {
    let mut plan = ParsedClausePlan::default();
    let clean = strip_parens(clause);

    // `：` がある場合、コスト側を除いた右辺だけを効果本体とする
    let after_colon = if let Some(idx) = clean.find('：') {
        &clean[idx + '：'.len_utf8()..]
    } else {
        strip_leading_tags(&clean)
    };

    // 「〜してもよい。そうした場合、〜」の前提コスト部をスキップ
    let effect_body = if let Some(idx) = after_colon.find("てもよい。そうした場合、") {
        &after_colon[idx + "てもよい。そうした場合、".len()..]
    } else {
        after_colon
    };

    let sentences: Vec<&str> = effect_body
        .split('。')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    let mut seen_target_or_search = false;
    for sentence in sentences {
        if sentence.starts_with("この能力は")
            || sentence.starts_with("この効果は")
            || sentence.starts_with("残りを")
        {
            continue;
        }

        // 0) 特殊：自分の証拠から場に出るヒラメキ
        if sentence == "このキャラをスリープ状態で登場させる" {
            plan.immediate_before
                .push(ImmediateOp::MoveSelfFromEvidenceToFieldSleep);
            continue;
        }
        // 事件解決勝利
        if sentence.contains("ゲームに勝利する") {
            plan.immediate_before.push(ImmediateOp::SolveCaseVictory);
            continue;
        }

        // 1) デッキサーチ（「自分のデッキのカードを上からN枚見る...」を含む）
        if parse_deck_look_count(effect_body).is_some()
            && (sentence.contains("デッキのカードを上から")
                || sentence.contains("その中から")
                || sentence.contains("カードを手札に加えた場合、手札を"))
        {
            if plan.deck_search.is_none() {
                if let Some(search_n) = parse_deck_look_count(effect_body) {
                    let rest_from_look = effect_body
                        .find("デッキのカードを上から")
                        .map(|i| &effect_body[i..])
                        .unwrap_or(effect_body);
                    plan.deck_search =
                        Some(parse_deck_search_step(search_n, rest_from_look, dynamic_level));
                    seen_target_or_search = true;
                }
            }
            continue;
        }

        // 2) 条件付き追撃文（例：「〚カード名［江戸川コナン］〛を登場させた場合、カードを1枚引く」）
        if seen_target_or_search {
            if let Some(tstep) = plan.target_step.as_mut() {
                if let Some((cond_branch, ops)) = parse_conditional_follow_up(sentence) {
                    tstep.conditional_follow_up = Some((cond_branch, ops));
                    continue;
                }
            }
        }

        // 3) 対象選択を伴う文（「〜選び、...」または「手札から〜登場させる」）
        if !seen_target_or_search && is_target_selection_sentence(sentence) {
            if let Some(tstep) = parse_target_step(sentence, dynamic_level, source_card_type) {
                plan.target_step = Some(tstep);
                seen_target_or_search = true;
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
        if seen_target_or_search {
            if let Some(ds) = plan.deck_search.as_mut() {
                ds.post_ops.extend(ops);
            } else if let Some(tstep) = plan.target_step.as_mut() {
                tstep.follow_up.extend(ops);
            }
        } else {
            plan.immediate_before.extend(ops);
        }
    }

    // fallback: 1文に「〜を見る。その中から〜」がまとまっていたが上で捕捉済み以外の場合
    if plan.deck_search.is_none() && effect_body.contains("デッキのカードを上から") {
        if let Some(search_n) = parse_deck_look_count(effect_body) {
            plan.deck_search = Some(parse_deck_search_step(search_n, effect_body, dynamic_level));
        }
    }

    plan
}

fn is_target_selection_sentence(s: &str) -> bool {
    if s.contains("選び") {
        return true;
    }
    if s.contains("手札から") && s.contains("登場させ") {
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
    filter.count = count.max(1);

    let post_add_discard = if text.contains("手札に加えた場合、手札を") {
        extract_count_around(text, "手札を", "枚リムーブ")
    } else {
        0
    };

    DeckSearchStep {
        search_count,
        pick_action,
        filter,
        post_add_discard,
        post_ops: Vec::new(),
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
    // 選択対象フレーズとアクションフレーズに分割
    let (target_phrase, action_phrase) = if let Some(idx) = sentence.find("選び、") {
        (&sentence[..idx], &sentence[idx + "選び、".len()..])
    } else if let Some(idx) = sentence.find("選び") {
        (&sentence[..idx], &sentence[idx + "選び".len()..])
    } else if let Some(idx) = sentence.find("手札から") {
        let rest = &sentence[idx..];
        if let Some(didx) = rest.find("登場させ") {
            (&rest[..didx], &rest[didx..])
        } else {
            return None;
        }
    } else {
        return None;
    };

    // action_phrase を先頭のアクション節と後続効果（「〜し、カードを1枚引く」等）に分離
    let (primary_action_text, trailing_text) = split_action_and_follow_up(action_phrase);
    let mut action = parse_target_action(primary_action_text)?;

    let zone = if target_phrase.contains("リムーブエリア") {
        TargetZone::Remove
    } else if target_phrase.contains("手札から") || target_phrase.contains("手札にある") {
        TargetZone::Hand
    } else {
        TargetZone::Field
    };
    if zone == TargetZone::Hand && matches!(action, TargetAction::ReviveActive | TargetAction::ReviveSleep) {
        action = TargetAction::PlayFromHand;
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

    let follow_up = if let Some(t) = trailing_text {
        parse_immediate_ops(t)
    } else {
        Vec::new()
    };

    Some(TargetStep {
        action,
        filter,
        follow_up,
        conditional_follow_up: None,
    })
}

fn split_action_and_follow_up(action_phrase: &str) -> (&str, Option<&str>) {
    // 例: "リムーブし、カードを1枚引く" / "スリープさせ、AP-1000する"
    for sep in ["リムーブし、", "スリープさせ、", "スタンさせ、", "アクティブにし、", "デッキの下に移し、", "登場させ、", "手札に加え、"] {
        if let Some(idx) = action_phrase.find(sep) {
            let cut = idx + sep.len() - "、".len();
            return (&action_phrase[..cut], Some(&action_phrase[idx + sep.len()..]));
        }
    }
    if let Some(idx) = action_phrase.find("する。") {
        let cut = idx + "する".len();
        return (&action_phrase[..cut], Some(&action_phrase[idx + "する。".len()..]));
    }
    (action_phrase, None)
}

fn parse_target_action(text: &str) -> Option<TargetAction> {
    if text.contains("手札に加える") || text.contains("手札に加え") {
        return Some(TargetAction::RecoverToHand);
    }
    if text.contains("スリープ状態で登場") {
        return Some(TargetAction::ReviveSleep);
    }
    if text.contains("登場させ") {
        // 手札から vs リムーブから は呼び出し側ゾーンで判定してもよいが、zone=Hand のとき専用アクションを返すこともできる
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
    if text.contains("〚突撃［事件］〛") {
        return Some(TargetAction::GiveRush(Some("CASE".to_string())));
    }
    if text.contains("〚突撃［キャラ］〛") {
        return Some(TargetAction::GiveRush(Some("CHAR".to_string())));
    }
    if text.contains("〚突撃〛") || text.contains("〚迅速〛") {
        return Some(TargetAction::GiveRush(None));
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
    let phrase = isolate_target_noun_phrase(raw_phrase);

    let owner = if zone == TargetZone::Hand || zone == TargetZone::Deck {
        OwnerScope::SelfOnly
    } else if phrase.contains("自分か相手の") || phrase.contains("どちらかの") {
        OwnerScope::Both
    } else if phrase.contains("相手の") {
        OwnerScope::OppOnly
    } else if phrase.contains("自分の") || zone == TargetZone::Remove {
        // リムーブエリアは「相手の」がない限り自分
        if phrase.contains("相手の") {
            OwnerScope::OppOnly
        } else {
            OwnerScope::SelfOnly
        }
    } else if beneficial {
        OwnerScope::SelfOnly
    } else {
        OwnerScope::OppOnly
    };

    let exclude_self = phrase.contains("このキャラ以外の");
    let acting_only = phrase.contains("アクション中の");

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

    let count = extract_count_around(phrase, "を", "枚")
        .max(1);

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
    let global_keyword = if phrase.contains("【カットイン】を持つ") {
        Some("【カットイン】".to_string())
    } else if phrase.contains("【ヒラメキ】を持つ") {
        Some("【ヒラメキ】".to_string())
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
    for (idx, part) in or_parts.iter().enumerate() {
        let mut b = parse_single_branch(part, trailing_types.clone(), global_keyword.clone());
        // 先頭にだけ書かれた「レベル5以下の」や末尾の「【青】の」を他のブランチにも継承
        if idx > 0 && b.max_level.is_none() {
            b.max_level = branches.first().and_then(|f: &TargetBranch| f.max_level);
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
    part: &str,
    card_types: Vec<CardType>,
    require_keyword: Option<String>,
) -> TargetBranch {
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
    b.min_level = extract_number_before(part, "レベル", "以上")
        .or_else(|| extract_number_before(part, "コスト", "以上"));
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
    // 1) 所有者チェック
    match filter.owner {
        OwnerScope::SelfOnly if card_owner_id != owner_player_id => return false,
        OwnerScope::OppOnly if card_owner_id == owner_player_id => return false,
        OwnerScope::Both => {}
        _ => {}
    }

    // 2) このキャラ以外チェック
    if filter.exclude_self && actor_id == Some(card.instance_id.as_str()) {
        return false;
    }

    // 3) アクション中のキャラ限定チェック
    if filter.acting_only {
        if let Some(aid) = acting_card_id {
            if card.instance_id != aid {
                return false;
            }
        }
    }

    // 4) 状態（アクティブ / スリープ / スタン）チェック
    if !filter.orientations.is_empty() && !filter.orientations.contains(&card.orientation) {
        return false;
    }

    let Some(def) = catalog.get(&card.definition_id) else {
        return false;
    };

    // 5) 相手の効果で選ばれない（「【絆〇〇】相手の能力や効果によって選ばれない。」）チェック
    if card_owner_id != owner_player_id {
        if let Some(opp_ctx) = opp_cond_info {
            if card_is_untargetable_by_opponent(def, opp_ctx) {
                return false;
            }
        }
    }

    // 6) ブランチ（特徴 / カード名 / 色 / レベル / AP / LP / カードタイプ）のいずれかに一致するか
    filter
        .branches
        .iter()
        .any(|b| branch_matches(def, effective_ap, b))
}

/// `CardDefinition` + 有効APが 1 つの `TargetBranch` に一致するか判定
pub fn branch_matches(def: &CardDefinition, effective_ap: i32, b: &TargetBranch) -> bool {
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

        let discard = extract_count_around(p, "手札を", "枚リムーブ");
        if discard > 0 {
            ops.push(ImmediateOp::DiscardHand(discard));
        }

        if p.contains("相手の証拠") && p.contains("リムーブ") {
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
        } else if p.contains("自分の裏向きの証拠") && p.contains("表向き") {
            let n = extract_count_around(p, "証拠を", "つ").max(1);
            ops.push(ImmediateOp::FlipEvidenceUp(n));
        } else if p.contains("自分の表向きの証拠") && p.contains("裏向き") {
            let n = extract_count_around(p, "証拠を", "つ").max(1);
            ops.push(ImmediateOp::FlipEvidenceDown(n));
        } else if p.contains("相手の証拠") && p.contains("表向き") {
            // Opponent's evidence flip handled gracefully as evidence op if needed
        } else if p.contains("証拠を") && (p.contains("得る") || p.contains("増やす")) {
            let n = extract_count_around(p, "証拠を", "つ").max(1);
            ops.push(ImmediateOp::GainEvidence(n));
        }

        if p.contains("デッキのカードを上から") && p.contains("リムーブ") {
            let n = extract_count_around(p, "上から", "枚リムーブ");
            if n > 0 {
                ops.push(ImmediateOp::MillDeck(n));
            }
        }

        if let Some(buff) = parse_signed_ap(p, true) {
            ops.push(ImmediateOp::SelfApBuff(buff));
        }

        if p.contains("このキャラをアクティブに") {
            ops.push(ImmediateOp::SelfActivate);
        }

        if p.contains("このキャラは〚突撃［事件］〛") {
            ops.push(ImmediateOp::SelfGiveRush(Some("CASE".to_string())));
        } else if p.contains("このキャラは〚突撃［キャラ］〛") {
            ops.push(ImmediateOp::SelfGiveRush(Some("CHAR".to_string())));
        } else if p.contains("このキャラは〚突撃〛") || p.contains("このキャラは〚迅速〛")
        {
            ops.push(ImmediateOp::SelfGiveRush(None));
        }
    }
    ops
}

/// パッシブ（常時/自分ターン中/相手ターン中）のAP補正を計算する
pub fn compute_passive_ap_bonus(clause: &str, ctx: &PlayerCondInfo) -> i32 {
    if check_clause_conditions(clause, ctx).is_err() {
        return 0;
    }
    let clean = strip_parens(clause);
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

fn extract_bracketed_value<'a>(text: &'a str, start_pat: &str, end_pat: &str) -> Option<&'a str> {
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
}
