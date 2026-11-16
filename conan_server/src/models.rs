use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CardType {
    CHARACTER,
    EVENT,
    PARTNER,
    CASE,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CardOrientation {
    ACTIVE,
    SLEEP,
    STUN,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Zone {
    DECK,
    HAND,
    FIELD,
    PARTNER,
    CASE,
    FILE,
    EVIDENCE,
    REMOVE,
    RESOLVING,
    PENDING_EVIDENCE_REMOVAL,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MatchPhase {
    WAITING,
    SETUP_HIDDEN,
    MULLIGAN_FIRST,
    MULLIGAN_SECOND,
    REVEAL,
    AUTO_RESTORE_PARTNER,
    AUTO_ACTIVATE,
    AUTO_DRAW,
    AUTO_FILE,
    MAIN,
    DEDUCE,
    ACTION_GUARD,
    ACTION_CONTACT,
    ACTION_CASE,
    END_TRIGGERS,
    END_EXPIRE,
    FINISHED,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CasePhase {
    INTRO,
    RESOLUTION,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardDefinition {
    pub definition_id: String,
    #[serde(default)]
    pub canonical_id: String,
    pub card_type: CardType,
    #[serde(default)]
    pub colors: Vec<String>,
    #[serde(default)]
    pub level: i32,
    #[serde(default)]
    pub ap: i32,
    #[serde(default)]
    pub lp: i32,
    #[serde(default)]
    pub names: Vec<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub traits: Vec<String>,
    #[serde(default)]
    pub case_levels: Option<HashMap<String, i32>>,
    #[serde(default)]
    pub main_thumb: Option<String>,
    #[serde(default)]
    pub main_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardInstance {
    pub instance_id: String,
    pub definition_id: String,
    pub owner_id: String,
    pub zone: Zone,
    #[serde(default = "default_orientation")]
    pub orientation: CardOrientation,
    #[serde(default = "default_entered_turn")]
    pub entered_turn: i32,
    #[serde(default)]
    pub face_up: bool,
    #[serde(default)]
    pub temp_ap_bonus: i32,
    #[serde(default)]
    pub has_rush: bool,
    #[serde(default)]
    pub rush_target: Option<String>,
    #[serde(default)]
    pub used_declare_this_turn: bool,
    #[serde(default)]
    pub set_cards: Vec<CardInstance>,
    #[serde(default)]
    pub can_attack_active: bool,
    #[serde(default)]
    pub must_guard_this_turn: bool,
    #[serde(default)]
    pub has_bullet: bool,
    #[serde(default)]
    pub base_ap_override: Option<i32>,
    #[serde(default)]
    pub temp_level_delta: i32,
    #[serde(default)]
    pub draw_on_action_this_turn: bool,
    #[serde(default)]
    pub contact_draw_and_ap1000_this_turn: bool,
    #[serde(default)]
    pub bullet_on_case_attack_discard_this_turn: bool,
    #[serde(default)]
    pub temp_name_override: Option<String>,
}

impl Default for CardInstance {
    fn default() -> Self {
        Self {
            instance_id: String::new(),
            definition_id: String::new(),
            owner_id: String::new(),
            zone: Zone::DECK,
            orientation: default_orientation(),
            entered_turn: default_entered_turn(),
            face_up: false,
            temp_ap_bonus: 0,
            has_rush: false,
            rush_target: None,
            used_declare_this_turn: false,
            set_cards: Vec::new(),
            can_attack_active: false,
            must_guard_this_turn: false,
            has_bullet: false,
            base_ap_override: None,
            temp_level_delta: 0,
            draw_on_action_this_turn: false,
            contact_draw_and_ap1000_this_turn: false,
            bullet_on_case_attack_discard_this_turn: false,
            temp_name_override: None,
        }
    }
}

fn default_orientation() -> CardOrientation {
    CardOrientation::ACTIVE
}

fn default_entered_turn() -> i32 {
    -1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeckCardItem {
    pub definition_id: String,
    pub count: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeckDefinition {
    pub deck_id: Option<String>,
    pub deck_name: Option<String>,
    pub catalog_version: Option<String>,
    pub partner_id: String,
    pub case_id: String,
    pub main_deck: Vec<DeckCardItem>,
}
