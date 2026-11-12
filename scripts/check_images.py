import json

with open('scripts/conan-card-debug/response.raw.json', 'r', encoding='utf-8-sig') as f:
    data = json.load(f)

cards = data.get('data', [])

# Check images and thumbs
for c in cards[:5]:
    print(c['card_num'], c['title'], "thumb:", c.get('main_thumb'), "path:", c.get('main_path'))
