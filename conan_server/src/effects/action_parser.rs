use super::*;

pub fn parse_target_step(
    sentence: &str,
    dynamic_level: Option<i32>,
    _source_card_type: CardType,
) -> Option<TargetStep> {
    // 特殊0: 「このイベントを〜1枚にセットする」「このイベントをそのキャラにセットする」
    if sentence.contains("このイベントを") && sentence.contains("にセットする") {
        let after = sentence.split_once("このイベントを").map(|x| x.1)?;
        let target_phrase = after.split("にセットする").next()?.trim();
        let mut filter = if target_phrase == "そのキャラ" {
            TargetFilter {
                target_prev_chosen: true,
                owner: OwnerScope::SelfOnly,
                ..Default::default()
            }
        } else {
            let mut f = parse_target_filter(target_phrase, TargetZone::Field, true, dynamic_level);
            f.owner = OwnerScope::SelfOnly;
            f
        };
        filter.count = 1;
        return Some(TargetStep {
            action: TargetAction::SetThisEvent,
            filter,
            bonuses: Vec::new(),
            follow_up: Vec::new(),
            conditional_follow_up: None,
            optional: false,
            then_abort_if_skipped: false,
            scale_by_cost_flipped_evidence: false,
        });
    }

    // 特殊1: 「ターン終了時までそのキャラのカード名を、自分の現場にいるそのキャラ以外のレベル8以下のキャラ1枚のカード名に書き換えてもよい」（B09052）
    if sentence.contains("そのキャラのカード名を、") && sentence.contains("カード名に書き換えてもよい") {
        let scope = sentence
            .split_once("カード名を、")
            .map(|x| x.1)
            .unwrap_or(sentence);
        let target_phrase = scope.split("のカード名に書き換").next().unwrap_or(scope);
        let mut filter = parse_target_filter(target_phrase, TargetZone::Field, true, dynamic_level);
        filter.owner = OwnerScope::SelfOnly;
        filter.exclude_prev_chosen = true;
        filter.up_to = true;
        return Some(TargetStep {
            action: TargetAction::CopyNameFromOtherSelfChar,
            filter,
            bonuses: Vec::new(),
            follow_up: Vec::new(),
            conditional_follow_up: None,
            optional: true,
            then_abort_if_skipped: false,
            scale_by_cost_flipped_evidence: false,
        });
    }

    // 特殊2: 「〜そのキャラをAP＋3000し、〚突撃...〛を与え、自分のデッキのカードを上から1枚裏向きでそのキャラにセットする」（B07058 / B03133 / D09025 / D11019）
    if !sentence.contains("選び")
        && !sentence.contains("手札から")
        && (sentence.contains("そのキャラに") || sentence.contains("そのキャラを"))
    {
        let bonuses = extract_bonuses_from_action(sentence);
        if let Some(primary) = parse_target_action(sentence) {
            let mut f = TargetFilter {
                target_prev_chosen: true,
                owner: OwnerScope::SelfOnly,
                ..Default::default()
            };
            let _ = &mut f;
            return Some(TargetStep {
                action: primary,
                filter: f,
                bonuses,
                follow_up: Vec::new(),
                conditional_follow_up: None,
                optional: sentence.contains("てもよい"),
                then_abort_if_skipped: false,
                scale_by_cost_flipped_evidence: false,
            });
        }
    }

    // 選択対象フレーズとアクションフレーズに分割
    let (target_phrase, action_phrase) = if let Some(idx) = sentence.find("選び、") {
        (&sentence[..idx], &sentence[idx + "選び、".len()..])
    } else if let Some(idx) = sentence.find("選び") {
        (&sentence[..idx], &sentence[idx + "選び".len()..])
    } else if let Some(idx) = sentence.find("手札から") {
        let rest = &sentence[idx..];
        if let Some(didx) = rest.find("登場させ") {
            (&rest[..didx], &rest[didx..])
        } else if let Some(didx) = rest.find("使用") {
            (&rest[..didx], &rest[didx..])
        } else if let Some(didx) = rest.find("裏向きで証拠として得る") {
            (&rest[..didx], &rest[didx..])
        } else {
            return None;
        }
    } else {
        return None;
    };

    // action_phrase を先頭のアクション節と後続効果（「〜し、カードを1枚引く」等）に分離
    let (primary_action_text, trailing_text) = split_action_and_follow_up(action_phrase);
    let mut bonuses = extract_bonuses_from_action(primary_action_text);
    let mut action = parse_target_action(primary_action_text)?;

    // 「自分の現場にいる〚特徴［警察］〛のキャラを1枚まで選び、自分のデッキのカードを上から1枚裏向きでセットする」（B10033）
    let zone = if target_phrase.contains("セットされているカード") {
        TargetZone::SetCards
    } else if target_phrase.contains("証拠") {
        TargetZone::Evidence
    } else if target_phrase.contains("リムーブエリア") {
        TargetZone::Remove
    } else if target_phrase.contains("手札から") || target_phrase.contains("手札にある") {
        TargetZone::Hand
    } else {
        TargetZone::Field
    };

    if zone == TargetZone::Hand {
        if matches!(action, TargetAction::ReviveActive | TargetAction::ReviveSleep) {
            action = TargetAction::PlayFromHand;
            // B08092 / PR291 / B03024 等で「スリープ状態で登場」を保持するため bonus に記載するか action 維持
            // 現状 PlayFromHand で `ReviveSleep` 文言は parse_target_step の follow_up 等で区分、または target_phrase/action_phrase 判定
        }
    }
    if zone == TargetZone::Evidence && matches!(action, TargetAction::RecoverToHand) {
        action = TargetAction::MoveEvidenceToHand;
    }
    if zone == TargetZone::SetCards {
        let cnt = extract_count_around(target_phrase, "合わせて", "枚")
            .max(extract_count_around(target_phrase, "を", "枚"))
            .max(1);
        action = TargetAction::RemoveSetCards(cnt);
    }

    let mut filter = parse_target_filter(
        target_phrase,
        zone,
        action.is_beneficial(),
        dynamic_level,
    );
    if sentence.contains("アクション中のキャラ") {
        filter.acting_only = true;
    }
    if action_phrase.contains("スリープ状態で登場") && zone == TargetZone::Hand {
        // engine.rs が action_phrase の スリープ状態 を判別できるようにする、または ReviveSleep を Hand で使う
        // Wait: 既存の `handle_target_select_answer` は `PLAY_FROM_HAND` が Active 固定な場合があるので、`PLAY_FROM_HAND` 用 sleep か判定する
        // 既存互換のため `ReviveSleep` と同等に判定する follow_up? いいえ、`TargetStep.filter` の `orientations` には使えないので、
        // `TargetBonus` ではなく既存の `ReviveSleep` vs `PlayFromHand` を整理！
        // 実際 `PlayFromHand` 時にスリープ状態で登場は `PR291` `B03024` `B08092` のみ！
        // よってここでは `PLAY_FROM_HAND` を `ReviveSleep` とは区別しつつ、もし action_phrase に「スリープ状態で登場」があれば
        // `TargetBonus` は使わず `TargetAction::ReviveSleep` (zone==Hand の時は手札からスリープ登場) として扱うのが超シンプル！
        action = TargetAction::ReviveSleep;
    }
    // すでに primary action が与えた効果自身は bonuses から重複除外する
    bonuses.retain(|b| match (&action, b) {
        (TargetAction::ApBuff(v), TargetBonus::ApBuff(bv)) => v != bv,
        (TargetAction::GiveRush(r), TargetBonus::GiveRush(br)) => r != br,
        (TargetAction::GiveBullet, TargetBonus::GiveBullet) => false,
        (TargetAction::GiveCanAttackActive, TargetBonus::GiveCanAttackActive) => false,
        (TargetAction::SetDeckTopFaceDown(n), TargetBonus::SetDeckTopFaceDown(bn)) => n != bn,
        (TargetAction::GrantContactForceAttack, TargetBonus::GrantContactForceAttackThisTurn) => false,
        (TargetAction::GrantBulletOnCaseAttackDiscard, TargetBonus::BulletOnCaseAttackDiscardThisTurn) => false,
        _ => true,
    });

    let follow_up = if let Some(t) = trailing_text {
        parse_immediate_ops(t)
    } else {
        Vec::new()
    };

    if matches!(action, TargetAction::ApBuff(_))
        && !sentence.contains("相手の")
        && filter.owner == OwnerScope::OppOnly
    {
        filter.owner = OwnerScope::SelfOnly;
    }

    Some(TargetStep {
        action,
        filter,
        bonuses,
        follow_up,
        conditional_follow_up: None,
        optional: sentence.contains("てもよい"),
        then_abort_if_skipped: false,
        scale_by_cost_flipped_evidence: sentence.contains("コストで表向きにした証拠1つにつき"),
    })
}

pub fn split_action_and_follow_up(action_phrase: &str) -> (&str, Option<&str>) {
    // 複合ターゲットボーナス（「〜AP＋2000し、〚突撃［キャラ］〛を与える」や「〜〚突撃［事件］〛と「...引く。」を与える」「〜AP＋3000し、〜セットする」）は分割せず primary 内に残す
    for sep in [
        "リムーブし、",
        "スリープさせ、",
        "スタンさせ、",
        "アクティブにし、",
        "デッキの下に移し、",
        "登場させ、",
        "手札に加え、",
        "手札に移し、",
        "裏向きにし、",
        "表向きにし、",
    ] {
        if let Some(idx) = action_phrase.find(sep) {
            let cut = idx + sep.len() - "、".len();
            return (&action_phrase[..cut], Some(&action_phrase[idx + sep.len()..]));
        }
    }
    if let Some(idx) = action_phrase.find("する。") {
        let cut = idx + "する".len();
        return (&action_phrase[..cut], Some(&action_phrase[idx + "する。".len()..]));
    }
    // 「〜レベル－1する。カードを1枚引き...」（別文・または「し、カードを」系）
    if let Some(idx) = action_phrase.find("レベル－") {
        if let Some(suru) = action_phrase[idx..].find("する。") {
            let cut = idx + suru + "する".len();
            return (&action_phrase[..cut], Some(&action_phrase[cut + "。".len()..]));
        }
    }
    if let Some(idx) = action_phrase.find("与え、カードを") {
        let cut = idx + "与え".len();
        return (&action_phrase[..cut], Some(&action_phrase[idx + "与え、".len()..]));
    }
    (action_phrase, None)
}

pub fn extract_bonuses_from_action(text: &str) -> Vec<TargetBonus> {
    let mut out = Vec::new();
    if let Some(ap) = parse_signed_ap(text, true) {
        if !text.contains("コンタクト中、このキャラをAP＋") && !text.contains("コンタクト中、そのキャラをAP＋") {
            out.push(TargetBonus::ApBuff(ap));
        }
    }
    if text.contains("〚突撃［レベル4以下のキャラ］〛") {
        out.push(TargetBonus::GiveRush(Some("CHAR_LV4".to_string())));
    } else if text.contains("〚突撃［事件］〛") {
        out.push(TargetBonus::GiveRush(Some("CASE".to_string())));
    } else if text.contains("〚突撃［キャラ］〛") {
        out.push(TargetBonus::GiveRush(Some("CHAR".to_string())));
    } else if text.contains("〚迅速〛") {
        out.push(TargetBonus::GiveRush(Some("SWIFT".to_string())));
    } else if text.contains("〚突撃〛") {
        out.push(TargetBonus::GiveRush(None));
    }
    if text.contains("〚ブレット〛") && !text.contains("そうした場合") {
        out.push(TargetBonus::GiveBullet);
    }
    if text.contains("アクティブ状態のキャラを指定してアクションできる") {
        out.push(TargetBonus::GiveCanAttackActive);
    }
    if text.contains("このキャラがアクションしたとき、カードを1枚引く") {
        out.push(TargetBonus::DrawOnActionThisTurn);
    }
    if text.contains("このキャラがコンタクトしたとき、カードを1枚引き、そのコンタクト中、そのキャラをAP＋1000する") {
        out.push(TargetBonus::ContactDrawAndAp1000ThisTurn);
    }
    if text.contains("このキャラがアクション［事件］したとき、手札を1枚リムーブしてもよい。そうした場合、ターン終了時までこのキャラは〚ブレット〛") {
        out.push(TargetBonus::BulletOnCaseAttackDiscardThisTurn);
    }
    if text.contains("相手の現場にいるキャラを1枚まで選び、このキャラとのコンタクトを発生させる") {
        out.push(TargetBonus::GrantContactForceAttackThisTurn);
    }
    if text.contains("デッキのカードを上から") && text.contains("枚裏向きで") && text.contains("セットする") {
        let n = extract_count_around(text, "上から", "枚裏向き").max(1);
        out.push(TargetBonus::SetDeckTopFaceDown(n));
    }
    out
}

pub fn parse_target_action(text: &str) -> Option<TargetAction> {
    if text.contains("ガードできる場合、必ずガードする") {
        return Some(TargetAction::GiveMustGuard);
    }
    // 「相手はそのカードを〜証拠として得る」と「手札から裏向きで証拠として得る」を区別
    if text.contains("相手はそのカードを表向きのまま証拠として得る")
        || text.contains("持ち主はそのカードを表向きで証拠として得る")
    {
        return Some(TargetAction::TurnIntoOwnerFaceUpEvidence);
    }
    if text.contains("証拠として得る") && !text.contains("相手はそのカード") && !text.contains("持ち主はそのカード") {
        return Some(TargetAction::PlaceHandCardAsEvidenceFaceDown);
    }
    if text.contains("このキャラとのコンタクトを発生させる") {
        return Some(TargetAction::GrantContactForceAttack);
    }
    if text.contains("デッキの上か下に移す") {
        return Some(TargetAction::BottomDeck);
    }
    if text.contains("使用する") && text.contains("イベント") {
        return Some(TargetAction::PlayEventFromHand);
    }
    if (text.contains("表向きにする") || text.contains("表向きにし")) && !text.contains("表向きにした") {
        return Some(TargetAction::FlipEvidenceFaceUp);
    }
    if text.contains("裏向きにする") || text.contains("裏向きにし") {
        return Some(TargetAction::FlipEvidenceFaceDown);
    }
    if text.contains("手札に移す") || text.contains("手札に移し") {
        return Some(TargetAction::BounceToHand);
    }
    if text.contains("デッキの上に移") {
        return Some(TargetAction::DeckTopFromRemove);
    }
    if text.contains("手札に加える") || text.contains("手札に加え") {
        return Some(TargetAction::RecoverToHand);
    }
    if text.contains("裏向きで") && text.contains("セットする") && !text.contains("AP＋") {
        let n = extract_count_around(text, "上から", "枚裏向き").max(1);
        return Some(TargetAction::SetDeckTopFaceDown(n));
    }
    if text.contains("元のAPを0にする") {
        return Some(TargetAction::SetBaseApZero);
    }
    if text.contains("元のLPを0にする") {
        return Some(TargetAction::SetBaseLpZero);
    }
    if let Some(lp_val) = parse_signed_lp(text, true) {
        return Some(TargetAction::LpBuff(lp_val));
    }
    if text.contains("パートナーエリアに移す") {
        return Some(TargetAction::MoveToPartnerArea);
    }
    if text.contains("セットされている裏向きのカードを") && text.contains("移す") {
        return Some(TargetAction::MoveFaceDownSetToTarget);
    }
    if text.contains("自分の現場にいるキャラ1枚にセットする") {
        return Some(TargetAction::SetCardOnTargetChar);
    }
    if text.contains("このキャラにセットする") {
        return Some(TargetAction::SetCardOnSelf);
    }
    if text.contains("【青】のキャラ1枚の下に重ねる") {
        return Some(TargetAction::StackRemoveUnderAllyBlue);
    }
    if text.contains("このキャラの下に重なっているカードを") && text.contains("そのキャラの下に重ねる") {
        let n = extract_count_around(text, "カードを", "枚まで").max(1);
        return Some(TargetAction::TransferStackedUnderTarget(n));
    }
    if text.contains("このキャラをそのキャラの下に重ねる") {
        return Some(TargetAction::StackSelfUnderTarget);
    }
    if text.contains("元の能力を無効にする") {
        return Some(TargetAction::NegateBaseAbilities);
    }
    if text.contains("カード名を公開したキャラのカード名に書き換える") {
        return Some(TargetAction::CopyNameFromRevealed);
    }
    if text.contains("【ヒラメキ】の効果を発動させ") {
        return Some(TargetAction::TriggerTargetHirameki);
    }
    if text.contains("【絆】は指定のカード名のキャラが自分の現場にいるものとして扱う") {
        return Some(TargetAction::GrantBondAsAny);
    }
    if let Some(lv_d) = extract_number_before(text, "レベル－", "する") {
        return Some(TargetAction::LevelDelta(-lv_d));
    }
    if text.contains("スリープ状態で登場") {
        return Some(TargetAction::ReviveSleep);
    }
    if text.contains("登場させ") {
        return Some(TargetAction::ReviveActive);
    }
    if text.contains("デッキの下に移") {
        return Some(TargetAction::BottomDeck);
    }
    if text.contains("リムーブ") {
        return Some(TargetAction::Remove);
    }
    if text.contains("スタン") {
        return Some(TargetAction::Stun);
    }
    if text.contains("スリープ") {
        return Some(TargetAction::Sleep);
    }
    if text.contains("アクティブに") {
        return Some(TargetAction::Activate);
    }
    if let Some(val) = parse_signed_ap(text, true) {
        return Some(TargetAction::ApBuff(val));
    }
    if let Some(val) = parse_signed_ap(text, false) {
        return Some(TargetAction::ApDebuff(val));
    }
    if text.contains("このキャラがコンタクトしたとき、カードを1枚引き、そのコンタクト中") {
        return Some(TargetAction::ApBuff(0));
    }
    if text.contains("アクション［事件］したとき、手札を1枚リムーブしてもよい") {
        return Some(TargetAction::GrantBulletOnCaseAttackDiscard);
    }
    if text.contains("〚ブレット〛") {
        return Some(TargetAction::GiveBullet);
    }
    if text.contains("〚突撃［レベル4以下のキャラ］〛") {
        return Some(TargetAction::GiveRush(Some("CHAR_LV4".to_string())));
    }
    if text.contains("〚突撃［事件］〛") {
        return Some(TargetAction::GiveRush(Some("CASE".to_string())));
    }
    if text.contains("〚突撃［キャラ］〛") {
        return Some(TargetAction::GiveRush(Some("CHAR".to_string())));
    }
    if text.contains("〚迅速〛") {
        return Some(TargetAction::GiveRush(Some("SWIFT".to_string())));
    }
    if text.contains("〚突撃〛") {
        return Some(TargetAction::GiveRush(None));
    }
    if text.contains("アクティブ状態のキャラを指定してアクションできる") {
        return Some(TargetAction::GiveCanAttackActive);
    }
    None
}

/// 対象選択を伴わない即時処理文を `Vec<ImmediateOp>` へ変換する
/// 「カードを2枚引き、手札を2枚リムーブする」のように順序も保持する
pub fn parse_immediate_ops(sentence: &str) -> Vec<ImmediateOp> {
    let mut ops = Vec::new();

    // 全文レベルの全体効果・制限チェック
    if sentence.contains("相手の現場にいるすべてのキャラをスリープさせる") {
        ops.push(ImmediateOp::MassOppSleepAll);
        return ops;
    }
    if sentence.contains("スリープ状態のすべてのキャラをスタンさせる") {
        ops.push(ImmediateOp::MassAllSleepToStun);
        return ops;
    }
    if sentence.contains("コンタクト中のすべてのキャラをリムーブする") {
        ops.push(ImmediateOp::MassContactRemove);
        return ops;
    }
    if sentence.contains("すべてのキャラをリムーブする") {
        ops.push(ImmediateOp::MassAllRemove);
    }
    if sentence.contains("すべてのキャラをターン終了時までAP＋") {
        if let Some(tr) = extract_bracketed_value(sentence, "特徴［", "］") {
            if let Some(ap) = parse_signed_ap(sentence, true) {
                ops.push(ImmediateOp::MassSelfTraitApBuff {
                    trait_name: tr.to_string(),
                    buff: ap,
                });
                return ops;
            }
        }
    }
    if sentence.contains("このカードを表向きのまま証拠として得る") {
        ops.push(ImmediateOp::GainThisAsFaceUpEvidence);
        return ops;
    }
    if sentence.contains("ネクストヒントできない") {
        ops.push(ImmediateOp::BlockHintThisTurn);
        return ops;
    }
    if sentence.contains("自分はイベントを使用できない") {
        ops.push(ImmediateOp::BlockSelfEventsThisTurn);
        return ops;
    }
    if sentence.contains("相手に証拠を1つ与えてもよい") {
        ops.push(ImmediateOp::OptionalGiveOppEvidence(1));
        return ops;
    }
    if sentence.contains("キャラ1枚につき、自分のデッキのカードを上から") && sentence.contains("リムーブ") {
        let n = extract_count_around(sentence, "上から", "枚リムーブ").max(2);
        ops.push(ImmediateOp::DeckTopRemovePerFieldChar(n));
        return ops;
    }

    for part in sentence.split('、') {
        let p = part.trim();
        if p.is_empty() {
            continue;
        }
        let draw = extract_count_around(p, "カードを", "枚引く")
            .max(extract_count_around(p, "を", "枚引"));
        if draw > 0 && p.contains('引') {
            ops.push(ImmediateOp::Draw(draw));
        }

        if p.contains("手札が") && p.contains("枚になるまで手札をリムーブ") {
            let keep = extract_count_around(p, "手札が", "枚になるまで");
            ops.push(ImmediateOp::DiscardHandTo(keep));
        } else if p.contains("手札をすべてリムーブ") {
            ops.push(ImmediateOp::DiscardAllHand);
        } else if !p.contains("相手") {
            let (discard, hand_filter) = extract_hand_remove_spec(p);
            if discard > 0 {
                if let Some(hf) = hand_filter {
                    ops.push(ImmediateOp::DiscardFilteredHand {
                        count: discard,
                        filter: hf,
                    });
                } else {
                    ops.push(ImmediateOp::DiscardHand(discard));
                }
            }
        }

        if p.contains("FILEエリアにあるカードを上から") && (p.contains("手札に加え") || p.contains("手札に加える")) {
            let n = extract_count_around(p, "上から", "枚手札").max(1);
            ops.push(ImmediateOp::FileTopToHand(n));
        } else if p.contains("FILEエリアにあるカードを上から") && p.contains("リムーブ") {
            let n = extract_count_around(p, "上から", "枚リムーブ").max(1);
            ops.push(ImmediateOp::FileTopRemove(n));
        }

        if p.contains("相手の証拠を上から") && p.contains("リムーブ") {
            let n = extract_count_around(p, "上から", "つリムーブ").max(1);
            ops.push(ImmediateOp::OppTopEvidenceRemove(n));
        } else if p.contains("相手の証拠") && p.contains("リムーブ") {
            let n = extract_count_around(p, "相手の証拠を", "つリムーブ")
                .max(extract_count_around(p, "上から", "つリムーブ"))
                .max(1);
            ops.push(ImmediateOp::OppRemoveEvidence(n));
        } else if p.contains("証拠") && p.contains("リムーブ") && !p.contains("コスト") {
            let n = extract_count_around(p, "上から", "つリムーブ")
                .max(extract_count_around(p, "証拠を", "つリムーブ"))
                .max(1);
            ops.push(ImmediateOp::SelfRemoveEvidence {
                count: n,
                trigger_hirameki: !sentence.contains("【ヒラメキ】は発動しない"),
            });
        } else if p.contains("自分の裏向きの証拠") && sentence.contains("表向き") {
            let n = extract_count_around(p, "証拠を", "つ").max(1);
            ops.push(ImmediateOp::FlipEvidenceUp(n));
        } else if !p.contains("相手") && p.contains("表向きの証拠") && sentence.contains("裏向き") {
            let n = extract_count_around(p, "証拠を", "つ").max(1);
            ops.push(ImmediateOp::FlipEvidenceDown(n));
        } else if p.contains("相手の") && p.contains("証拠") && sentence.contains("表向き") {
            let n = extract_count_around(p, "証拠を", "つ").max(1);
            ops.push(ImmediateOp::OppFlipEvidenceUp(n));
        } else if !p.contains("相手に") && p.contains("証拠を") && (p.contains("得る") || p.contains("増やす")) {
            let n = extract_count_around(p, "証拠を", "つ").max(1);
            ops.push(ImmediateOp::GainEvidence(n));
        }

        if p.contains("このカードを手札に加える") {
            ops.push(ImmediateOp::MoveSelfFromEvidenceToHand);
        }
        if p.contains("このカードをパートナーエリアに移す") {
            ops.push(ImmediateOp::MoveSelfFromEvidenceToPartner);
        }
        if p.contains("相手は手札を") && p.contains("ランダムにリムーブ") {
            let n = extract_count_around(p, "手札を", "枚ランダム").max(1);
            ops.push(ImmediateOp::OppDiscardRandom(n));
        } else if p.contains("相手は手札を") && p.contains("リムーブ") {
            let n = extract_count_around(p, "手札を", "枚リムーブ").max(1);
            ops.push(ImmediateOp::OppDiscardHand(n));
        }
        if sentence.contains("相手は") && p.contains("【カットイン】") && sentence.contains("使用できない") {
            ops.push(ImmediateOp::BlockOppCutinThisTurn);
        }
        if (p.contains("【変装】") && (p.contains("使用できない") || sentence.contains("使用できない")))
            || p.contains("相手は【変装】を使用できない")
        {
            ops.push(ImmediateOp::BlockOppDisguiseThisTurn);
        }
        if sentence.contains("その【ヒラメキ】の効果を発動") {
            if sentence.contains("松田陣平") {
                ops.push(ImmediateOp::TriggerFieldHirameki("松田陣平".to_string()));
            } else if sentence.contains("YAIBA") {
                ops.push(ImmediateOp::TriggerEvidenceHiramekiByTrait("YAIBA".to_string()));
            }
        }

        if p.contains("相手のデッキのカードを上から") && p.contains("リムーブ") {
            let n = extract_count_around(p, "上から", "枚リムーブ");
            if n > 0 {
                ops.push(ImmediateOp::OppMillDeck(n));
            }
        } else if p.contains("デッキのカードを上から") && p.contains("リムーブ") {
            if p.contains("そのカードのレベルと同じ枚数リムーブ") {
                ops.push(ImmediateOp::MillDeckByLastChosenLevel);
            } else {
                let n = extract_count_around(p, "上から", "枚リムーブ");
                if n > 0 {
                    ops.push(ImmediateOp::MillDeck(n));
                }
            }
        }

        if let Some(buff) = parse_signed_ap(p, true) {
            ops.push(ImmediateOp::SelfApBuff(buff));
        }

        if !p.contains("与える") && !sentence.contains('「') {
            if let Some(lp_buff) = parse_signed_lp(p, true) {
                ops.push(ImmediateOp::SelfLpBuff(lp_buff));
            }
            if let Some(lv_delta) = parse_signed_level(p, true).or_else(|| parse_signed_level(p, false)) {
                ops.push(ImmediateOp::SelfLevelDelta(lv_delta));
            }
        }

        if sentence.contains("自分の現場にいる")
            && sentence.contains("元のLPが")
            && sentence.contains("元のLPを")
            && sentence.contains("にする")
            && !ops.iter().any(|op| matches!(op, ImmediateOp::SetSelfFieldBaseLp { .. }))
        {
            let from_lp = extract_count_around(sentence, "元のLPが", "で") as i32;
            let level = extract_count_around(sentence, "レベル", "の") as i32;
            let to_lp = extract_count_around(sentence, "元のLPを", "にする") as i32;
            let trait_name = sentence
                .find("〚特徴［")
                .and_then(|s| {
                    let rest = &sentence[s + "〚特徴［".len()..];
                    rest.find("］〛").map(|e| rest[..e].to_string())
                })
                .unwrap_or_default();
            if level > 0 && !trait_name.is_empty() {
                ops.push(ImmediateOp::SetSelfFieldBaseLp {
                    from_lp,
                    level,
                    trait_name,
                    to_lp,
                });
            }
        }

        if p.contains("このキャラをアクティブに") {
            ops.push(ImmediateOp::SelfActivate);
        }

        if !p.contains("与える") && (p.contains("このキャラ") || sentence.contains("このキャラ")) {
            if p.contains("〚突撃［レベル4以下のキャラ］〛") {
                ops.push(ImmediateOp::SelfGiveRush(Some("CHAR_LV4".to_string())));
            } else if p.contains("〚突撃［事件］〛") {
                ops.push(ImmediateOp::SelfGiveRush(Some("CASE".to_string())));
            } else if p.contains("〚突撃［キャラ］〛") {
                ops.push(ImmediateOp::SelfGiveRush(Some("CHAR".to_string())));
            } else if p.contains("〚迅速〛") {
                ops.push(ImmediateOp::SelfGiveRush(Some("SWIFT".to_string())));
            } else if p.contains("〚突撃〛") {
                ops.push(ImmediateOp::SelfGiveRush(None));
            }
        }
    }
    ops
}

