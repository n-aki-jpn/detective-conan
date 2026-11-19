use super::*;

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
        let (fsc, fsf) = extract_field_sleep_spec(prefix);
        cost.field_sleep = fsc;
        cost.field_sleep_filter = fsf;
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
        cost.file_remove = extract_count_around(seg, "FILEを", "枚リムーブ")
            .max(extract_count_around(seg, "FILEエリアのカードを", "枚リムーブ"));
    }

    cost
}

/// `"〚...をN枚スリープさせる〛"` を解析する
pub fn extract_field_sleep_spec(prefix: &str) -> (usize, Option<TargetFilter>) {
    if !prefix.contains("スリープさせる") {
        return (0, None);
    }
    let inner = prefix
        .split('〚')
        .filter_map(|seg| seg.split('〛').next())
        .find(|seg| seg.contains("スリープさせる"))
        .unwrap_or(prefix);
    let Some(sleep_idx) = inner.find("スリープさせる") else {
        return (0, None);
    };
    let scope = inner[..sleep_idx].trim();
    let count = extract_count_around(inner, "を", "枚スリープ").max(1);
    let mut f = parse_target_filter(scope, TargetZone::Field, true, None);
    f.owner = OwnerScope::SelfOnly;
    f.zone = TargetZone::Field;
    f.count = count;
    f.orientations = vec![CardOrientation::ACTIVE];
    (count, Some(f))
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

        // 「このターン中、自分は〚カード名［X］〛を使用できず、登場させることができない。」（B06103 ジン）
        // ターン内・カード名ベースの使用BANを登録
        if let Some(ban_name) = extract_turn_name_ban(sentence) {
            push_op_or_extra(&mut plan, seen_target_or_search, ImmediateOp::BanNameThisTurn(ban_name));
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
        // 手札から自身を登場させる（B06103 ジン等）
        if sentence.contains("手札からこのキャラをスリープ状態で登場")
            || sentence.contains("このキャラを手札からスリープ状態で登場")
        {
            if let Some(comma_idx) = sentence.find("、手札からこのキャラをスリープ状態で登場")
                .or_else(|| sentence.find("、このキャラを手札からスリープ状態で登場"))
            {
                let prefix = sentence[..comma_idx].trim();
                let normalized_prefix = if prefix.ends_with("リムーブし") {
                    format!("{}選び、リムーブする", &prefix[..prefix.len() - "リムーブし".len()])
                } else {
                    prefix.to_string()
                };
                if let Some(mut step) = parse_target_step(&normalized_prefix, dynamic_level, source_card_type) {
                    step.follow_up.push(ImmediateOp::PlaySelfFromHand { sleep: true });
                    plan.target_step = Some(step);
                    seen_target_or_search = true;
                    continue;
                }
            }
            let op = ImmediateOp::PlaySelfFromHand { sleep: true };
            push_op_or_extra(&mut plan, seen_target_or_search, op);
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

pub fn push_op_or_extra(plan: &mut ParsedClausePlan, seen_target_or_search: bool, op: ImmediateOp) {
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

pub fn assign_or_queue_target_step(
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

pub fn strip_playable_restriction_sentences(text: &str) -> String {
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

pub fn split_before_later_optional_cost(text: &str) -> (&str, Option<&str>) {
    if let Some(idx) = text.find("てもよい。そうした場合、") {
        let before = &text[..idx];
        if let Some(dot) = before.rfind('。') {
            let end = dot + '。'.len_utf8();
            return (&text[..end], Some(text[end..].trim()));
        }
    }
    (text, None)
}

pub fn expand_multi_target_and_sentences(chunk: &str) -> String {
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

pub fn replace_sonochara_with_base(base_sentence: &str, alt_body: &str) -> String {
    if let Some((base_target, _)) = base_sentence.split_once("選び、") {
        alt_body.replace("そのキャラ", &format!("{}選び", base_target))
    } else {
        alt_body.to_string()
    }
}

pub fn parse_optional_cost_target_step(
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
    if seg.starts_with("このキャラを") || seg.contains("このキャラをリムーブエリアから") {
        cost_step.filter.self_only = true;
        cost_step.filter.owner = OwnerScope::SelfOnly;
        if seg.contains("リムーブエリアから") {
            cost_step.filter.zone = TargetZone::Remove;
        }
    }
    if cost_step.action == TargetAction::Sleep && cost_step.filter.orientations.is_empty() {
        cost_step.filter.orientations = vec![CardOrientation::ACTIVE];
    } else if cost_step.action == TargetAction::Stun && cost_step.filter.orientations.is_empty() {
        cost_step.filter.orientations = vec![CardOrientation::ACTIVE, CardOrientation::SLEEP];
    }
    Some(cost_step)
}

pub fn try_parse_leading_optional_cost_clause(
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

    // 3.5) 裏向きの証拠をNつ表向きにしてもよい（B09113, B11002, B11097）
    if seg.contains("裏向きの証拠を") && seg.contains("表向き") {
        let count = extract_count_around(seg, "裏向きの証拠を", "つ表向き").max(1);
        let mut filter = TargetFilter::default();
        filter.owner = OwnerScope::SelfOnly;
        filter.zone = TargetZone::Evidence;
        filter.count = count;
        filter.up_to = false;
        filter.face_down_only = true;
        plan.target_step = Some(TargetStep {
            action: TargetAction::FlipEvidenceFaceUp,
            filter,
            bonuses: Vec::new(),
            follow_up: Vec::new(),
            conditional_follow_up: None,
            optional: true,
            then_abort_if_skipped: true,
            scale_by_cost_flipped_evidence: false,
        });
        if !then_rest.is_empty() {
            plan.extra_clauses.push(then_rest.to_string());
        }
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

pub fn parse_reveal_deck_until(
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

pub fn try_combine_pick_and_split(
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

pub fn try_parse_conditional_sentence(
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
    // 「...」内の「場合、」（付与される能力テキスト）は条件分岐として扱わない
    if cond_raw.matches('「').count() > cond_raw.matches('」').count() {
        return None;
    }
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

pub fn extract_deck_search_scope(effect_body: &str) -> &str {
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

pub fn is_target_selection_sentence(s: &str) -> bool {
    let outer = crate::effects::text_utils::strip_quotes(s);
    let s = outer.as_str();
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

pub fn parse_deck_look_count(text: &str) -> Option<usize> {
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

pub fn parse_deck_search_step(
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

pub fn parse_conditional_follow_up(sentence: &str) -> Option<(TargetBranch, Vec<ImmediateOp>)> {
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

