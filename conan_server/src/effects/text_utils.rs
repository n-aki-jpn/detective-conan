use super::*;

/// 「このターン中、自分は〚カード名［X］〛を使用できず、登場させることができない。」（B06103 ジン等）
/// によるターン内・カード名ベースの使用BAN名を抽出する
pub fn extract_turn_name_ban(sentence: &str) -> Option<String> {
    if !sentence.contains("このターン中")
        || (!sentence.contains("使用できず") && !sentence.contains("使用できない"))
    {
        return None;
    }
    extract_bracketed_value(sentence, "〚カード名［", "］〛").map(|s| s.to_string())
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

/// カギ括弧 `「...」` で囲まれた付与能力テキスト等を除去する（トリガー種別判定での誤爆防止）
pub fn strip_quotes(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut depth = 0usize;
    for ch in input.chars() {
        if ch == '「' {
            depth += 1;
        } else if ch == '」' {
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

pub fn line_matches_trigger(line: &str, trigger: TriggerKind, card_type: CardType) -> bool {
    let clean = strip_parens(line);
    let unquoted = strip_quotes(&clean);
    match trigger {
        TriggerKind::OnPlay => {
            if card_type == CardType::EVENT {
                let t = unquoted.trim_start();
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
                unquoted.contains("【登場時】")
            }
        }
        TriggerKind::Declare => {
            extract_leading_bracket_tags(line).iter().any(|t| *t == "宣言")
        }
        TriggerKind::Cutin => line.trim_start().starts_with("【カットイン】"),
        TriggerKind::Hirameki => line.trim_start().starts_with("【ヒラメキ】"),
        TriggerKind::DisguiseCond => unquoted.contains("【変装】"),
        TriggerKind::OnDisguise => unquoted.contains("【変装時】"),
        TriggerKind::OnRemove => {
            !extract_leading_bracket_tags(line).iter().any(|t| *t == "宣言")
                && (unquoted.contains("【現場リムーブ時】")
                    || unquoted.contains("自分の現場にいるこのキャラが自分の能力や効果によってリムーブされたとき"))
        }
        TriggerKind::OnCharRemoved => {
            !unquoted.contains("【登場時】")
                && !unquoted.contains("【宣言】")
                && !unquoted.contains("【カットイン】")
                && !unquoted.contains("【ヒラメキ】")
                && !unquoted.contains("【現場リムーブ時】")
                && !unquoted.starts_with("このイベントがセットされている")
                && !unquoted.contains("自分の現場にいるこのキャラが自分の能力や効果によってリムーブされたとき")
                && !unquoted.contains("コンタクトによってリムーブされたとき")
                && !unquoted.contains("証拠がリムーブされたとき")
                && unquoted.contains("キャラ")
                && unquoted.contains("リムーブされたとき")
        }
        TriggerKind::OnTurnEnd => {
            // 「ターン終了時まで〜」という継続期間表記や付与テキスト内の「ターン終了時、」はトリガーではない
            !unquoted.contains("【登場時】")
                && !unquoted.contains("【宣言】")
                && (unquoted.contains("ターン終了時、")
                    || unquoted.contains("自分のターン終了時、")
                    || unquoted.contains("相手のターン終了時、"))
        }
        TriggerKind::OnAction => {
            !unquoted.starts_with("このイベントがセットされている")
                && !unquoted.contains("【登場時】")
                && !unquoted.contains("【宣言】")
                && !unquoted.contains("アクション［事件］によって証拠を得たとき")
                && !unquoted.contains("アクションによって証拠を得たとき")
                && !unquoted.contains("ガードされたとき")
                && !unquoted.contains("ガードしたとき")
                && (unquoted.contains("アクションしたとき")
                    || unquoted.contains("アクション［キャラ］したとき")
                    || unquoted.contains("アクション［事件］したとき")
                    || unquoted.contains("推理したとき")
                    || unquoted.contains("推理かアクションしたとき"))
        }
        TriggerKind::OnContact => {
            !unquoted.contains("【登場時】")
                && !unquoted.contains("【宣言】")
                && (unquoted.contains("コンタクトしたとき") || unquoted.contains("コンタクトされたとき"))
        }
        TriggerKind::OnContactWin => {
            !unquoted.contains("【登場時】")
                && !unquoted.contains("【宣言】")
                && (unquoted.contains("コンタクトで勝利したとき")
                    || unquoted.contains("コンタクトによってリムーブされたとき"))
        }
        TriggerKind::OnHintUse => {
            !unquoted.contains("【登場時】")
                && !unquoted.contains("【宣言】")
                && unquoted.contains("ネクストヒントで")
                && unquoted.contains("を使用したとき")
        }
        TriggerKind::OnAllyPlay => {
            !unquoted.contains("【登場時】")
                && !unquoted.contains("【宣言】")
                && !unquoted.contains("【現場リムーブ時】")
                && !unquoted.starts_with("このイベントがセットされている")
                && (unquoted.contains("【疾風】") || unquoted.contains("登場したとき"))
        }
        TriggerKind::OnEvidenceGain => {
            !unquoted.contains("【登場時】")
                && !unquoted.contains("【宣言】")
                && !unquoted.contains("【ヒラメキ】")
                && !unquoted.starts_with("このイベントがセットされている")
                && (unquoted.contains("アクション［事件］によって証拠を得たとき")
                    || unquoted.contains("アクションによって証拠を得たとき")
                    || unquoted.contains("自分が証拠を得たとき"))
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
pub fn extract_bracket_tags(text: &str) -> Vec<&str> {
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

pub const COLORS: &[&str] = &["青", "緑", "白", "赤", "黄", "黒"];

pub fn normalize_card_name(s: &str) -> String {
    strip_parens(s).replace('&', "＆")
}

/// カード表示名（`江戸川コナン(B04005)` やデュオ名 `江戸川コナン＆工藤新一(B05005)`、
/// 別名付き `黒衣の騎士・スペイド（工藤新一）` 等）から比較用の個別カード名トークン一覧を抽出する
pub fn extract_distinct_card_names(raw: &str) -> Vec<String> {
    let mut no_ascii_parens = String::with_capacity(raw.len());
    let mut depth = 0usize;
    for ch in raw.chars() {
        if ch == '(' {
            depth += 1;
        } else if ch == ')' {
            depth = depth.saturating_sub(1);
        } else if depth == 0 {
            no_ascii_parens.push(ch);
        }
    }

    let mut out: Vec<String> = Vec::new();
    let mut push_unique = |s: &str| {
        let trimmed = s.trim();
        if !trimmed.is_empty() && !out.iter().any(|x| x == trimmed) {
            out.push(trimmed.to_string());
        }
    };

    for part in no_ascii_parens.split(&['&', '＆'][..]) {
        let p = part.trim();
        if p.is_empty() {
            continue;
        }
        if let (Some(open_idx), Some(close_idx)) = (p.find('（'), p.rfind('）')) {
            if open_idx < close_idx {
                let base = p[..open_idx].trim();
                let alias = p[open_idx + '（'.len_utf8()..close_idx].trim();
                let after = p[close_idx + '）'.len_utf8()..].trim();
                if !base.is_empty() && after.is_empty() && !alias.is_empty() && alias != "笑" {
                    push_unique(base);
                    push_unique(alias);
                    continue;
                }
            }
        }
        push_unique(p);
    }
    out
}

fn raw_name_matches_query(candidate_name: &str, query: &str, query_tokens: &[String]) -> bool {
    let q = normalize_card_name(query);
    let q_amp = query.replace('&', "＆");
    let n_amp = candidate_name.replace('&', "＆");
    if n_amp == q_amp
        || normalize_card_name(candidate_name) == q
        || n_amp.contains(&format!("（{}）", q_amp))
    {
        return true;
    }
    if query_tokens.is_empty() {
        return false;
    }
    let name_tokens = extract_distinct_card_names(candidate_name);
    query_tokens
        .iter()
        .all(|qt| name_tokens.iter().any(|t| t == qt))
}

pub fn has_name_match(def: &CardDefinition, query: &str) -> bool {
    let query_tokens = extract_distinct_card_names(query);
    def.names
        .iter()
        .any(|n| raw_name_matches_query(n, query, &query_tokens))
}

pub fn has_trait_match(def: &CardDefinition, query: &str) -> bool {
    def.traits.iter().any(|t| t == query)
}

/// インスタンス（一時的なカード名書き換え・ゾーン限定別名を含む）のカード名一致判定
pub fn has_instance_name_match(inst: &CardInstance, def: &CardDefinition, query: &str) -> bool {
    let query_tokens = extract_distinct_card_names(query);
    if let Some(ref ov) = inst.temp_name_override {
        return raw_name_matches_query(ov, query, &query_tokens);
    }
    if has_name_match(def, query) {
        return true;
    }
    if matches!(inst.zone, crate::models::Zone::DECK | crate::models::Zone::REMOVE) {
        for line in def.description.lines() {
            if (line.contains("デッキかリムーブエリアにある") || line.contains("リムーブエリアにある"))
                && line.contains("としても扱う")
            {
                if let Some(start) = line.find("〚カード名［") {
                    let rest = &line[start + "〚カード名［".len()..];
                    if let Some(end) = rest.find("］〛") {
                        let alias = &rest[..end];
                        if raw_name_matches_query(alias, query, &query_tokens) {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

pub fn extract_leading_bracket_tags(text: &str) -> Vec<&str> {
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

pub fn normalize_digit(ch: char) -> char {
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

pub fn parse_signed_level(text: &str, positive: bool) -> Option<i32> {
    let markers: &[&str] = if positive {
        &["レベル＋", "レベル+"]
    } else {
        &["レベル－", "レベル-", "レベルー"]
    };
    for m in markers {
        if let Some(val) = extract_number_before(text, m, "") {
            return Some(if positive { val } else { -val });
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

