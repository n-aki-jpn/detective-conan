use super::*;

    #[test]
    fn test_canonical_id_validation() {
        let path = if std::path::Path::new("data/dummy/cards.json").exists() {
            "data/dummy/cards.json"
        } else {
            "../data/dummy/cards.json"
        };
        let catalog = CardCatalog::load_from_json(path).expect("catalog load failed");
        
        let card1 = catalog.get("B01005").expect("B01005 should exist");
        let card2 = catalog.get("B01005P").expect("B01005P should exist");
        assert_eq!(card1.canonical_id, "0001");
        assert_eq!(card2.canonical_id, "0001");

        // Helper to validate deck items
        let validate_deck_counts = |items: &[DeckCardItem]| -> Result<(), String> {
            let mut canonical_counts: HashMap<String, i32> = HashMap::new();
            for item in items {
                let card_def = catalog.get(&item.definition_id);
                let is_unlimited = card_def
                    .map(|d| d.description.contains("デッキに何枚でも入れることができる"))
                    .unwrap_or(false);
                if is_unlimited {
                    continue;
                }
                let canonical_id = card_def
                    .map(|d| d.canonical_id.clone())
                    .unwrap_or_else(|| item.definition_id.clone());

                let count = canonical_counts.entry(canonical_id.clone()).or_insert(0);
                *count += item.count;
                if *count > 3 {
                    return Err(format!("同一カードID（{}）はパラレル版も含めて合計3枚までです（現在: {}枚）", canonical_id, *count));
                }
            }
            Ok(())
        };

        // 2 + 2 = 4 (Same canonical ID) -> should fail
        let invalid_items = vec![
            DeckCardItem { definition_id: "B01005".to_string(), count: 2 },
            DeckCardItem { definition_id: "B01005P".to_string(), count: 2 },
        ];
        assert!(validate_deck_counts(&invalid_items).is_err());

        // 0627 (犯人) allows unlimited copies -> 10 copies should succeed
        if let Some(hannin_def) = catalog.definitions.values().find(|d| d.canonical_id == "0627") {
            let unlimited_items = vec![
                DeckCardItem { definition_id: hannin_def.definition_id.clone(), count: 10 },
            ];
            assert!(validate_deck_counts(&unlimited_items).is_ok());
        }

        // 2 + 1 = 3 (Same canonical ID) -> should succeed
        let valid_items = vec![
            DeckCardItem { definition_id: "B01005".to_string(), count: 2 },
            DeckCardItem { definition_id: "B01005P".to_string(), count: 1 },
            DeckCardItem { definition_id: "B01006".to_string(), count: 3 },
        ];
        assert!(validate_deck_counts(&valid_items).is_ok());
    }

    #[test]
    fn test_color_matching_hand_play() {
        let path = if std::path::Path::new("data/dummy/cards.json").exists() {
            "data/dummy/cards.json"
        } else {
            "../data/dummy/cards.json"
        };
        let catalog = CardCatalog::load_from_json(path).expect("catalog load failed");

        // Verify case card B05120P (all colors or 5 colors)
        if let Some(case_card) = catalog.get("B05120P") {
            assert!(case_card.colors.len() >= 5 || case_card.colors.contains(&"青".to_string()));
        }

        // Verify character card B01005 (blue)
        let blue_card = catalog.get("B01005").expect("B01005 should exist");
        assert!(blue_card.colors.contains(&"青".to_string()));
    }

    #[test]
    fn test_color_code_resolution() {
        assert_eq!(resolve_color_code(&["青".to_string()]), "B");
        assert_eq!(resolve_color_code(&["緑".to_string()]), "G");
        assert_eq!(resolve_color_code(&["白".to_string()]), "W");
        assert_eq!(resolve_color_code(&["赤".to_string()]), "R");
        assert_eq!(resolve_color_code(&["黄".to_string()]), "Y");
        assert_eq!(resolve_color_code(&["黒".to_string()]), "K");
        assert_eq!(resolve_color_code(&["青".to_string(), "緑".to_string()]), "BG");
    }

    #[test]
    fn test_effect_condition_and_cost_checking() {
        let path = if std::path::Path::new("data/dummy/cards.json").exists() {
            "data/dummy/cards.json"
        } else {
            "../data/dummy/cards.json"
        };
        let catalog = CardCatalog::load_from_json(path).expect("catalog load failed");
        let mut engine = MatchEngine::new(catalog);

        // Setup mock player with Blue partner and 4 files
        let mut p1 = models::CardInstance {
            instance_id: "c1".to_string(),
            definition_id: "B01001".to_string(), // Partner Conan (Blue)
            owner_id: "p1".to_string(),
            zone: models::Zone::PARTNER,
            orientation: models::CardOrientation::ACTIVE,
            entered_turn: 0,
            face_up: true,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
            ..Default::default()
        };
        let mut ps = engine::PlayerState::new("p1".to_string());
        ps.partner = Some(p1);
        for i in 0..4 {
            ps.file.push(models::CardInstance {
                instance_id: format!("f{}", i),
                definition_id: "B01005".to_string(),
                owner_id: "p1".to_string(),
                zone: models::Zone::FILE,
                orientation: models::CardOrientation::ACTIVE,
                entered_turn: 0,
                face_up: false,
                temp_ap_bonus: 0,
                has_rush: false,
                rush_target: None,
                used_declare_this_turn: false,
                ..Default::default()
            });
        }
        engine.players.insert("p1".to_string(), ps);

        // Partner color condition check
        assert!(engine.check_effect_conditions("p1", "【パートナー青】効果テキスト"));
        assert!(!engine.check_effect_conditions("p1", "【パートナー赤】効果テキスト"));

        // FILE condition check (currently 4 files)
        assert!(engine.check_effect_conditions("p1", "【FILE4】効果テキスト"));
        assert!(!engine.check_effect_conditions("p1", "【FILE5】効果テキスト"));

        // Add 1 more file -> now 5 files
        engine.players.get_mut("p1").unwrap().file.push(models::CardInstance {
            instance_id: "f4".to_string(),
            definition_id: "B01005".to_string(),
            owner_id: "p1".to_string(),
            zone: models::Zone::FILE,
            orientation: models::CardOrientation::ACTIVE,
            entered_turn: 0,
            face_up: false,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
            ..Default::default()
        });
        assert!(engine.check_effect_conditions("p1", "【FILE5】効果テキスト"));
    }

    #[test]
    fn test_on_remove_and_turn_end_triggers() {
        let path = if std::path::Path::new("data/dummy/cards.json").exists() {
            "data/dummy/cards.json"
        } else {
            "../data/dummy/cards.json"
        };
        let catalog = CardCatalog::load_from_json(path).expect("catalog load failed");
        let mut engine = MatchEngine::new(catalog);

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let mut ps2 = engine::PlayerState::new("p2".to_string());

        // Give p1 a card in deck
        ps1.deck.push(models::CardInstance {
            instance_id: "d1".to_string(),
            definition_id: "B01005".to_string(),
            owner_id: "p1".to_string(),
            zone: models::Zone::DECK,
            orientation: models::CardOrientation::ACTIVE,
            entered_turn: 0,
            face_up: false,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
            ..Default::default()
        });

        // Field card with turn end trigger
        ps1.field.push(models::CardInstance {
            instance_id: "f1".to_string(),
            definition_id: "B02088".to_string(), // 自分か相手のターン終了時、自分の証拠を上から1つリムーブする
            owner_id: "p1".to_string(),
            zone: models::Zone::FIELD,
            orientation: models::CardOrientation::ACTIVE,
            entered_turn: 0,
            face_up: true,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
            ..Default::default()
        });
        // 1 evidence
        ps1.evidence.push(models::CardInstance {
            instance_id: "ev1".to_string(),
            definition_id: "B01005".to_string(),
            owner_id: "p1".to_string(),
            zone: models::Zone::EVIDENCE,
            orientation: models::CardOrientation::ACTIVE,
            entered_turn: 0,
            face_up: true,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
            ..Default::default()
        });

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);
        engine.turn_player_id = "p2".to_string(); // p2's turn, so p1 is in opponent's turn

        // Test on remove trigger on card D03013 (【相手ターン中】【現場リムーブ時】カードを1枚引く。)
        assert_eq!(engine.players.get("p1").unwrap().hand.len(), 0);
        engine.trigger_on_remove("p1", "D03013");
        assert_eq!(engine.players.get("p1").unwrap().hand.len(), 1);

        // Test turn end trigger on B02088
        assert_eq!(engine.players.get("p1").unwrap().evidence.len(), 1);
        engine.trigger_on_turn_end("p1");
        assert_eq!(engine.players.get("p1").unwrap().evidence.len(), 0);
        assert_eq!(engine.players.get("p1").unwrap().remove.len(), 1);
    }

    #[test]
    fn test_trait_condition_rush_restriction_and_contact() {
        let path = if std::path::Path::new("data/dummy/cards.json").exists() {
            "data/dummy/cards.json"
        } else {
            "../data/dummy/cards.json"
        };
        let mut catalog = CardCatalog::load_from_json(path).expect("catalog load failed");

        // Insert synthetic test definitions for deterministic verification
        catalog.definitions.insert(
            "TEST_TRAIT_DETECTIVE".to_string(),
            models::CardDefinition {
                definition_id: "TEST_TRAIT_DETECTIVE".to_string(),
                canonical_id: "TEST_TRAIT_DETECTIVE".to_string(),
                card_type: models::CardType::CHARACTER,
                colors: vec!["青".to_string()],
                level: 2,
                ap: 3000,
                lp: 1,
                names: vec!["テスト探偵団".to_string()],
                description: "このキャラがコンタクトしたとき、AP＋1000する。\n相手の現場にいるキャラがこのキャラとのコンタクトによってリムーブされたとき、カードを1枚引く。".to_string(),
                traits: vec!["少年探偵団".to_string()],
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
        let mut ps2 = engine::PlayerState::new("p2".to_string());

        // Initially no 少年探偵団 on p1's field
        ps1.deck.push(models::CardInstance {
            instance_id: "p1_d1".to_string(),
            definition_id: "B01005".to_string(),
            owner_id: "p1".to_string(),
            zone: models::Zone::DECK,
            orientation: models::CardOrientation::ACTIVE,
            entered_turn: 0,
            face_up: false,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
            ..Default::default()
        });

        // p2 has 1 SLEEP character, 1 case, and 1 evidence
        ps2.field.push(models::CardInstance {
            instance_id: "opp_c1".to_string(),
            definition_id: "B01005".to_string(),
            owner_id: "p2".to_string(),
            zone: models::Zone::FIELD,
            orientation: models::CardOrientation::SLEEP,
            entered_turn: 1,
            face_up: true,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
            ..Default::default()
        });
        ps2.r#case = Some(models::CardInstance {
            instance_id: "opp_case".to_string(),
            definition_id: "B01002".to_string(),
            owner_id: "p2".to_string(),
            zone: models::Zone::CASE,
            orientation: models::CardOrientation::ACTIVE,
            entered_turn: 0,
            face_up: true,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
            ..Default::default()
        });
        ps2.evidence.push(models::CardInstance {
            instance_id: "opp_ev1".to_string(),
            definition_id: "B01005".to_string(),
            owner_id: "p2".to_string(),
            zone: models::Zone::EVIDENCE,
            orientation: models::CardOrientation::ACTIVE,
            entered_turn: 0,
            face_up: false,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
            ..Default::default()
        });

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        // 1. Trait condition check (before adding 少年探偵団)
        let trait_cond = "自分の現場に〚特徴［少年探偵団］〛のキャラがいる場合、カードを1枚引く。";
        assert!(!engine.check_effect_conditions("p1", trait_cond));

        // Add character with 少年探偵団 trait & 突撃［キャラ］
        engine.players.get_mut("p1").unwrap().field.push(models::CardInstance {
            instance_id: "p1_c1".to_string(),
            definition_id: "TEST_TRAIT_DETECTIVE".to_string(),
            owner_id: "p1".to_string(),
            zone: models::Zone::FIELD,
            orientation: models::CardOrientation::ACTIVE,
            entered_turn: 3, // entered this turn
            face_up: true,
            temp_ap_bonus: 0,
            has_rush: true,
            rush_target: Some("CHAR".to_string()),
            used_declare_this_turn: false,
            ..Default::default()
        });
        assert!(engine.check_effect_conditions("p1", trait_cond));

        // 2. Rush［キャラ］ target restriction check via prompt_main_action
        engine.prompt_main_action("p1");
        let pd = engine.pending_decision.as_ref().unwrap();
        let actions = pd["options"]["actions"].as_array().unwrap();
        let action_opt = actions.iter().find(|a| a["type"] == "ACTION").unwrap();
        let valid_targets = action_opt["actorTargetMap"]["p1_c1"].as_array().unwrap();
        assert_eq!(valid_targets.len(), 1);
        assert_eq!(valid_targets[0], "opp_c1"); // Case "opp_case" must be excluded

        // 3. Contact trigger & contact win trigger
        engine.trigger_on_contact("p1", "p1_c1", "p2", "opp_c1");
        let bonus = engine.players.get("p1").unwrap().field[0].temp_ap_bonus;
        assert_eq!(bonus, 1000);

        assert_eq!(engine.players.get("p1").unwrap().hand.len(), 0);
        engine.trigger_on_contact_win("p1", "p1_c1", "TEST_TRAIT_DETECTIVE");
        assert_eq!(engine.players.get("p1").unwrap().hand.len(), 1);
    }

    #[test]
    fn test_cutin_target_passive_ap_and_disguise() {
        let path = if std::path::Path::new("data/dummy/cards.json").exists() {
            "data/dummy/cards.json"
        } else {
            "../data/dummy/cards.json"
        };
        let mut catalog = CardCatalog::load_from_json(path).expect("catalog load failed");

        catalog.definitions.insert(
            "TEST_PASSIVE_OPP_TURN".to_string(),
            models::CardDefinition {
                definition_id: "TEST_PASSIVE_OPP_TURN".to_string(),
                canonical_id: "TEST_PASSIVE_OPP_TURN".to_string(),
                card_type: models::CardType::CHARACTER,
                colors: vec!["青".to_string()],
                level: 3,
                ap: 3000,
                lp: 1,
                names: vec!["守備キャラ".to_string()],
                description: "【相手ターン中】AP＋1000".to_string(),
                traits: vec![],
                case_levels: None,
                main_thumb: None,
                main_path: None,
            },
        );
        catalog.definitions.insert(
            "TEST_CUTIN_SELECT".to_string(),
            models::CardDefinition {
                definition_id: "TEST_CUTIN_SELECT".to_string(),
                canonical_id: "TEST_CUTIN_SELECT".to_string(),
                card_type: models::CardType::CHARACTER,
                colors: vec!["青".to_string()],
                level: 2,
                ap: 2000,
                lp: 1,
                names: vec!["カットイン選択".to_string()],
                description: "【カットイン】相手の現場にいるキャラを1枚まで選び、スリープさせる。".to_string(),
                traits: vec![],
                case_levels: None,
                main_thumb: None,
                main_path: None,
            },
        );
        catalog.definitions.insert(
            "TEST_DISGUISE_KID".to_string(),
            models::CardDefinition {
                definition_id: "TEST_DISGUISE_KID".to_string(),
                canonical_id: "TEST_DISGUISE_KID".to_string(),
                card_type: models::CardType::CHARACTER,
                colors: vec!["白".to_string()],
                level: 4,
                ap: 4000,
                lp: 1,
                names: vec!["テストキッド".to_string()],
                description: "【変装時】カードを1枚引く。このキャラをAP＋1000する。".to_string(),
                traits: vec![],
                case_levels: None,
                main_thumb: None,
                main_path: None,
            },
        );

        let mut engine = MatchEngine::new(catalog);
        engine.turn = 2;
        engine.turn_player_id = "p2".to_string(); // p2's turn -> opponent turn for p1
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let mut ps2 = engine::PlayerState::new("p2".to_string());

        let passive_card = models::CardInstance {
            instance_id: "p1_def".to_string(),
            definition_id: "TEST_PASSIVE_OPP_TURN".to_string(),
            owner_id: "p1".to_string(),
            zone: models::Zone::FIELD,
            orientation: models::CardOrientation::ACTIVE,
            entered_turn: 1,
            face_up: true,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
            ..Default::default()
        };
        ps1.field.push(passive_card.clone());
        ps1.hand.push(models::CardInstance {
            instance_id: "p1_cutin".to_string(),
            definition_id: "TEST_CUTIN_SELECT".to_string(),
            owner_id: "p1".to_string(),
            zone: models::Zone::HAND,
            orientation: models::CardOrientation::ACTIVE,
            entered_turn: 0,
            face_up: true,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
            ..Default::default()
        });
        ps1.deck.push(models::CardInstance {
            instance_id: "p1_dk1".to_string(),
            definition_id: "B01005".to_string(),
            owner_id: "p1".to_string(),
            zone: models::Zone::DECK,
            orientation: models::CardOrientation::ACTIVE,
            entered_turn: 0,
            face_up: false,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
            ..Default::default()
        });
        // バッファ山札（山札が0枚になった瞬間のリフレッシュでこのテスト本体の効果が干渉されないよう余裕を持たせる）
        for i in 0..3 {
            ps1.deck.insert(0, models::CardInstance {
                instance_id: format!("p1_buf_{}", i),
                definition_id: "B01005".to_string(),
                owner_id: "p1".to_string(),
                zone: models::Zone::DECK,
                orientation: models::CardOrientation::ACTIVE,
                entered_turn: 0,
                face_up: false,
                ..Default::default()
            });
        }

        ps2.field.push(models::CardInstance {
            instance_id: "p2_atk".to_string(),
            definition_id: "B01005".to_string(),
            owner_id: "p2".to_string(),
            zone: models::Zone::FIELD,
            orientation: models::CardOrientation::ACTIVE,
            entered_turn: 1,
            face_up: true,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
            ..Default::default()
        });

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        // 1. Passive AP check: opponent turn -> 3000 + 1000 = 4000
        assert_eq!(engine.get_effective_ap("p1", &passive_card), 4000);
        // Switch turn to p1 -> own turn -> 3000
        engine.turn_player_id = "p1".to_string();
        assert_eq!(engine.get_effective_ap("p1", &passive_card), 3000);
        engine.turn_player_id = "p2".to_string();

        // 2. Cut-in target select check
        engine.action_context = Some(serde_json::json!({
            "actorId": "p2_atk",
            "targetId": "p1_def",
            "attackerPlayerId": "p2",
            "defenderPlayerId": "p1"
        }));
        let dec_id = engine.generate_decision_id();
        engine.pending_decision = Some(serde_json::json!({
            "decisionId": dec_id,
            "revision": engine.revision,
            "playerId": "p1",
            "kind": "guard",
            "options": {"guardCardIds": [], "cutinCardIds": ["p1_cutin"]}
        }));
        engine.submit_decision("p1", &serde_json::json!({
            "decisionId": dec_id,
            "revision": engine.revision,
            "value": {"cutinCardId": "p1_cutin"}
        }));
        let pd = engine.pending_decision.as_ref().unwrap();
        assert_eq!(pd["kind"], "targetSelect");
        let ts_dec_id = pd["decisionId"].as_str().unwrap().to_string();
        engine.submit_decision("p1", &serde_json::json!({
            "decisionId": ts_dec_id,
            "revision": engine.revision,
            "value": {"targetId": "p2_atk", "actorId": "p1_cutin", "effectType": "SLEEP"}
        }));
        assert_eq!(engine.players.get("p2").unwrap().field[0].orientation, models::CardOrientation::SLEEP);
        assert_eq!(engine.pending_decision.as_ref().unwrap()["kind"], "guard");

        // 3. Disguise trigger check
        engine.players.get_mut("p1").unwrap().field.push(models::CardInstance {
            instance_id: "p1_kid".to_string(),
            definition_id: "TEST_DISGUISE_KID".to_string(),
            owner_id: "p1".to_string(),
            zone: models::Zone::FIELD,
            orientation: models::CardOrientation::ACTIVE,
            entered_turn: 2,
            face_up: true,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
            ..Default::default()
        });
        let hand_before = engine.players.get("p1").unwrap().hand.len();
        let need_ui = engine.trigger_on_disguise("p1", "p1_kid", "TEST_DISGUISE_KID");
        assert!(!need_ui);
        assert_eq!(engine.players.get("p1").unwrap().hand.len(), hand_before + 1);
        assert_eq!(engine.players.get("p1").unwrap().field.last().unwrap().temp_ap_bonus, 1000);
    }

    #[test]
    fn test_case_card_resolution_and_declare_effects() {
        let path = if std::path::Path::new("data/dummy/cards.json").exists() {
            "data/dummy/cards.json"
        } else {
            "../data/dummy/cards.json"
        };
        let catalog = CardCatalog::load_from_json(path).expect("catalog load failed");
        let mut engine = MatchEngine::new(catalog);
        engine.turn = 4;
        engine.turn_player_id = "p1".to_string();
        engine.player_order = vec!["p1".to_string(), "p2".to_string()];

        let mut ps1 = engine::PlayerState::new("p1".to_string());
        let mut ps2 = engine::PlayerState::new("p2".to_string());

        // Case B11099: 解決編になったとき手札1枚リムーブ / 【解決編】【宣言】【ターン1】裏向き証拠2つ表向き -> AP-3000
        ps1.r#case = Some(models::CardInstance {
            instance_id: "p1_case".to_string(),
            definition_id: "B11099".to_string(),
            owner_id: "p1".to_string(),
            zone: models::Zone::CASE,
            orientation: models::CardOrientation::ACTIVE,
            entered_turn: 0,
            face_up: true,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
            ..Default::default()
        });
        for i in 0..7 {
            ps1.file.push(models::CardInstance {
                instance_id: format!("p1_f{}", i),
                definition_id: "B01005".to_string(),
                owner_id: "p1".to_string(),
                zone: models::Zone::FILE,
                orientation: models::CardOrientation::ACTIVE,
                entered_turn: 0,
                face_up: false,
                temp_ap_bonus: 0,
                has_rush: false,
                rush_target: None,
                used_declare_this_turn: false,
                ..Default::default()
            });
        }
        ps1.hand.push(models::CardInstance {
            instance_id: "p1_h1".to_string(),
            definition_id: "B01005".to_string(),
            owner_id: "p1".to_string(),
            zone: models::Zone::HAND,
            orientation: models::CardOrientation::ACTIVE,
            entered_turn: 0,
            face_up: true,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
            ..Default::default()
        });
        for i in 0..2 {
            ps1.evidence.push(models::CardInstance {
                instance_id: format!("p1_ev{}", i),
                definition_id: "B01005".to_string(),
                owner_id: "p1".to_string(),
                zone: models::Zone::EVIDENCE,
                orientation: models::CardOrientation::ACTIVE,
                entered_turn: 0,
                face_up: false, // face down
                temp_ap_bonus: 0,
                has_rush: false,
                rush_target: None,
                used_declare_this_turn: false,
                ..Default::default()
            });
        }

        ps2.field.push(models::CardInstance {
            instance_id: "p2_c1".to_string(),
            definition_id: "B01005".to_string(),
            owner_id: "p2".to_string(),
            zone: models::Zone::FIELD,
            orientation: models::CardOrientation::ACTIVE,
            entered_turn: 1,
            face_up: true,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
            ..Default::default()
        });

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        // 1. Trigger resolution transition -> prompts hand discard, then hand size goes from 1 to 0
        engine.check_resolution_transition("p1");
        assert_eq!(engine.players.get("p1").unwrap().case_phase, models::CasePhase::RESOLUTION);
        let discard_pd = engine.pending_decision.as_ref().unwrap().clone();
        assert_eq!(discard_pd["kind"], "targetSelect");
        assert_eq!(discard_pd["options"]["effectType"], "DISCARD_HAND");
        let discard_dec_id = discard_pd["decisionId"].as_str().unwrap().to_string();
        let target_hand_id = discard_pd["options"]["candidates"][0]["instanceId"].as_str().unwrap().to_string();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": discard_dec_id,
            "revision": engine.revision,
            "value": {"targetId": target_hand_id, "effectType": "DISCARD_HAND"}
        }));
        assert_eq!(engine.players.get("p1").unwrap().hand.len(), 0);

        // 2. Declare option should include case card
        engine.prompt_main_action("p1");
        let pd = engine.pending_decision.as_ref().unwrap().clone();
        let actions = pd["options"]["actions"].as_array().unwrap();
        let declare_act = actions.iter().find(|a| a["type"] == "DECLARE").unwrap();
        let actors = declare_act["actors"].as_array().unwrap();
        assert!(actors.iter().any(|a| a["actorId"] == "p1_case"));

        // 3. Execute DECLARE on case card
        let dec_id = pd["decisionId"].as_str().unwrap().to_string();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": dec_id,
            "revision": engine.revision,
            "value": {"type": "DECLARE", "actorId": "p1_case"}
        }));

        // 証拠を表向きにするコストは選択式で支払う
        let flip_pd = engine.pending_decision.as_ref().unwrap().clone();
        assert_eq!(flip_pd["options"]["effectType"], "FLIP_EVIDENCE_FACE_UP");
        let flip_ids: Vec<String> = flip_pd["options"]["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["instanceId"].as_str().unwrap().to_string())
            .collect();
        let flip_dec_id = flip_pd["decisionId"].as_str().unwrap().to_string();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": flip_dec_id,
            "revision": engine.revision,
            "value": {"targetIds": flip_ids}
        }));

        // Two evidence cards should now be face_up
        assert!(engine.players.get("p1").unwrap().evidence.iter().all(|e| e.face_up));

        // Target select for AP-3000 should be pending
        let ts_pd = engine.pending_decision.as_ref().unwrap().clone();
        assert_eq!(ts_pd["kind"], "targetSelect");
        let ts_dec_id = ts_pd["decisionId"].as_str().unwrap().to_string();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": ts_dec_id,
            "revision": engine.revision,
            "value": {"targetId": "p2_c1", "actorId": "p1_case", "effectType": "AP_DEBUFF"}
        }));

        assert_eq!(engine.players.get("p2").unwrap().field[0].temp_ap_bonus, -3000);
    }

    #[test]
    fn test_event_card_play_and_remove_zone() {
        let path = if std::path::Path::new("data/dummy/cards.json").exists() {
            "data/dummy/cards.json"
        } else {
            "../data/dummy/cards.json"
        };
        let mut catalog = CardCatalog::load_from_json(path).expect("catalog load failed");

        catalog.definitions.insert(
            "TEST_EVENT_DRAW".to_string(),
            models::CardDefinition {
                definition_id: "TEST_EVENT_DRAW".to_string(),
                canonical_id: "TEST_EVENT_DRAW".to_string(),
                card_type: models::CardType::EVENT,
                colors: vec!["青".to_string()],
                level: 2,
                ap: 0,
                lp: 0,
                names: vec!["テストイベント".to_string()],
                description: "カードを1枚引く。".to_string(),
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

        // Fill p1 field to max 5 chars (cannot play char, but event is still playable once per turn)
        ps1.normal_hand_played_this_turn = false;
        for i in 0..5 {
            ps1.field.push(models::CardInstance {
                instance_id: format!("p1_f{}", i),
                definition_id: "B01005".to_string(),
                owner_id: "p1".to_string(),
                zone: models::Zone::FIELD,
                orientation: models::CardOrientation::ACTIVE,
                entered_turn: 1,
                face_up: true,
                temp_ap_bonus: 0,
                has_rush: false,
                rush_target: None,
                used_declare_this_turn: false,
                ..Default::default()
            });
        }
        // Add 3 FILEs
        for i in 0..3 {
            ps1.file.push(models::CardInstance {
                instance_id: format!("p1_fl{}", i),
                definition_id: "B01005".to_string(),
                owner_id: "p1".to_string(),
                zone: models::Zone::FILE,
                orientation: models::CardOrientation::ACTIVE,
                entered_turn: 0,
                face_up: false,
                temp_ap_bonus: 0,
                has_rush: false,
                rush_target: None,
                used_declare_this_turn: false,
                ..Default::default()
            });
        }
        // Add event card to hand and 1 card in deck
        ps1.hand.push(models::CardInstance {
            instance_id: "p1_ev_card".to_string(),
            definition_id: "TEST_EVENT_DRAW".to_string(),
            owner_id: "p1".to_string(),
            zone: models::Zone::HAND,
            orientation: models::CardOrientation::ACTIVE,
            entered_turn: 0,
            face_up: true,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
            ..Default::default()
        });
        ps1.deck.push(models::CardInstance {
            instance_id: "p1_dk1".to_string(),
            definition_id: "B01005".to_string(),
            owner_id: "p1".to_string(),
            zone: models::Zone::DECK,
            orientation: models::CardOrientation::ACTIVE,
            entered_turn: 0,
            face_up: false,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
            ..Default::default()
        });
        // バッファ山札（1枚ドローで山札が0枚になった瞬間のリフレッシュが発動し、テスト本体の効果を邪魔しないように余裕を持たせる）
        for i in 0..3 {
            ps1.deck.insert(0, models::CardInstance {
                instance_id: format!("p1_buf_ev_{}", i),
                definition_id: "B01005".to_string(),
                owner_id: "p1".to_string(),
                zone: models::Zone::DECK,
                orientation: models::CardOrientation::ACTIVE,
                entered_turn: 0,
                face_up: false,
                ..Default::default()
            });
        }

        engine.players.insert("p1".to_string(), ps1);
        engine.players.insert("p2".to_string(), ps2);

        // Even with 5 field cards & normal_hand_played = true, event card should be playable
        engine.prompt_main_action("p1");
        let pd = engine.pending_decision.as_ref().unwrap().clone();
        let actions = pd["options"]["actions"].as_array().unwrap();
        let hand_play = actions.iter().find(|a| a["type"] == "HAND_PLAY").unwrap();
        assert!(hand_play["cardIds"].as_array().unwrap().contains(&serde_json::json!("p1_ev_card")));

        // Play event card
        let dec_id = pd["decisionId"].as_str().unwrap().to_string();
        let _ = engine.submit_decision("p1", &serde_json::json!({
            "decisionId": dec_id,
            "revision": engine.revision,
            "value": {"type": "HAND_PLAY", "cardId": "p1_ev_card"}
        }));

        let p1_after = engine.players.get("p1").unwrap();
        // Field stays 5, event goes to remove, and 1 card drawn into hand
        assert_eq!(p1_after.field.len(), 5);
        assert_eq!(p1_after.remove.len(), 1);
        assert_eq!(p1_after.remove[0].definition_id, "TEST_EVENT_DRAW");
        assert_eq!(p1_after.hand.len(), 1);
    }

    #[test]
    fn test_smart_cpu_agent_decisions() {
        let path = if std::path::Path::new("data/dummy/cards.json").exists() {
            "data/dummy/cards.json"
        } else {
            "../data/dummy/cards.json"
        };
        let mut catalog = CardCatalog::load_from_json(path).expect("catalog load failed");

        for (id, ap, lvl) in [
            ("CPU_LOW", 2000, 2),
            ("CPU_MID", 3000, 3),
            ("CPU_HIGH", 5000, 5),
        ] {
            catalog.definitions.insert(
                id.to_string(),
                models::CardDefinition {
                    definition_id: id.to_string(),
                    canonical_id: id.to_string(),
                    card_type: models::CardType::CHARACTER,
                    colors: vec!["青".to_string()],
                    level: lvl,
                    ap,
                    lp: 1,
                    names: vec![id.to_string()],
                    description: String::new(),
                    traits: vec![],
                    case_levels: None,
                    main_thumb: None,
                    main_path: None,
                },
            );
        }

        let mut engine = MatchEngine::new(catalog);
        engine.turn = 3;
        engine.turn_player_id = "player2".to_string();
        engine.player_order = vec!["player1".to_string(), "player2".to_string()];

        let mut ps1 = engine::PlayerState::new("player1".to_string());
        let mut ps2 = engine::PlayerState::new("player2".to_string());

        // player1 (opponent) has AP2000 (SLEEP) and AP5000 (SLEEP)
        ps1.field.push(models::CardInstance {
            instance_id: "p1_weak".to_string(),
            definition_id: "CPU_LOW".to_string(),
            owner_id: "player1".to_string(),
            zone: models::Zone::FIELD,
            orientation: models::CardOrientation::SLEEP,
            entered_turn: 1,
            face_up: true,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
            ..Default::default()
        });
        ps1.field.push(models::CardInstance {
            instance_id: "p1_strong".to_string(),
            definition_id: "CPU_HIGH".to_string(),
            owner_id: "player1".to_string(),
            zone: models::Zone::FIELD,
            orientation: models::CardOrientation::SLEEP,
            entered_turn: 1,
            face_up: true,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
            ..Default::default()
        });

        // player2 (CPU) has AP3000 (ACTIVE)
        ps2.field.push(models::CardInstance {
            instance_id: "p2_mid".to_string(),
            definition_id: "CPU_MID".to_string(),
            owner_id: "player2".to_string(),
            zone: models::Zone::FIELD,
            orientation: models::CardOrientation::ACTIVE,
            entered_turn: 1,
            face_up: true,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
            ..Default::default()
        });
        // player2 hand has Lv2 and Lv5 for mulligan test
        ps2.hand.push(models::CardInstance {
            instance_id: "h_lv2".to_string(),
            definition_id: "CPU_LOW".to_string(),
            owner_id: "player2".to_string(),
            zone: models::Zone::HAND,
            orientation: models::CardOrientation::ACTIVE,
            entered_turn: 0,
            face_up: true,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
            ..Default::default()
        });
        ps2.hand.push(models::CardInstance {
            instance_id: "h_lv5".to_string(),
            definition_id: "CPU_HIGH".to_string(),
            owner_id: "player2".to_string(),
            zone: models::Zone::HAND,
            orientation: models::CardOrientation::ACTIVE,
            entered_turn: 0,
            face_up: true,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
            ..Default::default()
        });

        engine.players.insert("player1".to_string(), ps1);
        engine.players.insert("player2".to_string(), ps2);

        let cpu = agent::RandomAgent::new("player2");

        // 1. Mulligan test: returns Lv5, keeps Lv2
        let mul_dec = serde_json::json!({
            "kind": "mulligan",
            "options": {"handCardIds": ["h_lv2", "h_lv5"]}
        });
        let mul_ans = cpu.choose_action_with_engine(&engine, &mul_dec);
        assert_eq!(mul_ans["value"]["cardIds"], serde_json::json!(["h_lv5"]));

        // 2. MainAction test: AP3000 attacks AP2000 (not AP5000)
        let act_dec = serde_json::json!({
            "kind": "mainAction",
            "options": {
                "actions": [
                    {"type": "END_TURN"},
                    {
                        "type": "ACTION",
                        "actorIds": ["p2_mid"],
                        "targetIds": ["p1_weak", "p1_strong"],
                        "actorTargetMap": {"p2_mid": ["p1_weak", "p1_strong"]}
                    }
                ]
            }
        });
        let act_ans = cpu.choose_action_with_engine(&engine, &act_dec);
        assert_eq!(act_ans["value"]["type"], "ACTION");
        assert_eq!(act_ans["value"]["targetId"], "p1_weak");

        // 3. Guard test: if attacker AP is 2000, AP3000 blocks it
        engine.action_context = Some(serde_json::json!({
            "attackerPlayerId": "player1",
            "actorId": "p1_weak",
            "targetId": "p2_case"
        }));
        let guard_dec = serde_json::json!({
            "kind": "guard",
            "options": {"guardCardIds": ["p2_mid"], "cutinCardIds": []}
        });
        let guard_ans = cpu.choose_action_with_engine(&engine, &guard_dec);
        assert_eq!(guard_ans["value"]["guardCardId"], "p2_mid");
    }

