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
    card = {
        "definitionId": definition_id,
        "canonicalId": entry.get('card_num') or definition_id,
        "dummyName": f"{entry.get('title','')} ({definition_id})",
        "title": entry.get('title',''),
        "definitionVersion": entry.get('version','1'),
        "cardType": entry.get('type','CHARACTER').upper(),
        "colors": [entry.get('color')] if entry.get('color') else [],
        "level": entry.get('level', 0),
        "ap": entry.get('ap', 0),
        "lp": entry.get('lp', 0),
        "description": entry.get('description',''),
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
