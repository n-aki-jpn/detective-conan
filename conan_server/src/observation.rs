use crate::engine::{MatchEngine, PlayerState};
use crate::models::{CardInstance, CardOrientation, CasePhase, MatchPhase};
use serde_json::{json, Value};

pub fn build_player_observation(engine: &MatchEngine, viewing_player_id: &str) -> Value {
    let catalog = &engine.catalog;

    let serialize_card = |c: &CardInstance, is_secret: bool| -> Value {
        let orientation_str = match c.orientation {
            CardOrientation::ACTIVE => "ACTIVE",
            CardOrientation::SLEEP => "SLEEP",
            CardOrientation::STUN => "STUN",
        };

        if is_secret {
            return json!({
                "instanceId": c.instance_id,
                "faceUp": false,
                "orientation": orientation_str
            });
        }

        let c_def = catalog.get(&c.definition_id);
        let name = c_def.and_then(|d| d.names.first().cloned()).unwrap_or_else(|| c.definition_id.clone());
        let desc = c_def.map(|d| d.description.clone()).unwrap_or_default();
        let colors = c_def.map(|d| d.colors.clone()).unwrap_or_default();
        let level = c_def.map(|d| d.level).unwrap_or(0);
        let base_ap = c_def.map(|d| d.ap).unwrap_or(0);
        let effective_ap = engine.get_effective_ap(&c.owner_id, c);
        let lp = c_def.map(|d| d.lp).unwrap_or(0);
        let card_type = c_def.map(|d| format!("{:?}", d.card_type)).unwrap_or_else(|| "CHARACTER".to_string());
        let main_thumb = c_def.and_then(|d| d.main_thumb.clone());
        let main_path = c_def.and_then(|d| d.main_path.clone());

        json!({
            "instanceId": c.instance_id,
            "definitionId": c.definition_id,
            "name": name,
            "description": desc,
            "colors": colors,
            "level": level,
            "ap": effective_ap,
            "baseAp": base_ap,
            "hasRush": c.has_rush,
            "usedDeclareThisTurn": c.used_declare_this_turn,
            "lp": lp,
            "cardType": card_type,
            "orientation": orientation_str,
            "enteredTurn": c.entered_turn,
            "faceUp": true,
            "mainThumb": main_thumb,
            "mainPath": main_path
        })
    };

    let serialize_player = |ps: &PlayerState, is_me: bool| -> Value {
        let case_phase_str = match ps.case_phase {
            CasePhase::INTRO => "INTRO",
            CasePhase::RESOLUTION => "RESOLUTION",
        };

        json!({
            "playerId": ps.player_id,
            "isFirstPlayer": ps.is_first_player,
            "casePhase": case_phase_str,
            "fileCount": ps.effective_file_count(),
            "deckCount": ps.deck.len(),
            "evidenceCount": ps.evidence.len(),
            "removeCount": ps.remove.len(),
            "handCount": ps.hand.len(),
            "hand": ps.hand.iter().map(|c| serialize_card(c, !is_me)).collect::<Vec<_>>(),
            "field": ps.field.iter().map(|c| serialize_card(c, false)).collect::<Vec<_>>(),
            "partner": ps.partner.as_ref().map(|c| serialize_card(c, false)),
            "case": ps.r#case.as_ref().map(|c| serialize_card(c, false)),
            "evidence": ps.evidence.iter().map(|c| serialize_card(c, !c.face_up)).collect::<Vec<_>>(),
            "remove": ps.remove.iter().map(|c| serialize_card(c, false)).collect::<Vec<_>>(),
            "normalHandPlayed": ps.normal_hand_played_this_turn,
            "hintPlayed": ps.hint_played_this_turn
        })
    };

    let me_ps = engine.players.get(viewing_player_id);
    let opp_pid = engine.player_order.iter().find(|pid| *pid != viewing_player_id).cloned().unwrap_or_default();
    let opp_ps = engine.players.get(&opp_pid);

    let pending_dec = match &engine.pending_decision {
        Some(dec) if dec.get("playerId").and_then(|v| v.as_str()) == Some(viewing_player_id) => {
            Some(dec.clone())
        }
        _ => None,
    };

    let decision_player_id = engine.pending_decision.as_ref().and_then(|d| d.get("playerId").and_then(|v| v.as_str()));

    let phase_str = match engine.phase {
        MatchPhase::WAITING => "WAITING",
        MatchPhase::SETUP_HIDDEN => "SETUP_HIDDEN",
        MatchPhase::MULLIGAN_FIRST => "MULLIGAN_FIRST",
        MatchPhase::MULLIGAN_SECOND => "MULLIGAN_SECOND",
        MatchPhase::REVEAL => "REVEAL",
        MatchPhase::AUTO_RESTORE_PARTNER => "AUTO_RESTORE_PARTNER",
        MatchPhase::AUTO_ACTIVATE => "AUTO_ACTIVATE",
        MatchPhase::AUTO_DRAW => "AUTO_DRAW",
        MatchPhase::AUTO_FILE => "AUTO_FILE",
        MatchPhase::MAIN => "MAIN",
        MatchPhase::DEDUCE => "DEDUCE",
        MatchPhase::ACTION_GUARD => "ACTION_GUARD",
        MatchPhase::ACTION_CONTACT => "ACTION_CONTACT",
        MatchPhase::ACTION_CASE => "ACTION_CASE",
        MatchPhase::END_TRIGGERS => "END_TRIGGERS",
        MatchPhase::END_EXPIRE => "END_EXPIRE",
        MatchPhase::FINISHED => "FINISHED",
    };

    json!({
        "matchId": "match_live",
        "revision": engine.revision,
        "phase": phase_str,
        "turn": engine.turn,
        "turnPlayerId": engine.turn_player_id,
        "decisionPlayerId": decision_player_id,
        "viewingPlayerId": viewing_player_id,
        "me": me_ps.map(|p| serialize_player(p, true)).unwrap_or(Value::Null),
        "opponent": opp_ps.map(|p| serialize_player(p, false)).unwrap_or(Value::Null),
        "pendingDecision": pending_dec,
        "winnerId": engine.winner_id,
        "finishReason": engine.finish_reason,
        "logs": &engine.logs
    })
}
