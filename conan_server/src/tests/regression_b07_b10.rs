use super::*;

#[test]
fn test_bug3_b07094_gin_and_optional_compound_costs() {
    let catalog = load_test_catalog();

    // Case A: B07094 alone (no lv<=7 char on own field) => TurnEnd must NOT fire opponent removal
    {
        let mut engine = MatchEngine::new(catalog.clone());
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];
        engine.phase = models::MatchPhase::END_TRIGGERS;

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let mut ps2 = engine::PlayerState::new("p2".to_string());
        ps1.partner = Some(test_inst("p1_p", "B03106", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE)); // 黒
        ps1.field.push(test_inst("p1_gin", "B07094", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE)); // lv8
        ps1.hand.push(test_inst("h1", "B01011", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps2.field.push(test_inst("opp_c", "B01011", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];
    engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        engine.trigger_on_turn_end("p1");
        assert!(engine.pending_decision.is_none(), "Without self lv<=7 field char, B07094 compound cost must not prompt");
        assert_eq!(engine.players.get("p1").unwrap().hand.len(), 1);
        assert_eq!(engine.players.get("p2").unwrap().field.len(), 1);
    }

    // Case B: B07094 + self_lv4 present -> pay hand discard -> SKIP self_lv4 remove -> opponent char NOT removed
    {
        let mut engine = MatchEngine::new(catalog.clone());
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];
        engine.phase = models::MatchPhase::END_TRIGGERS;

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let mut ps2 = engine::PlayerState::new("p2".to_string());
        ps1.partner = Some(test_inst("p1_p", "B03106", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
        ps1.r#case = Some(test_inst("p1_c", "B01024", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
        ps2.r#case = Some(test_inst("p2_c", "B01024", "p2", models::Zone::CASE, models::CardOrientation::ACTIVE));
        ps1.field.push(test_inst("p1_gin", "B07094", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.field.push(test_inst("self_lv4", "B01011", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("h1", "B01011", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps2.field.push(test_inst("opp_c", "B01014", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];
    engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        engine.trigger_on_turn_end("p1");
        let pd1 = engine.pending_decision.clone().expect("Step 1 DISCARD_HAND must prompt");
        assert_eq!(pd1["kind"], "targetSelect");
        assert_eq!(pd1["options"]["effectType"], "DISCARD_HAND");
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd1["decisionId"],
            "revision": engine.revision,
            "value": {"targetId": "h1", "actorId": "p1_gin", "effectType": "DISCARD_HAND"}
        }));

        let pd2 = engine.pending_decision.clone().expect("Step 2 self lv<=7 REMOVE must prompt");
        assert_eq!(pd2["kind"], "targetSelect");
        assert_eq!(pd2["options"]["effectType"], "REMOVE");
        let cands2: Vec<&str> = pd2["options"]["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|c| c["instanceId"].as_str())
            .collect();
        assert_eq!(cands2, vec!["self_lv4"]);

        // Skip step 2 -> must abort follow-up opponent removal
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd2["decisionId"],
            "revision": engine.revision,
            "value": {"skip": true}
        }));
        assert!(engine.players.get("p1").unwrap().field.iter().any(|c| c.instance_id == "self_lv4"));
        assert!(engine.players.get("p2").unwrap().field.iter().any(|c| c.instance_id == "opp_c"), "opp_c must NOT be removed when step 2 is skipped");
    }

    // Case C: Pay both hand discard AND self_lv4 remove -> prompts opponent removal & removes opp_c
    {
        let mut engine = MatchEngine::new(catalog.clone());
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];
        engine.phase = models::MatchPhase::END_TRIGGERS;

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let mut ps2 = engine::PlayerState::new("p2".to_string());
        ps1.partner = Some(test_inst("p1_p", "B03106", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
        ps1.r#case = Some(test_inst("p1_c", "B01024", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
        ps2.r#case = Some(test_inst("p2_c", "B01024", "p2", models::Zone::CASE, models::CardOrientation::ACTIVE));
        ps1.field.push(test_inst("p1_gin", "B07094", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.field.push(test_inst("self_lv4", "B01011", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("h1", "B01011", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps2.field.push(test_inst("opp_c", "B01014", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        for i in 0..4 {
            ps1.deck.push(test_inst(&format!("c_d1_{}", i), "B01011", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
            ps2.deck.push(test_inst(&format!("c_d2_{}", i), "B01011", "p2", models::Zone::DECK, models::CardOrientation::ACTIVE));
        }
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];
    engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        engine.trigger_on_turn_end("p1");
        let pd1 = engine.pending_decision.clone().unwrap();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd1["decisionId"],
            "revision": engine.revision,
            "value": {"targetId": "h1", "actorId": "p1_gin", "effectType": "DISCARD_HAND"}
        }));
        let pd2 = engine.pending_decision.clone().unwrap();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd2["decisionId"],
            "revision": engine.revision,
            "value": {"targetId": "self_lv4", "actorId": "p1_gin", "effectType": "REMOVE"}
        }));

        assert!(engine.players.get("p1").unwrap().remove.iter().any(|c| c.instance_id == "self_lv4"));
        let pd3 = engine.pending_decision.clone().expect("Step 3 character REMOVE must prompt after paying both costs");
        assert_eq!(pd3["kind"], "targetSelect");
        assert_eq!(pd3["options"]["effectType"], "REMOVE");
        let cands3: Vec<&str> = pd3["options"]["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|c| c["instanceId"].as_str())
            .collect();
        assert!(cands3.contains(&"opp_c"));
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd3["decisionId"],
            "revision": engine.revision,
            "value": {"targetId": "opp_c", "actorId": "p1_gin", "effectType": "REMOVE"}
        }));
        assert!(engine.players.get("p1").unwrap().remove.iter().any(|c| c.instance_id == "self_lv4"));
        assert!(engine.players.get("p2").unwrap().remove.iter().any(|c| c.instance_id == "opp_c"));
    }

    // Case D: Cross-card B04049 (このキャラをスリープさせ、手札からFBIを1枚リムーブしてもよい) skip leaves self ACTIVE and opp intact
    {
        let mut engine = MatchEngine::new(catalog);
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];
        engine.phase = models::MatchPhase::MAIN;

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let mut ps2 = engine::PlayerState::new("p2".to_string());
        ps1.partner = Some(test_inst("p1_p", "B01060", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE)); // 赤
        ps1.r#case = Some(test_inst("p1_c", "B01024", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
        for i in 0..8 {
            ps1.file.push(test_inst(&format!("f{}", i), "B01062", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
        }
        ps1.hand.push(test_inst("h_subaru", "B04049", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("h_fbi", "B01062", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps2.field.push(test_inst("opp_c", "B01014", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];
    engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        engine.prompt_main_action("p1");
        let pd1 = engine.pending_decision.clone().unwrap();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd1["decisionId"],
            "revision": engine.revision,
            "value": {"type": "HAND_PLAY", "cardId": "h_subaru"}
        }));
        let pd_opt = engine.pending_decision.clone().expect("B04049 optional DISCARD_HAND must prompt");
        assert_eq!(pd_opt["options"]["effectType"], "DISCARD_HAND");
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_opt["decisionId"],
            "revision": engine.revision,
            "value": {"skip": true}
        }));
        let subaru = engine.players.get("p1").unwrap().field.iter().find(|c| c.instance_id == "h_subaru").unwrap();
        assert_eq!(subaru.orientation, models::CardOrientation::ACTIVE, "Skipping B04049 optional cost must not sleep self");
        assert!(engine.players.get("p2").unwrap().field.iter().any(|c| c.instance_id == "opp_c"));
    }
}

#[test]
fn test_b08089_hell_angel_on_remove() {
    let catalog = catalog::CardCatalog::load_from_json("../data/dummy/cards.json").unwrap();
    let b08089 = catalog.get("B08089").unwrap();
    let clauses = effects::extract_clauses(&b08089.description, effects::TriggerKind::OnRemove, b08089.card_type);
    assert!(!clauses.is_empty(), "B08089 must have OnRemove clause: {}", b08089.description);
    let plan = effects::parse_effect_steps(&clauses[0], None, b08089.card_type);

    let mut engine = engine::MatchEngine::new(catalog.clone());
    engine.turn = 2;
    engine.turn_player_id = "p2".to_string(); // 相手ターン中
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];

    let mut ps1 = engine::PlayerState::new("p1".to_string());
    ps1.case_phase = models::CasePhase::RESOLUTION;
    ps1.deck.push(models::CardInstance {
        instance_id: "d_1".to_string(),
        definition_id: "B01002".to_string(),
        owner_id: "p1".to_string(),
        zone: models::Zone::DECK,
        ..Default::default()
    });
    // バッファ山札（B08089の1枚ドローで山札が0枚になった瞬間のリフレッシュを誘発させないための余裕）
    for i in 0..3 {
        ps1.deck.insert(0, models::CardInstance {
            instance_id: format!("buf_ha_{}", i),
            definition_id: "B01002".to_string(),
            owner_id: "p1".to_string(),
            zone: models::Zone::DECK,
            ..Default::default()
        });
    }
    ps1.hand.push(models::CardInstance {
        instance_id: "h_1".to_string(),
        definition_id: "B01002".to_string(),
        owner_id: "p1".to_string(),
        zone: models::Zone::HAND,
        ..Default::default()
    });
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];
    engine.players.insert("p1".to_string(), ps1);
    engine.players.insert("p2".to_string(), engine::PlayerState::new("p2".to_string()));

    engine.trigger_on_remove("p1", "B08089");

    let obs = build_player_observation(&engine, "p1");
    assert_eq!(
        obs["pendingDecision"]["kind"].as_str(),
        Some("targetSelect"),
        "解決編のB08089現場リムーブ時効果で1枚引いた後に手札1枚リムーブの選択が出ること (plan: {:?}, logs: {:?})",
        plan,
        engine.logs
    );
}

#[test]
fn test_b09058_declare_requires_sleeping_other_akai_family_char() {
    let catalog = load_test_catalog();

    // 1) 他にアクティブな特徴［赤井家］キャラがいない場合は宣言不可・ドロー不可
    {
        let mut engine = MatchEngine::new(catalog.clone());
        engine.phase = models::MatchPhase::MAIN;
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        ps1.field.push(test_inst("haneda_58", "B09058", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.field.push(test_inst("non_akai", "B01005", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.deck.push(test_inst("d1", "B01005", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];
    engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), engine::PlayerState::new("p2".to_string()));
        engine.prompt_main_action("p1");

        let obs = build_player_observation(&engine, "p1");
        let actions = obs["pendingDecision"]["options"]["actions"].as_array().unwrap();
        let has_declare = actions
            .iter()
            .find(|a| a["type"] == "DECLARE")
            .and_then(|a| a["actors"].as_array())
            .map(|arr| arr.iter().any(|x| x["actorId"] == "haneda_58"))
            .unwrap_or(false);
        assert!(!has_declare, "他に特徴［赤井家］のアクティブキャラがいない場合、B09058の宣言は使用不可であること");
        assert_eq!(engine.players["p1"].hand.len(), 0);
    }

    // 2) 他にアクティブな特徴［赤井家］キャラがいる場合は、そのキャラをスリープさせてから1ドローすること
    {
        let mut engine = MatchEngine::new(catalog.clone());
        engine.phase = models::MatchPhase::MAIN;
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        ps1.field.push(test_inst("haneda_58", "B09058", "p1", models::Zone::FIELD, models::CardOrientation::SLEEP));
        ps1.field.push(test_inst("other_akai", "B11056", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.deck.push(test_inst("d1", "B01005", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];
    engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), engine::PlayerState::new("p2".to_string()));
        engine.prompt_main_action("p1");

        let obs1 = build_player_observation(&engine, "p1");
        engine.submit_decision("p1", &serde_json::json!({
            "decisionId": obs1["pendingDecision"]["decisionId"],
            "revision": engine.revision,
            "value": { "type": "DECLARE", "actorId": "haneda_58" }
        })).unwrap();

        let obs2 = build_player_observation(&engine, "p1");
        let b10036_line2 = engine.catalog.get("B10036").unwrap().description.lines().nth(1).unwrap().to_string();
    let b10036_plan = effects::parse_effect_steps(&b10036_line2, None, models::CardType::CHARACTER);
    assert_eq!(
        obs2["pendingDecision"]["kind"].as_str(),
        Some("targetSelect"),
        "logs: {:?}, plan: {:?}",
        engine.logs,
        b10036_plan.target_step
    );
        assert_eq!(obs2["pendingDecision"]["options"]["effectType"].as_str(), Some("SLEEP"));
        let cands: Vec<&str> = obs2["pendingDecision"]["options"]["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|c| c["instanceId"].as_str())
            .collect();
        assert_eq!(cands, vec!["other_akai"]);

        engine.submit_decision("p1", &serde_json::json!({
            "decisionId": obs2["pendingDecision"]["decisionId"],
            "revision": engine.revision,
            "value": { "targetId": "other_akai" }
        })).unwrap();

        let other = engine.players["p1"].field.iter().find(|c| c.instance_id == "other_akai").unwrap();
        assert_eq!(other.orientation, models::CardOrientation::SLEEP, "コストで選んだ赤井家キャラがスリープすること");
        assert_eq!(engine.players["p1"].hand.len(), 1, "コスト支払い後に1枚ドローすること");
    }
}

#[test]
fn test_b09011_declare_sets_base_lp_to_1_for_lv4_lp0_shounen_tanteidan() {
    let catalog = catalog::CardCatalog::load_from_json("../data/dummy/cards.json").unwrap();
    let match_id = catalog
        .definitions
        .iter()
        .find(|(_, d)| {
            d.card_type == models::CardType::CHARACTER
                && d.level == 4
                && d.lp == 0
                && d.traits.iter().any(|t| t == "少年探偵団")
        })
        .map(|(id, _)| id.clone())
        .expect("Lv4・LP0・少年探偵団のキャラがcards.jsonに存在すること");
    let non_lv4_id = catalog
        .definitions
        .iter()
        .find(|(_, d)| {
            d.card_type == models::CardType::CHARACTER
                && d.level != 4
                && d.lp == 0
                && d.traits.iter().any(|t| t == "少年探偵団")
        })
        .map(|(id, _)| id.clone())
        .expect("Lv4以外・LP0・少年探偵団のキャラがcards.jsonに存在すること");

    let mut engine = MatchEngine::new(catalog);
    engine.phase = models::MatchPhase::MAIN;
    engine.turn = 4;
    engine.turn_player_id = "p1".to_string();
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];

    let mut ps1 = engine::PlayerState::new("p1".to_string());
    let mut c_b09011 = test_inst("c_b09011", "B09011", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE);
    c_b09011.entered_turn = 0;
    let mut c_match = test_inst("c_match", &match_id, "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE);
    c_match.entered_turn = 0;
    let mut c_non_lv4 = test_inst("c_non_lv4", &non_lv4_id, "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE);
    c_non_lv4.entered_turn = 0;
    ps1.field = vec![c_b09011, c_match, c_non_lv4];
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];
    engine.players.insert("p1".to_string(), ps1);
    engine.players.insert("p2".to_string(), engine::PlayerState::new("p2".to_string()));
    engine.prompt_main_action("p1");

    assert_eq!(engine.get_effective_lp("p1", &engine.players["p1"].field[1]), 0);

    let obs = build_player_observation(&engine, "p1");
    let dec_id = obs["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": dec_id,
        "revision": engine.revision,
        "value": { "type": "DECLARE", "actorId": "c_b09011" }
    })).unwrap();

    let p1 = &engine.players["p1"];
    assert_eq!(engine.get_effective_lp("p1", &p1.field[1]), 1, "Lv4・LP0・少年探偵団はLP1になること");
    assert_eq!(engine.get_effective_lp("p1", &p1.field[2]), 0, "Lv4以外は対象外でLP0のまま");
}

#[test]
fn test_b10038_quoted_ability_not_mis_parsed_as_on_play_revive() {
    let catalog = catalog::CardCatalog::load_from_json("../data/dummy/cards.json").unwrap();
    let mut engine = MatchEngine::new(catalog);
    engine.phase = models::MatchPhase::MAIN;
    engine.turn = 4;
    engine.turn_player_id = "p1".to_string();
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];

    let mut ps1 = engine::PlayerState::new("p1".to_string());
    ps1.field.push(test_inst("c_b10038", "B10038", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
    ps1.remove.push(test_inst("c_lv3", "B01001", "p1", models::Zone::REMOVE, models::CardOrientation::ACTIVE));
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];
    engine.players.insert("p1".to_string(), ps1);
    engine.players.insert("p2".to_string(), engine::PlayerState::new("p2".to_string()));

    let deployed = vec![("c_b10038".to_string(), "B10038".to_string(), models::CardOrientation::ACTIVE)];
    engine.resolve_deployed_characters("p1", None, &deployed, "MAIN");

    let obs = build_player_observation(&engine, "p1");
    assert_ne!(
        obs["pendingDecision"]["kind"].as_str(),
        Some("targetSelect"),
        "B10038登場時に「...」内の『レベル3以下のキャラを登場させる』が誤爆しないこと"
    );
}

#[test]
fn test_b10101_rush_filter_excludes_conditional_rush() {
    let mut catalog = catalog::CardCatalog::load_from_json("../data/dummy/cards.json").unwrap();
    let f = effects::parse_target_filter(
        "〚突撃〛を持つキャラ",
        effects::TargetZone::Deck,
        true,
        None,
    );

    // 常時突撃（B10037: 〚突撃〛）はマッチすること
    let b10037_inst = test_inst("c_b10037", "B10037", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE);
    assert!(
        effects::card_matches_filter(&catalog, None, "p1", "p1", &b10037_inst, 6000, &f, None, None),
        "常時〚突撃〛を持つB10037は対象に含まれること"
    );

    // 条件付き突撃付与（例: 「〜場合、このキャラは〚突撃〛を持つ」や登場時突撃付与）はマッチしないこと
    let conditional_rush = models::CardDefinition {
        definition_id: "TEST_COND_RUSH".to_string(),
        canonical_id: "TEST_COND_RUSH".to_string(),
        names: vec!["テストキャラ".to_string()],
        card_type: models::CardType::CHARACTER,
        colors: vec!["白".to_string()],
        level: 4,
        ap: 4000,
        lp: 1,
        description: "自分の現場に〇〇がいる場合、このキャラは〚突撃〛を持つ。".to_string(),
        case_levels: None,
        main_thumb: None,
        main_path: None,
        traits: vec![],
    };
    catalog.definitions.insert("TEST_COND_RUSH".to_string(), conditional_rush);
    let cond_inst = test_inst("c_cond", "TEST_COND_RUSH", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE);
    assert!(
        !effects::card_matches_filter(&catalog, None, "p1", "p1", &cond_inst, 4000, &f, None, None),
        "条件を満たして突撃を得るキャラは『突撃を持つキャラ』に含まれないこと"
    );
}

#[test]
fn test_b10037_contact_win_requires_other_10000_ap_char() {
    let catalog = catalog::CardCatalog::load_from_json("../data/dummy/cards.json").unwrap();
    let mut engine = MatchEngine::new(catalog);
    engine.phase = models::MatchPhase::MAIN;
    engine.turn = 4;
    engine.turn_player_id = "p1".to_string();
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];

    let mut ps1 = engine::PlayerState::new("p1".to_string());
    let mut kyogoku = test_inst("c_kyogoku", "B10037", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE);
    kyogoku.entered_turn = 0;
    let mut ally = test_inst("c_ally", "B01001", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE);
    ally.entered_turn = 0;
    ps1.field = vec![kyogoku, ally];
    ps1.hand.push(test_inst("c_h1", "B01001", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];
    engine.players.insert("p1".to_string(), ps1);
    engine.players.insert("p2".to_string(), engine::PlayerState::new("p2".to_string()));

    // 1) 京極真自身とのコンタクト勝利では発動しないこと
    engine.trigger_on_contact_win("p1", "c_kyogoku", "B10037");
    assert!(
        engine.pending_decision.is_none(),
        "京極真自身とのコンタクト勝利ではB10037の上の効果は発動しないこと"
    );

    // 2) 他キャラでもAP10000未満（B01001はAP1000）とのコンタクト勝利では発動しないこと
    engine.trigger_on_contact_win("p1", "c_ally", "B01001");
    assert!(
        engine.pending_decision.is_none(),
        "AP10000未満の他キャラとのコンタクト勝利ではB10037の上の効果は発動しないこと"
    );

    // 3) 京極真以外のAP10000以上のキャラとのコンタクト勝利では発動すること
    engine.players.get_mut("p1").unwrap().field[1].temp_ap_bonus = 10000;
    engine.trigger_on_contact_win("p1", "c_ally", "B01001");
    assert!(
        engine.pending_decision.is_some(),
        "京極真以外のAP10000以上のキャラとのコンタクト勝利ではB10037の上の効果が発動すること"
    );
}

#[test]
fn test_b10038_treated_as_kaito_kid_in_deck_and_remove_only() {
    let catalog = catalog::CardCatalog::load_from_json("../data/dummy/cards.json").unwrap();
    let f_deck = effects::parse_target_filter(
        "〚カード名［怪盗キッド］〛のキャラ",
        effects::TargetZone::Deck,
        true,
        None,
    );
    let f_rem = effects::parse_target_filter(
        "〚カード名［怪盗キッド］〛のキャラ",
        effects::TargetZone::Remove,
        true,
        None,
    );

    let in_deck = test_inst("c_deck", "B10038", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE);
    assert!(
        effects::card_matches_filter(&catalog, None, "p1", "p1", &in_deck, 7000, &f_deck, None, None),
        "デッキにあるB10038は〚カード名［怪盗キッド］〛として扱われること"
    );

    let in_remove = test_inst("c_rem", "B10038", "p1", models::Zone::REMOVE, models::CardOrientation::ACTIVE);
    assert!(
        effects::card_matches_filter(&catalog, None, "p1", "p1", &in_remove, 7000, &f_rem, None, None),
        "リムーブエリアにあるB10038は〚カード名［怪盗キッド］〛として扱われること"
    );

    let on_field = test_inst("c_field", "B10038", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE);
    assert!(
        !effects::card_matches_filter(&catalog, None, "p1", "p1", &on_field, 7000, &f_rem, None, None),
        "現場にあるB10038は怪盗キッドとして扱われないこと"
    );
}

#[test]
fn test_b10098_declare_cost_sleeps_and_moves_self_to_partner_area() {
    let catalog = catalog::CardCatalog::load_from_json("../data/dummy/cards.json").unwrap();
    let mut engine = MatchEngine::new(catalog);
    engine.phase = models::MatchPhase::MAIN;
    engine.turn = 4;
    engine.turn_player_id = "p1".to_string();
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];

    let mut ps1 = engine::PlayerState::new("p1".to_string());
    let mut c_b10098 = test_inst("c_b10098", "B10098", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE);
    c_b10098.entered_turn = 0;
    ps1.field.push(c_b10098);
    // リムーブエリアに〚突撃〛を持つLv8以下の怪盗キッド/服部平次を用意（B10038はLv8なので対象外、B01001等は非対象。コスト確認用なので1枚なくても宣言自体は可能）
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];
    engine.players.insert("p1".to_string(), ps1);
    engine.players.insert("p2".to_string(), engine::PlayerState::new("p2".to_string()));
    engine.prompt_main_action("p1");

    let obs = build_player_observation(&engine, "p1");
    let dec_id = obs["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": dec_id,
        "revision": engine.revision,
        "value": { "type": "DECLARE", "actorId": "c_b10098" }
    })).unwrap();

    let p1 = &engine.players["p1"];
    assert!(p1.field.is_empty(), "B10098はコストで場から移動すること");
    assert_eq!(p1.extra_partners.len(), 1, "B10098はパートナーエリアに移ること");
    assert_eq!(p1.extra_partners[0].instance_id, "c_b10098");
    assert_eq!(p1.extra_partners[0].orientation, models::CardOrientation::SLEEP, "【スリープ】コストによりスリープ状態で移ること");
}

#[test]
fn test_b10098_revived_char_can_attack_active() {
    let catalog = catalog::CardCatalog::load_from_json("../data/dummy/cards.json").unwrap();
    let mut engine = MatchEngine::new(catalog);
    engine.phase = models::MatchPhase::MAIN;
    engine.turn = 4;
    engine.turn_player_id = "p1".to_string();
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];

    let mut ps1 = engine::PlayerState::new("p1".to_string());
    let mut c_b10098 = test_inst("c_b10098", "B10098", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE);
    c_b10098.entered_turn = 0;
    ps1.field.push(c_b10098);
    // リムーブエリアに Lv8・突撃・怪盗キッド扱い の B10038 を用意
    ps1.remove.push(test_inst("c_kid", "B10038", "p1", models::Zone::REMOVE, models::CardOrientation::ACTIVE));
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];
    engine.players.insert("p1".to_string(), ps1);
    engine.players.insert("p2".to_string(), engine::PlayerState::new("p2".to_string()));
    engine.prompt_main_action("p1");

    let obs1 = build_player_observation(&engine, "p1");
    let dec1 = obs1["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": dec1,
        "revision": engine.revision,
        "value": { "type": "DECLARE", "actorId": "c_b10098" }
    })).unwrap();

    let obs2 = build_player_observation(&engine, "p1");
    let b10036_line2 = engine.catalog.get("B10036").unwrap().description.lines().nth(1).unwrap().to_string();
    let b10036_plan = effects::parse_effect_steps(&b10036_line2, None, models::CardType::CHARACTER);
    assert_eq!(
        obs2["pendingDecision"]["kind"].as_str(),
        Some("targetSelect"),
        "logs: {:?}, plan: {:?}",
        engine.logs,
        b10036_plan.target_step
    );
    let dec2 = obs2["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": dec2,
        "revision": engine.revision,
        "value": { "targetId": "c_kid", "actorId": "c_b10098", "effectType": "REVIVE_ACTIVE" }
    })).unwrap();

    let revived = engine.players["p1"].field.iter().find(|c| c.instance_id == "c_kid").expect("B10038が登場していること");
    assert!(revived.can_attack_active, "B10098で登場させたキャラはアクティブ状態のキャラへアクションできること");
}

#[test]
fn test_b10098_partner_area_grants_contact_ap2000_on_both_turns() {
    let catalog = catalog::CardCatalog::load_from_json("../data/dummy/cards.json").unwrap();
    let mut engine = MatchEngine::new(catalog);
    engine.phase = models::MatchPhase::MAIN;
    engine.turn = 4;
    engine.turn_player_id = "p1".to_string();
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];

    let mut ps1 = engine::PlayerState::new("p1".to_string());
    ps1.extra_partners.push(test_inst("c_b10098", "B10098", "p1", models::Zone::PARTNER, models::CardOrientation::SLEEP));
    ps1.field.push(test_inst("c_lv8", "B10038", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));

    let mut ps2 = engine::PlayerState::new("p2".to_string());
    ps2.field.push(test_inst("p2_c1", "B10038", "p2", models::Zone::FIELD, models::CardOrientation::SLEEP));

    engine.player_order = vec!["p1".to_string(), "p2".to_string()];
    engine.players.insert("p1".to_string(), ps1);
    engine.players.insert("p2".to_string(), ps2);

    // 1) 自分ターン（攻撃側）でコンタクトしたとき +2000 されること
    let c_lv8_ref = &engine.players["p1"].field[0];
    assert_eq!(
        engine.get_effective_ap_with_contact("p1", c_lv8_ref, true),
        9000,
        "自分ターン中にパートナーエリアのB10098がLv8以上のキャラ(7000)にAP+2000すること"
    );

    // 2) 相手ターン（防御側）でコンタクトされたときも +2000 されること
    engine.turn_player_id = "p2".to_string();
    let c_lv8_ref2 = &engine.players["p1"].field[0];
    assert_eq!(
        engine.get_effective_ap_with_contact("p1", c_lv8_ref2, true),
        9000,
        "相手ターン中にパートナーエリアのB10098がLv8以上のキャラ(7000)にAP+2000すること"
    );
}

#[test]
fn test_b10036_forced_contact_uses_other_sleeping_ally() {
    let catalog = catalog::CardCatalog::load_from_json("../data/dummy/cards.json").unwrap();
    let mut engine = MatchEngine::new(catalog);
    engine.phase = models::MatchPhase::MAIN;
    engine.turn = 4;
    engine.turn_player_id = "p1".to_string();
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];

    let mut ps1 = engine::PlayerState::new("p1".to_string());
    ps1.case = Some(test_inst("p1_case", "B10101", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
    ps1.case_phase = models::CasePhase::RESOLUTION;
    ps1.partner = Some(test_inst("p1_p", "B10036P", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
    let mut sonoko = test_inst("c_sonoko", "B10036", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE);
    sonoko.entered_turn = 0;
    let mut sleeping_ally = test_inst("c_ally_sleep", "B10038", "p1", models::Zone::FIELD, models::CardOrientation::SLEEP);
    sleeping_ally.entered_turn = 0;
    ps1.field = vec![sonoko, sleeping_ally];

    let mut ps2 = engine::PlayerState::new("p2".to_string());
    ps2.field.push(test_inst("p2_c1", "B10038", "p2", models::Zone::FIELD, models::CardOrientation::SLEEP));
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];
    engine.players.insert("p1".to_string(), ps1);
    engine.players.insert("p2".to_string(), ps2);
    engine.prompt_main_action("p1");

    let obs1 = build_player_observation(&engine, "p1");
    let dec1 = obs1["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": dec1,
        "revision": engine.revision,
        "value": { "type": "DECLARE", "actorId": "c_sonoko" }
    })).unwrap();

    let obs2 = build_player_observation(&engine, "p1");
    let b10036_line2 = engine.catalog.get("B10036").unwrap().description.lines().nth(1).unwrap().to_string();
    let b10036_plan = effects::parse_effect_steps(&b10036_line2, None, models::CardType::CHARACTER);
    assert_eq!(
        obs2["pendingDecision"]["kind"].as_str(),
        Some("targetSelect"),
        "logs: {:?}, plan: {:?}",
        engine.logs,
        b10036_plan.target_step
    );
    let dec2 = obs2["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": dec2,
        "revision": engine.revision,
        "value": { "targetId": "p2_c1", "actorId": "c_sonoko", "effectType": "GRANT_CONTACT_FORCE_ATTACK" }
    })).unwrap();

    if let Some(ref ctx) = engine.action_context {
        assert_eq!(ctx["actorId"].as_str(), Some("c_ally_sleep"), "園子自身ではなく他のスリープ味方キャラがコンタクト元になること");
    }
}

#[test]
fn test_b10101_declare_grants_contact_win_ability_without_immediate_deck_search() {
    let catalog = load_test_catalog();
    let mut engine = MatchEngine::new(catalog);
    engine.phase = models::MatchPhase::MAIN;
    engine.turn = 8;
    engine.turn_player_id = "p1".to_string();
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];

    let mut ps1 = engine::PlayerState::new("p1".to_string());
    ps1.case = Some(test_inst("p1_case", "B10101", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
    ps1.case_phase = models::CasePhase::RESOLUTION;
    for i in 0..2 {
        let mut ev = test_inst(&format!("ev_{}", i), "B01001", "p1", models::Zone::EVIDENCE, models::CardOrientation::ACTIVE);
        ev.face_up = false;
        ps1.evidence.push(ev);
    }
    let mut heiji = test_inst("c_heiji", "B02018", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE);
    heiji.entered_turn = 0;
    ps1.field.push(heiji);
    for i in 0..4 {
        ps1.deck.push(test_inst(&format!("d_{}", i), "B01001", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
    }
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];
    engine.players.insert("p1".to_string(), ps1);
    engine.players.insert("p2".to_string(), engine::PlayerState::new("p2".to_string()));
    engine.prompt_main_action("p1");

    // 1) B10101 の【宣言】を発動（コスト: 裏向き証拠2つを表向きに）
    let obs1 = build_player_observation(&engine, "p1");
    let dec1 = obs1["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": dec1,
        "revision": engine.revision,
        "value": { "type": "DECLARE", "actorId": "p1_case" }
    })).unwrap();

    // 証拠表向きコスト選択 (FLIP_EVIDENCE_FACE_UP) が開くので2つ選択する
    let obs_cost = build_player_observation(&engine, "p1");
    assert_eq!(obs_cost["pendingDecision"]["kind"].as_str(), Some("targetSelect"));
    assert_eq!(obs_cost["pendingDecision"]["options"]["effectType"].as_str(), Some("FLIP_EVIDENCE_FACE_UP"));
    let dec_cost = obs_cost["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": dec_cost,
        "revision": engine.revision,
        "value": { "targetIds": ["ev_0", "ev_1"] }
    })).unwrap();

    // 証拠表向き直後に deckSearch が即座に開いてはならず、能力付与対象の targetSelect (GRANT_ABILITY) が開くこと
    let obs2 = build_player_observation(&engine, "p1");
    assert_eq!(
        obs2["pendingDecision"]["kind"].as_str(),
        Some("targetSelect"),
        "証拠を表向きにした瞬間にdeckSearchではなく対象キャラ選択が開くこと"
    );
    assert_eq!(
        obs2["pendingDecision"]["options"]["effectType"].as_str(),
        Some("GRANT_ABILITY")
    );
    let dec2 = obs2["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": dec2,
        "revision": engine.revision,
        "value": { "targetId": "c_heiji" }
    })).unwrap();

    // 付与完了後は mainAction に戻り、まだデッキは見ていないこと
    let obs3 = build_player_observation(&engine, "p1");
    assert_eq!(obs3["pendingDecision"]["kind"].as_str(), Some("mainAction"));
    assert_eq!(
        engine.granted_abilities_this_turn.get("c_heiji").map(|v| v.len()),
        Some(1),
        "対象キャラにターン終了時までの能力が付与されていること"
    );

    // 2) コンタクト勝利時に初めて deckSearch が発動すること
    engine.trigger_on_contact_win("p1", "c_heiji", "B02018");
    let obs4 = build_player_observation(&engine, "p1");
    assert_eq!(
        obs4["pendingDecision"]["kind"].as_str(),
        Some("deckSearch"),
        "コンタクトによって相手キャラをリムーブした瞬間に初めて上から4枚見るdeckSearchが発動すること"
    );
}

#[test]
fn test_b10037_triggers_when_ally_reaches_10000_ap_via_cutin_in_contact() {
    let catalog = load_test_catalog();
    let mut engine = MatchEngine::new(catalog);
    engine.phase = models::MatchPhase::MAIN;
    engine.turn = 8;
    engine.turn_player_id = "p1".to_string();
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];

    let mut ps1 = engine::PlayerState::new("p1".to_string());
    let mut b10037 = test_inst("c_b10037", "B10037", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE);
    b10037.entered_turn = 0;
    let mut attacker = test_inst("c_ally", "B01001", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE);
    attacker.entered_turn = 0;
    // B01001 (AP8000 等) がカットインで +2000 以上を受けて AP10000 以上になる状況を再現
    attacker.temp_ap_bonus = 0;
    ps1.field = vec![b10037, attacker];
    ps1.hand.push(test_inst("h_cutin", "B01002", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));

    let mut ps2 = engine::PlayerState::new("p2".to_string());
    ps2.field.push(test_inst("p2_def", "B01005", "p2", models::Zone::FIELD, models::CardOrientation::SLEEP));
    for i in 0..3 {
        ps2.evidence.push(test_inst(&format!("p2_ev{}", i), "B01005", "p2", models::Zone::EVIDENCE, models::CardOrientation::ACTIVE));
    }

    engine.player_order = vec!["p1".to_string(), "p2".to_string()];
    engine.players.insert("p1".to_string(), ps1);
    engine.players.insert("p2".to_string(), ps2);
    engine.contact_ap_buffs.insert("c_ally".to_string(), 3000);
    engine.action_context = Some(serde_json::json!({
        "attackerPlayerId": "p1",
        "defenderPlayerId": "p2",
        "actorId": "c_ally",
        "targetId": "p2_def"
    }));

    // proceed_to_action_resolution(None) を呼ぶと、バトル決着時は contact_ap_buffs を維持したまま trigger_on_contact_win が走り B10037 が発動すること
    engine.proceed_to_action_resolution(None);

    assert!(
        engine.pending_decision.is_some(),
        "カットイン込みでAP10000以上になった味方のコンタクト勝利でB10037の上の効果が発動すること"
    );
    assert!(
        engine.contact_ap_buffs.is_empty(),
        "コンタクト終了後には contact_ap_buffs がクリアされていること"
    );
}

#[test]
fn test_b09078_deck_search_dual_pick_and_conditional_discard() {
    let catalog = load_test_catalog();

    // ケース1: キャラとイベントの両方を1枚ずつ（計2枚）手札に加えた場合のみ手札リムーブが発生する
    let mut engine = MatchEngine::new(catalog.clone());
    engine.phase = models::MatchPhase::MAIN;
    engine.turn_player_id = "p1".to_string();
    let mut ps1 = engine::PlayerState::new("p1".to_string());
    ps1.partner = Some(test_inst("p1_p", "B01079", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
    ps1.case = Some(test_inst("p1_c", "PR115", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
    ps1.case_phase = models::CasePhase::INTRO;
    for i in 0..4 {
        ps1.file.push(test_inst(&format!("f{}", i), "B06089", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
    }
    ps1.field.push(test_inst("b09078", "B09078", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
    ps1.hand.push(test_inst("h_keep", "B06089", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
    // 山札の上3枚（末尾からポップされるため逆順）: B08068P(黄キャラ), D05014(黄イベント), B06089(黄キャラ)
    ps1.deck.push(test_inst("d3", "B06089", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
    ps1.deck.push(test_inst("d2", "D05014", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
    ps1.deck.push(test_inst("d1", "B08068P", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));

    let mut ps2 = engine::PlayerState::new("p2".to_string());
    ps2.partner = Some(test_inst("p2_p", "B01079", "p2", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
    ps2.case = Some(test_inst("p2_c", "PR115", "p2", models::Zone::CASE, models::CardOrientation::ACTIVE));
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];
    engine.players.insert("p1".to_string(), ps1);
    engine.players.insert("p2".to_string(), ps2);
    engine.resolve_deployed_characters(
        "p1",
        None,
        &[("b09078".to_string(), "B09078".to_string(), models::CardOrientation::ACTIVE)],
        "MAIN",
    );

    let dec = engine.pending_decision.as_ref().unwrap();
    assert_eq!(dec["kind"], "deckSearch");
    assert_eq!(dec["options"]["postAddDiscard"], 1);
    assert_eq!(dec["options"]["postAddDiscardMinPicked"], 2);
    assert_eq!(dec["options"]["maxSelect"], 2, "キャラ1枚とイベント1枚の計2枚まで選択可能であること");
    let revealed = dec["options"]["revealedCards"].as_array().unwrap();
    assert!(revealed.iter().all(|c| c["selectable"] == true), "黄キャラ・黄イベントすべてが選択可能であること");

    // キャラ1枚(d1)とイベント1枚(d2)の2枚を選択
    let ds_id = dec["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": ds_id,
        "revision": engine.revision,
        "value": {"chosenCardIds": ["d1", "d2"]}
    })).unwrap();

    let after_2 = engine.pending_decision.as_ref().unwrap();
    assert_eq!(engine.players["p1"].hand.len(), 3, "h_keep + d1 + d2 の3枚が手札にあること");
    assert_eq!(after_2["kind"], "targetSelect", "2枚手札に加えた場合は手札1枚リムーブが発生すること");

    // ケース2: 1枚だけ手札に加えた場合は手札リムーブが発生しない
    let mut engine2 = MatchEngine::new(catalog);
    engine2.phase = models::MatchPhase::MAIN;
    engine2.turn_player_id = "p1".to_string();
    let mut ps1_b = engine::PlayerState::new("p1".to_string());
    ps1_b.partner = Some(test_inst("p1_p", "B01079", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
    ps1_b.case = Some(test_inst("p1_c", "PR115", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
    ps1_b.case_phase = models::CasePhase::INTRO;
    for i in 0..4 {
        ps1_b.file.push(test_inst(&format!("f{}", i), "B06089", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
    }
    ps1_b.field.push(test_inst("b09078", "B09078", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
    ps1_b.hand.push(test_inst("h_keep", "B06089", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
    ps1_b.deck.push(test_inst("d3", "B06089", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
    ps1_b.deck.push(test_inst("d2", "D05014", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
    ps1_b.deck.push(test_inst("d1", "B08068P", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
    engine2.player_order = vec!["p1".to_string(), "p2".to_string()];
    engine2.players.insert("p1".to_string(), ps1_b);
    let mut ps2_b = engine::PlayerState::new("p2".to_string());
    ps2_b.partner = Some(test_inst("p2_p", "B01079", "p2", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
    ps2_b.case = Some(test_inst("p2_c", "PR115", "p2", models::Zone::CASE, models::CardOrientation::ACTIVE));
    engine2.players.insert("p2".to_string(), ps2_b);
    engine2.resolve_deployed_characters(
        "p1",
        None,
        &[("b09078".to_string(), "B09078".to_string(), models::CardOrientation::ACTIVE)],
        "MAIN",
    );

    let ds_id2 = engine2.pending_decision.as_ref().unwrap()["decisionId"].as_str().unwrap().to_string();
    engine2.submit_decision("p1", &serde_json::json!({
        "decisionId": ds_id2,
        "revision": engine2.revision,
        "value": {"chosenCardIds": ["d1"]}
    })).unwrap();

    let after_1 = engine2.pending_decision.as_ref().unwrap();
    assert_eq!(after_1["kind"], "mainAction", "1枚しか手札に加えていない場合は手札リムーブが発生せずメインアクションに戻ること");
}

#[test]
fn test_b08068_declare_reveal_hand_and_remove_opponent_only() {
    let catalog = load_test_catalog();
    let mut engine = MatchEngine::new(catalog);
    engine.phase = models::MatchPhase::MAIN;
    engine.turn_player_id = "p1".to_string();

    let mut ps1 = engine::PlayerState::new("p1".to_string());
    ps1.partner = Some(test_inst("p1_p", "B01079", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
    ps1.case = Some(test_inst("p1_c", "PR115", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
    ps1.field.push(test_inst("b08068", "B08068P", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
    ps1.field.push(test_inst("p1_poaro", "B09078", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
    ps1.hand.push(test_inst("h1", "B06089", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
    ps1.hand.push(test_inst("h2", "B06089", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));

    let mut ps2 = engine::PlayerState::new("p2".to_string());
    ps2.partner = Some(test_inst("p2_p", "B01079", "p2", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
    ps2.case = Some(test_inst("p2_c", "PR115", "p2", models::Zone::CASE, models::CardOrientation::ACTIVE));
    ps2.field.push(test_inst("p2_lv4", "B09078", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE));
    ps2.field.push(test_inst("p2_lv5", "PR163", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE));

    engine.player_order = vec!["p1".to_string(), "p2".to_string()];
    engine.players.insert("p1".to_string(), ps1);
    engine.players.insert("p2".to_string(), ps2);
    engine.prompt_main_action("p1");

    let desc = engine.catalog.get("B08068P").unwrap().description.clone();
    engine.execute_declare_clause(
        "p1",
        "b08068",
        "B08068P",
        false,
        None,
        models::CardOrientation::ACTIVE,
        &desc,
    );

    // 1. まず手札の喫茶ポアロキャラを公開するフェーズが開くこと
    let rev_dec = engine.pending_decision.as_ref().unwrap();
    assert_eq!(rev_dec["kind"], "targetSelect");
    assert_eq!(rev_dec["options"]["effectType"], "B08068_REVEAL_HAND");
    let rev_id = rev_dec["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": rev_id,
        "revision": engine.revision,
        "value": {"targetIds": ["h1", "h2"]}
    })).unwrap();

    // 2. 公開2枚 + 現場の喫茶ポアロ2枚 = Lv.4以下、かつ相手キャラのみが対象になること
    assert!(engine.logs.iter().any(|l| l.contains("計 2 枚")), "logs={:?}", engine.logs);
    let rem_dec = engine.pending_decision.as_ref().unwrap();
    assert_eq!(rem_dec["kind"], "targetSelect");
    assert_eq!(rem_dec["options"]["maxSelect"], 1);
    let cands = rem_dec["options"]["candidates"].as_array().unwrap();
    assert_eq!(cands.len(), 1);
    assert_eq!(cands[0]["instanceId"], "p2_lv4");

    let rem_id = rem_dec["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": rem_id,
        "revision": engine.revision,
        "value": {"targetIds": ["p2_lv4"]}
    })).unwrap();

    assert_eq!(engine.players["p1"].hand.len(), 2, "公開コストなので手札は減らないこと");
    assert_eq!(engine.players["p1"].field.len(), 2, "自分のキャラはリムーブされないこと");
    assert_eq!(engine.players["p2"].field.len(), 1, "相手のLv.4のみリムーブされること");
}

#[test]
fn test_b10036_forced_contact_picks_from_multiple_sleeping_allies() {
    let catalog = load_test_catalog();
    let mut engine = MatchEngine::new(catalog);
    engine.phase = models::MatchPhase::MAIN;
    engine.turn_player_id = "p1".to_string();
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];

    let mut ps1 = engine::PlayerState::new("p1".to_string());
    ps1.partner = Some(test_inst("p1_p", "PR250", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
    ps1.case = Some(test_inst("p1_c", "B10101", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
    ps1.field.push(test_inst("b10036", "B10036", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
    ps1.field.push(test_inst("ally_1", "B06067", "p1", models::Zone::FIELD, models::CardOrientation::SLEEP));
    ps1.field.push(test_inst("ally_2", "B10037", "p1", models::Zone::FIELD, models::CardOrientation::SLEEP));

    let mut ps2 = engine::PlayerState::new("p2".to_string());
    ps2.partner = Some(test_inst("p2_p", "B03106", "p2", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
    ps2.case = Some(test_inst("p2_c", "B06105P", "p2", models::Zone::CASE, models::CardOrientation::ACTIVE));
    ps2.field.push(test_inst("opp_target", "D07019", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE));

    engine.players.insert("p1".to_string(), ps1);
    engine.players.insert("p2".to_string(), ps2);

    let desc = engine.catalog.get("B10036").unwrap().description.clone();
    engine.execute_declare_clause("p1", "b10036", "B10036", false, None, models::CardOrientation::ACTIVE, &desc);

    // 1. まず相手キャラを選ぶ
    let dec1 = engine.pending_decision.as_ref().unwrap();
    assert_eq!(dec1["kind"], "targetSelect");
    let id1 = dec1["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": id1,
        "revision": engine.revision,
        "value": {"targetIds": ["opp_target"]}
    })).unwrap();

    // 2. 自分の場にスリープキャラが2体いるため、コンタクトさせるキャラを選ぶフェーズが開くこと
    let dec2 = engine.pending_decision.as_ref().unwrap();
    assert_eq!(dec2["kind"], "targetSelect");
    assert_eq!(dec2["options"]["effectType"], "FORCE_CONTACT_PICK_ALLY");
    let cands = dec2["options"]["candidates"].as_array().unwrap();
    assert_eq!(cands.len(), 2);

    // ally_2 (B10037) を選んでコンタクトを発生させる
    let id2 = dec2["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": id2,
        "revision": engine.revision,
        "value": {"targetIds": ["ally_2"]}
    })).unwrap();

    assert!(
        engine.logs.iter().any(|l| l.contains("京極真(B10037)") && l.contains("コンタクト（バトル）が発生")),
        "選んだ ally_2 (京極真) でコンタクトが発生すること"
    );
}

#[test]
fn test_b07081_on_enter_recover_event_only_discards_if_hand_at_least_6() {
    let catalog = load_test_catalog();

    // ケース1: 加えた後の手札が5枚 -> リムーブは発生しない
    let mut engine1 = MatchEngine::new(catalog.clone());
    engine1.phase = models::MatchPhase::MAIN;
    engine1.turn_player_id = "p1".to_string();
    engine1.player_order = vec!["p1".to_string(), "p2".to_string()];

    let mut ps1 = engine::PlayerState::new("p1".to_string());
    ps1.partner = Some(test_inst("p1_p", "B01079", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
    ps1.case = Some(test_inst("p1_c", "PR115", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
    ps1.field.push(test_inst("b07081", "B07081", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
    for i in 0..4 {
        ps1.hand.push(test_inst(&format!("h{}", i), "B06089", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
    }
    ps1.remove.push(test_inst("rem_ev", "D05014", "p1", models::Zone::REMOVE, models::CardOrientation::ACTIVE));

    engine1.players.insert("p1".to_string(), ps1);
    engine1.players.insert("p2".to_string(), engine::PlayerState::new("p2".to_string()));

    engine1.resolve_deployed_characters(
        "p1",
        None,
        &[("b07081".to_string(), "B07081".to_string(), models::CardOrientation::ACTIVE)],
        "MAIN",
    );

    let dec1 = engine1.pending_decision.as_ref().unwrap();
    let id1 = dec1["decisionId"].as_str().unwrap().to_string();
    engine1.submit_decision("p1", &serde_json::json!({
        "decisionId": id1,
        "revision": engine1.revision,
        "value": {"targetIds": ["rem_ev"]}
    })).unwrap();

    assert_eq!(engine1.players["p1"].hand.len(), 5, "手札は5枚になること");
    assert_eq!(
        engine1.pending_decision.as_ref().unwrap()["kind"],
        "mainAction",
        "手札が5枚（6枚未満）のときは手札リムーブが発生しないこと dec={:?}",
        engine1.pending_decision
    );

    // ケース2: 加えた後の手札が6枚 -> 手札1枚リムーブが発生する
    let mut engine2 = MatchEngine::new(catalog);
    engine2.phase = models::MatchPhase::MAIN;
    engine2.turn_player_id = "p1".to_string();
    engine2.player_order = vec!["p1".to_string(), "p2".to_string()];

    let mut ps1_b = engine::PlayerState::new("p1".to_string());
    ps1_b.partner = Some(test_inst("p1_p", "B01079", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
    ps1_b.case = Some(test_inst("p1_c", "PR115", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
    ps1_b.field.push(test_inst("b07081", "B07081", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
    for i in 0..5 {
        ps1_b.hand.push(test_inst(&format!("h{}", i), "B06089", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
    }
    ps1_b.remove.push(test_inst("rem_ev", "D05014", "p1", models::Zone::REMOVE, models::CardOrientation::ACTIVE));

    engine2.players.insert("p1".to_string(), ps1_b);
    engine2.players.insert("p2".to_string(), engine::PlayerState::new("p2".to_string()));

    engine2.resolve_deployed_characters(
        "p1",
        None,
        &[("b07081".to_string(), "B07081".to_string(), models::CardOrientation::ACTIVE)],
        "MAIN",
    );

    let dec2 = engine2.pending_decision.as_ref().unwrap();
    let id2 = dec2["decisionId"].as_str().unwrap().to_string();
    engine2.submit_decision("p1", &serde_json::json!({
        "decisionId": id2,
        "revision": engine2.revision,
        "value": {"targetIds": ["rem_ev"]}
    })).unwrap();

    assert_eq!(engine2.players["p1"].hand.len(), 6, "手札は6枚になること");
    assert_eq!(
        engine2.pending_decision.as_ref().unwrap()["kind"],
        "targetSelect",
        "手札が6枚以上のときは手札1枚リムーブが発生すること"
    );
}

#[test]
fn test_b08029_grants_can_attack_active_to_deployed_char_even_after_recover() {
    let catalog = load_test_catalog();
    let mut engine = MatchEngine::new(catalog);
    engine.phase = models::MatchPhase::MAIN;
    engine.turn_player_id = "p1".to_string();
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];

    let mut ps1 = engine::PlayerState::new("p1".to_string());
    ps1.partner = Some(test_inst("p1_p", "PR220", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
    ps1.case = Some(test_inst("p1_c", "B08030", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
    ps1.remove.push(test_inst("rem_iori", "B11020", "p1", models::Zone::REMOVE, models::CardOrientation::ACTIVE));
    ps1.remove.push(test_inst("rem_momiji", "D02011", "p1", models::Zone::REMOVE, models::CardOrientation::ACTIVE));

    let mut ps2 = engine::PlayerState::new("p2".to_string());
    ps2.partner = Some(test_inst("p2_p", "B03001", "p2", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
    ps2.case = Some(test_inst("p2_c", "D08026", "p2", models::Zone::CASE, models::CardOrientation::ACTIVE));
    ps2.field.push(test_inst("opp_active", "B01016", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE));

    engine.players.insert("p1".to_string(), ps1);
    engine.players.insert("p2".to_string(), ps2);

    let desc = engine.catalog.get("B08029").unwrap().description.clone();
    engine.execute_declare_clause("p1", "ev_b08029", "B08029", false, None, models::CardOrientation::ACTIVE, &desc);

    // 1. リムーブから伊織無我を登場させる
    let dec1 = engine.pending_decision.as_ref().unwrap();
    let id1 = dec1["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": id1,
        "revision": engine.revision,
        "value": {"targetIds": ["rem_iori"]}
    })).unwrap();

    // 2. リムーブから大岡紅葉を手札に加える
    let dec2 = engine.pending_decision.as_ref().unwrap();
    let id2 = dec2["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": id2,
        "revision": engine.revision,
        "value": {"targetIds": ["rem_momiji"]}
    })).unwrap();

    // 3. モード選択で「アクティブ状態のキャラを指定してアクションできる」を選ぶ
    let dec3 = engine.pending_decision.as_ref().unwrap();
    assert_eq!(dec3["kind"], "modeSelect");
    let id3 = dec3["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": id3,
        "revision": engine.revision,
        "value": {"modeIndex": 1}
    })).unwrap();

    if let Some(dec4) = engine.pending_decision.as_ref() {
        if dec4["kind"] == "targetSelect" {
            let id4 = dec4["decisionId"].as_str().unwrap().to_string();
            engine.submit_decision("p1", &serde_json::json!({
                "decisionId": id4,
                "revision": engine.revision,
                "value": {"targetIds": ["rem_iori"]}
            })).unwrap();
        }
    }

    // 伊織無我(rem_iori)に can_attack_active が付与されていること
    let iori = engine.players["p1"].field.iter().find(|c| c.instance_id == "rem_iori").unwrap();
    assert!(iori.can_attack_active, "登場させた伊織無我にアクティブ指定アクション権が付与されること");
}

#[test]
fn test_b08029_grants_rush_char_to_deployed_char_even_after_skipped_recover() {
    let catalog = load_test_catalog();
    let mut engine = MatchEngine::new(catalog);
    engine.phase = models::MatchPhase::MAIN;
    engine.turn_player_id = "p1".to_string();
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];

    let mut ps1 = engine::PlayerState::new("p1".to_string());
    ps1.partner = Some(test_inst("p1_p", "PR220", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
    ps1.case = Some(test_inst("p1_c", "B08030", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
    ps1.remove.push(test_inst("rem_iori", "B09026", "p1", models::Zone::REMOVE, models::CardOrientation::ACTIVE));

    let mut ps2 = engine::PlayerState::new("p2".to_string());
    ps2.partner = Some(test_inst("p2_p", "B03001", "p2", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
    ps2.case = Some(test_inst("p2_c", "D08026", "p2", models::Zone::CASE, models::CardOrientation::ACTIVE));

    engine.players.insert("p1".to_string(), ps1);
    engine.players.insert("p2".to_string(), ps2);

    let desc = engine.catalog.get("B08029").unwrap().description.clone();
    engine.execute_declare_clause("p1", "ev_b08029", "B08029", false, None, models::CardOrientation::ACTIVE, &desc);

    // 1. リムーブから伊織無我(B09026)を登場させる（大岡紅葉はリムーブにないのでスキップされる）
    let dec1 = engine.pending_decision.as_ref().unwrap();
    let id1 = dec1["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": id1,
        "revision": engine.revision,
        "value": {"targetIds": ["rem_iori"]}
    })).unwrap();

    // 2. モード選択で「突撃［キャラ］を与える」(modeIndex: 0)を選ぶ
    let dec2 = engine.pending_decision.as_ref().unwrap();
    assert_eq!(dec2["kind"], "modeSelect");
    let id2 = dec2["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": id2,
        "revision": engine.revision,
        "value": {"modeIndex": 0}
    })).unwrap();

    if let Some(dec3) = engine.pending_decision.as_ref() {
        if dec3["kind"] == "targetSelect" {
            let id3 = dec3["decisionId"].as_str().unwrap().to_string();
            engine.submit_decision("p1", &serde_json::json!({
                "decisionId": id3,
                "revision": engine.revision,
                "value": {"targetIds": ["rem_iori"]}
            })).unwrap();
        }
    }

    let iori = engine.players["p1"].field.iter().find(|c| c.instance_id == "rem_iori").unwrap();
    assert!(iori.has_rush, "登場させた伊織無我に突撃が付与されること");
    assert_eq!(iori.rush_target.as_deref(), Some("CHAR"), "突撃［キャラ］であること");
}

#[test]
fn test_b08023_on_enter_mode_select() {
    let catalog = load_test_catalog();
    let mut engine = MatchEngine::new(catalog);
    engine.phase = models::MatchPhase::MAIN;
    engine.turn = 6;
    engine.turn_player_id = "p1".to_string();
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];

    let mut ps1 = engine::PlayerState::new("p1".to_string());
    ps1.field.push(test_inst("c_b08023", "B08023", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));

    let mut ps2 = engine::PlayerState::new("p2".to_string());
    ps2.field.push(test_inst("opp_c", "B11009", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE));
    ps2.deck.push(test_inst("opp_d1", "B11009", "p2", models::Zone::DECK, models::CardOrientation::ACTIVE));

    engine.players.insert("p1".to_string(), ps1);
    engine.players.insert("p2".to_string(), ps2);

    engine.resolve_deployed_characters(
        "p1",
        None,
        &[("c_b08023".to_string(), "B08023".to_string(), models::CardOrientation::ACTIVE)],
        "MAIN",
    );

    let dec1 = engine.pending_decision.as_ref().unwrap();
    assert_eq!(dec1["kind"], "modeSelect", "B08023登場時にmodeSelectが開くこと");
    assert_eq!(dec1["options"]["modes"].as_array().unwrap().len(), 3);
    let id1 = dec1["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": id1,
        "revision": engine.revision,
        "value": {"modeIndex": 2}
    })).unwrap();

    let dec2 = engine.pending_decision.as_ref().unwrap();
    assert_eq!(dec2["kind"], "targetSelect");
    let id2 = dec2["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": id2,
        "revision": engine.revision,
        "value": {"targetIds": ["opp_c"]}
    })).unwrap();

    let opp_c = &engine.players["p2"].field[0];
    assert_eq!(opp_c.set_cards.len(), 1, "相手のキャラに1枚裏向きでセットされること");
    assert!(!opp_c.set_cards[0].face_up);
    assert_eq!(opp_c.orientation, models::CardOrientation::SLEEP, "相手のキャラがスリープすること");
}


