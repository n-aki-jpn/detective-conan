pub(crate) use crate::catalog::CardCatalog;
pub(crate) use crate::engine::MatchEngine;
pub(crate) use crate::models::{self, DeckCardItem};
pub(crate) use crate::observation::build_player_observation;
pub(crate) use crate::official_deck::{parse_official_deck_html, resolve_color_code};
pub(crate) use crate::{agent, catalog, effects, engine};
pub(crate) use std::collections::HashMap;

mod basic_and_cpu;
mod cards_and_combat;
mod catalog_audit;
mod fuzz_matches;
mod http_integration;
mod pwa;
mod rewind;
mod regression_b01_b06;
mod regression_b07_b10;
mod regression_b11;
mod regression_mechanics;
mod replay_recording;
mod ui_smoke;

pub(super) fn test_inst(
    iid: &str,
    did: &str,
    owner: &str,
    zone: models::Zone,
    ori: models::CardOrientation,
) -> models::CardInstance {
    models::CardInstance {
        instance_id: iid.to_string(),
        definition_id: did.to_string(),
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
}

pub(super) fn load_test_catalog() -> CardCatalog {
    let path = if std::path::Path::new("data/dummy/cards.json").exists() {
        "data/dummy/cards.json"
    } else {
        "../data/dummy/cards.json"
    };
    CardCatalog::load_from_json(path).expect("catalog load failed")
}

#[allow(dead_code)]
pub(super) fn setup_main_match(turn: i32, turn_player_id: &str) -> MatchEngine {
    let mut engine = MatchEngine::new(load_test_catalog());
    engine.phase = models::MatchPhase::MAIN;
    engine.turn = turn;
    engine.turn_player_id = turn_player_id.to_string();
    engine.player_order = vec!["p1".to_string(), "p2".to_string()];
    engine.players.insert("p1".to_string(), engine::PlayerState::new("p1".to_string()));
    engine.players.insert("p2".to_string(), engine::PlayerState::new("p2".to_string()));
    engine
}
