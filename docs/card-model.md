# カードモデル（設計）
definitionId/canonicalIdは正規化したダミー同一カードID。印刷版は任意のprintIdから定義へ参照。
訂正適用後にダミー化。テキストなし版も正規定義を参照し、能力なしと見なさない [E,N]。
過去リプレイは当時のcatalogVersion/contentHashを固定。ライブの定義へ自動更新しない。
sourceRefsは根拠資料・箇所。Eの適用日とページ更新日を混同しない。

## 定義と実体
CardDefinitionは名称参照、色、元数値、能力。CardInstanceはinstanceId,definitionId,ownerId,zone,orientation,enteredTurn,faceUp,attachments,stackedCards,modifiers,usage。
名乗りとACTIVE/SLEEP/STUNは別管理。元値置換・加減算・付与能力を別レイヤー。
複合名はNAME_A_AND_B,NAME_A,NAME_B。特徴はTRAIT_*。表示文字列で絆判定しない。
使用可能エリアactiveZones、使用条件activationConditions、コストcosts、効果内条件resolutionConditionsを分ける。
canUseAction/isAbilityEffective/resolveEffectを分離。効果がない合法な使用を一律に除外しない。

## 移動原因
cause: rule/cost/effect/actionResult/switch/refresh。
sourceEffectId,sourceActionId,from,to,instanceIdをイベントに記録。
誘発マッチャーは原因ごとの明示表で判定。コストを効果扱いしない [M p.14]。

## 未解決効果
PendingEffect: effectId,ownerId,sourceInstanceId,abilityId,handlerId,triggerEventId,context。
発生源が離れても消さない。保存すべき発動時値と解決時に読む値は各handler契約で定義。
未知handlerは拒否。条件やparamsはhandler固有スキーマでも検証する。

## 終了処理
finishMatch(outcomes,reason,sourceEffectId)。事件解決以外の効果からも呼ぶ。
終了時の特殊勝利が存在する [N]。共通勝利処理を事件解決ボタン専用にしない。
