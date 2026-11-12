import argparse
import csv
import hashlib
import io
import json
import os
from pathlib import Path
from datetime import datetime, timezone
from urllib.request import Request, urlopen

URL = "https://www.takaratomy.co.jp/products/conan-cardgame/cardlist/cards"

def main():
    parser = argparse.ArgumentParser(description="Save and validate the official card-search JSON snapshot (no images).")
    parser.add_argument("--out", default="conan-card-snapshots")
    parser.add_argument("--input", help="Use a downloaded JSON file instead of fetching")
    args = parser.parse_args()
    if args.input:
        raw = Path(args.input).read_bytes()
        source = str(Path(args.input))
    else:
        request = Request(URL, headers={"User-Agent": "PersonalCardDataBackup/1.0", "Accept": "application/json", "Accept-Encoding": "identity"})
        with urlopen(request, timeout=120) as response:
            raw = response.read()
            source = response.geturl()
    obj = json.loads(raw.decode("utf-8-sig"))
    cards = obj.get("data") if isinstance(obj, dict) else None
    if not isinstance(cards, list) or not cards:
        raise ValueError("Expected a nonempty data array; no snapshot saved")
    if not all(isinstance(c, dict) and "card_num" in c and "card_id" in c for c in cards):
        raise ValueError("Unexpected card schema; no snapshot saved")
    if obj.get("next_page_url") or (obj.get("links") or {}).get("next"):
        raise ValueError("Paginated response detected; refusing to label this a full snapshot")
    total = obj.get("total", (obj.get("meta") or {}).get("total"))
    if total is not None and int(total) != len(cards):
        debug_dir = Path("conan-card-debug")
        debug_dir.mkdir(parents=True, exist_ok=True)

        # 取得結果を原文のまま退避。
        # 全件取得済みとして扱わない。
        raw_path = debug_dir / "response.raw.json"
        raw_path.write_bytes(raw)

        metadata = {
            key: value
            for key, value in obj.items()
            if key != "data"
        }

        diagnostic = {
            "received_count": len(cards),
            "reported_total": total,
            "top_level_keys": list(obj.keys()),
            "metadata": metadata,
            "first_record": {
                key: cards[0].get(key)
                for key in ("id", "card_id", "card_num")
            },
            "last_record": {
                key: cards[-1].get(key)
                for key in ("id", "card_id", "card_num")
            },
        }

        diagnostic_text = json.dumps(
            diagnostic,
            ensure_ascii=False,
            indent=2,
        )

        (debug_dir / "diagnostic.json").write_text(
            diagnostic_text,
            encoding="utf-8",
        )

        print(diagnostic_text)
        print(f"\n取得した原本を退避しました: {raw_path.resolve()}")

        raise SystemExit(
            "\n全件取得の確認待ちです。"
            "このファイルは完全なスナップショットとして扱わないでください。"
        )
    now = datetime.now(timezone.utc)
    dest = Path(args.out) / now.strftime("%Y%m%dT%H%M%S.%fZ")
    dest.mkdir(parents=True, exist_ok=False)
    (dest / "cards.raw.json").write_bytes(raw)
    (dest / "cards.readable.json").write_text(json.dumps(obj, ensure_ascii=False, indent=2), encoding="utf-8")
    (dest / "cards.jsonl").write_text("".join(json.dumps(c, ensure_ascii=False) + "\n" for c in cards), encoding="utf-8")
    keys = sorted({k for c in cards for k in c})
    with (dest / "cards.csv").open("w", encoding="utf-8-sig", newline="") as f:
        w = csv.DictWriter(f, fieldnames=keys)
        w.writeheader()
        for c in cards:
            w.writerow({k: json.dumps(v, ensure_ascii=False) if isinstance(v, (dict, list)) else v for k,v in c.items()})
    report = {"source": source, "fetchedAtUTC": now.isoformat(), "recordCount": len(cards), "distinctCardIds": len({str(c["card_id"]) for c in cards}), "distinctCardNumbers": len({str(c["card_num"]) for c in cards}), "rawBytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest(), "serverTotal": total, "completeness": "Server total matched" if total is not None else "Whole endpoint response saved; no server total provided, not independently verified against search UI", "imagesDownloaded": False, "fields": keys}
    (dest / "manifest.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps({"savedTo": str(dest.resolve()), **report}, ensure_ascii=False, indent=2))

if __name__ == "__main__":
    main()
