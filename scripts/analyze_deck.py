import json
import re

with open('scripts/conan-card-debug/response.raw.json', 'r', encoding='utf-8-sig') as f:
    raw_data = json.load(f)

raw_cards = raw_data.get('data', [])

# 1. Deduplicate by canonical card_num (ignoring P, Sec variants for definition)
# Actually, each card_num can be its definition or we can key by card_num.
# Let's see all unique card_num:
print(f"Total raw records: {len(raw_cards)}")

# Filter to unique card_num
unique_cards = {}
for c in raw_cards:
    num = c['card_num']
    if num not in unique_cards:
        unique_cards[num] = c

print(f"Unique card_num count: {len(unique_cards)}")

# Classify cards by type
partners = []
cases = []
characters = []
events = []

for num, c in unique_cards.items():
    t = c.get('type')
    if t == 'パートナー':
        partners.append(c)
    elif t == '事件':
        cases.append(c)
    elif t == 'キャラ':
        characters.append(c)
    elif t == 'イベント':
        events.append(c)

print(f"Partners: {len(partners)}, Cases: {len(cases)}, Characters: {len(characters)}, Events: {len(events)}")
for p in partners:
    print(f"Partner: {p['card_num']} {p['title']} ({p.get('color')})")
for cs in cases:
    print(f"Case: {cs['card_num']} {cs['title']} ({cs.get('color')}) diff={cs.get('difficulty_first')}/{cs.get('difficulty_second')}")
