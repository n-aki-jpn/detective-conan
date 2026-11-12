# Detective Conan Card Game Project

名探偵コナンカードゲームの実装仕様・検証用データ・対戦エンジン・シミュレーションスクリプトを管理するリポジトリです。

---

## 📁 ディレクトリ構成

```text
detective-conan/
├── README.md               # プロジェクト総合概要（本ファイル）
├── docs/                   # 仕様書・ルール設計・JSONスキーマ
│   ├── README.md           # 仕様書インデックス（v0.3）
│   ├── v0.3.1-update.md    # v0.3.1 差分更新仕様
│   ├── rules.md            # 基本ルール仕様
│   ├── floor-rules.md      # フロアルール要約
│   ├── resolution-timing.md # タイミング仕様
│   ├── resolution-timing-addendum.md # タイミング仕様追記
│   ├── state-machine.md    # 状態遷移モデル
│   ├── decisions.md        # 入出力契約・エンジンAPI設計
│   ├── api.md              # 通信API仕様
│   ├── card-model.md       # カードデータモデル
│   ├── card-schema.json    # カード定義スキーマ
│   ├── icon-condition-schema.json # アイコン条件スキーマ
│   ├── turn-limit-schema.json     # ターン回数制限スキーマ
│   ├── icon-mapping.md     # アイコン対応表
│   ├── icon-tests.md       # アイコンテスト仕様
│   ├── open-issues.md      # 未解決・確認課題
│   ├── logging-and-simulation.md # ログ・シミュレーション仕様
│   ├── test-cases.md       # 受入テストケース一覧
│   └── dummy-card-example.json   # カード定義例
│
├── src/
│   └── conan_engine/       # コア対戦エンジン (MVP実装)
│       ├── models.py       # カード・ゾーン・フェーズ等のデータ構造
│       ├── catalog.py      # カードカタログローダー
│       ├── engine.py       # ルールステートマシン & 対戦エンジン
│       └── agent.py        # CPUエージェント (RandomAgent)
│
├── tests/                  # 単体テスト・受入テスト
│   └── test_engine.py      # 初期引き直し、リソース配布、推理、アシスト等のテスト
│
├── data/                   # データ置き場
│   └── dummy/              # 検証用ダミーデータ (U06)
│       ├── README.md       # ダミーデッキ設計概要
│       ├── cards.json      # ダミーカード定義 (14種類40枚 + パートナー + 事件)
│       ├── deck.json       # デッキ構成定義
│       ├── deck-list.csv   # デッキリストCSV
│       ├── simulation-profile.json # シミュレーション設定
│       └── validation-report.json  # 検証レポート
│
└── scripts/                # ユーティリティ・実行スクリプト
    ├── simulate.py         # ヘッドレス高速対戦シミュレーター
    ├── save_conan_cards.py # 公式カードデータ取得・保存スクリプト
    ├── カードデータ保存手順.md  # データ保存スクリプトの実行手順書
    └── apply_update.py     # docs仕様書差分適用スクリプト
```

---

## 🚀 クイックスタート

### 1. 単体テストの実行
```bash
$env:PYTHONPATH="src"
python -m unittest discover -s tests -p "test_*.py"
```

### 2. 対戦シミュレーションの実行
ダミーデッキ同士のCPU対戦を高速に実行・検証できます。
```bash
python scripts/simulate.py --games 100 --seed 42
```
