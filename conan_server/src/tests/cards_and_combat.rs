use super::*;

    #[test]
    fn test_trait_target_filtering_mori_office_and_server_validation() {
        let catalog = load_test_catalog();
        // Verify catalog loaded traits properly
        let b04005 = catalog.get("B04005").expect("B04005 exists");
        assert!(b04005.traits.iter().any(|t| t == "毛利探偵事務所"));

        let mut engine = MatchEngine::new(catalog);
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let ps2 = engine::PlayerState::new("p2".to_string());
        // B04005: 【宣言】【ターン1】このキャラ以外の〚特徴［毛利探偵事務所］〛のキャラを1枚まで選び、AP+1000
        ps1.field.push(test_inst("p1_conan_b04005", "B04005", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        // B01007: 毛利蘭 (毛利探偵事務所) -> MUST match
        ps1.field.push(test_inst("p1_ran", "B01007", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        // B01006: 阿笠博士 (少年探偵団, 科学者 - NOT 毛利探偵事務所!) -> MUST BE EXCLUDED
        ps1.field.push(test_inst("p1_agasa", "B01006", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        for i in 0..7 {
            ps1.file.push(test_inst(&format!("f{}", i), "B01005", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
        }

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        engine.prompt_main_action("p1");
        let pd = engine.pending_decision.as_ref().unwrap().clone();
        let dec_id = pd["decisionId"].as_str().unwrap().to_string();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": dec_id,
            "revision": engine.revision,
            "value": {"type": "DECLARE", "actorId": "p1_conan_b04005"}
        }));

        let ts_pd = engine.pending_decision.as_ref().unwrap().clone();
        assert_eq!(ts_pd["kind"], "targetSelect");
        let cands = ts_pd["options"]["candidates"].as_array().unwrap();
        let cand_ids: Vec<&str> = cands.iter().filter_map(|c| c["instanceId"].as_str()).collect();
        // Only p1_ran matches! Both self (p1_conan_b04005) and non-Mori-office (p1_agasa) must be excluded.
        assert_eq!(cand_ids, vec!["p1_ran"]);

        // Server-side validation must reject tampering with invalid target p1_agasa
        let ts_dec_id = ts_pd["decisionId"].as_str().unwrap().to_string();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": ts_dec_id,
            "revision": engine.revision,
            "value": {"targetId": "p1_agasa", "actorId": "p1_conan_b04005", "effectType": "AP_BUFF"}
        }));
        assert_eq!(engine.players.get("p1").unwrap().field[2].temp_ap_bonus, 0);
    }

    #[test]
    fn test_b04005_clause_isolation_and_post_draw_discard() {
        let catalog = load_test_catalog();
        let mut engine = MatchEngine::new(catalog);
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let ps2 = engine::PlayerState::new("p2".to_string());
        for i in 0..7 {
            ps1.file.push(test_inst(&format!("f{}", i), "B01005", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
        }
        ps1.deck.push(test_inst("d1", "B01007", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        ps1.deck.push(test_inst("d2", "B01006", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("h_b04005", "B04005", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps1.field.push(test_inst("p1_ran", "B01007", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        engine.prompt_main_action("p1");
        let pd = engine.pending_decision.as_ref().unwrap().clone();
        let dec_id = pd["decisionId"].as_str().unwrap().to_string();
        // Play B04005 from empty remaining hand: should draw 2 then remove 2 from hand (NOT fizzle, and NOT trigger Declare AP+1000)
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": dec_id,
            "revision": engine.revision,
            "value": {"type": "HAND_PLAY", "cardId": "h_b04005"}
        }));

        // B04005: 2枚ドロー後、手札を2枚選ぶ DISCARD_HAND が開く
        let pd_disc = engine.pending_decision.as_ref().unwrap().clone();
        assert_eq!(pd_disc["kind"], "targetSelect");
        assert_eq!(pd_disc["options"]["effectType"], "DISCARD_HAND");
        assert_eq!(pd_disc["options"]["maxSelect"], 2);
        assert_eq!(pd_disc["options"]["optional"], false);

        let cands = pd_disc["options"]["candidates"].as_array().unwrap();
        assert_eq!(cands.len(), 2);
        let ids: Vec<&str> = cands.iter().map(|c| c["instanceId"].as_str().unwrap()).collect();
        let disc_id = pd_disc["decisionId"].as_str().unwrap().to_string();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": disc_id,
            "revision": engine.revision,
            "value": {"targetIds": ids, "actorId": "h_b04005", "effectType": "DISCARD_HAND"}
        }));

        let ps1_after = engine.players.get("p1").unwrap();
        assert_eq!(ps1_after.deck.len(), 0);
        assert_eq!(ps1_after.remove.len(), 2);
        assert_eq!(ps1_after.hand.len(), 0);
        // Should return cleanly to mainAction without opening Declare's targetSelect
        assert_eq!(engine.pending_decision.as_ref().unwrap()["kind"], "mainAction");
    }

    #[test]
    fn test_b05006_state_branching_presence_cond_and_followup_draw() {
        let catalog = load_test_catalog();
        let mut engine = MatchEngine::new(catalog);
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let mut ps2 = engine::PlayerState::new("p2".to_string());
        ps1.partner = Some(test_inst("p1_partner", "B01001", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE)); // Blue partner
        ps1.field.push(test_inst("p1_b05006", "B05006", "p1", models::Zone::FIELD, models::CardOrientation::SLEEP));
        ps1.deck.push(test_inst("d1", "B01005", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));

        // Enemy low AP (<=8000) vs high AP (9000 -> must be filtered out!)
        ps2.field.push(test_inst("opp_low", "B01005", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps2.field.push(test_inst("opp_hi", "B01004", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE)); // Check AP below if >8000, else add synthetic 9000

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        // Without another 毛利探偵事務所 or 妃英理 on field, DECLARE must NOT be offered for B05006
        engine.prompt_main_action("p1");
        let pd1 = engine.pending_decision.as_ref().unwrap();
        let has_decl = pd1["options"]["actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["type"] == "DECLARE");
        assert!(!has_decl);

        // Add 毛利蘭 (毛利探偵事務所) and make sure opp_hi is AP 9000
        engine.players.get_mut("p1").unwrap().field.push(test_inst("p1_ran", "B01007", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        engine.players.get_mut("p2").unwrap().field[1].temp_ap_bonus = 9000;

        engine.prompt_main_action("p1");
        let pd2 = engine.pending_decision.as_ref().unwrap().clone();
        let dec_id = pd2["decisionId"].as_str().unwrap().to_string();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": dec_id,
            "revision": engine.revision,
            "value": {"type": "DECLARE", "actorId": "p1_b05006"}
        }));

        let ts_pd = engine.pending_decision.as_ref().unwrap().clone();
        assert_eq!(ts_pd["kind"], "targetSelect");
        let cands = ts_pd["options"]["candidates"].as_array().unwrap();
        let cand_ids: Vec<&str> = cands.iter().filter_map(|c| c["instanceId"].as_str()).collect();
        assert_eq!(cand_ids, vec!["opp_low"]); // opp_hi (AP > 8000) excluded!

        let ts_dec_id = ts_pd["decisionId"].as_str().unwrap().to_string();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": ts_dec_id,
            "revision": engine.revision,
            "value": {"targetId": "opp_low", "actorId": "p1_b05006", "effectType": "REMOVE"}
        }));

        // Enemy opp_low removed AND follow-up "カードを1枚引く" drew d1 into hand!
        assert_eq!(engine.players.get("p2").unwrap().field.len(), 1);
        assert_eq!(engine.players.get("p1").unwrap().hand.len(), 1);
    }

    #[test]
    fn test_deck_search_selectable_filtering_and_validation() {
        let mut catalog = load_test_catalog();
        catalog.definitions.insert(
            "TEST_SEARCH_LP0_BLUE".to_string(),
            models::CardDefinition {
                definition_id: "TEST_SEARCH_LP0_BLUE".to_string(),
                canonical_id: "TEST_SEARCH_LP0_BLUE".to_string(),
                card_type: models::CardType::CHARACTER,
                colors: vec!["青".to_string()],
                level: 2,
                ap: 2000,
                lp: 0,
                names: vec!["サーチャー".to_string()],
                description: "【登場時】自分のデッキのカードを上から2枚見る。その中からLP0の【青】のキャラを1枚まで公開して手札に加え、残りを好きな順番でデッキの下に移す。".to_string(),
                traits: vec![],
                case_levels: None,
                main_thumb: None,
                main_path: None,
            },
        );
        catalog.definitions.insert(
            "TEST_RED_LP0".to_string(),
            models::CardDefinition {
                definition_id: "TEST_RED_LP0".to_string(),
                canonical_id: "TEST_RED_LP0".to_string(),
                card_type: models::CardType::CHARACTER,
                colors: vec!["赤".to_string()],
                level: 2,
                ap: 2000,
                lp: 0,
                names: vec!["非対象赤".to_string()],
                description: "".to_string(),
                traits: vec![],
                case_levels: None,
                main_thumb: None,
                main_path: None,
            },
        );

        let mut engine = MatchEngine::new(catalog);
        engine.turn = 3;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let ps2 = engine::PlayerState::new("p2".to_string());
        for i in 0..5 {
            ps1.file.push(test_inst(&format!("f{}", i), "B01005", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
        }
        // Top of deck is last elements: bottom="B01005"(bottom), top2=["TEST_RED_LP0", "TEST_SEARCH_LP0_BLUE"]
        ps1.deck.push(test_inst("dk_bot", "B01005", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        ps1.deck.push(test_inst("dk_red", "TEST_RED_LP0", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        ps1.deck.push(test_inst("dk_blue0", "TEST_SEARCH_LP0_BLUE", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("h_src", "TEST_SEARCH_LP0_BLUE", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        engine.prompt_main_action("p1");
        let pd = engine.pending_decision.as_ref().unwrap().clone();
        let dec_id = pd["decisionId"].as_str().unwrap().to_string();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": dec_id,
            "revision": engine.revision,
            "value": {"type": "HAND_PLAY", "cardId": "h_src"}
        }));

        let ds_pd = engine.pending_decision.as_ref().unwrap().clone();
        assert_eq!(ds_pd["kind"], "deckSearch");
        let revealed = ds_pd["options"]["revealedCards"].as_array().unwrap();
        assert_eq!(revealed.len(), 2);
        let r_blue = revealed.iter().find(|c| c["instanceId"] == "dk_blue0").unwrap();
        let r_red = revealed.iter().find(|c| c["instanceId"] == "dk_red").unwrap();
        assert_eq!(r_blue["selectable"], true);
        assert_eq!(r_red["selectable"], false);

        // Picking valid dk_blue0 adds it to hand and places dk_red on bottom of deck
        let ds_id = ds_pd["decisionId"].as_str().unwrap().to_string();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": ds_id,
            "revision": engine.revision,
            "value": {"chosenCardId": "dk_blue0", "actorId": "h_src"}
        }));

        let ps1_after = engine.players.get("p1").unwrap();
        assert_eq!(ps1_after.hand.len(), 1);
        assert_eq!(ps1_after.hand[0].instance_id, "dk_blue0");
        assert_eq!(ps1_after.deck.len(), 2);
        assert_eq!(ps1_after.deck[0].instance_id, "dk_red"); // placed at deck bottom
    }

    #[test]
    fn test_hirameki_trigger_and_disguise_deck_bottom() {
        let catalog = load_test_catalog();
        let mut engine = MatchEngine::new(catalog);
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let mut ps2 = engine::PlayerState::new("p2".to_string());

        // Give p1 a white case (B01099 or white case) & 6 files & disguise B02038 + char on field
        ps1.r#case = Some(test_inst("p1_case", "B01100", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE)); // 青＆白
        for i in 0..6 {
            ps1.file.push(test_inst(&format!("f{}", i), "B01005", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
        }
        ps1.field.push(test_inst("p1_f1", "B01005", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("h_kid", "B02038", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE)); // 【変装】【事件白】【FILE6】
        ps1.deck.push(test_inst("p1_d1", "B01005", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));

        // p2 has evidence D03013 (【ヒラメキ】キャラを1枚まで選び、スリープさせる) & active attacker
        ps2.r#case = Some(test_inst("p2_case", "B01002", "p2", models::Zone::CASE, models::CardOrientation::ACTIVE));
        ps2.evidence.push(test_inst("p2_ev1", "D03013", "p2", models::Zone::EVIDENCE, models::CardOrientation::ACTIVE));

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        // 1) Disguise B02038 onto p1_f1 -> p1_f1 must move to DECK bottom (NOT remove), draw 1 from 【変装時】!
        engine.prompt_main_action("p1");
        let pd = engine.pending_decision.as_ref().unwrap().clone();
        let dec_id = pd["decisionId"].as_str().unwrap().to_string();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": dec_id,
            "revision": engine.revision,
            "value": {"type": "DISGUISE", "cardId": "h_kid", "targetId": "p1_f1"}
        }));

        let ps1_after_disguise = engine.players.get("p1").unwrap();
        assert_eq!(ps1_after_disguise.remove.len(), 0);
        assert_eq!(ps1_after_disguise.deck[0].instance_id, "p1_f1");
        assert_eq!(ps1_after_disguise.hand.len(), 1); // drew p1_d1 from 【変装時】

        // 2) Simulate Case attack hitting D03013 in p2's evidence -> Hirameki opens targetSelect for p2 to sleep p1's Kid!
        engine.action_context = Some(serde_json::json!({
            "attackerPlayerId": "p1",
            "defenderPlayerId": "p2",
            "actorId": "h_kid",
            "targetId": "p2_case"
        }));
        let g_pd_dec = engine.generate_decision_id();
        engine.pending_decision = Some(serde_json::json!({
            "decisionId": g_pd_dec,
            "revision": engine.revision,
            "playerId": "p2",
            "kind": "guard",
            "options": {}
        }));
        let _ = engine.submit_decision("p2", &serde_json::json!({
            "decisionId": g_pd_dec,
            "revision": engine.revision,
            "value": {"guardCardId": null}
        }));

        let h_pd = engine.pending_decision.as_ref().unwrap().clone();
        assert_eq!(h_pd["kind"], "targetSelect");
        assert_eq!(h_pd["playerId"], "p2");
        assert_eq!(h_pd["options"]["effectType"], "SLEEP");
        let h_dec_id = h_pd["decisionId"].as_str().unwrap().to_string();
        let _ = engine.submit_decision("p2", &serde_json::json!({
            "decisionId": h_dec_id,
            "revision": engine.revision,
            "value": {"targetId": "h_kid", "actorId": "p2_ev1", "effectType": "SLEEP"}
        }));

        assert_eq!(engine.players.get("p1").unwrap().field[0].orientation, models::CardOrientation::SLEEP);
        assert_eq!(engine.players.get("p1").unwrap().evidence.len(), 1); // p1 gained 1 evidence from case hit
    }

    #[test]
    fn test_hirameki_extended_ops_and_trigger_isolation() {
        let non_hirameki = "【登場時】自分のリムーブエリアにある【カットイン】と【ヒラメキ】以外の元の能力を持たないキャラを1枚まで選び、登場させる。";
        assert!(effects::extract_clauses(non_hirameki, effects::TriggerKind::Hirameki, models::CardType::CHARACTER).is_empty());

        let p_hand = effects::parse_effect_steps("【ヒラメキ】このカードを手札に加える。", None, models::CardType::CHARACTER);
        assert!(p_hand.immediate_before.iter().any(|op| matches!(op, effects::ImmediateOp::MoveSelfFromEvidenceToHand)));

        let p_partner = effects::parse_effect_steps("【ヒラメキ】このカードをパートナーエリアに移す。", None, models::CardType::CHARACTER);
        assert!(p_partner.immediate_before.iter().any(|op| matches!(op, effects::ImmediateOp::MoveSelfFromEvidenceToPartner)));

        let p_opp_disc = effects::parse_effect_steps("【ヒラメキ】相手は手札を1枚リムーブする。", None, models::CardType::CHARACTER);
        assert!(p_opp_disc.immediate_before.iter().any(|op| matches!(op, effects::ImmediateOp::OppDiscardHand(1))));

        let p_opp_mill = effects::parse_effect_steps("【ヒラメキ】相手のデッキのカードを上から4枚リムーブする。", None, models::CardType::CHARACTER);
        assert!(p_opp_mill.immediate_before.iter().any(|op| matches!(op, effects::ImmediateOp::OppMillDeck(4))));

        let p_opp_flip = effects::parse_effect_steps("【ヒラメキ】相手の裏向きの証拠を1つまで選び、表向きにする。", None, models::CardType::CHARACTER);
        assert!(p_opp_flip.immediate_before.iter().any(|op| matches!(op, effects::ImmediateOp::OppFlipEvidenceUp(1))));
    }

    #[test]
    fn test_hint_use_bottom_deck_and_remove_revival() {
        let catalog = load_test_catalog();
        let mut engine = MatchEngine::new(catalog);
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let mut ps2 = engine::PlayerState::new("p2".to_string());
        // B01005 Partner: 【パートナー青】【ターン1】自分がネクストヒントで手札を使用したとき、そのカードのレベル以下のキャラを1枚まで選び、デッキの下に移す。
        ps1.partner = Some(test_inst("p1_p_b01005", "B01005", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
        // B01001 Partner or Blue? B01005 Partner isn't Blue unless Partner colors include 青 - wait B01005 itself is 青 char, check B01001 Partner vs trigger
        ps1.partner = Some(test_inst("p1_partner", "B01001", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
        ps1.field.push(test_inst("p1_watcher", "B01005", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        // Enemy field: Lv4 vs Lv8
        ps2.field.push(test_inst("opp_lv4", "B01007", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE)); // Check actual levels or make sure
        // Put Lv4 in remove for B02038 revival check too
        ps1.remove.push(test_inst("rem_lv4_w", "B02038", "p1", models::Zone::REMOVE, models::CardOrientation::ACTIVE));

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        // Trigger OnHintUse with hint card level 5: B01007(Lv5? Let's verify) vs Lv8
        let lv_b01007 = engine.catalog.get("B01007").unwrap().level;
        let triggered = engine.trigger_on_hint_use("p1", lv_b01007);
        assert!(triggered);
        let ts = engine.pending_decision.as_ref().unwrap().clone();
        assert_eq!(ts["options"]["effectType"], "BOTTOM_DECK");
        let ds_id = ts["decisionId"].as_str().unwrap().to_string();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": ds_id,
            "revision": engine.revision,
            "value": {"targetId": "opp_lv4", "actorId": "p1_watcher", "effectType": "BOTTOM_DECK"}
        }));

        assert_eq!(engine.players.get("p2").unwrap().field.len(), 0);
        assert_eq!(engine.players.get("p2").unwrap().deck.len(), 1);
    }

    #[test]
    fn test_id0102_small_detective_deck_search_then_file_level_hand_play() {
        let catalog = load_test_catalog();
        let mut engine = engine::MatchEngine::new(catalog);
        engine.phase = models::MatchPhase::MAIN;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let ps2 = engine::PlayerState::new("p2".to_string());

        ps1.r#case = Some(test_inst("p1_case", "B01002", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE)); // 青事件
        // FILEエリア3枚（よって手札登場の上限レベルは 3 以下）
        for i in 0..3 {
            ps1.file.push(test_inst(&format!("f{}", i), "B01021", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
        }

        // 山札上3枚（Vecの末尾がデッキトップ）: 青イベント(B01022), 緑キャラ(B01029), 青キャラ(B01021)
        ps1.deck.push(test_inst("d_base", "B01020", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        ps1.deck.push(test_inst("d_ev", "B01022", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        ps1.deck.push(test_inst("d_green", "B01029", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        ps1.deck.push(test_inst("d_blue", "B01021", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));

        // 手札: D01014 (小さくなった名探偵, ID0102), Lv3青キャラ(B01021), Lv5青キャラ(B01020), Lv5緑キャラ(B01029)
        ps1.hand.push(test_inst("h_ev", "D01014", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("h_b3", "B01021", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("h_b5", "B01020", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("h_g5", "B01029", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        // D01014 をイベントプレイ
        engine.prompt_main_action("p1");
        let pd1 = engine.pending_decision.as_ref().unwrap().clone();
        let d1_id = pd1["decisionId"].as_str().unwrap().to_string();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": d1_id,
            "revision": engine.revision,
            "value": {"type": "HAND_PLAY", "cardId": "h_ev"}
        }));

        // 1段階目: deckSearch (TO_HAND であり PLAY_ACTIVE ではないこと！)
        let pd_ds = engine.pending_decision.as_ref().unwrap().clone();
        assert_eq!(pd_ds["kind"], "deckSearch");
        assert_eq!(pd_ds["options"]["pickAction"], "TO_HAND");
        let revealed = pd_ds["options"]["revealedCards"].as_array().unwrap();
        assert_eq!(revealed.len(), 3);
        let s_blue = revealed.iter().find(|c| c["instanceId"] == "d_blue").unwrap()["selectable"].as_bool().unwrap();
        let s_green = revealed.iter().find(|c| c["instanceId"] == "d_green").unwrap()["selectable"].as_bool().unwrap();
        let s_ev = revealed.iter().find(|c| c["instanceId"] == "d_ev").unwrap()["selectable"].as_bool().unwrap();
        assert!(s_blue, "【青】キャラは選択可能");
        assert!(!s_green, "【緑】キャラは選択不可");
        assert!(!s_ev, "【青】イベントは選択不可");

        let ds_id = pd_ds["decisionId"].as_str().unwrap().to_string();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": ds_id,
            "revision": engine.revision,
            "value": {"chosenCardId": "d_blue", "actorId": "h_ev"}
        }));

        // d_blue は手札へ、残り2枚(d_green, d_ev)はデッキ下へ
        assert!(engine.players.get("p1").unwrap().hand.iter().any(|c| c.instance_id == "d_blue"));
        assert_eq!(engine.players.get("p1").unwrap().deck.len(), 3);

        // 2段階目: 続けて「手札から自分のFILEエリアの枚数(3)以下のレベルの【青】のキャラを1枚まで登場させる」が開く！
        let pd_ts = engine.pending_decision.as_ref().expect("followUpTarget で targetSelect が開くべき").clone();
        assert_eq!(pd_ts["kind"], "targetSelect");
        assert_eq!(pd_ts["options"]["effectType"], "PLAY_FROM_HAND");
        assert_eq!(pd_ts["options"]["targetZone"], "HAND");
        let cands = pd_ts["options"]["candidates"].as_array().unwrap();
        let cand_ids: Vec<&str> = cands.iter().filter_map(|c| c["instanceId"].as_str()).collect();
        assert!(cand_ids.contains(&"h_b3"), "Lv3青キャラは候補に含まれる");
        assert!(cand_ids.contains(&"d_blue"), "先ほど加えたLv3青キャラも候補に含まれる");
        assert!(!cand_ids.contains(&"h_b5"), "FILE3枚時にLv5青キャラは候補外");
        assert!(!cand_ids.contains(&"h_g5"), "緑キャラは候補外");

        let ts_id = pd_ts["decisionId"].as_str().unwrap().to_string();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": ts_id,
            "revision": engine.revision,
            "value": {"targetId": "h_b3", "actorId": "h_ev", "effectType": "PLAY_FROM_HAND"}
        }));

        assert!(engine.players.get("p1").unwrap().field.iter().any(|c| c.instance_id == "h_b3"));

        // 追加検証: FILE1枚 + パートナーアシスト(実効FILE2)・手札にキャラ0枚の状態でD01014を使い、デッキからLv2青キャラ(B01019)を加えてそのまま登場できること
        let mut engine2 = engine::MatchEngine::new(load_test_catalog());
        engine2.phase = models::MatchPhase::MAIN;
        engine2.turn_player_id = "p1".to_string();
        engine2.player_order = vec!["p1".to_string(), "p2".to_string()];
        let mut p1_s = engine::PlayerState::new("p1".to_string());
        let p2_s = engine::PlayerState::new("p2".to_string());
        p1_s.r#case = Some(test_inst("p1_case", "B01002", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
        p1_s.file.push(test_inst("f2_0", "B01021", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
        p1_s.partner = Some(test_inst("p1_prt", "B01001", "p1", models::Zone::FILE, models::CardOrientation::SLEEP)); // パートナーのアシスト中（実効FILE = 2）
        p1_s.deck.push(test_inst("d2_0", "B01020", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        p1_s.deck.push(test_inst("d2_1", "B01020", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        p1_s.deck.push(test_inst("d2_lv2", "B01019", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        p1_s.hand.push(test_inst("h2_ev", "D01014", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        engine2.players.insert("p1".to_string(), p1_s);
        engine2.players.insert("p2".to_string(), p2_s);

        engine2.prompt_main_action("p1");
        let pd2_1 = engine2.pending_decision.as_ref().unwrap().clone();
        let _ = engine2.submit_decision("p1", &serde_json::json!({
            "decisionId": pd2_1["decisionId"],
            "revision": engine2.revision,
            "value": {"type": "HAND_PLAY", "cardId": "h2_ev"}
        }));
        let pd2_ds = engine2.pending_decision.as_ref().unwrap().clone();
        assert_eq!(pd2_ds["kind"], "deckSearch");
        let _ = engine2.submit_decision("p1", &serde_json::json!({
            "decisionId": pd2_ds["decisionId"],
            "revision": engine2.revision,
            "value": {"chosenCardId": "d2_lv2", "actorId": "h2_ev"}
        }));
        let pd2_ts = engine2.pending_decision.as_ref().expect("FILE2でも手札に加えたLv2キャラが登場選択に出るべき").clone();
        assert_eq!(pd2_ts["kind"], "targetSelect");
        assert_eq!(pd2_ts["options"]["effectType"], "PLAY_FROM_HAND");
        let cands2 = pd2_ts["options"]["candidates"].as_array().unwrap();
        assert!(cands2.iter().any(|c| c["instanceId"] == "d2_lv2"), "手札に加えたLv2キャラが候補に含まれる");
        let _ = engine2.submit_decision("p1", &serde_json::json!({
            "decisionId": pd2_ts["decisionId"],
            "revision": engine2.revision,
            "value": {"targetId": "d2_lv2", "actorId": "h2_ev", "effectType": "PLAY_FROM_HAND"}
        }));
        assert!(engine2.players.get("p1").unwrap().field.iter().any(|c| c.instance_id == "d2_lv2"));
    }

    #[test]
    fn test_b03025_remainder_to_remove_and_follow_up_hand_play() {
        let catalog = load_test_catalog();
        let mut engine = engine::MatchEngine::new(catalog);
        engine.phase = models::MatchPhase::MAIN;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let ps2 = engine::PlayerState::new("p2".to_string());
        ps1.r#case = Some(test_inst("p1_case", "B01002", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
        for i in 0..6 {
            ps1.file.push(test_inst(&format!("f{}", i), "B01021", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
        }

        // 山札上5枚: 少年探偵団(B01021) x1 + 非探偵団(B01020) x4
        for i in 0..4 {
            ps1.deck.push(test_inst(&format!("d_other_{}", i), "B01020", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        }
        ps1.deck.push(test_inst("d_dt", "B01021", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));

        ps1.hand.push(test_inst("h_ev", "B03025", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("h_dt", "B01021", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        engine.prompt_main_action("p1");
        let pd1 = engine.pending_decision.as_ref().unwrap().clone();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd1["decisionId"],
            "revision": engine.revision,
            "value": {"type": "HAND_PLAY", "cardId": "h_ev"}
        }));

        let pd_ds = engine.pending_decision.as_ref().unwrap().clone();
        assert_eq!(pd_ds["kind"], "deckSearch");
        assert_eq!(pd_ds["options"]["pickAction"], "TO_HAND");
        assert_eq!(pd_ds["options"]["remainderToRemove"], true);

        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_ds["decisionId"],
            "revision": engine.revision,
            "value": {"chosenCardId": "d_dt", "actorId": "h_ev"}
        }));

        // 残り4枚はデッキ下ではなくリムーブエリアへ (B03025自身+残り4枚=5枚)
        let ps1_mid = engine.players.get("p1").unwrap();
        assert_eq!(ps1_mid.deck.len(), 0);
        assert_eq!(ps1_mid.remove.len(), 5);

        // 続けて手札から少年探偵団を登場させる targetSelect が開く
        let pd_ts = engine.pending_decision.as_ref().unwrap().clone();
        assert_eq!(pd_ts["kind"], "targetSelect");
        assert_eq!(pd_ts["options"]["effectType"], "PLAY_FROM_HAND");
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_ts["decisionId"],
            "revision": engine.revision,
            "value": {"targetId": "h_dt", "actorId": "h_ev", "effectType": "PLAY_FROM_HAND"}
        }));
        assert!(engine.players.get("p1").unwrap().field.iter().any(|c| c.instance_id == "h_dt"));
    }

    #[test]
    fn test_interactive_hand_discard_cost_optional_and_deck_search() {
        let catalog = load_test_catalog();
        let mut engine = engine::MatchEngine::new(catalog);
        engine.phase = models::MatchPhase::MAIN;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let ps2 = engine::PlayerState::new("p2".to_string());
        ps1.partner = Some(test_inst("p1_partner", "B01001", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE)); // 青
        ps1.r#case = Some(test_inst("p1_case", "B01002", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
        for i in 0..8 {
            ps1.file.push(test_inst(&format!("f{}", i), "B01021", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
        }

        // 1) B01007 (【パートナー青】【宣言】〚手札を1枚リムーブする〛：AP+1000 & 突撃)
        ps1.field.push(test_inst("f_ran", "B01007", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("h_keep", "B01021", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("h_drop", "B01020", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        engine.prompt_main_action("p1");
        let pd1 = engine.pending_decision.as_ref().unwrap().clone();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd1["decisionId"],
            "revision": engine.revision,
            "value": {"type": "DECLARE", "actorId": "f_ran"}
        }));

        // コストの手札1枚をプレイヤーが選べること
        let pd_cost = engine.pending_decision.as_ref().unwrap().clone();
        assert_eq!(pd_cost["kind"], "targetSelect");
        assert_eq!(pd_cost["options"]["effectType"], "DISCARD_HAND");
        assert_eq!(pd_cost["options"]["optional"], false);
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_cost["decisionId"],
            "revision": engine.revision,
            "value": {"targetId": "h_drop", "actorId": "f_ran", "effectType": "DISCARD_HAND"}
        }));

        let ps1_after_decl = engine.players.get("p1").unwrap();
        assert!(ps1_after_decl.hand.iter().any(|c| c.instance_id == "h_keep"));
        assert!(ps1_after_decl.remove.iter().any(|c| c.instance_id == "h_drop"));
        let ran = ps1_after_decl.field.iter().find(|c| c.instance_id == "f_ran").unwrap();
        assert_eq!(ran.temp_ap_bonus, 1000);
        assert!(ran.has_rush);

        // 2) PR041 (【登場時】手札から【緑】のイベントを1枚リムーブしてもよい。そうした場合、カードを2枚引く。)
        {
            let ps1_mut = engine.players.get_mut("p1").unwrap();
            ps1_mut.hand.push(test_inst("h_pr041", "PR041", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
            ps1_mut.hand.push(test_inst("h_green_ev", "B04026", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE)); // 緑イベント
            ps1_mut.deck.push(test_inst("d1", "B01021", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
            ps1_mut.deck.push(test_inst("d2", "B01021", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        }
        let pd2 = engine.pending_decision.as_ref().unwrap().clone();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd2["decisionId"],
            "revision": engine.revision,
            "value": {"type": "HAND_PLAY", "cardId": "h_pr041"}
        }));

        let pd_opt = engine.pending_decision.as_ref().unwrap().clone();
        assert_eq!(pd_opt["kind"], "targetSelect");
        assert_eq!(pd_opt["options"]["effectType"], "DISCARD_HAND");
        assert_eq!(pd_opt["options"]["optional"], true);
        let opt_cands = pd_opt["options"]["candidates"].as_array().unwrap();
        let opt_ids: Vec<&str> = opt_cands.iter().filter_map(|c| c["instanceId"].as_str()).collect();
        assert!(opt_ids.contains(&"h_green_ev"), "【緑】イベントは候補に含まれる");
        assert!(!opt_ids.contains(&"h_keep"), "【青】キャラは候補に含まれない");

        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_opt["decisionId"],
            "revision": engine.revision,
            "value": {"targetId": "h_green_ev", "actorId": "h_pr041", "effectType": "DISCARD_HAND"}
        }));
        let ps1_after_pr = engine.players.get("p1").unwrap();
        assert!(ps1_after_pr.remove.iter().any(|c| c.instance_id == "h_green_ev"));
        // h_keep(1) + d1 + d2 = 3枚
        assert_eq!(ps1_after_pr.hand.len(), 3);

        // 3) B01016 (【登場時】デッキ4枚見て【青】手札に加え、加えた場合手札を1枚リムーブ)
        {
            let ps1_mut = engine.players.get_mut("p1").unwrap();
            ps1_mut.normal_hand_played_this_turn = false;
            ps1_mut.hand.push(test_inst("h_b01016", "B01016", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
            for i in 0..4 {
                ps1_mut.deck.push(test_inst(&format!("ds_{}", i), "B01021", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
            }
        }
        let pd3 = engine.pending_decision.as_ref().unwrap().clone();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd3["decisionId"],
            "revision": engine.revision,
            "value": {"type": "HAND_PLAY", "cardId": "h_b01016"}
        }));
        let pd_ds = engine.pending_decision.as_ref().unwrap().clone();
        assert_eq!(pd_ds["kind"], "deckSearch");
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_ds["decisionId"],
            "revision": engine.revision,
            "value": {"chosenCardId": "ds_0", "actorId": "h_b01016"}
        }));

        // 手札に加えたあと、捨てる手札をプレイヤーが選べる targetSelect (DISCARD_HAND) が開くこと！
        let pd_post_disc = engine.pending_decision.as_ref().unwrap().clone();
        assert_eq!(pd_post_disc["kind"], "targetSelect");
        assert_eq!(pd_post_disc["options"]["effectType"], "DISCARD_HAND");
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_post_disc["decisionId"],
            "revision": engine.revision,
            "value": {"targetId": "ds_0", "actorId": "h_b01016", "effectType": "DISCARD_HAND"}
        }));
        assert!(engine.players.get("p1").unwrap().remove.iter().any(|c| c.instance_id == "ds_0"));
    }

    #[test]
    fn test_contact_disguise_and_cutin_limit() {
        let mut catalog = CardCatalog::load_from_json(std::path::Path::new("../data/dummy/cards.json"))
            .expect("catalog load failed");

        catalog.definitions.insert("T_BASE".to_string(), models::CardDefinition {
            definition_id: "T_BASE".to_string(),
            canonical_id: "T_BASE".to_string(),
            card_type: models::CardType::CHARACTER,
            colors: vec!["青".to_string()],
            level: 2,
            ap: 2000,
            lp: 1,
            names: vec!["ベースキャラ".to_string()],
            description: String::new(),
            traits: vec![],
            case_levels: None,
            main_thumb: None,
            main_path: None,
        });
        catalog.definitions.insert("T_DISG".to_string(), models::CardDefinition {
            definition_id: "T_DISG".to_string(),
            canonical_id: "T_DISG".to_string(),
            card_type: models::CardType::CHARACTER,
            colors: vec!["青".to_string()],
            level: 2,
            ap: 5000,
            lp: 1,
            names: vec!["変装キャラ".to_string()],
            description: "【変装】自分のキャラ1人と入れ替える。".to_string(),
            traits: vec![],
            case_levels: None,
            main_thumb: None,
            main_path: None,
        });
        catalog.definitions.insert("T_CUTIN".to_string(), models::CardDefinition {
            definition_id: "T_CUTIN".to_string(),
            canonical_id: "T_CUTIN".to_string(),
            card_type: models::CardType::CHARACTER,
            colors: vec!["青".to_string()],
            level: 1,
            ap: 1000,
            lp: 1,
            names: vec!["カットインキャラ".to_string()],
            description: "【カットイン】自分のキャラ1人のAP+1000。".to_string(),
            traits: vec![],
            case_levels: None,
            main_thumb: None,
            main_path: None,
        });

        let mut engine = MatchEngine::new(catalog);
        engine.turn = 3;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];
        engine.phase = models::MatchPhase::MAIN;

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let mut ps2 = engine::PlayerState::new("p2".to_string());

        let mk = |id: &str, def: &str, owner: &str, zone: models::Zone, ori: models::CardOrientation| {
            models::CardInstance {
                instance_id: id.to_string(),
                definition_id: def.to_string(),
                owner_id: owner.to_string(),
                zone,
                orientation: ori,
                entered_turn: 1,
                face_up: true,
                temp_ap_bonus: 0,
                has_rush: false,
                rush_target: None,
                used_declare_this_turn: false,
                ..Default::default()
            }
        };

        for i in 0..5 {
            ps1.file.push(mk(&format!("f1_{}", i), "T_BASE", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
            ps2.file.push(mk(&format!("f2_{}", i), "T_BASE", "p2", models::Zone::FILE, models::CardOrientation::ACTIVE));
        }
        ps2.deck.push(mk("d2_top", "T_BASE", "p2", models::Zone::DECK, models::CardOrientation::ACTIVE));

        ps1.field.push(mk("p1_atk", "T_BASE", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps2.field.push(mk("p2_def", "T_BASE", "p2", models::Zone::FIELD, models::CardOrientation::SLEEP));
        ps2.field.push(mk("p2_guard", "T_BASE", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps2.hand.push(mk("p2_disg", "T_DISG", "p2", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps2.hand.push(mk("p2_cutin", "T_CUTIN", "p2", models::Zone::HAND, models::CardOrientation::ACTIVE));

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        // 1. MAINフェイズにはDISGUISEが含まれない
        engine.prompt_main_action("p1");
        let pd_main = engine.pending_decision.clone().unwrap();
        let actions = pd_main["options"]["actions"].as_array().unwrap();
        assert!(!actions.iter().any(|a| a["type"] == "DISGUISE"));

        // 2. p1がp2_defへコンタクト
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_main["decisionId"],
            "revision": engine.revision,
            "value": {"type": "ACTION", "actorId": "p1_atk", "targetId": "p2_def"}
        }));

        // 3. まずp2のガードフェーズ（ガードをスルー）
        let pd_guard = engine.pending_decision.clone().unwrap();
        assert_eq!(pd_guard["kind"], "guard");
        assert_eq!(pd_guard["playerId"], "p2");
        let _ = engine.submit_decision("p2", &serde_json::json!({
            "decisionId": pd_guard["decisionId"],
            "revision": engine.revision,
            "value": {}
        }));

        // 4. カットインフェーズでp2にdisguiseOptionsとcutinCardIdsの両方が提示される
        let pd_cutin = engine.pending_decision.clone().unwrap();
        assert_eq!(pd_cutin["kind"], "guard");
        assert_eq!(pd_cutin["playerId"], "p2");
        assert_eq!(pd_cutin["options"]["disguiseTargetId"], "p2_def");
        assert!(!pd_cutin["options"]["disguiseOptions"].as_array().unwrap().is_empty());
        assert!(!pd_cutin["options"]["cutinCardIds"].as_array().unwrap().is_empty());

        // 5. p2が変装を使用
        let _ = engine.submit_decision("p2", &serde_json::json!({
            "decisionId": pd_cutin["decisionId"],
            "revision": engine.revision,
            "value": {"disguiseCardId": "p2_disg", "targetId": "p2_def"}
        }));

        // p2_defはデッキの一番下(index 0)へ移動し、p2_disgが場に出る
        let ps2_after = engine.players.get("p2").unwrap();
        assert_eq!(ps2_after.deck[0].instance_id, "p2_def");
        assert!(ps2_after.field.iter().any(|c| c.instance_id == "p2_disg"));

        // 6. カットイン・変装は各プレイヤー1回までのため、p1にカットインがなければバトル解決してmainActionへ戻る
        let pd_after = engine.pending_decision.clone().unwrap();
        assert_eq!(pd_after["kind"], "mainAction");
    }

    #[test]
    fn test_b05005_next_hint_blue_trigger_and_independent_turn1_declare() {
        let mut catalog = load_test_catalog();
        // Ensure a known non-blue opponent character (AP 8000) and a non-blue card in hand exist
        let b05005_lv = catalog.get("B05005").unwrap().level;
        assert!(b05005_lv == 9);
        catalog.definitions.insert(
            "TEST_NONBLUE_8000".to_string(),
            models::CardDefinition {
                definition_id: "TEST_NONBLUE_8000".to_string(),
                canonical_id: "TEST_NONBLUE_8000".to_string(),
                card_type: models::CardType::CHARACTER,
                colors: vec!["緑".to_string()],
                level: 7,
                ap: 8000,
                lp: 1,
                names: vec!["テスト緑8000".to_string()],
                description: String::new(),
                traits: vec![],
                case_levels: None,
                main_thumb: None,
                main_path: None,
            },
        );
        catalog.definitions.insert(
            "TEST_NONBLUE_LOW".to_string(),
            models::CardDefinition {
                definition_id: "TEST_NONBLUE_LOW".to_string(),
                canonical_id: "TEST_NONBLUE_LOW".to_string(),
                card_type: models::CardType::CHARACTER,
                colors: vec!["青".to_string()], // Blue only for playing via Blue case, separate test for non-blue below
                level: 2,
                ap: 2000,
                lp: 1,
                names: vec!["テスト青2".to_string()],
                description: String::new(),
                traits: vec![],
                case_levels: None,
                main_thumb: None,
                main_path: None,
            },
        );
        // Check color matching directly: green hint must NOT match B05005's 【青】のカードを使用したとき
        let b05005_desc = catalog.get("B05005").unwrap().description.clone();
        let hint_clauses = effects::extract_clauses(&b05005_desc, effects::TriggerKind::OnHintUse, models::CardType::CHARACTER);
        assert_eq!(hint_clauses.len(), 1);
        assert!(!effects::check_hint_use_match(&hint_clauses[0], &["緑".to_string()]));
        assert!(effects::check_hint_use_match(&hint_clauses[0], &["青".to_string()]));

        let mut engine = MatchEngine::new(catalog);
        engine.turn = 4;
        engine.phase = models::MatchPhase::MAIN;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let mut ps2 = engine::PlayerState::new("p2".to_string());
        ps1.r#case = Some(test_inst("p1_case", "B01002", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
        ps1.partner = Some(test_inst("p1_partner", "B01001", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
        ps1.field.push(test_inst("p1_b05005", "B05005", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        for i in 0..5 {
            ps1.file.push(test_inst(&format!("f1_{}", i), "B01021", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
        }
        ps1.hand.push(test_inst("h_blue_1", "TEST_NONBLUE_LOW", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("h_blue_2", "TEST_NONBLUE_LOW", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));

        ps2.field.push(test_inst("opp_g8000", "TEST_NONBLUE_8000", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps2.deck.push(test_inst("opp_dtop", "TEST_NONBLUE_LOW", "p2", models::Zone::DECK, models::CardOrientation::ACTIVE));

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        // 1. ネクストヒントでFILEから手札に加え、【青】カード(TEST_NONBLUE_LOW)を使用
        engine.prompt_main_action("p1");
        let pd1 = engine.pending_decision.as_ref().unwrap().clone();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd1["decisionId"],
            "revision": engine.revision,
            "value": {"type": "HINT_TAKE"}
        }));
        let pd_hint = engine.pending_decision.as_ref().unwrap().clone();
        assert_eq!(pd_hint["kind"], "hintPlay");
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_hint["decisionId"],
            "revision": engine.revision,
            "value": {"cardId": "h_blue_1"}
        }));

        // 2. B05005の【パートナー青】【ターン1】が誘発し、AP8000以下の非青キャラ(opp_g8000)もデッキボトム対象として選べる！
        let pd_ts = engine.pending_decision.as_ref().expect("B05005 OnHintUse targetSelect must open");
        assert_eq!(pd_ts["kind"], "targetSelect");
        assert_eq!(pd_ts["options"]["effectType"], "BOTTOM_DECK");
        let candidates = pd_ts["options"]["candidates"].as_array().unwrap();
        assert!(candidates.iter().any(|c| c["instanceId"] == "opp_g8000"));
        let ts_id = pd_ts["decisionId"].as_str().unwrap().to_string();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": ts_id,
            "revision": engine.revision,
            "value": {"targetId": "opp_g8000", "actorId": "p1_b05005", "effectType": "BOTTOM_DECK"}
        }));

        let ps2_after = engine.players.get("p2").unwrap();
        assert!(ps2_after.field.is_empty());
        assert_eq!(ps2_after.deck[0].instance_id, "opp_g8000");

        // 3. 同じターン内でも B05005 の【宣言】【ターン1】はまだ未使用であるため、DECLARE候補に残る（共有バグが直っていること）
        let pd_main2 = engine.pending_decision.as_ref().unwrap();
        let actions2 = pd_main2["options"]["actions"].as_array().unwrap();
        let decl_opt = actions2.iter().find(|a| a["type"] == "DECLARE").expect("DECLARE action must still be available on B05005");
        let decl_actors = decl_opt["actors"].as_array().unwrap();
        assert!(decl_actors.iter().any(|a| a["actorId"] == "p1_b05005"));

        // 4. 同じターンにもう一度青ヒント使用を呼び出しても【ターン1】により再発動しない
        assert!(!engine.trigger_on_hint_use_with_colors("p1", 2, &["青".to_string()]));
    }

    #[test]
    fn test_passive_aura_ap_and_partner_lp_effects() {
        let mut catalog = load_test_catalog();
        catalog.definitions.insert(
            "TEST_TAKAGI_GREEN".to_string(),
            models::CardDefinition {
                definition_id: "TEST_TAKAGI_GREEN".to_string(),
                canonical_id: "TEST_TAKAGI_GREEN".to_string(),
                card_type: models::CardType::CHARACTER,
                colors: vec!["黄".to_string()],
                level: 4,
                ap: 4000,
                lp: 1,
                names: vec!["高木渉".to_string()],
                description: String::new(),
                traits: vec!["警察".to_string(), "警視庁".to_string()],
                case_levels: None,
                main_thumb: None,
                main_path: None,
            },
        );

        let mut engine = MatchEngine::new(catalog);
        engine.turn = 4;
        engine.phase = models::MatchPhase::MAIN;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let ps2 = engine::PlayerState::new("p2".to_string());
        // 緑パートナー D02001
        ps1.partner = Some(test_inst("p1_p", "D02001", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
        // B01038 (自分ターン中、このキャラ以外の【緑】キャラAP+1000)
        // B01032 (緑 Lv3 base AP=3000)
        // B01027 (緑 Lv7 base LP=0, 【パートナー緑】LP+1)
        // B02012 (自分ターン中、自分の現場にいるレベル5以下の【青】キャラをAP+1000, 自身Lv7 base AP=6000)
        // B01011 (青 Lv4 base AP=2000)
        ps1.field.push(test_inst("c_b01038", "B01038", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.field.push(test_inst("c_b01032", "B01032", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.field.push(test_inst("c_b01027", "B01027", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.field.push(test_inst("c_b02012", "B02012", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.field.push(test_inst("c_b01011", "B01011", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.deck.push(test_inst("p1_d1", "B01021", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        {
            let ps1_ref = engine.players.get("p1").unwrap();
            let c38 = &ps1_ref.field[0];
            let c32 = &ps1_ref.field[1];
            let c27 = &ps1_ref.field[2];
            let c2012 = &ps1_ref.field[3];
            let c11 = &ps1_ref.field[4];

            let b01038_base = engine.catalog.get("B01038").unwrap().ap;
            assert_eq!(engine.get_effective_ap("p1", c38), b01038_base); // 「このキャラ以外の」なので自身は増えない
            assert_eq!(engine.get_effective_ap("p1", c32), 4000); // 3000 + B01038(+1000)
            assert_eq!(engine.get_effective_ap("p1", c27), 6000); // 5000 + B01038(+1000)
            assert_eq!(engine.get_effective_ap("p1", c2012), 6000); // Lv7なのでB02012のLv5以下バフは対象外
            assert_eq!(engine.get_effective_ap("p1", c11), 3000); // Lv4青: 2000 + B02012(+1000)

            assert_eq!(engine.get_effective_lp("p1", c27), 1); // 【パートナー緑】LP+1 (base 0 -> 1)
        }

        // B01027 で DEDUCE を実行すると LP 1 分の証拠を獲得する
        engine.prompt_main_action("p1");
        let pd1 = engine.pending_decision.as_ref().unwrap().clone();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd1["decisionId"],
            "revision": engine.revision,
            "value": {"type": "DEDUCE", "actorId": "c_b01027"}
        }));
        assert_eq!(engine.players.get("p1").unwrap().evidence.len(), 1);

        // 相手ターンになると【自分ターン中】オーラが消える
        engine.turn_player_id = "p2".to_string();
        let ps1_ref = engine.players.get("p1").unwrap();
        assert_eq!(engine.get_effective_ap("p1", &ps1_ref.field[1]), 3000);
        assert_eq!(engine.get_effective_ap("p1", &ps1_ref.field[4]), 2000);

        // パートナーエリア有効能力 (B08062: すべてが佐藤美和子/高木渉なら全味方AP+1000)
        engine.turn_player_id = "p1".to_string();
        {
            let ps1_mut = engine.players.get_mut("p1").unwrap();
            ps1_mut.partner = Some(test_inst("p1_b08062", "B08062", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
            ps1_mut.field = vec![test_inst("c_takagi", "TEST_TAKAGI_GREEN", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE)];
        }
        let ps1_ref2 = engine.players.get("p1").unwrap();
        assert_eq!(engine.get_effective_ap("p1", &ps1_ref2.field[0]), 5000); // 4000 + B08062 in Partner(+1000)
    }

    #[test]
    fn test_on_action_variants_char_case_and_deduce() {
        let catalog = load_test_catalog();
        let mut engine = MatchEngine::new(catalog);
        engine.turn = 4;
        engine.phase = models::MatchPhase::MAIN;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let mut ps2 = engine::PlayerState::new("p2".to_string());
        // 赤パートナー B01060
        ps1.partner = Some(test_inst("p1_p", "B01060", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
        ps2.r#case = Some(test_inst("p2_case", "B01002", "p2", models::Zone::CASE, models::CardOrientation::ACTIVE));
        ps2.evidence.push(test_inst("p2_ev1", "B01021", "p2", models::Zone::EVIDENCE, models::CardOrientation::ACTIVE));
        ps2.field.push(test_inst("opp_sleep", "B01021", "p2", models::Zone::FIELD, models::CardOrientation::SLEEP)); // Lv3 < Lv7
        ps2.field.push(test_inst("opp_guard", "B01021", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE)); // ACTIVEなのでガード権あり
        ps1.deck.push(test_inst("d1", "B01021", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));

        // B01032: 【ターン1】このキャラがアクション［キャラ］したときAP+1000
        // B01062: 【パートナー赤】【ターン1】自分の現場にいるキャラがアクション［事件］したときLv7以下リムーブ ＋ 【宣言】【ターン1】別枠
        ps1.field.push(test_inst("c_b01032", "B01032", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.field.push(test_inst("c_b01062", "B01062", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        // 1. B01032 が相手キャラ (opp_sleep) にアクション［キャラ］ -> B01032 APが +1000 される (3000 -> 4000)
        //    B01062 の アクション［事件］トリガーは発動しない
        engine.prompt_main_action("p1");
        let pd1 = engine.pending_decision.as_ref().unwrap().clone();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd1["decisionId"],
            "revision": engine.revision,
            "value": {"type": "ACTION", "actorId": "c_b01032", "targetId": "opp_sleep"}
        }));
        let pd_guard = engine.pending_decision.as_ref().unwrap().clone();
        assert_eq!(pd_guard["kind"], "guard");
        assert_eq!(engine.players.get("p1").unwrap().field[0].temp_ap_bonus, 1000);

        // ガード/カットインをパスして解決へ
        for _ in 0..4 {
            let pd = match engine.pending_decision.as_ref() {
                Some(d) if d["kind"] == "guard" => d.clone(),
                _ => break,
            };
            let pid = pd["playerId"].as_str().unwrap().to_string();
            let _ = engine.submit_decision(&pid, &serde_json::json!({
                "decisionId": pd["decisionId"],
                "revision": engine.revision,
                "value": {"pass": true}
            }));
        }

        // 2. 今度は c_b01032 をアクティブに戻し、別のLv4青(B01011)を出してから相手事件へアクション［事件］
        {
            let ps1_m = engine.players.get_mut("p1").unwrap();
            ps1_m.field.push(test_inst("c_atk_case", "B01011", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
            ps1_m.field.last_mut().unwrap().entered_turn = 1;
            let ps2_m = engine.players.get_mut("p2").unwrap();
            ps2_m.field.push(test_inst("opp_lv3", "B01021", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        }
        engine.prompt_main_action("p1");
        let pd2 = engine.pending_decision.as_ref().unwrap().clone();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd2["decisionId"],
            "revision": engine.revision,
            "value": {"type": "ACTION", "actorId": "c_atk_case", "targetId": "p2_case"}
        }));

        // B01062 の【パートナー赤】【ターン1】が誘発し、targetSelect (REMOVE) が開く！
        let pd_ts = engine.pending_decision.as_ref().expect("B01062 OnAction(Case) must prompt targetSelect");
        assert_eq!(pd_ts["kind"], "targetSelect");
        assert_eq!(pd_ts["options"]["effectType"], "REMOVE");
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_ts["decisionId"],
            "revision": engine.revision,
            "value": {"targetId": "opp_lv3", "actorId": "c_b01062", "effectType": "REMOVE"}
        }));

        // opp_lv3 はリムーブされ、そのまま事件アタックのカットイン/ガード確認へ進む
        assert!(engine.players.get("p2").unwrap().field.iter().all(|c| c.instance_id != "opp_lv3"));
        assert_eq!(engine.pending_decision.as_ref().unwrap()["kind"], "guard");
    }

    #[test]
    fn test_on_ally_play_triggers_b11003_and_d02005() {
        let catalog = load_test_catalog();
        let mut engine = MatchEngine::new(catalog);
        engine.turn = 4;
        engine.phase = models::MatchPhase::MAIN;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let mut ps2 = engine::PlayerState::new("p2".to_string());
        ps1.partner = Some(test_inst("p1_p", "B01001", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
        ps1.r#case = Some(test_inst("p1_case", "B01002", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
        for i in 0..5 {
            ps1.file.push(test_inst(&format!("f{}", i), "B01021", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
        }
        // B11003: 【パートナー青】【自分ターン中】【ターン1】レベル4の〚特徴［少年探偵団］〛のキャラが自分の現場に登場したとき、AP8000以下のキャラを1枚まで選び、リムーブする。
        ps1.field.push(test_inst("c_b11003", "B11003", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("h_b01011", "B01011", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE)); // Lv4 青 少年探偵団
        ps2.field.push(test_inst("opp_target", "B01021", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE));

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        engine.prompt_main_action("p1");
        let pd1 = engine.pending_decision.as_ref().unwrap().clone();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd1["decisionId"],
            "revision": engine.revision,
            "value": {"type": "HAND_PLAY", "cardId": "h_b01011"}
        }));

        // B11003 の登場時ウォッチャー効果が発火し targetSelect (REMOVE) になる
        let pd_ts = engine.pending_decision.as_ref().expect("B11003 OnAllyPlay must prompt targetSelect");
        assert_eq!(pd_ts["kind"], "targetSelect", "logs: {:?}", engine.logs);
        assert_eq!(pd_ts["options"]["effectType"], "REMOVE");
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_ts["decisionId"],
            "revision": engine.revision,
            "value": {"targetId": "opp_target", "actorId": "c_b11003", "effectType": "REMOVE"}
        }));
        assert!(engine.players.get("p2").unwrap().field.is_empty());
        assert_eq!(engine.players.get("p2").unwrap().remove.len(), 1);

        // 後半: D02005 自身が登場したときの「このキャラか〚カード名［服部平次］〛が自分の現場に登場したとき、キャラを1枚まで選び、スリープさせる。」も実戦フローで発火する
        {
            engine.catalog.definitions.insert(
                "TEST_CASE_GREEN".to_string(),
                models::CardDefinition {
                    definition_id: "TEST_CASE_GREEN".to_string(),
                    canonical_id: "TEST_CASE_GREEN".to_string(),
                    card_type: models::CardType::CASE,
                    colors: vec!["緑".to_string()],
                    level: 0,
                    ap: 0,
                    lp: 0,
                    names: vec!["テスト緑事件".to_string()],
                    description: String::new(),
                    traits: vec![],
                    case_levels: None,
                    main_thumb: None,
                    main_path: None,
                },
            );
            let d02005_lv = engine.catalog.get("D02005").unwrap().level;
            let ps1_m = engine.players.get_mut("p1").unwrap();
            ps1_m.normal_hand_played_this_turn = false;
            for i in 0..d02005_lv {
                if (ps1_m.file.len() as i32) < i {
                    ps1_m.file.push(test_inst(&format!("f_ext_{}", i), "B01021", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
                }
            }
            ps1_m.hand.push(test_inst("h_d02005", "D02005", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
            ps1_m.r#case = Some(test_inst("p1_case_g", "TEST_CASE_GREEN", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
            engine.players.get_mut("p2").unwrap().field.push(test_inst("opp_act", "B01021", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        }
        engine.prompt_main_action("p1");
        let pd2 = engine.pending_decision.as_ref().unwrap().clone();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd2["decisionId"],
            "revision": engine.revision,
            "value": {"type": "HAND_PLAY", "cardId": "h_d02005"}
        }));
        let pd_ts2 = engine.pending_decision.as_ref().expect("D02005 self play must prompt SLEEP targetSelect");
        assert_eq!(pd_ts2["kind"], "targetSelect");
        assert_eq!(pd_ts2["options"]["effectType"], "SLEEP");
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_ts2["decisionId"],
            "revision": engine.revision,
            "value": {"targetId": "opp_act", "actorId": "h_d02005", "effectType": "SLEEP"}
        }));
        assert_eq!(engine.players.get("p2").unwrap().field[0].orientation, models::CardOrientation::SLEEP);
    }

    #[test]
    fn test_ally_play_trigger_queue_keeps_all_consecutive_entries() {
        let catalog = load_test_catalog();
        let mut engine = MatchEngine::new(catalog);
        engine.turn = 4;
        engine.phase = models::MatchPhase::MAIN;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let ps2 = engine::PlayerState::new("p2".to_string());
        ps1.partner = Some(test_inst("p1_p", "B01001", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
        ps1.r#case = Some(test_inst("p1_case", "B01002", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
        for i in 0..6 {
            ps1.deck.push(test_inst(&format!("d{}", i), "B01021", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
        }
        ps1.field.push(test_inst("c_b11003", "B11003", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.field.push(test_inst("ai", "B01016", "p1", models::Zone::FIELD, models::CardOrientation::SLEEP));
        ps1.field.push(test_inst("ayumi", "B01011", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        // 五人囃子のように連続して2体登場させる（1体目の登場時効果で選択待ちになっても2体目の誘発を失わない）
        let _ = engine.resolve_character_enter_field_with_source("p1", "ai", "B01016", models::CardOrientation::SLEEP, Some(0), "MAIN");
        let _ = engine.resolve_character_enter_field_with_source("p1", "ayumi", "B01011", models::CardOrientation::ACTIVE, Some(0), "MAIN");
        assert_eq!(engine.pending_ally_play.len(), 2, "連続登場した2体分の登場時誘発待ちが保持されること");
    }

    #[test]
    fn test_case_attack_guard_then_cutin_both_sides_and_case_resolve() {
        let mut catalog = load_test_catalog();
        catalog.definitions.insert("T_AP4000_BLUE".to_string(), models::CardDefinition {
            definition_id: "T_AP4000_BLUE".to_string(),
            canonical_id: "T_AP4000_BLUE".to_string(),
            card_type: models::CardType::CHARACTER,
            colors: vec!["青".to_string()],
            level: 4,
            ap: 4000,
            lp: 1,
            names: vec!["アタッカー4000".to_string()],
            description: String::new(),
            traits: vec![],
            case_levels: None,
            main_thumb: None,
            main_path: None,
        });
        catalog.definitions.insert("T_CUTIN_P1000_BLUE".to_string(), models::CardDefinition {
            definition_id: "T_CUTIN_P1000_BLUE".to_string(),
            canonical_id: "T_CUTIN_P1000_BLUE".to_string(),
            card_type: models::CardType::CHARACTER,
            colors: vec!["青".to_string()],
            level: 1,
            ap: 1000,
            lp: 1,
            names: vec!["カットイン青1000".to_string()],
            description: "【カットイン】自分のキャラ1人のAP+1000。".to_string(),
            traits: vec![],
            case_levels: None,
            main_thumb: None,
            main_path: None,
        });

        let mut engine = MatchEngine::new(catalog);
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];
        engine.phase = models::MatchPhase::MAIN;

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let mut ps2 = engine::PlayerState::new("p2".to_string());
        ps1.r#case = Some(test_inst("p1_case", "B01002", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
        ps2.r#case = Some(test_inst("p2_case", "B01002", "p2", models::Zone::CASE, models::CardOrientation::ACTIVE));
        for i in 0..5 {
            ps1.file.push(test_inst(&format!("f1_{}", i), "T_AP4000_BLUE", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
            ps2.file.push(test_inst(&format!("f2_{}", i), "T_AP4000_BLUE", "p2", models::Zone::FILE, models::CardOrientation::ACTIVE));
        }
        for i in 0..3 {
            ps1.deck.push(test_inst(&format!("d1_{}", i), "T_AP4000_BLUE", "p1", models::Zone::DECK, models::CardOrientation::ACTIVE));
            ps2.deck.push(test_inst(&format!("d2_{}", i), "T_AP4000_BLUE", "p2", models::Zone::DECK, models::CardOrientation::ACTIVE));
        }
        ps2.evidence.push(test_inst("ev2_0", "T_AP4000_BLUE", "p2", models::Zone::EVIDENCE, models::CardOrientation::ACTIVE));
        ps1.field.push(test_inst("p1_atk", "T_AP4000_BLUE", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps2.field.push(test_inst("p2_guard", "T_AP4000_BLUE", "p2", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps1.hand.push(test_inst("p1_cutin", "T_CUTIN_P1000_BLUE", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps2.hand.push(test_inst("p2_cutin", "T_CUTIN_P1000_BLUE", "p2", models::Zone::HAND, models::CardOrientation::ACTIVE));

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        // 1. p1がp2の事件にアタック
        engine.prompt_main_action("p1");
        let pd_main = engine.pending_decision.clone().unwrap();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_main["decisionId"],
            "revision": engine.revision,
            "value": {"type": "ACTION", "actorId": "p1_atk", "targetId": "p2_case"}
        }));

        // 2. p2がp2_guardでガードを宣言
        let pd_guard = engine.pending_decision.clone().unwrap();
        assert_eq!(pd_guard["kind"], "guard");
        assert_eq!(pd_guard["playerId"], "p2");
        let _ = engine.submit_decision("p2", &serde_json::json!({
            "decisionId": pd_guard["decisionId"],
            "revision": engine.revision,
            "value": {"guardCardId": "p2_guard"}
        }));

        // 3. ガードによるコンタクト発生後、APが低い方（同APなら攻撃側p1）からカットインフェーズへ移行する
        let pd_cut_atk = engine.pending_decision.clone().expect("attacker contactCutin should be prompted first when AP <= defender");
        assert_eq!(pd_cut_atk["kind"], "guard");
        assert_eq!(pd_cut_atk["playerId"], "p1");
        assert!(pd_cut_atk["options"]["guardCardIds"].as_array().unwrap().is_empty());
        assert!(pd_cut_atk["options"]["cutinCardIds"].as_array().unwrap().iter().any(|v| v["instanceId"] == "p1_cutin"));
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_cut_atk["decisionId"],
            "revision": engine.revision,
            "value": {}
        }));

        // 4. p1がパスしたら、防御側p2のカットインフェーズに移る（即座にバトル解決しない）
        assert_eq!(engine.players.get("p2").unwrap().remove.len(), 0);
        let pd_cut_def = engine.pending_decision.clone().expect("defender contactCutin should be prompted after attacker pass");
        assert_eq!(pd_cut_def["kind"], "guard");
        assert_eq!(pd_cut_def["playerId"], "p2");
        assert!(pd_cut_def["options"]["guardCardIds"].as_array().unwrap().is_empty());
        assert!(pd_cut_def["options"]["cutinCardIds"].as_array().unwrap().iter().any(|v| v["instanceId"] == "p2_cutin"));

        // 5. 攻撃側p1がガードされた事件アタック中にカットインを使用できる
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_cut_atk["decisionId"],
            "revision": engine.revision,
            "value": {"cutinCardId": "p1_cutin"}
        }));
        if let Some(pd_after) = engine.pending_decision.clone() {
            if pd_after["kind"] == "targetSelect" {
                let _ = engine.submit_decision("p1", &serde_json::json!({
                    "decisionId": pd_after["decisionId"],
                    "revision": engine.revision,
                    "value": {"targetId": "p1_atk", "actorId": "p1_cutin", "effectType": "AP_BUFF"}
                }));
            }
        }

        // 6. p1がカットイン使用後、まだ未使用のp2に1度チャンスが回り、p2がパスして解決する
        if let Some(pd_cut_def2) = engine.pending_decision.clone() {
            if pd_cut_def2["kind"] == "guard" {
                assert_eq!(pd_cut_def2["playerId"], "p2");
                let _ = engine.submit_decision("p2", &serde_json::json!({
                    "decisionId": pd_cut_def2["decisionId"],
                    "revision": engine.revision,
                    "value": {}
                }));
            }
        }

        // p1_atkがAP5000でp2_guard(AP4000)に勝利し、p2_guardがリムーブされ、p1_atkは場に残る
        assert!(engine.players.get("p1").unwrap().field.iter().any(|c| c.instance_id == "p1_atk"));
        assert!(engine.players.get("p2").unwrap().remove.iter().any(|c| c.instance_id == "p2_guard"));
        assert!(!engine.players.get("p2").unwrap().field.iter().any(|c| c.instance_id == "p2_guard"));
    }

    #[test]
    fn test_char_attack_equal_ap_defender_pass_prompts_attacker_cutin() {
        let mut catalog = load_test_catalog();
        catalog.definitions.insert("T_EQ_4000".to_string(), models::CardDefinition {
            definition_id: "T_EQ_4000".to_string(),
            canonical_id: "T_EQ_4000".to_string(),
            card_type: models::CardType::CHARACTER,
            colors: vec!["青".to_string()],
            level: 4,
            ap: 4000,
            lp: 1,
            names: vec!["同APキャラ".to_string()],
            description: String::new(),
            traits: vec![],
            case_levels: None,
            main_thumb: None,
            main_path: None,
        });
        catalog.definitions.insert("T_EQ_CUTIN".to_string(), models::CardDefinition {
            definition_id: "T_EQ_CUTIN".to_string(),
            canonical_id: "T_EQ_CUTIN".to_string(),
            card_type: models::CardType::CHARACTER,
            colors: vec!["青".to_string()],
            level: 1,
            ap: 1000,
            lp: 1,
            names: vec!["同AP突破カットイン".to_string()],
            description: "【カットイン】自分のキャラ1人のAP+1000。".to_string(),
            traits: vec![],
            case_levels: None,
            main_thumb: None,
            main_path: None,
        });

        let mut engine = MatchEngine::new(catalog);
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];
        engine.phase = models::MatchPhase::MAIN;

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let mut ps2 = engine::PlayerState::new("p2".to_string());
        ps1.r#case = Some(test_inst("p1_case", "B01002", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
        ps2.r#case = Some(test_inst("p2_case", "B01002", "p2", models::Zone::CASE, models::CardOrientation::ACTIVE));
        for i in 0..5 {
            ps1.file.push(test_inst(&format!("f1_{}", i), "T_EQ_4000", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
            ps2.file.push(test_inst(&format!("f2_{}", i), "T_EQ_4000", "p2", models::Zone::FILE, models::CardOrientation::ACTIVE));
        }
        ps1.field.push(test_inst("p1_atk", "T_EQ_4000", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        ps2.field.push(test_inst("p2_def", "T_EQ_4000", "p2", models::Zone::FIELD, models::CardOrientation::SLEEP));
        ps1.hand.push(test_inst("p1_cutin", "T_EQ_CUTIN", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps2.hand.push(test_inst("p2_cutin", "T_EQ_CUTIN", "p2", models::Zone::HAND, models::CardOrientation::ACTIVE));

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        // 1. p1が同APのp2_defにアタック
        engine.prompt_main_action("p1");
        let pd_main = engine.pending_decision.clone().unwrap();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_main["decisionId"],
            "revision": engine.revision,
            "value": {"type": "ACTION", "actorId": "p1_atk", "targetId": "p2_def"}
        }));

        // 2. p2がカットイン/ガードを使わずにパス
        let pd_guard = engine.pending_decision.clone().unwrap();
        assert_eq!(pd_guard["kind"], "guard");
        let _ = engine.submit_decision("p2", &serde_json::json!({
            "decisionId": pd_guard["decisionId"],
            "revision": engine.revision,
            "value": {}
        }));

        // 3. p2_defはまだリムーブされず、p1のカットインフェーズへ移る
        assert_eq!(engine.players.get("p2").unwrap().remove.len(), 0);
        let pd_cut_atk = engine.pending_decision.clone().expect("attacker contactCutin must be prompted");
        assert_eq!(pd_cut_atk["kind"], "guard");
        assert_eq!(pd_cut_atk["playerId"], "p1");
        assert!(pd_cut_atk["options"]["guardCardIds"].as_array().unwrap().is_empty());
        assert!(pd_cut_atk["options"]["cutinCardIds"].as_array().unwrap().iter().any(|v| v["instanceId"] == "p1_cutin"));

        // 4. p1がカットインを使用し、p2がパスして決着
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_cut_atk["decisionId"],
            "revision": engine.revision,
            "value": {"cutinCardId": "p1_cutin"}
        }));
        if let Some(pd_after) = engine.pending_decision.clone() {
            if pd_after["kind"] == "targetSelect" {
                let _ = engine.submit_decision("p1", &serde_json::json!({
                    "decisionId": pd_after["decisionId"],
                    "revision": engine.revision,
                    "value": {"targetId": "p1_atk", "actorId": "p1_cutin", "effectType": "AP_BUFF"}
                }));
            }
        }
        if let Some(pd_cut_def2) = engine.pending_decision.clone() {
            if pd_cut_def2["kind"] == "guard" {
                let _ = engine.submit_decision("p2", &serde_json::json!({
                    "decisionId": pd_cut_def2["decisionId"],
                    "revision": engine.revision,
                    "value": {}
                }));
            }
        }

        assert!(engine.players.get("p1").unwrap().field.iter().any(|c| c.instance_id == "p1_atk"));
        assert!(engine.players.get("p2").unwrap().remove.iter().any(|c| c.instance_id == "p2_def"));
    }

    #[test]
    fn test_rush_character_cannot_deduce_on_entered_turn() {
        let mut catalog = load_test_catalog();
        catalog.definitions.insert("T_RUSH_CHAR".to_string(), models::CardDefinition {
            definition_id: "T_RUSH_CHAR".to_string(),
            canonical_id: "T_RUSH_CHAR".to_string(),
            card_type: models::CardType::CHARACTER,
            colors: vec!["黄".to_string()],
            level: 4,
            ap: 4000,
            lp: 1,
            names: vec!["突撃テストキャラ".to_string()],
            description: "【登場時】ターン終了時までこのキャラは〚突撃［キャラ］〛を持つ。".to_string(),
            traits: vec![],
            case_levels: None,
            main_thumb: None,
            main_path: None,
        });

        let mut engine = MatchEngine::new(catalog);
        engine.turn = 3;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];
        engine.phase = models::MatchPhase::MAIN;

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let mut ps2 = engine::PlayerState::new("p2".to_string());
        let mut rush_c = test_inst("p1_rush", "T_RUSH_CHAR", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE);
        rush_c.entered_turn = 3;
        rush_c.has_rush = true;
        rush_c.rush_target = Some("CHAR".to_string());
        ps1.field.push(rush_c);
        ps2.field.push(test_inst("p2_sleep", "T_RUSH_CHAR", "p2", models::Zone::FIELD, models::CardOrientation::SLEEP));

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        engine.prompt_main_action("p1");
        let pd = engine.pending_decision.as_ref().unwrap();
        let actions = pd["options"]["actions"].as_array().unwrap();

        // 登場ターンは突撃を持っていても DEDUCE は不可
        let deduce_actors: Vec<&str> = actions
            .iter()
            .find(|a| a["type"] == "DEDUCE")
            .and_then(|a| a["actorIds"].as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        assert!(!deduce_actors.contains(&"p1_rush"), "Rush character must not be able to DEDUCE on entered turn");

        // 登場ターンでも ACTION（アタック）は可能
        let action_actors: Vec<&str> = actions
            .iter()
            .find(|a| a["type"] == "ACTION")
            .and_then(|a| a["actorIds"].as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        assert!(action_actors.contains(&"p1_rush"), "Rush character should be able to ACTION on entered turn");
    }

    #[test]
    fn test_b11017_gonin_bayashi_effect() {
        let catalog = load_test_catalog();
        assert!(catalog.definitions.contains_key("B11017"), "B11017 must exist in catalog");

        let mut engine = MatchEngine::new(catalog);
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];
        engine.phase = models::MatchPhase::MAIN;

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let ps2 = engine::PlayerState::new("p2".to_string());
        ps1.partner = Some(test_inst("p1_partner", "B01001", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
        ps1.r#case = Some(test_inst("p1_case", "B01002", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
        for i in 0..8 {
            ps1.file.push(test_inst(&format!("f{}", i), "B01021", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
        }

        ps1.hand.push(test_inst("h_b11017", "B11017", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps1.remove.push(test_inst("r_conan1", "B01011", "p1", models::Zone::REMOVE, models::CardOrientation::ACTIVE)); // Lv4 江戸川コナン (少年探偵団)
        ps1.remove.push(test_inst("r_conan2", "B01011", "p1", models::Zone::REMOVE, models::CardOrientation::ACTIVE)); // 同名 Lv4 江戸川コナン
        ps1.remove.push(test_inst("r_genta", "B01014", "p1", models::Zone::REMOVE, models::CardOrientation::ACTIVE));  // Lv4 小嶋元太 (少年探偵団)
        ps1.remove.push(test_inst("r_ai", "B01016", "p1", models::Zone::REMOVE, models::CardOrientation::ACTIVE));     // Lv4 灰原哀 (少年探偵団)
        ps1.remove.push(test_inst("r_other", "B01005", "p1", models::Zone::REMOVE, models::CardOrientation::ACTIVE));  // Lv5 非対象

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        engine.prompt_main_action("p1");
        let pd1 = engine.pending_decision.as_ref().unwrap().clone();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": pd1["decisionId"],
            "revision": engine.revision,
            "value": {"type": "HAND_PLAY", "cardId": "h_b11017"}
        }));

        let pd_ts = engine.pending_decision.as_ref().expect("B11017 must prompt targetSelect");
        assert_eq!(pd_ts["kind"], "targetSelect");
        assert_eq!(pd_ts["options"]["effectType"], "REVIVE_SPLIT_GONIN");
        assert_eq!(pd_ts["options"]["maxSelect"], 3);
        assert_eq!(pd_ts["options"]["distinctNames"], true);
        let cand_ids: Vec<&str> = pd_ts["options"]["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|c| c["instanceId"].as_str())
            .collect();
        assert!(!cand_ids.contains(&"r_other"));
        assert!(cand_ids.contains(&"r_conan1"));
        assert!(cand_ids.contains(&"r_genta"));
        assert!(cand_ids.contains(&"r_ai"));

        let ts_dec_id = pd_ts["decisionId"].as_str().unwrap().to_string();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": ts_dec_id,
            "revision": engine.revision,
            "value": {
                "targetIds": ["r_conan1", "r_genta", "r_ai"],
                "actorId": "h_b11017",
                "effectType": "REVIVE_SPLIT_GONIN"
            }
        }));

        let ps1_after = engine.players.get("p1").unwrap();
        let conan = ps1_after.field.iter().find(|c| c.instance_id == "r_conan1").expect("1st pick must deploy to field");
        assert_eq!(conan.orientation, models::CardOrientation::ACTIVE);
        let genta = ps1_after.field.iter().find(|c| c.instance_id == "r_genta").expect("2nd pick must deploy to field in SLEEP");
        assert_eq!(genta.orientation, models::CardOrientation::SLEEP);
        assert!(ps1_after.hand.iter().any(|c| c.instance_id == "r_ai"), "3rd pick must go to hand");
        assert!(ps1_after.remove.iter().any(|c| c.instance_id == "h_b11017"));
    }

    #[test]
    fn test_all_event_cards_have_valid_effects() {
        let catalog = load_test_catalog();
        let events: Vec<&models::CardDefinition> = catalog
            .definitions
            .values()
            .filter(|d| d.card_type == models::CardType::EVENT)
            .collect();
        assert!(events.len() >= 200, "Expected >= 200 event cards, got {}", events.len());

        let mut unparsed: Vec<String> = Vec::new();
        for ev in &events {
            if effects::parse_event_mode_choice(&ev.description).is_some() {
                continue;
            }
            let on_play_clauses = effects::extract_clauses(&ev.description, effects::TriggerKind::OnPlay, models::CardType::EVENT);
            let mut has_exec = false;
            for cl in &on_play_clauses {
                let p = effects::parse_effect_steps(cl, Some(ev.level), models::CardType::EVENT);
                if !p.immediate_before.is_empty()
                    || p.target_step.is_some()
                    || p.deck_search.is_some()
                    || p.follow_up_target.is_some()
                    || !p.extra_clauses.is_empty()
                {
                    has_exec = true;
                    break;
                }
            }
            if !has_exec {
                let decl = effects::extract_clauses(&ev.description, effects::TriggerKind::Declare, models::CardType::EVENT);
                let cutin = effects::extract_clauses(&ev.description, effects::TriggerKind::Cutin, models::CardType::EVENT);
                let hirameki = effects::extract_clauses(&ev.description, effects::TriggerKind::Hirameki, models::CardType::EVENT);
                if decl.is_empty() && cutin.is_empty() && hirameki.is_empty() {
                    unparsed.push(format!("{}: {}", ev.definition_id, ev.description));
                }
            }
        }
        assert!(
            unparsed.is_empty(),
            "Unparsed event cards ({}):\n{}",
            unparsed.len(),
            unparsed.join("\n")
        );

        // Runtime smoke 1: Mode choice event B04014 (最後の切り札)
        let mut engine_mode = MatchEngine::new(catalog.clone());
        engine_mode.turn = 4;
        engine_mode.turn_player_id = "p1".to_string();
        engine_mode.player_order = vec!["p1".to_string(), "p2".to_string()];
        engine_mode.phase = models::MatchPhase::MAIN;
        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let mut ps2 = engine::PlayerState::new("p2".to_string());
        ps1.partner = Some(test_inst("p1_p", "B01001", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
        ps1.r#case = Some(test_inst("p1_c", "B01002", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
        ps1.case_phase = models::CasePhase::RESOLUTION;
        for i in 0..8 {
            ps1.file.push(test_inst(&format!("mf{}", i), "B01021", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
        }
        ps1.hand.push(test_inst("h_mode", "B04014", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        ps2.field.push(test_inst("opp_c", "B01005", "p2", models::Zone::FIELD, models::CardOrientation::SLEEP));
        engine_mode.players.insert("p1".to_string(), ps1);
        engine_mode.players.insert("p2".to_string(), ps2);
        engine_mode.prompt_main_action("p1");
        let pd_m = engine_mode.pending_decision.as_ref().unwrap().clone();
        let _ = engine_mode.submit_decision("p1", &serde_json::json!({
            "decisionId": pd_m["decisionId"],
            "revision": engine_mode.revision,
            "value": {"type": "HAND_PLAY", "cardId": "h_mode"}
        }));
        let pd_choice = engine_mode.pending_decision.as_ref().expect("B04014 should prompt modeSelect");
        assert_eq!(pd_choice["kind"], "modeSelect");

        // Runtime smoke 2: Set-card event B02013 (ターボエンジン付きスケートボード)
        let mut engine_set = MatchEngine::new(catalog);
        engine_set.turn = 4;
        engine_set.turn_player_id = "p1".to_string();
        engine_set.player_order = vec!["p1".to_string(), "p2".to_string()];
        engine_set.phase = models::MatchPhase::MAIN;
        let mut s_ps1 = engine::PlayerState::new("p1".to_string());
        let s_ps2 = engine::PlayerState::new("p2".to_string());
        s_ps1.partner = Some(test_inst("sp1_p", "B01001", "p1", models::Zone::PARTNER, models::CardOrientation::ACTIVE));
        s_ps1.r#case = Some(test_inst("sp1_c", "B01002", "p1", models::Zone::CASE, models::CardOrientation::ACTIVE));
        for i in 0..8 {
            s_ps1.file.push(test_inst(&format!("sf{}", i), "B01021", "p1", models::Zone::FILE, models::CardOrientation::ACTIVE));
        }
        s_ps1.hand.push(test_inst("h_skate", "B02013", "p1", models::Zone::HAND, models::CardOrientation::ACTIVE));
        s_ps1.field.push(test_inst("p1_conan", "B01011", "p1", models::Zone::FIELD, models::CardOrientation::ACTIVE));
        engine_set.players.insert("p1".to_string(), s_ps1);
        engine_set.players.insert("p2".to_string(), s_ps2);
        engine_set.prompt_main_action("p1");
        let spd = engine_set.pending_decision.as_ref().unwrap().clone();
        let _ = engine_set.submit_decision("p1", &serde_json::json!({
            "decisionId": spd["decisionId"],
            "revision": engine_set.revision,
            "value": {"type": "HAND_PLAY", "cardId": "h_skate"}
        }));
        let sts = engine_set.pending_decision.as_ref().expect("B02013 should prompt targetSelect to attach set card");
        assert_eq!(sts["kind"], "targetSelect");
        let _ = engine_set.submit_decision("p1", &serde_json::json!({
            "decisionId": sts["decisionId"],
            "revision": engine_set.revision,
            "value": {"targetId": "p1_conan", "actorId": "h_skate", "effectType": sts["options"]["effectType"]}
        }));
        let conan_inst = engine_set.players.get("p1").unwrap().field.iter().find(|c| c.instance_id == "p1_conan").unwrap();
        assert_eq!(conan_inst.set_cards.len(), 1);
        assert!(engine_set.effective_rush(conan_inst).0, "Skateboard set card must grant Rush");
    }

