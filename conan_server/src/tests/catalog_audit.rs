use super::*;
use crate::effects::{extract_clauses, parse_effect_steps, strip_parens, TriggerKind};
use std::collections::BTreeMap;

#[test]
fn audit_unparsed_card_effects() {
    let catalog = load_test_catalog();

    // 既知の未対応・特殊処理除外カード（新規増加時に気づけるよう管理）
    let known_ignored_ids: &[&str] = &[];

    let trigger_checks: &[(&str, TriggerKind)] = &[
        ("【登場時】", TriggerKind::OnPlay),
        ("【宣言】", TriggerKind::Declare),
        ("【カットイン】", TriggerKind::Cutin),
        ("【ヒラメキ】", TriggerKind::Hirameki),
        ("【現場リムーブ時】", TriggerKind::OnRemove),
        ("【変装時】", TriggerKind::OnDisguise),
        ("ターン終了時", TriggerKind::OnTurnEnd),
    ];

    let mut missing_clause_cards: BTreeMap<String, Vec<(String, String, String)>> = BTreeMap::new();
    let mut empty_plan_cards: BTreeMap<String, Vec<(String, String, String)>> = BTreeMap::new();

    let mut defs: Vec<_> = catalog.definitions.values().collect();
    defs.sort_by(|a, b| a.definition_id.cmp(&b.definition_id));

    for def in defs {
        if def.description.trim().is_empty() {
            continue;
        }
        if known_ignored_ids.contains(&def.definition_id.as_str()) {
            continue;
        }
        // パラレル版(*P)の重複レポートを避けるため通常版のみを主対象とする
        if def.definition_id.ends_with('P') {
            continue;
        }

        let card_name = def
            .names
            .first()
            .cloned()
            .unwrap_or_else(|| def.definition_id.clone());
        let clean_desc = strip_parens(&def.description);

        for (kw, kind) in trigger_checks {
            if clean_desc.contains(kw) {
                // 引用符「...」の中だけに含まれている場合は除外
                let has_outside_quote = def
                    .description
                    .lines()
                    .any(|line| {
                        let l = strip_parens(line.trim());
                        !l.starts_with('「') && l.contains(kw)
                    });
                if !has_outside_quote {
                    continue;
                }

                let clauses = extract_clauses(&def.description, *kind, def.card_type);
                if clauses.is_empty() {
                    missing_clause_cards
                        .entry((*kw).to_string())
                        .or_default()
                        .push((def.definition_id.clone(), card_name.clone(), def.description.clone()));
                    continue;
                }

                for clause in clauses {
                    let plan = parse_effect_steps(&clause, None, def.card_type);
                    let is_empty_plan = plan.immediate_before.is_empty()
                        && plan.target_step.is_none()
                        && plan.deck_search.is_none()
                        && plan.post_target_ops.is_empty()
                        && plan.extra_clauses.is_empty();

                    let is_special_or_passive = clause.contains("代わりに")
                        || clause.contains("AP＋")
                        || clause.contains("AP+")
                        || clause.contains("AP－")
                        || clause.contains("AP-")
                        || clause.contains("以下から")
                        || clause.contains("このターン中");

                    if is_empty_plan && !is_special_or_passive {
                        empty_plan_cards
                            .entry((*kw).to_string())
                            .or_default()
                            .push((def.definition_id.clone(), card_name.clone(), clause));
                    }
                }
            }
        }
    }

    println!("\n=== [Catalog Audit] extract_clauses 空振りレポート ===");
    let mut missing_total = 0usize;
    for (kw, list) in &missing_clause_cards {
        println!("--- {} ({}件) ---", kw, list.len());
        missing_total += list.len();
        for (id, name, desc) in list.iter().take(15) {
            println!("  [{}] {}: {}", id, name, desc.replace('\n', " / "));
        }
        if list.len() > 15 {
            println!("  ... 他 {}件", list.len() - 15);
        }
    }

    println!("\n=== [Catalog Audit] parse_effect_steps 空プランレポート ===");
    let mut empty_total = 0usize;
    for (kw, list) in &empty_plan_cards {
        println!("--- {} ({}件) ---", kw, list.len());
        empty_total += list.len();
        for (id, name, clause) in list.iter().take(15) {
            println!("  [{}] {}: {}", id, name, clause);
        }
        if list.len() > 15 {
            println!("  ... 他 {}件", list.len() - 15);
        }
    }

    println!(
        "\n📊 [Catalog Audit Summary] 句抽出空振り: {}件 / プラン空: {}件",
        missing_total, empty_total
    );
}

#[test]
fn test_catalog_cards_have_pack_metadata_and_restrictions_apply() {
    let mut cat = load_test_catalog();
    let cards = cat.raw_json["cards"].as_array().expect("cards array");
    assert!(!cards.is_empty());
    for c in cards {
        let def_id = c["definitionId"].as_str().unwrap_or("");
        let pkg = c["package"].as_str().unwrap_or("");
        let code = c["packCode"].as_str().unwrap_or("");
        assert!(!pkg.is_empty(), "card {} must have package", def_id);
        assert!(!code.is_empty(), "card {} must have packCode", def_id);
    }

    let cfg = crate::catalog::PackRestrictionsConfig {
        unreleased_packs: vec!["CT-P11".to_string()],
        restrict_pr: true,
        restricted_card_ids: vec!["B01001".to_string()],
    };
    crate::catalog::apply_pack_restrictions(&mut cat.raw_json, &cfg);
    let updated = cat.raw_json["cards"].as_array().unwrap();
    let p11_card = updated.iter().find(|c| c["packCode"] == "CT-P11").unwrap();
    assert_eq!(p11_card["isRestricted"], true);
    let pr_card = updated.iter().find(|c| c["packCode"] == "PR").unwrap();
    assert_eq!(pr_card["isRestricted"], true);
    let specific = updated.iter().find(|c| c["definitionId"] == "B01001").unwrap();
    assert_eq!(specific["isRestricted"], true);
    let normal_p01 = updated
        .iter()
        .find(|c| c["packCode"] == "CT-P01" && c["definitionId"] != "B01001")
        .unwrap();
    assert_eq!(normal_p01["isRestricted"], false);
}
