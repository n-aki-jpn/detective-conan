// Detective Conan TCG Core State Machine and Rules Manager
const PHASES = {
  AUTO: "AUTO_PHASE",
  MAIN: "MAIN_PHASE",
  END: "END_PHASE",
  GUARD: "GUARD_PHASE"
};

const GAME_PHASES = {
  INCIDENT: "INCIDENT_PHASE",
  SOLUTION: "SOLUTION_PHASE"
};

class ConanTCGState {
  constructor() {
    this.reset();
  }

  reset() {
    this.players = [
      this.createEmptyPlayerState("Player 1", "Blue"),
      this.createEmptyPlayerState("Player 2", "Green")
    ];
    this.turnOwner = 0; // 0 = Player 1, 1 = Player 2
    this.currentPhase = PHASES.AUTO;
    this.turnNumber = 1;
    this.logs = [];
    // Don't clear onStateChangeCallbacks - they should persist across game resets
    if (!this.onStateChangeCallbacks) {
      this.onStateChangeCallbacks = [];
    }

    // Tracking current turn actions
    this.turnActions = {
      normalPlayUsed: false,
      nextHintCount: 0,
      hasUsedNextHintThisTurn: false
    };

    // Guard phase state
    this.guardPhase = {
      active: false,
      attackerId: null,
      defenderId: null, // ガードキャラのID
      targetId: null,
      targetType: null, // "case" or "character"
      attackerPlayerIndex: null,
      defenderPlayerIndex: null,
      cutInUsed: false, // カットイン使用済みフラグ
      disguiseUsed: false // 変装使用済みフラグ
    };
   }

  createEmptyPlayerState(name, preferredColor) {
    return {
      name: name,
      color: preferredColor,
      partner: null,
      caseCard: null,
      hand: [],
      deck: [],
      fileArea: [],
      evidenceArea: [],
      removeArea: [],
      field: Array(5).fill(null), // 5 character slots
      gamePhase: GAME_PHASES.INCIDENT
    };
  }

  addLog(msg) {
    const timestamp = new Date().toLocaleTimeString();
    const formattedMsg = `[${timestamp}] ${msg}`;
    this.logs.push(formattedMsg);
    console.log(formattedMsg);
    this.triggerStateChange();
  }

  triggerStateChange() {
    this.onStateChangeCallbacks.forEach(cb => cb(this));
  }

  findCard(instanceId) {
    for (let pIdx = 0; pIdx < 2; pIdx++) {
      const player = this.players[pIdx];
      // Check partner
      if (player.partner && player.partner.instanceId === instanceId) {
        return { card: player.partner, playerIndex: pIdx, area: "partner", index: null };
      }
      // Check case
      if (player.caseCard && player.caseCard.instanceId === instanceId) {
        return { card: player.caseCard, playerIndex: pIdx, area: "caseCard", index: null };
      }
      // Check hand
      const handIdx = player.hand.findIndex(c => c.instanceId === instanceId);
      if (handIdx !== -1) {
        return { card: player.hand[handIdx], playerIndex: pIdx, area: "hand", index: handIdx };
      }
      // Check file
      const fileIdx = player.fileArea.findIndex(c => c.instanceId === instanceId);
      if (fileIdx !== -1) {
        return { card: player.fileArea[fileIdx], playerIndex: pIdx, area: "fileArea", index: fileIdx };
      }
      // Check evidence
      const evIdx = player.evidenceArea.findIndex(c => c.instanceId === instanceId);
      if (evIdx !== -1) {
        return { card: player.evidenceArea[evIdx], playerIndex: pIdx, area: "evidenceArea", index: evIdx };
      }
      // Check remove
      const remIdx = player.removeArea.findIndex(c => c.instanceId === instanceId);
      if (remIdx !== -1) {
        return { card: player.removeArea[remIdx], playerIndex: pIdx, area: "removeArea", index: remIdx };
      }
      // Check field
      for (let fIdx = 0; fIdx < 5; fIdx++) {
        if (player.field[fIdx] && player.field[fIdx].instanceId === instanceId) {
          return { card: player.field[fIdx], playerIndex: pIdx, area: "field", index: fIdx };
        }
      }
      // Check deck
      const deckIdx = player.deck.findIndex(c => c.instanceId === instanceId);
      if (deckIdx !== -1) {
        return { card: player.deck[deckIdx], playerIndex: pIdx, area: "deck", index: deckIdx };
      }
    }
    return null;
  }

  drawCard(playerIndex, log = true) {
    const player = this.players[playerIndex];
    const card = this.popCardFromDeck(playerIndex);
    if (card) {
      card.state = "active";
      player.hand.push(card);
      if (log) this.addLog(`${player.name}が山札から1枚ドローしました。（残高: ${player.deck.length}枚）`);
      return card;
    } else {
      this.addLog(`警告: ${player.name}の山札が空でリフレッシュもできませんでした。`);
      return null;
    }
  }

  declareDefeat(playerIndex, reason) {
    const player = this.players[playerIndex];
    const winner = this.players[1 - playerIndex];
    this.addLog(`★敗北宣告★ ${player.name} は${reason}`);
    alert(`【ゲーム終了】\n${winner.name}の勝利です！\n${player.name}は${reason}`);
    this.triggerStateChange();
  }

  // ダミー関数：カットインや変装時にUIへ通知するための関数。現状はログ出力のみ。
  // 実際のゲームでは、ここでカットイン演出や変装演出を行う
  triggerCutIn(type, card, effect) {
    this.addLog(`🎬 ${card.name} が ${type === "cutin" ? "カットイン" : "変装"} を発動！ 効果: ${effect}`);
    this.triggerStateChange(); // UIを再描画して、例えばカットイン中のAP変化を反映させる
  }

  // Use Cut-in during contact
  useCutIn(playerIndex, cardInstanceId) {
    if (!this.guardPhase.active || this.currentPhase !== PHASES.GUARD) {
      this.addLog("カットインはガードフェーズ中のみ使用できます。");
      return false;
    }

    if (this.guardPhase.cutInUsed) {
      this.addLog("カットインは1回のコンタクトにつき1枚だけ使用できます。");
      return false;
    }

    const cardLoc = this.findCard(cardInstanceId);
    if (!cardLoc || cardLoc.playerIndex !== playerIndex || cardLoc.area !== "hand") {
      this.addLog("手札からカットインカードを選択してください。");
      return false;
    }

    const card = cardLoc.card;
    if (!card.cutIn) {
      this.addLog("このカードはカットイン能力を持っていません。");
      return false;
    }

    // Remove from hand and add to remove area
    const player = this.players[playerIndex];
    player.hand.splice(cardLoc.index, 1);
    player.removeArea.push(card);

    // Apply cut-in effect
    // コンタクト中の自分側のキャラのAPを増やす
    let targetChar = null;
    if (playerIndex === this.guardPhase.attackerPlayerIndex) {
      targetChar = this.findCard(this.guardPhase.attackerId)?.card;
    } else if (playerIndex === this.guardPhase.defenderPlayerIndex) {
      // ガード中のキャラにカットインを使用した場合、そのガードキャラのAPを増やす
      targetChar = this.findCard(this.guardPhase.defenderId)?.card;
    }

    if (!targetChar) {
      this.addLog("カットイン対象のキャラが見つかりません。（自分とコンタクト中のキャラ）");
      // エラー発生時はカードを手札に戻す
      player.removeArea.pop(); 
      player.hand.push(card);
      return false;
    }

    const originalAP = targetChar.ap;

    // Parse cut-in effect (e.g., "AP＋2000")
    if (card.cutIn.includes("AP＋")) {
      const apBonus = parseInt(card.cutIn.replace("AP＋", ""));
      targetChar.ap += apBonus;
      this.addLog(`${player.name} が ${card.name} をカットインで使用！ ${targetChar.name} のAPを +${apBonus} しました。（${originalAP} → ${targetChar.ap}）`);
    }

    this.guardPhase.cutInUsed = true;
    this.triggerCutIn("cutin", card, card.cutIn);
    this.triggerStateChange(); // UIを更新してAP変更を反映
    return true;
  }

  // Use Disguise during contact
  useDisguise(playerIndex, cardInstanceId) {
    if (!this.guardPhase.active || this.currentPhase !== PHASES.GUARD) {
      this.addLog("変装はガードフェーズ中のみ使用できます。");
      return false;
    }

    if (this.guardPhase.disguiseUsed) {
      this.addLog("変装は1回のコンタクトにつき1枚だけ使用できます。");
      return false;
    }

    const cardLoc = this.findCard(cardInstanceId);
    if (!cardLoc || cardLoc.playerIndex !== playerIndex || cardLoc.area !== "hand") {
      this.addLog("手札から変装カードを選択してください。");
      return false;
    }

    const card = cardLoc.card;
    if (!card.disguise) {
      this.addLog("このカードは変装能力を持っていません。");
      return false;
    }
    
    // 変装元のキャラを特定 (コンタクト中の自分側のキャラ)
    let originalCharLoc = null;
    let targetCharIdInGuardPhase = null;

    if (playerIndex === this.guardPhase.attackerPlayerIndex) {
      targetCharIdInGuardPhase = this.guardPhase.attackerId;
    } else if (playerIndex === this.guardPhase.defenderPlayerIndex) {
      targetCharIdInGuardPhase = this.guardPhase.defenderId;
    }

    originalCharLoc = this.findCard(targetCharIdInGuardPhase);

    if (!originalCharLoc || originalCharLoc.area !== "field" || originalCharLoc.playerIndex !== playerIndex) {
      this.addLog("変装対象のキャラが現場にいません。（自分とコンタクト中のキャラ）");
      return false;
    }

    const player = this.players[playerIndex];
    const originalCard = originalCharLoc.card;

    // 元のカードの状態や効果を引き継ぐ
    card.state = originalCard.state; // 状態を引き継ぐ
    card.nameDeclaring = originalCard.nameDeclaring; // 名乗り状態を引き継ぐ
    card.ap = originalCard.ap; // 現在のAPを引き継ぐ
    // その他の効果があれば、ここで引き継ぎロジックを追加

    // 手札から変装カードを削除
    player.hand.splice(cardLoc.index, 1);

    // 現場のキャラを変装カードに入れ替える
    player.field[originalCharLoc.index] = card;

    // 元のカードを裏向きで山札の下に戻す
    originalCard.state = "facedown";
    player.deck.unshift(originalCard); // 山札の下に追加

    // ガードフェーズの参照を更新
    if (playerIndex === this.guardPhase.attackerPlayerIndex) {
      this.guardPhase.attackerId = card.instanceId;
    } else {
      this.guardPhase.defenderId = card.instanceId;
    }

    this.guardPhase.disguiseUsed = true;
    this.addLog(`${player.name} が ${card.name} で変装！ ${originalCard.name} と入れ替わりました。`);
    this.triggerCutIn("disguise", card, "変装！");
    this.triggerStateChange(); // UIを更新してキャラの入れ替わりを反映
    return true;
  }

  // Refresh deck from remove area
  refreshDeckFromRemoveArea(playerIndex) {
    const player = this.players[playerIndex];
    const removeCards = [...player.removeArea];
    player.removeArea = [];

    // Shuffle helper (Fisher-Yates)
    for (let i = removeCards.length - 1; i > 0; i--) {
      const j = Math.floor(Math.random() * (i + 1));
      [removeCards[i], removeCards[j]] = [removeCards[j], removeCards[i]];
    }

    // Set face down and assign to deck
    removeCards.forEach(c => {
      c.state = "facedown";
    });
    player.deck = removeCards;

    this.addLog(`🔄 ${player.name} の山札がなくなったため、リムーブエリアのカード ${player.deck.length} 枚をシャッフルして山札をリフレッシュしました。`);
  }

  // Pop a card from deck, refreshing if empty, or declaring defeat if remove area is also empty
   popCardFromDeck(playerIndex) {
     const player = this.players[playerIndex];
     if (player.deck.length === 0) {
       if (player.removeArea.length === 0) {
         this.declareDefeat(playerIndex, "山札がなく、リムーブエリアも0枚のため敗北しました。");
         return null;
       }
       this.refreshDeckFromRemoveArea(playerIndex);
     }
     return player.deck.pop();
   }

   // Initialize a new game
   initGame(p1PartnerId, p1CaseId, p2PartnerId, p2CaseId) {
     this.reset();
     
     // Explicitly reset game phases for both players
     this.players[0].gamePhase = GAME_PHASES.INCIDENT;
     this.players[1].gamePhase = GAME_PHASES.INCIDENT;
     
     // Set Partners & Cases
     const p1PartnerData = CARD_DATABASE.find(c => c.id === p1PartnerId);
     const p1CaseData = CARD_DATABASE.find(c => c.id === p1CaseId);
     const p2PartnerData = CARD_DATABASE.find(c => c.id === p2PartnerId);
     const p2CaseData = CARD_DATABASE.find(c => c.id === p2CaseId);

     if (p1PartnerData && p1CaseData && p2PartnerData && p2CaseData) {
       this.players[0].partner = { ...p1PartnerData, instanceId: "P1_PARTNER", state: "active", nameDeclaring: false };
       this.players[0].caseCard = { ...p1CaseData, instanceId: "P1_CASE", state: "active", nameDeclaring: false };
       this.players[1].partner = { ...p2PartnerData, instanceId: "P2_PARTNER", state: "active", nameDeclaring: false };
       this.players[1].caseCard = { ...p2CaseData, instanceId: "P2_CASE", state: "active", nameDeclaring: false };
     }

     // Generate starter decks
     this.players[0].deck = generateStarterDeck(p1CaseData.color);
     this.players[1].deck = generateStarterDeck(p2CaseData.color);

     // Initial hand: draw 5 cards for both players
     for (let i = 0; i < 5; i++) {
       this.drawCard(0, false);
       this.drawCard(1, false);
     }

     this.addLog("ゲームを開始しました！初期手札5枚をドローしました。");
     
     // Start Turn 1
     this.startTurn();
   }

   // Auto Phase Automations
   startTurn() {
     const activePlayer = this.players[this.turnOwner];
     this.currentPhase = PHASES.AUTO;
     this.addLog(`${activePlayer.name}のターン ${this.turnNumber} (オートフェイズ開始)`);

    // 1. Untap (Stand cards)
    // Stand partners
    if (activePlayer.partner) {
      if (activePlayer.partner.state === "stun") {
        activePlayer.partner.state = "sleep";
        this.addLog(`パートナー: ${activePlayer.partner.name} はスタンからスリープに回復しました。`);
      } else if (activePlayer.partner.state === "sleep") {
        activePlayer.partner.state = "active";
      }
    } else {
      // Check if partner is in FILE area and return to partner area
      const partnerInFile = activePlayer.fileArea.find(card => card.type === "Partner");
      if (partnerInFile) {
        activePlayer.fileArea = activePlayer.fileArea.filter(card => card !== partnerInFile);
        partnerInFile.state = "active";
        activePlayer.partner = partnerInFile;
        this.addLog(`パートナー: ${partnerInFile.name} がFILEエリアからパートナーエリアに戻りました。`);
      }
    }
    // Stand characters on field
    activePlayer.field.forEach(char => {
      if (char) {
        char.nameDeclaring = false; // Reset summoning sickness at start of turn
        if (char.state === "stun") {
          char.state = "sleep";
          this.addLog(`${char.name} はスタンからスリープに回復しました。`);
        } else if (char.state === "sleep") {
          char.state = "active";
        }
      }
    });

    // 2. Draw 1 card
    this.drawCard(this.turnOwner);

    // 3. Place cards in FILE area
    // 先攻1ターン目 (Turn 1, Player 1) -> 1 card
    // Any other turn -> 2 cards
    const fileCount = (this.turnNumber === 1 && this.turnOwner === 0) ? 1 : 2;
    for (let i = 0; i < fileCount; i++) {
      const fileCard = this.popCardFromDeck(this.turnOwner);
      if (fileCard) {
        fileCard.state = "facedown";
        activePlayer.fileArea.push(fileCard);
      }
    }
    this.addLog(`${activePlayer.name}のFILEエリアに裏向きで${fileCount}枚セットしました。（現在のFILE: ${activePlayer.fileArea.length}枚）`);

    // Reset turn actions tracker
    this.turnActions = {
      normalPlayUsed: false,
      nextHintCount: 0,
      hasUsedNextHintThisTurn: false
    };

    // Transition to Main Phase immediately or let the UI handle it?
    // According to rule flow, Auto Phase finishes instantly, moving to Main Phase
    this.transitionToPhase(PHASES.MAIN);
  }

  // Handle phase transitions
  transitionToPhase(nextPhase) {
    if (nextPhase === PHASES.MAIN) {
      this.currentPhase = PHASES.MAIN;
      this.addLog(`${this.players[this.turnOwner].name}のメインフェイズを開始しました。`);
    } else if (nextPhase === PHASES.END) {
      this.currentPhase = PHASES.END;
      this.addLog(`${this.players[this.turnOwner].name}のエンドフェイズです。`);
      
      // End turn automatically and pass to next player
      this.endTurn();
    }
    this.triggerStateChange();
  }

  endTurn() {
    this.addLog(`${this.players[this.turnOwner].name}のターンが終了しました。`);
    this.turnOwner = 1 - this.turnOwner; // Toggle between 0 and 1
    if (this.turnOwner === 0) {
      this.turnNumber++;
    }
    this.startTurn();
  }

  // Validate playing a card from hand
  canPlayCardFromHand(playerIndex, card) {
    // Must be owner's Main Phase
    if (this.turnOwner !== playerIndex || this.currentPhase !== PHASES.MAIN) {
      return { valid: false, reason: "自分のメインフェイズ中のみプレイ可能です。" };
    }

    // Normal play validation (1 per turn, and only if Next Hint hasn't been used)
    if (!this.turnActions.hasUsedNextHintThisTurn) {
      if (this.turnActions.normalPlayUsed) {
        return { valid: false, reason: "通常の手札プレイは1ターンに1回までです。さらにプレイするには「ネクストヒント」を行ってください。" };
      }
    } else {
      // If next hint was used, playing is only allowed immediately after Next Hint.
      // We will allow immediate play after Next Hint. In a simplified state machine,
      // nextHintCount acts as a play authorization.
      if (this.turnActions.nextHintCount === 0) {
        return { valid: false, reason: "ネクストヒントを使用した後は、次のネクストヒントを行うまでカードを手札から使用できません。" };
      }
    }

    const player = this.players[playerIndex];
    
    // Cost Level check: level of card must be <= number of files
    if (card.level > player.fileArea.length) {
      return { valid: false, reason: `コスト不足: カードのレベルは ${card.level} ですが、FILE枚数は ${player.fileArea.length} 枚です。` };
    }

    // Color rule check: card color must match Case card color
    // Exception: Kaitou Kid (CT-CH-W01) has "神出鬼没" ability allowing play regardless of color
    if (player.caseCard && card.id !== "CT-CH-W01") {
      if (card.color !== player.caseCard.color) {
        return { valid: false, reason: `色エラー: 事件カードの色(${player.caseCard.color})と異なる色のカード(${card.color})はプレイできません。` };
      }
    }

    return { valid: true };
  }

  // Play card from hand
  playCard(playerIndex, cardInstanceId, fieldSlot = null) {
    const player = this.players[playerIndex];
    const cardLoc = this.findCard(cardInstanceId);
    if (!cardLoc || cardLoc.area !== "hand" || cardLoc.playerIndex !== playerIndex) {
      return false;
    }

    const card = cardLoc.card;
    const validation = this.canPlayCardFromHand(playerIndex, card);
    if (!validation.valid) {
      this.addLog(`プレイ不可: ${validation.reason}`);
      return false;
    }

    // Remove from hand
    player.hand.splice(cardLoc.index, 1);

    if (card.type === "Character") {
      // Place in Field Slot (if slot is full, remove previous card)
      if (fieldSlot === null || fieldSlot < 0 || fieldSlot >= 5) {
        // Find empty slot
        fieldSlot = player.field.findIndex(slot => slot === null);
        if (fieldSlot === -1) fieldSlot = 0; // fallback to slot 0
      }

      const prevCard = player.field[fieldSlot];
      if (prevCard) {
        player.removeArea.push(prevCard);
        this.addLog(`現場スロット${fieldSlot + 1}の ${prevCard.name} は押し出されてリムーブエリアへ送られました。`);
      }

      card.state = "active";
      card.nameDeclaring = true; // Has summoning sickness
      player.field[fieldSlot] = card;
      this.addLog(`${player.name}が ${card.name} (Lv ${card.level}) を現場のスロット${fieldSlot + 1}に登場させました。`);
    } else if (card.type === "Event") {
      // Events go straight to Remove area after resolution
      card.state = "active";
      player.removeArea.push(card);
      this.addLog(`${player.name}がイベント ${card.name} (Lv ${card.level}) を使用しました。（効果解決後、リムーブエリアへ）`);
    }

    // Consume play token
    if (this.turnActions.hasUsedNextHintThisTurn) {
      this.turnActions.nextHintCount = Math.max(0, this.turnActions.nextHintCount - 1);
    } else {
      this.turnActions.normalPlayUsed = true;
    }

    this.triggerStateChange();
    return true;
  }

  // Next Hint Action
  canPerformNextHint(playerIndex) {
    if (this.turnOwner !== playerIndex || this.currentPhase !== PHASES.MAIN) {
      return { valid: false, reason: "自分のメインフェイズ中のみネクストヒントを実行可能です。" };
    }

    const player = this.players[playerIndex];
    if (player.fileArea.length === 0) {
      return { valid: false, reason: "FILEエリアにカードがありません。" };
    }

    return { valid: true };
  }

  performNextHint(playerIndex) {
    const validation = this.canPerformNextHint(playerIndex);
    if (!validation.valid) {
      this.addLog(`ネクストヒント失敗: ${validation.reason}`);
      return false;
    }

    const player = this.players[playerIndex];
    // Pull top card from FILE area to hand
    const fileCard = player.fileArea.pop();
    fileCard.state = "active";
    player.hand.push(fileCard);

    // Update state flags
    this.turnActions.hasUsedNextHintThisTurn = true;
    this.turnActions.nextHintCount = 1; // Authorizes exactly 1 card play

    this.addLog(`${player.name}がネクストヒントを実行しました。FILEを1枚手札に加えました。(現在のFILE: ${player.fileArea.length}枚)`);
    this.triggerStateChange();
    return true;
  }

  // Cancel remaining Next Hint play token if user decides not to play
  skipNextHintPlay() {
    if (this.turnActions.nextHintCount > 0) {
      this.turnActions.nextHintCount = 0;
      this.addLog("ネクストヒントでの手札使用をスキップしました。");
      this.triggerStateChange();
    }
  }

  // Reasoning action (gather evidence)
  performReasoning(playerIndex, charInstanceId) {
    if (this.turnOwner !== playerIndex || this.currentPhase !== PHASES.MAIN) {
      this.addLog("自分のメインフェイズ中のみ推理を行えます。");
      return false;
    }

    const loc = this.findCard(charInstanceId);
    if (!loc || loc.playerIndex !== playerIndex || (loc.area !== "field" && loc.area !== "partner")) {
      return false;
    }

    const char = loc.card;
    if (char.state !== "active") {
      this.addLog(`${char.name} はアクティブ状態ではないため、推理できません。`);
      return false;
    }

    if (char.nameDeclaring) {
      this.addLog(`${char.name} は名乗り状態（登場ターン）のため、推理できません。`);
      return false;
    }

    // Sleep character
    char.state = "sleep";

    const opponentIndex = 1 - playerIndex;
    const opponent = this.players[opponentIndex];

    // For simplicity in Phase 1, we will trigger a log. 
    // The user can choose to Guard manually or let the reasoning resolve.
    // Let's implement a system where a prompt or status log records the action,
    // and if resolved, gives LP evidence.
    const lpGained = char.lp || 1;
    
    // Add to evidence area: we will pop from top of deck to evidence area face down
    let actualGained = 0;
    for (let i = 0; i < lpGained; i++) {
      const evCard = this.popCardFromDeck(playerIndex);
      if (evCard) {
        evCard.state = "facedown";
        this.players[playerIndex].evidenceArea.push(evCard);
        actualGained++;
      }
    }

    this.addLog(`${this.players[playerIndex].name} が ${char.name} をスリープして推理を実行しました！ 証拠を ${actualGained} 枚獲得！（現在の証拠: ${this.players[playerIndex].evidenceArea.length}枚）`);
    this.triggerStateChange();
    return true;
  }

  // Action (attack opponent's sleep character or opponent's Case/Evidence)
  performAction(playerIndex, charInstanceId, targetType, targetInstanceId) {
    if (this.turnOwner !== playerIndex || this.currentPhase !== PHASES.MAIN) {
      return false;
    }

    const attackerLoc = this.findCard(charInstanceId);
    if (!attackerLoc || attackerLoc.playerIndex !== playerIndex || attackerLoc.area !== "field") {
      return false;
    }

    const attacker = attackerLoc.card;
    if (attacker.state !== "active") {
      this.addLog(`${attacker.name} はアクティブではないため、アクションできません。`);
      return false;
    }

    const targetLoc = this.findCard(targetInstanceId);
    if (!targetLoc) {
      return false;
    }

    const opponentIndex = 1 - playerIndex;
    const opponent = this.players[opponentIndex];

    // Check if opponent has active characters that can guard
    const hasActiveGuards = opponent.field.some(char => char && char.state === "active");

    if (hasActiveGuards) {
      // Start guard phase
      this.guardPhase = {
        active: true,
        attackerId: charInstanceId,
        targetId: targetInstanceId,
        targetType: targetType,
        attackerPlayerIndex: playerIndex,
        defenderPlayerIndex: opponentIndex
      };
      this.currentPhase = PHASES.GUARD;
      this.turnOwner = opponentIndex; // Switch to defender for guard phase
      attacker.state = "sleep";
      if (targetType === "character") {
        this.addLog(`${this.players[playerIndex].name} が ${attacker.name} で ${targetLoc.card.name} にアクションを仕掛けました！`);
      } else {
        this.addLog(`${this.players[playerIndex].name} が ${attacker.name} で相手の事件にアクションを仕掛けました！`);
      }
      this.addLog(`${opponent.name} のターン: ガードするキャラを選択してください。`);
      this.triggerStateChange();
      return true;
    }

    // No guard possible or attacking case, resolve immediately
    attacker.state = "sleep";

    if (targetType === "character") {
      const targetChar = targetLoc.card;
      if (targetChar.state !== "sleep") {
        this.addLog(`ターゲット不可: ${targetChar.name} はスリープ状態ではありません。`);
        attacker.state = "active"; // undo
        return false;
      }

      this.addLog(`${this.players[playerIndex].name} が ${attacker.name} で ${targetChar.name} にアクションを仕掛けました！`);

      // AP comparison
      const player1AP = attacker.ap;
      const player2AP = targetChar.ap;
      if (player1AP >= player2AP) {
        // Remove defender
        const targetPlayer = this.players[targetLoc.playerIndex];
        targetPlayer.field[targetLoc.index] = null;
        targetPlayer.removeArea.push(targetChar);
        this.addLog(`コンタクト成功: ${attacker.name}(AP ${player1AP}) が ${targetChar.name}(AP ${player2AP}) を倒し、リムーブエリアへ送りました。`);
      } else {
        this.addLog(`コンタクト失敗: ${targetChar.name}(AP ${player2AP}) のAPが上回ったため、${attacker.name}(AP ${player1AP}) は敗北しましたが、リムーブされません。`);
      }
    } else if (targetType === "case") {
      if (opponent.evidenceArea.length === 0) {
        this.addLog("ターゲット不可: 相手に証拠がありません。");
        attacker.state = "active"; // undo
        return false;
      }

      this.addLog(`${this.players[playerIndex].name} が ${attacker.name} で相手の事件にアクションを仕掛けました！`);

      // Steal 1 evidence
      const stolenCard = opponent.evidenceArea.pop();
      this.players[playerIndex].evidenceArea.push(stolenCard);
      this.addLog(`コンタクト成功: 相手の証拠を1枚奪い、自分の証拠エリアに移しました。（現在の証拠: 自分の証拠 ${this.players[playerIndex].evidenceArea.length}枚、相手の証拠 ${opponent.evidenceArea.length}枚）`);
    }

    this.triggerStateChange();
    return true;
  }

  // Guard action (defender selects a character to guard)
  performGuard(playerIndex, guardCharInstanceId) {
    if (!this.guardPhase.active || this.currentPhase !== PHASES.GUARD) {
      return false;
    }

    if (playerIndex !== this.guardPhase.defenderPlayerIndex) {
      return false;
    }

    const guardLoc = this.findCard(guardCharInstanceId);
    if (!guardLoc || guardLoc.playerIndex !== playerIndex || guardLoc.area !== "field") {
      return false;
    }

    const guardChar = guardLoc.card;
    if (guardChar.state !== "active") {
      this.addLog(`${guardChar.name} はアクティブではないため、ガードできません。`);
      return false;
    }

    const attackerLoc = this.findCard(this.guardPhase.attackerId);
    const attacker = attackerLoc.card;

    // Set defenderId for cut-in/disguise tracking
    this.guardPhase.defenderId = guardCharInstanceId;

    // Apply guard ability (reduce attacker AP by 1000)
    const originalAttackerAP = attacker.ap;
    attacker.ap = Math.max(0, attacker.ap - 1000);

    this.addLog(`${this.players[playerIndex].name} が ${guardChar.name} でガード！ 攻撃者のAPを -1000 しました。`);

    // Resolve the action with reduced AP - guard character vs attacker
    this.addLog(`${this.players[this.guardPhase.attackerPlayerIndex].name} の ${attacker.name} (AP ${attacker.ap}) vs ${guardChar.name} (AP ${guardChar.ap})`);

    if (attacker.ap >= guardChar.ap) {
      const guardPlayer = this.players[guardLoc.playerIndex];
      guardPlayer.field[guardLoc.index] = null;
      guardPlayer.removeArea.push(guardChar);
      this.addLog(`コンタクト成功: ${attacker.name} が ${guardChar.name} を倒し、リムーブエリアへ送りました。`);

      // If target was case, steal evidence after successful guard contact
      if (this.guardPhase.targetType === "case") {
        const opponent = this.players[this.guardPhase.attackerPlayerIndex];
        const defender = this.players[this.guardPhase.defenderPlayerIndex];

        if (defender.evidenceArea.length === 0) {
          this.addLog("相手に証拠がないため、証拠奪取は失敗しました。");
        } else {
          const stolenCard = defender.evidenceArea.pop();
          opponent.evidenceArea.push(stolenCard);
          this.addLog(`コンタクト成功: 相手の証拠を1枚奪い、自分の証拠エリアに移しました。`);
        }
      }
    } else {
      this.addLog(`コンタクト失敗: ${guardChar.name} のAPが上回ったため、${attacker.name} は敗北しましたが、リムーブされません。`);
    }

    // Restore attacker AP
    attacker.ap = originalAttackerAP;
    guardChar.state = "sleep";

    // End guard phase and return to attacker's turn
    const attackerPlayerIndex = this.guardPhase.attackerPlayerIndex;
    this.guardPhase.active = false;
    this.guardPhase = {
      active: false,
      attackerId: null,
      defenderId: null,
      targetId: null,
      targetType: null,
      attackerPlayerIndex: null,
      defenderPlayerIndex: null,
      cutInUsed: false,
      disguiseUsed: false
    };
    this.currentPhase = PHASES.MAIN;
    this.turnOwner = attackerPlayerIndex;
    this.addLog(`${this.players[this.turnOwner].name} のターンに戻りました。`);
    this.triggerStateChange();
    return true;
  }

  // Skip guard (defender chooses not to guard)
  skipGuard() {
    if (!this.guardPhase.active || this.currentPhase !== PHASES.GUARD) {
      return false;
    }

    const attackerLoc = this.findCard(this.guardPhase.attackerId);
    const attacker = attackerLoc.card;

    this.addLog(`${this.players[this.guardPhase.defenderPlayerIndex].name} はガードしませんでした。`);

    // Resolve the action without guard
    if (this.guardPhase.targetType === "character") {
      const targetLoc = this.findCard(this.guardPhase.targetId);
      const targetChar = targetLoc.card;

      this.addLog(`${this.players[this.guardPhase.attackerPlayerIndex].name} の ${attacker.name} (AP ${attacker.ap}) vs ${targetChar.name} (AP ${targetChar.ap})`);

      if (attacker.ap >= targetChar.ap) {
        const targetPlayer = this.players[targetLoc.playerIndex];
        targetPlayer.field[targetLoc.index] = null;
        targetPlayer.removeArea.push(targetChar);
        this.addLog(`コンタクト成功: ${attacker.name} が ${targetChar.name} を倒し、リムーブエリアへ送りました。`);
      } else {
        this.addLog(`コンタクト失敗: ${targetChar.name} のAPが上回ったため、${attacker.name} は敗北しましたが、リムーブされません。`);
      }
    } else if (this.guardPhase.targetType === "case") {
      const opponent = this.players[this.guardPhase.attackerPlayerIndex];
      const defender = this.players[this.guardPhase.defenderPlayerIndex];

      if (defender.evidenceArea.length === 0) {
        this.addLog("相手に証拠がないため、証拠奪取は失敗しました。");
      } else {
        const stolenCard = defender.evidenceArea.pop();
        opponent.evidenceArea.push(stolenCard);
        this.addLog(`コンタクト成功: 相手の証拠を1枚奪い、自分の証拠エリアに移しました。`);
      }
    }

    // End guard phase and return to attacker's turn
    const attackerPlayerIndex = this.guardPhase.attackerPlayerIndex;
    this.guardPhase.active = false;
    this.guardPhase = {
      active: false,
      attackerId: null,
      defenderId: null,
      targetId: null,
      targetType: null,
      attackerPlayerIndex: null,
      defenderPlayerIndex: null,
      cutInUsed: false,
      disguiseUsed: false
    };
    this.currentPhase = PHASES.MAIN;
    this.turnOwner = attackerPlayerIndex;
    this.addLog(`${this.players[this.turnOwner].name} のターンに戻りました。`);
    this.triggerStateChange();
    return true;
  }

  // Use Cut-in during contact
  useCutIn(playerIndex, cardInstanceId) {
    if (!this.guardPhase.active || this.currentPhase !== PHASES.GUARD) {
      this.addLog("カットインはガードフェーズ中のみ使用できます。");
      return false;
    }

    if (this.guardPhase.cutInUsed) {
      this.addLog("カットインは1回のコンタクトにつき1枚だけ使用できます。");
      return false;
    }

    const cardLoc = this.findCard(cardInstanceId);
    if (!cardLoc || cardLoc.playerIndex !== playerIndex || cardLoc.area !== "hand") {
      this.addLog("手札からカットインカードを選択してください。");
      return false;
    }

    const card = cardLoc.card;
    if (!card.cutIn) {
      this.addLog("このカードはカットイン能力を持っていません。");
      return false;
    }

    // Remove from hand and add to remove area
    const player = this.players[playerIndex];
    player.hand.splice(cardLoc.index, 1);
    player.removeArea.push(card);

    // Apply cut-in effect
    const attackerLoc = this.findCard(this.guardPhase.attackerId);
    if (!attackerLoc) {
      this.addLog("攻撃キャラが見つかりません。");
      return false;
    }

    const attacker = attackerLoc.card;
    const originalAP = attacker.ap;

    // Parse cut-in effect (e.g., "AP＋2000")
    if (card.cutIn.includes("AP＋")) {
      const apBonus = parseInt(card.cutIn.replace("AP＋", ""));
      attacker.ap += apBonus;
      this.addLog(`${player.name} が ${card.name} をカットインで使用！ ${attacker.name} のAPを +${apBonus} しました。（${originalAP} → ${attacker.ap}）`);
    }

    this.guardPhase.cutInUsed = true;
    this.triggerCutIn("cutin", card, card.cutIn);
    this.triggerStateChange();
    return true;
  }

  // Use Disguise during contact
  useDisguise(playerIndex, cardInstanceId) {
    if (!this.guardPhase.active || this.currentPhase !== PHASES.GUARD) {
      this.addLog("変装はガードフェーズ中のみ使用できます。");
      return false;
    }

    if (this.guardPhase.disguiseUsed) {
      this.addLog("変装は1回のコンタクトにつき1枚だけ使用できます。");
      return false;
    }

    const cardLoc = this.findCard(cardInstanceId);
    if (!cardLoc || cardLoc.playerIndex !== playerIndex || cardLoc.area !== "hand") {
      this.addLog("手札から変装カードを選択してください。");
      return false;
    }

    const card = cardLoc.card;
    if (!card.disguise) {
      this.addLog("このカードは変装能力を持っていません。");
      return false;
    }

    // Determine which player is using disguise (attacker or defender)
    const isAttacker = (playerIndex === this.guardPhase.attackerPlayerIndex);
    const targetCharId = isAttacker ? this.guardPhase.attackerId : this.guardPhase.defenderId;

    if (!targetCharId) {
      this.addLog("変装対象のキャラが見つかりません。");
      return false;
    }

    const targetLoc = this.findCard(targetCharId);
    if (!targetLoc || targetLoc.area !== "field") {
      this.addLog("変装対象のキャラが現場にいません。");
      return false;
    }

    const player = this.players[playerIndex];
    const originalCard = targetLoc.card;

    // Save original card's state and effects
    const originalState = originalCard.state;
    const originalAP = originalCard.ap;

    // Remove from hand
    player.hand.splice(cardLoc.index, 1);

    // Replace the character in the field
    card.state = originalState; // Inherit state
    card.ap = originalAP; // Inherit AP (will be recalculated based on card's base AP)
    player.field[targetLoc.index] = card;

    // Move original card to bottom of deck (face down)
    originalCard.state = "facedown";
    player.deck.push(originalCard);

    // Update guard phase reference
    if (isAttacker) {
      this.guardPhase.attackerId = card.instanceId;
    } else {
      this.guardPhase.defenderId = card.instanceId;
    }

    this.guardPhase.disguiseUsed = true;
    this.addLog(`${player.name} が ${card.name} で変装！ ${originalCard.name} と入れ替わりました。`);
    this.triggerCutIn("disguise", card, "変装！");
    this.triggerStateChange();
    return true;
  }

  // Attempt Game Resolution (Win)
  checkGameWinningCondition(playerIndex) {
    const player = this.players[playerIndex];
    if (!player.partner || !player.caseCard) return false;

    // Check if Partner is active (needs to sleep to resolve)
    if (player.partner.state !== "active") {
      this.addLog(`勝利宣言不可: パートナー ${player.partner.name} がアクティブではありません。`);
      return false;
    }

    // Check if in solution phase
    if (this.currentGamePhase !== GAME_PHASES.SOLUTION) {
      this.addLog(`勝利宣言不可: 解決編状態ではありません。`);
      return false;
    }

    // Check evidence count vs case level (incident level)
    // Use firstPlayerLevel for player 0 (first player), secondPlayerLevel for player 1
    const caseLevel = (playerIndex === 0) 
      ? (player.caseCard.firstPlayerLevel || player.caseCard.level)
      : (player.caseCard.secondPlayerLevel || player.caseCard.level);
    
    if (player.evidenceArea.length < caseLevel) {
      this.addLog(`勝利宣言不可: 証拠の枚数（${player.evidenceArea.length}枚）が、事件レベル（${caseLevel}）未満です。`);
      return false;
    }

    // Solve the case!
    player.partner.state = "sleep";
    this.addLog(`★勝利宣言★ ${player.name} がパートナー「${player.partner.name}」の【事件解決】能力を使用！「${player.caseCard.name}」を解決し、ゲームに勝利しました！`);
    alert(`【ゲーム終了】\n${player.name}の勝利です！\n事件「${player.caseCard.name}」を解決しました！`);
    this.triggerStateChange();
    return true;
  }

  // Card movement in Sandbox Mode
  moveCard(instanceId, targetArea, targetPlayerIndex, additionalOptions = {}) {
    console.log("moveCard called:", instanceId, targetArea, targetPlayerIndex);
    const loc = this.findCard(instanceId);
    if (!loc) {
      this.addLog(`エラー: カード(ID: ${instanceId})が見つかりません。`);
      return false;
    }

    const { card, playerIndex: sourcePlayerIndex, area: sourceArea, index: sourceIndex } = loc;
    const sourcePlayer = this.players[sourcePlayerIndex];
    const targetPlayer = this.players[targetPlayerIndex];

    // Track if this is a partner moving to FILE area (for solution phase transition)
    const isPartnerMovingToFile = (card.type === "Partner" && sourceArea === "partner" && targetArea === "fileArea");

    // Remove from source
    if (sourceArea === "partner") {
      sourcePlayer.partner = null;
    } else if (sourceArea === "caseCard") {
      sourcePlayer.caseCard = null;
    } else if (sourceArea === "hand") {
      sourcePlayer.hand.splice(sourceIndex, 1);
    } else if (sourceArea === "fileArea") {
      sourcePlayer.fileArea.splice(sourceIndex, 1);
    } else if (sourceArea === "evidenceArea") {
      sourcePlayer.evidenceArea.splice(sourceIndex, 1);
    } else if (sourceArea === "removeArea") {
      sourcePlayer.removeArea.splice(sourceIndex, 1);
    } else if (sourceArea === "field") {
      sourcePlayer.field[sourceIndex] = null;
    } else if (sourceArea === "deck") {
      sourcePlayer.deck.splice(sourceIndex, 1);
    }

    // Configure card face and status based on target area
    if (targetArea === "fileArea" || targetArea === "evidenceArea") {
      card.state = "facedown";
    } else if (targetArea === "deck") {
      card.state = "facedown";
    } else {
      card.state = "active";
    }
    card.nameDeclaring = false; // Reset summoning sickness when moved manually

    // Insert into target
    if (targetArea === "partner") {
      targetPlayer.partner = card;
      this.addLog(`[サンドボックス] ${card.name} を ${targetPlayer.name} のパートナーエリアに移動しました。`);
    } else if (targetArea === "caseCard") {
      targetPlayer.caseCard = card;
      this.addLog(`[サンドボックス] ${card.name} を ${targetPlayer.name} の事件エリアに移動しました。`);
    } else if (targetArea === "hand") {
      targetPlayer.hand.push(card);
      this.addLog(`[サンドボックス] ${card.name} を ${targetPlayer.name} の手札に移動しました。`);
    } else if (targetArea === "fileArea") {
      targetPlayer.fileArea.push(card);
      this.addLog(`[サンドボックス] ${card.name} を ${targetPlayer.name} のFILEエリアに移動しました。`);

      // Check if partner moved to FILE area to transition to solution phase
      // Official rule: FILE area must have 7+ cards (including partner) to transition to solution phase
      if (isPartnerMovingToFile) {
        const fileThreshold = 7;
        if (targetPlayer.fileArea.length >= fileThreshold && targetPlayer.gamePhase === GAME_PHASES.INCIDENT) {
          targetPlayer.gamePhase = GAME_PHASES.SOLUTION;
          this.addLog(`★解決編開始★ ${targetPlayer.name} のFILEが${fileThreshold}枚以上になり、パートナーがFILEエリアへ移動しました。解決編に突入します！`);
        }
      }
    } else if (targetArea === "evidenceArea") {
      targetPlayer.evidenceArea.push(card);
      this.addLog(`[サンドボックス] ${card.name} を ${targetPlayer.name} の証拠エリアに移動しました。`);
    } else if (targetArea === "removeArea") {
      targetPlayer.removeArea.push(card);
      this.addLog(`[サンドボックス] ${card.name} を ${targetPlayer.name} のリムーブエリアに移動しました。`);
    } else if (targetArea === "field") {
      let slot = additionalOptions.slot;
      if (slot === undefined || slot < 0 || slot >= 5) {
        slot = targetPlayer.field.findIndex(s => s === null);
        if (slot === -1) slot = 0;
      }
      
      const displaced = targetPlayer.field[slot];
      if (displaced) {
        targetPlayer.removeArea.push(displaced);
        this.addLog(`[サンドボックス] スロット${slot + 1} にあった ${displaced.name} は押し出されてリムーブエリアへ送られました。`);
      }
      targetPlayer.field[slot] = card;
      this.addLog(`[サンドボックス] ${card.name} を ${targetPlayer.name} の現場スロット${slot + 1}に移動しました。`);
    } else if (targetArea === "deck") {
      if (additionalOptions.top) {
        targetPlayer.deck.push(card);
      } else {
        targetPlayer.deck.unshift(card); // insert at bottom of deck
      }
      this.addLog(`[サンドボックス] ${card.name} を ${targetPlayer.name} の山札に移動しました。`);
    }

     this.triggerStateChange();
     return true;
   }

   // Register a callback function to be called when the state changes
   subscribe(callback) {
     this.onStateChangeCallbacks.push(callback);
   }

   // Toggle card rotation state
   toggleCardState(instanceId, stateType) {
     const loc = this.findCard(instanceId);
     if (!loc) return false;

     const card = loc.card;
     if (stateType === "active" || stateType === "sleep" || stateType === "stun") {
       const oldState = card.state;
       card.state = stateType;
       this.addLog(`[サンドボックス] ${card.name} の状態を ${oldState} から ${stateType} に変更しました。`);
       this.triggerStateChange();
       return true;
     }
     return false;
   }
 }

 if (typeof window !== "undefined") {
   window.PHASES = PHASES;
   window.ConanTCGState = ConanTCGState;
 }
