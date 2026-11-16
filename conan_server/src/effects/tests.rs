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
