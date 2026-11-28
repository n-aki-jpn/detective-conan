use super::*;

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

pub fn parse_rush_keyword_token(text: &str) -> Option<(bool, Option<&'static str>)> {
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
        if trimmed.is_empty() || trimmed.starts_with('・') {
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
    self_set_cards_len: usize,
    actor_instance_id: &str,
    actor_def: &CardDefinition,
    actor_ap: i32,
    target_instance_id: Option<&str>,
    target_def: Option<&CardDefinition>,
    target_ap: Option<i32>,
) -> bool {
    let clean = strip_parens(clause);
    let leading_tags = extract_leading_bracket_tags(&clean);
    for tag in &leading_tags {
        if let Some(num_s) = tag.strip_suffix("枚以上") {
            if let Ok(req_under) = num_s.parse::<usize>() {
                if self_set_cards_len < req_under {
                    return false;
                }
            }
        }
    }
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
    if clause.contains("【疾風】") && !clause.contains("【疾風】を持つ") {
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

/// 現場のキャラがリムーブされたときの常駐誘発（`〜のキャラがリムーブされたとき`）に一致するか判定する
#[allow(clippy::too_many_arguments)]
pub fn check_char_removed_match(
    clause: &str,
    owner_is_watcher: bool,
    self_instance_id: &str,
    removed_instance_id: &str,
    removed_def: &CardDefinition,
    removed_ap: i32,
    removed_level: i32,
    removed_orientation: CardOrientation,
    by_own_ability_of_removed_owner: bool,
) -> bool {
    let clean = strip_parens(clause);
    let body = strip_leading_tags(&clean);
    let Some(idx) = body.find("リムーブされたとき") else {
        return false;
    };
    let trig = body[..idx].trim();

    if trig.contains("自分の能力や効果によって") && !by_own_ability_of_removed_owner {
        return false;
    }

    let mut eff_def = removed_def.clone();
    eff_def.level = removed_level;

    if trig.contains("相手の現場に") {
        if owner_is_watcher {
            return false;
        }
        let subj = trig
            .replace("相手の現場にいる", "")
            .replace("相手の現場に", "");
        let subj = subj.trim().trim_end_matches(['が', 'に', 'で']).trim();
        let branches = parse_branches_from_phrase(subj, CardType::CHARACTER);
        return branches.iter().any(|b| branch_matches(&eff_def, removed_ap, b));
    }

    if !owner_is_watcher {
        return false;
    }
    if trig.contains("このキャラ以外の") && self_instance_id == removed_instance_id {
        return false;
    }

    if !removed_instance_id.is_empty() && self_instance_id == removed_instance_id {
        if trig.starts_with("スリープ状態のこのキャラ") {
            return removed_orientation == CardOrientation::SLEEP;
        }
        if trig.contains("このキャラか") || trig.starts_with("このキャラ") {
            return true;
        }
        return false;
    }

    let Some((_, after_field)) = trig.split_once("自分の現場にいる") else {
        return false;
    };
    let subj = after_field
        .replace("このキャラ以外の", "")
        .replace("自分の能力や効果によって", "");
    let subj = subj.trim().trim_end_matches(['が', 'に', 'で']).trim();

    if subj.contains("スリープ状態の") && removed_orientation != CardOrientation::SLEEP {
        return false;
    }
    if subj.contains("スタン状態の") && removed_orientation != CardOrientation::STUN {
        return false;
    }
    if subj.contains("アクティブ状態の") && removed_orientation != CardOrientation::ACTIVE {
        return false;
    }

    let branches = parse_branches_from_phrase(subj, CardType::CHARACTER);
    branches.iter().any(|b| branch_matches(&eff_def, removed_ap, b))
}

pub fn check_passive_extra_conditions(body: &str, pctx: &PassiveEvalContext) -> bool {
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

pub fn compute_passive_multiplier(body: &str, pctx: &PassiveEvalContext) -> Option<i32> {
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

pub fn extract_passive_effect_part(body: &str) -> &str {
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
    if clean.trim_start().starts_with('・')
        || clean.contains("元の能力を無効")
        || clean.contains("選ばれない")
        || clean.contains("ミスリード")
        || clean.contains("ターン終了時まで")
        || clean.contains("選び")
    {
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
    if clean.trim_start().starts_with('・')
        || clean.contains("元の能力を無効")
        || clean.contains("ミスリード")
        || clean.contains("ターン終了時まで")
        || clean.contains("選び")
    {
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

