use crate::models::DeckCardItem;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishDeckRequest {
    #[serde(default)]
    pub partner_id: Option<String>,
    #[serde(default)]
    pub case_id: Option<String>,
    #[serde(default)]
    pub main_deck: Option<Vec<DeckCardItem>>,

    #[serde(default)]
    pub partner: Option<Vec<String>>,
    #[serde(default)]
    pub incident: Option<Vec<String>>,
    #[serde(default)]
    pub other: Option<Vec<String>>,

    #[serde(default)]
    pub color: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportOfficialDeckRequest {
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub deck_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ImportedOfficialDeck {
    pub partner_id: String,
    pub case_id: String,
    pub main_deck: Vec<DeckCardItem>,
    pub raw: Value,
}

/// 9文字の公式コード、または `d_code` クエリパラメータを含む公式URLからコードを抽出・検証する。
pub fn extract_official_code(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.len() == 9 && trimmed.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Ok(trimmed.to_string());
    }
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        if let Ok(parsed) = url::Url::parse(trimmed) {
            if let Some((_, code)) = parsed.query_pairs().find(|(k, _)| k == "d_code") {
                let code = code.trim();
                if code.len() == 9 && code.chars().all(|c| c.is_ascii_alphanumeric()) {
                    return Ok(code.to_string());
                }
            }
        }
    }
    Err("公式コードは9文字の英数字で入力してください。".to_string())
}

pub fn resolve_color_code(colors: &[String]) -> String {
    let mut code = String::new();
    for col in colors {
        for ch in col.chars() {
            let letter = match ch {
                '青' => 'B',
                '緑' => 'G',
                '白' => 'W',
                '赤' => 'R',
                '黄' => 'Y',
                '黒' => 'K',
                _ => continue,
            };
            if !code.contains(letter) {
                code.push(letter);
            }
        }
    }
    if code.is_empty() {
        "B".to_string()
    } else {
        code
    }
}

fn decode_html_entities(s: &str) -> String {
    s.replace("&quot;", "\"")
        .replace("&#34;", "\"")
        .replace("&#x22;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

fn extract_attr_value(tag: &str, attr_name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    for quote in ['"', '\''] {
        let pat = format!("{}={}", attr_name, quote);
        if let Some(pos) = lower.find(&pat) {
            let start = pos + pat.len();
            if let Some(end_rel) = tag[start..].find(quote) {
                return Some(decode_html_entities(&tag[start..start + end_rel]));
            }
        }
    }
    None
}

pub fn parse_official_deck_html(html: &str) -> Result<Value, String> {
    let mut deck_values = Vec::new();
    let mut search_from = 0;
    while let Some(rel_start) = html[search_from..].find('<') {
        let tag_start = search_from + rel_start;
        let Some(rel_end) = html[tag_start..].find('>') else {
            break;
        };
        let tag = &html[tag_start..tag_start + rel_end + 1];
        search_from = tag_start + rel_end + 1;

        let tag_lower = tag.to_ascii_lowercase();
        if tag_lower.starts_with("<input") {
            if let Some(name) = extract_attr_value(tag, "name") {
                if name == "deck" {
                    if let Some(val) = extract_attr_value(tag, "value") {
                        deck_values.push(val);
                    }
                }
            }
        }
    }

    if deck_values.is_empty() {
        return Err("HTML内に <input name=\"deck\" value=\"...\"> がありません。コードが無効、またはページ構造が変わった可能性があります。".to_string());
    }

    let mut parsed_decks = Vec::new();
    for val in deck_values {
        let deck: Value = serde_json::from_str(&val)
            .map_err(|e| format!("deck属性の値をJSONとして解析できませんでした: {}", e))?;
        let obj = deck
            .as_object()
            .ok_or_else(|| "deckがJSONオブジェクトではありません。".to_string())?;
        let mut normalized = serde_json::Map::new();
        for key in ["partner", "incident", "other"] {
            let arr = obj
                .get(key)
                .and_then(|v| v.as_array())
                .ok_or_else(|| format!("デッキ情報に '{}' がないか、配列ではありません。", key))?;
            if arr.iter().any(|c| c.as_str().map(|s| s.is_empty()).unwrap_or(true)) {
                return Err(format!("'{}' に不正なカード番号が含まれています。", key));
            }
            normalized.insert(key.to_string(), Value::Array(arr.clone()));
        }
        parsed_decks.push(Value::Object(normalized));
    }

    if parsed_decks.iter().any(|d| d != &parsed_decks[0]) {
        return Err("異なる内容のdeck属性が複数見つかりました。HTMLの確認が必要です。".to_string());
    }

    Ok(parsed_decks.into_iter().next().unwrap())
}

pub fn build_imported_deck(deck: Value) -> ImportedOfficialDeck {
    let partner_id = deck["partner"]
        .as_array()
        .and_then(|a| a.first())
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let case_id = deck["incident"]
        .as_array()
        .and_then(|a| a.first())
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let mut counts: Vec<(String, i32)> = Vec::new();
    if let Some(other) = deck["other"].as_array() {
        for item in other {
            if let Some(cid) = item.as_str() {
                if let Some(existing) = counts.iter_mut().find(|(id, _)| id == cid) {
                    existing.1 += 1;
                } else {
                    counts.push((cid.to_string(), 1));
                }
            }
        }
    }
    let main_deck = counts
        .into_iter()
        .map(|(definition_id, count)| DeckCardItem {
            definition_id,
            count,
        })
        .collect();

    ImportedOfficialDeck {
        partner_id,
        case_id,
        main_deck,
        raw: deck,
    }
}

pub fn extract_d_code_from_location(
    endpoint: &str,
    location: &str,
) -> Result<(String, Option<String>), String> {
    let base_url = url::Url::parse(endpoint).map_err(|_| "URL解析エラー".to_string())?;
    if let Ok(joined) = base_url.join(location) {
        let d_code = joined
            .query_pairs()
            .find(|(k, _)| k == "d_code")
            .map(|(_, code)| code.to_string());
        Ok((joined.to_string(), d_code))
    } else {
        Ok((location.to_string(), None))
    }
}

pub fn imported_deck_response_json(
    code: &str,
    imported: &ImportedOfficialDeck,
    saved: bool,
    deck_id: Option<&str>,
    deck_name: Option<&str>,
) -> Value {
    let mut map = serde_json::Map::new();
    map.insert("success".to_string(), json!(true));
    map.insert("saved".to_string(), json!(saved));
    if let Some(id) = deck_id {
        map.insert("deckId".to_string(), json!(id));
    }
    if let Some(name) = deck_name {
        map.insert("deckName".to_string(), json!(name));
    }
    map.insert("code".to_string(), json!(code));
    map.insert("partnerId".to_string(), json!(imported.partner_id));
    map.insert("caseId".to_string(), json!(imported.case_id));
    map.insert("mainDeck".to_string(), json!(imported.main_deck));
    map.insert("raw".to_string(), imported.raw.clone());
    Value::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_official_code_variants() {
        assert_eq!(extract_official_code(" Ab123Cd45 ").unwrap(), "Ab123Cd45");
        assert_eq!(
            extract_official_code(
                "https://www.takaratomy.co.jp/products/conan-cardgame/deckrecipe/create?d_code=XYZ123456"
            )
            .unwrap(),
            "XYZ123456"
        );
        assert!(extract_official_code("short").is_err());
        assert!(extract_official_code("invalid!@").is_err());
        assert!(extract_official_code("https://example.com/?foo=bar").is_err());
    }

    #[test]
    fn test_parse_official_deck_html_and_build_deck() {
        let html = r#"
        <html><body>
          <form>
            <input type="hidden" name="deck" value="{&quot;partner&quot;:[&quot;B01005P&quot;],&quot;incident&quot;:[&quot;B01005&quot;],&quot;other&quot;:[&quot;B01001&quot;,&quot;B01002&quot;,&quot;B01001&quot;]}">
          </form>
        </body></html>
        "#;
        let raw = parse_official_deck_html(html).unwrap();
        let imported = build_imported_deck(raw);
        assert_eq!(imported.partner_id, "B01005P");
        assert_eq!(imported.case_id, "B01005");
        assert_eq!(imported.main_deck.len(), 2);
        assert_eq!(imported.main_deck[0].definition_id, "B01001");
        assert_eq!(imported.main_deck[0].count, 2);
        assert_eq!(imported.main_deck[1].definition_id, "B01002");
        assert_eq!(imported.main_deck[1].count, 1);
    }

    #[test]
    fn test_parse_official_deck_html_errors() {
        assert!(parse_official_deck_html("<div>no input</div>").is_err());
        let mismatch = r#"
          <input name="deck" value='{"partner":["P1"],"incident":["I1"],"other":["C1"]}'>
          <input name="deck" value='{"partner":["P2"],"incident":["I1"],"other":["C1"]}'>
        "#;
        assert!(parse_official_deck_html(mismatch).is_err());
    }

    #[test]
    fn test_resolve_color_code_and_location() {
        assert_eq!(
            resolve_color_code(&["青".to_string(), "赤".to_string(), "青".to_string()]),
            "BR"
        );
        assert_eq!(resolve_color_code(&[]), "B");

        let (full, code) = extract_d_code_from_location(
            "https://www.takaratomy.co.jp/products/conan-cardgame/deckrecipe/store",
            "/products/conan-cardgame/deckrecipe/create?d_code=ABC123DEF",
        )
        .unwrap();
        assert_eq!(
            full,
            "https://www.takaratomy.co.jp/products/conan-cardgame/deckrecipe/create?d_code=ABC123DEF"
        );
        assert_eq!(code.as_deref(), Some("ABC123DEF"));
    }
}
