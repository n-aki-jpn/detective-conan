use super::*;

#[test]
fn test_bug1_and_bug4_rush_variants_swift_and_deduce_restrictions() {
    let catalog = load_test_catalog();

    // 1) Bug 1: B07094 (突撃[事件]) must NOT deduce on entered turn, only attack CASE
    {
        let mut engine = MatchEngine::new(catalog.clone());
        engine.turn = 3;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];
        engine.phase = models::MatchPhase::MAIN;

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let mut ps2 = engine::PlayerState::new("p2".to_string());
        ps1.partner = Some(test_inst("p1_p", "B03106", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE)); // 黒
        ps1.r#case = Some(test_inst("p1_c", "B01024", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
        ps2.r#case = Some(test_inst("p2_c", "B01024", "p2", models::Zone::CASE, models::CardOrientation::ACTIVE));
        let mut gin = test_inst("p1_gin", "B07094", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE);
        gin.entered_turn = 3;
        ps1.field.push(gin.clone());
        ps2.field.push(test_inst("p2_sleep", "B01011", "p2", models::Zone::FIELD, models::CardOrientation::SLEEP));
        ps2.evidence.push(test_inst("p2_ev1", "B01011", "p2", models::Zone::EVIDENCE, models::CardOrientation::ACTIVE));
        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        assert_eq!(engine.effective_rush(&gin), (true, Some("CASE".to_string())));
        assert!(!engine.effective_swift(&gin));

        engine.prompt_main_action("p1");
        let pd = engine.pending_decision.as_ref().unwrap().clone();
        let actions = pd["options"]["actions"].as_array().unwrap();
        let deduce_actors: Vec<&str> = actions
            .iter()
            .find(|a| a["type"] == "DEDUCE")
            .and_then(|a| a["actorIds"].as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        assert!(!deduce_actors.contains(&"p1_gin"), "突撃[事件] must not allow DEDUCE on entered turn");

        let action_entry = actions.iter().find(|a| a["type"] == "ACTION").unwrap();
        let action_actors: Vec<&str> = action_entry["actorIds"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert!(action_actors.contains(&"p1_gin"));
        let gin_targets: Vec<&str> = action_entry["actorTargetMap"]["p1_gin"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(gin_targets, vec!["p2_c"], "突撃[事件] can only target CASE on entered turn");

        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd["decisionId"],
            "revision": engine.revision,
            "value": {"type": "DEDUCE", "actorId": "p1_gin"}
        }));
        assert_eq!(
            engine.players.get("p1").unwrap().field[0].orientation,
            models::CardOrientation::ACTIVE,
            "Rejected DEDUCE for 突撃[事件] must keep character ACTIVE"
        );
        assert!(
            engine.players.get("p1").unwrap().evidence.is_empty(),
            "Rejected DEDUCE for 突撃[事件] must not add evidence"
        );
    }

    // 2) Bug 4: Conditional passive Rush variants (D07012, B07096, B09008, B08007)
    {
        let mut engine = MatchEngine::new(catalog.clone());
        engine.turn = 3;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];
        engine.phase = models::MatchPhase::MAIN;

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let mut ps2 = engine::PlayerState::new("p2".to_string());
        ps1.partner = Some(test_inst("p1_p_blue", "B01001", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE)); // 青
        ps1.r#case = Some(test_inst("p1_c", "B01024", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
        ps2.r#case = Some(test_inst("p2_c", "B01024", "p2", models::Zone::CASE, models::CardOrientation::ACTIVE));

        let mut vodka_d07012 = test_inst("p1_vodka", "D07012", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE);
        vodka_d07012.entered_turn = 3;
        let mut vodka_lv4 = test_inst("p1_vodka_lv4", "B07096", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE);
        vodka_lv4.entered_turn = 3;
        let mut akagi = test_inst("p1_akagi", "B09008", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE);
        akagi.entered_turn = 3;
        let mut mitsu = test_inst("p1_mitsu", "B08007", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE);
        mitsu.entered_turn = 3;

        ps1.field.push(vodka_d07012);
        ps1.field.push(vodka_lv4);
        ps1.field.push(akagi);
        ps1.field.push(mitsu);
        ps2.field.push(test_inst("p2_lv4", "B01011", "p2", models::Zone::FIELD, models::CardOrientation::SLEEP));
        ps2.field.push(test_inst("p2_lv8", "B01005", "p2", models::Zone::FIELD, models::CardOrientation::SLEEP));
        ps2.evidence.push(test_inst("p2_ev1", "B01011", "p2", models::Zone::EVIDENCE, models::CardOrientation::ACTIVE));
        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        // With Blue partner: D07012 has NO Rush; B08007 has 突撃[事件]
        {
            let ps1_ref = engine.players.get("p1").unwrap();
            assert_eq!(engine.effective_rush(&ps1_ref.field[0]), (false, None));
            assert_eq!(engine.effective_rush(&ps1_ref.field[2]), (false, None)); // B09008 AP5000 < 6000
            assert_eq!(engine.effective_rush(&ps1_ref.field[3]), (true, Some("CASE".to_string())));
        }

        // Switch to Black partner: D07012 gets full Rush, B07096 gets CHAR_LV4 Rush
        engine.players.get_mut("p1").unwrap().partner =
            Some(test_inst("p1_p_black", "B03106", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
        // Give B09008 +1000 AP (total 6000) and B08007 1 under_card (+ 突撃[キャラ])
        engine.players.get_mut("p1").unwrap().field[2].temp_ap_bonus = 1000;
        engine.players.get_mut("p1").unwrap().field[3].set_cards.push(
            test_inst("u1", "B01011", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE),
        );
        // Put back Blue partner? Check D07012 & B07096 with Black partner first
        {
            let ps1_ref = engine.players.get("p1").unwrap();
            assert_eq!(engine.effective_rush(&ps1_ref.field[0]), (true, None), "D07012 must gain 突撃 with Black partner");
            assert_eq!(engine.effective_rush(&ps1_ref.field[1]), (true, Some("CHAR_LV4".to_string())));
            assert_eq!(engine.effective_rush(&ps1_ref.field[2]), (true, None), "B09008 at AP6000 must gain 突撃");
        }
        engine.prompt_main_action("p1");
        let pd = engine.pending_decision.as_ref().unwrap();
        let actions = pd["options"]["actions"].as_array().unwrap();
        let action_entry = actions.iter().find(|a| a["type"] == "ACTION").unwrap();
        let vodka_targets: Vec<&str> = action_entry["actorTargetMap"]["p1_vodka"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert!(vodka_targets.contains(&"p2_c") && vodka_targets.contains(&"p2_lv4") && vodka_targets.contains(&"p2_lv8"));

        let lv4_targets: Vec<&str> = action_entry["actorTargetMap"]["p1_vodka_lv4"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert!(lv4_targets.contains(&"p2_lv4"));
        assert!(!lv4_targets.contains(&"p2_lv8"));
        assert!(!lv4_targets.contains(&"p2_c"));

        // With Blue partner + 1 under_card, B08007 merges 突撃[事件] + 突撃[キャラ] => (true, None)
        engine.players.get_mut("p1").unwrap().partner =
            Some(test_inst("p1_p_blue", "B01001", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
        let ps1_ref = engine.players.get("p1").unwrap();
        assert_eq!(engine.effective_rush(&ps1_ref.field[3]), (true, None));
    }

    // 3) Bug 1 / 4 cross-card: B10015 (迅速 with 4 distinct 毛利探偵事務所 incl. 妃英理) CAN deduce on entered turn
    {
        let mut engine = MatchEngine::new(catalog);
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];
        engine.phase = models::MatchPhase::MAIN;

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let ps2 = engine::PlayerState::new("p2".to_string());
        ps1.partner = Some(test_inst("p1_p", "B01001", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
        ps1.r#case = Some(test_inst("p1_c", "B01024", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
        let mut b10015 = test_inst("p1_mori", "B10015", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE);
        b10015.entered_turn = 4;
        ps1.field.push(b10015);
        ps1.field.push(test_inst("p1_conan", "B01005", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.field.push(test_inst("p1_ran", "B01007", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        // Only 3 distinct 毛利探偵事務所 -> no Swift yet
        {
            let m = &engine.players.get("p1").unwrap().field[0];
            assert!(!engine.effective_swift(m));
        }
        // Adding 妃英理 (B01013) -> granted 毛利探偵事務所 by B10015 -> 4 distinct names -> Swift!
        engine.players.get_mut("p1").unwrap().field.push(
            test_inst("p1_eri", "B01013", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE),
        );
        {
            let m = &engine.players.get("p1").unwrap().field[0];
            assert!(engine.effective_swift(m), "B10015 must grant Swift once 4 distinct 毛利探偵事務所 are present");
        }
        engine.prompt_main_action("p1");
        let pd = engine.pending_decision.as_ref().unwrap();
        let deduce_actors: Vec<&str> = pd["options"]["actions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["type"] == "DEDUCE")
            .and_then(|a| a["actorIds"].as_array())
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert!(deduce_actors.contains(&"p1_mori"), "Character with 迅速 must be able to DEDUCE on entered turn");
    }
}

#[test]
fn test_bug2_cutin_self_turn_ap_buff_and_cross_cards() {
    let catalog = load_test_catalog();

    // 1) Standard cut-in: 【カットイン】【自分ターン中】AP＋3000 (B02007) + B10085 aura (+2000)
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
        // Attacker AP6000 vs Defender AP7000
        ps1.field.push(test_inst("p1_atk", "B01008", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("h_cut3000", "B02007", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps2.field.push(test_inst("p2_def", "B01005", "p2", models::Zone::FIELD, models::CardOrientation::SLEEP));
        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        engine.prompt_main_action("p1");
        let pd1 = engine.pending_decision.clone().unwrap();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd1["decisionId"],
            "revision": engine.revision,
            "value": {"type": "ACTION", "actorId": "p1_atk", "targetId": "p2_def"}
        }));

        let pd_cut = engine.pending_decision.clone().expect("p1 contactCutin should open");
        assert_eq!(pd_cut["kind"], "guard");
        assert_eq!(pd_cut["playerId"], "p1");
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_cut["decisionId"],
            "revision": engine.revision,
            "value": {"cutinCardId": "h_cut3000"}
        }));

        let p1_atk = engine.players.get("p1").unwrap().field.iter().find(|c| c.instance_id == "p1_atk").unwrap();
        assert_eq!(p1_atk.temp_ap_bonus, 0, "コンタクト終了後にカットインのAPバフは切れていること");
        assert!(engine.contact_ap_buffs.is_empty());
        assert!(engine.players.get("p2").unwrap().remove.iter().any(|c| c.instance_id == "p2_def"), "6000+3000 > 7000 must win combat");
    }

    // 2) B10085 (in field/partner) adds +2000 on own-turn cutin on top of B02007 (+3000) => +5000 total
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
        ps1.field.push(test_inst("p1_atk", "B01008", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.field.push(test_inst("p1_chianti", "B10085", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("h_cut3000", "B02007", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps2.field.push(test_inst("p2_def", "B01005", "p2", models::Zone::FIELD, models::CardOrientation::SLEEP));
        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        engine.prompt_main_action("p1");
        let pd1 = engine.pending_decision.clone().unwrap();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd1["decisionId"],
            "revision": engine.revision,
            "value": {"type": "ACTION", "actorId": "p1_atk", "targetId": "p2_def"}
        }));
        let pd_cut = engine.pending_decision.clone().unwrap();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_cut["decisionId"],
            "revision": engine.revision,
            "value": {"cutinCardId": "h_cut3000"}
        }));
        let p1_atk = engine.players.get("p1").unwrap().field.iter().find(|c| c.instance_id == "p1_atk").unwrap();
        assert_eq!(p1_atk.temp_ap_bonus, 0);
    }

    // 3) B11015: 工藤新一 => +3000, non-工藤新一 => +1000
    for (is_shinichi, expected_ap) in [(false, 1000), (true, 3000)] {
        let mut engine = MatchEngine::new(catalog.clone());
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];
        engine.phase = models::MatchPhase::MAIN;

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let mut ps2 = engine::PlayerState::new("p2".to_string());
        ps1.partner = Some(test_inst("p1_p", "B01001", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
        ps1.r#case = Some(test_inst("p1_c", "B01024", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
        let atk_def = if is_shinichi { "B01009" } else { "B01008" };
        ps1.field.push(test_inst("p1_atk", atk_def, "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("h_b11015", "B11015", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps2.field.push(test_inst("p2_def", "B01005", "p2", models::Zone::FIELD, models::CardOrientation::SLEEP));
        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        engine.prompt_main_action("p1");
        let pd1 = engine.pending_decision.clone().unwrap();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd1["decisionId"],
            "revision": engine.revision,
            "value": {"type": "ACTION", "actorId": "p1_atk", "targetId": "p2_def"}
        }));
        let pd_cut = engine.pending_decision.clone().unwrap();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_cut["decisionId"],
            "revision": engine.revision,
            "value": {"cutinCardId": "h_b11015"}
        }));
        let p1_atk = engine.players.get("p1").unwrap().field.iter().find(|c| c.instance_id == "p1_atk").unwrap();
        assert_eq!(p1_atk.temp_ap_bonus, 0, "expected contact AP {} must have expired", expected_ap);
    }

    // 4) B05040: optional discard hand scales AP by removed level (lv8 => +8000, skip => 0)
    for do_discard in [true, false] {
        let mut engine = MatchEngine::new(catalog.clone());
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];
        engine.phase = models::MatchPhase::MAIN;

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let mut ps2 = engine::PlayerState::new("p2".to_string());
        ps1.partner = Some(test_inst("p1_p", "B01001", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
        ps1.r#case = Some(test_inst("p1_c", "B01024", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
        ps1.field.push(test_inst("p1_atk", "B01008", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("h_b05040", "B05040", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("h_lv8", "B01005", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps2.field.push(test_inst("p2_def", "B01005", "p2", models::Zone::FIELD, models::CardOrientation::SLEEP));
        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        engine.prompt_main_action("p1");
        let pd1 = engine.pending_decision.clone().unwrap();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd1["decisionId"],
            "revision": engine.revision,
            "value": {"type": "ACTION", "actorId": "p1_atk", "targetId": "p2_def"}
        }));
        let pd_cut = engine.pending_decision.clone().unwrap();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_cut["decisionId"],
            "revision": engine.revision,
            "value": {"cutinCardId": "h_b05040"}
        }));
        let pd_disc = engine.pending_decision.clone().expect("B05040 must prompt optional DISCARD_HAND");
        assert_eq!(pd_disc["kind"], "targetSelect");
        assert_eq!(pd_disc["options"]["effectType"], "DISCARD_HAND");
        let answer_val = if do_discard {
            serde_json::json!({"targetId": "h_lv8", "actorId": "h_b05040", "effectType": "DISCARD_HAND"})
        } else {
            serde_json::json!({"skip": true})
        };
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_disc["decisionId"],
            "revision": engine.revision,
            "value": answer_val
        }));
        let p1_atk = engine.players.get("p1").unwrap().field.iter().find(|c| c.instance_id == "p1_atk").unwrap();
        assert_eq!(p1_atk.temp_ap_bonus, 0, "コンタクト終了後にAPバフは切れていること (discard={})", do_discard);
    }
}

#[test]
fn test_id0371_sherry_single_discard_and_cross_card_deck_search_conditionals() {
    let catalog = load_test_catalog();

    // 1) ID0371 シェリー (B03122 / D07019): 複数手札があっても手札に加えた後のリムーブは1回（1枚）のみ
    for (tag, def_id) in [("B03122", "B03122"), ("D07019", "D07019")] {
        let mut engine = MatchEngine::new(catalog.clone());
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];
        engine.phase = models::MatchPhase::MAIN;

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let ps2 = engine::PlayerState::new("p2".to_string());
        ps1.partner = Some(test_inst("p1_p", "B03106", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
        ps1.r#case = Some(test_inst("p1_c", "B01024", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
        for i in 0..8 {
            ps1.file.push(test_inst(&format!("f{}", i), "B01021", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
        }
        ps1.hand.push(test_inst("h_sherry", def_id, "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("h_ex1", "B01021", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("h_ex2", "B01020", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        for i in 0..3 {
            ps1.deck.push(test_inst(&format!("dk_other_{}", i), "B01021", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        }
        ps1.deck.push(test_inst("dk_black", "B03122", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        engine.prompt_main_action("p1");
        let pd_main = engine.pending_decision.clone().unwrap();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_main["decisionId"],
            "revision": engine.revision,
            "value": {"type": "HAND_PLAY", "cardId": "h_sherry"}
        }));

        let pd_ds = engine.pending_decision.clone().expect("deckSearch must open");
        assert_eq!(pd_ds["kind"], "deckSearch");
        assert_eq!(pd_ds["options"]["postAddDiscard"], 1);
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_ds["decisionId"],
            "revision": engine.revision,
            "value": {"chosenCardId": "dk_black", "actorId": "h_sherry"}
        }));

        let pd_disc = engine.pending_decision.clone().expect("single DISCARD_HAND prompt expected");
        assert_eq!(pd_disc["kind"], "targetSelect");
        assert_eq!(pd_disc["options"]["effectType"], "DISCARD_HAND");
        assert_eq!(pd_disc["options"]["maxSelect"], 1);
        assert_eq!(pd_disc["options"]["optional"], false);

        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_disc["decisionId"],
            "revision": engine.revision,
            "value": {"targetId": "h_ex1", "actorId": "h_sherry", "effectType": "DISCARD_HAND"}
        }));

        assert_eq!(
            engine.pending_decision.as_ref().unwrap()["kind"],
            "mainAction",
            "{} must NOT force a second hand discard",
            tag
        );
        let p1_after = engine.players.get("p1").unwrap();
        assert_eq!(p1_after.hand.len(), 2, "{} hand should be h_ex2 + dk_black", tag);
        assert_eq!(p1_after.remove.len(), 1, "{} remove should have only h_ex1", tag);
        assert!(p1_after.remove.iter().any(|c| c.instance_id == "h_ex1"));

        // Skip picking in deckSearch -> 0 discards
        {
            let ps1m = engine.players.get_mut("p1").unwrap();
            ps1m.normal_hand_played_this_turn = false;
            ps1m.hand.push(test_inst("h_sherry2", def_id, "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
            for i in 0..4 {
                ps1m.deck.push(test_inst(&format!("dk_skip_{}", i), "B03122", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
            }
        }
        let pd_main2 = engine.pending_decision.clone().unwrap();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_main2["decisionId"],
            "revision": engine.revision,
            "value": {"type": "HAND_PLAY", "cardId": "h_sherry2"}
        }));
        let pd_ds2 = engine.pending_decision.clone().unwrap();
        assert_eq!(pd_ds2["kind"], "deckSearch");
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_ds2["decisionId"],
            "revision": engine.revision,
            "value": {"chosenCardIds": [], "actorId": "h_sherry2"}
        }));
        assert_eq!(engine.pending_decision.as_ref().unwrap()["kind"], "mainAction");
        assert_eq!(engine.players.get("p1").unwrap().remove.len(), 1);
    }

    // 2) Cross-card B10054 (レベル6以下のカードを手札に加えた場合、手札を1枚リムーブする)
    for (pick_lv6, expect_discard) in [(true, true), (false, false)] {
        let mut engine = MatchEngine::new(catalog.clone());
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];
        engine.phase = models::MatchPhase::MAIN;

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let ps2 = engine::PlayerState::new("p2".to_string());
        ps1.partner = Some(test_inst("p1_p", "B01060", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
        ps1.r#case = Some(test_inst("p1_c", "B03138", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE)); // 赤女の悲劇 (赤/黄)
        for i in 0..8 {
            ps1.file.push(test_inst(&format!("f{}", i), "B01021", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
        }
        ps1.hand.push(test_inst("h_niide", "B10054", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("h_ex1", "B01021", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("h_ex2", "B01020", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps1.deck.push(test_inst("dk1", "B01021", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        ps1.deck.push(test_inst("dk2", "B01021", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        ps1.deck.push(test_inst("dk_lv6", "B04051", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE)); // Lv6 赤
        ps1.deck.push(test_inst("dk_lv8", "B01062", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE)); // Lv8 赤
        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        engine.prompt_main_action("p1");
        let pd_main = engine.pending_decision.clone().unwrap();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_main["decisionId"],
            "revision": engine.revision,
            "value": {"type": "HAND_PLAY", "cardId": "h_niide"}
        }));
        let pd_ds = engine.pending_decision.clone().unwrap();
        assert_eq!(pd_ds["kind"], "deckSearch");
        let chosen_id = if pick_lv6 { "dk_lv6" } else { "dk_lv8" };
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_ds["decisionId"],
            "revision": engine.revision,
            "value": {"chosenCardId": chosen_id, "actorId": "h_niide"}
        }));
        if expect_discard {
            let pd_disc = engine.pending_decision.clone().unwrap();
            assert_eq!(pd_disc["kind"], "targetSelect");
            assert_eq!(pd_disc["options"]["effectType"], "DISCARD_HAND");
            assert_eq!(pd_disc["options"]["maxSelect"], 1);
            let _ = engine.submit_decision("p1", &serde_json::json!({
                "decisionId": pd_disc["decisionId"],
                "revision": engine.revision,
                "value": {"targetId": "h_ex1", "actorId": "h_niide", "effectType": "DISCARD_HAND"}
            }));
            assert_eq!(engine.pending_decision.as_ref().unwrap()["kind"], "mainAction");
            assert_eq!(engine.players.get("p1").unwrap().remove.len(), 1);
        } else {
            assert_eq!(engine.pending_decision.as_ref().unwrap()["kind"], "mainAction", "Lv8 pick must NOT trigger discard");
            assert_eq!(engine.players.get("p1").unwrap().remove.len(), 0);
        }
    }

    // 3) Cross-card B08050 (諸星大/宮野志保/宮野エレーナ/宮野厚司 以外を手札に加えた場合、手札を1枚リムーブ)
    for (pick_excluded, expect_discard) in [(true, false), (false, true)] {
        let mut engine = MatchEngine::new(catalog.clone());
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];
        engine.phase = models::MatchPhase::MAIN;

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let ps2 = engine::PlayerState::new("p2".to_string());
        ps1.partner = Some(test_inst("p1_p", "B01060", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
        ps1.r#case = Some(test_inst("p1_c", "B01024", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
        for i in 0..8 {
            ps1.file.push(test_inst(&format!("f{}", i), "B01021", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
        }
        ps1.hand.push(test_inst("h_akemi", "B08050", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("h_ex1", "B01021", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("h_ex2", "B01020", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps1.deck.push(test_inst("dk1", "B01021", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        ps1.deck.push(test_inst("dk_shiho", "B01018", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        ps1.deck.push(test_inst("dk_sherry", "B03122", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        engine.prompt_main_action("p1");
        let pd_main = engine.pending_decision.clone().unwrap();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_main["decisionId"],
            "revision": engine.revision,
            "value": {"type": "HAND_PLAY", "cardId": "h_akemi"}
        }));
        let pd_ds = engine.pending_decision.clone().unwrap();
        assert_eq!(pd_ds["kind"], "deckSearch");
        let chosen_id = if pick_excluded { "dk_shiho" } else { "dk_sherry" };
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_ds["decisionId"],
            "revision": engine.revision,
            "value": {"chosenCardId": chosen_id, "actorId": "h_akemi"}
        }));
        if expect_discard {
            let pd_disc = engine.pending_decision.clone().unwrap();
            assert_eq!(pd_disc["kind"], "targetSelect");
            assert_eq!(pd_disc["options"]["effectType"], "DISCARD_HAND");
            let _ = engine.submit_decision("p1", &serde_json::json!({
                "decisionId": pd_disc["decisionId"],
                "revision": engine.revision,
                "value": {"targetId": "h_ex1", "actorId": "h_akemi", "effectType": "DISCARD_HAND"}
            }));
            assert_eq!(engine.pending_decision.as_ref().unwrap()["kind"], "mainAction");
            assert!(engine.players.get("p1").unwrap().remove.iter().any(|c| c.instance_id == "h_ex1"));
        } else {
            assert_eq!(engine.pending_decision.as_ref().unwrap()["kind"], "mainAction", "宮野志保 must NOT trigger discard");
            assert!(!engine.players.get("p1").unwrap().remove.iter().any(|c| c.zone == models::Zone::REMOVE && c.instance_id == "h_ex1"));
        }
    }

    // 4) Cross-card B07004 (キャラを登場させた場合、手札を1枚リムーブする)
    for (deploy_genta, expect_discard) in [(true, true), (false, false)] {
        let mut engine = MatchEngine::new(catalog.clone());
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];
        engine.phase = models::MatchPhase::MAIN;

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let ps2 = engine::PlayerState::new("p2".to_string());
        ps1.partner = Some(test_inst("p1_p", "B01001", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
        ps1.r#case = Some(test_inst("p1_c", "B01024", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
        ps1.field.push(test_inst("f_ayumi", "B07004", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("h_ex1", "B01021", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("h_ex2", "B01020", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        for i in 0..3 {
            ps1.deck.push(test_inst(&format!("dk{}", i), "B01021", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        }
        ps1.deck.push(test_inst("dk_genta", "B01014", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        engine.prompt_main_action("p1");
        let pd_main = engine.pending_decision.clone().unwrap();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_main["decisionId"],
            "revision": engine.revision,
            "value": {"type": "DECLARE", "actorId": "f_ayumi"}
        }));
        let pd_ds = engine.pending_decision.clone().unwrap();
        assert_eq!(pd_ds["kind"], "deckSearch");
        let val = if deploy_genta {
            serde_json::json!({"chosenCardId": "dk_genta", "actorId": "f_ayumi"})
        } else {
            serde_json::json!({"chosenCardIds": [], "actorId": "f_ayumi"})
        };
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_ds["decisionId"],
            "revision": engine.revision,
            "value": val
        }));
        if expect_discard {
            let pd_disc = engine.pending_decision.clone().unwrap();
            assert_eq!(pd_disc["kind"], "targetSelect");
            assert_eq!(pd_disc["options"]["effectType"], "DISCARD_HAND");
            let _ = engine.submit_decision("p1", &serde_json::json!({
                "decisionId": pd_disc["decisionId"],
                "revision": engine.revision,
                "value": {"targetId": "h_ex1", "actorId": "f_ayumi", "effectType": "DISCARD_HAND"}
            }));
            assert_eq!(engine.pending_decision.as_ref().unwrap()["kind"], "mainAction");
            assert!(engine.players.get("p1").unwrap().field.iter().any(|c| c.instance_id == "dk_genta"));
            assert_eq!(engine.players.get("p1").unwrap().remove.len(), 1);
        } else {
            assert_eq!(engine.pending_decision.as_ref().unwrap()["kind"], "mainAction");
            assert_eq!(engine.players.get("p1").unwrap().remove.len(), 0);
        }
    }

    // 5) Cross-card PR265 (カードを手札に加えた場合、自分のデッキのカードを上からそのカードのレベルと同じ枚数リムーブする)
    {
        let mut engine = MatchEngine::new(catalog);
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];
        engine.phase = models::MatchPhase::MAIN;

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let ps2 = engine::PlayerState::new("p2".to_string());
        ps1.partner = Some(test_inst("p1_p", "B01060", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
        ps1.r#case = Some(test_inst("p1_c", "B01024", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
        for i in 0..8 {
            ps1.file.push(test_inst(&format!("f{}", i), "B01021", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
        }
        ps1.hand.push(test_inst("h_kazami", "PR265", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        for i in 0..6 {
            ps1.deck.push(test_inst(&format!("dk_rem_{}", i), "B01021", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        }
        ps1.deck.push(test_inst("dk_sato", "B01089", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE)); // Lv5 警視庁
        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        engine.prompt_main_action("p1");
        let pd_main = engine.pending_decision.clone().unwrap();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_main["decisionId"],
            "revision": engine.revision,
            "value": {"type": "HAND_PLAY", "cardId": "h_kazami"}
        }));
        let pd_ds = engine.pending_decision.clone().unwrap();
        assert_eq!(pd_ds["kind"], "deckSearch");
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_ds["decisionId"],
            "revision": engine.revision,
            "value": {"chosenCardId": "dk_sato", "actorId": "h_kazami"}
        }));

        assert_eq!(engine.pending_decision.as_ref().unwrap()["kind"], "mainAction");
        let p1_after = engine.players.get("p1").unwrap();
        assert!(p1_after.hand.iter().any(|c| c.instance_id == "dk_sato"));
        assert_eq!(p1_after.remove.len(), 5, "PR265 must mill exact level (5) cards from deck");
    }
}

#[test]
fn test_self_ability_remove_revive_sleep_vodka_and_cross_cards() {
    let catalog = catalog::CardCatalog::load_from_json("../data/dummy/cards.json").unwrap();
    let vodka_id = catalog.definitions.keys().find(|k| k.starts_with("B11085")).cloned().expect("B11085 must exist in cards.json");
    let black_partner_id = catalog.definitions.values()
        .find(|d| d.card_type == models::CardType::PARTNER && d.colors.iter().any(|c| c.contains("黒")))
        .map(|d| d.definition_id.clone())
        .expect("black partner must exist");

    let mut engine = engine::MatchEngine::new(catalog);
    engine.turn = 1;
    engine.turn_player_id = "p1".to_string();
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];

    let mut ps1 = engine::PlayerState::new("p1".to_string());
    ps1.partner = Some(models::CardInstance {
        instance_id: "p1_partner".to_string(),
        definition_id: black_partner_id,
        owner_id: "p1".to_string(),
        zone: models::Zone::PARTNER,
        orientation: models::CardOrientation::ACTIVE,
        ..Default::default()
    });
    ps1.field.push(models::CardInstance {
        instance_id: "vodka_1".to_string(),
        definition_id: vodka_id.clone(),
        owner_id: "p1".to_string(),
        zone: models::Zone::FIELD,
        orientation: models::CardOrientation::ACTIVE,
        ..Default::default()
    });
    ps1.field.push(models::CardInstance {
        instance_id: "vodka_2".to_string(),
        definition_id: vodka_id.clone(),
        owner_id: "p1".to_string(),
        zone: models::Zone::FIELD,
        orientation: models::CardOrientation::SLEEP,
        ..Default::default()
    });
    engine.players.insert("p1".to_string(), ps1);
    engine.players.insert("p2".to_string(), engine::PlayerState::new("p2".to_string()));

    // 1) アクションしていない vodka_1 が自分の効果でリムーブされても復活しない
    {
        let ps = engine.players.get_mut("p1").unwrap();
        let pos = ps.field.iter().position(|c| c.instance_id == "vodka_1").unwrap();
        let mut c = ps.field.remove(pos);
        c.zone = models::Zone::REMOVE;
        ps.remove.push(c);
    }
    engine.trigger_on_remove_char("p1", "vodka_1", &vodka_id, true);
    assert!(engine.players.get("p1").unwrap().remove.iter().any(|c| c.instance_id == "vodka_1"));
    assert!(!engine.players.get("p1").unwrap().field.iter().any(|c| c.instance_id == "vodka_1"));

    // 2) アクションしていた vodka_2 が自分の効果でリムーブされたらスリープ状態で現場に登場する
    engine.acted_this_turn_ids.insert("vodka_2".to_string());
    {
        let ps = engine.players.get_mut("p1").unwrap();
        let pos = ps.field.iter().position(|c| c.instance_id == "vodka_2").unwrap();
        let mut c = ps.field.remove(pos);
        c.zone = models::Zone::REMOVE;
        ps.remove.push(c);
    }
    engine.trigger_on_remove_char("p1", "vodka_2", &vodka_id, true);
    let v2_field = engine.players.get("p1").unwrap().field.iter().find(|c| c.instance_id == "vodka_2");
    assert!(v2_field.is_some(), "vodka_2 must revive to field when acted this turn");
    assert_eq!(v2_field.unwrap().orientation, models::CardOrientation::SLEEP);

    // 3) 相手の効果でリムーブされた場合は復活しない
    {
        let ps = engine.players.get_mut("p1").unwrap();
        let pos = ps.field.iter().position(|c| c.instance_id == "vodka_2").unwrap();
        let mut c = ps.field.remove(pos);
        c.zone = models::Zone::REMOVE;
        ps.remove.push(c);
    }
    engine.trigger_on_remove_char("p1", "vodka_2", &vodka_id, false);
    assert!(!engine.players.get("p1").unwrap().field.iter().any(|c| c.instance_id == "vodka_2"));
}

#[test]
fn test_parse_official_deck_html() {
    let html = r#"<html><body><form><input type="hidden" name="deck" value="{&quot;partner&quot;:[&quot;B01001&quot;],&quot;incident&quot;:[&quot;B01002&quot;],&quot;other&quot;:[&quot;B01003&quot;,&quot;B01003&quot;,&quot;B01004&quot;]}"></form></body></html>"#;
    let deck = parse_official_deck_html(html).expect("Must parse valid official deck html");
    assert_eq!(deck["partner"][0], "B01001");
    assert_eq!(deck["incident"][0], "B01002");
    assert_eq!(deck["other"].as_array().unwrap().len(), 3);
}

#[test]
fn test_mr_partner_area_movement_and_duplicate_removal() {
    let catalog = catalog::CardCatalog::load_from_json("../data/dummy/cards.json").unwrap();
    let mut engine = engine::MatchEngine::new(catalog.clone());
    engine.turn = 3;
    engine.turn_player_id = "p2".to_string(); // 相手(p2)のターン中
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];
    engine.phase = models::MatchPhase::MAIN;

    let mut ps1 = engine::PlayerState::new("p1".to_string());
    ps1.field.push(models::CardInstance {
        instance_id: "mr_conan_shinichi".to_string(),
        definition_id: "B05005".to_string(), // Lv9 MR 江戸川コナン＆工藤新一
        owner_id: "p1".to_string(),
        zone: models::Zone::FIELD,
        ..Default::default()
    });
    engine.players.insert("p1".to_string(), ps1);
    engine.players.insert("p2".to_string(), engine::PlayerState::new("p2".to_string()));

    // 1) 相手ターン中に MR が現場を離れた場合、パートナーエリア (extra_partners) に移動すること
    let res = engine.move_field_char_to_remove("p1", "mr_conan_shinichi", false, true);
    assert!(res.is_some());
    let ps1_after_opp = &engine.players["p1"];
    assert!(ps1_after_opp.field.is_empty());
    assert!(ps1_after_opp.remove.is_empty(), "リムーブエリアには残らないこと");
    assert_eq!(ps1_after_opp.extra_partners.len(), 1, "パートナーエリア(extra_partners)に移動していること");
    assert_eq!(ps1_after_opp.extra_partners[0].instance_id, "mr_conan_shinichi");
    assert_eq!(ps1_after_opp.extra_partners[0].zone, models::Zone::PARTNER);

    // 2) 自分のターンに新たな MR が現場に登場した場合、パートナーエリアにいた既存の MR はリムーブされること
    engine.turn_player_id = "p1".to_string();
    engine.players.get_mut("p1").unwrap().field.push(models::CardInstance {
        instance_id: "mr_new".to_string(),
        definition_id: "B06003".to_string(), // Lv9 MR 毛利蘭＆江戸川コナン
        owner_id: "p1".to_string(),
        zone: models::Zone::FIELD,
        ..Default::default()
    });
    engine.resolve_character_enter_field_with_source(
        "p1",
        "mr_new",
        "B06003",
        models::CardOrientation::ACTIVE,
        None,
        "MAIN",
    );

    let ps1_after_new_mr = &engine.players["p1"];
    assert!(ps1_after_new_mr.extra_partners.is_empty(), "既存MRがパートナーエリアからリムーブされていること");
    assert!(
        ps1_after_new_mr.remove.iter().any(|c| c.instance_id == "mr_conan_shinichi"),
        "既存MRがリムーブエリアに置かれていること"
    );
    assert!(
        ps1_after_new_mr.field.iter().any(|c| c.instance_id == "mr_new"),
        "新たなMRが現場に残っていること"
    );
}

#[test]
fn test_refresh_triggers_at_the_exact_moment_deck_hits_zero() {
    let catalog = load_test_catalog();

    // A) 山札が0枚になった瞬間にリフレッシュが発動し、removeが山札へ戻って続行する（docs/rules.md §リフレッシュ / docs/test-cases.md T024, T026）
    {
        let mut engine = MatchEngine::new(catalog.clone());
        engine.phase = models::MatchPhase::MAIN;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        // 山札は最終1枚のみ。これを gain_evidence で引いた瞬間に0枚になる。
        ps1.deck.push(test_inst("d_last", "B01011", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        // remove に1枚あるのでリフレッシュは成功し敗北しない。
        ps1.remove.push(test_inst("r1", "B01011", "p1", models::Zone::REMOVE, models::CardOrientation::ACTIVE));
        engine.players.insert("p1".to_string(), ps1);

        // 相手の cascade 敗北を防ぐため p2 には余裕の山札を持たせる。
        let mut ps2 = engine::PlayerState::new("p2".to_string());
        for i in 0..5 {
            ps2.deck.push(test_inst(&format!("p2_buf_{}", i), "B01011", "p2", models::Zone::DECK, models::CardOrientation::ACTIVE));
        }
        engine.players.insert("p2".to_string(), ps2);

        engine.gain_evidence("p1", 1);

        // 0枚になった瞬間に割り込んでリフレッシュ済み → remove の r1 がシャッフルされて山札へ戻る（deck>=1）
        let p1 = engine.players.get("p1").unwrap();
        assert!(p1.deck.iter().any(|c| c.instance_id == "r1"), "0枚になった瞬間のリフレッシュで remove(r1) が山札へ戻ること");
        assert!(!p1.remove.iter().any(|c| c.instance_id == "r1"), "リフレッシュで remove は空になること");
        assert!(engine.refreshed_players.contains("p1"), "p1 がリフレッシュ済みとして記録されること");
        assert_ne!(engine.phase, models::MatchPhase::FINISHED, "remove があれば敗北しない");
        // リフレッシュの副次効果で相手に証拠1枚
        assert_eq!(engine.players.get("p2").unwrap().evidence.len(), 1, "リフレッシュで相手の証拠が1枚増えること");
    }

    // B) remove が空のまま0枚になった瞬間は即敗北（docs/rules.md §リフレッシュ / docs/test-cases.md T025）
    {
        let mut engine = MatchEngine::new(catalog.clone());
        engine.phase = models::MatchPhase::MAIN;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        ps1.deck.push(test_inst("d_last", "B01011", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        engine.players.insert("p1".to_string(), ps1);

        let mut ps2 = engine::PlayerState::new("p2".to_string());
        for i in 0..5 {
            ps2.deck.push(test_inst(&format!("p2_buf2_{}", i), "B01011", "p2", models::Zone::DECK, models::CardOrientation::ACTIVE));
        }
        engine.players.insert("p2".to_string(), ps2);

        // 最後の1枚を引いて deck=0 & remove=0 → 0枚になった瞬間にリフレッシュ発動できず敗北
        engine.gain_evidence("p1", 1);

        assert_eq!(engine.phase, models::MatchPhase::FINISHED, "remove 空で0枚になった瞬間に即敗北すること");
        assert_eq!(engine.winner_id.as_deref(), Some("p2"), "リフレッシュ枯渇で相手(p2)の勝ちになること");
    }
}

#[test]
fn test_mulligan_and_refresh_cards_gained_as_evidence_stay_face_down_and_b11099_flips_exact_two() {
    let catalog = load_test_catalog();

    // 1) start_match -> マリガンで手札(face_up=true)を山札へ戻し、その後 gain_evidence してもすべて裏向きであること
    {
        let mut engine = MatchEngine::new(catalog.clone());
        let deck = models::DeckDefinition {
            deck_id: None,
            deck_name: None,
            catalog_version: None,
            partner_id: "B01001".to_string(),
            case_id: "B11099".to_string(),
            main_deck: vec![
                models::DeckCardItem {
                    definition_id: "B01005".to_string(),
                    count: 10,
                },
                models::DeckCardItem {
                    definition_id: "B11056".to_string(),
                    count: 10,
                },
            ],
        };
        engine.add_player("p1", &deck);
        engine.add_player("p2", &deck);
        engine.start_match();

        let first_pid = engine.player_order[0].clone();
        let all_hand_ids: Vec<String> = engine.players[&first_pid]
            .hand
            .iter()
            .map(|c| c.instance_id.clone())
            .collect();
        assert_eq!(all_hand_ids.len(), 5);
        let obs_mul = build_player_observation(&engine, &first_pid);
        engine.submit_decision(&first_pid, &serde_json::json!({
            "decisionId": obs_mul["pendingDecision"]["decisionId"],
            "revision": engine.revision,
            "value": { "cardIds": all_hand_ids }
        })).unwrap();
        assert!(
            engine.players[&first_pid].deck.iter().all(|c| !c.face_up),
            "マリガンで山札へ戻したカードの face_up は false にリセットされること"
        );

        let deck_len = engine.players[&first_pid].deck.len();
        engine.gain_evidence(&first_pid, deck_len);
        let evs = &engine.players[&first_pid].evidence;
        assert_eq!(evs.len(), deck_len);
        assert!(
            evs.iter().all(|e| !e.face_up),
            "マリガン後の山札から獲得した証拠はすべて裏向きであること"
        );
    }

    // 2) リフレッシュでリムーブ(face_up=true)から山札に戻したカードも、獲得された証拠は裏向きであること
    {
        let mut engine = MatchEngine::new(catalog.clone());
        engine.phase = models::MatchPhase::MAIN;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        for i in 0..3 {
            ps1.remove.push(test_inst(&format!("rm_{}", i), "B01005", "p1", models::Zone::REMOVE, models::CardOrientation::ACTIVE));
            // test_inst 側で face_up=true になっていることも確認しつつリフレッシュへ送る
            ps1.remove[i].face_up = true;
        }
        let mut ps2 = engine::PlayerState::new("p2".to_string());
        for i in 0..5 {
            ps2.deck.push(test_inst(&format!("p2d_{}", i), "B01005", "p2", models::Zone::DECK, models::CardOrientation::ACTIVE));
        }
        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        engine.gain_evidence("p1", 2);
        let p1 = &engine.players["p1"];
        assert_eq!(p1.evidence.len(), 2);
        assert!(
            p1.evidence.iter().all(|e| !e.face_up),
            "リフレッシュ直後に獲得した証拠はすべて裏向きであること"
        );
        assert!(p1.deck.iter().all(|d| !d.face_up));
    }

    // 3) B11099（あんた何者や）で裏向き証拠を2つ表向きにすると、正確に2枚だけ表向きになること
    {
        let mut engine = MatchEngine::new(catalog.clone());
        engine.phase = models::MatchPhase::MAIN;
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        ps1.case_phase = models::CasePhase::RESOLUTION;
        ps1.r#case = Some(test_inst("p1_case", "B11099", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
        for i in 0..4 {
            let mut ev = test_inst(&format!("ev_{}", i), "B01005", "p1", models::Zone::EVIDENCE, models::CardOrientation::ACTIVE);
            ev.face_up = false;
            ps1.evidence.push(ev);
        }
        let mut ps2 = engine::PlayerState::new("p2".to_string());
        ps2.field.push(test_inst("opp_c", "B05055", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);
        engine.prompt_main_action("p1");

        let obs1 = build_player_observation(&engine, "p1");
        engine.submit_decision("p1", &serde_json::json!({
            "decisionId": obs1["pendingDecision"]["decisionId"],
            "revision": engine.revision,
            "value": { "type": "DECLARE", "actorId": "p1_case" }
        })).unwrap();

        let obs2 = build_player_observation(&engine, "p1");
        assert_eq!(obs2["pendingDecision"]["options"]["effectType"].as_str(), Some("FLIP_EVIDENCE_FACE_UP"));
        engine.submit_decision("p1", &serde_json::json!({
            "decisionId": obs2["pendingDecision"]["decisionId"],
            "revision": engine.revision,
            "value": { "targetIds": ["ev_0", "ev_1"] }
        })).unwrap();

        let obs3 = build_player_observation(&engine, "p1");
        assert_eq!(obs3["pendingDecision"]["options"]["effectType"].as_str(), Some("AP_DEBUFF"));
        engine.submit_decision("p1", &serde_json::json!({
            "decisionId": obs3["pendingDecision"]["decisionId"],
            "revision": engine.revision,
            "value": { "targetId": "opp_c" }
        })).unwrap();

        let face_up_cnt = engine.players["p1"].evidence.iter().filter(|e| e.face_up).count();
        assert_eq!(face_up_cnt, 2, "B11099で2枚選んだら表向き証拠は正確に2枚であること");
    }

    // 4) 横展開: B11097（諸伏景光＆スコッチ）の「自分の裏向きの証拠を2つ表向きにしてもよい。そうした場合、カードを1枚引く。」で2枚だけ表向きになり1ドローされること
    {
        let mut engine = MatchEngine::new(catalog.clone());
        engine.phase = models::MatchPhase::MAIN;
        engine.turn = 4;
        engine.turn_player_id = "p2".to_string(); // 相手ターン中
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        ps1.field.push(test_inst("scotch", "B11097", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.field.push(test_inst("swift_ally", "B09084", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        for i in 0..4 {
            let mut ev = test_inst(&format!("s_ev_{}", i), "B01005", "p1", models::Zone::EVIDENCE, models::CardOrientation::ACTIVE);
            ev.face_up = false;
            ps1.evidence.push(ev);
        }
        for i in 0..4 {
            let mut dc = test_inst(&format!("s_d_{}", i), "B01005", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE);
            dc.face_up = false;
            ps1.deck.push(dc);
        }
        let mut ps2 = engine::PlayerState::new("p2".to_string());
        for i in 0..4 {
            ps2.deck.push(test_inst(&format!("p2_d_{}", i), "B01005", "p2", models::Zone::DECK, models::CardOrientation::ACTIVE));
        }
        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        engine.resolve_character_enter_field_with_source(
            "p1",
            "swift_ally",
            "B09084",
            models::CardOrientation::ACTIVE,
            None,
            "MAIN",
        );

        let obs_flip = build_player_observation(&engine, "p1");
        assert_eq!(
            obs_flip["pendingDecision"]["kind"].as_str(),
            Some("targetSelect"),
            "B11097の『自分の裏向きの証拠を2つ表向きにしてもよい』の選択が表示されること (logs: {:?})",
            engine.logs
        );
        assert_eq!(
            obs_flip["pendingDecision"]["options"]["effectType"].as_str(),
            Some("FLIP_EVIDENCE_FACE_UP")
        );
        assert_eq!(obs_flip["pendingDecision"]["options"]["maxSelect"].as_u64(), Some(2));
        // 1つ目の「カードを1枚引く。」で1枚引いているはず
        assert_eq!(engine.players["p1"].hand.len(), 1);

        engine.submit_decision("p1", &serde_json::json!({
            "decisionId": obs_flip["pendingDecision"]["decisionId"],
            "revision": engine.revision,
            "value": { "targetIds": ["s_ev_0", "s_ev_1"] }
        })).unwrap();

        let p1 = &engine.players["p1"];
        let face_up_cnt = p1.evidence.iter().filter(|e| e.face_up).count();
        assert_eq!(face_up_cnt, 2, "B11097で2つ選択した場合、二重表向きにならず正確に2枚のみ表向きになること");
        assert_eq!(p1.hand.len(), 2, "そうした場合の1ドローで合計2枚手札になること");
    }
}

#[test]
fn test_target_action_prompt_title_and_constraints_real_cards() {
    let catalog = load_test_catalog();

    // 1) B11057 のレベル-1 効果のタイトルに符号付きの「レベル－1」が入ること
    {
        let mut engine = MatchEngine::new(catalog.clone());
        engine.phase = models::MatchPhase::MAIN;
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        ps1.field.push(test_inst("c_debuff", "B11057", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        engine.players.insert("p1".to_string(), ps1);
        let mut ps2 = engine::PlayerState::new("p2".to_string());
        ps2.field.push(test_inst("opp_c", "B05055", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        engine.players.insert("p2".to_string(), ps2);

        let deployed = vec![("c_debuff".to_string(), "B11057".to_string(), models::CardOrientation::ACTIVE)];
        engine.resolve_deployed_characters("p1", None, &deployed, "MAIN");
        let obs = build_player_observation(&engine, "p1");
        assert_eq!(obs["pendingDecision"]["kind"].as_str(), Some("targetSelect"));
        let title = obs["pendingDecision"]["options"]["title"].as_str().unwrap_or("");
        assert!(
            title.contains("レベル－1"),
            "LevelDelta(-1) prompt title should show 'レベル－1', got: {}",
            title
        );
    }

    // 2) B03006 の同名不可制約がタイトル末尾にヒント表記されること
    {
        let mut engine = MatchEngine::new(catalog);
        engine.phase = models::MatchPhase::MAIN;
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        ps1.field.push(test_inst("c_kessei", "B03006", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.remove.push(test_inst("r_ayumi", "B11007", "p1", models::Zone::REMOVE, models::CardOrientation::ACTIVE));
        ps1.remove.push(test_inst("r_genta", "B11008", "p1", models::Zone::REMOVE, models::CardOrientation::ACTIVE));
        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), engine::PlayerState::new("p2".to_string()));

        let deployed = vec![("c_kessei".to_string(), "B03006".to_string(), models::CardOrientation::ACTIVE)];
        engine.resolve_deployed_characters("p1", None, &deployed, "MAIN");
        let obs = build_player_observation(&engine, "p1");
        assert_eq!(obs["pendingDecision"]["kind"].as_str(), Some("targetSelect"));
        let title = obs["pendingDecision"]["options"]["title"].as_str().unwrap_or("");
        assert!(
            title.contains("同名不可"),
            "distinct_names prompt title should include '同名不可', got: {}",
            title
        );
    }
}

#[test]
fn test_cpu_does_not_waste_assist_when_unnecessary() {
    let catalog = load_test_catalog();

    // 1) 後攻1ターン目 (Turn 2, FILE 2): 手札に Lv.2 と Lv.4 があるとき、無駄な ASSIST をせず Lv.2 登場 + パートナー推理(証拠+1)を行うこと
    {
        let mut engine = MatchEngine::new(catalog.clone());
        engine.phase = models::MatchPhase::MAIN;
        engine.turn = 2;
        engine.turn_player_id = "p2".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        ps1.r#case = Some(test_inst("p1_c", "B01024", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
        ps1.partner = Some(test_inst("p1_p", "B01001", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
        engine.players.insert("p1".to_string(), ps1);

        let mut ps2 = engine::PlayerState::new("p2".to_string());
        ps2.r#case = Some(test_inst("p2_c", "B01024", "p2", models::Zone::CASE, models::CardOrientation::ACTIVE));
        ps2.partner = Some(test_inst("p2_p", "B01001", "p2", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
        for i in 0..2 {
            ps2.file.push(test_inst(&format!("f_{}", i), "B01021", "p2", models::Zone::FILE, models::CardOrientation::ACTIVE));
        }
        for i in 0..10 {
            ps2.deck.push(test_inst(&format!("d_{}", i), "B01021", "p2", models::Zone::DECK, models::CardOrientation::ACTIVE));
        }
        ps2.hand.push(test_inst("h_lv2", "D08008", "p2", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps2.hand.push(test_inst("h_lv4", "B11008", "p2", models::Zone::HAND, models::CardOrientation::ACTIVE));
        engine.players.insert("p2".to_string(), ps2);

        let cpu = agent::RandomAgent::new("p2");
        engine.prompt_main_action("p2");

        let mut taken_types = Vec::new();
        for _ in 0..6 {
            let dec = engine.pending_decision.clone().expect("pending_decision");
            let ans = cpu.choose_action_with_engine(&engine, &dec);
            let ty = ans["value"]["type"].as_str().unwrap_or("").to_string();
            taken_types.push(ty.clone());
            if ty == "END_TURN" {
                break;
            }
            engine.submit_decision("p2", &serde_json::json!({
                "decisionId": dec["decisionId"],
                "revision": engine.revision,
                "value": ans["value"]
            })).unwrap();
        }

        assert!(
            !taken_types.iter().any(|t| t == "ASSIST"),
            "FILE 2 で Lv.2 を出せるのに無駄な ASSIST を行ってはならない (actions: {:?})",
            taken_types
        );
        assert!(taken_types.iter().any(|t| t == "HAND_PLAY"), "Lv.2 キャラを展開すること (actions: {:?})", taken_types);
        assert!(taken_types.iter().any(|t| t == "DEDUCE"), "パートナーで推理(証拠+1)すること (actions: {:?})", taken_types);
        let p2 = &engine.players["p2"];
        assert_eq!(p2.evidence.len(), 1);
        assert_eq!(p2.partner.as_ref().unwrap().zone, models::Zone::PARTNER);
        assert!(p2.field.iter().any(|c| c.instance_id == "h_lv2"));
    }

    // 2) 先攻1ターン目 (Turn 1, FILE 1): 手札に Lv.2 カードが無い (Lv.4 のみ) とき、無駄な ASSIST をせず END_TURN すること
    {
        let mut engine = MatchEngine::new(catalog.clone());
        engine.phase = models::MatchPhase::MAIN;
        engine.turn = 1;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        ps1.is_first_player = true;
        ps1.r#case = Some(test_inst("p1_c", "B01024", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
        ps1.partner = Some(test_inst("p1_p", "B01001", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
        ps1.file.push(test_inst("f_0", "B01021", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
        for i in 0..10 {
            ps1.deck.push(test_inst(&format!("d_{}", i), "B01021", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        }
        ps1.hand.push(test_inst("h_lv4", "B11008", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        engine.players.insert("p1".to_string(), ps1);
        let mut ps2 = engine::PlayerState::new("p2".to_string());
        ps2.r#case = Some(test_inst("p2_c", "B01024", "p2", models::Zone::CASE, models::CardOrientation::ACTIVE));
        engine.players.insert("p2".to_string(), ps2);

        let cpu = agent::RandomAgent::new("p1");
        engine.prompt_main_action("p1");

        let mut taken_types = Vec::new();
        for _ in 0..4 {
            let dec = engine.pending_decision.clone().expect("pending_decision");
            let ans = cpu.choose_action_with_engine(&engine, &dec);
            let ty = ans["value"]["type"].as_str().unwrap_or("").to_string();
            taken_types.push(ty.clone());
            if ty == "END_TURN" {
                break;
            }
            engine.submit_decision("p1", &serde_json::json!({
                "decisionId": dec["decisionId"],
                "revision": engine.revision,
                "value": ans["value"]
            })).unwrap();
        }
        assert!(
            !taken_types.iter().any(|t| t == "ASSIST"),
            "FILE 1 で Lv.2 が無いのに無駄な ASSIST を行ってはならない (actions: {:?})",
            taken_types
        );
        assert_eq!(taken_types.last().map(String::as_str), Some("END_TURN"));
        assert_eq!(engine.players["p1"].partner.as_ref().unwrap().zone, models::Zone::PARTNER);

        // ASSIST / END_TURN のみの選択肢（Step 9 フォールバック到達時）でも ASSIST せず END_TURN を選ぶこと
        let direct_dec = serde_json::json!({
            "decisionId": "d_no_assist",
            "revision": engine.revision,
            "playerId": "p1",
            "kind": "mainAction",
            "options": {
                "actions": [
                    { "type": "ASSIST" },
                    { "type": "END_TURN" }
                ]
            }
        });
        let direct_ans = cpu.choose_action_with_engine(&engine, &direct_dec);
        assert_eq!(direct_ans["value"]["type"].as_str(), Some("END_TURN"));
    }

    // 3) FILE 4 で手札に 【FILE5】持ちの Lv.4 キャラ (B11008 小嶋元太) があるときは、先に ASSIST して FILE 5 にしてから登場させること
    {
        let mut engine = MatchEngine::new(catalog);
        engine.phase = models::MatchPhase::MAIN;
        engine.turn = 5;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        ps1.is_first_player = true;
        ps1.r#case = Some(test_inst("p1_c", "B01024", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
        ps1.partner = Some(test_inst("p1_p", "B01001", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
        for i in 0..4 {
            ps1.file.push(test_inst(&format!("f_{}", i), "B01021", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
        }
        for i in 0..10 {
            ps1.deck.push(test_inst(&format!("d_{}", i), "B01021", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        }
        ps1.hand.push(test_inst("h_genta", "B11008", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("h_lv2", "D08008", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        engine.players.insert("p1".to_string(), ps1);
        let mut ps2 = engine::PlayerState::new("p2".to_string());
        ps2.r#case = Some(test_inst("p2_c", "B01024", "p2", models::Zone::CASE, models::CardOrientation::ACTIVE));
        engine.players.insert("p2".to_string(), ps2);

        let cpu = agent::RandomAgent::new("p1");
        engine.prompt_main_action("p1");

        let dec1 = engine.pending_decision.clone().unwrap();
        let ans1 = cpu.choose_action_with_engine(&engine, &dec1);
        assert_eq!(ans1["value"]["type"].as_str(), Some("ASSIST"), "【FILE5】解禁のため先に ASSIST すること");
        engine.submit_decision("p1", &serde_json::json!({
            "decisionId": dec1["decisionId"],
            "revision": engine.revision,
            "value": ans1["value"]
        })).unwrap();

        let dec2 = engine.pending_decision.clone().unwrap();
        let ans2 = cpu.choose_action_with_engine(&engine, &dec2);
        assert_eq!(ans2["value"]["type"].as_str(), Some("HAND_PLAY"));
        assert_eq!(ans2["value"]["cardId"].as_str(), Some("h_genta"));
    }
}

#[test]
fn test_cpu_strongest_difficulty_and_four_tier_behaviors() {
    use crate::agent::{CpuDifficulty, CpuWeights, RandomAgent};

    // 1) CpuDifficulty 4種の JSON 往復および Default == Strongest
    assert_eq!(CpuDifficulty::default(), CpuDifficulty::Strongest);
    for (diff, raw) in [
        (CpuDifficulty::Easy, "\"easy\""),
        (CpuDifficulty::Normal, "\"normal\""),
        (CpuDifficulty::Hard, "\"hard\""),
        (CpuDifficulty::Strongest, "\"strongest\""),
    ] {
        assert_eq!(serde_json::to_string(&diff).unwrap(), raw);
        let decoded: CpuDifficulty = serde_json::from_str(raw).unwrap();
        assert_eq!(decoded, diff);
    }

    // 2) 旧形式(12フィールドのみ) JSON から CpuWeights への後方互換デシリアライズ
    let legacy_json = r#"{
        "level_weight": 1100.0,
        "ap_weight": 0.5,
        "lp_weight": 800.0,
        "rush_bonus": 1000.0,
        "on_play_bonus": 900.0,
        "event_penalty": -500.0,
        "removal_bonus": 700.0,
        "assist_min_field": 2,
        "deduce_lp_weight": 1100.0,
        "attack_remove_weight": 900.0,
        "case_attack_bonus": 1000.0,
        "disguise_ap_threshold": 600
    }"#;
    let parsed_weights: CpuWeights = serde_json::from_str(legacy_json).expect("legacy weights deserialization");
    assert_eq!(parsed_weights.level_weight, 1100.0);
    assert_eq!(parsed_weights.evidence_distance_weight.early, CpuWeights::default().evidence_distance_weight.early);
    assert_eq!(parsed_weights.evidence_distance_weight.late, CpuWeights::default().evidence_distance_weight.late);

    // テスト用カタログ構築
    let mut catalog = load_test_catalog();
    let add_def = |cat: &mut catalog::CardCatalog, id: &str, ct: models::CardType, color: &str, lvl: i32, ap: i32, lp: i32, desc: &str| {
        cat.definitions.insert(
            id.to_string(),
            models::CardDefinition {
                definition_id: id.to_string(),
                canonical_id: id.to_string(),
                card_type: ct,
                colors: vec![color.to_string()],
                level: lvl,
                ap,
                lp,
                names: vec![id.to_string()],
                description: desc.to_string(),
                traits: vec![],
                case_levels: None,
                main_thumb: None,
                main_path: None,
            },
        );
    };
    add_def(&mut catalog, "T_PARTNER_BLUE", models::CardType::PARTNER, "青", 0, 0, 1, "");
    add_def(&mut catalog, "T_PARTNER_RED", models::CardType::PARTNER, "赤", 0, 0, 1, "");
    add_def(&mut catalog, "T_CASE_BLUE", models::CardType::CASE, "青", 0, 0, 0, "");
    add_def(&mut catalog, "T_CHAR_LV4", models::CardType::CHARACTER, "青", 4, 4000, 1, "");
    add_def(&mut catalog, "T_CHAR_LV6", models::CardType::CHARACTER, "青", 6, 6000, 1, "");
    add_def(&mut catalog, "T_CHAR_LV7", models::CardType::CHARACTER, "青", 7, 7000, 1, "");
    add_def(&mut catalog, "T_CHAR_AP5000", models::CardType::CHARACTER, "青", 5, 5000, 1, "");
    add_def(&mut catalog, "T_CHAR_AP6000", models::CardType::CHARACTER, "青", 6, 6000, 1, "");
    add_def(&mut catalog, "T_CHAR_AP7000", models::CardType::CHARACTER, "青", 7, 7000, 1, "");
    add_def(&mut catalog, "T_CHAR_WEAK", models::CardType::CHARACTER, "青", 2, 2000, 1, "");
    add_def(&mut catalog, "T_CI_1000", models::CardType::CHARACTER, "青", 2, 2000, 1, "【カットイン】AP+1000");
    add_def(&mut catalog, "T_CI_3000", models::CardType::CHARACTER, "青", 4, 4000, 1, "【カットイン】AP+3000");

    // 3) マリガン（Lv4単体キープ）: Lv2無しでも Lv4 はキープし Lv7 を戻す
    {
        let mut engine = MatchEngine::new(catalog.clone());
        engine.turn = 0;
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];
        let mut ps1 = engine::PlayerState::new("p1".to_string());
        ps1.hand.push(test_inst("h_lv4", "T_CHAR_LV4", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("h_lv7", "T_CHAR_LV7", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), engine::PlayerState::new("p2".to_string()));

        let strongest = RandomAgent::with_difficulty("p1", CpuDifficulty::Strongest);
        let mul_dec = serde_json::json!({
            "kind": "mulligan",
            "options": { "handCardIds": ["h_lv4", "h_lv7"] }
        });
        let ans = strongest.choose_action_with_engine(&engine, &mul_dec);
        assert_eq!(ans["value"]["cardIds"], serde_json::json!(["h_lv7"]), "Strongest は Lv2 無しでも Lv4 単体キープし Lv7 のみ返すこと");
    }

    // 4) 赤対面での序盤P推理回避（Strongest は END_TURN、Hard は P推理 DEDUCE）
    {
        let mut engine = MatchEngine::new(catalog.clone());
        engine.turn = 1;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        ps1.case = Some(test_inst("p1_case", "T_CASE_BLUE", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
        ps1.case_phase = models::CasePhase::INTRO;
        ps1.partner = Some(test_inst("p1_p", "T_PARTNER_BLUE", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
        engine.players.insert("p1".to_string(), ps1);

        let mut ps2 = engine::PlayerState::new("p2".to_string());
        ps2.partner = Some(test_inst("p2_p_red", "T_PARTNER_RED", "p2", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
        engine.players.insert("p2".to_string(), ps2);

        let dec = serde_json::json!({
            "kind": "mainAction",
            "options": {
                "actions": [
                    { "type": "DEDUCE", "actorIds": ["p1_p"] },
                    { "type": "END_TURN" }
                ]
            }
        });

        let strongest = RandomAgent::with_difficulty("p1", CpuDifficulty::Strongest);
        let hard = RandomAgent::with_difficulty("p1", CpuDifficulty::Hard);
        assert_eq!(
            strongest.choose_action_with_engine(&engine, &dec)["value"]["type"].as_str(),
            Some("END_TURN"),
            "Strongest は赤対面・事件編・証拠0枚時にP推理を控えること"
        );
        assert_eq!(
            hard.choose_action_with_engine(&engine, &dec)["value"]["type"].as_str(),
            Some("DEDUCE"),
            "Hard は従来の先行P推理を行うこと"
        );
    }

    // 5) 後攻3T解決編移行直後(FILE 7)での NH 禁止
    {
        let mut engine = MatchEngine::new(catalog.clone());
        engine.turn = 6; // 後攻3ターン目: my_turn_index == 3
        engine.turn_player_id = "p2".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps2 = engine::PlayerState::new("p2".to_string());
        ps2.case_phase = models::CasePhase::RESOLUTION;
        for i in 0..7 {
            ps2.file.push(test_inst(&format!("f2_{}", i), "T_CHAR_LV4", "p2", models::Zone::FILE, models::CardOrientation::ACTIVE));
        }
        ps2.hand.push(test_inst("h2_lv6", "T_CHAR_LV6", "p2", models::Zone::HAND, models::CardOrientation::ACTIVE));
        engine.players.insert("p1".to_string(), engine::PlayerState::new("p1".to_string()));
        engine.players.insert("p2".to_string(), ps2);

        let dec = serde_json::json!({
            "kind": "mainAction",
            "options": {
                "actions": [
                    { "type": "HINT_TAKE" },
                    { "type": "END_TURN" }
                ]
            }
        });
        let strongest = RandomAgent::with_difficulty("p2", CpuDifficulty::Strongest);
        assert_eq!(
            strongest.choose_action_with_engine(&engine, &dec)["value"]["type"].as_str(),
            Some("END_TURN"),
            "Strongest は後攻3T解決編移行直後(FILE 7)に次ターンFILE 5転落を防ぐためNHを使わないこと"
        );
    }

    // 6) リーサル時のパートナー温存（解決編・必要証拠まであと1・場にアクティブLP1あり）
    {
        let mut engine = MatchEngine::new(catalog.clone());
        engine.turn = 9;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        ps1.case_phase = models::CasePhase::RESOLUTION;
        ps1.partner = Some(test_inst("p1_p", "T_PARTNER_BLUE", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
        for i in 0..6 {
            ps1.evidence.push(test_inst(&format!("ev_{}", i), "T_CHAR_LV4", "p1", models::Zone::EVIDENCE, models::CardOrientation::ACTIVE));
        }
        for i in 0..7 {
            ps1.file.push(test_inst(&format!("f_{}", i), "T_CHAR_LV4", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
        }
        ps1.field.push(test_inst("c_field1", "T_CHAR_LV4", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), engine::PlayerState::new("p2".to_string()));

        let dec = serde_json::json!({
            "kind": "mainAction",
            "options": {
                "actions": [
                    { "type": "ASSIST" },
                    { "type": "DEDUCE", "actorIds": ["p1_p", "c_field1"] },
                    { "type": "END_TURN" }
                ]
            }
        });
        let strongest = RandomAgent::with_difficulty("p1", CpuDifficulty::Strongest);
        let ans = strongest.choose_action_with_engine(&engine, &dec);
        assert_eq!(ans["value"]["type"].as_str(), Some("DEDUCE"));
        assert_eq!(
            ans["value"]["actorId"].as_str(),
            Some("c_field1"),
            "リーサル達成時はパートナーを事件解決用に温存し場キャラで推理すること"
        );
    }

    // 7) 最小コストカットインと勝っている側の温存
    {
        let mut engine = MatchEngine::new(catalog.clone());
        engine.turn = 6;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        ps1.field.push(test_inst("att_5000", "T_CHAR_AP5000", "p1", models::Zone::FIELD, models::CardOrientation::SLEEP));
        ps1.field.push(test_inst("att_7000", "T_CHAR_AP7000", "p1", models::Zone::FIELD, models::CardOrientation::SLEEP));
        ps1.hand.push(test_inst("ci_3000", "T_CI_3000", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("ci_1000", "T_CI_1000", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));

        let mut ps2 = engine::PlayerState::new("p2".to_string());
        ps2.field.push(test_inst("def_6000", "T_CHAR_AP6000", "p2", models::Zone::FIELD, models::CardOrientation::SLEEP));
        ps2.field.push(test_inst("def_5000", "T_CHAR_AP5000", "p2", models::Zone::FIELD, models::CardOrientation::SLEEP));

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        let strongest = RandomAgent::with_difficulty("p1", CpuDifficulty::Strongest);

        // 劣勢(5000 vs 6000): 同値勝ちなため +1000 の最小カットインを選ぶ
        engine.action_context = Some(serde_json::json!({
            "attackerPlayerId": "p1",
            "defenderPlayerId": "p2",
            "actorId": "att_5000",
            "targetId": "def_6000"
        }));
        let guard_lose = serde_json::json!({
            "kind": "guard",
            "options": {
                "cutinCardIds": [
                    { "instanceId": "ci_3000" },
                    { "instanceId": "ci_1000" }
                ],
                "guardCardIds": []
            }
        });
        assert_eq!(
            strongest.choose_action_with_engine(&engine, &guard_lose)["value"]["cutinCardId"].as_str(),
            Some("ci_1000"),
            "勝敗が覆る最小ボーナス(+1000)のカットインを選ぶこと"
        );

        // 優勢(7000 vs 5000): 勝っている側からはカットインを切らない
        engine.action_context = Some(serde_json::json!({
            "attackerPlayerId": "p1",
            "defenderPlayerId": "p2",
            "actorId": "att_7000",
            "targetId": "def_5000"
        }));
        assert_eq!(
            strongest.choose_action_with_engine(&engine, &guard_lose)["value"]["guardCardId"],
            serde_json::Value::Null,
            "既に勝っている側からはカットインを切らずスキップすること"
        );
    }

    // 8) mustGuard 強制ガード遵守 (Strongest & Hard)
    {
        let mut engine = MatchEngine::new(catalog.clone());
        engine.turn = 5;
        engine.turn_player_id = "p2".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        ps1.field.push(test_inst("g_weak", "T_CHAR_WEAK", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.field.push(test_inst("g_mid", "T_CHAR_AP5000", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.case = Some(test_inst("p1_case", "T_CASE_BLUE", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));

        let mut ps2 = engine::PlayerState::new("p2".to_string());
        ps2.field.push(test_inst("att_big", "T_CHAR_AP7000", "p2", models::Zone::FIELD, models::CardOrientation::SLEEP));
        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        engine.action_context = Some(serde_json::json!({
            "attackerPlayerId": "p2",
            "defenderPlayerId": "p1",
            "actorId": "att_big",
            "targetId": "p1_case"
        }));
        let guard_must = serde_json::json!({
            "kind": "guard",
            "options": {
                "guardCardIds": ["g_mid", "g_weak"],
                "cutinCardIds": [],
                "mustGuard": true
            }
        });
        for diff in [CpuDifficulty::Hard, CpuDifficulty::Strongest] {
            let cpu = RandomAgent::with_difficulty("p1", diff);
            assert_eq!(
                cpu.choose_action_with_engine(&engine, &guard_must)["value"]["guardCardId"].as_str(),
                Some("g_weak"),
                "{:?} でも mustGuard 時は最小APブロッカーで必ずガードすること",
                diff
            );
        }
    }

    // 9) evaluate_board_state (Tapered Evaluation): FILE 6維持と証拠優位が高評価になること
    {
        let mut engine = MatchEngine::new(catalog);
        engine.turn = 8;
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        ps1.case_phase = models::CasePhase::RESOLUTION;
        for i in 0..6 {
            ps1.file.push(test_inst(&format!("f1_{}", i), "T_CHAR_LV4", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
        }
        ps1.evidence.push(test_inst("e1_0", "T_CHAR_LV4", "p1", models::Zone::EVIDENCE, models::CardOrientation::ACTIVE));

        let mut ps2 = engine::PlayerState::new("p2".to_string());
        ps2.case_phase = models::CasePhase::RESOLUTION;
        for i in 0..6 {
            ps2.file.push(test_inst(&format!("f2_{}", i), "T_CHAR_LV4", "p2", models::Zone::FILE, models::CardOrientation::ACTIVE));
        }
        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        let strongest = RandomAgent::with_difficulty("p1", CpuDifficulty::Strongest);
        let score_f6 = strongest.evaluate_board_state(&engine, "p2");

        // 自証拠を1つ増やすとスコアが明確に上昇すること
        engine.players.get_mut("p1").unwrap().evidence.push(test_inst("e1_1", "T_CHAR_LV4", "p1", models::Zone::EVIDENCE, models::CardOrientation::ACTIVE));
        let score_more_ev = strongest.evaluate_board_state(&engine, "p2");
        assert!(score_more_ev > score_f6, "自証拠増加で評価値が上昇すること ({} > {})", score_more_ev, score_f6);

        // 解決編で FILE 6 -> FILE 5 へ落ちるとペナルティで大幅低下すること
        engine.players.get_mut("p1").unwrap().evidence.pop();
        engine.players.get_mut("p1").unwrap().file.pop();
        let score_f5 = strongest.evaluate_board_state(&engine, "p2");
        assert!(score_f6 > score_f5 + 3000.0, "解決編でFILE6維持はFILE5転落より明確に高評価であること ({} vs {})", score_f6, score_f5);
    }
}

#[test]
fn test_b05102_auto_skip_first_target_still_draws_and_deploys_from_hand() {
    let catalog = load_test_catalog();
    let mut engine = MatchEngine::new(catalog);
    engine.turn = 1;
    engine.phase = models::MatchPhase::MAIN;
    engine.turn_player_id = "p1".to_string();
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];

    let mut ps1 = engine::PlayerState::new("p1".to_string());
    ps1.case_phase = models::CasePhase::INTRO;
    ps1.partner = Some(test_inst("p1_p", "B01079", "p1", models::Zone::PARTNER, models::CardOrientation::SLEEP));
    ps1.case = Some(test_inst("p1_c", "PR115", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
    ps1.file.push(test_inst("p1_f1", "B06092", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
    ps1.file.push(test_inst("p1_f2", "B06092", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
    ps1.hand.push(test_inst("ev_b05102", "B05102", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
    ps1.hand.push(test_inst("ch_lv2", "B06092", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
    ps1.hand.push(test_inst("ch_lv5", "PR163", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
    ps1.deck.push(test_inst("dk_1", "B09078", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
    ps1.deck.push(test_inst("dk_2", "B09078", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
    ps1.deck.push(test_inst("dk_3", "B09078", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));

    let mut ps2 = engine::PlayerState::new("p2".to_string());
    ps2.case_phase = models::CasePhase::INTRO;
    ps2.partner = Some(test_inst("p2_p", "B01060", "p2", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
    ps2.case = Some(test_inst("p2_c", "B11099", "p2", models::Zone::CASE, models::CardOrientation::ACTIVE));

    engine.players.insert("p1".to_string(), ps1);
    engine.players.insert("p2".to_string(), ps2);

    engine.prompt_main_action("p1");
    let pd = engine.pending_decision.clone().expect("mainAction があるはず");
    let dec_id = pd["decisionId"].as_str().unwrap().to_string();
    let res = engine.submit_decision(
        "p1",
        &serde_json::json!({
            "decisionId": dec_id,
            "revision": engine.revision,
            "value": {"type": "HAND_PLAY", "cardId": "ev_b05102"}
        }),
    );
    assert!(res.is_ok(), "B05102 使用が成功すること: {:?}", res);

    // 相手現場0体で第1対象が自動スキップされても、1枚ドロー後に手札登場(PLAY_FROM_HAND)の選択が出ること
    let dec2 = engine.pending_decision.clone().expect("PLAY_FROM_HAND の targetSelect が提示されるはず");
    assert_eq!(dec2["kind"], "targetSelect");
    assert_eq!(dec2["options"]["effectType"], "PLAY_FROM_HAND");
    assert_eq!(dec2["options"]["targetZone"], "HAND");

    let cand_ids: Vec<&str> = dec2["options"]["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["instanceId"].as_str())
        .collect();
    assert!(cand_ids.contains(&"ch_lv2"), "FILE 2枚以下なので Lv2 の B06092 が候補に含まれること");
    assert!(!cand_ids.contains(&"ch_lv5"), "Lv5 の PR163 は候補に含まれないこと");

    let dec2_id = dec2["decisionId"].as_str().unwrap().to_string();
    let res2 = engine.submit_decision(
        "p1",
        &serde_json::json!({
            "decisionId": dec2_id,
            "revision": engine.revision,
            "value": {
                "targetId": "ch_lv2",
                "targetIds": ["ch_lv2"],
                "actorId": "ev_b05102",
                "effectType": "PLAY_FROM_HAND"
            }
        }),
    );
    assert!(res2.is_ok(), "B06092 の登場選択が成功すること: {:?}", res2);

    let p1_after = engine.players.get("p1").unwrap();
    assert!(p1_after.field.iter().any(|c| c.instance_id == "ch_lv2"), "B06092 が現場に登場していること");
    assert_eq!(p1_after.deck.len(), 2, "B05102 の1ドローも解決されていること");
}

#[test]
fn test_b05102_manual_skip_first_target_still_draws_and_deploys_from_hand() {
    let catalog = load_test_catalog();
    let mut engine = MatchEngine::new(catalog);
    engine.turn = 1;
    engine.phase = models::MatchPhase::MAIN;
    engine.turn_player_id = "p1".to_string();
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];

    let mut ps1 = engine::PlayerState::new("p1".to_string());
    ps1.case_phase = models::CasePhase::INTRO;
    ps1.partner = Some(test_inst("p1_p", "B01079", "p1", models::Zone::PARTNER, models::CardOrientation::SLEEP));
    ps1.case = Some(test_inst("p1_c", "PR115", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
    ps1.file.push(test_inst("p1_f1", "B06092", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
    ps1.file.push(test_inst("p1_f2", "B06092", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
    ps1.hand.push(test_inst("ev_b05102", "B05102", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
    ps1.hand.push(test_inst("ch_lv2", "B06092", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
    ps1.deck.push(test_inst("dk_1", "B09078", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
    ps1.deck.push(test_inst("dk_2", "B09078", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
    ps1.deck.push(test_inst("dk_3", "B09078", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));

    let mut ps2 = engine::PlayerState::new("p2".to_string());
    ps2.case_phase = models::CasePhase::INTRO;
    ps2.partner = Some(test_inst("p2_p", "B01060", "p2", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
    ps2.case = Some(test_inst("p2_c", "B11099", "p2", models::Zone::CASE, models::CardOrientation::ACTIVE));
    ps2.field.push(test_inst("opp_ch", "B06092", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE));

    engine.players.insert("p1".to_string(), ps1);
    engine.players.insert("p2".to_string(), ps2);

    engine.prompt_main_action("p1");
    let pd = engine.pending_decision.clone().unwrap();
    let dec_id = pd["decisionId"].as_str().unwrap().to_string();
    engine
        .submit_decision(
            "p1",
            &serde_json::json!({
                "decisionId": dec_id,
                "revision": engine.revision,
                "value": {"type": "HAND_PLAY", "cardId": "ev_b05102"}
            }),
        )
        .unwrap();

    // 相手現場にキャラがいるため第1対象選択が出る -> 任意スキップする
    let dec1 = engine.pending_decision.clone().expect("第1対象選択が出るはず");
    assert_eq!(dec1["kind"], "targetSelect");
    assert_eq!(dec1["options"]["effectType"], "LEVEL_DELTA");
    let dec1_id = dec1["decisionId"].as_str().unwrap().to_string();
    engine
        .submit_decision(
            "p1",
            &serde_json::json!({
                "decisionId": dec1_id,
                "revision": engine.revision,
                "value": {
                    "targetId": null,
                    "targetIds": [],
                    "actorId": "ev_b05102",
                    "effectType": "LEVEL_DELTA"
                }
            }),
        )
        .unwrap();

    // スキップしても1枚ドローが実行され、続いて手札登場(PLAY_FROM_HAND)の選択が出ること
    assert_eq!(
        engine.players.get("p1").unwrap().deck.len(),
        2,
        "第1対象を手動スキップしても1枚ドローが実行されること"
    );
    let dec2 = engine.pending_decision.clone().expect("PLAY_FROM_HAND が提示されるはず");
    assert_eq!(dec2["kind"], "targetSelect");
    assert_eq!(dec2["options"]["effectType"], "PLAY_FROM_HAND");
    let dec2_id = dec2["decisionId"].as_str().unwrap().to_string();
    engine
        .submit_decision(
            "p1",
            &serde_json::json!({
                "decisionId": dec2_id,
                "revision": engine.revision,
                "value": {
                    "targetId": "ch_lv2",
                    "targetIds": ["ch_lv2"],
                    "actorId": "ev_b05102",
                    "effectType": "PLAY_FROM_HAND"
                }
            }),
        )
        .unwrap();
    assert!(engine.players.get("p1").unwrap().field.iter().any(|c| c.instance_id == "ch_lv2"));
}

#[test]
fn test_pr291_auto_skip_first_target_moves_to_partner_and_deploys_once() {
    let catalog = load_test_catalog();
    let mut engine = MatchEngine::new(catalog);
    engine.turn = 4;
    engine.phase = models::MatchPhase::MAIN;
    engine.turn_player_id = "p1".to_string();
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];

    // PR291: 【パートナー白】【解決編】AP7000以上のキャラを1枚まで選び、リムーブする。
    //        このカードをパートナーエリアに移す。
    //        自分の現場に〚カード名［怪盗キッド］〛がいる場合、手札からレベル3以下の【白】のキャラを1枚までスリープ状態で登場させてもよい。
    let mut ps1 = engine::PlayerState::new("p1".to_string());
    ps1.case_phase = models::CasePhase::RESOLUTION;
    ps1.partner = Some(test_inst("p1_p", "B01042", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
    ps1.case = Some(test_inst("p1_c", "B01059", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
    ps1.field.push(test_inst("kid_field", "B02038", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
    ps1.hand.push(test_inst("ev_pr291", "PR291", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
    ps1.hand.push(test_inst("white_lv2", "B01049", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
    for i in 0..5 {
        ps1.file.push(test_inst(&format!("f_{}", i), "B01049", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
    }

    let mut ps2 = engine::PlayerState::new("p2".to_string());
    ps2.case_phase = models::CasePhase::RESOLUTION;
    ps2.partner = Some(test_inst("p2_p", "B01060", "p2", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
    ps2.case = Some(test_inst("p2_c", "B11099", "p2", models::Zone::CASE, models::CardOrientation::ACTIVE));

    engine.players.insert("p1".to_string(), ps1);
    engine.players.insert("p2".to_string(), ps2);

    engine.prompt_main_action("p1");
    let pd = engine.pending_decision.clone().unwrap();
    let dec_id = pd["decisionId"].as_str().unwrap().to_string();
    engine
        .submit_decision(
            "p1",
            &serde_json::json!({
                "decisionId": dec_id,
                "revision": engine.revision,
                "value": {"type": "HAND_PLAY", "cardId": "ev_pr291"}
            }),
        )
        .unwrap();

    // AP7000以上がいないため第1対象が自動スキップされ、PR291がパートナーエリアへ移動した上で、手札白キャラ登場が1回だけ提示される
    assert!(
        engine.players.get("p1").unwrap().extra_partners.iter().any(|c| c.instance_id == "ev_pr291"),
        "PR291 がパートナーエリアへ移動していること"
    );
    let dec2 = engine.pending_decision.clone().expect("白キャラ登場の選択が提示されるはず");
    assert_eq!(dec2["kind"], "targetSelect");
    let dec2_id = dec2["decisionId"].as_str().unwrap().to_string();
    let et = dec2["options"]["effectType"].as_str().unwrap().to_string();
    engine
        .submit_decision(
            "p1",
            &serde_json::json!({
                "decisionId": dec2_id,
                "revision": engine.revision,
                "value": {
                    "targetId": "white_lv2",
                    "targetIds": ["white_lv2"],
                    "actorId": "ev_pr291",
                    "effectType": et
                }
            }),
        )
        .unwrap();

    // 二重発動せず mainAction に戻ること
    assert_eq!(
        engine.pending_decision.as_ref().and_then(|d| d["kind"].as_str()),
        Some("mainAction"),
        "PR291 の後続が二重発動せず mainAction に戻ること"
    );
    assert!(engine.players.get("p1").unwrap().field.iter().any(|c| c.instance_id == "white_lv2"));
}

#[test]
fn test_b04048_on_play_draws_to_7_and_returns_same_count_shuffled_to_deck_bottom() {
    let catalog = load_test_catalog();
    let mut engine = MatchEngine::new(catalog);
    engine.turn = 4;
    engine.phase = models::MatchPhase::MAIN;
    engine.turn_player_id = "p1".to_string();
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];

    // パートナー赤(B01060)の状態で B04048(Lv5)を登場させる。手札は B04048 以外に5枚 -> 7枚になるまで2枚引き、2枚選んで山札下へ
    let mut ps1 = engine::PlayerState::new("p1".to_string());
    ps1.partner = Some(test_inst("p1_p", "B01060", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
    ps1.case = Some(test_inst("p1_c", "B11099", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
    for i in 0..5 {
        ps1.file.push(test_inst(&format!("f_{}", i), "B04048", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
    }
    ps1.hand.push(test_inst("b04048", "B04048", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
    for i in 0..5 {
        ps1.hand.push(test_inst(&format!("h_{}", i), "B06092", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
    }
    for i in 0..5 {
        ps1.deck.push(test_inst(&format!("d_{}", i), "B09077", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
    }

    let mut ps2 = engine::PlayerState::new("p2".to_string());
    ps2.partner = Some(test_inst("p2_p", "B01079", "p2", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
    ps2.case = Some(test_inst("p2_c", "PR115", "p2", models::Zone::CASE, models::CardOrientation::ACTIVE));

    engine.players.insert("p1".to_string(), ps1);
    engine.players.insert("p2".to_string(), ps2);

    engine.prompt_main_action("p1");
    let pd = engine.pending_decision.clone().unwrap();
    let dec_id = pd["decisionId"].as_str().unwrap().to_string();
    engine
        .submit_decision(
            "p1",
            &serde_json::json!({
                "decisionId": dec_id,
                "revision": engine.revision,
                "value": {"type": "HAND_PLAY", "cardId": "b04048"}
            }),
        )
        .unwrap();

    // 7枚になるまで2枚引いた後、2枚を選んで山札下へ戻す targetSelect が提示される
    assert_eq!(engine.players["p1"].hand.len(), 7, "7枚になるまで2枚引いていること");
    assert_eq!(engine.players["p1"].deck.len(), 3, "山札から2枚引かれていること");

    let dec2 = engine.pending_decision.clone().expect("山札下へ戻す手札選択が出ること");
    assert_eq!(dec2["kind"], "targetSelect");
    assert_eq!(dec2["options"]["effectType"], "RETURN_HAND_TO_DECK_BOTTOM_SHUFFLED");
    assert_eq!(dec2["options"]["maxSelect"], 2);
    assert_eq!(dec2["options"]["optional"], false);

    let dec2_id = dec2["decisionId"].as_str().unwrap().to_string();
    engine
        .submit_decision(
            "p1",
            &serde_json::json!({
                "decisionId": dec2_id,
                "revision": engine.revision,
                "value": {
                    "targetIds": ["h_0", "h_1"],
                    "actorId": "b04048",
                    "effectType": "RETURN_HAND_TO_DECK_BOTTOM_SHUFFLED"
                }
            }),
        )
        .unwrap();

    assert_eq!(engine.players["p1"].hand.len(), 5, "2枚戻して手札が5枚になること");
    assert_eq!(engine.players["p1"].deck.len(), 5, "山札下へ2枚戻って山札が5枚になること");
}

#[test]
fn test_b04048_declare_specifies_card_name_and_searches_top_2() {
    let catalog = load_test_catalog();
    let mut engine = MatchEngine::new(catalog);
    engine.turn = 4;
    engine.phase = models::MatchPhase::MAIN;
    engine.turn_player_id = "p1".to_string();
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];

    let mut ps1 = engine::PlayerState::new("p1".to_string());
    ps1.partner = Some(test_inst("p1_p", "B01060", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
    ps1.case = Some(test_inst("p1_c", "B11099", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
    ps1.field.push(test_inst("b04048", "B04048", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
    // 山札上2枚(末尾2つ): 1枚目は榎本梓(B06090)、1番上は安室透(B09077)
    ps1.deck.push(test_inst("d_bot", "B06092", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
    ps1.deck.push(test_inst("d_top2", "B06090", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
    ps1.deck.push(test_inst("d_top1", "B09077", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));

    let mut ps2 = engine::PlayerState::new("p2".to_string());
    ps2.partner = Some(test_inst("p2_p", "B01079", "p2", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
    ps2.case = Some(test_inst("p2_c", "PR115", "p2", models::Zone::CASE, models::CardOrientation::ACTIVE));

    engine.players.insert("p1".to_string(), ps1);
    engine.players.insert("p2".to_string(), ps2);

    engine.prompt_main_action("p1");
    let pd = engine.pending_decision.clone().unwrap();
    let dec_id = pd["decisionId"].as_str().unwrap().to_string();
    engine
        .submit_decision(
            "p1",
            &serde_json::json!({
                "decisionId": dec_id,
                "revision": engine.revision,
                "value": {"type": "DECLARE", "actorId": "b04048"}
            }),
        )
        .unwrap();

    // 1. まずカード名指定の modeSelect(SPECIFY_CARD_NAME, allowFreeText: true) が出ること
    let dec_name = engine.pending_decision.clone().expect("カード名指定プロンプトが出ること");
    assert_eq!(dec_name["kind"], "modeSelect");
    assert_eq!(dec_name["options"]["specialKind"], "SPECIFY_CARD_NAME");
    assert_eq!(dec_name["options"]["allowFreeText"], true);

    let dec_name_id = dec_name["decisionId"].as_str().unwrap().to_string();
    engine
        .submit_decision(
            "p1",
            &serde_json::json!({
                "decisionId": dec_name_id,
                "revision": engine.revision,
                "value": { "cardName": "安室透", "modeIndex": 0 }
            }),
        )
        .unwrap();

    // 2. 山札上2枚がめくられ、「安室透」(d_top1) のみが selectable: true になっていること
    let dec_ds = engine.pending_decision.clone().expect("deckSearch が提示されること");
    assert_eq!(dec_ds["kind"], "deckSearch");
    let revealed = dec_ds["options"]["revealedCards"].as_array().unwrap();
    assert_eq!(revealed.len(), 2);
    let amuro = revealed.iter().find(|c| c["instanceId"] == "d_top1").unwrap();
    let azusa = revealed.iter().find(|c| c["instanceId"] == "d_top2").unwrap();
    assert_eq!(amuro["selectable"], true, "指定カード名「安室透」が選択可能であること");
    assert_eq!(azusa["selectable"], false, "指定外の「榎本梓」は選択不可であること");

    let dec_ds_id = dec_ds["decisionId"].as_str().unwrap().to_string();
    engine
        .submit_decision(
            "p1",
            &serde_json::json!({
                "decisionId": dec_ds_id,
                "revision": engine.revision,
                "value": { "chosenCardIds": ["d_top1"], "actorId": "b04048" }
            }),
        )
        .unwrap();

    assert!(engine.players["p1"].hand.iter().any(|c| c.instance_id == "d_top1"), "安室透を手札に加えていること");
    assert_eq!(engine.players["p1"].deck.len(), 2, "残りがデッキの下へ移っていること");
    assert_eq!(
        engine.pending_decision.as_ref().and_then(|d| d["kind"].as_str()),
        Some("mainAction"),
        "B04048 宣言効果が二重実行されず mainAction に戻ること"
    );
}

#[test]
fn test_b11038_specify_char_name_and_search_top_4() {
    let catalog = load_test_catalog();
    let mut engine = MatchEngine::new(catalog);
    engine.turn = 4;
    engine.phase = models::MatchPhase::MAIN;
    engine.turn_player_id = "p1".to_string();
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];

    let mut ps1 = engine::PlayerState::new("p1".to_string());
    ps1.partner = Some(test_inst("p1_p", "B01042", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
    ps1.case = Some(test_inst("p1_c", "B01059", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
    for i in 0..5 {
        ps1.file.push(test_inst(&format!("f_{}", i), "B01049", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
    }
    ps1.hand.push(test_inst("b11038", "B11038", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
    ps1.deck.push(test_inst("d1", "B06090", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
    ps1.deck.push(test_inst("d2", "B06092", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
    ps1.deck.push(test_inst("d3", "B09077", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
    ps1.deck.push(test_inst("d4", "B01049", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));

    let mut ps2 = engine::PlayerState::new("p2".to_string());
    ps2.partner = Some(test_inst("p2_p", "B01079", "p2", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
    ps2.case = Some(test_inst("p2_c", "PR115", "p2", models::Zone::CASE, models::CardOrientation::ACTIVE));

    engine.players.insert("p1".to_string(), ps1);
    engine.players.insert("p2".to_string(), ps2);

    engine.prompt_main_action("p1");
    let pd = engine.pending_decision.clone().unwrap();
    let dec_id = pd["decisionId"].as_str().unwrap().to_string();
    engine
        .submit_decision(
            "p1",
            &serde_json::json!({
                "decisionId": dec_id,
                "revision": engine.revision,
                "value": {"type": "HAND_PLAY", "cardId": "b11038"}
            }),
        )
        .unwrap();

    let dec_name = engine.pending_decision.clone().expect("B11038 でキャラカード名指定が出ること");
    assert_eq!(dec_name["options"]["specialKind"], "SPECIFY_CARD_NAME");
    assert_eq!(dec_name["options"]["charOnly"], true);

    let dec_name_id = dec_name["decisionId"].as_str().unwrap().to_string();
    engine
        .submit_decision(
            "p1",
            &serde_json::json!({
                "decisionId": dec_name_id,
                "revision": engine.revision,
                "value": { "cardName": "怪盗キッド", "modeIndex": 0 }
            }),
        )
        .unwrap();

    let dec_ds = engine.pending_decision.clone().expect("山札上4枚めくりが出ること");
    let revealed = dec_ds["options"]["revealedCards"].as_array().unwrap();
    assert_eq!(revealed.len(), 4);
    let kid = revealed.iter().find(|c| c["instanceId"] == "d4").unwrap();
    assert_eq!(kid["selectable"], true);

    let dec_ds_id = dec_ds["decisionId"].as_str().unwrap().to_string();
    engine
        .submit_decision(
            "p1",
            &serde_json::json!({
                "decisionId": dec_ds_id,
                "revision": engine.revision,
                "value": { "chosenCardIds": [], "actorId": "b11038" }
            }),
        )
        .unwrap();
    assert_eq!(
        engine.pending_decision.as_ref().and_then(|d| d["kind"].as_str()),
        Some("mainAction"),
        "B11038 登場時効果が二重実行されず mainAction に戻ること"
    );
}

#[test]
fn test_b06090_and_hirameki_cannot_target_self_in_remove_area() {
    let catalog = load_test_catalog();

    // 1) リムーブエリアに他の喫茶ポアロキャラがいない場合、B06090のヒラメキで自分自身を拾えないこと
    {
        let mut engine = MatchEngine::new(catalog.clone());
        engine.turn = 4;
        engine.phase = models::MatchPhase::MAIN;
        engine.turn_player_id = "p2".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        ps1.partner = Some(test_inst("p1_p", "B01079", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
        ps1.case = Some(test_inst("p1_c", "PR115", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
        ps1.evidence.push(test_inst("ev_azusa", "B06090", "p1", models::Zone::EVIDENCE, models::CardOrientation::ACTIVE));

        let mut ps2 = engine::PlayerState::new("p2".to_string());
        ps2.partner = Some(test_inst("p2_p", "B01060", "p2", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
        ps2.case = Some(test_inst("p2_c", "B11099", "p2", models::Zone::CASE, models::CardOrientation::ACTIVE));

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        let (opened_ui, _) = engine.remove_evidence_and_trigger_hirameki("p1", 1, "MAIN");
        assert!(!opened_ui, "他に有効な対象がいないため選択UIが開かないこと");
        assert!(engine.pending_decision.is_none());
        assert!(engine.players["p1"].hand.is_empty(), "B06090自身が手札に加わらないこと");
        assert!(
            engine.players["p1"].remove.iter().any(|c| c.instance_id == "ev_azusa"),
            "B06090はリムーブエリアに留まること"
        );
    }

    // 2) リムーブエリアに別の喫茶ポアロキャラ(B06092)がいる場合、B06092のみが候補になりB06090自身は除外されること
    {
        let mut engine = MatchEngine::new(catalog);
        engine.turn = 4;
        engine.phase = models::MatchPhase::MAIN;
        engine.turn_player_id = "p2".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        ps1.partner = Some(test_inst("p1_p", "B01079", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
        ps1.case = Some(test_inst("p1_c", "PR115", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
        ps1.evidence.push(test_inst("ev_azusa", "B06090", "p1", models::Zone::EVIDENCE, models::CardOrientation::ACTIVE));
        ps1.remove.push(test_inst("rem_kanemaru", "B06092", "p1", models::Zone::REMOVE, models::CardOrientation::ACTIVE));

        let mut ps2 = engine::PlayerState::new("p2".to_string());
        ps2.partner = Some(test_inst("p2_p", "B01060", "p2", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
        ps2.case = Some(test_inst("p2_c", "B11099", "p2", models::Zone::CASE, models::CardOrientation::ACTIVE));

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        let (opened_ui, _) = engine.remove_evidence_and_trigger_hirameki("p1", 1, "MAIN");
        assert!(opened_ui, "別の喫茶ポアロキャラ(B06092)が候補にいるため選択UIが開くこと");
        let dec = engine.pending_decision.as_ref().unwrap();
        let cand_ids: Vec<&str> = dec["options"]["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|c| c["instanceId"].as_str())
            .collect();
        assert!(cand_ids.contains(&"rem_kanemaru"), "B06092は候補に含まれること");
        assert!(!cand_ids.contains(&"ev_azusa"), "ヒラメキ発動中のB06090自身は候補に含まれないこと");
    }
}

