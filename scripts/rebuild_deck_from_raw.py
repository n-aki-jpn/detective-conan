import json

with open('scripts/conan-card-debug/response.raw.json', 'r', encoding='utf-8-sig') as f:
    raw_json = json.load(f)

raw_cards = raw_json.get('data', [])

catalog_cards = []
seen_def_ids = set()

type_map = {
    'キャラ': 'CHARACTER',
    'パートナー': 'PARTNER',
    '事件': 'CASE',
    'イベント': 'EVENT'
}

for c in raw_cards:
    def_id = c['card_num']
    if def_id in seen_def_ids:
        continue
    seen_def_ids.add(def_id)

    raw_t = c.get('type')
    card_type = type_map.get(raw_t, 'CHARACTER')

    color = c.get('color') or '青'
    colors = [color]

    level = int(c['cost']) if c.get('cost') is not None else 0
    ap = int(c['ap']) if c.get('ap') is not None else 0
    lp = int(c['lp']) if c.get('lp') is not None else 0

    case_levels = None
    if card_type == 'CASE':
        diff_1 = int(c.get('difficulty_first')) if c.get('difficulty_first') is not None else 7
        diff_2 = int(c.get('difficulty_second')) if c.get('difficulty_second') is not None else 6
        case_levels = {"first": diff_1, "second": diff_2}

    # Gather feature, cut_in, hirameki, henso
    feature_parts = []
    if c.get('feature'):
        feature_parts.append(c['feature'])
    if c.get('cut_in'):
        feature_parts.append(c['cut_in'])
    if c.get('hirameki'):
        feature_parts.append(c['hirameki'])
    if c.get('henso'):
        feature_parts.append(c['henso'])

    desc = "\n".join(feature_parts)
    if not desc:
        if card_type == 'CHARACTER':
            desc = "テキストなし"
        elif card_type == 'CASE':
            desc = f"事件レベル: 先攻{case_levels['first']} / 後攻{case_levels['second']}"

    img_thumb = c.get('main_thumb')
    img_path = c.get('main_path')

    catalog_cards.append({
        "definitionId": def_id,
        "canonicalId": def_id,
        "dummyName": f"{c.get('title')} ({def_id})",
        "title": c.get('title'),
        "definitionVersion": "1",
        "cardType": card_type,
        "colors": colors,
        "level": level,
        "ap": ap,
        "lp": lp,
        "description": desc,
        "cutIn": c.get('cut_in'),
        "hirameki": c.get('hirameki'),
        "henso": c.get('henso'),
        "traits": [c.get('category1'), c.get('category2'), c.get('category3')] if c.get('category1') else [],
        "abilities": [],
        "caseLevels": case_levels,
        "mainThumb": img_thumb,
        "mainPath": img_path
    })

catalog_output = {
    "catalogVersion": "official-raw-v1",
    "totalCards": len(catalog_cards),
    "cards": catalog_cards
}

with open('data/dummy/cards.json', 'w', encoding='utf-8') as f:
    json.dump(catalog_output, f, ensure_ascii=False, indent=2)

print(f"Updated data/dummy/cards.json with complete text (cut_in, hirameki).")
