from pathlib import Path
import sys, shutil
source = Path(__file__).resolve().parent / "docs"
root = Path(sys.argv[1]) if len(sys.argv) > 1 else Path.cwd()
target = root / "docs"
if not (target / "rules.md").is_file():
    raise SystemExit("v0.3のdocs/rules.mdがあるプロジェクトルートを指定してください")
marker = "<!-- ICON-UPDATE-v0.3.1 -->"
note = "\n\n" + marker + "\n## v0.3.1 優先追記\nU02・U03のアイコン確認は解決。旧版の未照合・保留記述に優先して、icon-mapping.md と resolution-timing-addendum.md を適用する。現場リムーブ時の処理順と条件・回数制限は同文書に従う。未確定事項は更新後open-issues.md参照。\n"
for name in ["rules.md", "resolution-timing.md", "state-machine.md", "card-model.md", "test-cases.md", "README.md", "CHANGELOG.md"]:
    path = target / name
    if path.exists() and marker not in path.read_text(encoding="utf-8"):
        backup = path.with_name(path.name + ".bak-v0.3.1")
        if not backup.exists(): shutil.copy2(path, backup)
        path.write_text(path.read_text(encoding="utf-8") + note, encoding="utf-8")
for path in source.iterdir():
    dest = target / path.name
    backup = dest.with_name(dest.name + ".bak-v0.3.1")
    if dest.exists() and not backup.exists(): shutil.copy2(dest, backup)
    shutil.copy2(path, dest)
print("v0.3.1の差分を適用しました")
