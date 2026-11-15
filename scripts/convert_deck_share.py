import base64
import json
import os
import re
import sys
from pathlib import Path

DEFAULT_SHARE_TEXT = """【名探偵コナンTCG デッキ共有】
デッキ名: 【プリセット】6色混色 (40枚)
CONAN1:eyJuIjoi44CQ44OX44Oq44K744OD44OI44CRNuiJsua3t-iJsiIsInAiOiJCMDEwMDEiLCJjIjoiUFIwNjciLCJtIjoiQjAxMDY5OjMsQjAyMDM5OjIsQjA1MDM3OjMsQjA3MDA1OjMsQjA4MDI4OjMsQjA5MDY0OjEsQjEwMDQwOjMsQjExMDA3UDozLEIxMTAxNTozLEIxMTA2MTozLEQwMjAwODozLEQwNjAyMzoyLEQwODAyMToyLFBSMTk3OjMsUFIyMDc6MyJ9"""

def decode_conan_share(text: str) -> dict:
    match = re.search(r"CONAN1:([A-Za-z0-9_-]+)", text)
    if not match:
        raise ValueError("テキスト内に有効な CONAN1: 共有コードが見つかりませんでした。")
    
    b64 = match.group(1)
    # URLセーフBase64のデコード対応 & パディング補完
    rem = len(b64) % 4
    if rem:
        b64 += "=" * (4 - rem)
    
    raw_json = base64.urlsafe_b64decode(b64).decode("utf-8")
    data = json.loads(raw_json)
    
    deck_name = data.get("n", "カスタムデッキ")
    partner_id = data.get("p", "")
    case_id = data.get("c", "")
    
    main_deck = []
    # "B01069:3,B02039:2,..."
    if "m" in data and data["m"]:
        for item in data["m"].split(","):
            parts = item.split(":")
            if len(parts) == 2:
                cid, cnt = parts[0], int(parts[1])
                main_deck.append({
                    "definitionId": cid,
                    "count": cnt
                })
    
    deck_id = re.sub(r"[^\w\-]", "_", deck_name).strip("_")
    if not deck_id:
        deck_id = "custom_deck"

    return {
        "deckId": deck_id,
        "deckName": deck_name,
        "partnerId": partner_id,
        "caseId": case_id,
        "mainDeck": main_deck
    }

def main():
    share_text = sys.argv[1] if len(sys.argv) > 1 else DEFAULT_SHARE_TEXT
    deck_obj = decode_conan_share(share_text)
    
    # git管理外の data/custom_decks/ に出力
    out_dir = Path("data/custom_decks")
    out_dir.mkdir(parents=True, exist_ok=True)
    
    filename = f"{deck_obj['deckId']}.json"
    out_path = out_dir / filename
    
    with open(out_path, "w", encoding="utf-8") as f:
        json.dump(deck_obj, f, ensure_ascii=False, indent=2)
        
    print(f"✅ デッキJSONを生成しました: {out_path}")
    print(f"   デッキ名: {deck_obj['deckName']}")
    print(f"   パートナー: {deck_obj['partnerId']}, 事件: {deck_obj['caseId']}")
    total_cards = sum(item["count"] for item in deck_obj["mainDeck"])
    print(f"   枚数: {total_cards}枚 ({len(deck_obj['mainDeck'])}種)")

if __name__ == "__main__":
    main()
