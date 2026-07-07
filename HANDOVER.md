# 名探偵コナン TCG プロトタイプ 引き継ぎ書

## プロジェクト概要
- **プロジェクト名**: Detective Conan TCG Phase 1 Prototype
- **バージョン**: 1.0.0
- **開発フェーズ**: Phase 1（ターン進行状態マシンとカード移動メカニクス）
- **目的**: ホットシートモードのサンドボックスTCGアプリ実装
- **言語設定**: 思考・回答・ログは日本語（settings.mdで設定）

## 実装済みの機能
- **HTML構造**: index.html - CSS Gridレイアウト、全ゲームゾーン（Case, Partner, Deck, FILE, Evidence, Field, Hand）
- **スタイリング**: style.css - プレミアムダークテーマ、Glassmorphism、カード状態（active/sleep/stun/face-down）
- **カードデータベース**: cards.js - Partners（Conan, Heiji）、Cases、Characters（Blue/Green/White/Red/Yellow）、Events
- **状態管理**: state.js - ターン進行状態マシン（AUTO/MAIN/GUARD/ENDフェイズ）、ゲームフェーズ（事件編/解決編）、カード移動、プレイ検証、ネクストヒント、推理、アクション、ガードフェーズ
- **UIレンダリング**: app.js - イベントハンドラー、ドラッグ＆ドロップ、ズームモーダル、コンテキストメニュー、サンドボックスツール、相手の情報表示

## 直近の修正内容

### 問題1: アクションボタンが動作しない
**原因**: メニューが表示されない問題（CSSのpositionとz-index、body overflow hiddenの影響）

**修正**:
- style.cssで`.context-menu`の`position`を`fixed`、`z-index`を`9999`に変更
- app.jsでメニュー表示時に一時的に`body`の`overflow`を`visible`に変更
- `addMenuOption`関数に`closeMenu`パラメータを追加して、アクションメニューを開く際にメニューを閉じないように修正

### 問題2: ガードフェーズの実装
**修正**:
- state.jsに`GUARD`フェーズと`guardPhase`オブジェクトを追加
- `performAction`関数を修正して、相手にアクティブなキャラがいる場合にガードフェーズを開始するように変更
- `performGuard`関数と`skipGuard`関数を追加
- app.jsにガードフェーズのUIを実装（ガード選択メニュー、ガードスキップボタン）
- HTMLにGUARDフェーズ表示と「ガードしない」ボタンを追加

### 問題3: ガードフェーズのコンタクトロジック修正
**原因**: ターゲットキャラと攻撃キャラでAP比較していたが、ガードキャラと攻撃キャラで比較すべき

**修正**:
- `performGuard`関数でガードキャラと攻撃キャラでAP比較するように修正
- ターゲットが「事件」の場合、攻撃者がガードキャラに勝った場合のみ証拠を奪うように修正

### 問題4: パートナーカードがリフレッシュフェイズでパートナーエリアに戻らない
**修正**:
- `startTurn`関数でパートナーがFILEエリアにある場合、自動的にパートナーエリアに戻す処理を追加

### 問題5: 証拠にアクションした時もガードフェーズを開始する
**修正**:
- `performAction`関数で証拠へのアクションでも相手にアクティブなキャラがいる場合にガードフェーズを開始するように修正

### 問題6: 事件編・解決編の実装
**修正**:
- state.jsに`GAME_PHASES`（`INCIDENT`、`SOLUTION`）と`currentGamePhase`を追加
- `moveCard`関数でパートナーがFILEエリアへ移動した時にFILEが6枚の場合、解決編に移行するロジックを追加
- HTMLにゲームフェーズ表示を追加
- app.jsでゲームフェーズの表示を更新するように修正

### 問題7: 画面表示の問題（相手のFILE数・証拠数・手札数が見えない）
**修正**:
- HTMLに相手の情報表示エリアを追加
- CSSで相手の情報表示エリアのスタイルを追加
- app.jsで相手のFILE数、証拠数、手札数を更新するように修正

### 問題8: カードデータとプレイ検証の色ルールが正しくない
**原因**: 
- generateStarterDeck関数でWhite/Red/Yellowのカードがすべての山札に含まれていた
- canPlayCardFromHand関数でWhite/Red/Yellowのカードがどの事件カードでもプレイできるようになっていた

**修正**:
- cards.jsのgenerateStarterDeck関数で、事件カードの色と一致するカードのみ山札に含めるように修正
- state.jsのcanPlayCardFromHand関数で、事件カードの色と一致しないカードはプレイできないように修正（怪盗キッドの「神出鬼没」能力は例外として実装）

### 問題9: 勝利条件と解決編条件が公式ルールと異なる
**原因**: 
- 勝利条件と解決編条件がパートナーカードごとに異なると誤認していたが、実際は全パートナー共通
- 解決編条件がFILE 6枚になっていたが、公式ルールではFILE 7枚
- 勝利条件の証拠枚数がパートナーごとに固定されていたが、実際は事件レベルによる

**修正**:
- cards.jsのパートナーカードのsolutionCondition.fileCountを7に統一
- cards.jsのパートナーカードのvictoryCondition.evidenceCountをnullに変更（事件レベルは事件カードによる）
- cards.jsのパートナーカードのvictoryCondition.requiresSolutionPhaseをtrueに追加
- cards.jsのパートナーカードのabilityTextを更新して【アシスト】能力を明記
- state.jsのcheckGameWinningCondition関数で解決編状態のチェックを追加
- state.jsのcheckGameWinningCondition関数で証拠枚数のチェックを事件レベル（先攻/後攻で異なる）に変更
- state.jsのmoveCard関数の解決編移行条件をFILE 7枚に固定

### 問題10: ゲームフェーズが全プレイヤーで共有されていた
**原因**: 
- currentGamePhaseがグローバル変数として管理されていたが、実際は各プレイヤーごとに独立したゲームフェーズを持つ
- パートナーがFILEエリアに移動した時の解決編移行判定で、partnerがnullになった後にチェックしていたため判定が失敗していた

**修正**:
- state.jsのcreateEmptyPlayerStateにgamePhaseプロパティを追加（初期値はINCIDENT）
- state.jsのreset関数からcurrentGamePhaseを削除（各プレイヤーのgamePhaseを使用）
- state.js of checkGameWinningConditionでthis.currentGamePhaseをplayer.gamePhaseに変更
- state.jsのmoveCardでパートナー移動前にフラグを設定し、targetPlayer.gamePhaseを使用して判定
- app.js of renderBoardでstate.currentGamePhaseをactivePlayer.gamePhaseに変更

### 問題11: 山札消費時のリフレッシュ処理と山札切れ時の敗北処理
**原因**:
- 山札がなくなったときに、リムーブエリアのカードをシャッフルして山札にするリフレッシュ処理が実装されていなかった。
- 山札がなくなったときにリムーブエリアが0枚のときの敗北処理も存在しなかった。

**修正**:
- `state.js` に山札からポップしてリフレッシュ・敗北を処理する `popCardFromDeck(playerIndex)`、リムーブエリアをシャッフルして山札にする `refreshDeckFromRemoveArea(playerIndex)`、敗北宣言を行う `declareDefeat(playerIndex, reason)` を実装。
- ドロー処理 (`drawCard`)、ターン開始時のFILEセット処理、推理アクション時の証拠獲得処理において、山札からカードを取り出す処理をすべて `popCardFromDeck` に置き換え。これによって、山札が空のときに自動的にリムーブエリアからシャッフルしてリフレッシュされるようになり、リムーブエリアも空なら敗北（ゲーム終了と敗北アナウンス）になるように対応。

### 問題12: ガードフェーズ終了後にアクションしたプレイヤーに表示が戻らない
**原因**:
- `performGuard`関数と`skipGuard`関数で、`guardPhase`をリセットした後に`this.guardPhase.attackerPlayerIndex`を参照していたため、`attackerPlayerIndex`が`null`になり、`turnOwner`も`null`になっていた。

**修正**:
- `guardPhase`をリセットする前に`attackerPlayerIndex`をローカル変数に保存し、その保存した変数を使用して`turnOwner`を設定するように修正。

## 次に確認すべきこと

1. **動作確認**: ブラウザをハードリロード（Cmd+Shift+R）して以下を確認
   - アクションボタンをクリックしてメニューが表示されるか
   - ガードフェーズが正しく動作するか（ガードキャラと攻撃キャラでコンタクトするか）
   - パートナーがリフレッシュフェイズでパートナーエリアに戻るか
   - 証拠にアクションした時もガードフェーズが開始されるか
   - パートナーの解決編条件（FILE枚数）を満たした状態でパートナーがFILEエリアへ移動した時に解決編に移行するか
   - パートナーの勝利条件（証拠枚数）を満たした状態で勝利宣言ができるか
   - 相手のFILE数、証拠数、手札数が表示されるか
   - 山札に事件カードと異なる色のカードが含まれていないか
   - 事件カードと異なる色のキャラ/イベントカードがプレイできないか（怪盗キッドを除く）
   - リムーブエリアをクリックして、リムーブされているすべてのカードを一覧で確認できるか

## ファイル構成
```
conan-tcg-prototype/
├── index.html          # メインHTML構造（スクリプト読み込み順序: cards.js → state.js → app.js）
├── style.css           # プレミアムスタイリング
├── cards.js            # カードデータベース
├── state.js            # ターン進行状態マシン（GUARDフェーズ、ゲームフェーズ追加済み）
├── app.js              # イベントハンドラーとUIレンダリング（ガードフェーズUI、相手情報表示追加済み）
├── settings.md         # 日本語設定ファイル（引き継ぎ書更新ルール追加済み）
└── HANDOVER.md         # この引き継ぎ書
```

## 重要な技術的詳細

### コールバック登録の仕組み
- app.jsで `state.subscribe(renderBoard)` を呼び出してコールバックを登録
- state.jsの `triggerStateChange()` が登録されたコールバックを呼び出す
- **重要**: reset()関数でコールバックをクリアしないように修正済み

### ゲームルール
- **ターン構造**: AUTOフェイズ → MAINフェイズ → ENDフェイズ
- **ガードフェーズ**: アクション時に相手にアクティブなキャラがいる場合、ガードフェーズを開始
- **ゲームフェーズ**: 初期状態は事件編、FILE6でパートナーがFILEエリアへ移動すると解決編に移行
- **手札プレイ制限**: 1ターンに1回（ネクストヒントで追加可能）
- **コスト**: カードレベル ≤ FILE枚数
- **色ルール**: 事件カードの色と一致（白/赤/黄は例外）
- **名乗り**: 登場ターンは推理/アクション不可
- **ガード**: ガードキャラが攻撃者のAPを-1000し、ガードキャラと攻撃キャラでコンタクト

### サンドボックスツール
- 山札ドロー
- FILE追加
- 証拠追加
- ゲームリセット
- プレイヤー切替（ホットシートモード）

## 開発環境
- **サーバー**: Python http.server（ポート8080）
- **ブラウザプレビュー**: http://localhost:8080
- **起動コマンド**: `python3 -m http.server 8080`

## 現在のTODOリスト
- [x] アクションボタンが動作しない問題を調査・修正する（メニュー表示は修正完了）
- [x] ガードフェーズを実装する（state.jsにガード状態管理を追加）
- [x] ガードフェーズのUIを実装する（app.jsにガード選択UIを追加）
- [x] ガードフェーズのコンタクトロジックを修正する（ガードキャラと攻撃キャラでコンタクトするように修正）
- [x] パートナーカードがリフレッシュフェイズでパートナーエリアに戻らない問題を修正する
- [x] 証拠にアクションした時もガードフェーズを開始するように修正する
- [x] 事件編・解決編の状態管理を追加する（state.jsにgamePhaseを追加）
- [x] FILE6でパートナーがFILEエリアへ移動した時に解決編に移行するロジックを追加する
- [x] UIに事件編・解決編の表示を追加する
- [x] 画面表示の問題を調査・修正する（FILE数・証拠数・手札数が見えない）
- [x] デバッグログを削除する
- [x] settings.mdに引き継ぎ書更新の指示を追加する
- [x] 引き継ぎ書を更新する
- [x] カードデータとプレイ検証の色ルールを修正する
- [x] 勝利条件と解決編条件を公式ルールに合わせて修正する
- [x] 山札がなくなったときにリムーブエリアからシャッフルしてリフレッシュする処理、およびリムーブエリアも空なら敗北となる処理を修正・実装
- [x] ガードフェーズ終了後にアクションしたプレイヤーに表示が戻らない問題を修正（guardPhaseリセット前にattackerPlayerIndexを保存するように修正）
- [x] カットイン機能を実装（HTML/CSS/JS、登場・推理・アクション・ガード・勝利・敗北時のカットイン）
- [x] コンタクト中のカットイン・変装機能を実装（state.jsにuseCutIn/useDisguise関数、app.jsにUI、カードデータにcutIn/disguiseプロパティ）
- [x] モバイル対応を実装（タッチイベント対応、レスポンシブCSS、ビューポート調整）
- [x] 「ガードしない」ボタンを自分(Player1)の現場という文字列の横に移動

## 注意事項
- 毎回作業終了時に必ず引き継ぎ書（HANDOVER.md）を更新すること（settings.md参照）
- 日本語で思考・回答・ログを行うこと（settings.md参照）
