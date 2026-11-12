import json

with open('scripts/conan-card-debug/response.raw.json', 'r', encoding='utf-8-sig') as f:
    raw_data = json.load(f)

blue_chars = []
for c in raw_data.get('data', []):
    if c.get('color') == '青' and c.get('type') == 'キャラ':
        blue_chars.append(c)

print(f"Blue characters: {len(blue_chars)}")
for c in blue_chars:
    print(f"{c['card_num']} | {c['title']} | cost={c.get('cost')} | ap={c.get('ap')} | lp={c.get('lp')} | feat={c.get('feature')}")
