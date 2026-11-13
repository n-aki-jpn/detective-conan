use rand::seq::SliceRandom;
use rand::Rng;
use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub struct RandomAgent {
    pub player_id: String,
}

impl RandomAgent {
    pub fn new(player_id: &str) -> Self {
        Self {
            player_id: player_id.to_string(),
        }
    }

    pub fn choose_action(&self, decision: &Value) -> Value {
        let kind = decision.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        let opts = decision.get("options").cloned().unwrap_or(json!({}));
        let mut rng = rand::thread_rng();

        match kind {
            "mulligan" => {
                let hand = opts.get("handCardIds").and_then(|v| v.as_array());
                if let Some(cards) = hand {
                    if !cards.is_empty() && rng.gen_bool(0.3) {
                        let chosen = cards.choose(&mut rng).unwrap();
                        return json!({"value": {"cardIds": [chosen]}});
                    }
                }
                json!({"value": {"cardIds": []}})
            }
            "mainAction" => {
                let empty_vec = vec![];
                let actions = opts.get("actions").and_then(|v| v.as_array()).unwrap_or(&empty_vec);
                let types: Vec<&str> = actions
                    .iter()
                    .filter_map(|a| a.get("type").and_then(|t| t.as_str()))
                    .collect();

                if types.contains(&"RESOLVE_CASE") {
                    return json!({"value": {"type": "RESOLVE_CASE"}});
                }
                if types.contains(&"HAND_PLAY") {
                    if let Some(act) = actions.iter().find(|a| a.get("type").and_then(|t| t.as_str()) == Some("HAND_PLAY")) {
                        if let Some(card_ids) = act.get("cardIds").and_then(|v| v.as_array()) {
                            if let Some(cid) = card_ids.choose(&mut rng) {
                                return json!({"value": {"type": "HAND_PLAY", "cardId": cid}});
                            }
                        }
                    }
                }
                if types.contains(&"DEDUCE") {
                    if let Some(act) = actions.iter().find(|a| a.get("type").and_then(|t| t.as_str()) == Some("DEDUCE")) {
                        if let Some(actor_ids) = act.get("actorIds").and_then(|v| v.as_array()) {
                            if let Some(aid) = actor_ids.choose(&mut rng) {
                                return json!({"value": {"type": "DEDUCE", "actorId": aid}});
                            }
                        }
                    }
                }
                if types.contains(&"ACTION") {
                    if let Some(act) = actions.iter().find(|a| a.get("type").and_then(|t| t.as_str()) == Some("ACTION")) {
                        let actor_ids = act.get("actorIds").and_then(|v| v.as_array());
                        let target_ids = act.get("targetIds").and_then(|v| v.as_array());
                        let actor_target_map = act.get("actorTargetMap").and_then(|v| v.as_object());
                        if let Some(aids) = actor_ids {
                            if let Some(aid_val) = aids.choose(&mut rng) {
                                let aid_str = aid_val.as_str().unwrap_or("");
                                let valid_tids = actor_target_map
                                    .and_then(|m| m.get(aid_str))
                                    .and_then(|v| v.as_array())
                                    .or(target_ids);
                                if let Some(tids) = valid_tids {
                                    if let Some(tid) = tids.choose(&mut rng) {
                                        return json!({"value": {"type": "ACTION", "actorId": aid_val, "targetId": tid}});
                                    }
                                }
                            }
                        }
                    }
                }
                if types.contains(&"DECLARE") && rng.gen_bool(0.7) {
                    if let Some(act) = actions.iter().find(|a| a.get("type").and_then(|t| t.as_str()) == Some("DECLARE")) {
                        if let Some(actors) = act.get("actors").and_then(|v| v.as_array()) {
                            if let Some(actor) = actors.choose(&mut rng) {
                                if let Some(aid) = actor.get("actorId").and_then(|v| v.as_str()) {
                                    return json!({"value": {"type": "DECLARE", "actorId": aid}});
                                }
                            }
                        }
                    }
                }
                if types.contains(&"DISGUISE") && rng.gen_bool(0.5) {
                    if let Some(act) = actions.iter().find(|a| a.get("type").and_then(|t| t.as_str()) == Some("DISGUISE")) {
                        let card_ids = act.get("cardIds").and_then(|v| v.as_array());
                        let target_ids = act.get("targetIds").and_then(|v| v.as_array());
                        if let (Some(cids), Some(tids)) = (card_ids, target_ids) {
                            if let (Some(cid), Some(tid)) = (cids.choose(&mut rng), tids.choose(&mut rng)) {
                                return json!({"value": {"type": "DISGUISE", "cardId": cid, "targetId": tid}});
                            }
                        }
                    }
                }
                if types.contains(&"ASSIST") && rng.gen_bool(0.5) {
                    return json!({"value": {"type": "ASSIST"}});
                }
                if types.contains(&"HINT_TAKE") && rng.gen_bool(0.3) {
                    return json!({"value": {"type": "HINT_TAKE"}});
                }

                json!({"value": {"type": "END_TURN"}})
            }
            "hintPlay" => {
                if let Some(playable) = opts.get("playableCardIds").and_then(|v| v.as_array()) {
                    if let Some(cid) = playable.choose(&mut rng) {
                        return json!({"value": {"cardId": cid}});
                    }
                }
                json!({"value": {"cardId": null}})
            }
            "guard" => {
                if let Some(guards) = opts.get("guardCardIds").and_then(|v| v.as_array()) {
                    if !guards.is_empty() && rng.gen_bool(0.5) {
                        let chosen = guards.choose(&mut rng).unwrap();
                        return json!({"value": {"guardCardId": chosen}});
                    }
                }
                json!({"value": {"guardCardId": null}})
            }
            "targetSelect" => {
                let actor_id = opts.get("actorId").and_then(|v| v.as_str()).unwrap_or("");
                let effect_type = opts.get("effectType").and_then(|v| v.as_str()).unwrap_or("");
                if let Some(candidates) = opts.get("candidates").and_then(|v| v.as_array()) {
                    if let Some(chosen) = candidates.choose(&mut rng) {
                        let tid = chosen.get("instanceId").and_then(|v| v.as_str());
                        return json!({
                            "value": {
                                "targetId": tid,
                                "actorId": actor_id,
                                "effectType": effect_type
                            }
                        });
                    }
                }
                json!({
                    "value": {
                        "targetId": null,
                        "actorId": actor_id,
                        "effectType": effect_type
                    }
                })
            }
            "deckSearch" => {
                let actor_id = opts.get("actorId").and_then(|v| v.as_str()).unwrap_or("");
                if let Some(cards) = opts.get("revealedCards").and_then(|v| v.as_array()) {
                    if let Some(chosen) = cards.choose(&mut rng) {
                        let cid = chosen.get("instanceId").and_then(|v| v.as_str());
                        return json!({
                            "value": {
                                "chosenCardId": cid,
                                "actorId": actor_id
                            }
                        });
                    }
                }
                json!({
                    "value": {
                        "chosenCardId": null,
                        "actorId": actor_id
                    }
                })
            }
            _ => json!({"value": {}}),
        }
    }
}
