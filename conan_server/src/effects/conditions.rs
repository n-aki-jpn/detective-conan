use super::*;

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

pub fn check_single_bracket_tag(tag: &str, ctx: &PlayerCondInfo) -> Result<(), String> {
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
            .or_else(|| extract_number_before(c, "合わせて", "枚以上"))
            .or_else(|| extract_number_before(c, "カードが", "枚以上"))
        {
            let scope_end = c.find("が合わせて").unwrap_or_else(|| {
                let before_maj = c.find("枚以上").unwrap_or(c.len());
                c[..before_maj].rfind('が').unwrap_or(before_maj)
            });
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
        let mut branches = parse_branches_from_phrase(req_slice, CardType::CHARACTER);
        if include_partner {
            for b in &mut branches {
                if b.card_types.contains(&CardType::CHARACTER)
                    && !b.card_types.contains(&CardType::PARTNER)
                {
                    b.card_types.push(CardType::PARTNER);
                }
            }
        }

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
        && (clean.contains("自分の現場に") || clean.contains("自分の現場かパートナーエリアに"))
    {
        if let Some((include_partner, req_slice)) = extract_field_presence_condition(&clean) {
            let cond_str = if include_partner {
                format!("自分の現場かパートナーエリアに{}がいる", req_slice)
            } else {
                format!("自分の現場に{}がいる", req_slice)
            };
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

pub fn check_case_tag(case_req: &str, case_def: Option<&CardDefinition>) -> Result<(), String> {
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

pub fn check_bond(bond_name: &str, ctx: &PlayerCondInfo) -> bool {
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

pub fn extract_field_presence_condition(clean: &str) -> Option<(bool, &str)> {
    let candidate = match (
        clean.rfind("自分の現場かパートナーエリアに"),
        clean.rfind("自分の現場に"),
    ) {
        (Some(p_idx), Some(f_idx)) => {
            if p_idx >= f_idx {
                (true, &clean[p_idx + "自分の現場かパートナーエリアに".len()..])
            } else {
                (false, &clean[f_idx + "自分の現場に".len()..])
            }
        }
        (Some(p_idx), None) => (true, &clean[p_idx + "自分の現場かパートナーエリアに".len()..]),
        (None, Some(f_idx)) => (false, &clean[f_idx + "自分の現場に".len()..]),
        (None, None) => return None,
    };
    let (include_partner, tail) = candidate;
    let end = tail
        .find("がいる場合")
        .or_else(|| tail.find("がいるなら"))
        .or_else(|| tail.find("が登場した場合"))?;
    Some((include_partner, &tail[..end]))
}

/// `【変装】` 句の条件チェック
pub fn check_disguise_conditions(clause: &str, ctx: &PlayerCondInfo) -> Result<(), String> {
    check_clause_conditions(clause, ctx)
}

