import json

with open('scripts/conan-card-debug/response.raw.json', 'r', encoding='utf-8-sig') as f:
    raw_data = json.load(f)

events = []
for c in raw_data.get('data', []):
    if c.get('type') == 'イベント':
        events.append(c)

print(f"Events: {len(events)}")
for c in events:
    print(f"{c['card_num']} | {c['title']} | cost={c.get('cost')} | color={c.get('color')} | feat={c.get('feature')}")
