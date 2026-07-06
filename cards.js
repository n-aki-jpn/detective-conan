// Card Database for Detective Conan TCG
const CARD_DATABASE = [
  // --- Partners ---
  {
    id: "CT-P-001",
    name: "江戸川コナン",
    type: "Partner",
    color: "Blue",
    level: 1,
    ap: 2000,
    lp: 1,
    traits: ["探偵", "小学生"],
    abilityText: "【事件解決】解決編状態で、自分の証拠が事件レベル以上あり、このカードをスリープすることで勝利する。\n【アシスト】このカードをスリープ：FILEエリアに移動する。自分のFILEエリアにカードが7枚以上ある場合、事件を解決編にする。\n【起動】このカードをスリープ：このターン、自分が次に手札から使用するキャラのレベル上限を+1する。",
    victoryCondition: {
      requiresSolutionPhase: true, // 解決編状態が必要
      evidenceCount: null // 事件レベルは事件カードによる
    },
    solutionCondition: {
      fileCount: 7 // 解決編に移行するのに必要なFILE枚数（パートナー含む）
    }
  },
  {
    id: "CT-P-002",
    name: "服部平次",
    type: "Partner",
    color: "Green",
    level: 1,
    ap: 2000,
    lp: 1,
    traits: ["探偵", "高校生"],
    abilityText: "【事件解決】解決編状態で、自分の証拠が事件レベル以上あり、このカードをスリープすることで勝利する。\n【アシスト】このカードをスリープ：FILEエリアに移動する。自分のFILEエリアにカードが7枚以上ある場合、事件を解決編にする。\n【起動】このカードをスリープ：このターン、次に使用する「特徴：高校生」のキャラのAPを+1000する。",
    victoryCondition: {
      requiresSolutionPhase: true, // 解決編状態が必要
      evidenceCount: null // 事件レベルは事件カードによる
    },
    solutionCondition: {
      fileCount: 7 // 解決編に移行するのに必要なFILE枚数（パートナー含む）
    }
  },

  // --- Cases ---
  {
    id: "CT-C-001",
    name: "消えた看板猫の謎",
    type: "Case",
    color: "Blue",
    level: 3,
    firstPlayerLevel: 7, // 先攻の事件レベル
    secondPlayerLevel: 6, // 後攻の事件レベル
    ap: 0,
    lp: 0,
    traits: ["日常"],
    abilityText: "【解決編条件】自分のFILEエリアが7枚以上。\n【事件レベル】先攻/後攻で異なる（証拠が事件レベル以上で解決可能）"
  },
  {
    id: "CT-C-002",
    name: "外交官殺人事件",
    type: "Case",
    color: "Green",
    level: 4,
    firstPlayerLevel: 7, // 先攻の事件レベル
    secondPlayerLevel: 6, // 後攻の事件レベル
    ap: 0,
    lp: 0,
    traits: ["殺人事件"],
    abilityText: "【解決編条件】自分のFILEエリアが7枚以上。\n【事件レベル】先攻/後攻で異なる（証拠が事件レベル以上で解決可能）"
  },

  // --- Characters (Blue) ---
  {
    id: "CT-CH-B01",
    name: "毛利蘭",
    type: "Character",
    color: "Blue",
    level: 2,
    ap: 3000,
    lp: 1,
    traits: ["女子高生", "空手部"],
    abilityText: "【登場時】相手のスリープ状態のレベル2以下のキャラ1枚をリムーブする。",
    cutIn: "AP＋2000",
    disguise: true
  },
  {
    id: "CT-CH-B02",
    name: "毛利小五郎",
    type: "Character",
    color: "Blue",
    level: 3,
    ap: 4000,
    lp: 1,
    traits: ["名探偵", "居眠り"],
    abilityText: "【常在】このカードがスリープ状態の間、自分の他の青のキャラすべてのAPを+1000する。"
  },
  {
    id: "CT-CH-B03",
    name: "灰原哀",
    type: "Character",
    color: "Blue",
    level: 1,
    ap: 1000,
    lp: 1,
    traits: ["少年探偵団", "元組織"],
    abilityText: "【起動】このカードをスリープ：自分の山札の上から1枚を見て、山札の上か下に戻す。",
    cutIn: "AP＋2000"
  },
  {
    id: "CT-CH-B04",
    name: "阿笠博士",
    type: "Character",
    color: "Blue",
    level: 2,
    ap: 2000,
    lp: 1,
    traits: ["発明家"],
    abilityText: "【登場時】自分の手札の「特徴：少年探偵団」のキャラ1枚を公開してもよい。そうした場合、1枚引く。"
  },
  {
    id: "CT-CH-B05",
    name: "工藤新一",
    type: "Character",
    color: "Blue",
    level: 5,
    ap: 6000,
    lp: 2,
    traits: ["探偵", "高校生"],
    abilityText: "【ダブル推理】このキャラが推理に成功した時、獲得する証拠を2枚にする。"
  },

  // --- Characters (Green) ---
  {
    id: "CT-CH-G01",
    name: "遠山和葉",
    type: "Character",
    color: "Green",
    level: 2,
    ap: 2000,
    lp: 1,
    traits: ["女子高生", "合気道"],
    abilityText: "【ガード】このキャラのガード時、コンタクト相手のキャラのAPを-1000する。",
    cutIn: "AP＋2000"
  },
  {
    id: "CT-CH-G02",
    name: "大滝警部",
    type: "Character",
    color: "Green",
    level: 3,
    ap: 4000,
    lp: 1,
    traits: ["警察", "大阪府警"],
    abilityText: "【登場時】自分の山札の上から2枚をFILEエリアに裏向きで置く。"
  },
  {
    id: "CT-CH-G03",
    name: "服部平蔵",
    type: "Character",
    color: "Green",
    level: 5,
    ap: 6000,
    lp: 2,
    traits: ["警察", "大阪府警本部長"],
    abilityText: "【常在】相手のキャラの【登場時】能力は発動しない。"
  },
  {
    id: "CT-CH-G04",
    name: "遠山銀司郎",
    type: "Character",
    color: "Green",
    level: 4,
    ap: 5000,
    lp: 1,
    traits: ["警察", "大阪府警刑事部長"],
    abilityText: "【起動】このキャラをスリープ：相手のレベル3以下のキャラ1枚をスリープ状態にする。"
  },

  // --- Characters (Neutral/Multi/Other) ---
  {
    id: "CT-CH-W01",
    name: "怪盗キッド",
    type: "Character",
    color: "White",
    level: 6,
    ap: 6000,
    lp: 2,
    traits: ["怪盗", "変装"],
    abilityText: "【神出鬼没】このカードは色に関係なく手札から使用できる。\n【起動】自分の手札を1枚捨てる：このターン、このキャラは相手からアクションの対象に選ばれない。"
  },
  {
    id: "CT-CH-R01",
    name: "赤井秀一",
    type: "Character",
    color: "Red",
    level: 6,
    ap: 7000,
    lp: 2,
    traits: ["FBI", "スナイパー"],
    abilityText: "【突撃】このキャラは場に出たターンでも推理やアクションを行うことができる。"
  },
  {
    id: "CT-CH-Y01",
    name: "安室透",
    type: "Character",
    color: "Yellow",
    level: 4,
    ap: 5000,
    lp: 1,
    traits: ["探偵", "トリプルフェイス"],
    abilityText: "【登場時】自分のFILEエリアのカード1枚を手札に加えてよい。そうした場合、自分の手札を1枚FILEエリアに裏向きで置く。"
  },

  // --- Events ---
  {
    id: "CT-EV-001",
    name: "ターボエンジン付きスケボー",
    type: "Event",
    color: "Blue",
    level: 1,
    ap: 0,
    lp: 0,
    traits: ["阿笠のメカ"],
    abilityText: "【使用時】自分の青のキャラ1枚を選び、このターン中、APを+2000する。"
  },
  {
    id: "CT-EV-002",
    name: "真実への閃き",
    type: "Event",
    color: "Blue",
    level: 2,
    ap: 0,
    lp: 0,
    traits: ["推理サポート"],
    abilityText: "【使用時】カードを2枚引く。"
  },
  {
    id: "CT-EV-003",
    name: "犯人の罠",
    type: "Event",
    color: "Green",
    level: 2,
    ap: 0,
    lp: 0,
    traits: ["トラップ"],
    abilityText: "【使用時】相手のアクティブ状態のキャラ1枚を選び、スリープ状態にする。"
  }
];

// Generates a deck of 40 cards based on preferred color
function generateStarterDeck(color = "Blue") {
  const deck = [];
  const validCards = CARD_DATABASE.filter(c => c.type !== "Partner" && c.type !== "Case");
  const colorFiltered = validCards.filter(c => c.color === color);

  if (colorFiltered.length === 0) {
    console.error("No valid cards found for color: " + color);
    return deck;
  }

  // Dynamically calculate max copies to prevent infinite loops if DB has few cards
  const maxCopies = Math.max(3, Math.ceil(40 / colorFiltered.length));
  
  let index = 0;
  const cardCounts = {};

  while (deck.length < 40) {
    const card = colorFiltered[index % colorFiltered.length];
    const currentCount = cardCounts[card.id] || 0;
    
    if (currentCount < maxCopies) {
      deck.push({ ...card, instanceId: `${card.id}_${deck.length + 1}` });
      cardCounts[card.id] = currentCount + 1;
    }
    
    index++;
  }

  // Shuffle deck helper
  for (let i = deck.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [deck[i], deck[j]] = [deck[j], deck[i]];
  }

  return deck;
}

if (typeof window !== "undefined") {
  window.CARD_DATABASE = CARD_DATABASE;
  window.generateStarterDeck = generateStarterDeck;
}
