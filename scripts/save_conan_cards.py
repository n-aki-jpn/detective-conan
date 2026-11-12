import argparse
import csv
import hashlib
import json
import time
from datetime import datetime, timezone
from pathlib import Path
from urllib.error import HTTPError
from urllib.parse import urlencode
from urllib.request import Request, urlopen

BASE = "https://www.takaratomy.co.jp/products/conan-cardgame/cardlist/cards"

def download(url):
    headers = {"Accept": "application/json", "X-Requested-With": "XMLHttpRequest", "Referer": "https://www.takaratomy.co.jp/products/conan-cardgame/cardlist/", "User-Agent": "PersonalCardBackup/2.0"}
    for attempt in range(3):
        try:
            with urlopen(Request(url, headers=headers), timeout=90) as r:
                raw = r.read()
            return raw, json.loads(raw.decode("utf-8-sig"))
        except HTTPError as e:
            if e.code not in (429, 500, 502, 503, 504) or attempt == 2:
                raise
            time.sleep(5 * (attempt + 1))
        except (TimeoutError, ConnectionError):
            if attempt == 2:
                raise
            time.sleep(5 * (attempt + 1))

def metadata(obj, key):
    meta = obj.get("meta")
    return obj.get(key, meta.get(key) if isinstance(meta, dict) else None)

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default="conan-card-snapshots")
    ap.add_argument("--delay", type=float, default=1.0)
    ap.add_argument("--max-pages", type=int, default=500)
    a = ap.parse_args()
    if a.delay < 0.5 or a.max_pages < 1:
        ap.error("delay must be >= 0.5 and max-pages >= 1")
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ")
    dest = Path(a.out) / stamp
    pages = dest / "pages"
    pages.mkdir(parents=True, exist_ok=False)
    report = {"source": BASE, "createdAtUTC": stamp, "complete": False, "pages": [], "reportedTotal": None, "receivedCount": 0}
    rows = []
    fingerprints = set()
    totals = set()
    expected_last = None
    try:
        for page in range(1, a.max_pages + 1):
            url = BASE + "?" + urlencode({"page": page})
            raw, obj = download(url)
            (pages / f"page-{page:04}.json").write_bytes(raw)
            if not isinstance(obj, dict) or not isinstance(obj.get("data"), list):
                raise ValueError("Unexpected response schema on page " + str(page))
            batch = obj["data"]
            if not all(isinstance(c, dict) and "card_id" in c and "card_num" in c for c in batch):
                raise ValueError("Unexpected card schema")
            current = metadata(obj, "current_page")
            if current is not None and int(current) != page:
                raise ValueError("Server did not return requested page")
            total = metadata(obj, "total")
            if total is not None:
                totals.add(int(total))
                if len(totals) > 1:
                    raise ValueError("Server total changed during download; retry later")
                report["reportedTotal"] = int(total)
            last = metadata(obj, "last_page")
            if last is not None:
                if expected_last is not None and expected_last != int(last):
                    raise ValueError("Last page changed during download")
                expected_last = int(last)
            if batch:
                fingerprint = hashlib.sha256(json.dumps(batch, sort_keys=True, ensure_ascii=False).encode()).hexdigest()
                if fingerprint in fingerprints:
                    raise ValueError("Repeated page detected; pagination may not be honored")
                fingerprints.add(fingerprint)
            rows.extend(batch)
            report["pages"].append({"page": page, "url": url, "count": len(batch), "sha256": hashlib.sha256(raw).hexdigest()})
            report["receivedCount"] = len(rows)
            print(f"page={page} count={len(batch)} accumulated={len(rows)} total={total}")
            next_known = "next_page_url" in obj
            next_value = obj.get("next_page_url")
            links = obj.get("links")
            if not next_known and isinstance(links, dict) and "next" in links:
                next_known, next_value = True, links["next"]
            terminal = (expected_last is not None and page >= expected_last) or (next_known and next_value is None) or not batch
            if terminal:
                if expected_last is not None and page < expected_last:
                    raise ValueError("Response ended before last_page")
                break
            time.sleep(a.delay)
        else:
            raise ValueError("Maximum page limit reached before completion")
        if report["reportedTotal"] is not None and len(rows) != report["reportedTotal"]:
            raise ValueError("Accumulated count differs from total")
        ids = [str(c["id"]) for c in rows if c.get("id") is not None]
        if len(ids) != len(set(ids)):
            raise ValueError("Duplicate server record IDs across pages; snapshot may have shifted")
        if not rows:
            raise ValueError("No card records received")
        (dest / "cards.all.json").write_text(json.dumps({"data": rows}, ensure_ascii=False, indent=2), encoding="utf-8")
        (dest / "cards.all.jsonl").write_text("".join(json.dumps(c, ensure_ascii=False) + "\n" for c in rows), encoding="utf-8")
        keys = sorted({k for c in rows for k in c})
        with (dest / "cards.all.csv").open("w", encoding="utf-8-sig", newline="") as f:
            writer = csv.DictWriter(f, fieldnames=keys)
            writer.writeheader()
            for c in rows:
                writer.writerow({k: json.dumps(v, ensure_ascii=False) if isinstance(v, (dict, list)) else v for k, v in c.items()})
        report.update(complete=True, distinctCardIds=len({str(c["card_id"]) for c in rows}), distinctCardNumbers=len({str(c["card_num"]) for c in rows}), completenessBasis="Pagination termination and server total if provided; not independently compared with website UI")
        print("Saved:", dest.resolve())
    except Exception as e:
        report["error"] = str(e)
        print("Incomplete. Raw pages preserved:", dest.resolve())
        raise
    finally:
        (dest / "manifest.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")

if __name__ == "__main__":
    main()
