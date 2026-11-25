use super::*;
use crate::models::DeckCardItem;
use crate::room::MatchRoom;
use serde_json::json;

fn rw_deck() -> models::DeckDefinition {
    models::DeckDefinition {
        deck_id: Some("rw".to_string()),
        deck_name: Some("rw".to_string()),
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

/// CPU戦：自分の手（mainAction/hintPlay）の直前スナップショットへ巻き戻せること
#[test]
fn rewind_restores_snapshot_in_cpu_match() {
    let catalog = load_test_catalog();
    let deck = rw_deck();
    let mut room = MatchRoom::new("t", catalog.clone(), deck.clone(), true, deck.clone());

    // 現在状態をスナップショットとして保持（actor=player1, 応答なし）
    room.snapshot = Some(room.engine.clone());
    room.snapshot_actor = Some("player1".to_string());
    room.snapshot_kind = Some("mainAction".to_string());
    room.others_decided_since_snapshot = 0;
    room.rewind_request = None;

    let before_turn = room.engine.turn;
    let before_revision = room.engine.revision;

    // スナップショット後に盤面を進めた（変更した）と仮定
    room.engine.turn += 100;
    assert_ne!(room.engine.turn, before_turn);

    // player1 が巻き戻し要求 → CPU戦なので自動同意・実行される
    assert!(room.can_rewind_by("player1"));
    room.request_rewind("player1").expect("CPU戦の巻き戻し要求は成功する");

    // 復元されている
    assert_eq!(room.engine.turn, before_turn, "ターンが巻き戻し時点に戻る");
    assert_eq!(room.engine.revision, before_revision, "revisionが巻き戻し時点に戻る");
    // 巻き戻し状態はリセット
    assert!(room.snapshot.is_none());
    assert!(room.rewind_request.is_none());
}

/// リーク防止：相手の応答が既に出ている場合は巻き戻し不可
#[test]
fn rewind_blocked_after_opponent_responded() {
    let catalog = load_test_catalog();
    let deck = rw_deck();
    let mut room = MatchRoom::new("t", catalog.clone(), deck.clone(), true, deck.clone());

    room.snapshot = Some(room.engine.clone());
    room.snapshot_actor = Some("player1".to_string());
    room.snapshot_kind = Some("mainAction".to_string());
    // 相手（player2）が既に1回応答したことにする（隠情報開示）
    room.others_decided_since_snapshot = 1;

    assert!(!room.can_rewind_by("player1"), "応答後は巻き戻し不可");
    let err = room.request_rewind("player1").unwrap_err();
    assert!(err.contains("巻き戻しできません"), "エラーメッセージ");
}

/// 他人の手は巻き戻せない（actor不一致）
#[test]
fn cannot_rewind_others_hand() {
    let catalog = load_test_catalog();
    let deck = rw_deck();
    let mut room = MatchRoom::new("t", catalog.clone(), deck.clone(), true, deck.clone());

    room.snapshot = Some(room.engine.clone());
    room.snapshot_actor = Some("player2".to_string()); // player2が打った手
    room.others_decided_since_snapshot = 0;

    assert!(!room.can_rewind_by("player1"), "自分の手でなければ巻き戻し不可");
}

/// 対人部屋：合意フロー（要求→相手同意で復元、拒否で不変、自己同意不可）
#[test]
fn human_room_rewind_requires_agreement() {
    let catalog = load_test_catalog();
    let deck = rw_deck();
    // 対人部屋（is_cpu=false）
    let mut room = MatchRoom::new("t", catalog.clone(), deck.clone(), false, deck.clone());
    room.join_p2(&catalog, deck.clone());

    // 手番がどちらであっても、状態を直接設定して機構を検証
    room.snapshot = Some(room.engine.clone());
    room.snapshot_actor = Some("player1".to_string());
    room.snapshot_kind = Some("mainAction".to_string());
    room.others_decided_since_snapshot = 0;
    room.rewind_request = None;

    let before_turn = room.engine.turn;
    room.engine.turn += 100;

    // player1 が要求（対人なので自動では適用されない）
    room.request_rewind("player1").expect("要求自体は成功");
    assert_eq!(room.rewind_request.as_deref(), Some("player1"));
    assert_eq!(room.engine.turn, before_turn + 100, "要求だけでは復元されない");

    // 本人（requester）の同意は不可
    assert!(room.accept_rewind("player1").is_err(), "自己同意は不可");

    // 相手（player2）が同意 → 復元
    room.accept_rewind("player2").expect("相手同意で復元");
    assert_eq!(room.engine.turn, before_turn, "同意で巻き戻し復元");
    assert!(room.snapshot.is_none());
}

/// 対人部屋：拒否すると巻き戻しされない
#[test]
fn human_room_rewind_rejected() {
    let catalog = load_test_catalog();
    let deck = rw_deck();
    let mut room = MatchRoom::new("t", catalog.clone(), deck.clone(), false, deck.clone());
    room.join_p2(&catalog, deck.clone());

    room.snapshot = Some(room.engine.clone());
    room.snapshot_actor = Some("player1".to_string());
    room.snapshot_kind = Some("mainAction".to_string());
    room.others_decided_since_snapshot = 0;

    let before_turn = room.engine.turn;
    room.engine.turn += 100;

    room.request_rewind("player1").expect("要求成功");
    room.reject_rewind("player2").expect("拒否成功");

    assert!(room.rewind_request.is_none(), "拒否で要求解除");
    assert_eq!(room.engine.turn, before_turn + 100, "拒否で盤面は不変");
}

/// submit_decision_tracked が mainAction 直前にスナップショットを取り、
/// 相手の応答で others カウンタが増えることを検証する軽量確認
#[test]
fn tracked_snapshot_counters_update() {
    let catalog = load_test_catalog();
    let deck = rw_deck();
    let mut room = MatchRoom::new("t", catalog.clone(), deck.clone(), false, deck.clone());
    room.join_p2(&catalog, deck.clone());

    // 初期状態：スナップショットなし
    assert!(room.snapshot.is_none());
    assert!(!room.can_rewind_by("player1"));

    // 打者=player1 の mainAction 相当を pending に設定して tracked 提出を試みる。
    // kind 処理自体はエンジン依存のため成否を問わず、スナップショット採番は行われる。
    room.engine.pending_decision =
        Some(json!({ "decisionId": "dec_test", "kind": "mainAction", "playerId": "player1" }));
    let _ = room.submit_decision_tracked("player1", &json!({ "value": {} }));

    // mainAction だったのでスナップショットが採られ、actor=player1 になっている
    assert!(room.snapshot.is_some(), "mainActionでスナップショット採番");
    assert_eq!(room.snapshot_actor.as_deref(), Some("player1"));
    assert_eq!(room.others_decided_since_snapshot, 0);

    // 次に player2 の応答系（打者以外）が提出されると others が増える
    room.engine.pending_decision =
        Some(json!({ "decisionId": "dec_test2", "kind": "guard", "playerId": "player2" }));
    let _ = room.submit_decision_tracked("player2", &json!({ "value": {} }));
    assert_eq!(
        room.others_decided_since_snapshot, 1,
        "打者以外の応答で others が増える（巻き戻し不可化）"
    );
    assert!(!room.can_rewind_by("player1"));
}
