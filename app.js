// Detective Conan TCG Application Controller & UI Renderer
console.log("app.js loaded - version 1.1");

document.addEventListener("DOMContentLoaded", () => {
  console.log("DOMContentLoaded fired");

  // Check if required globals are loaded
  if (typeof PHASES === "undefined") {
    console.error("PHASES is not defined. Make sure state.js is loaded before app.js.");
    return;
  }
  if (typeof ConanTCGState === "undefined") {
    console.error("ConanTCGState is not defined. Make sure state.js is loaded before app.js.");
    return;
  }

  console.log("Starting app initialization");

  // Initialize State
  const state = new ConanTCGState();
  
  // Track zoomed card
  let activeZoomCard = null;
  let zoomTimeout = null;

  // Track context menu target card
  let contextCardId = null;

  // DOM Elements
  const turnNumberVal = document.getElementById("turn-number-val");
  const currentPlayerName = document.getElementById("current-player-name");
  const currentPlayerBadge = document.getElementById("current-player-badge");
  
  const phaseAuto = document.getElementById("phase-auto");
  const phaseMain = document.getElementById("phase-main");
  const phaseGuard = document.getElementById("phase-guard");
  const phaseEnd = document.getElementById("phase-end");
  const gamePhaseValue = document.getElementById("game-phase-value");
  
  const btnNextPhase = document.getElementById("btn-next-phase");
  const btnSolveCase = document.getElementById("btn-solve-case");
  const btnNextHint = document.getElementById("btn-next-hint");
  const btnSkipHintPlay = document.getElementById("btn-skip-hint-play");
  const btnSwapPlayer = document.getElementById("btn-swap-player");
  const btnSkipGuard = document.getElementById("btn-skip-guard");
  
  const sbBtnDraw = document.getElementById("sb-btn-draw");
  const sbBtnAddFile = document.getElementById("sb-btn-add-file");
  const sbBtnAddEvidence = document.getElementById("sb-btn-add-evidence");
  const sbBtnReset = document.getElementById("sb-btn-reset");

  const zoneCase = document.getElementById("zone-case");
  const zonePartner = document.getElementById("zone-partner");
  const zoneDeck = document.getElementById("zone-deck");
  const zoneRemove = document.getElementById("zone-remove");
  const zoneFile = document.getElementById("zone-file");
  const zoneEvidence = document.getElementById("zone-evidence");
  const zoneField = document.getElementById("zone-field");
  const zoneHand = document.getElementById("zone-hand");
  
  const deckCount = document.getElementById("deck-count");
  const removeCount = document.getElementById("remove-count");
  const fileCount = document.getElementById("file-count");
  const evidenceCount = document.getElementById("evidence-count");
  const handCount = document.getElementById("hand-count");
  const playStatusMsg = document.getElementById("play-status-msg");

  const oppFileCount = document.getElementById("opp-file-count");
  const oppEvidenceCount = document.getElementById("opp-evidence-count");
  const oppHandCount = document.getElementById("opp-hand-count");
  
  const logBox = document.getElementById("log-box");
  const btnClearLogs = document.getElementById("btn-clear-logs");
  
  // Modal Elements
  const zoomModal = document.getElementById("zoom-modal");
  const btnCloseModal = document.getElementById("btn-close-modal");
  const removeAreaModal = document.getElementById("remove-area-modal");
  const btnCloseRemoveModal = document.getElementById("btn-close-remove-modal");
  const removeCardList = document.getElementById("remove-card-list");
  const zoomLevel = document.getElementById("zoom-level");
  const zoomName = document.getElementById("zoom-name");
  const zoomType = document.getElementById("zoom-type");
  const zoomColor = document.getElementById("zoom-color");
  const zoomInitials = document.getElementById("zoom-initials");
  const zoomAp = document.getElementById("zoom-ap");
  const zoomLp = document.getElementById("zoom-lp");
  const zoomInfoName = document.getElementById("zoom-info-name");
  const zoomTagColor = document.getElementById("zoom-tag-color");
  const zoomTagType = document.getElementById("zoom-tag-type");
  const zoomTraits = document.getElementById("zoom-traits");
  const zoomAbility = document.getElementById("zoom-ability");
  const zoomCardFrame = document.getElementById("zoom-card-frame");
  const zoomContactAbilities = document.getElementById("zoom-contact-abilities");
  const zoomCutin = document.getElementById("zoom-cutin");
  const zoomCutinText = document.getElementById("zoom-cutin-text");
  const zoomDisguise = document.getElementById("zoom-disguise");

  // Context Menus
  const cardContextMenu = document.getElementById("card-context-menu");
  const cardActionTargetMenu = document.getElementById("card-action-target-menu");
  const actionTargetList = document.getElementById("action-target-list");
  const btnCancelActionTarget = document.getElementById("btn-cancel-action-target");

  // --- RENDERING FUNCTIONS ---

  function renderCard(card, isFacedown = false) {
    const cardEl = document.createElement("div");
    cardEl.className = `tcg-card color-${card.color.toLowerCase()}`;
    cardEl.dataset.cardId = card.instanceId;
    cardEl.draggable = !isFacedown;

    if (isFacedown) {
      cardEl.classList.add("state-facedown");
      cardEl.title = "裏向きのカード";
    } else {
      if (card.state === "sleep") cardEl.classList.add("state-sleep");
      if (card.state === "stun") cardEl.classList.add("state-stun");

      // Card Header
      const header = document.createElement("div");
      header.className = "card-header-row";
      
      const level = document.createElement("div");
      level.className = "card-level";
      
      // For Case cards, show first/second player level based on current player
      if (card.type === "Case" && card.firstPlayerLevel && card.secondPlayerLevel) {
        const currentPIdx = state.turnOwner;
        level.textContent = (currentPIdx === 0) ? card.firstPlayerLevel : card.secondPlayerLevel;
      } else {
        level.textContent = card.level;
      }
      
      const dot = document.createElement("div");
      dot.className = "card-color-dot";
      
      header.appendChild(level);
      header.appendChild(dot);
      cardEl.appendChild(header);

      // Name
      const name = document.createElement("div");
      name.className = "card-name";
      name.textContent = card.name;
      cardEl.appendChild(name);

      // Illust Placeholder
      const illust = document.createElement("div");
      illust.className = "card-illust-mini";
      illust.textContent = card.name.charAt(0);
      
      // Type tag
      const typeLabel = document.createElement("span");
      typeLabel.className = "card-type-label";
      typeLabel.textContent = card.type === "Character" ? "キャラ" : card.type === "Event" ? "イベント" : card.type;
      illust.appendChild(typeLabel);

      cardEl.appendChild(illust);

      // Sickness Indicator
      if (card.nameDeclaring) {
        const sickness = document.createElement("div");
        sickness.className = "card-sickness-indicator";
        sickness.textContent = "名乗り";
        cardEl.appendChild(sickness);
      }

      // Specs (AP / LP)
      if (card.type === "Character" || card.type === "Partner") {
        const specs = document.createElement("div");
        specs.className = "card-specs";
        
        const ap = document.createElement("div");
        ap.className = "ap-val";
        ap.textContent = `AP:${card.ap}`;
        
        const lp = document.createElement("div");
        lp.className = "lp-val";
        lp.textContent = `LP:${card.lp}`;
        
        specs.appendChild(ap);
        specs.appendChild(lp);
        cardEl.appendChild(specs);
      }
    }

    // Attach interaction events
    attachCardEvents(cardEl, card);

    return cardEl;
  }

  function renderBoard() {
    const pIdx = state.turnOwner;
    const oppIdx = 1 - pIdx;
    
    const activePlayer = state.players[pIdx];
    const opponentPlayer = state.players[oppIdx];

    // Header values
    turnNumberVal.textContent = state.turnNumber;
    currentPlayerName.textContent = activePlayer.name;
    currentPlayerBadge.className = `player-turn-badge active-p${pIdx + 1}`;

    // Update opponent info
    oppFileCount.textContent = opponentPlayer.fileArea.length;
    oppEvidenceCount.textContent = opponentPlayer.evidenceArea.length;
    oppHandCount.textContent = opponentPlayer.hand.length;
    
    // Phase highlighting
    phaseAuto.classList.toggle("active", state.currentPhase === PHASES.AUTO);
    phaseMain.classList.toggle("active", state.currentPhase === PHASES.MAIN);
    phaseGuard.classList.toggle("active", state.currentPhase === PHASES.GUARD);
    phaseEnd.classList.toggle("active", state.currentPhase === PHASES.END);

    // Game phase display (show active player's game phase)
    if (activePlayer.gamePhase === GAME_PHASES.INCIDENT) {
      gamePhaseValue.textContent = "事件編";
      gamePhaseValue.style.color = "var(--accent-neon)";
    } else if (activePlayer.gamePhase === GAME_PHASES.SOLUTION) {
      gamePhaseValue.textContent = "解決編";
      gamePhaseValue.style.color = "var(--accent-gold)";
    }

    // Render Case
    zoneCase.innerHTML = "";
    if (activePlayer.caseCard) {
      zoneCase.appendChild(renderCard(activePlayer.caseCard));
      zoneCase.classList.remove("empty");
    } else {
      zoneCase.innerHTML = '<span class="slot-placeholder">事件カード</span>';
      zoneCase.classList.add("empty");
    }

    // Render Partner
    zonePartner.innerHTML = "";
    if (activePlayer.partner) {
      zonePartner.appendChild(renderCard(activePlayer.partner));
      zonePartner.classList.remove("empty");
    } else {
      zonePartner.innerHTML = '<span class="slot-placeholder">パートナー</span>';
      zonePartner.classList.add("empty");
    }

    // Render Piles count
    deckCount.textContent = activePlayer.deck.length;
    
    zoneRemove.innerHTML = "";
    removeCount.textContent = activePlayer.removeArea.length;
    zoneRemove.innerHTML = "";
    if (activePlayer.removeArea.length > 0) {
      activePlayer.removeArea.forEach(card => {
        const cardEl = renderCard(card);
        zoneRemove.appendChild(cardEl);
      });
      zoneRemove.classList.remove("empty");
      zoneRemove.addEventListener("click", openRemoveAreaModal);
    } else {
      zoneRemove.innerHTML = `<div class="pile-count">0</div><span class="slot-placeholder">REMOVE</span>`;
      zoneRemove.classList.add("empty");
      zoneRemove.removeEventListener("click", openRemoveAreaModal);
    }

    // Render FILE stack
    zoneFile.innerHTML = "";
    fileCount.textContent = `${activePlayer.fileArea.length}枚`;
    if (activePlayer.fileArea.length > 0) {
      activePlayer.fileArea.forEach(card => {
        // Render file cards as smaller mini representations
        const fileCard = document.createElement("div");
        fileCard.className = "file-card-mini";
        fileCard.dataset.cardId = card.instanceId;
        fileCard.title = "FILE (裏向きコスト)";
        attachCardEvents(fileCard, card);
        zoneFile.appendChild(fileCard);
      });
    } else {
      zoneFile.innerHTML = '<div class="file-empty-placeholder">FILEがありません</div>';
    }

    // Render Evidence stack
    zoneEvidence.innerHTML = "";
    evidenceCount.textContent = `${activePlayer.evidenceArea.length}枚`;
    if (activePlayer.evidenceArea.length > 0) {
      activePlayer.evidenceArea.forEach(card => {
        const evCard = document.createElement("div");
        evCard.className = "evidence-card-mini";
        evCard.dataset.cardId = card.instanceId;
        evCard.title = "証拠 (裏向き)";
        attachCardEvents(evCard, card);
        zoneEvidence.appendChild(evCard);
      });
    } else {
      zoneEvidence.innerHTML = '<div class="evidence-empty-placeholder">証拠がありません</div>';
    }

    // Render Field (Divide Field section into Opponent Field at top, Player Field at bottom)
    zoneField.innerHTML = "";
    
    // Opponent Row Header
    const oppHeader = document.createElement("div");
    oppHeader.className = "field-row-header";
    oppHeader.innerHTML = `<span>相手 (${opponentPlayer.name}) の現場</span>`;
    oppHeader.style.gridColumn = "span 5";
    oppHeader.style.fontSize = "0.6rem";
    oppHeader.style.color = "var(--text-secondary)";
    oppHeader.style.borderBottom = "1px solid rgba(255,255,255,0.03)";
    oppHeader.style.paddingBottom = "2px";
    oppHeader.style.marginBottom = "5px";
    zoneField.appendChild(oppHeader);

    // Opponent Field slots (Rendered from active player's viewpoint, slots 0 to 4)
    for (let i = 0; i < 5; i++) {
      const oppSlot = document.createElement("div");
      oppSlot.className = "card-slot character-slot opponent-field-slot empty";
      oppSlot.dataset.slotIdx = i;
      oppSlot.dataset.playerIdx = oppIdx;
      
      const card = opponentPlayer.field[i];
      if (card) {
        oppSlot.appendChild(renderCard(card));
        oppSlot.classList.remove("empty");
        // add border to clearly mark opponent card
        oppSlot.firstChild.style.borderStyle = "dotted";
      } else {
        oppSlot.innerHTML = '<span class="slot-placeholder">空き</span>';
      }
      zoneField.appendChild(oppSlot);
    }

    // Divider
    const fieldDivider = document.createElement("div");
    fieldDivider.style.gridColumn = "span 5";
    fieldDivider.style.height = "10px";
    zoneField.appendChild(fieldDivider);

    // Active Player Row Header
    const activeHeader = document.createElement("div");
    activeHeader.className = "field-row-header";
    activeHeader.innerHTML = `<span>自分 (${activePlayer.name}) の現場</span>`;
    activeHeader.style.gridColumn = "span 5";
    activeHeader.style.fontSize = "0.6rem";
    activeHeader.style.color = "var(--color-blue)";
    activeHeader.style.borderBottom = "1px solid rgba(0, 136, 255, 0.1)";
    activeHeader.style.paddingBottom = "2px";
    activeHeader.style.marginBottom = "5px";
    zoneField.appendChild(activeHeader);

    // Player Field slots
    for (let i = 0; i < 5; i++) {
      const playSlot = document.createElement("div");
      playSlot.className = "card-slot character-slot player-field-slot empty";
      playSlot.dataset.slotIdx = i;
      playSlot.dataset.playerIdx = pIdx;
      
      const card = activePlayer.field[i];
      if (card) {
        playSlot.appendChild(renderCard(card));
        playSlot.classList.remove("empty");
      } else {
        playSlot.innerHTML = '<span class="slot-placeholder">出撃枠</span>';
      }

      // Drag and drop event handlers
      playSlot.addEventListener("dragover", handleDragOver);
      playSlot.addEventListener("dragleave", handleDragLeave);
      playSlot.addEventListener("drop", (e) => handleDropOnField(e, i));

      zoneField.appendChild(playSlot);
    }

    // Render Hand
    zoneHand.innerHTML = "";
    handCount.textContent = `${activePlayer.hand.length}枚`;
    if (activePlayer.hand.length > 0) {
      activePlayer.hand.forEach(card => {
        zoneHand.appendChild(renderCard(card));
      });
    } else {
      zoneHand.innerHTML = '<div class="hand-empty-placeholder">手札がありません</div>';
    }

    // Update play status msg
    if (state.currentPhase === PHASES.GUARD) {
      playStatusMsg.textContent = "ガードフェーズ: ガードするキャラを選択するか、「ガードしない」をクリックしてください。";
      playStatusMsg.style.color = "var(--color-red)";
      btnSkipGuard.classList.remove("hide");
    } else if (state.turnActions.hasUsedNextHintThisTurn) {
      if (state.turnActions.nextHintCount > 0) {
        playStatusMsg.textContent = "ネクストヒントにより追加でカードを使用できます。";
        playStatusMsg.style.color = "var(--accent-neon)";
        btnSkipHintPlay.classList.remove("hide");
      } else {
        playStatusMsg.textContent = "ネクストヒント済。手札を使用するには次のネクストヒントが必要です。";
        playStatusMsg.style.color = "var(--text-secondary)";
        btnSkipHintPlay.classList.add("hide");
      }
      btnSkipGuard.classList.add("hide");
    } else {
      btnSkipHintPlay.classList.add("hide");
      btnSkipGuard.classList.add("hide");
      if (state.turnActions.normalPlayUsed) {
        playStatusMsg.textContent = "今ターンの通常の手札プレイは使用済みです。";
        playStatusMsg.style.color = "var(--text-secondary)";
      } else {
        playStatusMsg.textContent = "通常の手札プレイが1回可能です（事件と同じ色のレベル以下のカード）。";
        playStatusMsg.style.color = "var(--color-blue)";
      }
    }

    // Update Logs
    renderLogs();
  }

  function renderLogs() {
    logBox.innerHTML = "";
    state.logs.forEach(log => {
      const entry = document.createElement("div");
      entry.className = "log-entry";
      
      // Style entries based on content keywords
      if (log.includes("エラー") || log.includes("不可") || log.includes("警告")) {
        entry.classList.add("error-msg");
      } else if (log.includes("勝利")) {
        entry.classList.add("victory-msg");
      } else if (log.includes("開始") || log.includes("オートフェイズ")) {
        entry.classList.add("system-msg");
      } else if (log.includes("登場") || log.includes("ドロー") || log.includes("ネクストヒント")) {
        entry.classList.add("success-msg");
      }
      
      entry.textContent = log;
      logBox.appendChild(entry);
    });
    // Auto scroll logs
    logBox.scrollTop = logBox.scrollHeight;
  }

  // --- CARD INTERACTION INTERACTION EVENTS ---

  function attachCardEvents(cardEl, card) {
    // 1. Long press zoom (500ms)
    cardEl.addEventListener("pointerdown", (e) => {
      if (e.button !== 0 && e.pointerType === "mouse") return; // Allow touch/pen pointers
      clearTimeout(zoomTimeout);
      zoomTimeout = setTimeout(() => {
        openZoomModal(card);
      }, 500);
    });

    const clearZoomTimer = () => {
      clearTimeout(zoomTimeout);
    };

    cardEl.addEventListener("pointerup", clearZoomTimer);
    cardEl.addEventListener("pointermove", clearZoomTimer); // Cancel on drag/move
    cardEl.addEventListener("pointerleave", clearZoomTimer);
    cardEl.addEventListener("dragstart", (e) => {
      clearZoomTimer();
      cardEl.classList.add("dragging");
      if (e.dataTransfer) {
        e.dataTransfer.setData("text/plain", card.instanceId);
      }
    });
    cardEl.addEventListener("dragend", () => {
      cardEl.classList.remove("dragging");
    });

    // 2. Double click zoom
    cardEl.addEventListener("dblclick", () => {
      openZoomModal(card);
    });

    // 3. Right-click context menu (Context menu is very robust for testing)
    cardEl.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      clearZoomTimer();
      openContextMenu(e, card.instanceId);
    });

    // Tap support: click/tap triggers context menu for ALL cards (including hand) to allow easy mobile play
    cardEl.addEventListener("click", (e) => {
      const cardLoc = state.findCard(card.instanceId);
      if (cardLoc) {
        e.preventDefault();
        e.stopPropagation();
        openContextMenu(e, card.instanceId);
      }
    });
  }

  // --- DRAG & DROP HANDLERS ---

  function handleDragOver(e) {
    e.preventDefault();
    e.currentTarget.classList.add("dragover");
  }

  function handleDragLeave(e) {
    e.currentTarget.classList.remove("dragover");
  }

  function handleDropOnField(e, slotIdx) {
    e.preventDefault();
    e.currentTarget.classList.remove("dragover");
    
    const cardId = e.dataTransfer.getData("text/plain");
    const loc = state.findCard(cardId);
    if (!loc) return;

    const pIdx = state.turnOwner;

    // Rules-enforced play if coming from active player's hand
    if (loc.area === "hand" && loc.playerIndex === pIdx) {
      state.playCard(pIdx, cardId, slotIdx);
    } else {
      // Sandbox movement if coming from elsewhere
      state.moveCard(cardId, "field", pIdx, { slot: slotIdx });
    }
  }

  // Set up sandbox drop zones (allows dropping cards anywhere in sandbox mode)
  [zoneHand, zoneFile, zoneEvidence, zoneRemove].forEach(zone => {
    zone.addEventListener("dragover", (e) => {
      e.preventDefault();
      zone.classList.add("dragover");
    });
    zone.addEventListener("dragleave", () => {
      zone.classList.remove("dragover");
    });
    zone.addEventListener("drop", (e) => {
      e.preventDefault();
      zone.classList.remove("dragover");
      
      const cardId = e.dataTransfer.getData("text/plain");
      const targetZone = zone.dataset.zone;
      const activePIdx = state.turnOwner;

      if (cardId && targetZone) {
        state.moveCard(cardId, targetZone, activePIdx);
      }
    });
  });

  // Click deck to draw
  zoneDeck.addEventListener("click", () => {
    state.drawCard(state.turnOwner);
  });

  // --- ZOOM MODAL ACTIONS ---

  function openZoomModal(card) {
    activeZoomCard = card;
    
    // For Case cards, show first/second player level based on current player
    if (card.type === "Case" && card.firstPlayerLevel && card.secondPlayerLevel) {
      const currentPIdx = state.turnOwner;
      const displayLevel = (currentPIdx === 0) ? card.firstPlayerLevel : card.secondPlayerLevel;
      zoomLevel.textContent = displayLevel;
    } else {
      zoomLevel.textContent = card.level;
    }
    
    zoomName.textContent = card.name;
    zoomType.textContent = card.type === "Character" ? "キャラ" : card.type === "Event" ? "イベント" : card.type;
    zoomColor.textContent = card.color;
    zoomInitials.textContent = card.name.charAt(0);
    
    zoomAp.textContent = card.ap || 0;
    zoomLp.textContent = card.lp || 0;
    
    zoomInfoName.textContent = card.name;
    zoomTagColor.textContent = card.color;
    zoomTagColor.className = `tag-color color-${card.color.toLowerCase()}`;
    zoomTagType.textContent = card.type;
    
    zoomTraits.textContent = card.traits.join(", ");

    // For Case cards, dynamically generate ability text with first/second player levels
    if (card.type === "Case" && card.firstPlayerLevel && card.secondPlayerLevel) {
      const dynamicAbilityText = `【解決編条件】自分のFILEエリアが7枚以上。\n【事件レベル】先攻${card.firstPlayerLevel}/後攻${card.secondPlayerLevel}（証拠が事件レベル以上で解決可能）`;
      zoomAbility.textContent = dynamicAbilityText;
    } else {
      zoomAbility.textContent = card.abilityText || "効果テキストなし";
    }

    // Show/hide contact abilities (cut-in/disguise)
    if (card.cutIn || card.disguise) {
      zoomContactAbilities.style.display = "block";
      if (card.cutIn) {
        zoomCutin.style.display = "block";
        zoomCutinText.textContent = card.cutIn;
      } else {
        zoomCutin.style.display = "none";
      }
      if (card.disguise) {
        zoomDisguise.style.display = "block";
      } else {
        zoomDisguise.style.display = "none";
      }
    } else {
      zoomContactAbilities.style.display = "none";
    }

    // Set colors of modal preview border
    zoomCardFrame.className = `zoom-card-visual color-${card.color.toLowerCase()}`;

    // Hide/show stats in visual panel based on type
    const visualSpecs = zoomCardFrame.querySelector(".card-bottom-specs");
    if (card.type === "Character" || card.type === "Partner") {
      visualSpecs.style.display = "flex";
    } else {
      visualSpecs.style.display = "none";
    }

    zoomModal.classList.remove("hide");
  }

  function closeZoomModal() {
    zoomModal.classList.add("hide");
    activeZoomCard = null;
  }

  function openRemoveAreaModal() {
    removeCardList.innerHTML = "";
    const activePlayer = state.players[state.turnOwner];
    if (activePlayer.removeArea.length > 0) {
      activePlayer.removeArea.forEach(card => {
        removeCardList.appendChild(renderCard(card));
      });
    }
    removeAreaModal.classList.remove("hide");
  }

  function closeRemoveAreaModal() {
    removeAreaModal.classList.add("hide");
  }

  btnCloseModal.addEventListener("click", closeZoomModal);
  zoomModal.addEventListener("click", (e) => {
    if (e.target === zoomModal) closeZoomModal();
  });
  btnCloseRemoveModal.addEventListener("click", closeRemoveAreaModal);
  removeAreaModal.addEventListener("click", (e) => {
    if (e.target === removeAreaModal) closeRemoveAreaModal();
  });

  // --- CONTEXT MENU ACTIONS ---

  function openContextMenu(e, cardId) {
    contextCardId = cardId;
    const loc = state.findCard(cardId);
    if (!loc) return;

    const { card, playerIndex, area } = loc;
    const activePIdx = state.turnOwner;

    // Populate context menu lists dynamically
    const list = cardContextMenu.querySelector(".context-menu-list");
    list.innerHTML = "";

    // Close any open menus
    closeAllMenus();

    // Guard phase options
    if (state.currentPhase === PHASES.GUARD && state.guardPhase.active) {
      if (area === "field" && playerIndex === activePIdx && card.state === "active") {
        addMenuOption(list, "🛡️ ガードする", () => state.performGuard(activePIdx, cardId));
      } else {
        addMenuOption(list, "(ガードできません)", () => {}, false);
      }
      // Always include zoom detail option
      addMenuOption(list, "🔍 詳細表示 (ズーム)", () => openZoomModal(card));

      // Show menu at click coordinates
      cardContextMenu.style.left = `${e.pageX}px`;
      cardContextMenu.style.top = `${e.pageY}px`;
      cardContextMenu.classList.remove("hide");
      return;
    }

    // Contextual options based on location
    if (area === "hand" && playerIndex === activePIdx) {
      // Cards in own hand
      if (card.type === "Character") {
        addMenuOption(list, "現場に登場させる（スロット1）", () => state.playCard(activePIdx, cardId, 0));
        addMenuOption(list, "現場に登場させる（スロット2）", () => state.playCard(activePIdx, cardId, 1));
        addMenuOption(list, "現場に登場させる（スロット3）", () => state.playCard(activePIdx, cardId, 2));
      } else if (card.type === "Event") {
        addMenuOption(list, "イベントを使用する", () => state.playCard(activePIdx, cardId));
      }
      addMenuOption(list, "FILEエリアへ置く", () => state.moveCard(cardId, "fileArea", activePIdx));
      addMenuOption(list, "証拠エリアへ置く", () => state.moveCard(cardId, "evidenceArea", activePIdx));

      // Cut-in and Disguise options during guard phase
      if (state.guardPhase.active && state.currentPhase === "GUARD") {
        if (card.cutIn && !state.guardPhase.cutInUsed) {
          addMenuOption(list, "🎬 カットインを使用", () => state.useCutIn(activePIdx, cardId));
        }
        if (card.disguise && !state.guardPhase.disguiseUsed) {
          addMenuOption(list, "🎭 変装を使用", () => state.useDisguise(activePIdx, cardId));
        }
      }
    } else if (area === "field" || area === "partner") {
      // Cards on board
      if (card.state === "active") {
        addMenuOption(list, "スリープする (横倒)", () => state.toggleCardState(cardId, "sleep"));
        addMenuOption(list, "スタンする (逆倒)", () => state.toggleCardState(cardId, "stun"));

        // Reasoning is allowed for Characters/Partners on your side
        if (playerIndex === activePIdx && !card.nameDeclaring) {
          addMenuOption(list, "🔍 推理を行う (証拠獲得)", () => state.performReasoning(activePIdx, cardId));
        }

        // Action is allowed for Characters on your side
        if (playerIndex === activePIdx && area === "field") {
          addMenuOption(list, "⚔️ アクションを仕掛ける...", (event) => openActionTargetMenu(event, cardId), false);
        }
      } else {
        addMenuOption(list, "アクティブに戻す (起立)", () => state.toggleCardState(cardId, "active"));
        if (card.state === "stun") {
          addMenuOption(list, "スリープにする (逆から横)", () => state.toggleCardState(cardId, "sleep"));
        }
      }

      addMenuOption(list, "手札に戻す", () => state.moveCard(cardId, "hand", playerIndex));
      addMenuOption(list, "FILEエリアへ移動", () => state.moveCard(cardId, "fileArea", playerIndex));
      addMenuOption(list, "リムーブする", () => state.moveCard(cardId, "removeArea", playerIndex));
    } else {
      // File, Evidence, Remove, etc.
      addMenuOption(list, "手札に戻す", () => state.moveCard(cardId, "hand", playerIndex));
      addMenuOption(list, "リムーブする", () => state.moveCard(cardId, "removeArea", playerIndex));
      if (area !== "fileArea") {
        addMenuOption(list, "FILEエリアへ移動", () => state.moveCard(cardId, "fileArea", playerIndex));
      }
    }

    // Always include zoom detail option
    addMenuOption(list, "🔍 詳細表示 (ズーム)", () => openZoomModal(card));

    // Show menu at click coordinates
    cardContextMenu.style.left = `${e.pageX}px`;
    cardContextMenu.style.top = `${e.pageY}px`;
    cardContextMenu.classList.remove("hide");
  }

  function addMenuOption(parent, label, action, closeMenu = true) {
    const li = document.createElement("li");
    li.textContent = label;
    li.addEventListener("click", (e) => {
      e.stopPropagation();
      action(e);
      if (closeMenu) {
        closeAllMenus();
      }
    });
    parent.appendChild(li);
  }

  function openActionTargetMenu(e, cardId) {
    actionTargetList.innerHTML = "";
    const activePIdx = state.turnOwner;
    const oppPIdx = 1 - activePIdx;
    const opponent = state.players[oppPIdx];

    // 1. Option: Attack opponent's Case (Evidence theft)
    if (opponent.evidenceArea.length > 0) {
      const li = document.createElement("li");
      li.textContent = `相手の事件 (証拠 ${opponent.evidenceArea.length}枚あり)`;
      li.addEventListener("click", () => {
        state.performAction(activePIdx, cardId, "case", "P2_CASE"); // target dummy opponent case
        closeAllMenus();
      });
      actionTargetList.appendChild(li);
    }

    // 2. Option: Attack opponent's sleep characters
    let hasTargets = false;
    opponent.field.forEach((char, idx) => {
      if (char && char.state === "sleep") {
        const li = document.createElement("li");
        li.textContent = `${char.name} (現場 ${idx + 1} / AP:${char.ap} - スリープ)`;
        li.addEventListener("click", () => {
          state.performAction(activePIdx, cardId, "character", char.instanceId);
          closeAllMenus();
        });
        actionTargetList.appendChild(li);
        hasTargets = true;
      }
    });

    if (actionTargetList.children.length === 0) {
      const li = document.createElement("li");
      li.textContent = "(有効なアクション対象がありません)";
      li.style.color = "var(--text-secondary)";
      li.style.cursor = "default";
      actionTargetList.appendChild(li);
    }

    // Position menu - ensure it stays within viewport
    const menuWidth = 200;
    const menuHeight = cardActionTargetMenu.offsetHeight || 200;
    const x = parseInt(cardContextMenu.style.left) || 0;
    const y = parseInt(cardContextMenu.style.top) || 0;

    // Adjust if menu would go off right edge
    const adjustedX = (x + menuWidth > window.innerWidth) ? window.innerWidth - menuWidth - 10 : x;
    // Adjust if menu would go off bottom edge
    const adjustedY = (y + menuHeight > window.innerHeight) ? window.innerHeight - menuHeight - 10 : y;

    cardActionTargetMenu.style.left = `${adjustedX}px`;
    cardActionTargetMenu.style.top = `${adjustedY}px`;

    cardContextMenu.classList.add("hide");

    // Temporarily allow body overflow to show menu
    document.body.style.overflow = "visible";

    cardActionTargetMenu.classList.remove("hide");
  }

  function closeAllMenus() {
    cardContextMenu.classList.add("hide");
    cardActionTargetMenu.classList.add("hide");
    // Restore body overflow
    document.body.style.overflow = "hidden";
  }

  btnCancelActionTarget.addEventListener("click", closeAllMenus);
  document.addEventListener("click", (e) => {
    if (!cardContextMenu.contains(e.target) && !cardActionTargetMenu.contains(e.target)) {
      closeAllMenus();
    }
  });

  // --- BUTTON CLICKS ---

  if (btnNextPhase) {
    btnNextPhase.addEventListener("click", () => {
      console.log("btnNextPhase clicked, current phase:", state.currentPhase);
      if (state.currentPhase === PHASES.AUTO) {
        state.transitionToPhase(PHASES.MAIN);
      } else if (state.currentPhase === PHASES.MAIN) {
        state.transitionToPhase(PHASES.END);
      } else if (state.currentPhase === PHASES.END) {
        // Auto triggers next turn starts
        state.endTurn();
      }
    });
  }

  if (btnSolveCase) {
    btnSolveCase.addEventListener("click", () => {
      state.checkGameWinningCondition(state.turnOwner);
    });
  }

  if (btnNextHint) {
    btnNextHint.addEventListener("click", () => {
      state.performNextHint(state.turnOwner);
    });
  }

  if (btnSkipHintPlay) {
    btnSkipHintPlay.addEventListener("click", () => {
      state.skipNextHintPlay();
    });
  }

  if (btnSkipGuard) {
    btnSkipGuard.addEventListener("click", () => {
      state.skipGuard();
    });
  }

  if (btnSwapPlayer) {
    btnSwapPlayer.addEventListener("click", () => {
      // Manually swap active player index to simulate hot-seat
      const currentOwner = state.turnOwner;
      state.turnOwner = 1 - currentOwner;
      state.addLog(`プレイヤー表示を ${state.players[state.turnOwner].name} に切り替えました。`);
      state.triggerStateChange();
    });
  }

  // Sandbox actions
  if (sbBtnDraw) {
    sbBtnDraw.addEventListener("click", () => {
      state.drawCard(state.turnOwner);
    });
  }

  if (sbBtnAddFile) {
    sbBtnAddFile.addEventListener("click", () => {
      const activePlayer = state.players[state.turnOwner];
      if (activePlayer.deck.length > 0) {
        const card = activePlayer.deck.pop();
        card.state = "facedown";
        activePlayer.fileArea.push(card);
        state.addLog(`[サンドボックス] ${activePlayer.name} のFILEにカードを1枚追加しました。(FILE: ${activePlayer.fileArea.length}枚)`);
        state.triggerStateChange();
      }
    });
  }

  if (sbBtnAddEvidence) {
    sbBtnAddEvidence.addEventListener("click", () => {
      const activePlayer = state.players[state.turnOwner];
      if (activePlayer.deck.length > 0) {
        const card = activePlayer.deck.pop();
        card.state = "facedown";
        activePlayer.evidenceArea.push(card);
        state.addLog(`[サンドボックス] ${activePlayer.name} の証拠にカードを1枚追加しました。(証拠: ${activePlayer.evidenceArea.length}枚)`);
        state.triggerStateChange();
      }
    });
  }

  if (sbBtnReset) {
    sbBtnReset.addEventListener("click", () => {
      if (confirm("ゲームを最初からリセットしますか？")) {
        // Init Conan/Blue vs Heiji/Green
        state.initGame("CT-P-001", "CT-C-001", "CT-P-002", "CT-C-002");
      }
    });
  }

  if (btnClearLogs) {
    btnClearLogs.addEventListener("click", () => {
      state.logs = [];
      renderLogs();
    });
  }

  // --- INITIALIZATION ---

  // Connect rendering callback
  state.subscribe(renderBoard);

  // Load starter game: P1 Conan TCG starter vs P2 Heiji TCG starter
  state.initGame("CT-P-001", "CT-C-001", "CT-P-002", "CT-C-002");

  // Force initial render
  renderBoard();
});
