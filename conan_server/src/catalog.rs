use crate::models::{CardDefinition, CardType};
use serde_json::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct CardCatalog {
    pub catalog_version: String,
    pub definitions: HashMap<String, CardDefinition>,
    pub raw_json: Value,
}

impl CardCatalog {
    pub fn load_from_json<P: AsRef<Path>>(file_path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let file = File::open(file_path)?;
        let reader = BufReader::new(file);
        let mut raw_json: Value = serde_json::from_reader(reader)?;

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

                let description = card
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

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
                    traits: vec![],
                    case_levels,
                    main_thumb,
                    main_path,
                };

                definitions.insert(def_id, c_def);
            }
        }

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
