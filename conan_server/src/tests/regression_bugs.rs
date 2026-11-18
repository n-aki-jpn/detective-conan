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
    fn test_parse_official_deck_html() {
        let html = r#"<html><body><form><input type="hidden" name="deck" value="{&quot;partner&quot;:[&quot;B01001&quot;],&quot;incident&quot;:[&quot;B01002&quot;],&quot;other&quot;:[&quot;B01003&quot;,&quot;B01003&quot;,&quot;B01004&quot;]}"></form></body></html>"#;
        let deck = parse_official_deck_html(html).expect("Must parse valid official deck html");
        assert_eq!(deck["partner"][0], "B01001");
        assert_eq!(deck["incident"][0], "B01002");
        assert_eq!(deck["other"].as_array().unwrap().len(), 3);
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
        assert_eq!(obs2["pendingDecision"]["kind"].as_str(), Some("targetSelect"));
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
        assert_eq!(obs2["pendingDecision"]["kind"].as_str(), Some("targetSelect"));
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
    fn test_index_html_no_undefined_is_secret_in_card_slot() {
        let html = std::fs::read_to_string("../src/conan_engine/web/static/index.html")
            .expect("index.html must be readable");
        let start = html.find("function createCardSlot(").expect("createCardSlot must exist");
        let end = html[start..].find("\n    function ").map(|i| start + i).unwrap_or(html.len());
        let fn_body = &html[start..end];
        assert!(
            !fn_body.contains("isSecret"),
            "createCardSlot must not reference undefined variable isSecret"
        );
    }

    #[test]
    fn test_index_html_js_syntax_and_smoke() {
        let html = std::fs::read_to_string("../src/conan_engine/web/static/index.html")
            .expect("index.html must be readable");
        let script_start = html.rfind("<script>").expect("<script> tag must exist") + "<script>".len();
        let script_end = html[script_start..].find("</script>").expect("</script> tag must exist") + script_start;
        let js_code = &html[script_start..script_end];

        let tmp_dir = std::env::temp_dir();
        let js_path = tmp_dir.join("conan_index_check.js");
        std::fs::write(&js_path, js_code).expect("failed to write temp js");

        // 1. 構文チェック (node --check)
        let check_out = std::process::Command::new("node")
            .arg("--check")
            .arg(&js_path)
            .output()
            .expect("node must be available");
        assert!(
            check_out.status.success(),
            "index.html JS syntax error:\n{}",
            String::from_utf8_lossy(&check_out.stderr)
        );

        // 2. モックDOM上でスクリプトを読み込み、createCardSlot / createCardEl / render のUI更新をスモークテスト
        let smoke_js = format!(
            r#"
const assert = require('node:assert/strict');
const elsById = {{}};
const makeEl = () => {{
  const classes = new Set();
  const styleProps = {{}};
  const obj = {{
    style: {{
      setProperty(k, v) {{ styleProps[k] = v; }},
      removeProperty(k) {{ delete styleProps[k]; }},
      getPropertyValue(k) {{ return styleProps[k] || ''; }}
    }},
    classList: {{
      add(...xs) {{ xs.forEach(x => classes.add(x)); }},
      remove(...xs) {{ xs.forEach(x => classes.delete(x)); }},
      contains(x) {{ return classes.has(x); }},
      toggle(x, force) {{
        if (force === true) classes.add(x);
        else if (force === false) classes.delete(x);
        else if (classes.has(x)) classes.delete(x);
        else classes.add(x);
      }}
    }},
    dataset: {{}},
    children: [],
    appendChild(c) {{ this.children.push(c); return c; }},
    replaceWith() {{}},
    addEventListener() {{}},
    querySelector() {{ return makeEl(); }},
    querySelectorAll() {{ return []; }},
    setAttribute(k, v) {{ this['attr_' + k] = v; }},
    getAttribute(k) {{ return this['attr_' + k] || ''; }},
    getBoundingClientRect() {{ return {{ left: 0, top: 0, width: 100, height: 100 }}; }},
    paused: true,
    pause() {{ this.paused = true; }},
    play() {{ this.paused = false; return Promise.resolve(); }}
  }};
  return new Proxy(obj, {{ get(t, p) {{ return p in t ? t[p] : ''; }} }});
}};
const bottomDockEl = makeEl();
const getEl = (id) => {{
  if (!elsById[id]) elsById[id] = makeEl();
  return elsById[id];
}};
global.window = global;
global.innerWidth = 390;
global.addEventListener = () => {{}};
global.setInterval = () => 0;
global.setTimeout = () => 0;
global.location = {{ search: '', href: 'http://localhost:8000/' }};
global.localStorage = {{ getItem(){{ return null; }}, setItem(){{}}, removeItem(){{}} }};
global.document = {{
  getElementById: (id) => getEl(id),
  createElement: () => makeEl(),
  querySelector: (sel) => sel === '.bottom-dock' ? bottomDockEl : makeEl(),
  querySelectorAll: () => [],
  addEventListener: () => {{}}
}};
global.fetch = async () => ({{ ok: true, json: async () => ({{}}) }});
global.Audio = function() {{ return {{ play: async () => {{}}, pause: () => {{}} }}; }};
{js}
createCardSlot('slot_1', {{ instanceId: 'c1', definitionId: 'B01001', name: '江戸川コナン', cardType: 'CHARACTER', level: 1, ap: 1000, lp: 1, orientation: 'ACTIVE', zone: 'FIELD' }}, {{ isMine: true, zone: 'FIELD' }});
createCardEl(null, true, false, null);

// targetSelect UI スモークテスト（全文表示・強調バー・効果文トグル）
catalogData.push({{
  definitionId: 'B11007P',
  title: '吉田歩美',
  cardType: 'CHARACTER',
  level: 4,
  ap: 3000,
  lp: 1,
  colors: ['青'],
  description: '【登場時】リムーブする相手のキャラを1枚まで選ぶ。',
  mainPath: 'sample_b11007p.jpg'
}});
const samplePlayers = () => ({{
  me: {{
    playerId: 'player1',
    isFirstPlayer: false,
    casePhase: 'RESOLUTION',
    fileCount: 8,
    rawFileCount: 7,
    partnerInFile: true,
    deckCount: 17,
    evidenceCount: 0,
    removeCount: 8,
    handCount: 1,
    hand: [{{ instanceId: 'h1', definitionId: 'B01001', name: '江戸川コナン', cardType: 'CHARACTER', level: 4, ap: 4000, lp: 1, orientation: 'ACTIVE', zone: 'HAND', faceUp: true }}],
    field: [{{ instanceId: 'inst_ayumi', definitionId: 'B11007P', name: '吉田歩美', cardType: 'CHARACTER', level: 4, ap: 3000, lp: 1, orientation: 'ACTIVE', zone: 'FIELD', faceUp: true }}],
    partner: {{ instanceId: 'p1_p', definitionId: 'B01001', name: '江戸川コナン', cardType: 'PARTNER', level: 0, ap: 0, lp: 1, orientation: 'ACTIVE', zone: 'PARTNER', faceUp: true }},
    extraPartners: [],
    case: {{ instanceId: 'p1_c', definitionId: 'B01024', name: '事件', cardType: 'CASE', level: 0, ap: 0, lp: 0, orientation: 'ACTIVE', zone: 'CASE', faceUp: true }},
    evidence: [],
    remove: []
  }},
  opponent: {{
    playerId: 'player2',
    isFirstPlayer: true,
    casePhase: 'RESOLUTION',
    fileCount: 6,
    rawFileCount: 5,
    partnerInFile: true,
    deckCount: 19,
    evidenceCount: 3,
    removeCount: 6,
    handCount: 4,
    hand: [],
    field: [{{ instanceId: 'opp_1', definitionId: 'B05055', name: '相手キャラ', cardType: 'CHARACTER', level: 4, ap: 4000, lp: 1, orientation: 'SLEEP', zone: 'FIELD', faceUp: true }}],
    partner: {{ instanceId: 'p2_p', definitionId: 'B01001', name: '江戸川コナン', cardType: 'PARTNER', level: 0, ap: 0, lp: 1, orientation: 'ACTIVE', zone: 'PARTNER', faceUp: true }},
    extraPartners: [],
    case: {{ instanceId: 'p2_c', definitionId: 'B01024', name: '事件', cardType: 'CASE', level: 0, ap: 0, lp: 0, orientation: 'ACTIVE', zone: 'CASE', faceUp: true }},
    evidence: [],
    remove: []
  }}
}});

render({{
  revision: 12,
  turn: 8,
  turnPlayerId: 'player1',
  decisionPlayerId: 'player1',
  viewingPlayerId: 'player1',
  logs: ['テスト開始'],
  ...samplePlayers(),
  pendingDecision: {{
    decisionId: 'dec_target_1',
    revision: 12,
    playerId: 'player1',
    kind: 'targetSelect',
    options: {{
      actorId: 'inst_ayumi',
      actorName: '吉田歩美(B11007P)',
      effectType: 'REMOVE',
      targetZone: 'FIELD',
      maxSelect: 1,
      optional: true,
      title: 'リムーブする相手のカードを1枚選択してください',
      candidates: [
        {{ instanceId: 'opp_1', definitionId: 'B05055', name: '相手キャラ', level: 4, ap: 4000, zoneLabel: '現場', zone: 'FIELD', isOpponent: true }}
      ]
    }}
  }}
}});

assert.equal(getEl('dock-command-bar').classList.contains('is-prompt-active'), true);
assert.equal(bottomDockEl.classList.contains('is-prompt-active'), true);
assert.equal(getEl('dock-status-desc').innerText, 'リムーブする相手のカードを1枚選択してください');
assert.ok(getEl('dock-status-badge').innerText.includes('吉田歩美(B11007P)'));
assert.ok(getEl('dock-status-badge').innerText.includes('🔍'));
assert.ok(getEl('dock-status-sub').innerText.includes('緑枠'));
assert.equal(getEl('dock-effect-toggle').style.display, 'inline-flex');
assert.equal(getEl('dock-effect-text').style.display, 'none');
getEl('dock-effect-toggle').onclick();
assert.equal(getEl('dock-effect-text').style.display, 'block');
assert.ok(getEl('dock-effect-text').innerText.includes('リムーブする相手のキャラ'));

// 通常手番に戻ったら強調状態と効果文トグルが解除されること
render({{
  revision: 13,
  turn: 8,
  turnPlayerId: 'player1',
  decisionPlayerId: 'player1',
  viewingPlayerId: 'player1',
  logs: ['テスト開始'],
  ...samplePlayers(),
  pendingDecision: {{
    decisionId: 'dec_main_1',
    revision: 13,
    playerId: 'player1',
    kind: 'mainAction',
    options: {{
      actions: [{{ type: 'END_TURN' }}]
    }}
  }}
}});
assert.equal(getEl('dock-command-bar').classList.contains('is-prompt-active'), false);
assert.equal(bottomDockEl.classList.contains('is-prompt-active'), false);
assert.equal(getEl('dock-effect-toggle').style.display, 'none');
"#,
            js = js_code
        );
        let smoke_path = tmp_dir.join("conan_index_smoke.js");
        std::fs::write(&smoke_path, smoke_js).expect("failed to write smoke js");
        let smoke_out = std::process::Command::new("node")
            .arg(&smoke_path)
            .output()
            .expect("node smoke run failed");
        assert!(
            smoke_out.status.success(),
            "index.html JS runtime smoke error:\n{}",
            String::from_utf8_lossy(&smoke_out.stderr)
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
            assert_eq!(obs2["pendingDecision"]["kind"].as_str(), Some("targetSelect"));
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
    assert_eq!(obs2["pendingDecision"]["kind"].as_str(), Some("targetSelect"));
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
    assert_eq!(obs2["pendingDecision"]["kind"].as_str(), Some("targetSelect"));
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
        assert_eq!(obs2["pendingDecision"]["kind"].as_str(), Some("targetSelect"));
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
    assert_eq!(obs2["pendingDecision"]["kind"].as_str(), Some("targetSelect"));
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

