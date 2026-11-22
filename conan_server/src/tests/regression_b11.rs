use super::*;

#[test]
fn test_bug5_b11009_mitsuhiko_contact_only_target_and_contact_abort() {
    let catalog = load_test_catalog();

    // 1) Fewer than 2 distinct lv4 少年探偵団 => B11009 cutin is NOT available
    {
        let mut engine = MatchEngine::new(catalog.clone());
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];
        engine.phase = models::MatchPhase::MAIN;

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let mut ps2 = engine::PlayerState::new("p2".to_string());
        ps1.partner = Some(test_inst("p1_p", "B01001", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
        ps1.r#case = Some(test_inst("p1_c", "B01024", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
        ps1.field.push(test_inst("p1_conan", "B01011", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("h_b11009", "B11009", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps2.field.push(test_inst("p2_contact", "B01011", "p2", models::Zone::FIELD, models::CardOrientation::SLEEP));
        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        engine.prompt_main_action("p1");
        let pd1 = engine.pending_decision.clone().unwrap();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd1["decisionId"],
            "revision": engine.revision,
            "value": {"type": "ACTION", "actorId": "p1_conan", "targetId": "p2_contact"}
        }));
        // Without 2 distinct lv4 少年探偵団, cutin cannot be used so attack resolves directly
        assert!(engine.players.get("p1").unwrap().hand.iter().any(|c| c.instance_id == "h_b11009"));
    }

    // 2) With 2 distinct lv4 少年探偵団: B11009 only offers contacting lv<=7 chars (NOT non-contacting idle or lv8) and aborts contact on remove
    {
        let mut engine = MatchEngine::new(catalog.clone());
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];
        engine.phase = models::MatchPhase::MAIN;

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let mut ps2 = engine::PlayerState::new("p2".to_string());
        ps1.partner = Some(test_inst("p1_p", "B01001", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
        ps1.r#case = Some(test_inst("p1_c", "B01024", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
        ps2.r#case = Some(test_inst("p2_c", "B01024", "p2", models::Zone::CASE, models::CardOrientation::ACTIVE));
        ps1.field.push(test_inst("p1_conan", "B01011", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE)); // lv4 少年探偵団 (江戸川コナン)
        ps1.field.push(test_inst("p1_genta", "B01014", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE)); // lv4 少年探偵団 (小嶋元太)
        ps1.hand.push(test_inst("h_b11009", "B11009", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps1.deck.push(test_inst("dk1", "B01011", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        // バッファ山札（B11009の【リムーブ時】1枚ドローで山札が0枚になった瞬間のリフレッシュを誘発させないための余裕）
        for i in 0..3 {
            ps1.deck.insert(0, test_inst(&format!("buf_cut_{}", i), "B01011", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        }

        ps2.field.push(test_inst("p2_contact", "B01016", "p2", models::Zone::FIELD, models::CardOrientation::SLEEP)); // lv4 AP4000
        ps2.field.push(test_inst("p2_idle", "B01014", "p2", models::Zone::FIELD, models::CardOrientation::SLEEP));    // lv4 AP5000 (not contacting)
        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        engine.prompt_main_action("p1");
        let pd1 = engine.pending_decision.clone().unwrap();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd1["decisionId"],
            "revision": engine.revision,
            "value": {"type": "ACTION", "actorId": "p1_conan", "targetId": "p2_contact"}
        }));

        let pd_cut = engine.pending_decision.clone().expect("B11009 cutin must be offered");
        assert_eq!(pd_cut["kind"], "guard");
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_cut["decisionId"],
            "revision": engine.revision,
            "value": {"cutinCardId": "h_b11009"}
        }));

        let pd_ts = engine.pending_decision.clone().expect("B11009 must prompt targetSelect");
        assert_eq!(pd_ts["kind"], "targetSelect");
        assert_eq!(pd_ts["options"]["effectType"], "REMOVE");
        let cands: Vec<&str> = pd_ts["options"]["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|c| c["instanceId"].as_str())
            .collect();
        assert!(cands.contains(&"p2_contact"), "Contacting lv<=7 opponent char must be selectable");
        assert!(!cands.contains(&"p2_idle"), "Non-contacting field char must NOT be selectable");
        assert!(!cands.contains(&"p1_genta"), "Non-contacting own field char must NOT be selectable");

        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_ts["decisionId"],
            "revision": engine.revision,
            "value": {"targetId": "p2_contact", "actorId": "h_b11009", "effectType": "REMOVE"}
        }));

        assert!(engine.players.get("p2").unwrap().remove.iter().any(|c| c.instance_id == "p2_contact"));
        assert!(engine.players.get("p2").unwrap().field.iter().any(|c| c.instance_id == "p2_idle"));
        assert!(engine.action_context.is_none(), "Contact must abort when contacting char leaves field");
        assert_eq!(engine.pending_decision.as_ref().unwrap()["kind"], "mainAction");
        assert!(engine.players.get("p1").unwrap().field.iter().any(|c| c.instance_id == "p1_conan"), "Attacker stays alive after contact abort");
        assert!(engine.players.get("p1").unwrap().hand.iter().any(|c| c.instance_id == "dk1"), "B11009 must draw 1 card");
    }

    // 3) Cross-card B01098 (コンタクト中のすべてのキャラをリムーブする): only removes contacting chars, leaves non-contacting chars
    {
        let mut engine = MatchEngine::new(catalog);
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];
        engine.phase = models::MatchPhase::MAIN;

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let mut ps2 = engine::PlayerState::new("p2".to_string());
        ps1.partner = Some(test_inst("p1_p", "B03106", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
        ps1.r#case = Some(test_inst("p1_c", "B01024", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
        ps1.field.push(test_inst("p1_atk", "B01098", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("h_b01098", "B01098", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps2.field.push(test_inst("p2_contact", "B01005", "p2", models::Zone::FIELD, models::CardOrientation::SLEEP));
        ps2.field.push(test_inst("p2_idle", "B01014", "p2", models::Zone::FIELD, models::CardOrientation::SLEEP));
        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        engine.prompt_main_action("p1");
        let pd1 = engine.pending_decision.clone().unwrap();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd1["decisionId"],
            "revision": engine.revision,
            "value": {"type": "ACTION", "actorId": "p1_atk", "targetId": "p2_contact"}
        }));
        let pd_cut = engine.pending_decision.clone().unwrap();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_cut["decisionId"],
            "revision": engine.revision,
            "value": {"cutinCardId": "h_b01098"}
        }));

        assert!(engine.players.get("p1").unwrap().remove.iter().any(|c| c.instance_id == "p1_atk"));
        assert!(engine.players.get("p2").unwrap().remove.iter().any(|c| c.instance_id == "p2_contact"));
        assert!(engine.players.get("p2").unwrap().field.iter().any(|c| c.instance_id == "p2_idle"), "B01098 must NOT remove non-contacting chars");
    }
}

#[test]
fn test_b11082_gin_vermouth_declare_stun_and_grant_on_remove_draw() {
    let catalog = catalog::CardCatalog::load_from_json("../data/dummy/cards.json").unwrap();
    let gv_id = catalog.definitions.keys().find(|k| k.starts_with("B11082")).cloned().expect("B11082 must exist");
    let gv_def = catalog.definitions.get(&gv_id).unwrap().clone();
    let declare_clauses = effects::extract_clauses(&gv_def.description, effects::TriggerKind::Declare, gv_def.card_type);
    assert!(!declare_clauses.is_empty(), "B11082 must have Declare clause");
    let plan = effects::parse_effect_steps(&declare_clauses[0], None, gv_def.card_type);
    assert!(plan.immediate_before.is_empty(), "Must not draw immediately without cost");
    assert!(plan.target_step.is_some());
    assert_eq!(plan.target_step.as_ref().unwrap().action, effects::TargetAction::StunAndGrantOnRemoveDraw);

    let mut engine = engine::MatchEngine::new(catalog);
    engine.turn = 1;
    engine.turn_player_id = "p1".to_string();
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];

    let mut ps1 = engine::PlayerState::new("p1".to_string());
    ps1.field.push(models::CardInstance {
        instance_id: "blk_lv6".to_string(),
        definition_id: "B11085".to_string(),
        owner_id: "p1".to_string(),
        zone: models::Zone::FIELD,
        orientation: models::CardOrientation::ACTIVE,
        ..Default::default()
    });
    ps1.deck.push(models::CardInstance {
        instance_id: "d1".to_string(),
        definition_id: "B11085".to_string(),
        owner_id: "p1".to_string(),
        zone: models::Zone::DECK,
        ..Default::default()
    });
    engine.players.insert("p1".to_string(), ps1);
    engine.players.insert("p2".to_string(), engine::PlayerState::new("p2".to_string()));

    // 効果適用をシミュレート
    engine.execute_clause_plan("p1", "gv_1", "ジン＆ベルモット", Some(9), &plan, "MAIN", None);
    let pd = engine.pending_decision.clone().expect("Must prompt target selection");
    assert_eq!(pd["kind"], "targetSelect");
    let _ = engine.submit_decision("p1", &serde_json::json!({
        "decisionId": pd["decisionId"],
        "revision": engine.revision,
        "value": {"targetId": "blk_lv6"}
    }));

    // 対象がスタンし、付与されていること
    let target = engine.players.get("p1").unwrap().field.iter().find(|c| c.instance_id == "blk_lv6").unwrap();
    assert_eq!(target.orientation, models::CardOrientation::STUN);
    assert!(engine.granted_on_remove_draw_until_opp_end.contains_key("blk_lv6"));

    // 現場からリムーブされたら1枚引くこと
    let hand_before = engine.players.get("p1").unwrap().hand.len();
    engine.trigger_on_remove_char("p1", "blk_lv6", "B11085", false);
    assert_eq!(engine.players.get("p1").unwrap().hand.len(), hand_before + 1);
}

#[test]
fn test_b11007_ayumi_on_enter_rush() {
    let catalog = catalog::CardCatalog::load_from_json("../data/dummy/cards.json").unwrap();
    let mut engine = engine::MatchEngine::new(catalog.clone());
    engine.turn = 2;
    engine.turn_player_id = "p1".to_string();
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];

    let mut ps1 = engine::PlayerState::new("p1".to_string());
    for i in 0..5 {
        ps1.file.push(models::CardInstance {
            instance_id: format!("f_{}", i),
            definition_id: "B01002".to_string(),
            owner_id: "p1".to_string(),
            zone: models::Zone::FILE,
            ..Default::default()
        });
    }
    ps1.field.push(models::CardInstance {
        instance_id: "ayumi_1".to_string(),
        definition_id: "B11007".to_string(),
        owner_id: "p1".to_string(),
        zone: models::Zone::FIELD,
        orientation: models::CardOrientation::ACTIVE,
        entered_turn: 2,
        ..Default::default()
    });
    engine.players.insert("p1".to_string(), ps1);
    engine.players.insert("p2".to_string(), engine::PlayerState::new("p2".to_string()));

    let b11007 = catalog.get("B11007").unwrap();
    let clauses = effects::extract_clauses(&b11007.description, effects::TriggerKind::OnPlay, b11007.card_type);
    assert!(!clauses.is_empty(), "B11007 must have OnPlay clause: {}", b11007.description);

    engine.resolve_character_enter_field_with_source(
        "p1",
        "ayumi_1",
        "B11007",
        models::CardOrientation::ACTIVE,
        None,
        "main",
    );

    let ayumi = engine.players["p1"].field.iter().find(|c| c.instance_id == "ayumi_1")
        .expect("ayumi_1 must be on field");
    assert!(
        ayumi.has_rush,
        "FILE5条件を満たして登場したB11007吉田歩美はhas_rush=trueになること (logs: {:?})",
        engine.logs
    );
}

#[test]
fn test_b11093_gin_and_vermouth_mode_condition() {
    let catalog = catalog::CardCatalog::load_from_json("../data/dummy/cards.json").unwrap();
    let make_card = |id: &str, def_id: &str, owner: &str, zone: models::Zone, ori: models::CardOrientation| {
        models::CardInstance {
            instance_id: id.to_string(),
            definition_id: def_id.to_string(),
            owner_id: owner.to_string(),
            zone,
            orientation: ori,
            face_up: true,
            ..Default::default()
        }
    };

    let setup_engine = |field_defs: Vec<(&str, &str)>, partner_def: &str, extra_partner_defs: Vec<(&str, &str)>| {
        let mut engine = engine::MatchEngine::new(catalog.clone());
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        ps1.case_phase = models::CasePhase::RESOLUTION;
        ps1.partner = Some(make_card(
            "p1_partner",
            partner_def,
            "p1",
            models::Zone::PARTNER,
            models::CardOrientation::ACTIVE,
        ));
        for (eid, edef) in extra_partner_defs {
            ps1.extra_partners.push(make_card(
                eid,
                edef,
                "p1",
                models::Zone::PARTNER,
                models::CardOrientation::ACTIVE,
            ));
        }
        for i in 0..8 {
            ps1.file.push(make_card(
                &format!("f_{}", i),
                "B03107",
                "p1",
                models::Zone::FILE,
                models::CardOrientation::ACTIVE,
            ));
        }
        for (fid, fdef) in field_defs {
            ps1.field.push(make_card(
                fid,
                fdef,
                "p1",
                models::Zone::FIELD,
                models::CardOrientation::ACTIVE,
            ));
        }
        ps1.hand.push(make_card(
            "b11093_card",
            "B11093",
            "p1",
            models::Zone::HAND,
            models::CardOrientation::ACTIVE,
        ));
        for i in 0..4 {
            ps1.deck.push(make_card(
                &format!("d_{}", i),
                "B06103",
                "p1",
                models::Zone::DECK,
                models::CardOrientation::ACTIVE,
            ));
        }

        let mut ps2 = engine::PlayerState::new("p2".to_string());
        ps2.field.push(make_card(
            "opp_c1",
            "B01005",
            "p2",
            models::Zone::FIELD,
            models::CardOrientation::ACTIVE,
        ));
        ps2.field.push(make_card(
            "opp_c2",
            "B01005",
            "p2",
            models::Zone::FIELD,
            models::CardOrientation::ACTIVE,
        ));

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);
        engine.prompt_main_action("p1");
        engine
    };

    let play_b11093 = |engine: &mut engine::MatchEngine| {
        let obs = build_player_observation(engine, "p1");
        let dec_id = obs["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
        engine.submit_decision("p1", &serde_json::json!({
            "decisionId": dec_id,
            "revision": engine.revision,
            "value": { "type": "HAND_PLAY", "cardId": "b11093_card" }
        })).unwrap();
    };

    // 1. 現場・パートナーにジン＆ベルモットがいない（単体のジンとベルモットのみいる）場合は1つ選択になる
    {
        let mut engine = setup_engine(
            vec![("gin_1", "B06103"), ("verm_1", "B10095")],
            "B03107",
            vec![],
        );
        play_b11093(&mut engine);
        let obs = build_player_observation(&engine, "p1");
        assert_eq!(
            obs["pendingDecision"]["kind"].as_str(),
            Some("modeSelect"),
            "ジン＆ベルモット不在時は両発動せずmodeSelectになること (logs: {:?})",
            engine.logs
        );
        assert_eq!(obs["pendingDecision"]["options"]["maxPick"].as_u64(), Some(1));
        assert!(
            !engine.logs.iter().any(|l| l.contains("選択肢をすべて上から順に行います")),
            "条件未達時に全実行ログが出ないこと"
        );
    }

    // 2. 現場にB11082（ジン＆ベルモット）がいる場合は両方上から順に実行される
    {
        let mut engine = setup_engine(
            vec![("gv_field", "B11082")],
            "B03107",
            vec![],
        );
        play_b11093(&mut engine);
        assert!(
            engine.logs.iter().any(|l| l.contains("選択肢をすべて上から順に行います")),
            "現場にB11082がいる場合は全実行ログが出ること (logs: {:?})",
            engine.logs
        );
        // 1つ目のモード「キャラを1枚まで選び、スリープさせる。カードを1枚引く。」が開始される
        let obs1 = build_player_observation(&engine, "p1");
        assert_eq!(obs1["pendingDecision"]["kind"].as_str(), Some("targetSelect"));
        assert_eq!(obs1["pendingDecision"]["options"]["effectType"].as_str(), Some("SLEEP"));
        let dec1 = obs1["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
        engine.submit_decision("p1", &serde_json::json!({
            "decisionId": dec1,
            "revision": engine.revision,
            "value": { "targetId": "opp_c1" }
        })).unwrap();

        assert_eq!(engine.players["p1"].hand.len(), 1, "1つ目の選択肢で1枚ドローしていること");
        // 続いて2つ目のモード「自分の現場にいる【黒】のキャラを1枚リムーブしてもよい〜」へ進む
        let obs2 = build_player_observation(&engine, "p1");
        assert_eq!(
            obs2["pendingDecision"]["kind"].as_str(),
            Some("targetSelect"),
            "1つ目完了後に2つ目のモードへ遷移すること (logs: {:?})",
            engine.logs
        );
        assert_eq!(obs2["pendingDecision"]["options"]["effectType"].as_str(), Some("REMOVE"));
    }

    // 3. パートナーエリア（extra_partners）にB11082（ジン＆ベルモット）がいる場合も両方実行される
    {
        let mut engine = setup_engine(
            vec![("blk_1", "B11085")],
            "B03107",
            vec![("gv_partner", "B11082")],
        );
        play_b11093(&mut engine);
        assert!(
            engine.logs.iter().any(|l| l.contains("選択肢をすべて上から順に行います")),
            "パートナーエリアにB11082がいる場合も全実行ログが出ること (logs: {:?})",
            engine.logs
        );
        let obs = build_player_observation(&engine, "p1");
        assert_eq!(obs["pendingDecision"]["kind"].as_str(), Some("targetSelect"));
        assert_eq!(obs["pendingDecision"]["options"]["effectType"].as_str(), Some("SLEEP"));
    }
}

#[test]
fn test_b11086_vermouth_declare_level_limit_and_trigger_order() {
    let catalog = catalog::CardCatalog::load_from_json("../data/dummy/cards.json").unwrap();
    let mut engine = engine::MatchEngine::new(catalog.clone());
    engine.turn = 6;
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
    ps1.field.push(models::CardInstance {
        instance_id: "vermouth_1".to_string(),
        definition_id: "B11086".to_string(), // Lv8 ベルモット
        owner_id: "p1".to_string(),
        zone: models::Zone::FIELD,
        orientation: models::CardOrientation::ACTIVE,
        ..Default::default()
    });
    ps1.field.push(models::CardInstance {
        instance_id: "vodka_lv5".to_string(),
        definition_id: "B03116".to_string(), // Lv5 ウォッカ
        owner_id: "p1".to_string(),
        zone: models::Zone::FIELD,
        orientation: models::CardOrientation::ACTIVE,
        ..Default::default()
    });
    ps1.deck.push(models::CardInstance {
        instance_id: "d_1".to_string(),
        definition_id: "B01097".to_string(),
        owner_id: "p1".to_string(),
        zone: models::Zone::DECK,
        ..Default::default()
    });

    let mut ps2 = engine::PlayerState::new("p2".to_string());
    ps2.field.push(models::CardInstance {
        instance_id: "opp_genta_lv8".to_string(),
        definition_id: "B07008".to_string(), // Lv8 小嶋元太 (SLEEP) -> Lv5以下ではないため対象外であるべき！
        owner_id: "p2".to_string(),
        zone: models::Zone::FIELD,
        orientation: models::CardOrientation::SLEEP,
        ..Default::default()
    });
    ps2.field.push(models::CardInstance {
        instance_id: "opp_low_lv4".to_string(),
        definition_id: "B01014".to_string(), // Lv4 小嶋元太 (SLEEP) -> Lv5以下なので対象になるべき！
        owner_id: "p2".to_string(),
        zone: models::Zone::FIELD,
        orientation: models::CardOrientation::SLEEP,
        ..Default::default()
    });

    engine.players.insert("p1".to_string(), ps1);
    engine.players.insert("p2".to_string(), ps2);
    engine.prompt_main_action("p1");

    // 1) B11086 ベルモットの【宣言】を発動
    let obs1 = build_player_observation(&engine, "p1");
    let dec1 = obs1["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": dec1,
        "revision": engine.revision,
        "value": { "type": "DECLARE", "actorId": "vermouth_1" }
    })).unwrap();

    // 2) コストとして自分の現場の黒キャラ vodka_lv5 (B03116, Lv5) をリムーブ
    let obs2 = build_player_observation(&engine, "p1");
    let dec2 = obs2["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": dec2,
        "revision": engine.revision,
        "value": { "targetId": "vodka_lv5" }
    })).unwrap();

    // 3) 公式ルール通り、まずベルモットの「そうした場合、リムーブしたキャラのレベル以下のレベルのスリープ状態のキャラを1枚まで選び、リムーブする」が先に解決され、
    //    候補には Lv4 の opp_low_lv4 のみが含まれ、Lv8 の opp_genta_lv8 (B07008) は含まれないこと！
    let obs3 = build_player_observation(&engine, "p1");
    assert_eq!(obs3["pendingDecision"]["kind"].as_str(), Some("targetSelect"));
    assert_eq!(obs3["pendingDecision"]["options"]["actorId"].as_str(), Some("vermouth_1"), "ウォッカの誘発より先にベルモットの後続効果（そうした場合〜）を解決すること");
    let cands: Vec<&str> = obs3["pendingDecision"]["options"]["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["instanceId"].as_str())
        .collect();
    assert!(cands.contains(&"opp_low_lv4"), "Lv5以下のスリープキャラ(opp_low_lv4)が候補に含まれること: {:?}", cands);
    assert!(!cands.contains(&"opp_genta_lv8"), "Lv8の小嶋元太(B07008)はLv5以下ではないため候補に含まれないこと: {:?}", cands);

    let dec3 = obs3["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": dec3,
        "revision": engine.revision,
        "value": { "targetId": "opp_low_lv4" }
    })).unwrap();

    // 4) ベルモットの宣言効果がすべて終わった後に、待機していたウォッカ(B03116)の自己蘇生選択が表示されること！
    let obs4 = build_player_observation(&engine, "p1");
    assert_eq!(obs4["pendingDecision"]["kind"].as_str(), Some("targetSelect"));
    assert_eq!(obs4["pendingDecision"]["options"]["actorId"].as_str(), Some("vodka_lv5"));
}

#[test]
fn test_b11056_actor_ap_or_less_remove_targets_opponent_not_self() {
    let mut catalog = load_test_catalog();
    catalog.definitions.insert(
        "TEST_CASE_GR".to_string(),
        models::CardDefinition {
            definition_id: "TEST_CASE_GR".to_string(),
            canonical_id: "TEST_CASE_GR".to_string(),
            card_type: models::CardType::CASE,
            colors: vec!["緑".to_string(), "赤".to_string()],
            level: 0,
            ap: 0,
            lp: 0,
            names: vec!["緑赤事件".to_string()],
            description: String::new(),
            traits: vec![],
            case_levels: None,
            main_thumb: None,
            main_path: None,
        },
    );

    let mut engine = MatchEngine::new(catalog);
    engine.turn = 4;
    engine.turn_player_id = "p1".to_string();
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];
    engine.phase = models::MatchPhase::MAIN;

    let mut ps1 = engine::PlayerState::new("p1".to_string());
    ps1.r#case = Some(test_inst("p1_case", "TEST_CASE_GR", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
    ps1.field.push(test_inst("haneda", "B11056", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE)); // AP5000

    let mut ps2 = engine::PlayerState::new("p2".to_string());
    ps2.field.push(test_inst("opp_low", "B05055", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE)); // AP4000
    ps2.field.push(test_inst("opp_high", "B05046", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE)); // AP7000

    engine.players.insert("p1".to_string(), ps1);
    engine.players.insert("p2".to_string(), ps2);
    engine.prompt_main_action("p1");

    let obs1 = build_player_observation(&engine, "p1");
    let dec1 = obs1["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": dec1,
        "revision": engine.revision,
        "value": { "type": "DECLARE", "actorId": "haneda" }
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
    let cands: Vec<&str> = obs2["pendingDecision"]["options"]["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["instanceId"].as_str())
        .collect();
    assert!(cands.contains(&"opp_low"), "AP4000の相手キャラが候補に含まれること: {:?}", cands);
    assert!(!cands.contains(&"opp_high"), "AP7000の相手キャラは候補に含まれないこと: {:?}", cands);
    assert!(!cands.contains(&"haneda"), "自分自身(haneda)が誤って候補に含まれないこと: {:?}", cands);

    let dec2 = obs2["pendingDecision"]["decisionId"].as_str().unwrap().to_string();
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": dec2,
        "revision": engine.revision,
        "value": { "targetId": "opp_low" }
    })).unwrap();

    assert!(engine.players["p2"].remove.iter().any(|c| c.instance_id == "opp_low"));
    assert!(engine.players["p1"].field.iter().any(|c| c.instance_id == "haneda"));
}

#[test]
fn test_b11022_kazuha_action_trigger_fires_after_b11096_action_trigger() {
    let mut catalog = load_test_catalog();
    catalog.definitions.insert(
        "TEST_CASE_GR_22".to_string(),
        models::CardDefinition {
            definition_id: "TEST_CASE_GR_22".to_string(),
            canonical_id: "TEST_CASE_GR_22".to_string(),
            card_type: models::CardType::CASE,
            colors: vec!["緑".to_string(), "赤".to_string()],
            level: 0,
            ap: 0,
            lp: 0,
            names: vec!["緑赤事件".to_string()],
            description: String::new(),
            traits: vec![],
            case_levels: None,
            main_thumb: None,
            main_path: None,
        },
    );
    catalog.definitions.insert(
        "TEST_RED_LV2".to_string(),
        models::CardDefinition {
            definition_id: "TEST_RED_LV2".to_string(),
            canonical_id: "TEST_RED_LV2".to_string(),
            card_type: models::CardType::CHARACTER,
            colors: vec!["赤".to_string()],
            level: 2,
            ap: 2000,
            lp: 1,
            names: vec!["赤Lv2".to_string()],
            description: String::new(),
            traits: vec![],
            case_levels: None,
            main_thumb: None,
            main_path: None,
        },
    );

    let mut engine = MatchEngine::new(catalog);
    engine.phase = models::MatchPhase::MAIN;
    engine.turn = 9;
    engine.turn_player_id = "p1".to_string();
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];

    let mut ps1 = engine::PlayerState::new("p1".to_string());
    ps1.r#case = Some(test_inst("p1_case", "TEST_CASE_GR_22", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
    ps1.field.push(test_inst("heiji_jodie", "B11096", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
    let mut kazuha = test_inst("kazuha_22", "B11022", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE);
    kazuha.has_rush = true;
    ps1.field.push(kazuha);
    ps1.hand.push(test_inst("h_cost", "B01005", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
    ps1.remove.push(test_inst("rm_lv2", "TEST_RED_LV2", "p1", models::Zone::REMOVE, models::CardOrientation::ACTIVE));
    ps1.deck.push(test_inst("d1", "B01005", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
    ps1.deck.push(test_inst("d2", "B01005", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));

    let mut ps2 = engine::PlayerState::new("p2".to_string());
    ps2.field.push(test_inst("opp_low", "B05054", "p2", models::Zone::FIELD, models::CardOrientation::SLEEP)); // AP1000
    ps2.deck.push(test_inst("p2_d1", "B01005", "p2", models::Zone::DECK, models::CardOrientation::ACTIVE));

    engine.players.insert("p1".to_string(), ps1);
    engine.players.insert("p2".to_string(), ps2);
    engine.prompt_main_action("p1");

    // 1) B11022 遠山和葉で AP1000 の相手キャラに攻撃
    let obs1 = build_player_observation(&engine, "p1");
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": obs1["pendingDecision"]["decisionId"],
        "revision": engine.revision,
        "value": { "type": "ACTION", "actorId": "kazuha_22", "targetId": "opp_low" }
    })).unwrap();

    // 2) 先に B11096 のアクション時誘発（手札を1枚リムーブしてもよい）が開く
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
    assert_eq!(obs2["pendingDecision"]["options"]["effectType"].as_str(), Some("DISCARD_HAND"));
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": obs2["pendingDecision"]["decisionId"],
        "revision": engine.revision,
        "value": { "targetId": "h_cost" }
    })).unwrap();

    // 3) 続いて、キューに退避されていた B11022 のターン1誘発（リムーブエリアからLv2緑/赤キャラ登場）が開くこと！
    let obs3 = build_player_observation(&engine, "p1");
    assert_eq!(
        obs3["pendingDecision"]["kind"].as_str(),
        Some("targetSelect"),
        "B11096解決後にB11022のアクション時誘発targetSelectが表示されること (logs: {:?})",
        engine.logs
    );
    assert_eq!(obs3["pendingDecision"]["options"]["actorId"].as_str(), Some("kazuha_22"));
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": obs3["pendingDecision"]["decisionId"],
        "revision": engine.revision,
        "value": { "targetId": "rm_lv2" }
    })).unwrap();

    assert!(
        engine.players["p1"].field.iter().any(|c| c.instance_id == "rm_lv2"),
        "B11022の効果でリムーブエリアのLv2赤キャラが場に登場していること"
    );
}

#[test]
fn test_b11022_deployed_char_with_on_play_discard_resumes_contact() {
    let mut catalog = load_test_catalog();
    catalog.definitions.insert(
        "TEST_CASE_GR_22B".to_string(),
        models::CardDefinition {
            definition_id: "TEST_CASE_GR_22B".to_string(),
            canonical_id: "TEST_CASE_GR_22B".to_string(),
            card_type: models::CardType::CASE,
            colors: vec!["緑".to_string(), "赤".to_string()],
            level: 0,
            ap: 0,
            lp: 0,
            names: vec!["緑赤事件".to_string()],
            description: String::new(),
            traits: vec![],
            case_levels: None,
            main_thumb: None,
            main_path: None,
        },
    );

    let mut engine = MatchEngine::new(catalog);
    engine.phase = models::MatchPhase::MAIN;
    engine.turn = 9;
    engine.turn_player_id = "p1".to_string();
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];

    let mut ps1 = engine::PlayerState::new("p1".to_string());
    ps1.case_phase = models::CasePhase::RESOLUTION;
    ps1.r#case = Some(test_inst("p1_case", "TEST_CASE_GR_22B", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
    let mut kazuha = test_inst("kazuha_22", "B11022", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE);
    kazuha.has_rush = true;
    ps1.field.push(kazuha);
    ps1.remove.push(test_inst("james_61", "B11061", "p1", models::Zone::REMOVE, models::CardOrientation::ACTIVE)); // Lv2 赤 登場時1ドロー1リムーブ
    ps1.deck.push(test_inst("d1", "B01005", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
    ps1.deck.push(test_inst("d2", "B01005", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));

    let mut ps2 = engine::PlayerState::new("p2".to_string());
    ps2.field.push(test_inst("opp_low", "B05054", "p2", models::Zone::FIELD, models::CardOrientation::SLEEP)); // AP1000
    ps2.deck.push(test_inst("p2_d1", "B01005", "p2", models::Zone::DECK, models::CardOrientation::ACTIVE));

    engine.players.insert("p1".to_string(), ps1);
    engine.players.insert("p2".to_string(), ps2);
    engine.prompt_main_action("p1");

    // 1) B11022 で AP1000 の相手キャラに攻撃
    let obs1 = build_player_observation(&engine, "p1");
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": obs1["pendingDecision"]["decisionId"],
        "revision": engine.revision,
        "value": { "type": "ACTION", "actorId": "kazuha_22", "targetId": "opp_low" }
    })).unwrap();

    // 2) B11022 のアクション時誘発で B11061 をスリープ状態で登場させる
    let obs2 = build_player_observation(&engine, "p1");
    assert_eq!(obs2["pendingDecision"]["options"]["actorId"].as_str(), Some("kazuha_22"));
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": obs2["pendingDecision"]["decisionId"],
        "revision": engine.revision,
        "value": { "targetId": "james_61" }
    })).unwrap();

    // 3) B11061 の登場時効果（1ドロー後の手札1枚リムーブ）を解決
    let obs3 = build_player_observation(&engine, "p1");
    assert_eq!(obs3["pendingDecision"]["kind"].as_str(), Some("targetSelect"));
    assert_eq!(obs3["pendingDecision"]["options"]["effectType"].as_str(), Some("DISCARD_HAND"));
    engine.submit_decision("p1", &serde_json::json!({
        "decisionId": obs3["pendingDecision"]["decisionId"],
        "revision": engine.revision,
        "value": { "targetId": "d2" }
    })).unwrap();

    // 4) 登場時効果の解決後、中断されていたコンタクトが再開・決着し、opp_low がリムーブされて mainAction に戻ること！
    assert!(
        engine.action_context.is_none(),
        "コンタクトが完了し action_context がクリアされていること (logs: {:?})",
        engine.logs
    );
    assert!(
        engine.players["p2"].remove.iter().any(|c| c.instance_id == "opp_low"),
        "コンタクト決着により opp_low がリムーブされていること"
    );
    assert_eq!(
        build_player_observation(&engine, "p1")["pendingDecision"]["kind"].as_str(),
        Some("mainAction")
    );
}

#[test]
fn test_b11003_multi_declare_self_lv4_minus_and_b11057_b11017_full_scenario() {
    let catalog = load_test_catalog();

    // 1) B11003 灰原哀の2つの宣言効果の選択・レベル-4発動・B11057のレベル-1がリムーブ後/ターン経過後に残らないこと・B11017(2枚出してもB11003発火&手札送り&突撃のみtriggerOrder抑制)
    {
        let mut engine = MatchEngine::new(catalog.clone());
        engine.phase = models::MatchPhase::MAIN;
        engine.turn = 6;
        engine.turn_player_id = "p2".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        ps1.partner = Some(test_inst("p1_p", "B03001", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE)); // 青パートナー
        for i in 0..7 {
            ps1.file.push(test_inst(&format!("f1_{}", i), "B01021", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
        }
        ps1.field.push(test_inst("genta_old", "B11008", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.field.push(test_inst("ayumi_1", "D08008", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        for i in 0..8 {
            ps1.deck.push(test_inst(&format!("p1d_{}", i), "B01005", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        }

        let mut ps2 = engine::PlayerState::new("p2".to_string());
        ps2.field.push(test_inst("akai_57", "B11057", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        for i in 0..5 {
            ps2.deck.push(test_inst(&format!("p2d_{}", i), "B01005", "p2", models::Zone::DECK, models::CardOrientation::ACTIVE));
        }
        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        // ターン6 (p2): B11057 の登場時で p1 の B11008 のレベルを -1 し、その後リムーブする
        assert!(engine.resolve_on_play_effects_with_source("p2", "akai_57", "B11057", false, None, "MAIN"));
        let obs_akai = build_player_observation(&engine, "p2");
        engine.submit_decision("p2", &serde_json::json!({
            "decisionId": obs_akai["pendingDecision"]["decisionId"],
            "revision": engine.revision,
            "value": { "targetId": "genta_old" }
        })).unwrap();
        assert_eq!(engine.players["p1"].field.iter().find(|c| c.instance_id == "genta_old").unwrap().temp_level_delta, -1);

        // リムーブされた時点で temp_level_delta は 0 にクリアされること
        assert!(engine.move_field_char_to_remove("p1", "genta_old", true, false).is_some());
        let genta_rm = engine.players["p1"].remove.iter().find(|c| c.instance_id == "genta_old").unwrap();
        assert_eq!(genta_rm.temp_level_delta, 0, "現場を離れてリムーブされたキャラのtemp_level_deltaは即クリアされること");

        // ターン7 (p1) 開始シミュレーション
        engine.start_turn(7, "p1");
        engine.players.get_mut("p1").unwrap().field.push(test_inst("ai_03", "B11003", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        engine.players.get_mut("p1").unwrap().hand.push(test_inst("gonin", "B11017", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        engine.players.get_mut("p1").unwrap().remove.push(test_inst("mitsu_9", "B11009", "p1", models::Zone::REMOVE, models::CardOrientation::ACTIVE));
        engine.prompt_main_action("p1");

        // B11003 の【宣言】を発動 -> 2つの宣言効果があるため modeSelect で選択できること！
        let obs_m1 = build_player_observation(&engine, "p1");
        engine.submit_decision("p1", &serde_json::json!({
            "decisionId": obs_m1["pendingDecision"]["decisionId"],
            "revision": engine.revision,
            "value": { "type": "DECLARE", "actorId": "ai_03" }
        })).unwrap();

        let obs_dec_mode = build_player_observation(&engine, "p1");
        assert_eq!(obs_dec_mode["pendingDecision"]["kind"].as_str(), Some("modeSelect"));
        assert_eq!(obs_dec_mode["pendingDecision"]["options"]["specialKind"].as_str(), Some("DECLARE_CLAUSE_SELECT"));
        let modes = obs_dec_mode["pendingDecision"]["options"]["modes"].as_array().unwrap();
        assert_eq!(modes.len(), 2, "B11003の2つの宣言効果が選択肢に表示されること");
        let lv_idx = modes.iter().position(|m| m.as_str().unwrap_or("").contains("レベル－4")).unwrap();

        engine.submit_decision("p1", &serde_json::json!({
            "decisionId": obs_dec_mode["pendingDecision"]["decisionId"],
            "revision": engine.revision,
            "value": { "modeIndex": lv_idx }
        })).unwrap();

        // デッキ3枚リムーブ＋自身のレベルが 8 -> 4 になっていること！
        let ai_after_dec = engine.players["p1"].field.iter().find(|c| c.instance_id == "ai_03").unwrap();
        assert_eq!(ai_after_dec.temp_level_delta, -4, "B11003のレベル-4効果が適用されていること (logs: {:?})", engine.logs);

        // 2つ目の【宣言】（【スリープ】：Lv4少年探偵団 AP+2000）も同ターン中に発動でき、B11003自身(Lv4になった)や他を強化できること！
        let obs_m2 = build_player_observation(&engine, "p1");
        engine.submit_decision("p1", &serde_json::json!({
            "decisionId": obs_m2["pendingDecision"]["decisionId"],
            "revision": engine.revision,
            "value": { "type": "DECLARE", "actorId": "ai_03" }
        })).unwrap();
        let obs_buff = build_player_observation(&engine, "p1");
        assert_eq!(obs_buff["pendingDecision"]["kind"].as_str(), Some("targetSelect"));
        engine.submit_decision("p1", &serde_json::json!({
            "decisionId": obs_buff["pendingDecision"]["decisionId"],
            "revision": engine.revision,
            "value": { "targetId": "ai_03" }
        })).unwrap();
        assert_eq!(engine.players["p1"].field.iter().find(|c| c.instance_id == "ai_03").unwrap().temp_ap_bonus, 2000);

        // 五人囃子 (B11017) でリムーブエリアの B11008 と B11009 を登場させる
        let obs_gonin = build_player_observation(&engine, "p1");
        engine.submit_decision("p1", &serde_json::json!({
            "decisionId": obs_gonin["pendingDecision"]["decisionId"],
            "revision": engine.revision,
            "value": { "type": "HAND_PLAY", "cardId": "gonin" }
        })).unwrap();

        let obs_g1 = build_player_observation(&engine, "p1");
        assert_eq!(obs_g1["pendingDecision"]["options"]["effectType"].as_str(), Some("REVIVE_SPLIT_GONIN"));
        engine.submit_decision("p1", &serde_json::json!({
            "decisionId": obs_g1["pendingDecision"]["decisionId"],
            "revision": engine.revision,
            "value": { "targetIds": ["genta_old", "mitsu_9"] }
        })).unwrap();

        let obs_g2 = build_player_observation(&engine, "p1");
        assert_eq!(obs_g2["pendingDecision"]["options"]["effectType"].as_str(), Some("REVIVE_SPLIT_PICK_ACTIVE"));
        engine.submit_decision("p1", &serde_json::json!({
            "decisionId": obs_g2["pendingDecision"]["decisionId"],
            "revision": engine.revision,
            "value": { "targetId": "genta_old" }
        })).unwrap();

        let obs_g3 = build_player_observation(&engine, "p1");
        assert_eq!(obs_g3["pendingDecision"]["options"]["effectType"].as_str(), Some("REVIVE_SPLIT_PICK_SLEEP"));
        engine.submit_decision("p1", &serde_json::json!({
            "decisionId": obs_g3["pendingDecision"]["decisionId"],
            "revision": engine.revision,
            "value": { "targetId": "mitsu_9" }
        })).unwrap();

        // B11008(突撃のみ) と B11009(突撃のみ) が登場した際、triggerOrder選択肢は出ず、そのまま B11003 のリムーブ効果の targetSelect が表示されること！
        let obs_ai_rem = build_player_observation(&engine, "p1");
        assert_eq!(
            obs_ai_rem["pendingDecision"]["kind"].as_str(),
            Some("targetSelect"),
            "突撃のみ同士でtriggerOrderが出ず、即座にB11003のリムーブ効果が発火すること (logs: {:?})",
            engine.logs
        );
        assert_eq!(obs_ai_rem["pendingDecision"]["options"]["actorId"].as_str(), Some("ai_03"));
        assert_eq!(obs_ai_rem["pendingDecision"]["options"]["effectType"].as_str(), Some("REMOVE"));
        engine.submit_decision("p1", &serde_json::json!({
            "decisionId": obs_ai_rem["pendingDecision"]["decisionId"],
            "revision": engine.revision,
            "value": { "targetId": "akai_57" }
        })).unwrap();
        assert!(engine.players["p2"].remove.iter().any(|c| c.instance_id == "akai_57"));
    }

    // 2) B11017 でリムーブエリアに2枚しかないときでも「1枚登場＋スリープ登場スキップ ➔ 残り1枚を手札に加える」ができること
    {
        let mut engine = MatchEngine::new(catalog.clone());
        engine.phase = models::MatchPhase::MAIN;
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        ps1.hand.push(test_inst("gonin2", "B11017", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps1.remove.push(test_inst("r1", "B11008", "p1", models::Zone::REMOVE, models::CardOrientation::ACTIVE));
        ps1.remove.push(test_inst("r2", "B11009", "p1", models::Zone::REMOVE, models::CardOrientation::ACTIVE));
        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), engine::PlayerState::new("p2".to_string()));
        engine.prompt_main_action("p1");

        let obs1 = build_player_observation(&engine, "p1");
        engine.submit_decision("p1", &serde_json::json!({
            "decisionId": obs1["pendingDecision"]["decisionId"],
            "revision": engine.revision,
            "value": { "type": "HAND_PLAY", "cardId": "gonin2" }
        })).unwrap();

        let obs2 = build_player_observation(&engine, "p1");
        engine.submit_decision("p1", &serde_json::json!({
            "decisionId": obs2["pendingDecision"]["decisionId"],
            "revision": engine.revision,
            "value": { "targetIds": ["r1", "r2"] }
        })).unwrap();

        let obs3 = build_player_observation(&engine, "p1");
        engine.submit_decision("p1", &serde_json::json!({
            "decisionId": obs3["pendingDecision"]["decisionId"],
            "revision": engine.revision,
            "value": { "targetId": "r1" }
        })).unwrap();

        // スリープ登場をスキップ
        let obs4 = build_player_observation(&engine, "p1");
        assert_eq!(obs4["pendingDecision"]["options"]["effectType"].as_str(), Some("REVIVE_SPLIT_PICK_SLEEP"));
        engine.submit_decision("p1", &serde_json::json!({
            "decisionId": obs4["pendingDecision"]["decisionId"],
            "revision": engine.revision,
            "value": { "targetIds": [] }
        })).unwrap();

        let p1 = &engine.players["p1"];
        assert!(p1.field.iter().any(|c| c.instance_id == "r1"));
        assert!(p1.hand.iter().any(|c| c.instance_id == "r2"), "2枚しかない状態でもスリープ登場をスキップして残りを手札に加えられること");
    }

    // 3) 登場時効果1件(非突撃) + 突撃のみ1件が同時に出ても triggerOrder 選択肢が出ないこと
    {
        let mut engine = MatchEngine::new(catalog.clone());
        engine.phase = models::MatchPhase::MAIN;
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        for i in 0..5 {
            ps1.file.push(test_inst(&format!("f_{}", i), "B01021", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
        }
        ps1.field.push(test_inst("c_rush", "B11008", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE)); // 【FILE5】【登場時】突撃
        ps1.field.push(test_inst("c_debuff", "B11057", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE)); // 【自分ターン中】【登場時】レベル-1
        engine.players.insert("p1".to_string(), ps1);
        let mut ps2 = engine::PlayerState::new("p2".to_string());
        ps2.field.push(test_inst("opp_c", "B05055", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        engine.players.insert("p2".to_string(), ps2);

        let deployed = vec![
            ("c_rush".to_string(), "B11008".to_string(), models::CardOrientation::ACTIVE),
            ("c_debuff".to_string(), "B11057".to_string(), models::CardOrientation::ACTIVE),
        ];
        engine.resolve_deployed_characters("p1", None, &deployed, "MAIN");
        let obs = build_player_observation(&engine, "p1");
        assert_ne!(
            obs["pendingDecision"]["kind"].as_str(),
            Some("triggerOrder"),
            "突撃のみと非突撃の同時登場時はtriggerOrderを出さずに直接解決へ進むこと (logs: {:?})",
            engine.logs
        );
        assert_eq!(obs["pendingDecision"]["kind"].as_str(), Some("targetSelect"));
        assert!(engine.players["p1"].field.iter().find(|c| c.instance_id == "c_rush").unwrap().has_rush);
    }

    // 4) 非突撃の登場時効果2件が同時に出た場合は従来通り triggerOrder 選択肢が出ること
    {
        let mut engine = MatchEngine::new(catalog.clone());
        engine.phase = models::MatchPhase::MAIN;
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        ps1.field.push(test_inst("c_1", "B11057", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.field.push(test_inst("c_2", "B11057", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        engine.players.insert("p1".to_string(), ps1);
        let mut ps2 = engine::PlayerState::new("p2".to_string());
        ps2.field.push(test_inst("opp_c", "B05055", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        engine.players.insert("p2".to_string(), ps2);

        let deployed = vec![
            ("c_1".to_string(), "B11057".to_string(), models::CardOrientation::ACTIVE),
            ("c_2".to_string(), "B11057".to_string(), models::CardOrientation::ACTIVE),
        ];
        engine.resolve_deployed_characters("p1", None, &deployed, "MAIN");
        let obs = build_player_observation(&engine, "p1");
        assert_eq!(obs["pendingDecision"]["kind"].as_str(), Some("triggerOrder"));
    }
}

