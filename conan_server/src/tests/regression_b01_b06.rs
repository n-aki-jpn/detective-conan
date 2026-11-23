use super::*;

#[test]
fn test_b03006_shounen_tanteidan_on_enter_stack() {
    let catalog = catalog::CardCatalog::load_from_json("../data/dummy/cards.json").unwrap();
    let card = catalog.get("B03006").expect("B03006 must exist in catalog");
    let clauses = effects::extract_clauses(&card.description, effects::TriggerKind::OnPlay, models::CardType::CHARACTER);
    assert!(!clauses.is_empty(), "Must extract 【登場時】");
    let plan = effects::parse_effect_steps(&clauses[0], Some(8), models::CardType::CHARACTER);
    let ts = plan.target_step.as_ref().expect("Must have target_step");
    assert_eq!(ts.action, effects::TargetAction::StackUnderSelf);
    assert_eq!(ts.filter.zone, effects::TargetZone::Remove);
    assert_eq!(ts.filter.count, 5);
}

#[test]
fn test_b06103_gin_hand_declare() {
    let catalog = catalog::CardCatalog::load_from_json("../data/dummy/cards.json").unwrap();
    let mut engine = engine::MatchEngine::new(catalog.clone());
    engine.turn = 2;
    engine.turn_player_id = "p1".to_string();
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];

    let mut ps1 = engine::PlayerState::new("p1".to_string());
    let mut cs = models::CardInstance {
        instance_id: "case1".to_string(),
        definition_id: "B01002".to_string(),
        owner_id: "p1".to_string(),
        zone: models::Zone::CASE,
        ..Default::default()
    };
    cs.orientation = models::CardOrientation::ACTIVE;
    ps1.r#case = Some(cs);
    ps1.case_phase = models::CasePhase::RESOLUTION;

    ps1.field.push(models::CardInstance {
        instance_id: "blk_1".to_string(),
        definition_id: "B11085".to_string(),
        owner_id: "p1".to_string(),
        zone: models::Zone::FIELD,
        ..Default::default()
    });
    ps1.hand.push(models::CardInstance {
        instance_id: "gin_h".to_string(),
        definition_id: "B06103".to_string(),
        owner_id: "p1".to_string(),
        zone: models::Zone::HAND,
        ..Default::default()
    });
    engine.players.insert("p1".to_string(), ps1);
    engine.players.insert("p2".to_string(), engine::PlayerState::new("p2".to_string()));
    engine.prompt_main_action("p1");

    let b06103 = catalog.get("B06103").unwrap();
    let clauses = effects::extract_clauses(&b06103.description, effects::TriggerKind::Declare, b06103.card_type);
    assert!(!clauses.is_empty(), "B06103 must have Declare clause: {}", b06103.description);
    let obs = build_player_observation(&engine, "p1");
    let actions = obs["pendingDecision"]["options"]["actions"]
        .as_array()
        .expect("pendingDecision.options.actions must exist");
    let declare_action = actions.iter().find(|a| a["type"] == "DECLARE")
        .unwrap_or_else(|| panic!("DECLARE action must exist; clauses={:?}, actions={:?}", clauses, actions));
    let actors = declare_action["actors"].as_array().expect("actors must exist");
    assert!(
        actors.iter().any(|a| a["actorId"].as_str() == Some("gin_h")),
        "手札のB06103ジンがDECLAREのactorsに含まれること: {:?}",
        actors
    );

    // 実際にDECLAREを実行し、現場の黒キャラをリムーブした後にgin_h自身がスリープ状態で登場することを確認
    let dec_id = obs["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": dec_id,
        "revision": engine.revision,
        "value": { "type": "DECLARE", "actorId": "gin_h" }
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
    let ts_id = obs2["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": ts_id,
        "revision": engine.revision,
        "value": { "targetId": "blk_1" }
    })).unwrap();

    let ps1_after = &engine.players["p1"];
    assert!(ps1_after.remove.iter().any(|c| c.instance_id == "blk_1"), "blk_1がリムーブされていること");
    let gin_on_field = ps1_after.field.iter().find(|c| c.instance_id == "gin_h")
        .expect("gin_hが現場に登場していること");
    assert_eq!(gin_on_field.orientation, models::CardOrientation::SLEEP, "gin_hがスリープ状態で登場していること");

    // 場に出た後のgin_hは「この能力はこのカードが手札にあり」の宣言対象にならないこと
    let obs3 = build_player_observation(&engine, "p1");
    let declare_after = obs3["pendingDecision"]["options"]["actions"]
        .as_array()
        .and_then(|arr| arr.iter().find(|a| a["type"] == "DECLARE"));
    let actors_after: Vec<&str> = declare_after
        .and_then(|d| d["actors"].as_array())
        .map(|arr| arr.iter().filter_map(|a| a["actorId"].as_str()).collect())
        .unwrap_or_default();
    assert!(
        !actors_after.contains(&"gin_h"),
        "場に出た後のgin_hはDECLAREのactorsに含まれないこと: {:?}",
        actors_after
    );
}

/// B06103 ジンの「このターン中、自分は〚カード名［ジン］〛を使用できず、登場させることができない」の回帰テスト
/// 手札にジンが2枚ある状態で1枚目を宣言・登場させた後、2枚目は同じターンに宣言・プレイできなくなる

#[test]
fn test_b06103_gin_turn_name_ban_second_hand_jin() {
    let catalog = catalog::CardCatalog::load_from_json("../data/dummy/cards.json").unwrap();
    let mut engine = engine::MatchEngine::new(catalog.clone());
    engine.turn = 2;
    engine.turn_player_id = "p1".to_string();
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];

    let mut ps1 = engine::PlayerState::new("p1".to_string());
    let mut cs = models::CardInstance {
        instance_id: "case1".to_string(),
        definition_id: "B01002".to_string(),
        owner_id: "p1".to_string(),
        zone: models::Zone::CASE,
        ..Default::default()
    };
    cs.orientation = models::CardOrientation::ACTIVE;
    ps1.r#case = Some(cs);
    ps1.case_phase = models::CasePhase::RESOLUTION;

    ps1.field.push(models::CardInstance {
        instance_id: "blk_1".to_string(),
        definition_id: "B11085".to_string(),
        owner_id: "p1".to_string(),
        zone: models::Zone::FIELD,
        ..Default::default()
    });
    ps1.hand.push(models::CardInstance {
        instance_id: "gin_h".to_string(),
        definition_id: "B06103".to_string(),
        owner_id: "p1".to_string(),
        zone: models::Zone::HAND,
        ..Default::default()
    });
    ps1.hand.push(models::CardInstance {
        instance_id: "jin2_h".to_string(),
        definition_id: "B06103".to_string(),
        owner_id: "p1".to_string(),
        zone: models::Zone::HAND,
        ..Default::default()
    });
    engine.players.insert("p1".to_string(), ps1);
    engine.players.insert("p2".to_string(), engine::PlayerState::new("p2".to_string()));
    engine.prompt_main_action("p1");

    // 宣言前に両方のジンがDECLARE候補に含まれること
    let obs = build_player_observation(&engine, "p1");
    let actions = obs["pendingDecision"]["options"]["actions"]
        .as_array()
        .expect("actions must exist");
    let declare_action = actions.iter().find(|a| a["type"] == "DECLARE")
        .expect("DECLARE action must exist");
    let actors: Vec<&str> = declare_action["actors"].as_array().unwrap()
        .iter().filter_map(|a| a["actorId"].as_str()).collect();
    assert!(actors.contains(&"gin_h") && actors.contains(&"jin2_h"),
        "宣言前に両方のジンがDECLARE候補になること: {:?}", actors);

    // 1枚目のジンを宣言し、現場の黒キャラをリムーブして登場させる
    let dec_id = obs["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": dec_id,
        "revision": engine.revision,
        "value": { "type": "DECLARE", "actorId": "gin_h" }
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
    let ts_id = obs2["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": ts_id,
        "revision": engine.revision,
        "value": { "targetId": "blk_1" }
    })).unwrap();

    // 効果解決後、ターン内BANが登録されていること
    assert!(
        engine.turn_name_bans.get("p1").map(|b| b.iter().any(|n| n.contains("ジン"))).unwrap_or(false),
        "ターン内・カード名BAN（ジン）がp1に登録されていること: {:?}", engine.turn_name_bans
    );

    // 2枚目のジン(jin2_h)は同一ターンにDECLARE候補・HAND_PLAY候補に含まれないこと
    let obs3 = build_player_observation(&engine, "p1");
    let actions3 = obs3["pendingDecision"]["options"]["actions"]
        .as_array()
        .expect("actions must exist after resolve");
    let declare_after = actions3.iter().find(|a| a["type"] == "DECLARE");
    let actors_after: Vec<&str> = declare_after
        .and_then(|d| d["actors"].as_array())
        .map(|arr| arr.iter().filter_map(|a| a["actorId"].as_str()).collect())
        .unwrap_or_default();
    assert!(
        !actors_after.contains(&"jin2_h"),
        "BAN中のjin2_hはDECLARE候補に含まれないこと: {:?}",
        actors_after
    );
    let hand_play_after = actions3.iter().find(|a| a["type"] == "HAND_PLAY");
    let hand_ids: Vec<&str> = hand_play_after
        .and_then(|h| h["cardIds"].as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    assert!(
        !hand_ids.contains(&"jin2_h"),
        "BAN中のjin2_hはHAND_PLAY候補に含まれないこと: {:?}",
        hand_ids
    );
}

#[test]
fn test_d08026_ao_no_kojo_declare() {
    let catalog = catalog::CardCatalog::load_from_json("../data/dummy/cards.json").unwrap();
    let mut engine = engine::MatchEngine::new(catalog.clone());
    engine.turn = 2;
    engine.turn_player_id = "p1".to_string();
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];

    let mut ps1 = engine::PlayerState::new("p1".to_string());
    let mut cs = models::CardInstance {
        instance_id: "case_1".to_string(),
        definition_id: "D08026".to_string(),
        owner_id: "p1".to_string(),
        zone: models::Zone::CASE,
        ..Default::default()
    };
    cs.orientation = models::CardOrientation::ACTIVE;
    ps1.r#case = Some(cs);
    ps1.case_phase = models::CasePhase::RESOLUTION;
    ps1.evidence.push(models::CardInstance {
        instance_id: "ev_1".to_string(),
        definition_id: "B01002".to_string(),
        owner_id: "p1".to_string(),
        zone: models::Zone::EVIDENCE,
        face_up: false,
        ..Default::default()
    });
    ps1.field.push(models::CardInstance {
        instance_id: "haibara_1".to_string(),
        definition_id: "B01016".to_string(),
        owner_id: "p1".to_string(),
        zone: models::Zone::FIELD,
        ..Default::default()
    });
    engine.players.insert("p1".to_string(), ps1);
    engine.players.insert("p2".to_string(), engine::PlayerState::new("p2".to_string()));
    engine.prompt_main_action("p1");

    let obs = build_player_observation(&engine, "p1");
    let dec_id = obs["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": dec_id,
        "revision": engine.revision,
        "value": { "type": "DECLARE", "actorId": "case_1" }
    })).unwrap();

    let d08026 = catalog.get("D08026").unwrap();
    let clauses = effects::extract_clauses(&d08026.description, effects::TriggerKind::Declare, d08026.card_type);
    let plan = effects::parse_effect_steps(&clauses[0], None, d08026.card_type);

    let obs2 = build_player_observation(&engine, "p1");
    assert_eq!(
        obs2["pendingDecision"]["kind"].as_str(),
        Some("targetSelect"),
        "D08026青の古城探索事件の宣言後に少年探偵団キャラの対象選択が出ること (plan: {:?}, logs: {:?})",
        plan,
        engine.logs
    );
}

#[test]
fn test_d08026_ap_scales_with_flipped_evidence() {
    let catalog = catalog::CardCatalog::load_from_json("../data/dummy/cards.json").unwrap();
    let mut engine = engine::MatchEngine::new(catalog.clone());
    engine.turn = 2;
    engine.turn_player_id = "p1".to_string();
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];

    let mut ps1 = engine::PlayerState::new("p1".to_string());
    ps1.r#case = Some(models::CardInstance {
        instance_id: "case_1".to_string(),
        definition_id: "D08026".to_string(),
        owner_id: "p1".to_string(),
        zone: models::Zone::CASE,
        orientation: models::CardOrientation::ACTIVE,
        ..Default::default()
    });
    ps1.case_phase = models::CasePhase::RESOLUTION;
    for i in 0..3 {
        ps1.evidence.push(models::CardInstance {
            instance_id: format!("ev_{}", i),
            definition_id: "B01002".to_string(),
            owner_id: "p1".to_string(),
            zone: models::Zone::EVIDENCE,
            face_up: false,
            ..Default::default()
        });
    }
    ps1.field.push(models::CardInstance {
        instance_id: "haibara_1".to_string(),
        definition_id: "B01016".to_string(),
        owner_id: "p1".to_string(),
        zone: models::Zone::FIELD,
        ..Default::default()
    });
    engine.players.insert("p1".to_string(), ps1);
    engine.players.insert("p2".to_string(), engine::PlayerState::new("p2".to_string()));
    engine.prompt_main_action("p1");

    let base_ap = {
        let c = engine.players["p1"].field[0].clone();
        engine.get_effective_ap("p1", &c)
    };

    let obs = build_player_observation(&engine, "p1");
    let dec_id = obs["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": dec_id,
        "revision": engine.revision,
        "value": { "type": "DECLARE", "actorId": "case_1" }
    })).unwrap();

    let obs2 = build_player_observation(&engine, "p1");
    eprintln!("PD1: {}", obs2["pendingDecision"]);
    let dec_id = obs2["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": dec_id,
        "revision": engine.revision,
        "value": { "targetIds": ["ev_0", "ev_1", "ev_2"] }
    })).unwrap();

    let obs3 = build_player_observation(&engine, "p1");
    eprintln!("PD2: {}", obs3["pendingDecision"]);
    let dec_id = obs3["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": dec_id,
        "revision": engine.revision,
        "value": { "targetId": "haibara_1" }
    })).unwrap();

    let new_ap = {
        let c = engine.players["p1"].field[0].clone();
        engine.get_effective_ap("p1", &c)
    };
    assert_eq!(new_ap - base_ap, 3000, "3枚表向きなら+3000 (logs: {:?})", engine.logs);
}

#[test]
fn test_b03116_vodka_revive_on_gin_remove() {
    let catalog = catalog::CardCatalog::load_from_json("../data/dummy/cards.json").unwrap();
    let mut engine = engine::MatchEngine::new(catalog.clone());
    engine.turn = 2;
    engine.turn_player_id = "p1".to_string();
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];

    let mut ps1 = engine::PlayerState::new("p1".to_string());
    ps1.case_phase = models::CasePhase::RESOLUTION;
    ps1.partner = Some(models::CardInstance {
        instance_id: "p1_partner".to_string(),
        definition_id: "B03107".to_string(), // 黒パートナー
        owner_id: "p1".to_string(),
        zone: models::Zone::PARTNER,
        ..Default::default()
    });
    ps1.field.push(models::CardInstance {
        instance_id: "vodka_1".to_string(),
        definition_id: "B03116".to_string(),
        owner_id: "p1".to_string(),
        zone: models::Zone::FIELD,
        ..Default::default()
    });
    ps1.hand.push(models::CardInstance {
        instance_id: "gin_h".to_string(),
        definition_id: "B06103".to_string(),
        owner_id: "p1".to_string(),
        zone: models::Zone::HAND,
        ..Default::default()
    });
    ps1.deck.push(models::CardInstance {
        instance_id: "d_1".to_string(),
        definition_id: "B01002".to_string(),
        owner_id: "p1".to_string(),
        zone: models::Zone::DECK,
        ..Default::default()
    });
    engine.players.insert("p1".to_string(), ps1);
    engine.players.insert("p2".to_string(), engine::PlayerState::new("p2".to_string()));
    engine.prompt_main_action("p1");

    let obs = build_player_observation(&engine, "p1");
    let dec_id = obs["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": dec_id,
        "revision": engine.revision,
        "value": { "type": "DECLARE", "actorId": "gin_h" }
    })).unwrap();

    let obs2 = build_player_observation(&engine, "p1");
    let ts_id = obs2["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": ts_id,
        "revision": engine.revision,
        "value": { "targetId": "vodka_1" }
    })).unwrap();

    let obs3 = build_player_observation(&engine, "p1");
    assert_eq!(obs3["pendingDecision"]["kind"].as_str(), Some("targetSelect"));
    let rev_id = obs3["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": rev_id,
        "revision": engine.revision,
        "value": { "targetId": "vodka_1" }
    })).unwrap();

    let ps1_after = &engine.players["p1"];
    assert!(
        ps1_after.field.iter().any(|c| c.instance_id == "vodka_1"),
        "B03116ウォッカがリムーブエリアから現場に復活していること (logs: {:?})",
        engine.logs
    );
    assert!(
        ps1_after.field.iter().any(|c| c.instance_id == "gin_h"),
        "B06103ジンも手札から現場に登場していること (logs: {:?})",
        engine.logs
    );
}

#[test]
fn test_b03006_rush_with_set_cards() {
    let catalog = catalog::CardCatalog::load_from_json("../data/dummy/cards.json").unwrap();
    let mut engine = engine::MatchEngine::new(catalog.clone());
    engine.turn = 8;
    engine.turn_player_id = "p1".to_string();
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];

    let mut sd = models::CardInstance {
        instance_id: "sd_1".to_string(),
        definition_id: "B03006".to_string(),
        owner_id: "p1".to_string(),
        zone: models::Zone::FIELD,
        orientation: models::CardOrientation::ACTIVE,
        face_up: true,
        entered_turn: 8,
        ..Default::default()
    };
    for i in 0..5 {
        let sc = models::CardInstance {
            instance_id: format!("sc_{}", i),
            definition_id: "B01001".to_string(),
            owner_id: "p1".to_string(),
            zone: models::Zone::FIELD,
            face_up: true,
            ..Default::default()
        };
        sd.set_cards.push(sc);
    }
    let mut ps1 = engine::PlayerState::new("p1".to_string());
    ps1.field.push(sd);
    engine.players.insert("p1".to_string(), ps1);
    let mut ps2 = engine::PlayerState::new("p2".to_string());
    ps2.field.push(models::CardInstance {
        instance_id: "p2_char".to_string(),
        definition_id: "B01001".to_string(),
        owner_id: "p2".to_string(),
        zone: models::Zone::FIELD,
        orientation: models::CardOrientation::SLEEP,
        face_up: true,
        ..Default::default()
    });
    engine.players.insert("p2".to_string(), ps2);
    engine.prompt_main_action("p1");

    let ps1_ref = &engine.players["p1"];
    let (has_rush, rush_target) = engine.effective_rush(&ps1_ref.field[0]);
    assert!(has_rush, "B03006に5枚セットされている場合は突撃が有効になること (rush_target={:?})", rush_target);
    let obs = build_player_observation(&engine, "p1");
    let actions = obs["pendingDecision"]["options"]["actions"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let action_opt = actions.iter().find(|o| o["type"].as_str() == Some("ACTION"));
    let actors = action_opt
        .and_then(|o| o["actorIds"].as_array())
        .cloned()
        .unwrap_or_default();
    assert!(
        actors.iter().any(|a| a.as_str() == Some("sd_1")),
        "sd_1がACTIONのactorIdsに含まれること: {:?}",
        actors
    );
}

#[test]
fn test_b05005_declare_grants_must_guard() {
    let catalog = catalog::CardCatalog::load_from_json("../data/dummy/cards.json").unwrap();
    let desc = catalog.get("B05005").unwrap().description.clone();
    let clauses = effects::extract_clauses(&desc, effects::TriggerKind::Declare, models::CardType::CHARACTER);
    assert!(!clauses.is_empty());
    let plan = effects::parse_effect_steps(&clauses[0], None, models::CardType::CHARACTER);
    let ts = plan.target_step.as_ref().unwrap_or_else(|| panic!("target_step必須: {:?}", plan));
    assert_eq!(ts.action, effects::TargetAction::GiveMustGuard);
    assert_eq!(ts.filter.owner, effects::OwnerScope::OppOnly);
}

#[test]
fn test_b05062_remove_count_combined_mode_condition() {
    let catalog = catalog::CardCatalog::load_from_json("../data/dummy/cards.json").unwrap();
    let make_card = |id: &str, def_id: &str, owner: &str, zone: models::Zone| {
        models::CardInstance {
            instance_id: id.to_string(),
            definition_id: def_id.to_string(),
            owner_id: owner.to_string(),
            zone,
            orientation: models::CardOrientation::ACTIVE,
            face_up: true,
            ..Default::default()
        }
    };

    let run_case = |remove_defs: Vec<&str>| {
        let mut engine = engine::MatchEngine::new(catalog.clone());
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        ps1.case_phase = models::CasePhase::RESOLUTION;
        for i in 0..8 {
            ps1.file.push(make_card(&format!("f_{}", i), "B05043", "p1", models::Zone::FILE));
        }
        for (idx, did) in remove_defs.iter().enumerate() {
            ps1.remove.push(make_card(&format!("rm_{}", idx), did, "p1", models::Zone::REMOVE));
        }
        ps1.hand.push(make_card("b05062_card", "B05062", "p1", models::Zone::HAND));
        for i in 0..4 {
            ps1.deck.push(make_card(&format!("d_{}", i), "B05046", "p1", models::Zone::DECK));
        }

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), engine::PlayerState::new("p2".to_string()));
        engine.prompt_main_action("p1");

        let obs = build_player_observation(&engine, "p1");
        let dec_id = obs["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
        engine.submit_decision("p1", &serde_json::json!({
            "decisionId": dec_id,
            "revision": engine.revision,
            "value": { "type": "HAND_PLAY", "cardId": "b05062_card" }
        })).unwrap();
        engine
    };

    // 京極真1枚 + 鈴木財閥2枚 = 計3枚（+ 無関係2枚）では条件未達 -> modeSelect
    let eng3 = run_case(vec!["B05051", "B05046", "B05054", "B01001", "B01005"]);
    let obs3 = build_player_observation(&eng3, "p1");
    assert_eq!(
        obs3["pendingDecision"]["kind"].as_str(),
        Some("modeSelect"),
        "対象合計3枚の場合はmodeSelectになること (logs: {:?})",
        eng3.logs
    );

    // 京極真1枚 + 鈴木財閥3枚 = 合わせて4枚 -> 条件達成で3つとも順次実行
    let eng4 = run_case(vec!["B05051", "B05046", "B05054", "B05055"]);
    assert!(
        eng4.logs.iter().any(|l| l.contains("選択肢をすべて上から順に行います")),
        "京極真+鈴木財閥が合わせて4枚の場合は全実行ログが出ること (logs: {:?})",
        eng4.logs
    );
    let obs4 = build_player_observation(&eng4, "p1");
    assert_eq!(
        obs4["pendingDecision"]["kind"].as_str(),
        Some("targetSelect"),
        "1つ目の『カードを2枚引き、手札を1枚リムーブする』の手札リムーブ選択へ進むこと (logs: {:?})",
        eng4.logs
    );
    assert_eq!(eng4.players["p1"].hand.len(), 2, "カードを2枚引いていること");
}

#[test]
fn test_b05106_gin_and_vodka_and_cross_char_removed_triggers() {
    let catalog = load_test_catalog();

    // 1-A. B05106 ジン＆ウォッカ: 効果による相手キャラリムーブ時に誘発（AP8000以下のみ対象・ターン1）
    {
        let mut engine = MatchEngine::new(catalog.clone());
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        ps1.case_phase = models::CasePhase::RESOLUTION;
        ps1.partner = Some(test_inst("p1_p", "B05104", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
        for i in 0..8 {
            ps1.file.push(test_inst(&format!("f_{}", i), "B05104", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
        }
        ps1.field.push(test_inst("gv_1", "B05106", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("verm_h", "B11086", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));

        let mut ps2 = engine::PlayerState::new("p2".to_string());
        ps2.field.push(test_inst("opp_low", "B05055", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE)); // Lv6 AP4000
        ps2.field.push(test_inst("opp_mid", "B05046", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE)); // Lv8 AP7000
        let mut opp_high = test_inst("opp_high", "B05051", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE); // Lv8 AP8000 + 2000 = AP10000
        opp_high.temp_ap_bonus = 2000;
        ps2.field.push(opp_high);

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);
        engine.prompt_main_action("p1");

        // ベルモット登場時効果で相手 Lv6 をリムーブ
        let obs1 = build_player_observation(&engine, "p1");
        engine.submit_decision("p1", &serde_json::json!({
            "decisionId": obs1["pendingDecision"]["decisionId"],
            "revision": engine.revision,
            "value": { "type": "HAND_PLAY", "cardId": "verm_h" }
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
        engine.submit_decision("p1", &serde_json::json!({
            "decisionId": obs2["pendingDecision"]["decisionId"],
            "revision": engine.revision,
            "value": { "targetId": "opp_low" }
        })).unwrap();

        // 相手キャラリムーブ後、B05106 ジン＆ウォッカの誘発効果でAP8000以下選択が出ること
        let obs3 = build_player_observation(&engine, "p1");
        assert_eq!(
            obs3["pendingDecision"]["kind"].as_str(),
            Some("targetSelect"),
            "B05106の『相手の現場にいるキャラがリムーブされたとき』効果が発動すること (logs: {:?})",
            engine.logs
        );
        assert_eq!(obs3["pendingDecision"]["options"]["actorId"].as_str(), Some("gv_1"));
        assert_eq!(obs3["pendingDecision"]["options"]["effectType"].as_str(), Some("REMOVE"));
        let cands: Vec<&str> = obs3["pendingDecision"]["options"]["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|c| c["instanceId"].as_str())
            .collect();
        assert!(cands.contains(&"opp_mid"), "AP7000は対象に含まれること: {:?}", cands);
        assert!(!cands.contains(&"opp_high"), "AP10000はAP8000以下対象から除外されること: {:?}", cands);

        engine.submit_decision("p1", &serde_json::json!({
            "decisionId": obs3["pendingDecision"]["decisionId"],
            "revision": engine.revision,
            "value": { "targetId": "opp_mid" }
        })).unwrap();

        assert!(engine.players["p2"].remove.iter().any(|c| c.instance_id == "opp_low"));
        assert!(engine.players["p2"].remove.iter().any(|c| c.instance_id == "opp_mid"));
        let obs4 = build_player_observation(&engine, "p1");
        assert_eq!(
            obs4["pendingDecision"]["kind"].as_str(),
            Some("mainAction"),
            "ターン1制限によりB05106でopp_midをリムーブしても2重発動せずmainActionに戻ること (logs: {:?})",
            engine.logs
        );
    }

    // 1-B. B05106 ジン＆ウォッカ: バトルによる相手キャラリムーブ時も発動しmainActionで上書きされないこと
    {
        let mut engine = MatchEngine::new(catalog.clone());
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        ps1.case_phase = models::CasePhase::RESOLUTION;
        ps1.partner = Some(test_inst("p1_p", "B05104", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
        ps1.field.push(test_inst("gv_1", "B05106", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.field.push(test_inst("atk_1", "B11086", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));

        let mut ps2 = engine::PlayerState::new("p2".to_string());
        ps2.field.push(test_inst("opp_def", "B05055", "p2", models::Zone::FIELD, models::CardOrientation::SLEEP)); // AP4000
        ps2.field.push(test_inst("opp_2", "B05054", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE)); // AP1000（ガード可能）

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);
        engine.prompt_main_action("p1");

        let obs1 = build_player_observation(&engine, "p1");
        engine.submit_decision("p1", &serde_json::json!({
            "decisionId": obs1["pendingDecision"]["decisionId"],
            "revision": engine.revision,
            "value": { "type": "ACTION", "actorId": "atk_1", "targetId": "opp_def" }
        })).unwrap();

        // p2 ガードパス -> バトルで opp_def リムーブ -> B05106 誘発
        let obs_g = build_player_observation(&engine, "p2");
        assert_eq!(obs_g["pendingDecision"]["kind"].as_str(), Some("guard"));
        engine.submit_decision("p2", &serde_json::json!({
            "decisionId": obs_g["pendingDecision"]["decisionId"],
            "revision": engine.revision,
            "value": { "pass": true }
        })).unwrap();

        let obs_gv = build_player_observation(&engine, "p1");
        assert_eq!(
            obs_gv["pendingDecision"]["kind"].as_str(),
            Some("targetSelect"),
            "バトル勝利で相手リムーブ時もB05106のtargetSelectが上書きされず表示されること (logs: {:?})",
            engine.logs
        );
        assert_eq!(obs_gv["pendingDecision"]["options"]["actorId"].as_str(), Some("gv_1"));
        engine.submit_decision("p1", &serde_json::json!({
            "decisionId": obs_gv["pendingDecision"]["decisionId"],
            "revision": engine.revision,
            "value": { "targetId": "opp_2" }
        })).unwrap();

        assert!(engine.players["p2"].remove.iter().any(|c| c.instance_id == "opp_def"));
        assert!(engine.players["p2"].remove.iter().any(|c| c.instance_id == "opp_2"));
        assert_eq!(
            build_player_observation(&engine, "p1")["pendingDecision"]["kind"].as_str(),
            Some("mainAction")
        );
    }

    // 2. 横展開: B05066 赤井秀一＆沖矢昴（相手キャラリムーブ時 Lv8以下リムーブ）
    {
        let mut engine = MatchEngine::new(catalog.clone());
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        ps1.partner = Some(test_inst("p1_p", "B05064", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE)); // 赤パートナー
        ps1.field.push(test_inst("akai_sub", "B05066", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));

        let mut ps2 = engine::PlayerState::new("p2".to_string());
        ps2.field.push(test_inst("opp_1", "B05055", "p2", models::Zone::FIELD, models::CardOrientation::SLEEP));
        ps2.field.push(test_inst("opp_2", "B05046", "p2", models::Zone::FIELD, models::CardOrientation::SLEEP)); // Lv8

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);
        engine.prompt_main_action("p1");

        let obs1 = build_player_observation(&engine, "p1");
        engine.submit_decision("p1", &serde_json::json!({
            "decisionId": obs1["pendingDecision"]["decisionId"],
            "revision": engine.revision,
            "value": { "type": "ACTION", "actorId": "akai_sub", "targetId": "opp_1" }
        })).unwrap();
        let obs_gv = build_player_observation(&engine, "p1");
        assert_eq!(obs_gv["pendingDecision"]["kind"].as_str(), Some("targetSelect"));
        assert_eq!(obs_gv["pendingDecision"]["options"]["actorId"].as_str(), Some("akai_sub"));
    }

    // 3. 横展開: B07096 ウォッカ（自ターン中 相手Lv4以下リムーブ時1ドロー）
    {
        let mut engine = MatchEngine::new(catalog.clone());
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        ps1.partner = Some(test_inst("p1_p", "B05104", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
        ps1.field.push(test_inst("vodka_4", "B07096", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.deck.push(test_inst("d1", "B05104", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        ps1.deck.push(test_inst("d2", "B05104", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));

        let mut ps2 = engine::PlayerState::new("p2".to_string());
        ps2.field.push(test_inst("opp_lv6", "B05055", "p2", models::Zone::FIELD, models::CardOrientation::SLEEP)); // Lv6 -> 非誘発
        ps2.field.push(test_inst("opp_lv4", "B11007", "p2", models::Zone::FIELD, models::CardOrientation::SLEEP)); // Lv4 -> 誘発

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        engine.move_field_char_to_remove("p2", "opp_lv6", false, true);
        engine.trigger_on_remove_char("p2", "opp_lv6", "B05055", false);
        assert_eq!(engine.players["p1"].hand.len(), 0, "Lv6リムーブではB07096は誘発しないこと");

        engine.move_field_char_to_remove("p2", "opp_lv4", false, true);
        engine.trigger_on_remove_char("p2", "opp_lv4", "B11007", false);
        assert_eq!(engine.players["p1"].hand.len(), 1, "Lv4以下リムーブでB07096が1枚ドローすること");
    }

    // 4. 横展開: B05059 白馬探（相手ターン中 スリープ状態の自身/探偵キャラリムーブ時1ドロー）
    {
        let mut engine = MatchEngine::new(catalog.clone());
        engine.turn = 4;
        engine.turn_player_id = "p2".to_string(); // 相手ターン中
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        ps1.field.push(test_inst("hakuba", "B05059", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.field.push(test_inst("det_act", "B05050", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.field.push(test_inst("det_slp", "B05050", "p1", models::Zone::FIELD, models::CardOrientation::SLEEP));
        ps1.deck.push(test_inst("d1", "B05059", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        ps1.deck.push(test_inst("d2", "B05059", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), engine::PlayerState::new("p2".to_string()));

        // アクティブな探偵リムーブでは不発
        engine.move_field_char_to_remove("p1", "det_act", false, true);
        engine.trigger_on_remove_char("p1", "det_act", "B05050", false);
        assert_eq!(engine.players["p1"].hand.len(), 0);

        // スリープ状態の探偵リムーブで1ドロー
        engine.move_field_char_to_remove("p1", "det_slp", false, true);
        engine.trigger_on_remove_char("p1", "det_slp", "B05050", false);
        assert_eq!(engine.players["p1"].hand.len(), 1, "B05059がスリープ状態探偵リムーブで1枚ドローすること");
    }

    // 5. 横展開: B03112 ライ（自ターン中 自分能力・効果で黒キャラリムーブ時LP+1）
    {
        let mut engine = MatchEngine::new(catalog.clone());
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        ps1.field.push(test_inst("rye", "B03112", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.field.push(test_inst("verm_d", "B11086", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.field.push(test_inst("blk_sac", "B11085", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), engine::PlayerState::new("p2".to_string()));
        engine.prompt_main_action("p1");

        assert_eq!(engine.get_effective_lp("p1", &engine.players["p1"].field[0]), 1);

        let obs1 = build_player_observation(&engine, "p1");
        engine.submit_decision("p1", &serde_json::json!({
            "decisionId": obs1["pendingDecision"]["decisionId"],
            "revision": engine.revision,
            "value": { "type": "DECLARE", "actorId": "verm_d" }
        })).unwrap();
        let obs2 = build_player_observation(&engine, "p1");
        engine.submit_decision("p1", &serde_json::json!({
            "decisionId": obs2["pendingDecision"]["decisionId"],
            "revision": engine.revision,
            "value": { "targetId": "blk_sac" }
        })).unwrap();

        assert_eq!(
            engine.get_effective_lp("p1", &engine.players["p1"].field[0]),
            2,
            "自分の能力で黒キャラをリムーブしたためB03112ライがLP+1されること (logs: {:?})",
            engine.logs
        );
    }
}

#[test]
fn test_b06104_ka_mass_remove_with_b03116_vodka_revive_and_draw() {
    let catalog = catalog::CardCatalog::load_from_json("../data/dummy/cards.json").unwrap();
    let mut engine = engine::MatchEngine::new(catalog.clone());
    engine.turn = 12;
    engine.turn_player_id = "p1".to_string();
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];
    engine.phase = models::MatchPhase::MAIN;

    let mut ps1 = engine::PlayerState::new("p1".to_string());
    ps1.case_phase = models::CasePhase::RESOLUTION;
    ps1.partner = Some(models::CardInstance {
        instance_id: "p1_partner".to_string(),
        definition_id: "B03107".to_string(), // 黒パートナー
        owner_id: "p1".to_string(),
        zone: models::Zone::PARTNER,
        ..Default::default()
    });
    let mut cs = models::CardInstance {
        instance_id: "p1_case".to_string(),
        definition_id: "B03121".to_string(), // 黒事件
        owner_id: "p1".to_string(),
        zone: models::Zone::CASE,
        ..Default::default()
    };
    cs.orientation = models::CardOrientation::ACTIVE;
    ps1.r#case = Some(cs);

    // FILE 7枚（うち上から2枚がカッで手札に加わる）
    for idx in 0..5 {
        ps1.file.push(models::CardInstance {
            instance_id: format!("f_{}", idx),
            definition_id: "B01097".to_string(),
            owner_id: "p1".to_string(),
            zone: models::Zone::FILE,
            ..Default::default()
        });
    }
    ps1.file.push(models::CardInstance {
        instance_id: "f_vodka7".to_string(),
        definition_id: "B05107".to_string(), // Lv7 ウォッカ
        owner_id: "p1".to_string(),
        zone: models::Zone::FILE,
        ..Default::default()
    });
    ps1.file.push(models::CardInstance {
        instance_id: "f_top".to_string(),
        definition_id: "B01097".to_string(),
        owner_id: "p1".to_string(),
        zone: models::Zone::FILE,
        ..Default::default()
    });

    // 現場に B03116 ウォッカ
    ps1.field.push(models::CardInstance {
        instance_id: "vodka_b03116".to_string(),
        definition_id: "B03116".to_string(),
        owner_id: "p1".to_string(),
        zone: models::Zone::FIELD,
        ..Default::default()
    });

    // 手札に B06104 カッ
    ps1.hand.push(models::CardInstance {
        instance_id: "ka_ev".to_string(),
        definition_id: "B06104".to_string(),
        owner_id: "p1".to_string(),
        zone: models::Zone::HAND,
        ..Default::default()
    });

    // 山札に1枚（B03116 の蘇生後1ドロー用）
    ps1.deck.push(models::CardInstance {
        instance_id: "d_draw".to_string(),
        definition_id: "B01097".to_string(),
        owner_id: "p1".to_string(),
        zone: models::Zone::DECK,
        ..Default::default()
    });

    engine.players.insert("p1".to_string(), ps1);
    engine.players.insert("p2".to_string(), engine::PlayerState::new("p2".to_string()));
    engine.prompt_main_action("p1");

    // 1) カッ (B06104) を手札からプレイ
    let obs1 = build_player_observation(&engine, "p1");
    let dec1 = obs1["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": dec1,
        "revision": engine.revision,
        "value": { "type": "HAND_PLAY", "cardId": "ka_ev" }
    })).unwrap();

    // 2) カッの「手札からレベル7以下の【黒】のキャラを1枚まで登場させる」選択で f_vodka7 (B05107) を選択
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
        "value": { "targetId": "f_vodka7" }
    })).unwrap();

    // 3) カッの効果解決完了後、待機していた B03116 ウォッカの「リムーブエリアからスリープ状態で登場させてもよい」選択が出ること！
    let obs3 = build_player_observation(&engine, "p1");
    assert_eq!(
        obs3["pendingDecision"]["kind"].as_str(),
        Some("targetSelect"),
        "カッの解決後にB03116ウォッカの自己蘇生targetSelectが表示されること (logs: {:?})",
        engine.logs
    );
    let dec3 = obs3["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": dec3,
        "revision": engine.revision,
        "value": { "targetId": "vodka_b03116" }
    })).unwrap();

    let ps1_after = &engine.players["p1"];
    assert!(
        ps1_after.field.iter().any(|c| c.instance_id == "f_vodka7"),
        "カッで登場させたB05107ウォッカが現場にいること"
    );
    let revived_v = ps1_after.field.iter().find(|c| c.instance_id == "vodka_b03116")
        .expect("B03116ウォッカもリムーブエリアから現場に蘇生していること");
    assert_eq!(revived_v.orientation, models::CardOrientation::SLEEP);
    assert!(
        ps1_after.hand.iter().any(|c| c.instance_id == "d_draw"),
        "B03116ウォッカの『そうした場合、カードを1枚引く』でd_drawを手札に引いていること"
    );
}

#[test]
fn test_b03006_and_distinct_names_reject_same_character_different_card_ids() {
    let catalog = load_test_catalog();

    // extract_distinct_card_names / has_name_match の単体検証（デュオ・別名・ID括弧除去）
    assert_eq!(
        effects::extract_distinct_card_names("江戸川コナン(B04005)"),
        vec!["江戸川コナン".to_string()]
    );
    assert_eq!(
        effects::extract_distinct_card_names("江戸川コナン＆工藤新一(B05005)"),
        vec!["江戸川コナン".to_string(), "工藤新一".to_string()]
    );
    assert_eq!(
        effects::extract_distinct_card_names("黒衣の騎士・スペイド（工藤新一）(B07001)"),
        vec!["黒衣の騎士・スペイド".to_string(), "工藤新一".to_string()]
    );

    // 1) リムーブエリアに異なるカードIDの同名カード（B04005 / B05010 江戸川コナン）+ 灰原哀 + 吉田歩美がある状態で B03006 を登場させ、
    //    クライアントが4枚すべてを送信してもサーバー側バリデーションで2枚目のコナンが弾かれて3枚のみ重なること
    {
        let mut engine = MatchEngine::new(catalog.clone());
        engine.phase = models::MatchPhase::MAIN;
        engine.turn = 8;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        ps1.field.push(test_inst("c_kessei", "B03006", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.remove.push(test_inst("conan_04005", "B04005", "p1", models::Zone::REMOVE, models::CardOrientation::ACTIVE));
        ps1.remove.push(test_inst("haibara", "B09011", "p1", models::Zone::REMOVE, models::CardOrientation::ACTIVE));
        ps1.remove.push(test_inst("ayumi", "B11007", "p1", models::Zone::REMOVE, models::CardOrientation::ACTIVE));
        ps1.remove.push(test_inst("conan_05010", "B05010", "p1", models::Zone::REMOVE, models::CardOrientation::ACTIVE));
        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), engine::PlayerState::new("p2".to_string()));

        let deployed = vec![("c_kessei".to_string(), "B03006".to_string(), models::CardOrientation::ACTIVE)];
        engine.resolve_deployed_characters("p1", None, &deployed, "MAIN");

        let obs = build_player_observation(&engine, "p1");
        assert_eq!(obs["pendingDecision"]["kind"].as_str(), Some("targetSelect"));
        assert_eq!(obs["pendingDecision"]["options"]["distinctNames"].as_bool(), Some(true));

        let candidates = obs["pendingDecision"]["options"]["candidates"].as_array().unwrap();
        for cid in ["conan_04005", "conan_05010"] {
            let cand = candidates
                .iter()
                .find(|c| c["instanceId"].as_str() == Some(cid))
                .expect("candidate must exist");
            let cnames: Vec<&str> = cand["cardNames"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|v| v.as_str())
                .collect();
            assert_eq!(cnames, vec!["江戸川コナン"]);
        }

        // 2) CPU (RandomAgent) に選ばせてもコナン2枚を同時選択せず、3枚のみ選ぶこと
        let cpu = agent::RandomAgent::new("p1");
        let cpu_ans = cpu.choose_action_with_engine(&engine, &obs["pendingDecision"]);
        let cpu_ids: Vec<&str> = cpu_ans["value"]["targetIds"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(cpu_ids.len(), 3, "CPUもコナン重複を除いた3枚のみ選択すること: {:?}", cpu_ids);
        assert!(
            !(cpu_ids.contains(&"conan_04005") && cpu_ids.contains(&"conan_05010")),
            "CPUが同名異IDのコナンを両方選んではならない: {:?}",
            cpu_ids
        );

        // 3) 4枚（コナン2種含む）を無理やり送信しても、サーバー側で2枚目の conan_05010 が除外され3枚のみ下に重なること
        let dec_id = obs["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
        engine.submit_decision("p1", &serde_json::json!({
            "decisionId": dec_id,
            "revision": engine.revision,
            "value": {
                "targetIds": ["conan_04005", "haibara", "ayumi", "conan_05010"],
                "actorId": "c_kessei",
                "effectType": "STACK_UNDER_SELF"
            }
        })).unwrap();

        let p1 = &engine.players["p1"];
        let kessei = p1.field.iter().find(|c| c.instance_id == "c_kessei").unwrap();
        let stacked_ids: Vec<&str> = kessei.set_cards.iter().map(|s| s.instance_id.as_str()).collect();
        assert_eq!(stacked_ids, vec!["conan_04005", "haibara", "ayumi"]);
        assert!(
            p1.remove.iter().any(|c| c.instance_id == "conan_05010"),
            "2枚目の江戸川コナン(B05010)はリムーブエリアに残ること"
        );
    }
}

#[test]
fn test_d04007_case_action_gain_evidence_trigger() {
    let catalog = catalog::CardCatalog::load_from_json("../data/dummy/cards.json").unwrap();
    let mut engine = MatchEngine::new(catalog);
    engine.phase = models::MatchPhase::MAIN;
    engine.turn = 4;
    engine.turn_player_id = "p1".to_string();
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];

    let mut ps1 = engine::PlayerState::new("p1".to_string());
    let mut mary = test_inst("c_mary", "D04007", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE);
    mary.entered_turn = 0;
    let mut atk = test_inst("c_atk", "B01001", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE);
    atk.entered_turn = 0;
    ps1.field = vec![mary, atk];
    ps1.hand.push(test_inst("c_h1", "B01001", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
    for i in 0..5 {
        ps1.deck.push(test_inst(&format!("c_d{}", i), "B01001", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
    }

    let mut ps2 = engine::PlayerState::new("p2".to_string());
    ps2.evidence.push(test_inst("p2_ev1", "B01001", "p2", models::Zone::EVIDENCE, models::CardOrientation::ACTIVE));
    for i in 0..5 {
        ps2.deck.push(test_inst(&format!("p2_d{}", i), "B01001", "p2", models::Zone::DECK, models::CardOrientation::ACTIVE));
    }
    ps2.case = Some(test_inst("p2_case", "B01002C", "p2", models::Zone::CASE, models::CardOrientation::ACTIVE));

    engine.players.insert("p1".to_string(), ps1);
    engine.players.insert("p2".to_string(), ps2);
    engine.prompt_main_action("p1");

    // 1) 事件へアクションする
    let obs1 = build_player_observation(&engine, "p1");
    let dec1 = obs1["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": dec1,
        "revision": engine.revision,
        "value": { "type": "ACTION", "actorId": "c_atk", "targetId": "p2_case" }
    })).unwrap();

    // 2) p2はガードせずパスする
    let obs2 = build_player_observation(&engine, "p2");
    if obs2["pendingDecision"]["playerId"].as_str() == Some("p2") {
        let dec2 = obs2["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
        engine.submit_decision("p2", &serde_json::json!({
            "decisionId": dec2,
            "revision": engine.revision,
            "value": { "pass": true }
        })).unwrap();
    }

    // 3) ログに相手の証拠からリムーブされたカード名が記載されていること & D04007の任意手札リムーブが開くこと
    assert!(
        engine.logs.iter().any(|l| l.contains("証拠から [")),
        "事件アクションヒット時のログにリムーブされた証拠カード名が含まれること: {:?}",
        engine.logs
    );

    let obs3 = build_player_observation(&engine, "p1");
    assert_eq!(obs3["pendingDecision"]["kind"].as_str(), Some("targetSelect"));
    assert_eq!(obs3["pendingDecision"]["options"]["effectType"].as_str(), Some("DISCARD_HAND"));

    // 4) 手札を1枚リムーブすると、D04007の効果で追加の証拠が+1（合計2）されること
    let dec3 = obs3["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": dec3,
        "revision": engine.revision,
        "value": { "targetId": "c_h1", "actorId": "c_mary", "effectType": "DISCARD_HAND" }
    })).unwrap();

    assert_eq!(
        engine.players["p1"].evidence.len(),
        2,
        "事件攻撃+1とD04007効果+1で証拠が2になること。logs: {:?}",
        engine.logs
    );
}

#[test]
fn test_b02018_declare_mills_3_and_sets_deck_top_face_down_on_self() {
    let catalog = catalog::CardCatalog::load_from_json("../data/dummy/cards.json").unwrap();
    let mut engine = MatchEngine::new(catalog);
    engine.phase = models::MatchPhase::MAIN;
    engine.turn = 4;
    engine.turn_player_id = "p1".to_string();
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];

    let mut ps1 = engine::PlayerState::new("p1".to_string());
    let mut heiji = test_inst("c_heiji", "B02018", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE);
    heiji.entered_turn = 0;
    ps1.field.push(heiji);
    for i in 0..6 {
        ps1.deck.push(test_inst(&format!("c_d{}", i), "B01001", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
    }
    engine.players.insert("p1".to_string(), ps1);
    engine.players.insert("p2".to_string(), engine::PlayerState::new("p2".to_string()));
    engine.prompt_main_action("p1");

    let obs = build_player_observation(&engine, "p1");
    let dec_id = obs["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": dec_id,
        "revision": engine.revision,
        "value": { "type": "DECLARE", "actorId": "c_heiji" }
    })).unwrap();

    let p1 = &engine.players["p1"];
    assert_eq!(p1.remove.len(), 3, "コストで山札から3枚リムーブされること");
    assert_eq!(p1.field[0].set_cards.len(), 1, "山札から1枚が裏向きで自身にセットされること");
    assert!(!p1.field[0].set_cards[0].face_up, "セットされたカードは裏向きであること");
    assert!(p1.field[0].has_rush, "セット時効果で〚突撃〛を得ること");
}

#[test]
fn test_b02018_triggers_on_card_set_by_other_cards_and_respects_limits() {
    let catalog = catalog::CardCatalog::load_from_json("../data/dummy/cards.json").unwrap();
    let mut engine = MatchEngine::new(catalog);
    engine.phase = models::MatchPhase::MAIN;
    engine.turn = 4;
    engine.turn_player_id = "p1".to_string();
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];

    let mut ps1 = engine::PlayerState::new("p1".to_string());
    ps1.case = Some(test_inst("p1_case", "B02001", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
    ps1.case_phase = models::CasePhase::INTRO;
    for i in 0..7 {
        ps1.file.push(test_inst(&format!("f1_{}", i), "B01021", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
    }
    let mut heiji = test_inst("c_heiji", "B02018", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE);
    heiji.entered_turn = 4;
    let mut heizo = test_inst("c_heizo", "B02030", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE);
    heizo.entered_turn = 0;
    ps1.field = vec![heiji, heizo];
    ps1.hand.push(test_inst("ev_b01039", "B01039", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
    ps1.hand.push(test_inst("ch_kazuha", "B02023", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
    for i in 0..10 {
        ps1.deck.push(test_inst(&format!("d1_{}", i), "B01001", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
    }

    let mut ps2 = engine::PlayerState::new("p2".to_string());
    ps2.case = Some(test_inst("p2_case", "B02001", "p2", models::Zone::CASE, models::CardOrientation::ACTIVE));
    ps2.case_phase = models::CasePhase::INTRO;
    for i in 0..7 {
        ps2.file.push(test_inst(&format!("f2_{}", i), "B01021", "p2", models::Zone::FILE, models::CardOrientation::ACTIVE));
    }
    ps2.hand.push(test_inst("ch_momiji", "B02020", "p2", models::Zone::HAND, models::CardOrientation::ACTIVE));
    for i in 0..5 {
        ps2.deck.push(test_inst(&format!("d2_{}", i), "B01001", "p2", models::Zone::DECK, models::CardOrientation::ACTIVE));
    }

    engine.players.insert("p1".to_string(), ps1);
    engine.players.insert("p2".to_string(), ps2);
    engine.prompt_main_action("p1");

    // 1回目: 別キャラ B02030（服部平蔵）の【宣言】で裏向きセット（ACTIVE なので自動で〚突撃〛獲得）
    let obs = build_player_observation(&engine, "p1");
    let dec1 = obs["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": dec1,
        "revision": engine.revision,
        "value": { "type": "DECLARE", "actorId": "c_heizo" }
    })).unwrap();

    let obs_t1 = build_player_observation(&engine, "p1");
    assert_eq!(obs_t1["pendingDecision"]["kind"].as_str(), Some("targetSelect"));
    let dec_t1 = obs_t1["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": dec_t1,
        "revision": engine.revision,
        "value": { "targetId": "c_heiji" }
    })).unwrap();

    assert_eq!(engine.players["p1"].field[0].set_cards.len(), 1, "B02030で1枚セットされること");
    assert!(engine.players["p1"].field[0].has_rush, "1回目（ACTIVE）: 他カードによるセットで〚突撃〛を得ること");

    // 2回目: B02018 を SLEEP にしてイベント B01039 をセット → has_rush 済みのため自動で ACTIVE になる
    engine.players.get_mut("p1").unwrap().field[0].orientation = models::CardOrientation::SLEEP;
    let obs_ev = build_player_observation(&engine, "p1");
    let dec_ev = obs_ev["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": dec_ev,
        "revision": engine.revision,
        "value": { "type": "HAND_PLAY", "cardId": "ev_b01039" }
    })).unwrap();

    let obs_ev_t = build_player_observation(&engine, "p1");
    assert_eq!(obs_ev_t["pendingDecision"]["kind"].as_str(), Some("targetSelect"));
    let dec_ev_t = obs_ev_t["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": dec_ev_t,
        "revision": engine.revision,
        "value": { "targetId": "c_heiji" }
    })).unwrap();

    assert_eq!(engine.players["p1"].field[0].set_cards.len(), 2, "イベントB01039がセットされること");
    assert_eq!(
        engine.players["p1"].field[0].orientation,
        models::CardOrientation::ACTIVE,
        "2回目（SLEEP, has_rush済）: イベントセットでアクティブになること"
    );

    // 3回目: 再び SLEEP にし、B02023（遠山和葉）の【登場時】でセット → 【ターン2】上限により発動しない
    {
        let ps1 = engine.players.get_mut("p1").unwrap();
        ps1.field[0].orientation = models::CardOrientation::SLEEP;
        ps1.normal_hand_played_this_turn = false;
    }
    engine.prompt_main_action("p1");
    let obs_kaz = build_player_observation(&engine, "p1");
    let dec_kaz = obs_kaz["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": dec_kaz,
        "revision": engine.revision,
        "value": { "type": "HAND_PLAY", "cardId": "ch_kazuha" }
    })).unwrap();

    let obs_kaz_t = build_player_observation(&engine, "p1");
    assert_eq!(obs_kaz_t["pendingDecision"]["kind"].as_str(), Some("targetSelect"));
    let dec_kaz_t = obs_kaz_t["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": dec_kaz_t,
        "revision": engine.revision,
        "value": { "targetId": "c_heiji" }
    })).unwrap();

    assert_eq!(engine.players["p1"].field[0].set_cards.len(), 3, "B02023で3枚目がセットされること");
    assert_eq!(
        engine.players["p1"].field[0].orientation,
        models::CardOrientation::SLEEP,
        "3回目: 【ターン2】上限に達しているためアクティブ化しないこと"
    );

    // 4) 相手ターン中（p2のターン）に相手カード B02020（大岡紅葉）の効果で B02018 に裏向きセット → 【自分ターン中】未達のため発動しない
    engine.turn = 5;
    engine.turn_player_id = "p2".to_string();
    engine.turn_clause_usage.clear();
    {
        let ps1 = engine.players.get_mut("p1").unwrap();
        ps1.field[0].orientation = models::CardOrientation::SLEEP;
        ps1.field[0].has_rush = false;
    }
    engine.prompt_main_action("p2");
    let obs_p2 = build_player_observation(&engine, "p2");
    let dec_p2 = obs_p2["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p2", &serde_json::json!({
        "decisionId": dec_p2,
        "revision": engine.revision,
        "value": { "type": "HAND_PLAY", "cardId": "ch_momiji" }
    })).unwrap();

    let obs_p2_t = build_player_observation(&engine, "p2");
    assert_eq!(obs_p2_t["pendingDecision"]["kind"].as_str(), Some("targetSelect"));
    let dec_p2_t = obs_p2_t["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p2", &serde_json::json!({
        "decisionId": dec_p2_t,
        "revision": engine.revision,
        "value": { "targetId": "c_heiji" }
    })).unwrap();

    assert_eq!(engine.players["p1"].field[0].set_cards.len(), 4, "相手ターン中にB02020で4枚目がセットされること");
    assert_eq!(
        engine.players["p1"].field[0].orientation,
        models::CardOrientation::SLEEP,
        "相手ターン中は【自分ターン中】条件を満たさないためアクティブ化しないこと"
    );
    assert!(
        !engine.players["p1"].field[0].has_rush,
        "相手ターン中は【自分ターン中】条件を満たさないため〚突撃〛を得ないこと"
    );
}

#[test]
fn test_b02018_mode_select_when_sleep_without_rush_on_entry_turn() {
    let catalog = catalog::CardCatalog::load_from_json("../data/dummy/cards.json").unwrap();

    for choose_idx in [0usize, 1usize] {
        let mut engine = MatchEngine::new(catalog.clone());
        engine.phase = models::MatchPhase::MAIN;
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        for i in 0..6 {
            ps1.file.push(test_inst(&format!("f_{}", i), "B01021", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
        }
        let mut heiji = test_inst("c_heiji", "B02018", "p1", models::Zone::FIELD, models::CardOrientation::SLEEP);
        heiji.entered_turn = 4;
        heiji.has_rush = false;
        ps1.field.push(heiji);
        ps1.hand.push(test_inst("ev_b01039", "B01039", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), engine::PlayerState::new("p2".to_string()));
        engine.prompt_main_action("p1");

        let obs1 = build_player_observation(&engine, "p1");
        let dec1 = obs1["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
        engine.submit_decision("p1", &serde_json::json!({
            "decisionId": dec1,
            "revision": engine.revision,
            "value": { "type": "HAND_PLAY", "cardId": "ev_b01039" }
        })).unwrap();

        let obs2 = build_player_observation(&engine, "p1");
        assert_eq!(obs2["pendingDecision"]["kind"].as_str(), Some("targetSelect"));
        let dec2 = obs2["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
        engine.submit_decision("p1", &serde_json::json!({
            "decisionId": dec2,
            "revision": engine.revision,
            "value": { "targetId": "c_heiji" }
        })).unwrap();

        let obs3 = build_player_observation(&engine, "p1");
        assert_eq!(
            obs3["pendingDecision"]["kind"].as_str(),
            Some("modeSelect"),
            "登場ターンにSLEEPかつ非突撃ならアクティブか突撃のmodeSelectが出ること"
        );
        let dec3 = obs3["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
        engine.submit_decision("p1", &serde_json::json!({
            "decisionId": dec3,
            "revision": engine.revision,
            "value": { "modeIndex": choose_idx }
        })).unwrap();

        let heiji_after = &engine.players["p1"].field[0];
        if choose_idx == 0 {
            assert_eq!(heiji_after.orientation, models::CardOrientation::ACTIVE, "選択肢0でアクティブになること");
            assert!(!heiji_after.has_rush);
        } else {
            assert_eq!(heiji_after.orientation, models::CardOrientation::SLEEP);
            assert!(heiji_after.has_rush, "選択肢1で〚突撃〛を得ること");
        }
    }
}

#[test]
fn test_b06046_and_b06047_yaiba_card_set_triggers() {
    let catalog = catalog::CardCatalog::load_from_json("../data/dummy/cards.json").unwrap();

    // 1) B06046 鉄刃: 裏向きセットでは不発、YAIBAイベントセットでアクティブ（ターン2制限）
    {
        let mut engine = MatchEngine::new(catalog.clone());
        engine.phase = models::MatchPhase::MAIN;
        engine.turn = 6;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        for i in 0..8 {
            ps1.file.push(test_inst(&format!("f_{}", i), "B06046", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
        }
        let mut yaiba = test_inst("c_yaiba", "B06046", "p1", models::Zone::FIELD, models::CardOrientation::SLEEP);
        yaiba.entered_turn = 6;
        let mut heizo = test_inst("c_heizo", "B02030", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE);
        heizo.entered_turn = 0;
        ps1.field = vec![yaiba, heizo];
        ps1.hand.push(test_inst("ev1", "B06064", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("ev2", "B06064", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("ev3", "B06064", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        for i in 0..5 {
            ps1.deck.push(test_inst(&format!("d_{}", i), "B06048", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        }
        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), engine::PlayerState::new("p2".to_string()));
        engine.prompt_main_action("p1");

        // 裏向きセットでは特徴YAIBAを満たさず不発
        let obs_d = build_player_observation(&engine, "p1");
        let dec_d = obs_d["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
        engine.submit_decision("p1", &serde_json::json!({
            "decisionId": dec_d,
            "revision": engine.revision,
            "value": { "type": "DECLARE", "actorId": "c_heizo" }
        })).unwrap();
        let obs_dt = build_player_observation(&engine, "p1");
        let dec_dt = obs_dt["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
        engine.submit_decision("p1", &serde_json::json!({
            "decisionId": dec_dt,
            "revision": engine.revision,
            "value": { "targetId": "c_yaiba" }
        })).unwrap();
        assert_eq!(engine.players["p1"].field[0].set_cards.len(), 1);
        assert_eq!(
            engine.players["p1"].field[0].orientation,
            models::CardOrientation::SLEEP,
            "裏向きカードでは〚特徴［YAIBA］〛条件を満たさずB06046はアクティブにならないこと"
        );

        // 表向き YAIBA イベントセット 1回目・2回目でアクティブ、3回目はターン2上限で不発
        for (ev_id, expect_active) in [("ev1", true), ("ev2", true), ("ev3", false)] {
            {
                let ps1 = engine.players.get_mut("p1").unwrap();
                ps1.field[0].orientation = models::CardOrientation::SLEEP;
                ps1.normal_hand_played_this_turn = false;
            }
            engine.prompt_main_action("p1");
            let obs_e = build_player_observation(&engine, "p1");
            let dec_e = obs_e["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
            engine.submit_decision("p1", &serde_json::json!({
                "decisionId": dec_e,
                "revision": engine.revision,
                "value": { "type": "HAND_PLAY", "cardId": ev_id }
            })).unwrap();
            let obs_et = build_player_observation(&engine, "p1");
            assert_eq!(obs_et["pendingDecision"]["kind"].as_str(), Some("targetSelect"));
            let dec_et = obs_et["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
            engine.submit_decision("p1", &serde_json::json!({
                "decisionId": dec_et,
                "revision": engine.revision,
                "value": { "targetId": "c_yaiba" }
            })).unwrap();
            let expected = if expect_active {
                models::CardOrientation::ACTIVE
            } else {
                models::CardOrientation::SLEEP
            };
            assert_eq!(
                engine.players["p1"].field[0].orientation,
                expected,
                "イベント {} セット後のB06046状態が想定通りであること",
                ev_id
            );
        }
    }

    // 2) B06047 鉄刃: YAIBAカードセットでリムーブからLv5以下YAIBAをスリープ登場（ターン1制限）
    {
        let mut engine = MatchEngine::new(catalog);
        engine.phase = models::MatchPhase::MAIN;
        engine.turn = 6;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        for i in 0..7 {
            ps1.file.push(test_inst(&format!("f_{}", i), "B06047", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
        }
        let mut yaiba = test_inst("c_yaiba47", "B06047", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE);
        yaiba.entered_turn = 6;
        ps1.field.push(yaiba);
        ps1.remove.push(test_inst("rem_sayaka1", "B06048", "p1", models::Zone::REMOVE, models::CardOrientation::ACTIVE));
        ps1.remove.push(test_inst("rem_sayaka2", "B06048", "p1", models::Zone::REMOVE, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("ev1", "B06064", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("ev2", "B06064", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        for i in 0..5 {
            ps1.deck.push(test_inst(&format!("d_{}", i), "B06048", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        }
        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), engine::PlayerState::new("p2".to_string()));
        engine.prompt_main_action("p1");

        // 1枚目: セット後 B06047 効果でリムーブからスリープ登場の targetSelect が開く
        let obs1 = build_player_observation(&engine, "p1");
        let dec1 = obs1["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
        engine.submit_decision("p1", &serde_json::json!({
            "decisionId": dec1,
            "revision": engine.revision,
            "value": { "type": "HAND_PLAY", "cardId": "ev1" }
        })).unwrap();
        let obs_t1 = build_player_observation(&engine, "p1");
        let dec_t1 = obs_t1["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
        engine.submit_decision("p1", &serde_json::json!({
            "decisionId": dec_t1,
            "revision": engine.revision,
            "value": { "targetId": "c_yaiba47" }
        })).unwrap();

        let obs_rev = build_player_observation(&engine, "p1");
        assert_eq!(obs_rev["pendingDecision"]["kind"].as_str(), Some("targetSelect"));
        assert_eq!(
            obs_rev["pendingDecision"]["options"]["effectType"].as_str(),
            Some("REVIVE_SLEEP"),
            "B06047のセット時効果でREVIVE_SLEEP対象選択が開くこと"
        );
        let dec_rev = obs_rev["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
        engine.submit_decision("p1", &serde_json::json!({
            "decisionId": dec_rev,
            "revision": engine.revision,
            "value": { "targetId": "rem_sayaka1" }
        })).unwrap();

        // 蘇生された峰さやか(B06048)が場に登場し SLEEP であること（登場時deckSearchが開いていれば解決）
        if engine.pending_decision.as_ref().and_then(|d| d["kind"].as_str()) == Some("deckSearch") {
            let obs_ds = build_player_observation(&engine, "p1");
            let dec_ds = obs_ds["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
            engine.submit_decision("p1", &serde_json::json!({
                "decisionId": dec_ds,
                "revision": engine.revision,
                "value": { "chosenCardId": null }
            })).unwrap();
        }
        assert!(
            engine.players["p1"].field.iter().any(|c| c.instance_id == "rem_sayaka1" && c.orientation == models::CardOrientation::SLEEP),
            "B06047のセット時効果でリムーブエリアのB06048がスリープ状態で場に登場すること"
        );

        // 2枚目: 【ターン1】上限に達しているため再発動しないこと
        engine.players.get_mut("p1").unwrap().normal_hand_played_this_turn = false;
        engine.prompt_main_action("p1");
        let obs2 = build_player_observation(&engine, "p1");
        let dec2 = obs2["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
        engine.submit_decision("p1", &serde_json::json!({
            "decisionId": dec2,
            "revision": engine.revision,
            "value": { "type": "HAND_PLAY", "cardId": "ev2" }
        })).unwrap();
        let obs_t2 = build_player_observation(&engine, "p1");
        let dec_t2 = obs_t2["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
        engine.submit_decision("p1", &serde_json::json!({
            "decisionId": dec_t2,
            "revision": engine.revision,
            "value": { "targetId": "c_yaiba47" }
        })).unwrap();

        assert_eq!(
            engine.pending_decision.as_ref().and_then(|d| d["kind"].as_str()),
            Some("mainAction"),
            "【ターン1】制限により2回目のセットではB06047効果が発動せずmainActionに戻ること"
        );
    }
}

#[test]
fn test_b06067_declare_discard_selects_police_and_grants_can_attack_active() {
    let catalog = load_test_catalog();
    let mut engine = MatchEngine::new(catalog);
    engine.phase = models::MatchPhase::MAIN;
    engine.turn = 6;
    engine.turn_player_id = "p1".to_string();
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];

    let mut ps1 = engine::PlayerState::new("p1".to_string());
    let mut nakamori = test_inst("c_nakamori", "B06067", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE);
    nakamori.entered_turn = 6;
    ps1.field.push(nakamori);
    ps1.hand.push(test_inst("h_cost1", "B01001", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
    ps1.hand.push(test_inst("h_cost2", "B01005", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));

    let mut ps2 = engine::PlayerState::new("p2".to_string());
    ps2.field.push(test_inst("p2_active", "B01001", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE));

    engine.players.insert("p1".to_string(), ps1);
    engine.players.insert("p2".to_string(), ps2);
    engine.prompt_main_action("p1");

    // 1) B06067 の【宣言】を発動
    let obs1 = build_player_observation(&engine, "p1");
    let dec1 = obs1["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": dec1,
        "revision": engine.revision,
        "value": { "type": "DECLARE", "actorId": "c_nakamori" }
    })).unwrap();

    // 2) コストの手札1枚リムーブを選択
    let obs2 = build_player_observation(&engine, "p1");
    assert_eq!(obs2["pendingDecision"]["kind"].as_str(), Some("targetSelect"));
    assert_eq!(obs2["pendingDecision"]["options"]["effectType"].as_str(), Some("DISCARD_HAND"));
    let dec2 = obs2["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": dec2,
        "revision": engine.revision,
        "value": { "targetId": "h_cost1" }
    })).unwrap();

    // 3) 警察キャラを選ぶフェーズ (GIVE_CAN_ATTACK_ACTIVE) が開くこと
    let obs3 = build_player_observation(&engine, "p1");
    assert_eq!(
        obs3["pendingDecision"]["kind"].as_str(),
        Some("targetSelect"),
        "手札コスト支払い後に警察キャラを選ぶtargetSelectが開くこと"
    );
    assert_eq!(obs3["pendingDecision"]["options"]["effectType"].as_str(), Some("GIVE_CAN_ATTACK_ACTIVE"));
    let dec3 = obs3["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": dec3,
        "revision": engine.revision,
        "value": { "targetId": "c_nakamori" }
    })).unwrap();

    assert!(
        engine.players["p1"].field[0].can_attack_active,
        "選ばれた警察キャラに can_attack_active が付与されること"
    );

    // 4) mainAction で相手のアクティブ状態のキャラ (p2_active) にアクションできること
    let obs4 = build_player_observation(&engine, "p1");
    let actions = obs4["pendingDecision"]["options"]["actions"].as_array().unwrap();
    let act_entry = actions.iter().find(|a| a["type"].as_str() == Some("ACTION")).unwrap();
    let targets = act_entry["actorTargetMap"]["c_nakamori"].as_array().unwrap();
    assert!(
        targets.iter().any(|t| t.as_str() == Some("p2_active")),
        "相手のアクティブ状態のキャラにアクションできること: {:?}",
        targets
    );
}

#[test]
fn test_b02025_on_remove_does_not_pick_self_without_cutin() {
    let mut engine = setup_main_match(7, "p2");
    let mut kazuha = test_inst("c_kazuha", "B02025", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE);
    kazuha.entered_turn = 6;
    engine.players.get_mut("p1").unwrap().field.push(kazuha);

    // 相手ターン中に B02025 がリムーブされ、リムーブエリアに B02025 自身（カットイン非所持）のみがある状態
    if let Some((rem_def, _)) = engine.move_field_char_to_remove("p1", "c_kazuha", false, false) {
        engine.trigger_on_remove_char("p1", "c_kazuha", &rem_def, false);
    }

    assert!(
        engine.pending_decision.is_none(),
        "B02025自身は【カットイン】を持たないため、対象不在でRECOVER_TO_HANDの選択が開かないこと"
    );
    assert_eq!(
        engine.players["p1"].hand.len(),
        0,
        "【カットイン】を持たないB02025自身が手札に加わってはならないこと"
    );
}

#[test]
fn test_b06084_on_enter_reveals_opponent_hand_then_removes_lv9_or_less() {
    let catalog = load_test_catalog();
    let mut engine = MatchEngine::new(catalog);
    engine.phase = models::MatchPhase::MAIN;
    engine.turn_player_id = "p1".to_string();

    let mut ps1 = engine::PlayerState::new("p1".to_string());
    ps1.partner = Some(test_inst("p1_p", "B01079", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
    ps1.case = Some(test_inst("p1_c", "PR115", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
    ps1.field.push(test_inst("b06084", "B06084", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));

    let mut ps2 = engine::PlayerState::new("p2".to_string());
    ps2.partner = Some(test_inst("p2_p", "B01079", "p2", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
    ps2.case = Some(test_inst("p2_c", "PR115", "p2", models::Zone::CASE, models::CardOrientation::ACTIVE));
    ps2.hand.push(test_inst("p2_h1", "B06089", "p2", models::Zone::HAND, models::CardOrientation::ACTIVE));
    ps2.field.push(test_inst("p2_pr096", "PR096", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE));

    engine.player_order = vec!["p1".to_string(), "p2".to_string()];
    engine.players.insert("p1".to_string(), ps1);
    engine.players.insert("p2".to_string(), ps2);

    engine.resolve_deployed_characters(
        "p1",
        None,
        &[("b06084".to_string(), "B06084".to_string(), models::CardOrientation::ACTIVE)],
        "MAIN",
    );

    // 1. まず相手の手札を公開するフェーズが開くこと
    let view_dec = engine.pending_decision.as_ref().unwrap();
    assert_eq!(view_dec["kind"], "targetSelect");
    assert_eq!(view_dec["options"]["effectType"], "VIEW_OPPONENT_HAND");
    let cands = view_dec["options"]["candidates"].as_array().unwrap();
    assert_eq!(cands.len(), 1);
    assert_eq!(cands[0]["definitionId"], "B06089");

    // 確認後、次へ進む
    let view_id = view_dec["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": view_id,
        "revision": engine.revision,
        "value": {"targetIds": []}
    })).unwrap();

    // 2. 続いてレベル9以下のキャラを1枚まで選びリムーブするフェーズが開くこと
    let rem_dec = engine.pending_decision.as_ref().unwrap();
    assert_eq!(rem_dec["kind"], "targetSelect");
    let rem_id = rem_dec["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": rem_id,
        "revision": engine.revision,
        "value": {"targetIds": ["p2_pr096"]}
    })).unwrap();

    assert_eq!(engine.players["p2"].field.len(), 0, "相手のPR096がリムーブされること");
}

#[test]
fn test_b05102_still_draws_and_deploys_when_opponent_field_empty() {
    let catalog = load_test_catalog();
    let mut engine = MatchEngine::new(catalog);
    engine.phase = models::MatchPhase::MAIN;
    engine.turn_player_id = "p1".to_string();
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];

    let mut ps1 = engine::PlayerState::new("p1".to_string());
    ps1.partner = Some(test_inst("p1_p", "B01079", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
    ps1.case = Some(test_inst("p1_c", "PR115", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
    for i in 0..2 {
        ps1.file.push(test_inst(&format!("f{}", i), "B06089", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
    }
    ps1.hand.push(test_inst("ev", "B05102", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
    ps1.hand.push(test_inst("ch_lv2", "B06089", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
    ps1.deck.push(test_inst("d1", "B06089", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
    ps1.deck.push(test_inst("d2", "B06089", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));

    let mut ps2 = engine::PlayerState::new("p2".to_string());
    ps2.partner = Some(test_inst("p2_p", "B01060", "p2", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
    ps2.case = Some(test_inst("p2_c", "B01078", "p2", models::Zone::CASE, models::CardOrientation::ACTIVE));

    engine.players.insert("p1".to_string(), ps1);
    engine.players.insert("p2".to_string(), ps2);

    let desc = engine.catalog.get("B05102").unwrap().description.clone();
    engine.execute_declare_clause("p1", "ev", "B05102", false, None, models::CardOrientation::ACTIVE, &desc);

    assert_eq!(engine.players["p1"].hand.len(), 3, "対象不在でも後続の1枚ドローが実行されること");
}

#[test]
fn test_b01081_can_choose_deck_top_or_bottom() {
    let catalog = load_test_catalog();
    let mut engine = MatchEngine::new(catalog);
    engine.phase = models::MatchPhase::MAIN;
    engine.turn_player_id = "p1".to_string();
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];

    let mut ps1 = engine::PlayerState::new("p1".to_string());
    ps1.partner = Some(test_inst("p1_p", "B01079", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
    ps1.case = Some(test_inst("p1_c", "PR115", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
    ps1.field.push(test_inst("b01081", "B01081P", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));

    let mut ps2 = engine::PlayerState::new("p2".to_string());
    ps2.partner = Some(test_inst("p2_p", "PR250", "p2", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
    ps2.case = Some(test_inst("p2_c", "B10101", "p2", models::Zone::CASE, models::CardOrientation::ACTIVE));
    ps2.field.push(test_inst("opp_lv7", "B07041", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE));
    ps2.deck.push(test_inst("opp_d1", "D03009", "p2", models::Zone::DECK, models::CardOrientation::ACTIVE));

    engine.players.insert("p1".to_string(), ps1);
    engine.players.insert("p2".to_string(), ps2);

    let desc = engine.catalog.get("B01081P").unwrap().description.clone();
    engine.execute_declare_clause("p1", "b01081", "B01081P", false, None, models::CardOrientation::ACTIVE, &desc);

    // 1. まず対象キャラを選ぶ
    let dec1 = engine.pending_decision.as_ref().unwrap();
    assert_eq!(dec1["kind"], "targetSelect");
    let id1 = dec1["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": id1,
        "revision": engine.revision,
        "value": {"targetIds": ["opp_lv7"]}
    })).unwrap();

    // 2. デッキの上か下かを選ぶ modeSelect が開くこと
    let dec2 = engine.pending_decision.as_ref().unwrap();
    assert_eq!(dec2["kind"], "modeSelect");
    assert_eq!(dec2["options"]["specialKind"], "DECK_TOP_OR_BOTTOM");
    let id2 = dec2["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": id2,
        "revision": engine.revision,
        "value": {"modeIndex": 0}
    })).unwrap();

    assert_eq!(engine.players["p2"].field.len(), 0);
    assert_eq!(engine.players["p2"].deck.last().unwrap().instance_id, "opp_lv7", "デッキの上に移動すること");
}

#[test]
fn test_b05029_declare_requires_2_set_cards_removal() {
    let catalog = load_test_catalog();
    let mut engine = MatchEngine::new(catalog);
    engine.phase = models::MatchPhase::MAIN;
    engine.turn = 6;
    engine.turn_player_id = "p1".to_string();
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];

    let mut ps1 = engine::PlayerState::new("p1".to_string());
    ps1.partner = Some(test_inst("p1_partner", "B01031", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
    // スリープ状態にして2つ目の【パートナー緑】【宣言】【ターン1】のみ発動可能にする
    let mut b05029 = test_inst("c_b05029", "B05029", "p1", models::Zone::FIELD, models::CardOrientation::SLEEP);
    let mut sc1 = test_inst("sc_self", "B11009", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE);
    sc1.face_up = false;
    b05029.set_cards.push(sc1);
    ps1.field.push(b05029);

    let mut ps2 = engine::PlayerState::new("p2".to_string());
    let mut opp = test_inst("opp_lv4", "B11009", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE);
    let mut sc2 = test_inst("sc_opp", "B11009", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE);
    sc2.face_up = false;
    opp.set_cards.push(sc2);
    ps2.field.push(opp);

    engine.players.insert("p1".to_string(), ps1);
    engine.players.insert("p2".to_string(), ps2);

    engine.prompt_main_action("p1");
    let dec0 = engine.pending_decision.as_ref().unwrap()["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": dec0,
        "revision": engine.revision,
        "value": {"type": "DECLARE", "actorId": "c_b05029"}
    })).unwrap();

    // 1. まず裏向きセットカードを2枚選ぶ targetSelect が開くこと
    let dec1 = engine.pending_decision.as_ref().unwrap();
    assert_eq!(dec1["kind"], "targetSelect", "先にセットカード選択が開くこと: logs={:?}", engine.logs);
    let id1 = dec1["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": id1,
        "revision": engine.revision,
        "value": {"targetIds": ["sc_self", "sc_opp"]}
    })).unwrap();

    // セットカードが両方リムーブされていること
    assert_eq!(engine.players["p1"].field[0].set_cards.len(), 0);
    assert_eq!(engine.players["p2"].field[0].set_cards.len(), 0);

    // 2. 次にレベル7以下のキャラを選ぶ targetSelect が開くこと
    let dec2 = engine.pending_decision.as_ref().unwrap();
    assert_eq!(dec2["kind"], "targetSelect", "セットカード2枚リムーブ後にキャラ選択が開くこと");
    let id2 = dec2["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": id2,
        "revision": engine.revision,
        "value": {"targetIds": ["opp_lv4"]}
    })).unwrap();

    assert_eq!(engine.players["p2"].field.len(), 0, "レベル7以下のキャラがリムーブされること");
}



