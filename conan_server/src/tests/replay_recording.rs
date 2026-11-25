use super::*;
use crate::models::DeckCardItem;
use serde_json::json;

fn make_deck() -> models::DeckDefinition {
    models::DeckDefinition {
        deck_id: Some("test_replay".to_string()),
        deck_name: Some("test".to_string()),
        catalog_version: None,
        partner_id: "B01005P".to_string(),
        case_id: "B01005".to_string(),
        main_deck: (0..40)
            .map(|i| DeckCardItem {
                definition_id: format!("CARD{}", i % 10),
                count: 1,
            })
            .collect(),
    }
}

/// 同一シードなら、先攻順・手札の配列順まで完全に一致すること（リプレイ再現性の担保）
#[test]
fn same_seed_produces_deterministic_match_start() {
    let catalog = load_test_catalog();
    let deck = make_deck();
    let seed = 42u64;

    let mut e1 = MatchEngine::new_with_seed(catalog.clone(), seed);
    e1.add_player("p1", &deck);
    e1.add_player("p2", &deck);
    e1.start_match();

    let mut e2 = MatchEngine::new_with_seed(catalog, seed);
    e2.add_player("p1", &deck);
    e2.add_player("p2", &deck);
    e2.start_match();

    // 先攻プレイヤーが一致
    assert_eq!(
        e1.player_order[0], e2.player_order[0],
        "同一シードで先攻順が一致すべき"
    );

    // 各プレイヤーの手札（definition_id 順）が一致
    for pid in ["p1", "p2"] {
        let h1: Vec<String> = e1
            .players
            .get(pid)
            .unwrap()
            .hand
            .iter()
            .map(|c| c.definition_id.clone())
            .collect();
        let h2: Vec<String> = e2
            .players
            .get(pid)
            .unwrap()
            .hand
            .iter()
            .map(|c| c.definition_id.clone())
            .collect();
        assert_eq!(h1, h2, "{} の手札配列が同一シードで一致すべき", pid);
    }
}

/// 別シードなら手札配列が（高確率で）異なること＝シードが実際に効いていること
#[test]
fn different_seed_diverges() {
    let catalog = load_test_catalog();
    let deck = make_deck();

    let mut e1 = MatchEngine::new_with_seed(catalog.clone(), 1u64);
    e1.add_player("p1", &deck);
    e1.add_player("p2", &deck);
    e1.start_match();

    let mut e2 = MatchEngine::new_with_seed(catalog, 2u64);
    e2.add_player("p1", &deck);
    e2.add_player("p2", &deck);
    e2.start_match();

    let h1: Vec<String> = e1
        .players
        .get("p1")
        .unwrap()
        .hand
        .iter()
        .map(|c| c.definition_id.clone())
        .collect();
    let h2: Vec<String> = e2
        .players
        .get("p1")
        .unwrap()
        .hand
        .iter()
        .map(|c| c.definition_id.clone())
        .collect();
    assert_ne!(h1, h2, "別シードでは手札配列が異なるはず");
}

/// リプレイイベントが match_start / decision / match_end と eventSeq 付きで記録されること
#[test]
fn replay_log_records_events_with_seq() {
    let catalog = load_test_catalog();
    let deck = make_deck();

    let mut engine = MatchEngine::new_with_seed(catalog, 7u64);
    engine.set_match_id("m_test_1");
    engine.record_event(json!({ "type": "match_start", "seed": 7 }));
    engine.add_player("p1", &deck);
    engine.add_player("p2", &deck);
    engine.start_match();

    // start_match で mulligan の pending_decision が生成される
    assert!(engine.pending_decision.is_some(), "マリガン待機になるはず");

    // eventSeq が振られていること
    assert_eq!(engine.replay_log[0]["eventSeq"], json!(1));
    assert_eq!(engine.replay_log[0]["matchId"], json!("m_test_1"));
    assert_eq!(engine.replay_log[0]["type"], json!("match_start"));

    // マリガン回答を記録経由で送る（decision イベントが入る）
    // ※ 先攻はシード依存で player1/player2 のいずれかになるため、実際の待機プレイヤーへ送る
    let dec = engine.pending_decision.as_ref().unwrap().clone();
    let dec_id = dec["decisionId"].as_str().unwrap().to_string();
    let dec_player = dec["playerId"].as_str().unwrap().to_string();
    let res = engine.submit_decision(&dec_player, &json!({ "value": { "cardIds": [] } }));
    assert!(res.is_ok(), "マリガン回答は成功するはず: {:?}", res);
    let has_decision = engine
        .replay_log
        .iter()
        .any(|e| e["type"] == json!("decision") && e["kind"] == json!("mulligan"));
    assert!(has_decision, "decision イベントが記録されるべき");
}
