# ログ・CPU（設計）
RandomAgentへobserveと現在の選択肢だけを渡す。完全状態や秘密シードを渡さない。
loop: advanceUntilDecisionOrEnd→選択者を取得→そのagentが回答→検証・適用→記録。

ログを分離:
- serverPrivate: 完全状態、初期デッキ順、ゲーム乱数、処理フレーム、全イベント。管理者限定。
- playerView: 当人に合法的に見えた観測と過去の観測履歴。
- training: decisionId,観測直前,選択肢,回答,decisionPlayer,agentVersion,最終結果。
後で分かったカードを過去の観測へ埋め込まない。全知情報を方策入力へ混入させない。

metadata: schemaVersion,engineVersion,rulesVersion,catalogVersion,catalogHash,profileId,rulingAssumptions,matchId,eventSeq,decisionSeq,revision,turn,phase。
ゲーム乱数と各CPU乱数は別系列。乱数アルゴリズム版を固定。リプレイは記録入力で行いCPU再実行不要。
対人ゲームの秘密乱数は予測困難にしてサーバーに保持。シミュレーションは固定シード可。
例 simulate --games 100 --seed 42 --agent-a random --agent-b random。
階層選択か全合法手一様選択かはCPU仕様に記録。部分集合選択方法で分布が変わる。
上限はTRUNCATED、エラーはERROR、裁定未確定はNEEDS_RULING。公式引き分け／敗北へ勝手に変換しない。
正常勝敗、投了、大会決着、切断、特殊勝利、リフレッシュ敗北を区別。
評価は先後入替、正常終了率、CPU別勝率、処理速度、再生一致率。学習と評価は試合単位で分割。
ランダム対戦ログを増やすだけでは強いCPUへ自動的に学習されない。改良方策と学習工程は別途実装。
