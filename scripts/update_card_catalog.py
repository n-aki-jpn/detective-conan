import json, pathlib, sys

# Paths
raw_path = pathlib.Path(r"C:\dev\detective-conan\data\raw\conan-card\20261113\cards.all.json")
output_path = pathlib.Path(r"C:\dev\detective-conan\data\dummy\cards.json")

if not raw_path.exists():
    print(f"Raw file not found: {raw_path}", file=sys.stderr)
    sys.exit(1)

with raw_path.open(encoding='utf-8') as f:
    raw = json.load(f)

# Expect raw structure: {"data": [ { ... } , ... ] }
cards = []
for entry in raw.get('data', []):
    # Map fields to the format expected by CardCatalog.load_from_json
    definition_id = entry.get('card_num') or entry.get('definitionId')
    if not definition_id:
        continue

    # Level is in 'cost' in raw data, description is in 'feature'
    cost_val = entry.get('cost')
    level = 0
    if cost_val is not None:
        try:
            level = int(cost_val)
        except (ValueError, TypeError):
            level = 0

    ap_val = entry.get('ap')
    ap = 0
    if ap_val is not None:
        try:
            ap = int(ap_val)
        except (ValueError, TypeError):
            ap = 0

    lp_val = entry.get('lp')
    lp = 0
    if lp_val is not None:
        try:
            lp = int(lp_val)
        except (ValueError, TypeError):
            lp = 0

    desc = entry.get('feature') or entry.get('description') or ''

    # Case level difficulty for case cards
    diff_first = entry.get('difficulty_first')
    diff_second = entry.get('difficulty_second')
    case_levels = None
    if diff_first or diff_second:
        try:
            case_levels = {
                "first": int(diff_first) if diff_first else 6,
                "second": int(diff_second) if diff_second else 6
            }
        except Exception:
            case_levels = None

    canonical_id = entry.get('card_id') or entry.get('card_num') or definition_id
    sameid_limit = entry.get('rcp_sameid_limit', 3)
    try:
        sameid_limit = int(sameid_limit)
    except Exception:
        sameid_limit = 3

    card = {
        "definitionId": definition_id,
        "canonicalId": str(canonical_id),
        "sameIdLimit": sameid_limit,
        "dummyName": f"{entry.get('title','')} ({definition_id})",
        "title": entry.get('title',''),
        "definitionVersion": entry.get('version','1'),
        "cardType": entry.get('type','CHARACTER').upper(),
        "colors": [entry.get('color')] if entry.get('color') else [],
        "level": level,
        "ap": ap,
        "lp": lp,
        "description": desc,
        "caseLevels": case_levels,
        "mainThumb": entry.get('main_thumb',''),
        "mainPath": entry.get('main_path',''),
        "subThumb": entry.get('sub_thumb',''),
        "subPath": entry.get('sub_path','')
    }
    cards.append(card)

output = {
    "catalogVersion": "official-raw-v1",
    "totalCards": len(cards),
    "cards": cards
}

output_path.parent.mkdir(parents=True, exist_ok=True)
with output_path.open('w', encoding='utf-8') as f:
    json.dump(output, f, ensure_ascii=False, indent=2)
print(f"Wrote {len(cards)} cards to {output_path}")
