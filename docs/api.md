# 通信API（設計）
Node.js/TypeScript想定。小型の権威サーバーとヘッドレスで同じengineを使う。
createRoom/joinRoom/submitDeck/ready/reconnectとsubmitDecision。
部屋作成時にルール・カードカタログ・rulingAssumptionsを固定。
認証済み接続にplayerIdを結付ける。クライアントのplayerIdを本人確認に使わない。

```json
{"requestId":"r1","matchId":"m1","type":"submitDecision","payload":{"decisionId":"d7","revision":12,"value":{"kind":"pass"}}}
```
requestId再送は冪等。古いrevision/decisionIdは拒否して再同期。リトライで2回コストを払わない。
stateUpdateはrevision,turnPlayerId,decisionPlayerId,phase,observation,pendingDecision。
観測・合法な選択肢は宛先別に生成。サーバー完全状態をJSON化してクライアントで隠す方式は禁止。
相手手札・デッキ順・秘密カードのdefinitionId・シード・内部完全状態ハッシュを送らない。
表示用opaqueHandleと内部instanceIdを分離し、シャッフル後の秘密カードをIDで追跡させない。
通常ドロー／FILE配置は自動イベント。任意drawCard APIは作らない。
エラー: NOT_DECISION_PLAYER/STALE_DECISION/INVALID_TARGET/UNSUPPORTED_RULE/MATCH_FINISHED。
切断猶予・復帰期限はアプリ独自設定。公式の敗北条件と混同せず結果理由に記録。
