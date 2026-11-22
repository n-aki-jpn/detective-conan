use super::*;

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
    let self_only = phrase.starts_with("このキャラ") && !exclude_self && !phrase.starts_with("このキャラの");

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
    let max_level_from_prev = phrase.contains("そのキャラのレベル以下")
        || phrase.contains("リムーブしたキャラのレベル以下")
        || phrase.contains("リムーブしたカードのレベル以下")
        || phrase.contains("デッキの下に移したキャラのレベル以下")
        || phrase.contains("スタンさせたキャラのレベル以下");
    let max_level_from_removed_sum =
        phrase.contains("移したカードのレベルの合計以下")
            || phrase.contains("リムーブしたカードのレベルの合計以下")
            || phrase.contains("重ねたキャラのレベルの合計以下")
            || phrase.contains("発見されたカードのレベルの合計以下");
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
pub fn isolate_target_noun_phrase(raw: &str) -> &str {
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
    } else if phrase.contains("〚突撃〛を持つ") || phrase.contains("突撃を持つ") {
        Some("〚突撃".to_string())
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

pub fn split_or_segments(phrase: &str) -> Vec<&str> {
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

pub fn infer_card_types(phrase: &str, default_card_type: CardType) -> Vec<CardType> {
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

pub fn extract_color_filters(phrase: &str) -> Vec<String> {
    let mut cols = Vec::new();
    for col in COLORS {
        let tag = format!("【{}】", col);
        if phrase.contains(&tag) {
            cols.push((*col).to_string());
        }
    }
    cols
}

pub fn parse_single_branch(
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
    b.max_ap_from_actor = part.contains("このキャラのAP以下");
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
    let eff_def_owned;
    let eff_def = if card.temp_level_delta != 0 {
        let mut d = def.clone();
        d.level = (d.level + card.temp_level_delta).max(1);
        eff_def_owned = d;
        &eff_def_owned
    } else {
        def
    };
    filter.branches.iter().any(|b| {
        if card.temp_name_override.is_some() || matches!(card.zone, crate::models::Zone::DECK | crate::models::Zone::REMOVE) {
            if let Some(ref nm) = b.name_req {
                if !has_instance_name_match(card, eff_def, nm) {
                    return false;
                }
                let mut b_copy = b.clone();
                b_copy.name_req = None;
                return branch_matches_with_ctx(eff_def, effective_ap, &b_copy, self_file_count);
            }
            if let Some(ref ex_nm) = b.exclude_name {
                if has_instance_name_match(card, eff_def, ex_nm) {
                    return false;
                }
            }
        }
        branch_matches_with_ctx(eff_def, effective_ap, b, self_file_count)
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
        if req_kw == "〚突撃" {
            let has_innate_rush = crate::effects::text_utils::extract_clauses(
                &def.description,
                crate::effects::types::TriggerKind::Passive,
                def.card_type,
            )
            .into_iter()
            .any(|cl| {
                cl.contains("〚突撃")
                    && !cl.contains("場合、")
                    && !cl.contains("とき、")
                    && !cl.contains("ターン終了時まで")
                    && !cl.contains("を得る")
            });
            if !has_innate_rush {
                return false;
            }
        } else if req_kw.starts_with('【') && req_kw.ends_with('】') {
            let tag = req_kw.trim_start_matches('【').trim_end_matches('】');
            let has_tag = def.description.lines().any(|line| {
                crate::effects::text_utils::extract_leading_bracket_tags(line)
                    .iter()
                    .any(|t| *t == tag)
            });
            if !has_tag {
                return false;
            }
        } else if !def.description.contains(req_kw.as_str()) {
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

