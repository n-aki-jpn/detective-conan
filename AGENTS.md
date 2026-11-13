# AGENTS.md

## 言語設定

- **必ず日本語で反応する**: 本エージェントは、ユーザーからの全ての問い合わせ・指示に対して日本語で応答するように設定されています。
- この設定は、エージェントの動作全般に適用され、他の言語での出力は行いません。
- 必要に応じて、プロジェクト内の他のエージェントやプラグインにも同様の設定を追加してください。

---

## プロジェクト概要

名探偵コナンTCG（トレーディングカードゲーム）の対戦Webアプリおよびデッキ構築ツールです。
ルールエンジン、カードカタログ管理、CPU対戦ロジック、デッキビルダー、モバイル対応Web UIを提供しています。

---

## アーキテクチャと技術スタック

1. **バックエンド（Rust）**
   - **クレート**: `conan_server` (`conan_server/` ディレクトリ)
   - **フレームワーク**: `axum` + `tokio` (非同期HTTPサーバー)
   - **主なモジュール**:
     - `src/models.rs`: カード・ゾーン・フェーズなどの型定義
     - `src/catalog.rs`: カードカタログ（`data/dummy/cards.json`）の読み込み・正規化
     - `src/engine.rs`: TCG対戦ルールエンジン（マリガン、ターン処理、行動、ガード、解決等）
     - `src/agent.rs`: CPU（RandomAgent）による自動意思決定ロジック
     - `src/observation.rs`: プレイヤー視点（手札秘匿など）のJSONシリアライズ
     - `src/main.rs`: REST APIエンドポイント提供および静的ファイル配信
   - **主なAPIエンドポイント**:
     - `GET /api/state?player_id=...&room_id=...`: 対戦盤面状態取得（マルチ部屋対応）
     - `GET /api/catalog`: 全カードカタログ情報取得
     - `GET /api/deck`: 現在のアクティブデッキ情報取得
     - `GET /api/decks`: 保存済みデッキ一覧取得
     - `GET /api/decks/:id`: 特定デッキ取得
     - `DELETE /api/decks/:id`: デッキスロット削除
     - `POST /api/decks/:id/select`: デッキ選択
     - `POST /api/rooms/create`: オンライン対戦部屋作成（4桁コード生成）
     - `POST /api/rooms/join`: 対戦部屋入室（P2参加）
     - `POST /api/decision`: プレイヤーの行動・意思決定送信
     - `POST /api/reset`: 対戦状態初期化
     - `POST /api/set_cpu`: CPU対戦の有効化・無効化
     - `POST /api/save_deck`: デッキ構築の検証および保存
     - 静的ファイル配信: `src/conan_engine/web/static` のHTML・カード画像をルート直下に配信

2. **フロントエンド（Vanilla HTML / JS / CSS）**
   - **配置先**: `src/conan_engine/web/static/`
     - `index.html`: モバイルファースト・レスポンシブWeb UI（対戦盤面 ＆ デッキビルダー）
     - `card-images/`: カード表面画像ファイル（`.jpg`）
   - **UIの重要要件**:
     - モバイル/PC両対応のレスポンシブデザイン（横幅を有効活用し、デッキ構築時は複数列グリッド表示）。
     - カードタップによる詳細モーダル表示（拡大画像、効果テキスト、ステータス）。
     - デッキ構築の40枚制限・同名最大3枚・パートナー/事件必須などのバリデーション。

3. **データリソース**
   - `data/dummy/cards.json`: 公式カード情報（2400枚以上のカードデータ）
   - `data/dummy/deck.json`: 現在の保存済みデッキデータ
   - `data/private/card-images/`: 原本画像ファイル保存先

---

## 開発・実行手順

### 1. サーバーのビルドと起動（Rust）
サーバーは **必ずプロジェクトルート（`C:\dev\detective-conan`）を作業ディレクトリとして実行** してください（相対パスで `data/` や `src/` を解決するため）。

```powershell
# プロジェクトルートに移動
Set-Location C:\dev\detective-conan

# デバッグビルド & 実行
cargo run --manifest-path conan_server/Cargo.toml

# またはリリースバイナリのビルドと実行
cargo build --release --manifest-path conan_server/Cargo.toml
.\conan_server\target\release\conan_server.exe 8000
```

### 2. アクセス確認
- ブラウザで `http://localhost:8000` にアクセス
- 上部ナビゲーションで「⚔️ 対戦」と「🃏 デッキ構築」を切り替え可能

---

## 注意事項・実装ルール

1. **パス依存関係**:
   - サーバープロセスはプロジェクトルートをカレントディレクトリとして起動することを前提としています。
2. **カードタイプの正規化**:
   - 生データ中の日本語表記（`キャラ`, `パートナー`, `事件`, `イベント`）は、フロントエンドおよびルールエンジンとの整合性を保つため、`CHARACTER`, `PARTNER`, `CASE`, `EVENT` に正規化・統一して扱います。
3. **対戦ロジックの変更時**:
   - 対戦ルールや判定処理を変更する際は、`conan_server/src/engine.rs` および `observation.rs` の両方で整合性が取れているか確認してください。
