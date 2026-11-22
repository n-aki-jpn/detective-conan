use super::*;

#[test]
fn test_index_html_no_undefined_is_secret_in_card_slot() {
    let html = std::fs::read_to_string("../src/conan_engine/web/static/index.html")
        .expect("index.html must be readable");
    let start = html.find("function createCardSlot(").expect("createCardSlot must exist");
    let end = html[start..].find("\n    function ").map(|i| start + i).unwrap_or(html.len());
    let fn_body = &html[start..end];
    assert!(
        !fn_body.contains("isSecret"),
        "createCardSlot must not reference undefined variable isSecret"
    );
}

#[test]
fn test_index_html_js_syntax_and_smoke() {
    let html = std::fs::read_to_string("../src/conan_engine/web/static/index.html")
        .expect("index.html must be readable");
    let script_start = html.rfind("<script>").expect("<script> tag must exist") + "<script>".len();
    let script_end = html[script_start..].find("</script>").expect("</script> tag must exist") + script_start;
    let js_code = &html[script_start..script_end];

    let tmp_dir = std::env::temp_dir();
    let js_path = tmp_dir.join("conan_index_check.js");
    std::fs::write(&js_path, js_code).expect("failed to write temp js");

    // 1. 構文チェック (node --check)
    let check_out = std::process::Command::new("node")
        .arg("--check")
        .arg(&js_path)
        .output()
        .expect("node must be available");
    assert!(
        check_out.status.success(),
        "index.html JS syntax error:\n{}",
        String::from_utf8_lossy(&check_out.stderr)
    );

    // 2. モックDOM上でスクリプトを読み込み、createCardSlot / createCardEl / render のUI更新をスモークテスト
    let smoke_js = format!(
        r#"
const assert = require('node:assert/strict');
const elsById = {{}};
const makeEl = () => {{
  const classes = new Set();
  const styleProps = {{}};
  const obj = {{
style: {{
  setProperty(k, v) {{ styleProps[k] = v; }},
  removeProperty(k) {{ delete styleProps[k]; }},
  getPropertyValue(k) {{ return styleProps[k] || ''; }}
}},
classList: {{
  add(...xs) {{ xs.forEach(x => classes.add(x)); }},
  remove(...xs) {{ xs.forEach(x => classes.delete(x)); }},
  contains(x) {{ return classes.has(x); }},
  toggle(x, force) {{
    if (force === true) classes.add(x);
    else if (force === false) classes.delete(x);
    else if (classes.has(x)) classes.delete(x);
    else classes.add(x);
  }}
}},
dataset: {{}},
children: [],
appendChild(c) {{ this.children.push(c); return c; }},
replaceWith() {{}},
addEventListener() {{}},
querySelector() {{ return makeEl(); }},
querySelectorAll() {{ return []; }},
setAttribute(k, v) {{ this['attr_' + k] = v; }},
getAttribute(k) {{ return this['attr_' + k] || ''; }},
getBoundingClientRect() {{ return {{ left: 0, top: 0, width: 100, height: 100 }}; }},
paused: true,
pause() {{ this.paused = true; }},
play() {{ this.paused = false; return Promise.resolve(); }}
  }};
  return new Proxy(obj, {{ get(t, p) {{ return p in t ? t[p] : ''; }} }});
}};
const bottomDockEl = makeEl();
const getEl = (id) => {{
  if (!elsById[id]) elsById[id] = makeEl();
  return elsById[id];
}};
global.window = global;
global.innerWidth = 390;
global.addEventListener = () => {{}};
global.setInterval = () => 0;
global.setTimeout = () => 0;
global.location = {{ search: '', href: 'http://localhost:8000/' }};
global.localStorage = {{ getItem(){{ return null; }}, setItem(){{}}, removeItem(){{}} }};
global.document = {{
  getElementById: (id) => getEl(id),
  createElement: () => makeEl(),
  querySelector: (sel) => sel === '.bottom-dock' ? bottomDockEl : makeEl(),
  querySelectorAll: () => [],
  addEventListener: () => {{}}
}};
global.fetch = async () => ({{ ok: true, json: async () => ({{}}) }});
global.Audio = function() {{ return {{ play: async () => {{}}, pause: () => {{}} }}; }};
{js}
createCardSlot('slot_1', {{ instanceId: 'c1', definitionId: 'B01001', name: '江戸川コナン', cardType: 'CHARACTER', level: 1, ap: 1000, lp: 1, orientation: 'ACTIVE', zone: 'FIELD' }}, {{ isMine: true, zone: 'FIELD' }});
createCardEl(null, true, false, null);

// targetSelect UI スモークテスト（全文表示・強調バー・効果文トグル）
catalogData.push({{
  definitionId: 'B11007P',
  title: '吉田歩美',
  cardType: 'CHARACTER',
  level: 4,
  ap: 3000,
  lp: 1,
  colors: ['青'],
  description: '【登場時】リムーブする相手のキャラを1枚まで選ぶ。',
  mainPath: 'sample_b11007p.jpg'
}});
const samplePlayers = () => ({{
  me: {{
playerId: 'player1',
isFirstPlayer: false,
casePhase: 'RESOLUTION',
fileCount: 8,
rawFileCount: 7,
partnerInFile: true,
deckCount: 17,
evidenceCount: 0,
removeCount: 8,
handCount: 1,
hand: [{{ instanceId: 'h1', definitionId: 'B01001', name: '江戸川コナン', cardType: 'CHARACTER', level: 4, ap: 4000, lp: 1, orientation: 'ACTIVE', zone: 'HAND', faceUp: true }}],
field: [{{ instanceId: 'inst_ayumi', definitionId: 'B11007P', name: '吉田歩美', cardType: 'CHARACTER', level: 4, ap: 3000, lp: 1, orientation: 'ACTIVE', zone: 'FIELD', faceUp: true }}],
partner: {{ instanceId: 'p1_p', definitionId: 'B01001', name: '江戸川コナン', cardType: 'PARTNER', level: 0, ap: 0, lp: 1, orientation: 'ACTIVE', zone: 'PARTNER', faceUp: true }},
extraPartners: [],
case: {{ instanceId: 'p1_c', definitionId: 'B01024', name: '事件', cardType: 'CASE', level: 0, ap: 0, lp: 0, orientation: 'ACTIVE', zone: 'CASE', faceUp: true }},
evidence: [],
remove: []
  }},
  opponent: {{
playerId: 'player2',
isFirstPlayer: true,
casePhase: 'RESOLUTION',
fileCount: 6,
rawFileCount: 5,
partnerInFile: true,
deckCount: 19,
evidenceCount: 3,
removeCount: 6,
handCount: 4,
hand: [],
field: [{{ instanceId: 'opp_1', definitionId: 'B05055', name: '相手キャラ', cardType: 'CHARACTER', level: 4, ap: 4000, lp: 1, orientation: 'SLEEP', zone: 'FIELD', faceUp: true }}],
partner: {{ instanceId: 'p2_p', definitionId: 'B01001', name: '江戸川コナン', cardType: 'PARTNER', level: 0, ap: 0, lp: 1, orientation: 'ACTIVE', zone: 'PARTNER', faceUp: true }},
extraPartners: [],
case: {{ instanceId: 'p2_c', definitionId: 'B01024', name: '事件', cardType: 'CASE', level: 0, ap: 0, lp: 0, orientation: 'ACTIVE', zone: 'CASE', faceUp: true }},
evidence: [],
remove: []
  }}
}});

render({{
  revision: 12,
  turn: 8,
  turnPlayerId: 'player1',
  decisionPlayerId: 'player1',
  viewingPlayerId: 'player1',
  logs: ['テスト開始'],
  ...samplePlayers(),
  pendingDecision: {{
decisionId: 'dec_target_1',
revision: 12,
playerId: 'player1',
kind: 'targetSelect',
options: {{
  actorId: 'inst_ayumi',
  actorName: '吉田歩美(B11007P)',
  effectType: 'REMOVE',
  targetZone: 'FIELD',
  maxSelect: 1,
  optional: true,
  title: 'リムーブする相手のカードを1枚選択してください',
  candidates: [
    {{ instanceId: 'opp_1', definitionId: 'B05055', name: '相手キャラ', level: 4, ap: 4000, zoneLabel: '現場', zone: 'FIELD', isOpponent: true }}
  ]
}}
  }}
}});

assert.equal(getEl('dock-command-bar').classList.contains('is-prompt-active'), true);
assert.equal(bottomDockEl.classList.contains('is-prompt-active'), true);
assert.equal(getEl('dock-status-desc').innerText, 'リムーブする相手のカードを1枚選択してください');
assert.ok(getEl('dock-status-badge').innerText.includes('吉田歩美(B11007P)'));
assert.ok(getEl('dock-status-badge').innerText.includes('🔍'));
assert.ok(getEl('dock-status-sub').innerText.includes('緑枠'));
assert.equal(getEl('dock-effect-toggle').style.display, 'inline-flex');
assert.equal(getEl('dock-effect-text').style.display, 'none');
getEl('dock-effect-toggle').onclick();
assert.equal(getEl('dock-effect-text').style.display, 'block');
assert.ok(getEl('dock-effect-text').innerText.includes('リムーブする相手のキャラ'));

// 通常手番に戻ったら強調状態と効果文トグルが解除されること
render({{
  revision: 13,
  turn: 8,
  turnPlayerId: 'player1',
  decisionPlayerId: 'player1',
  viewingPlayerId: 'player1',
  logs: ['テスト開始'],
  ...samplePlayers(),
  pendingDecision: {{
decisionId: 'dec_main_1',
revision: 13,
playerId: 'player1',
kind: 'mainAction',
options: {{
  actions: [{{ type: 'END_TURN' }}]
}}
  }}
}});
assert.equal(getEl('dock-command-bar').classList.contains('is-prompt-active'), false);
assert.equal(bottomDockEl.classList.contains('is-prompt-active'), false);
assert.equal(getEl('dock-effect-toggle').style.display, 'none');

// distinctNames: true 時に「江戸川コナン(B04005)」選択後は「江戸川コナン(B05010)」が重複選択不可かつ半透明になること
getEl('decision-actions').children = [];
render({{
  revision: 14,
  turn: 8,
  turnPlayerId: 'player1',
  decisionPlayerId: 'player1',
  viewingPlayerId: 'player1',
  logs: ['テスト開始'],
  ...samplePlayers(),
  pendingDecision: {{
decisionId: 'dec_b03006_distinct',
revision: 14,
playerId: 'player1',
kind: 'targetSelect',
options: {{
  actorId: 'inst_ayumi',
  actorName: '結成 少年探偵団(B03006)',
  effectType: 'STACK_UNDER_SELF',
  targetZone: 'REMOVE',
  maxSelect: 5,
  distinctNames: true,
  optional: true,
  title: '下に重ねるカードをリムーブエリアから選択してください（同名不可）',
  candidates: [
    {{ instanceId: 'c_conan_1', definitionId: 'B04005', name: '江戸川コナン(B04005)', baseName: '江戸川コナン', cardNames: ['江戸川コナン'], level: 4, ap: 3000, zoneLabel: 'リムーブ', zone: 'REMOVE', isOpponent: false }},
    {{ instanceId: 'c_conan_2', definitionId: 'B05010', name: '江戸川コナン(B05010)', level: 2, ap: 2000, zoneLabel: 'リムーブ', zone: 'REMOVE', isOpponent: false }},
    {{ instanceId: 'c_haibara', definitionId: 'B09011', name: '灰原哀(B09011)', level: 4, ap: 4000, zoneLabel: 'リムーブ', zone: 'REMOVE', isOpponent: false }}
  ]
}}
  }}
}});
const decChildren = getEl('decision-actions').children;
const btnConfirmB3006 = decChildren[1];
const candWrapB3006 = decChildren[2];
assert.equal(candWrapB3006.children.length, 3);
candWrapB3006.children[0].onclick();
assert.equal(btnConfirmB3006.innerText, '決定（1/5枚）');
assert.ok(candWrapB3006.children[1].style.cssText.includes('opacity:0.42'));
assert.ok(!candWrapB3006.children[2].style.cssText.includes('opacity:0.42'));
// 2枚目の江戸川コナン(B05010)をクリックしても追加されないこと
candWrapB3006.children[1].onclick();
assert.equal(btnConfirmB3006.innerText, '決定（1/5枚）');
// 灰原哀は追加できること
candWrapB3006.children[2].onclick();
assert.equal(btnConfirmB3006.innerText, '決定（2/5枚）');

// カード行動メニュー：誤タップ防止（推理先頭・アシスト末尾+区切り線+2段階確認・アンカー開閉）
const actionFetchCalls = [];
global.fetch = async (url, opts) => {{
  if (String(url).includes('/api/decision')) {{
actionFetchCalls.push({{ url, opts }});
return {{ ok: true, json: async () => ({{}}) }};
  }}
  return {{ ok: true, json: async () => window.latestObs || ({{}}) }};
}};
render((lastObs = {{
  revision: 15,
  turn: 4,
  turnPlayerId: 'player1',
  decisionPlayerId: 'player1',
  viewingPlayerId: 'player1',
  logs: ['テスト開始'],
  ...samplePlayers(),
  pendingDecision: {{
decisionId: 'dec_partner_menu',
revision: 15,
playerId: 'player1',
kind: 'mainAction',
options: {{
  actions: [
    {{ type: 'ASSIST' }},
    {{ type: 'DEDUCE', actorIds: ['p1_partner', 'p1_field1'] }},
    {{ type: 'ACTION', actorIds: ['p1_field1'], targetIds: ['p2_case'] }},
    {{ type: 'END_TURN' }}
  ]
}}
  }}
}}));

// PARTNERタップ時: DEDUCE -> 区切り線 -> ASSIST(注意枠) の順になること
getEl('action-menu-buttons').children = [];
onBoardCardClick(
  {{ instanceId: 'p1_partner', definitionId: 'B01002P', name: '灰原哀', cardType: 'PARTNER', lp: 1, orientation: 'ACTIVE' }},
  {{ isMine: true, zone: 'PARTNER' }},
  {{ left: 20, top: 200, bottom: 290, width: 70, height: 90 }}
);
assert.equal(getEl('card-action-menu').classList.contains('anchored'), true);
assert.equal(getEl('card-action-menu').style.display, 'block');
const menuChildren = getEl('action-menu-buttons').children;
assert.equal(menuChildren.length, 3);
assert.equal(menuChildren[0].dataset.actionLabel, '🔎 パートナーで推理する');
assert.ok(menuChildren[0].style.cssText.includes('#059669'));
assert.equal(menuChildren[0].children.length, 2);
assert.equal(menuChildren[1].className, 'action-menu-divider');
assert.equal(menuChildren[2].dataset.actionLabel, '🤝 アシスト（FILEへ移動）');
assert.ok(menuChildren[2].className.includes('action-menu-caution'));

// 1回目タップでASSISTが即送信されメニューとanchoredクラスが閉じること
menuChildren[2].onclick();
assert.equal(actionFetchCalls.length, 1);
assert.ok(String(actionFetchCalls[0].url).includes('/api/decision'));
assert.ok(String((actionFetchCalls[0].opts && actionFetchCalls[0].opts.body) || '').includes('"ASSIST"'));
assert.equal(getEl('card-action-menu').style.display, 'none');
assert.equal(getEl('card-action-menu').classList.contains('anchored'), false);

// 証拠からリムーブされた旨の新着ログでトースト通知が表示されること
render({{
  ...window.latestObs,
  revision: 16,
  logs: [...(window.latestObs.logs || []), '💥 事件への攻撃ヒット！ player2 の証拠から [江戸川コナン] をリムーブし、player1 が証拠を1つ獲得！']
}});
assert.equal(getEl('evidence-remove-toast-name').innerText, '江戸川コナン');
assert.ok(getEl('evidence-remove-toast').classList.contains('active'));

// DEDUCE（推理）は1回目タップで即実行されること
getEl('action-menu-buttons').children = [];
onBoardCardClick(
  {{ instanceId: 'p1_partner', definitionId: 'B01002P', name: '灰原哀', cardType: 'PARTNER', lp: 1, orientation: 'ACTIVE' }},
  {{ isMine: true, zone: 'PARTNER' }}
);
const menuChildrenDeduce = getEl('action-menu-buttons').children;
assert.equal(menuChildrenDeduce[0].dataset.actionLabel, '🔎 パートナーで推理する');
menuChildrenDeduce[0].onclick();
assert.equal(actionFetchCalls.length, 2);
assert.ok(String((actionFetchCalls[1].opts && actionFetchCalls[1].opts.body) || '').includes('"DEDUCE"'));
assert.equal(getEl('card-action-menu').style.display, 'none');

// FIELDキャラではアタック(danger)と推理(エメラルド)のサブ説明付きコマンドが生成されること
const fieldCmds = getBoardCardCommands(
  {{ instanceId: 'p1_field1', definitionId: 'B01001', name: '江戸川コナン', cardType: 'CHARACTER', lp: 1, orientation: 'ACTIVE' }},
  {{ isMine: true, zone: 'FIELD' }},
  window.latestObs
);
assert.equal(fieldCmds.length, 2);
assert.ok(fieldCmds[0].className.includes('btn-danger'));
assert.ok(fieldCmds[1].cssText.includes('#059669'));
assert.ok(Boolean(fieldCmds[0].subLabel) && Boolean(fieldCmds[1].subLabel));
"#,
        js = js_code
    );
    let smoke_path = tmp_dir.join("conan_index_smoke.js");
    std::fs::write(&smoke_path, smoke_js).expect("failed to write smoke js");
    let smoke_out = std::process::Command::new("node")
        .arg(&smoke_path)
        .output()
        .expect("node smoke run failed");
    assert!(
        smoke_out.status.success(),
        "index.html JS runtime smoke error:\n{}",
        String::from_utf8_lossy(&smoke_out.stderr)
    );
}

