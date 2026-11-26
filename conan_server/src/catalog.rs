use crate::models::{CardDefinition, CardType};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

const EMBEDDED_PACK_RESTRICTIONS_JSON: &str =
    include_str!("../../data/dummy/pack_restrictions.json");

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackRestrictionsConfig {
    #[serde(default)]
    pub unreleased_packs: Vec<String>,
    #[serde(default)]
    pub restrict_pr: bool,
    #[serde(default)]
    pub restricted_card_ids: Vec<String>,
}

impl PackRestrictionsConfig {
    pub fn load_default() -> Self {
        let mut cfg: Self = serde_json::from_str(EMBEDDED_PACK_RESTRICTIONS_JSON).unwrap_or_default();
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Ok(raw) = std::fs::read_to_string("data/dummy/pack_restrictions.json") {
                if let Ok(disk_cfg) = serde_json::from_str::<Self>(&raw) {
                    cfg = disk_cfg;
                }
            }
            if let Ok(val) = std::env::var("UNRELEASED_PACKS") {
                for item in val.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                    if !cfg.unreleased_packs.iter().any(|x| x == item) {
                        cfg.unreleased_packs.push(item.to_string());
                    }
                }
            }
            if let Ok(val) = std::env::var("RESTRICT_PR") {
                let lower = val.trim().to_ascii_lowercase();
                if lower == "true" || lower == "1" {
                    cfg.restrict_pr = true;
                }
            }
            if let Ok(val) = std::env::var("RESTRICTED_CARD_IDS") {
                for item in val.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                    if !cfg.restricted_card_ids.iter().any(|x| x == item) {
                        cfg.restricted_card_ids.push(item.to_string());
                    }
                }
            }
        }
        cfg
    }

    pub fn is_card_restricted(&self, card: &Value) -> bool {
        let def_id = card
            .get("definitionId")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if !def_id.is_empty() && self.restricted_card_ids.iter().any(|id| id == def_id) {
            return true;
        }
        let pack_code = card.get("packCode").and_then(|v| v.as_str()).unwrap_or("");
        let package_name = card.get("package").and_then(|v| v.as_str()).unwrap_or("");
        let is_pr = pack_code == "PR" || package_name == "PRカード" || def_id.starts_with("PR");
        if self.restrict_pr && is_pr {
            return true;
        }
        if self
            .unreleased_packs
            .iter()
            .any(|p| (!pack_code.is_empty() && p == pack_code) || (!package_name.is_empty() && p == package_name))
        {
            return true;
        }
        false
    }
}

pub fn apply_pack_restrictions(raw_json: &mut Value, config: &PackRestrictionsConfig) {
    if let Some(cards_val) = raw_json.get_mut("cards").and_then(|v| v.as_array_mut()) {
        for card in cards_val.iter_mut() {
            let restricted = config.is_card_restricted(card);
            card["isRestricted"] = Value::Bool(restricted);
        }
    }
    if let Ok(cfg_val) = serde_json::to_value(config) {
        raw_json["packRestrictions"] = cfg_val;
    }
}

#[derive(Debug, Clone)]
pub struct CardCatalog {
    pub catalog_version: String,
    pub definitions: HashMap<String, CardDefinition>,
    pub raw_json: Value,
}

impl CardCatalog {
    pub fn empty() -> Self {
        Self {
            catalog_version: "empty".to_string(),
            definitions: HashMap::new(),
            raw_json: Value::Null,
        }
    }

    pub fn load_from_json<P: AsRef<Path>>(file_path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let file = File::open(file_path)?;
        let reader = BufReader::new(file);
        let raw_json: Value = serde_json::from_reader(reader)?;
        Self::from_raw_json(raw_json)
    }

    pub fn load_from_str(json_str: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let raw_json: Value = serde_json::from_str(json_str)?;
        Self::from_raw_json(raw_json)
    }

    fn from_raw_json(mut raw_json: Value) -> Result<Self, Box<dyn std::error::Error>> {

        let catalog_version = raw_json
            .get("catalogVersion")
            .and_then(|v| v.as_str())
            .unwrap_or("official-raw-v1")
            .to_string();

        let mut definitions = HashMap::new();

        if let Some(cards_val) = raw_json.get_mut("cards").and_then(|v| v.as_array_mut()) {
            for card in cards_val.iter_mut() {
                let def_id = match card.get("definitionId").and_then(|v| v.as_str()) {
                    Some(id) => id.to_string(),
                    None => continue,
                };

                let raw_type_str = card
                    .get("cardType")
                    .or_else(|| card.get("type"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("CHARACTER");

                let card_type = match raw_type_str.trim() {
                    "パートナー" | "PARTNER" => CardType::PARTNER,
                    "事件" | "CASE" => CardType::CASE,
                    "イベント" | "EVENT" => CardType::EVENT,
                    _ => CardType::CHARACTER,
                };

                // Also update the card object in raw_json so that /api/catalog serves normalized types
                let normalized_type_str = match card_type {
                    CardType::PARTNER => "PARTNER",
                    CardType::CASE => "CASE",
                    CardType::EVENT => "EVENT",
                    CardType::CHARACTER => "CHARACTER",
                };
                card["cardType"] = Value::String(normalized_type_str.to_string());

                let canonical_id = card
                    .get("canonicalId")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&def_id)
                    .to_string();

                let colors = card
                    .get("colors")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|c| c.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();

                let parse_i32 = |v: Option<&Value>| -> i32 {
                    match v {
                        Some(Value::Number(n)) => n.as_i64().unwrap_or(0) as i32,
                        Some(Value::String(s)) => s.parse::<i32>().unwrap_or(0),
                        _ => 0,
                    }
                };

                let level = parse_i32(card.get("level"));
                let ap = parse_i32(card.get("ap"));
                let lp = parse_i32(card.get("lp"));

                let name = card
                    .get("title")
                    .or_else(|| card.get("dummyName"))
                    .and_then(|v| v.as_str())
                    .unwrap_or(&def_id)
                    .to_string();

                let names = card
                    .get("names")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|c| c.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_else(|| vec![name]);

                let clean_desc = |s: &str| -> String {
                    s.replace("\r\n", "\n")
                        .replace('\r', "\n")
                        .replace("<br>", "\n")
                        .lines()
                        .map(|l| l.trim())
                        .filter(|l| !l.is_empty())
                        .collect::<Vec<_>>()
                        .join("\n")
                };

                let raw_desc = card
                    .get("description")
                    .or_else(|| card.get("feature"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let mut desc_parts: Vec<String> = clean_desc(raw_desc)
                    .lines()
                    .map(|l| l.to_string())
                    .collect();
                let desc_joined = desc_parts.join("\n");
                if let Some(henso) = card.get("henso").and_then(|v| v.as_str()) {
                    let h = clean_desc(henso);
                    if !h.is_empty() && !desc_joined.contains(&h) {
                        desc_parts.insert(0, h);
                    }
                }
                for key in ["hirameki", "cut_in"] {
                    if let Some(extra) = card.get(key).and_then(|v| v.as_str()) {
                        let e = clean_desc(extra);
                        if !e.is_empty() && !desc_joined.contains(&e) {
                            desc_parts.push(e);
                        }
                    }
                }
                let description = desc_parts.join("\n");

                let mut traits: Vec<String> = Vec::new();
                if let Some(arr) = card.get("traits").and_then(|v| v.as_array()) {
                    for item in arr {
                        if let Some(s) = item.as_str() {
                            let t = s.trim();
                            if !t.is_empty() && t != "なし" && !traits.iter().any(|x| x == t) {
                                traits.push(t.to_string());
                            }
                        }
                    }
                } else {
                    for key in ["category1", "category2", "category3"] {
                        if let Some(val) = card.get(key).and_then(|v| v.as_str()) {
                            for part in val.split([',', '、']) {
                                let t = part.trim();
                                if !t.is_empty() && t != "なし" && !traits.iter().any(|x| x == t) {
                                    traits.push(t.to_string());
                                }
                            }
                        }
                    }
                }

                let main_thumb = card
                    .get("mainThumb")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let main_path = card
                    .get("mainPath")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                let case_levels = card.get("caseLevels").and_then(|v| {
                    if let Some(obj) = v.as_object() {
                        let mut map = HashMap::new();
                        for (k, val) in obj {
                            map.insert(k.clone(), parse_i32(Some(val)));
                        }
                        Some(map)
                    } else {
                        None
                    }
                });

                let c_def = CardDefinition {
                    definition_id: def_id.clone(),
                    canonical_id,
                    card_type,
                    colors,
                    level,
                    ap,
                    lp,
                    names,
                    description,
                    traits,
                    case_levels,
                    main_thumb,
                    main_path,
                };

                definitions.insert(def_id, c_def);
            }
        }

        let pack_restrictions = PackRestrictionsConfig::load_default();
        apply_pack_restrictions(&mut raw_json, &pack_restrictions);

        Ok(CardCatalog {
            catalog_version,
            definitions,
            raw_json,
        })
    }

    pub fn get(&self, definition_id: &str) -> Option<&CardDefinition> {
        self.definitions.get(definition_id)
    }
}
