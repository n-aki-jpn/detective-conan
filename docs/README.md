# 非公開カードゲーム実装仕様 v0.3
更新: 2026-11-02
以前の雛形をこのdocs一式で置換する。差分だけのパッチではない。

## 基準資料
M: 添付 rule_manual-1.pdf オフィシャルルールマニュアルVer.2.6。
F: 添付 floor_rule-1.pdf フロアルールVer.1.37（2026-10-31）。
E: 公式カード訂正 https://www.takaratomy.co.jp/products/conan-cardgame/errata/
N: 公式テキスト補足 https://www.takaratomy.co.jp/products/conan-cardgame/no_text/
前の会話で確認した資料に基づく要約。PDF原文・画像・公式カード一覧は同梱しない。
各規則のM/Fは公式資料の根拠。「設計」はアプリ独自の契約。未確定事項はopen-issues.md参照。

## 読む順
rules → resolution-timing → state-machine → decisions → api → card-model → schema → logging-and-simulation → test-cases。
MVPは基本ルールと能力なしダミーキャラから開始。特殊能力はsupportedHandlersで制限。
この成果物は仕様書であり実装ではない。JSON検証と仕様整合チェックのみ実施し、対戦テストは未実行。
