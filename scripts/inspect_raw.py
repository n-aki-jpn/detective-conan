import json

with open('scripts/conan-card-debug/response.raw.json', 'r', encoding='utf-8-sig') as f:
    data = json.load(f)

cards = data.get('data', [])
print(f"Total cards: {len(cards)}")
for i, c in enumerate(cards):
    t = c.get('type')
    num = c.get('card_num')
    title = c.get('title')
    cost = c.get('cost')
    ap = c.get('ap')
    lp = c.get('lp')
    color = c.get('color')
    feature = c.get('feature')
    first = c.get('difficulty_first')
    sec = c.get('difficulty_second')
    print(f"[{i+1}] {t} | {num} | {title} | cost={cost} | ap={ap} | lp={lp} | color={color} | diff={first}/{sec} | feature={feature}")
