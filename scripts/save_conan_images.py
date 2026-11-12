import argparse
import hashlib
import io
import json
import os
import time
from pathlib import Path
from urllib.parse import quote, urlsplit
from urllib.request import Request, urlopen
from urllib.error import HTTPError

BASE = "https://www.takaratomy.co.jp/products/conan-cardgame/storage/card/"
REFERER = "https://www.takaratomy.co.jp/products/conan-cardgame/cardlist/"
SAMPLE = "194400fcb7d2fc.jpg"

def filename(value):
    if not isinstance(value, str) or not value.strip():
        return None
    value = value.strip()
    parts = urlsplit(value)
    if parts.scheme or parts.netloc:
        if not value.startswith(BASE):
            return None
        value = parts.path.rsplit("/", 1)[-1]
    if "/" in value or "\\" in value or value in (".", ".."):
        return None
    if Path(value).suffix.lower() not in (".jpg", ".jpeg", ".png", ".webp", ".gif", ".avif"):
        return None
    return value

def image_type(data):
    if data.startswith(b"\xff\xd8\xff"): return "jpeg"
    if data.startswith(b"\x89PNG\r\n\x1a\n"): return "png"
    if data[:6] in (b"GIF87a", b"GIF89a"): return "gif"
    if data[:4] == b"RIFF" and data[8:12] == b"WEBP": return "webp"
    if data[4:8] == b"ftyp" and (b"avif" in data[:40] or b"avis" in data[:40]): return "avif"
    return None

def main():
    p = argparse.ArgumentParser(description="Plan/download card images from saved official JSON. Default: plan only.")
    p.add_argument("input", help="Path to cards.all.json")
    p.add_argument("--fields", nargs="+", default=["main_path"])
    p.add_argument("--out", default="data/private/card-images")
    p.add_argument("--download", action="store_true")
    p.add_argument("--limit", type=int, default=0, help="0 means all planned unique images")
    p.add_argument("--delay", type=float, default=1.0)
    a = p.parse_args()
    if a.delay < 0.5 or a.limit < 0: p.error("delay >= 0.5 and limit >= 0 required")
    source = json.loads(Path(a.input).read_text(encoding="utf-8-sig"))
    cards = source.get("data") if isinstance(source, dict) else source
    if not isinstance(cards, list): raise ValueError("Expected card data array")
    if not all(isinstance(c, dict) for c in cards): raise ValueError("Invalid record")
    matches = [{"recordId": c.get("id"), "card_num": c.get("card_num"), "field": k} for c in cards for k,v in c.items() if isinstance(v,str) and v.endswith(SAMPLE)]
    plan = {}
    missing = 0
    for c in cards:
        for field in a.fields:
            name = filename(c.get(field))
            if not name:
                missing += 1
                continue
            url = BASE + quote(name, safe="")
            item = plan.setdefault(url, {"url": url, "filename": name, "references": []})
            item["references"].append({"recordId": c.get("id"), "card_num": c.get("card_num"), "card_id": c.get("card_id"), "field": field})
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    (out / "image-plan.json").write_text(json.dumps({"source":str(Path(a.input)),"fields":a.fields,"sampleFilenameMatches":matches,"missingOrRejectedValues":missing,"images":list(plan.values())},ensure_ascii=False,indent=2),encoding="utf-8")
    print(json.dumps({"uniqueImages":len(plan),"sampleFilenameMatches":matches,"plan":str(out / "image-plan.json"),"download":a.download},ensure_ascii=False,indent=2))
    if not a.download: return
    if not plan: raise ValueError("No usable filenames in selected fields")
    files = out / "files"
    files.mkdir(exist_ok=True)
    results = []
    items = list(plan.values())
    if a.limit: items = items[:a.limit]
    try:
        for i,item in enumerate(items,1):
            result = dict(item)
            dest = files / item["filename"]
            try:
                if dest.exists():
                    data = dest.read_bytes()
                    if not image_type(data): raise ValueError("Existing file is not a recognized image; inspect manually")
                    result["status"] = "existing"
                else:
                    req = Request(item["url"],headers={"Accept":"image/*", "Referer":REFERER, "User-Agent":"PersonalCardImageBackup/1.0"})
                    with urlopen(req,timeout=60) as r:
                        data = r.read(30 * 1024 * 1024 + 1)
                        content_type = r.headers.get("Content-Type","")
                    if len(data)>30*1024*1024: raise ValueError("Image exceeds size limit")
                    if not content_type.lower().startswith("image/") or not image_type(data):
                        raise ValueError("Response is not a recognized image")
                    temp = dest.with_name(dest.name+".part")
                    temp.write_bytes(data)
                    temp.replace(dest)
                    result["status"] = "saved"
                result.update(bytes=len(data),sha256=hashlib.sha256(data).hexdigest())
                print(f"{i}/{len(items)} {result['status']} {item['filename']}")
            except Exception as e:
                result.update(status="failed", error=str(e))
                print(f"{i}/{len(items)} failed {item['filename']}: {e}")
                if isinstance(e,HTTPError) and e.code in (401,403,429):
                    results.append(result)
                    raise RuntimeError("Stopped on authorization/rate-limit response; do not bypass restrictions") from e
            results.append(result)
            if i<len(items): time.sleep(a.delay)
    finally:
        summary={"planned":len(plan),"selected":len(items),"processed":len(results),"saved":sum(r['status']=='saved' for r in results),"existing":sum(r['status']=='existing' for r in results),"failed":sum(r['status']=='failed' for r in results),"results":results}
        (out / "image-manifest.json").write_text(json.dumps(summary,ensure_ascii=False,indent=2),encoding="utf-8")

if __name__ == "__main__":
    main()
