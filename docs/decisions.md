# 入出力契約（設計）
Commandはプレイヤーの意図、Eventは確定した結果。試行入力と確定イベントのログを分ける。

```ts
interface Decision {
  decisionId: string;
  revision: number;
  playerId: string;
  kind: string;
  options: unknown; // 種類別の検証済みスキーマ
}
interface DecisionAnswer {
  decisionId: string;
  revision: number;
  value: unknown;
}
```
種類: mulligan, mainAction, hintPlay, switch, guard, mislead, contactSupport, inspiration, effectOrder, targets, optionalEffect, reorder。
単一選択／部分集合／順列を区別。0枚許可とpassを明示。重複実体ID、選択数超過、不適切な対象を拒否。
contactSupportはkind=cutIn/disguise/pass。cutInにはカードinstanceIdとabilityIdが必要。
ヒントで取得後に本人へ更新観測を渡し、改めて使用を選ばせる。事前の一括確定を強制しない。
回答者は現在のdecisionPlayerのみ。revision・decisionId一致を検証。不正回答で状態変更しない。
ローカルCPUもこの契約に従う。legalActionsの巨大列挙に代え階層選択を使う場合、その選び方をagentVersionへ固定。

## エンジン
createMatch(config), advanceUntilDecisionOrEnd(), getPendingDecision(), observe(player), submitDecision(player,answer), getResult()。
自動処理は次の選択／終了／異常まで進める。合法手ゼロだからendTurnを捏造しない。
