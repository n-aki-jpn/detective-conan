use crate::catalog::CardCatalog;
use crate::engine::MatchEngine;
use crate::models::{CardDefinition, CardOrientation, CardType, CasePhase, DeckDefinition};
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};
use std::sync::Mutex;

/// 序盤(事件編)・終盤(解決編)の2フェーズ重みペア (Tapered Evaluation)
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TaperedWeight {
    pub early: f64,
    pub late: f64,
}

impl TaperedWeight {
    pub const fn new(early: f64, late: f64) -> Self {
        Self { early, late }
    }

    #[inline]
    pub fn eval(&self, phase: f64) -> f64 {
        let p = phase.clamp(0.0, 1.0);
        self.early * (1.0 - p) + self.late * p
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CpuWeights {
    // 既存重み（後方互換維持）
    pub level_weight: f64,
    pub ap_weight: f64,
    pub lp_weight: f64,
    pub rush_bonus: f64,
    pub on_play_bonus: f64,
    pub event_penalty: f64,
    pub removal_bonus: f64,
    pub assist_min_field: usize,
    pub deduce_lp_weight: f64,
    pub attack_remove_weight: f64,
    pub case_attack_bonus: f64,
    pub disguise_ap_threshold: i32,

    // Deep Research 第1部・第2部に基づく Tapered Evaluation 特徴量重み
    pub evidence_distance_weight: TaperedWeight,
    pub file_diff_weight: TaperedWeight,
    pub file_six_threshold_bonus: TaperedWeight,
    pub file_seven_threshold_bonus: TaperedWeight,
    pub file_drop_below_six_penalty: TaperedWeight,
    pub active_posture_mult: f64,
    pub sleep_posture_mult: f64,
    pub ap_lp_cross_weight: TaperedWeight,
    pub active_blocker_weight: TaperedWeight,
    pub hand_quality_weight: TaperedWeight,
    pub lethal_threat_penalty: f64,
    pub deck_refresh_danger_penalty: f64,
    pub zero_evidence_control_bonus: f64,
    pub opponent_response_discount: f64,
}

impl Default for CpuWeights {
    fn default() -> Self {
        Self {
            level_weight: 1050.0,
            ap_weight: 1.0,
            lp_weight: 650.0,
            rush_bonus: 1450.0,
            on_play_bonus: 950.0,
            event_penalty: -200.0,
            removal_bonus: 1200.0,
            assist_min_field: 2,
            deduce_lp_weight: 1250.0,
            attack_remove_weight: 1600.0,
            case_attack_bonus: 1150.0,
            disguise_ap_threshold: 1000,

            evidence_distance_weight: TaperedWeight::new(1800.0, 5200.0),
            file_diff_weight: TaperedWeight::new(900.0, 1400.0),
            file_six_threshold_bonus: TaperedWeight::new(400.0, 2600.0),
            file_seven_threshold_bonus: TaperedWeight::new(700.0, 1800.0),
            file_drop_below_six_penalty: TaperedWeight::new(-300.0, -3800.0),
            active_posture_mult: 1.0,
            sleep_posture_mult: 0.65,
            ap_lp_cross_weight: TaperedWeight::new(0.18, 0.30),
            active_blocker_weight: TaperedWeight::new(550.0, 1750.0),
            hand_quality_weight: TaperedWeight::new(420.0, 680.0),
            lethal_threat_penalty: -6500.0,
            deck_refresh_danger_penalty: -2400.0,
            zero_evidence_control_bonus: 750.0,
            opponent_response_discount: 0.85,
        }
    }
}

impl CpuWeights {
    pub fn load_or_default(path: &str) -> Self {
        if let Ok(content) = std::fs::read_to_string(path) {
            if let Ok(w) = serde_json::from_str::<Self>(&content) {
                return w;
            }
        }
        Self::default()
    }

    pub fn save(&self, path: &str) -> std::io::Result<()> {
        if let Some(parent) = std::path::Path::new(path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let s = serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string());
        std::fs::write(path, s)
    }

    /// SPSA 用に連続パラメータを固定長ベクトルへ変換
    fn to_vec(&self) -> Vec<f64> {
        vec![
            self.level_weight,
            self.ap_weight,
            self.lp_weight,
            self.rush_bonus,
            self.on_play_bonus,
            self.event_penalty,
            self.removal_bonus,
            self.deduce_lp_weight,
            self.attack_remove_weight,
            self.case_attack_bonus,
            self.evidence_distance_weight.early,
            self.evidence_distance_weight.late,
            self.file_six_threshold_bonus.late,
            self.file_drop_below_six_penalty.late,
            self.active_blocker_weight.late,
            self.lethal_threat_penalty,
            self.opponent_response_discount,
        ]
    }

    /// SPSA の各軸摂動スケール c_i
    fn spsa_scales() -> Vec<f64> {
        vec![
            140.0, // level_weight
            0.22,  // ap_weight
            120.0, // lp_weight
            200.0, // rush_bonus
            160.0, // on_play_bonus
            160.0, // event_penalty
            200.0, // removal_bonus
            180.0, // deduce_lp_weight
            220.0, // attack_remove_weight
            180.0, // case_attack_bonus
            300.0, // evidence_distance_weight.early
            600.0, // evidence_distance_weight.late
            350.0, // file_six_threshold_bonus.late
            450.0, // file_drop_below_six_penalty.late
            260.0, // active_blocker_weight.late
            700.0, // lethal_threat_penalty
            0.12,  // opponent_response_discount
        ]
    }

    /// 摂動ベクトルを適用した新しい重みを生成
    fn from_perturbed(&self, theta: &[f64]) -> Self {
        let mut w = self.clone();
        if theta.len() < 17 {
            return w;
        }
        w.level_weight = theta[0].max(150.0);
        w.ap_weight = theta[1].clamp(0.15, 3.0);
        w.lp_weight = theta[2].max(50.0);
        w.rush_bonus = theta[3].max(0.0);
        w.on_play_bonus = theta[4].max(0.0);
        w.event_penalty = theta[5].clamp(-2500.0, 1500.0);
        w.removal_bonus = theta[6].max(0.0);
        w.deduce_lp_weight = theta[7].max(150.0);
        w.attack_remove_weight = theta[8].max(250.0);
        w.case_attack_bonus = theta[9].max(0.0);
        w.evidence_distance_weight.early = theta[10].max(200.0);
        w.evidence_distance_weight.late = theta[11].max(1000.0);
        w.file_six_threshold_bonus.late = theta[12].max(200.0);
        w.file_drop_below_six_penalty.late = theta[13].min(-200.0);
        w.active_blocker_weight.late = theta[14].max(200.0);
        w.lethal_threat_penalty = theta[15].min(-1000.0);
        w.opponent_response_discount = theta[16].clamp(0.2, 1.5);
        w
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CpuDifficulty {
    Easy,
    Normal,
    Hard,
    Strongest,
}

impl Default for CpuDifficulty {
    fn default() -> Self {
        Self::Strongest
    }
}

/// 相手デッキの色・アーキタイプから競技定石上の警戒プロファイルを分類
#[derive(Debug, Clone, Copy)]
struct OpponentProfile {
    /// 赤・白・アグロなど早期ワンショット／事件アクション警戒対面か
    avoid_early_p_deduce: bool,
    /// FILE 8 基準での相手色別最大リーサル打点目安 (青5/赤5/黄4/緑4/白4)
    max_file8_burst: i32,
    /// 変装警戒が高い対面か（白・緑など）
    disguise_risk: bool,
    /// 縦置きブロッカーを貫通・無視しやすい対面か（赤・黄長野・青新蘭など）
    blocker_bypass_risk: bool,
}

#[derive(Debug)]
pub struct RandomAgent {
    pub player_id: String,
    pub weights: CpuWeights,
    pub difficulty: CpuDifficulty,
    last_turn: AtomicI32,
    hints_used_this_turn: AtomicUsize,
    last_hint_skipped: AtomicBool,
    /// 軽量ソフト信念: 相手がコンタクトでカットインを見送った観測回数
    opp_no_cutin_observations: AtomicUsize,
    /// 軽量ソフト信念: 相手がカットインを使用した観測回数
    opp_cutin_used_count: AtomicUsize,
    /// CPU判断用RNG（既定はエントロピー初期化。with_seedでシード固定し再現性を担保）
    /// ※ SharedState内で保持されるため Send が必要な Mutex を使用
    rng: Mutex<StdRng>,
}

impl Clone for RandomAgent {
    fn clone(&self) -> Self {
        Self {
            player_id: self.player_id.clone(),
            weights: self.weights.clone(),
            difficulty: self.difficulty,
            last_turn: AtomicI32::new(self.last_turn.load(Ordering::Relaxed)),
            hints_used_this_turn: AtomicUsize::new(self.hints_used_this_turn.load(Ordering::Relaxed)),
            last_hint_skipped: AtomicBool::new(self.last_hint_skipped.load(Ordering::Relaxed)),
            opp_no_cutin_observations: AtomicUsize::new(
                self.opp_no_cutin_observations.load(Ordering::Relaxed),
            ),
            opp_cutin_used_count: AtomicUsize::new(
                self.opp_cutin_used_count.load(Ordering::Relaxed),
            ),
            rng: Mutex::new(StdRng::from_entropy()),
        }
    }
}

impl RandomAgent {
    pub fn new(player_id: &str) -> Self {
        Self {
            player_id: player_id.to_string(),
            weights: CpuWeights::load_or_default("data/cpu_weights.json"),
            difficulty: CpuDifficulty::Strongest,
            last_turn: AtomicI32::new(-1),
            hints_used_this_turn: AtomicUsize::new(0),
            last_hint_skipped: AtomicBool::new(false),
            opp_no_cutin_observations: AtomicUsize::new(0),
            opp_cutin_used_count: AtomicUsize::new(0),
            rng: Mutex::new(StdRng::from_entropy()),
        }
    }

    /// シード指定でCPU判断RNGを初期化（固定シード・シミュレーション用）
    pub fn with_seed(player_id: &str, weights: CpuWeights, seed: u64) -> Self {
        Self {
            player_id: player_id.to_string(),
            weights,
            difficulty: CpuDifficulty::Strongest,
            last_turn: AtomicI32::new(-1),
            hints_used_this_turn: AtomicUsize::new(0),
            last_hint_skipped: AtomicBool::new(false),
            opp_no_cutin_observations: AtomicUsize::new(0),
            opp_cutin_used_count: AtomicUsize::new(0),
            rng: Mutex::new(StdRng::seed_from_u64(seed)),
        }
    }

    pub fn with_difficulty(player_id: &str, difficulty: CpuDifficulty) -> Self {
        Self {
            player_id: player_id.to_string(),
            weights: CpuWeights::load_or_default("data/cpu_weights.json"),
            difficulty,
            last_turn: AtomicI32::new(-1),
            hints_used_this_turn: AtomicUsize::new(0),
            last_hint_skipped: AtomicBool::new(false),
            opp_no_cutin_observations: AtomicUsize::new(0),
            opp_cutin_used_count: AtomicUsize::new(0),
            rng: Mutex::new(StdRng::from_entropy()),
        }
    }

    pub fn with_weights(player_id: &str, weights: CpuWeights) -> Self {
        Self {
            player_id: player_id.to_string(),
            weights,
            difficulty: CpuDifficulty::Strongest,
            last_turn: AtomicI32::new(-1),
            hints_used_this_turn: AtomicUsize::new(0),
            last_hint_skipped: AtomicBool::new(false),
            opp_no_cutin_observations: AtomicUsize::new(0),
            opp_cutin_used_count: AtomicUsize::new(0),
            rng: Mutex::new(StdRng::from_entropy()),
        }
    }

    fn candidate_distinct_names(cand: &Value) -> Vec<String> {
        if let Some(arr) = cand.get("cardNames").and_then(|v| v.as_array()) {
            return arr
                .iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect();
        }
        let cname = cand.get("name").and_then(|v| v.as_str()).unwrap_or("");
        crate::effects::extract_distinct_card_names(cname)
    }

    fn reset_turn_state_if_needed(&self, turn_number: i32) {
        let prev = self.last_turn.load(Ordering::Relaxed);
        if prev != turn_number {
            if turn_number <= 1 || turn_number < prev {
                self.opp_no_cutin_observations.store(0, Ordering::Relaxed);
                self.opp_cutin_used_count.store(0, Ordering::Relaxed);
            }
            self.last_turn.store(turn_number, Ordering::Relaxed);
            self.hints_used_this_turn.store(0, Ordering::Relaxed);
            self.last_hint_skipped.store(false, Ordering::Relaxed);
        }
    }

    /// 自分が先攻かどうか
    fn is_first_player(&self, engine: &MatchEngine) -> bool {
        engine
            .player_order
            .first()
            .map(|pid| pid == &self.player_id)
            .unwrap_or(true)
    }

    /// 自分視点のターン数 (1T, 2T, 3T, 4T...) を推定
    fn my_turn_index(&self, engine: &MatchEngine) -> i32 {
        ((engine.turn - 1).max(0) / 2) + 1
    }

    /// 必要証拠数（標準的な事件カードでは先攻7・後攻6）
    fn required_evidence_for(&self, engine: &MatchEngine, player_id: &str) -> usize {
        let is_first = engine
            .player_order
            .first()
            .map(|pid| pid == player_id)
            .unwrap_or(true);
        if is_first {
            7
        } else {
            6
        }
    }

    /// 40枚デッキから公開領域を差し引いた残存山札枚数の推定（リフレッシュ危険度算出用）
    fn estimate_remaining_deck(&self, engine: &MatchEngine, player_id: &str) -> usize {
        let Some(ps) = engine.players.get(player_id) else {
            return 20;
        };
        let known = ps.hand.len()
            + ps.field.len()
            + ps.remove.len()
            + ps.file.len()
            + ps.evidence.len();
        40usize.saturating_sub(known)
    }

    /// 連続ゲームフェーズ p ∈ [0.0, 1.0] (Tapered Evaluation用)
    fn compute_game_phase(&self, engine: &MatchEngine, opp_id: &str) -> f64 {
        let Some(my_ps) = engine.players.get(&self.player_id) else {
            return 0.0;
        };
        let opp_ps = engine.players.get(opp_id);

        let my_solved = if my_ps.case_phase == CasePhase::RESOLUTION {
            1.0
        } else {
            0.0
        };
        let opp_solved = opp_ps
            .map(|o| {
                if o.case_phase == CasePhase::RESOLUTION {
                    1.0
                } else {
                    0.0
                }
            })
            .unwrap_or(0.0);

        let my_file_prog = (my_ps.effective_file_count() as f64 / 8.0).clamp(0.0, 1.0);
        let opp_file_prog = opp_ps
            .map(|o| (o.effective_file_count() as f64 / 8.0).clamp(0.0, 1.0))
            .unwrap_or(0.0);

        let my_req = self.required_evidence_for(engine, &self.player_id) as f64;
        let opp_req = self.required_evidence_for(engine, opp_id) as f64;
        let my_ev_prog = (my_ps.evidence.len() as f64 / my_req).clamp(0.0, 1.0);
        let opp_ev_prog = opp_ps
            .map(|o| (o.evidence.len() as f64 / opp_req).clamp(0.0, 1.0))
            .unwrap_or(0.0);

        let solved_term = 0.5 * (my_solved + opp_solved);
        let file_term = my_file_prog.max(opp_file_prog);
        let ev_term = my_ev_prog.max(opp_ev_prog);

        (0.45 * solved_term + 0.30 * file_term + 0.25 * ev_term).clamp(0.0, 1.0)
    }

    /// 相手のパートナー・事件カードの色・名称から競技定石上の対面プロファイルを推定
    fn analyze_opponent_profile(&self, engine: &MatchEngine, opp_id: &str) -> OpponentProfile {
        let Some(opp_ps) = engine.players.get(opp_id) else {
            return OpponentProfile {
                avoid_early_p_deduce: false,
                max_file8_burst: 4,
                disguise_risk: false,
                blocker_bypass_risk: false,
            };
        };

        let mut colors: Vec<String> = Vec::new();
        let mut case_name = String::new();

        if let Some(p) = &opp_ps.partner {
            if let Some(def) = engine.catalog.get(&p.definition_id) {
                colors.extend(def.colors.iter().cloned());
            }
        }
        if let Some(c) = &opp_ps.r#case {
            if let Some(def) = engine.catalog.get(&c.definition_id) {
                colors.extend(def.colors.iter().cloned());
                if let Some(n) = def.names.first() {
                    case_name = n.clone();
                }
            }
        }

        let has_color = |kw: &str| colors.iter().any(|c| c.contains(kw));
        let is_red = has_color("赤") || has_color("RED") || has_color("Red");
        let is_white = has_color("白") || has_color("WHITE") || has_color("White");
        let is_blue = has_color("青") || has_color("BLUE") || has_color("Blue");
        let is_green = has_color("緑") || has_color("GREEN") || has_color("Green");
        let is_yellow = has_color("黄") || has_color("YELLOW") || has_color("Yellow");
        let is_sos_or_taiko = case_name.contains("SOS") || case_name.contains("太閤名人");

        // 第1部 1.2節: 赤(FBI/太閤名人)・白・赤黒・アグロ(SOS)は序盤のP推理を控えて事件アクション権を与えない
        let avoid_early_p_deduce = is_red || is_white || is_sos_or_taiko;

        // 第1部 1.6節: FILE 8 からの色別最大打点目安 (青5・赤5・黄4・緑4・白4)
        let max_file8_burst = if is_red || is_blue { 5 } else { 4 };

        // 白(キッド変装)・緑(和葉変装)は変装警戒
        let disguise_risk = is_white || is_green;

        // 赤(ブレット/直接生成)・黄(長野迅速)・青(8蘭/結成少年探偵団)はブロッカー貫通警戒
        let blocker_bypass_risk = is_red || is_yellow || is_blue;

        OpponentProfile {
            avoid_early_p_deduce,
            max_file8_burst,
            disguise_risk,
            blocker_bypass_risk,
        }
    }

    /// カードのテキストからカットインAP補正値をパース (MR=Lv9は+2000)
    fn parse_cutin_bonus(def: &CardDefinition) -> i32 {
        let desc = &def.description;
        if desc.contains("+4000") || desc.contains("＋4000") {
            return 4000;
        }
        if desc.contains("+3000") || desc.contains("＋3000") {
            return 3000;
        }
        if desc.contains("+2000") || desc.contains("＋2000") {
            return 2000;
        }
        if desc.contains("+1000") || desc.contains("＋1000") {
            return 1000;
        }
        if def.level >= 9 {
            return 2000;
        }
        if desc.contains("カットイン") {
            return 2000;
        }
        0
    }

    /// 情報獲得行動（ドロー・デッキ確認・サーチ）かどうか
    fn is_info_gain_card(def: &CardDefinition) -> bool {
        let d = &def.description;
        d.contains("カードを引く")
            || d.contains("1枚引く")
            || d.contains("2枚引く")
            || d.contains("デッキの上から")
            || d.contains("手札に加える")
    }

    /// 自分の証拠を直接増やす効果を持つかどうか
    fn gains_evidence_directly(def: &CardDefinition) -> i32 {
        let d = &def.description;
        if d.contains("証拠を2つ得") {
            2
        } else if d.contains("証拠を1つ得")
            || d.contains("証拠を1枚得")
            || d.contains("証拠を得")
        {
            1
        } else {
            0
        }
    }

    /// ミスリード持ちかどうか
    fn has_mislead(def: &CardDefinition) -> bool {
        def.description.contains("ミスリード")
    }

    /// 迅速持ちかどうか
    fn has_swift(def: &CardDefinition) -> bool {
        def.description.contains("迅速")
    }

    /// 突撃持ちかどうか
    fn has_rush(def: &CardDefinition) -> bool {
        def.description.contains("突撃")
    }

    /// ブレット（ガード不可）持ちかどうか
    fn has_bullet(def: &CardDefinition) -> bool {
        def.description.contains("ブレット")
    }

    /// 専用リーサル検出器 (第1部 1.6節・第2部 2.2節):
    /// パートナーを事件解決用に残したまま（自然FILE基準）、今ターン中に必要証拠へ到達できる見込みを判定する
    fn can_reach_lethal_this_turn(&self, engine: &MatchEngine, opp_id: &str) -> bool {
        let Some(my_ps) = engine.players.get(&self.player_id) else {
            return false;
        };
        if my_ps.case_phase != CasePhase::RESOLUTION {
            return false;
        }
        let req = self.required_evidence_for(engine, &self.player_id) as i32;
        let cur = my_ps.evidence.len() as i32;
        let needed = req - cur;
        if needed <= 0 {
            return true;
        }

        let opp_ps = engine.players.get(opp_id);
        let opp_active_count = opp_ps
            .map(|o| {
                o.field
                    .iter()
                    .filter(|c| c.orientation == CardOrientation::ACTIVE)
                    .count() as i32
            })
            .unwrap_or(0);
        let opp_has_ev = opp_ps.map(|o| !o.evidence.is_empty()).unwrap_or(false);

        // 1. 場のアクティブキャラから見込める証拠（ミスリード1体あたり-1を見込んで安全側計算）
        let opp_active_mislead = opp_ps
            .map(|o| {
                o.field
                    .iter()
                    .filter(|c| {
                        c.orientation == CardOrientation::ACTIVE
                            && engine
                                .catalog
                                .get(&c.definition_id)
                                .map(Self::has_mislead)
                                .unwrap_or(false)
                    })
                    .count() as i32
            })
            .unwrap_or(0);

        let mut projected_gain = 0i32;
        for fc in &my_ps.field {
            if fc.orientation == CardOrientation::ACTIVE && !engine.acted_this_turn_ids.contains(&fc.instance_id) {
                let lp = engine.get_effective_lp(&self.player_id, fc);
                let safe_lp = (lp - opp_active_mislead).max(0);
                projected_gain += safe_lp.max(if opp_has_ev && opp_active_count == 0 { 1 } else { 0 });
            }
        }

        // 2. 手札の迅速・突撃・直接証拠生成札（自然FILE範囲＋NH込みの最大見込み）
        let natural_file = my_ps.effective_file_count() as i32;
        let mut hand_burst = 0i32;
        let mut burst_cards_counted = 0i32;
        let max_plays = if !my_ps.normal_hand_played_this_turn { 2 } else { 1 };

        let mut hand_scores: Vec<i32> = my_ps
            .hand
            .iter()
            .filter_map(|c| engine.catalog.get(&c.definition_id))
            .filter(|d| d.level <= natural_file)
            .map(|d| {
                let direct_ev = Self::gains_evidence_directly(d);
                if Self::has_swift(d) {
                    (d.lp - opp_active_mislead).max(1) + direct_ev
                } else if Self::has_rush(d) && opp_has_ev && (opp_active_count == 0 || Self::has_bullet(d)) {
                    1 + direct_ev
                } else {
                    direct_ev
                }
            })
            .filter(|&v| v > 0)
            .collect();
        hand_scores.sort_by(|a, b| b.cmp(a));
        for v in hand_scores {
            if burst_cards_counted >= max_plays {
                break;
            }
            hand_burst += v;
            burst_cards_counted += 1;
        }

        // 相手山札リフレッシュ誘発 (+1)
        let opp_rem_deck = self.estimate_remaining_deck(engine, opp_id);
        let refresh_bonus = if opp_rem_deck <= 1 { 1 } else { 0 };

        projected_gain + hand_burst + refresh_bonus >= needed
    }

    /// 相手が次ターンにリーサルへ到達する危険度（超過打点見込み）を算出
    fn estimate_opponent_lethal_excess(&self, engine: &MatchEngine, opp_id: &str) -> i32 {
        let Some(my_ps) = engine.players.get(&self.player_id) else {
            return -5;
        };
        let Some(opp_ps) = engine.players.get(opp_id) else {
            return -5;
        };
        let opp_req = self.required_evidence_for(engine, opp_id) as i32;
        let opp_ev = opp_ps.evidence.len() as i32;
        let opp_file = opp_ps.effective_file_count() as i32;

        // 次ターンに相手が解決編へ入れないならリーサル圏外
        if opp_ps.case_phase != CasePhase::RESOLUTION && opp_file + 2 < 7 {
            return -5;
        }

        let profile = self.analyze_opponent_profile(engine, opp_id);
        let opp_board_lp: i32 = opp_ps
            .field
            .iter()
            .map(|c| engine.get_effective_lp(opp_id, c).max(1))
            .sum();

        let my_active_blockers = my_ps
            .field
            .iter()
            .filter(|c| c.orientation == CardOrientation::ACTIVE)
            .count() as i32;

        // 色別の手札・盤面合算見込み打点
        let hand_burst_est = if opp_file + 2 >= 8 {
            (profile.max_file8_burst - 1).min(opp_ps.hand.len() as i32)
        } else {
            2.min(opp_ps.hand.len() as i32)
        };

        let blocker_mitigation = if profile.blocker_bypass_risk {
            (my_active_blockers / 2).max(0)
        } else {
            my_active_blockers
        };

        let my_refresh_risk = if self.estimate_remaining_deck(engine, &self.player_id) <= 2 {
            1
        } else {
            0
        };

        let projected_opp_ev = opp_ev + opp_board_lp + hand_burst_est + my_refresh_risk - blocker_mitigation;
        projected_opp_ev - opp_req
    }

    /// 第2部 2.4節: Tapered Evaluation による現在盤面の静的評価スコア（自視点）
    pub fn evaluate_board_state(&self, engine: &MatchEngine, opp_id: &str) -> f64 {
        let Some(my_ps) = engine.players.get(&self.player_id) else {
            return 0.0;
        };
        let Some(opp_ps) = engine.players.get(opp_id) else {
            return 0.0;
        };

        let phase = self.compute_game_phase(engine, opp_id);
        let w = &self.weights;
        let mut score = 0.0;

        // 1. 勝利距離の非線形変換 (1 / (d + 1))
        let my_req = self.required_evidence_for(engine, &self.player_id) as i32;
        let opp_req = self.required_evidence_for(engine, opp_id) as i32;
        let my_rem = (my_req - my_ps.evidence.len() as i32).max(0) as f64;
        let opp_rem = (opp_req - opp_ps.evidence.len() as i32).max(0) as f64;
        let ev_dist_diff = (1.0 / (my_rem + 1.0)) - (1.0 / (opp_rem + 1.0));
        score += ev_dist_diff * w.evidence_distance_weight.eval(phase);

        // 証拠0枚コントロール（相手が赤・白等で自アクティブが少ない序盤）
        let profile = self.analyze_opponent_profile(engine, opp_id);
        if profile.avoid_early_p_deduce && my_ps.evidence.is_empty() && my_ps.case_phase != CasePhase::RESOLUTION {
            score += w.zero_evidence_control_bonus * (1.0 - phase);
        }

        // 2. FILE経済（閾値重視）
        let my_file = my_ps.effective_file_count() as i32;
        let opp_file = opp_ps.effective_file_count() as i32;
        score += (my_file - opp_file) as f64 * w.file_diff_weight.eval(phase);

        if my_file >= 6 {
            score += w.file_six_threshold_bonus.eval(phase);
        }
        if my_file >= 7 {
            score += w.file_seven_threshold_bonus.eval(phase);
        }
        if my_ps.case_phase == CasePhase::RESOLUTION && my_file <= 5 {
            score += w.file_drop_below_six_penalty.eval(phase);
        }
        if opp_ps.case_phase == CasePhase::RESOLUTION && opp_file <= 5 {
            score -= w.file_drop_below_six_penalty.eval(phase) * 0.7;
        }

        // 3. 盤面戦力・姿勢・AP×LP交差項・ブロッカー充足度
        let mut my_active_cnt = 0;
        for fc in &my_ps.field {
            let ap = engine.get_effective_ap(&self.player_id, fc) as f64;
            let lp = engine.get_effective_lp(&self.player_id, fc) as f64;
            let posture = if fc.orientation == CardOrientation::ACTIVE {
                my_active_cnt += 1;
                w.active_posture_mult
            } else {
                w.sleep_posture_mult
            };
            let lvl = engine.catalog.get(&fc.definition_id).map(|d| d.level).unwrap_or(1);
            let mr_bonus = if lvl >= 9 { 1800.0 } else if ap >= 7000.0 { 700.0 } else { 0.0 };
            score += posture * (ap * w.ap_weight + lp * w.lp_weight + ap * lp * w.ap_lp_cross_weight.eval(phase) + mr_bonus);
        }

        for oc in &opp_ps.field {
            let ap = engine.get_effective_ap(opp_id, oc) as f64;
            let lp = engine.get_effective_lp(opp_id, oc) as f64;
            let posture = if oc.orientation == CardOrientation::ACTIVE {
                w.active_posture_mult
            } else {
                w.sleep_posture_mult
            };
            let lvl = engine.catalog.get(&oc.definition_id).map(|d| d.level).unwrap_or(1);
            let mr_bonus = if lvl >= 9 { 1800.0 } else if ap >= 7000.0 { 700.0 } else { 0.0 };
            score -= posture * (ap * w.ap_weight + lp * w.lp_weight + ap * lp * w.ap_lp_cross_weight.eval(phase) + mr_bonus);
        }

        // アクティブ・ブロッカー充足度
        score += (my_active_cnt.min(3) as f64) * w.active_blocker_weight.eval(phase);

        // 4. 手札の質と次ターン接続性（上限飽和型 min(h, 8)）
        let hand_cap = my_ps.hand.len().min(8) as f64;
        let next_natural_file = (my_file + 2).min(10);
        let has_next_curve = my_ps.hand.iter().any(|c| {
            engine
                .catalog
                .get(&c.definition_id)
                .map(|d| d.card_type == CardType::CHARACTER && d.level >= (next_natural_file - 2).max(1) && d.level <= next_natural_file + 1)
                .unwrap_or(false)
        });
        let max_cutin = my_ps
            .hand
            .iter()
            .filter_map(|c| engine.catalog.get(&c.definition_id))
            .map(Self::parse_cutin_bonus)
            .max()
            .unwrap_or(0) as f64;
        score += (hand_cap + if has_next_curve { 1.5 } else { 0.0 } + max_cutin / 2000.0)
            * w.hand_quality_weight.eval(phase);

        // 5. 相手リーサル脅威・山札リフレッシュ危険度
        let lethal_excess = self.estimate_opponent_lethal_excess(engine, opp_id);
        if lethal_excess >= 0 {
            score += w.lethal_threat_penalty * (1.0 + 0.25 * lethal_excess as f64);
        }
        let my_rem_deck = self.estimate_remaining_deck(engine, &self.player_id);
        if my_rem_deck <= 4 {
            score += w.deck_refresh_danger_penalty * ((5 - my_rem_deck) as f64 / 4.0);
        }

        score
    }

    /// 第2部 2.2節: 1手先の「相手盤面応答」リスク評価
    /// 特定の自キャラをスリープさせた場合、返しターンに相手の場キャラから一方コンタクト除去される期待損失を算出する
    fn estimate_sleep_punish_risk(&self, engine: &MatchEngine, opp_id: &str, my_ap: i32, my_lp: i32) -> i32 {
        let Some(opp_ps) = engine.players.get(opp_id) else {
            return 0;
        };
        let Some(my_ps) = engine.players.get(&self.player_id) else {
            return 0;
        };

        // 返しターンに相手は全キャラがアクティブに復帰する想定
        let opp_can_kill_count = opp_ps
            .field
            .iter()
            .filter(|oc| engine.get_effective_ap(opp_id, oc) >= my_ap)
            .count();

        if opp_can_kill_count == 0 {
            return 0;
        }

        // 自分が他にアクティブブロッカーを残せるならリスク軽減
        let other_active = my_ps
            .field
            .iter()
            .filter(|fc| fc.orientation == CardOrientation::ACTIVE)
            .count()
            .saturating_sub(1);

        let base_loss = (my_ap / 2) + my_lp * 950;
        if other_active == 0 {
            (base_loss as f64 * self.weights.opponent_response_discount) as i32
        } else {
            (base_loss as f64 * self.weights.opponent_response_discount * 0.45) as i32
        }
    }

    /// 縦置き（アクティブ残し）すべき局面か判定する（第1部 1.4節）
    fn should_preserve_active_blocker(&self, engine: &MatchEngine, opp_id: &str, actor_id: &str) -> bool {
        let Some(my_ps) = engine.players.get(&self.player_id) else {
            return false;
        };
        let Some(fc) = my_ps.field.iter().find(|c| c.instance_id == actor_id) else {
            return false;
        };

        // 今ターンリーサルに届くならブロッカー温存不要（全力で決め切る）
        if self.can_reach_lethal_this_turn(engine, opp_id) {
            return false;
        }

        let active_count = my_ps
            .field
            .iter()
            .filter(|c| c.orientation == CardOrientation::ACTIVE)
            .count();

        let my_ap = engine.get_effective_ap(&self.player_id, fc);
        let my_lp = engine.get_effective_lp(&self.player_id, fc);
        let opp_lethal_excess = self.estimate_opponent_lethal_excess(engine, opp_id);
        let profile = self.analyze_opponent_profile(engine, opp_id);

        // 1. 終盤リーサルケア: 次ターン相手リーサル圏内なら最低1〜2面アクティブを維持
        let required_blockers = if profile.blocker_bypass_risk { 1 } else { 2 };
        if opp_lethal_excess >= -1 && active_count <= required_blockers {
            return true;
        }

        // 2. 証拠0枚コントロール: 赤・白等の速攻対面で証拠0枚・アクティブ1体のみなら安易に寝かせない
        if profile.avoid_early_p_deduce
            && my_ps.evidence.is_empty()
            && my_ps.case_phase != CasePhase::RESOLUTION
            && active_count <= 1
        {
            return true;
        }

        // 3. 序盤の盤面保全 (事件編): 寝かせると返しターンに相手キャラで一方処理される主力は縦維持
        if my_ps.case_phase != CasePhase::RESOLUTION && my_lp <= 1 {
            let punish = self.estimate_sleep_punish_risk(engine, opp_id, my_ap, my_lp);
            if punish >= 2200 && active_count <= 2 {
                return true;
            }
        }

        false
    }

    /// 【宣言】効果が空撃ちや自傷にならないか厳密に事前検証する
    fn is_declare_beneficial(&self, engine: &MatchEngine, actor_id: &str, opp_id: &str) -> bool {
        let Some(my_ps) = engine.players.get(&self.player_id) else {
            return false;
        };
        let opp_ps = engine.players.get(opp_id);
        let opp_field_len = opp_ps.map(|o| o.field.len()).unwrap_or(0);

        // 事件カードの宣言（青の古城探索事件 / ブラックインパクト！など）
        if let Some(case_card) = &my_ps.r#case {
            if case_card.instance_id == actor_id {
                let Some(def) = engine.catalog.get(&case_card.definition_id) else {
                    return false;
                };
                let desc = &def.description;
                let name = def.names.first().map(|s| s.as_str()).unwrap_or("");

                if name.contains("青の古城") || desc.contains("少年探偵団") {
                    let kids_count = my_ps
                        .field
                        .iter()
                        .filter(|fc| {
                            engine
                                .catalog
                                .get(&fc.definition_id)
                                .map(|d| d.traits.iter().any(|t| t.contains("少年探偵団")))
                                .unwrap_or(false)
                        })
                        .count() as i32;
                    if kids_count == 0 {
                        return false;
                    }
                    let has_valid_opp = opp_ps
                        .map(|o| {
                            o.field.iter().any(|oc| {
                                engine
                                    .catalog
                                    .get(&oc.definition_id)
                                    .map(|d| d.level <= kids_count + 2)
                                    .unwrap_or(false)
                            })
                        })
                        .unwrap_or(false);
                    return has_valid_opp;
                }

                if name.contains("ブラックインパクト") || desc.contains("相手は手札") {
                    return opp_ps.map(|o| !o.hand.is_empty()).unwrap_or(false);
                }

                if desc.contains("相手の現場") || desc.contains("キャラを1枚") || desc.contains("リムーブ") {
                    return opp_field_len > 0;
                }
                return true;
            }
        }

        // フィールドキャラの宣言
        if let Some(fc) = my_ps.field.iter().find(|c| c.instance_id == actor_id) {
            let Some(def) = engine.catalog.get(&fc.definition_id) else {
                return false;
            };
            let desc = &def.description;
            let name = def.names.first().map(|s| s.as_str()).unwrap_or("");

            // Strongest のみ: リフレッシュ自爆防止のため山札極少時の強制ドロー宣言を警戒
            if self.difficulty == CpuDifficulty::Strongest
                && self.estimate_remaining_deck(engine, &self.player_id) <= 1
                && (desc.contains("カードを引く") || desc.contains("1枚引く"))
                && !self.can_reach_lethal_this_turn(engine, opp_id)
            {
                return false;
            }

            if name.contains("ベルモット") && !name.contains("ジン") && desc.contains("リムーブ") {
                if opp_field_len == 0 {
                    return false;
                }
                let has_fodder = my_ps.field.iter().any(|c| {
                    c.instance_id != actor_id
                        && (engine.acted_this_turn_ids.contains(&c.instance_id)
                            || engine
                                .catalog
                                .get(&c.definition_id)
                                .map(|d| d.level <= 4)
                                .unwrap_or(false))
                });
                return has_fodder;
            }

            if desc.contains("このキャラ以外の") && my_ps.field.len() <= 1 {
                return false;
            }

            if desc.contains("相手の現場") || desc.contains("相手キャラ") || desc.contains("リムーブする") {
                if opp_field_len == 0 {
                    return false;
                }
                if desc.contains("少年探偵団") && my_ps.field.len() <= 1 {
                    return false;
                }
            }

            if (desc.contains("手札を1枚") || desc.contains("手札から")) && my_ps.hand.is_empty() {
                return false;
            }

            return true;
        }

        false
    }

    /// 宣言効果が「除去・情報獲得（ドロー/サーチ）」など盤面行動より先に切るべき高優先宣言か判定
    fn is_high_priority_declare(&self, engine: &MatchEngine, actor_id: &str) -> bool {
        let Some(my_ps) = engine.players.get(&self.player_id) else {
            return false;
        };
        let def = if let Some(cc) = &my_ps.r#case {
            if cc.instance_id == actor_id {
                engine.catalog.get(&cc.definition_id)
            } else {
                None
            }
        } else {
            None
        }
        .or_else(|| {
            my_ps
                .field
                .iter()
                .find(|c| c.instance_id == actor_id)
                .and_then(|c| engine.catalog.get(&c.definition_id))
        });

        let Some(def) = def else {
            return false;
        };
        let d = &def.description;
        Self::is_info_gain_card(def)
            || d.contains("リムーブ")
            || d.contains("スタン")
            || d.contains("スリープ")
            || d.contains("AP+")
            || d.contains("AP＋")
    }

    fn is_meaningful_hand_card(
        &self,
        engine: &MatchEngine,
        my_ps: &crate::engine::PlayerState,
        opp_field_len: usize,
        def: &CardDefinition,
    ) -> bool {
        let my_field_len = my_ps.field.len();
        let name = def.names.first().map(|s| s.as_str()).unwrap_or("");
        let desc = &def.description;
        if def.card_type == CardType::EVENT {
            if name.contains("五人囃子") || desc.contains("リムーブエリアから") {
                let has_revivable = my_ps.remove.iter().any(|rc| {
                    engine
                        .catalog
                        .get(&rc.definition_id)
                        .map(|d| d.card_type == CardType::CHARACTER && d.level <= 4)
                        .unwrap_or(false)
                });
                return has_revivable && my_field_len < 4;
            }
            if name == "カッ" || desc.contains("すべてをリムーブ") {
                return opp_field_len >= 2 && opp_field_len > my_field_len;
            }
            if name == "少年探偵団" {
                return my_ps.hand.len() >= 2 && my_field_len < 4;
            }
            if desc.contains("リムーブ") || desc.contains("スリープ") || desc.contains("スタン") {
                return opp_field_len > 0;
            }
        }
        true
    }

    /// Normal / Hard 用の手札プレイ前 ASSIST 判定
    fn should_assist_before_hand_play_standard(&self, engine: &MatchEngine, opp_id: &str) -> bool {
        let Some(my_ps) = engine.players.get(&self.player_id) else {
            return false;
        };
        let eff_file = my_ps.effective_file_count();
        let next_file = eff_file + 1;

        if my_ps.case_phase != CasePhase::RESOLUTION && eff_file == 6 {
            return true;
        }
        if my_ps.normal_hand_played_this_turn || my_ps.hint_played_this_turn {
            return false;
        }

        let opp_field_len = engine.players.get(opp_id).map(|o| o.field.len()).unwrap_or(0);

        if (5..=7).contains(&next_file) {
            let tag1 = format!("【FILE{}】", next_file);
            let tag2 = format!("【FILE {}】", next_file);
            let unlocks_file_req = my_ps.hand.iter().any(|c| {
                engine
                    .catalog
                    .get(&c.definition_id)
                    .map(|d| {
                        d.level <= next_file as i32
                            && (d.description.contains(&tag1) || d.description.contains(&tag2))
                            && self.is_meaningful_hand_card(engine, my_ps, opp_field_len, d)
                    })
                    .unwrap_or(false)
            });
            if unlocks_file_req {
                return true;
            }
        }

        let has_unlockable_card = my_ps.hand.iter().any(|c| {
            engine
                .catalog
                .get(&c.definition_id)
                .map(|d| {
                    d.level == next_file as i32
                        && self.is_meaningful_hand_card(engine, my_ps, opp_field_len, d)
                })
                .unwrap_or(false)
        });
        if !has_unlockable_card {
            return false;
        }

        if engine.turn == 1 {
            return true;
        }
        if self.difficulty == CpuDifficulty::Hard && next_file >= 6 {
            return true;
        }

        let has_current_playable = my_ps.hand.iter().any(|c| {
            engine
                .catalog
                .get(&c.definition_id)
                .map(|d| {
                    let name = d.names.first().map(|s| s.as_str()).unwrap_or("");
                    let is_proactive = d.card_type == CardType::CHARACTER
                        || name.contains("烏丸蓮耶の影")
                        || name.contains("小さくなった名探偵");
                    d.level <= eff_file as i32
                        && is_proactive
                        && self.is_meaningful_hand_card(engine, my_ps, opp_field_len, d)
                })
                .unwrap_or(false)
        });
        !has_current_playable
    }

    /// Strongest 用の手札プレイ前 ASSIST 判定 (第1部 1.2節反映)
    fn should_assist_before_hand_play_strongest(&self, engine: &MatchEngine, opp_id: &str) -> bool {
        let Some(my_ps) = engine.players.get(&self.player_id) else {
            return false;
        };

        // リーサル確定ターンはパートナーを事件解決用に絶対温存！(第1部 1.6節)
        if self.can_reach_lethal_this_turn(engine, opp_id) {
            return false;
        }

        let eff_file = my_ps.effective_file_count();
        let next_file = eff_file + 1;
        let is_first = self.is_first_player(engine);
        let my_t = self.my_turn_index(engine);

        // 1. FILEが6枚でまだ解決編でないなら、ASSISTで最速解決編(FILE 7)へ移行（後攻3Tの鉄則）
        if my_ps.case_phase != CasePhase::RESOLUTION && eff_file == 6 {
            return true;
        }

        if my_ps.normal_hand_played_this_turn || my_ps.hint_played_this_turn {
            return false;
        }

        let opp_ps = engine.players.get(opp_id);
        let opp_field_len = opp_ps.map(|o| o.field.len()).unwrap_or(0);
        let opp_sleep_len = opp_ps
            .map(|o| o.field.iter().filter(|c| c.orientation != CardOrientation::ACTIVE).count())
            .unwrap_or(0);

        // 2. ASSISTすることで【FILE5】〜【FILE7】効果の条件を達成し、そのターン中に出せるカードがあるとき
        if (5..=7).contains(&next_file) {
            let tag1 = format!("【FILE{}】", next_file);
            let tag2 = format!("【FILE {}】", next_file);
            let unlocks_file_req = my_ps.hand.iter().any(|c| {
                engine
                    .catalog
                    .get(&c.definition_id)
                    .map(|d| {
                        d.level <= next_file as i32
                            && (d.description.contains(&tag1) || d.description.contains(&tag2))
                            && self.is_meaningful_hand_card(engine, my_ps, opp_field_len, d)
                    })
                    .unwrap_or(false)
            });
            if unlocks_file_req {
                return true;
            }
        }

        // 3. ASSISTによって初めてプレイ可能になるレベル (level == next_file) の有効なカードが手札にあるか
        let unlockable_cards: Vec<&CardDefinition> = my_ps
            .hand
            .iter()
            .filter_map(|c| engine.catalog.get(&c.definition_id))
            .filter(|d| d.level == next_file as i32 && self.is_meaningful_hand_card(engine, my_ps, opp_field_len, d))
            .collect();

        if unlockable_cards.is_empty() {
            return false;
        }

        // 4. 先攻・後攻ごとの定石テーブル (第1部 1.2節)
        if is_first {
            match my_t {
                1 => return true,
                2 => {
                    let has_strong_lv4 = unlockable_cards
                        .iter()
                        .any(|d| d.card_type == CardType::CHARACTER && (Self::is_info_gain_card(d) || d.description.contains("【登場時】")));
                    if has_strong_lv4 {
                        return true;
                    }
                }
                3 => {
                    let has_power_lv6 = unlockable_cards.iter().any(|d| {
                        d.description.contains("リムーブ")
                            || d.description.contains("変装")
                            || d.description.contains("スタン")
                            || d.lp >= 2
                    });
                    if has_power_lv6 {
                        return true;
                    }
                }
                _ => {
                    if next_file >= 8 {
                        return true;
                    }
                }
            }
        } else {
            match my_t {
                1 => {
                    let has_tempo_lv3 = unlockable_cards
                        .iter()
                        .any(|d| d.card_type == CardType::CHARACTER && (d.ap >= 4000 || d.description.contains("AP+")));
                    if has_tempo_lv3 {
                        return true;
                    }
                }
                2 => {
                    let has_lv5_punish = unlockable_cards.iter().any(|d| {
                        (Self::has_rush(d) && opp_sleep_len > 0)
                            || (d.card_type == CardType::EVENT
                                && (d.description.contains("リムーブ") || d.description.contains("スタン"))
                                && opp_field_len > 0)
                    });
                    if has_lv5_punish {
                        return true;
                    }
                }
                _ => {
                    if next_file >= 8 {
                        return true;
                    }
                }
            }
        }

        let has_current_playable = my_ps.hand.iter().any(|c| {
            engine
                .catalog
                .get(&c.definition_id)
                .map(|d| {
                    let name = d.names.first().map(|s| s.as_str()).unwrap_or("");
                    let is_proactive = d.card_type == CardType::CHARACTER
                        || name.contains("烏丸蓮耶の影")
                        || name.contains("小さくなった名探偵");
                    d.level <= eff_file as i32
                        && is_proactive
                        && self.is_meaningful_hand_card(engine, my_ps, opp_field_len, d)
                })
                .unwrap_or(false)
        });
        !has_current_playable
    }

    /// ネクストヒント (HINT_TAKE) を使うべきか厳密判定する (第1部 1.3節反映)
    fn should_use_next_hint(&self, engine: &MatchEngine, opp_id: &str) -> bool {
        let Some(my_ps) = engine.players.get(&self.player_id) else {
            return false;
        };
        if self.last_hint_skipped.load(Ordering::Relaxed) || my_ps.field.len() >= 5 {
            return false;
        }

        let eff_file = my_ps.effective_file_count();
        let is_first = self.is_first_player(engine);
        let my_t = self.my_turn_index(engine);

        // 1. リーサル確定ターン: 次ターンがないためFILEを全変換して決め切る
        if self.can_reach_lethal_this_turn(engine, opp_id) && eff_file >= 2 {
            return true;
        }

        // 2. 事件編(先攻1~3T・後攻1~2T)は原則不使用 (第1部 1.3節 ⑤)
        if my_ps.case_phase != CasePhase::RESOLUTION {
            return false;
        }

        // 3. 後攻3Tの解決編移行直後はNH厳禁（自然FILE 5へ転落し後攻4TのMR9/8+8を失うため）(第1部 1.3節 ⑥)
        if !is_first && my_t == 3 && eff_file <= 7 {
            return false;
        }

        let next_file_level = eff_file.saturating_sub(1) as i32;
        let opp_lethal_danger = self.estimate_opponent_lethal_excess(engine, opp_id) >= 0;

        // 4. 終了時FILE 6以上を維持できるか（FILE 5以下への転落は相手リーサル阻止時のみ許容）(第1部 1.3節 ②③⑦)
        if next_file_level < 6 && !opp_lethal_danger {
            return false;
        }

        let has_playable_in_hand = my_ps.hand.iter().any(|c| {
            engine
                .catalog
                .get(&c.definition_id)
                .map(|d| {
                    d.level <= next_file_level
                        && (d.card_type == CardType::CHARACTER
                            || (opp_lethal_danger
                                && (d.description.contains("リムーブ") || d.description.contains("スタン"))))
                })
                .unwrap_or(false)
        });
        let top_file_playable = my_ps
            .file
            .last()
            .and_then(|fc| engine.catalog.get(&fc.definition_id))
            .map(|d| {
                d.card_type == CardType::CHARACTER
                    && d.level <= next_file_level
                    && !(next_file_level < 5
                        && (d.description.contains("【FILE5】") || d.description.contains("【FILE 5】")))
            })
            .unwrap_or(false);

        if !has_playable_in_hand && !top_file_playable {
            return false;
        }

        let max_hints = if opp_lethal_danger || eff_file >= 8 { 2 } else { 1 };
        let used = self.hints_used_this_turn.load(Ordering::Relaxed);
        used < max_hints
    }

    /// 共通 DISGUISE 評価ヘルパー
    fn choose_best_disguise(
        &self,
        engine: &MatchEngine,
        my_ps: &crate::engine::PlayerState,
        act: &Value,
    ) -> Option<Value> {
        let card_ids = act.get("cardIds").and_then(|v| v.as_array())?;
        let target_ids = act.get("targetIds").and_then(|v| v.as_array())?;
        let mut best_pair: Option<(&str, &str)> = None;
        let mut best_gain = 0;
        for cid_v in card_ids {
            let Some(cid) = cid_v.as_str() else { continue };
            let h_ap = my_ps
                .hand
                .iter()
                .find(|c| c.instance_id == cid)
                .and_then(|c| engine.catalog.get(&c.definition_id))
                .map(|d| d.ap)
                .unwrap_or(0);
            for tid_v in target_ids {
                let Some(tid) = tid_v.as_str() else { continue };
                if let Some(fc) = my_ps.field.iter().find(|c| c.instance_id == tid) {
                    let f_ap = engine.get_effective_ap(&self.player_id, fc);
                    let bonus = if fc.orientation != CardOrientation::ACTIVE { 1500 } else { 0 };
                    let gain = (h_ap - f_ap) + bonus;
                    if gain > best_gain {
                        best_gain = gain;
                        best_pair = Some((cid, tid));
                    }
                }
            }
        }
        best_pair.map(|(cid, tid)| json!({"value": {"type": "DISGUISE", "cardId": cid, "targetId": tid}}))
    }

    /// 共通 targetSelect 評価ヘルパー (Strongest では盤面危険度・ミスリード・縦置きを加点)
    fn choose_target_select_with_engine(
        &self,
        engine: &MatchEngine,
        my_ps: &crate::engine::PlayerState,
        opp_ps: Option<&crate::engine::PlayerState>,
        opp_id: &str,
        opts: &Value,
    ) -> Value {
        let actor_id = opts.get("actorId").and_then(|v| v.as_str()).unwrap_or("");
        let effect_type = opts.get("effectType").and_then(|v| v.as_str()).unwrap_or("");
        let optional = opts.get("optional").and_then(|v| v.as_bool()).unwrap_or(false);
        let max_select = opts.get("maxSelect").and_then(|v| v.as_u64()).unwrap_or(1).max(1) as usize;
        let distinct_names = opts.get("distinctNames").and_then(|v| v.as_bool()).unwrap_or(false);
        let distinct_levels = opts.get("distinctLevels").and_then(|v| v.as_bool()).unwrap_or(false);
        let distinct_colors = opts.get("distinctColors").and_then(|v| v.as_bool()).unwrap_or(false);
        let max_total_level = opts.get("maxTotalLevel").and_then(|v| v.as_i64());

        if let Some(candidates) = opts.get("candidates").and_then(|v| v.as_array()) {
            let all_cand_are_my_field = !candidates.is_empty()
                && candidates.iter().all(|c| {
                    c.get("instanceId")
                        .and_then(|v| v.as_str())
                        .map(|iid| my_ps.field.iter().any(|fc| fc.instance_id == iid))
                        .unwrap_or(false)
                });
            if optional && effect_type == "REMOVE" && all_cand_are_my_field {
                let opp_has_field = opp_ps.map(|o| !o.field.is_empty()).unwrap_or(false);
                if !opp_has_field {
                    return json!({"value": {"skip": true, "actorId": actor_id, "effectType": effect_type}});
                }
            }

            let is_strongest = self.difficulty == CpuDifficulty::Strongest;
            let mut scored: Vec<(&Value, &str, i32)> = candidates
                .iter()
                .filter_map(|cand| {
                    let tid = cand.get("instanceId").and_then(|v| v.as_str())?;
                    let ap = cand.get("ap").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                    let lvl = cand.get("level").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                    let name = cand.get("name").and_then(|v| v.as_str()).unwrap_or("");

                    let mut score = ap + lvl * if is_strongest { 550 } else { 500 };

                    if is_strongest && !all_cand_are_my_field && effect_type != "DISCARD_HAND" {
                        if let Some(opp) = opp_ps {
                            if let Some(oc) = opp.field.iter().find(|c| c.instance_id == tid) {
                                let o_lp = engine.get_effective_lp(opp_id, oc);
                                score += o_lp * 1100;
                                if oc.orientation == CardOrientation::ACTIVE {
                                    score += 1200;
                                }
                                if let Some(def) = engine.catalog.get(&oc.definition_id) {
                                    if Self::has_mislead(def) {
                                        score += 1800;
                                    }
                                    if def.level >= 9 {
                                        score += 1500;
                                    }
                                }
                            }
                        }
                    }

                    if effect_type == "DISCARD_HAND" {
                        if name.contains("五人囃子") || name.contains("沖野ヨーコ") {
                            score -= 6000;
                        }
                        if lvl >= 7
                            || name.contains("ジン")
                            || name.contains("ベルモット")
                            || name.contains("烏丸蓮耶の影")
                            || name == "少年探偵団"
                        {
                            score += 8000;
                        }
                    } else if effect_type == "REMOVE" && all_cand_are_my_field {
                        if name.contains("ウォッカ") && engine.acted_this_turn_ids.contains(tid) {
                            score = -10000;
                        }
                    }

                    Some((cand, tid, score))
                })
                .collect();

            if effect_type == "DISCARD_HAND"
                || effect_type == "FIELD_OVERFLOW_REMOVE"
                || (effect_type == "REMOVE" && all_cand_are_my_field)
            {
                scored.sort_by_key(|&(_, _, s)| s);
            } else {
                scored.sort_by(|a, b| b.2.cmp(&a.2));
            }

            let mut chosen_ids: Vec<&str> = Vec::new();
            let mut used_names = HashSet::new();
            let mut used_levels = HashSet::new();
            let mut used_colors = HashSet::new();
            let mut sum_level: i64 = 0;

            for (c, iid, _) in scored {
                if chosen_ids.len() >= max_select {
                    break;
                }
                let clv = c.get("level").and_then(|v| v.as_i64()).unwrap_or(0);
                let ccols: Vec<&str> = c
                    .get("colors")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|x| x.as_str()).collect())
                    .unwrap_or_default();

                let cnames = Self::candidate_distinct_names(c);
                if distinct_names && !cnames.is_empty() && cnames.iter().any(|n| used_names.contains(n)) {
                    continue;
                }
                if distinct_levels && used_levels.contains(&clv) {
                    continue;
                }
                if distinct_colors && ccols.iter().any(|col| used_colors.contains(*col)) {
                    continue;
                }
                if let Some(cap) = max_total_level {
                    if sum_level + clv > cap {
                        continue;
                    }
                }

                if distinct_names {
                    for n in cnames {
                        used_names.insert(n);
                    }
                }
                if distinct_levels {
                    used_levels.insert(clv);
                }
                if distinct_colors {
                    for col in ccols {
                        used_colors.insert(col.to_string());
                    }
                }
                sum_level += clv;
                chosen_ids.push(iid);
            }

            if let Some(&first_id) = chosen_ids.first() {
                return json!({
                    "value": {
                        "targetId": first_id,
                        "targetIds": chosen_ids,
                        "actorId": actor_id,
                        "effectType": effect_type
                    }
                });
            }
        }
        if optional {
            return json!({"value": {"skip": true, "actorId": actor_id, "effectType": effect_type}});
        }
        json!({
            "value": {
                "targetId": null,
                "targetIds": [],
                "actorId": actor_id,
                "effectType": effect_type
            }
        })
    }

    /// 共通 deckSearch 評価ヘルパー
    fn choose_deck_search_with_engine(
        &self,
        my_ps: &crate::engine::PlayerState,
        opts: &Value,
    ) -> Value {
        let actor_id = opts.get("actorId").and_then(|v| v.as_str()).unwrap_or("");
        let max_select = opts.get("maxSelect").and_then(|v| v.as_u64()).unwrap_or(1).max(1) as usize;
        let distinct_colors = opts.get("distinctColors").and_then(|v| v.as_bool()).unwrap_or(false);

        if let Some(cards) = opts.get("revealedCards").and_then(|v| v.as_array()) {
            let eff_file = my_ps.effective_file_count() as i32;
            let next_target_lv = (eff_file + 2).min(9);
            let is_strongest = self.difficulty == CpuDifficulty::Strongest;
            let mut scored: Vec<(&Value, &str, i32)> = cards
                .iter()
                .filter(|c| c.get("selectable").and_then(|v| v.as_bool()).unwrap_or(true))
                .filter_map(|c| {
                    let cid = c.get("instanceId").and_then(|v| v.as_str())?;
                    let lvl = c.get("level").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                    let ap = c.get("ap").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                    let curve_bonus = if is_strongest {
                        if lvl == next_target_lv || lvl == next_target_lv + 1 {
                            4200
                        } else if lvl <= eff_file + 2 {
                            2800
                        } else {
                            0
                        }
                    } else if lvl <= eff_file + 2 {
                        3000
                    } else {
                        0
                    };
                    Some((c, cid, curve_bonus + lvl * 500 + ap))
                })
                .collect();

            scored.sort_by(|a, b| b.2.cmp(&a.2));

            let mut chosen_ids: Vec<&str> = Vec::new();
            let mut used_colors = HashSet::new();

            for (c, cid, _) in scored {
                if chosen_ids.len() >= max_select {
                    break;
                }
                let ccols: Vec<&str> = c
                    .get("colors")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|x| x.as_str()).collect())
                    .unwrap_or_default();
                if distinct_colors && ccols.iter().any(|col| used_colors.contains(*col)) {
                    continue;
                }
                if distinct_colors {
                    for col in ccols {
                        used_colors.insert(col.to_string());
                    }
                }
                chosen_ids.push(cid);
            }

            if let Some(&first_id) = chosen_ids.first() {
                return json!({
                    "value": {
                        "chosenCardId": first_id,
                        "chosenCardIds": chosen_ids,
                        "actorId": actor_id
                    }
                });
            }
        }
        json!({
            "value": {
                "chosenCardId": null,
                "chosenCardIds": [],
                "actorId": actor_id
            }
        })
    }

    /// 盤面の公開情報と自分の手札を参照して難易度に応じた戦略的意思決定を行う
    pub fn choose_action_with_engine(&self, engine: &MatchEngine, decision: &Value) -> Value {
        self.reset_turn_state_if_needed(engine.turn);
        match self.difficulty {
            CpuDifficulty::Easy => self.choose_action(decision),
            CpuDifficulty::Normal | CpuDifficulty::Hard => self.choose_action_standard(engine, decision),
            CpuDifficulty::Strongest => self.choose_action_strongest(engine, decision),
        }
    }

    /// Normal / Hard 難易度向けの標準競技ヒューリスティック
    fn choose_action_standard(&self, engine: &MatchEngine, decision: &Value) -> Value {
        let kind = decision.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        let opts = decision.get("options").cloned().unwrap_or(json!({}));

        let my_ps = match engine.players.get(&self.player_id) {
            Some(ps) => ps,
            None => return self.choose_action(decision),
        };
        let opp_id = engine
            .player_order
            .iter()
            .find(|pid| *pid != &self.player_id)
            .cloned()
            .unwrap_or_default();
        let opp_ps = engine.players.get(&opp_id);

        match kind {
            "mulligan" => {
                let mut return_ids = vec![];
                let mut kept_early_cards = 0;
                let mut seen_names = HashSet::new();

                if let Some(hand_ids) = opts.get("handCardIds").and_then(|v| v.as_array()) {
                    for id_val in hand_ids {
                        if let Some(cid) = id_val.as_str() {
                            if let Some(c) = my_ps.hand.iter().find(|h| h.instance_id == cid) {
                                let def = engine.catalog.get(&c.definition_id);
                                let lvl = def.map(|d| d.level).unwrap_or(5);
                                let name = def
                                    .and_then(|d| d.names.first())
                                    .map(|s| s.as_str())
                                    .unwrap_or("");
                                let is_search_event = name.contains("烏丸蓮耶の影") || name.contains("小さくなった名探偵");
                                let is_haibara_lv4 = name.contains("灰原哀") && lvl <= 4;
                                let is_low_char = def.map(|d| d.card_type == CardType::CHARACTER).unwrap_or(false) && lvl <= 2;

                                let is_duplicate = !name.is_empty() && seen_names.contains(name) && !is_search_event;

                                if (is_search_event || is_low_char || is_haibara_lv4)
                                    && !is_duplicate
                                    && kept_early_cards < 3
                                {
                                    kept_early_cards += 1;
                                    if !name.is_empty() {
                                        seen_names.insert(name.to_string());
                                    }
                                } else {
                                    return_ids.push(json!(cid));
                                }
                            }
                        }
                    }
                }
                json!({"value": {"cardIds": return_ids}})
            }
            "mainAction" => {
                let empty_vec = vec![];
                let actions = opts.get("actions").and_then(|v| v.as_array()).unwrap_or(&empty_vec);
                let find_act = |t: &str| {
                    actions
                        .iter()
                        .find(|a| a.get("type").and_then(|v| v.as_str()) == Some(t))
                };

                if find_act("RESOLVE_CASE").is_some() {
                    return json!({"value": {"type": "RESOLVE_CASE"}});
                }

                let eff_file = my_ps.effective_file_count();

                if find_act("ASSIST").is_some() && self.should_assist_before_hand_play_standard(engine, &opp_id) {
                    return json!({"value": {"type": "ASSIST"}});
                }

                if let Some(act) = find_act("HAND_PLAY") {
                    if let Some(card_ids) = act.get("cardIds").and_then(|v| v.as_array()) {
                        let mut best_cid: Option<&str> = None;
                        let mut best_score = i32::MIN;
                        let opp_field_count = opp_ps.map(|o| o.field.len()).unwrap_or(0);
                        let my_field_count = my_ps.field.len();
                        let can_assist_later = find_act("ASSIST").is_some();

                        for cid_val in card_ids {
                            if let Some(cid) = cid_val.as_str() {
                                if let Some(c) = my_ps.hand.iter().find(|h| h.instance_id == cid) {
                                    if let Some(def) = engine.catalog.get(&c.definition_id) {
                                        let name = def.names.first().map(|s| s.as_str()).unwrap_or("");
                                        let desc = &def.description;

                                        let mut score_f = (def.level as f64) * self.weights.level_weight
                                            + (def.ap as f64) * self.weights.ap_weight
                                            + (def.lp as f64) * self.weights.lp_weight;

                                        if desc.contains("突撃") || desc.contains("迅速") {
                                            score_f += self.weights.rush_bonus;
                                        }
                                        if desc.contains("【登場時】") {
                                            score_f += self.weights.on_play_bonus;
                                        }

                                        for req_f in [5usize, 6, 7] {
                                            let tag1 = format!("【FILE{}】", req_f);
                                            let tag2 = format!("【FILE {}】", req_f);
                                            if eff_file < req_f && (desc.contains(&tag1) || desc.contains(&tag2)) {
                                                score_f -= if can_assist_later { 9000.0 } else { 4500.0 };
                                            }
                                        }

                                        if desc.contains("自分の現場に登場したとき") {
                                            score_f += 2500.0;
                                        }

                                        if name == "ラム" && desc.contains("リムーブエリア") {
                                            let has_good_target = my_ps.remove.iter().any(|rc| {
                                                engine
                                                    .catalog
                                                    .get(&rc.definition_id)
                                                    .map(|d| d.card_type == CardType::CHARACTER && d.level >= 4)
                                                    .unwrap_or(false)
                                            });
                                            if has_good_target && my_field_count < 4 {
                                                score_f += 2200.0;
                                            } else {
                                                score_f -= 3500.0;
                                            }
                                        }

                                        if def.card_type == CardType::EVENT {
                                            score_f += self.weights.event_penalty;

                                            if name.contains("烏丸蓮耶の影") || name.contains("小さくなった名探偵") {
                                                score_f += 3800.0;
                                            } else if name == "少年探偵団" {
                                                if my_ps.hand.len() >= 2 && my_field_count < 4 {
                                                    score_f += 3200.0;
                                                } else {
                                                    score_f -= 5000.0;
                                                }
                                            } else if name.contains("五人囃子") || desc.contains("リムーブエリアから") {
                                                let has_revivable = my_ps.remove.iter().any(|rc| {
                                                    engine
                                                        .catalog
                                                        .get(&rc.definition_id)
                                                        .map(|d| d.card_type == CardType::CHARACTER && d.level <= 4)
                                                        .unwrap_or(false)
                                                });
                                                if has_revivable && my_field_count < 4 {
                                                    score_f += 2200.0;
                                                } else {
                                                    score_f -= 9000.0;
                                                }
                                            } else if name == "カッ" || desc.contains("すべてをリムーブ") {
                                                if opp_field_count >= 2 && opp_field_count > my_field_count {
                                                    score_f += 3500.0;
                                                } else {
                                                    score_f -= 7000.0;
                                                }
                                            } else if desc.contains("リムーブ") || desc.contains("スリープ") || desc.contains("スタン") {
                                                if opp_field_count == 0 {
                                                    score_f -= 7000.0;
                                                } else {
                                                    score_f += self.weights.removal_bonus;
                                                }
                                            }
                                        }

                                        let score = score_f as i32;
                                        if score > best_score {
                                            best_score = score;
                                            best_cid = Some(cid);
                                        }
                                    }
                                }
                            }
                        }
                        if let Some(cid) = best_cid {
                            if best_score > -1000 {
                                return json!({"value": {"type": "HAND_PLAY", "cardId": cid}});
                            }
                        }
                    }
                }

                if self.difficulty == CpuDifficulty::Hard {
                    if let Some(ded_act) = find_act("DEDUCE") {
                        if let Some(actor_ids) = ded_act.get("actorIds").and_then(|v| v.as_array()) {
                            let mut best_priority_deducer: Option<(&str, i32, i32, i32)> = None;
                            for aid_v in actor_ids {
                                if let Some(aid) = aid_v.as_str() {
                                    let is_partner = my_ps.partner.as_ref().map(|p| p.instance_id == aid).unwrap_or(false);
                                    let (lp, ap) = if is_partner {
                                        let lp = my_ps
                                            .partner
                                            .as_ref()
                                            .and_then(|p| engine.catalog.get(&p.definition_id))
                                            .map(|d| d.lp)
                                            .unwrap_or(1);
                                        (lp, 0)
                                    } else if let Some(fc) = my_ps.field.iter().find(|c| c.instance_id == aid) {
                                        let lp = engine.get_effective_lp(&self.player_id, fc);
                                        let ap = engine.get_effective_ap(&self.player_id, fc);
                                        (lp, ap)
                                    } else {
                                        (1, 2000)
                                    };
                                    if is_partner || lp >= 2 {
                                        let key = (if is_partner { 1 } else { 0 }, lp, -ap);
                                        let better = best_priority_deducer
                                            .map(|(_, bp, blp, bap)| key > (bp, blp, bap))
                                            .unwrap_or(true);
                                        if better {
                                            best_priority_deducer = Some((aid, key.0, key.1, key.2));
                                        }
                                    }
                                }
                            }
                            if let Some((aid, _, _, _)) = best_priority_deducer {
                                return json!({"value": {"type": "DEDUCE", "actorId": aid}});
                            }
                        }
                    }
                }

                if let Some(act) = find_act("DECLARE") {
                    if let Some(actors) = act.get("actors").and_then(|v| v.as_array()) {
                        for actor in actors {
                            if let Some(aid) = actor.get("actorId").and_then(|v| v.as_str()) {
                                if self.is_declare_beneficial(engine, aid, &opp_id) {
                                    return json!({"value": {"type": "DECLARE", "actorId": aid}});
                                }
                            }
                        }
                    }
                }

                if let Some(act) = find_act("DISGUISE") {
                    if let Some(ans) = self.choose_best_disguise(engine, my_ps, act) {
                        return ans;
                    }
                }

                if let Some(act) = find_act("ACTION") {
                    let actor_ids = act.get("actorIds").and_then(|v| v.as_array());
                    let target_ids = act.get("targetIds").and_then(|v| v.as_array());
                    let actor_target_map = act.get("actorTargetMap").and_then(|v| v.as_object());

                    let mut best_action: Option<(&str, &str)> = None;
                    let mut best_score = 0;

                    if let (Some(aids), Some(opp)) = (actor_ids, opp_ps) {
                        let max_opp_active_blocker_ap = opp
                            .field
                            .iter()
                            .filter(|c| c.orientation == CardOrientation::ACTIVE)
                            .map(|c| engine.get_effective_ap(&opp_id, c))
                            .max()
                            .unwrap_or(0);

                        for aid_v in aids {
                            let aid = match aid_v.as_str() {
                                Some(s) => s,
                                None => continue,
                            };
                            let my_card = match my_ps.field.iter().find(|c| c.instance_id == aid) {
                                Some(c) => c,
                                None => continue,
                            };
                            let my_ap = engine.get_effective_ap(&self.player_id, my_card);
                            let my_lp = engine.get_effective_lp(&self.player_id, my_card);
                            let can_deduce = find_act("DEDUCE")
                                .and_then(|d| d.get("actorIds"))
                                .and_then(|v| v.as_array())
                                .map(|arr| arr.iter().any(|x| x.as_str() == Some(aid)))
                                .unwrap_or(false);

                            let valid_tids = actor_target_map
                                .and_then(|m| m.get(aid))
                                .and_then(|v| v.as_array())
                                .or(target_ids);

                            if let Some(tids) = valid_tids {
                                for tid_v in tids {
                                    let tid = match tid_v.as_str() {
                                        Some(s) => s,
                                        None => continue,
                                    };
                                    if let Some(opp_c) = opp.field.iter().find(|c| c.instance_id == tid) {
                                        let opp_ap = engine.get_effective_ap(&opp_id, opp_c);
                                        let opp_lp = engine.get_effective_lp(&opp_id, opp_c);
                                        if my_ap >= opp_ap {
                                            let base_rem = (self.weights.attack_remove_weight * 3.5) as i32;
                                            let lp_threat = opp_lp * 1200;
                                            let score = base_rem + opp_ap + lp_threat - (my_ap - opp_ap) / 4;
                                            if score > best_score {
                                                best_score = score;
                                                best_action = Some((aid, tid));
                                            }
                                        }
                                    } else if opp.r#case.as_ref().map(|c| c.instance_id == tid).unwrap_or(false) {
                                        if !opp.evidence.is_empty() {
                                            if max_opp_active_blocker_ap > my_ap && can_deduce {
                                                continue;
                                            }

                                            let urgency = if opp.case_phase == CasePhase::RESOLUTION { 2000 } else { 0 };
                                            let case_bonus = self.weights.case_attack_bonus as i32;
                                            let lp_penalty = if can_deduce {
                                                (my_lp as f64 * self.weights.deduce_lp_weight * 0.7) as i32
                                            } else {
                                                0
                                            };
                                            let score = 3800 + case_bonus + urgency - lp_penalty;
                                            if score > best_score {
                                                best_score = score;
                                                best_action = Some((aid, tid));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if let Some((aid, tid)) = best_action {
                        return json!({"value": {"type": "ACTION", "actorId": aid, "targetId": tid}});
                    }
                }

                if let Some(act) = find_act("DEDUCE") {
                    if let Some(actor_ids) = act.get("actorIds").and_then(|v| v.as_array()) {
                        let mut best_aid: Option<&str> = None;
                        let mut best_lp = -1;
                        for aid_v in actor_ids {
                            if let Some(aid) = aid_v.as_str() {
                                let lp = if my_ps.partner.as_ref().map(|p| p.instance_id == aid).unwrap_or(false) {
                                    my_ps
                                        .partner
                                        .as_ref()
                                        .and_then(|p| engine.catalog.get(&p.definition_id))
                                        .map(|d| d.lp)
                                        .unwrap_or(1)
                                } else {
                                    my_ps
                                        .field
                                        .iter()
                                        .find(|c| c.instance_id == aid)
                                        .map(|c| engine.get_effective_lp(&self.player_id, c))
                                        .unwrap_or(0)
                                };
                                if lp > best_lp {
                                    best_lp = lp;
                                    best_aid = Some(aid);
                                }
                            }
                        }
                        if let Some(aid) = best_aid {
                            return json!({"value": {"type": "DEDUCE", "actorId": aid}});
                        }
                    }
                }

                if find_act("ASSIST").is_some()
                    && my_ps.case_phase != CasePhase::RESOLUTION
                    && eff_file == 6
                {
                    return json!({"value": {"type": "ASSIST"}});
                }

                if find_act("HINT_TAKE").is_some()
                    && !self.last_hint_skipped.load(Ordering::Relaxed)
                    && my_ps.field.len() < 5
                    && eff_file >= 6
                {
                    let next_file_level = (eff_file - 1) as i32;
                    let top_file_playable = my_ps
                        .file
                        .last()
                        .and_then(|fc| engine.catalog.get(&fc.definition_id))
                        .map(|d| {
                            d.card_type == CardType::CHARACTER
                                && d.level <= next_file_level
                                && !(next_file_level < 5
                                    && (d.description.contains("【FILE5】") || d.description.contains("【FILE 5】")))
                        })
                        .unwrap_or(false);

                    let max_hints = if eff_file >= 8 { 2 } else { 1 };
                    let used = self.hints_used_this_turn.load(Ordering::Relaxed);
                    if used < max_hints && top_file_playable {
                        self.hints_used_this_turn.store(used + 1, Ordering::Relaxed);
                        return json!({"value": {"type": "HINT_TAKE"}});
                    }
                }

                json!({"value": {"type": "END_TURN"}})
            }
            "hintPlay" => {
                let eff_file = my_ps.effective_file_count();
                if let Some(playable) = opts.get("playableCardIds").and_then(|v| v.as_array()) {
                    let mut best_cid: Option<&str> = None;
                    let mut best_score = i32::MIN;
                    for cid_v in playable {
                        if let Some(cid) = cid_v.as_str() {
                            if let Some(c) = my_ps.hand.iter().find(|h| h.instance_id == cid) {
                                if let Some(def) = engine.catalog.get(&c.definition_id) {
                                    if def.card_type == CardType::EVENT {
                                        let name = def.names.first().map(|s| s.as_str()).unwrap_or("");
                                        if name.contains("五人囃子") && my_ps.remove.len() < 2 {
                                            continue;
                                        }
                                    }
                                    let mut score = def.level * 1000 + def.ap + def.lp * 400;
                                    if def.description.contains("突撃") {
                                        score += 1500;
                                    }
                                    if eff_file < 5
                                        && (def.description.contains("【FILE 5】") || def.description.contains("【FILE5】"))
                                    {
                                        score -= 3500;
                                    }
                                    if score > best_score {
                                        best_score = score;
                                        best_cid = Some(cid);
                                    }
                                }
                            }
                        }
                    }
                    if let Some(cid) = best_cid {
                        self.last_hint_skipped.store(false, Ordering::Relaxed);
                        return json!({"value": {"cardId": cid}});
                    }
                }
                self.last_hint_skipped.store(true, Ordering::Relaxed);
                json!({"value": {"cardId": null}})
            }
            "guard" => {
                let (attacker_ap, defender_ap, is_case_attack) = engine
                    .action_context
                    .as_ref()
                    .map(|ctx| {
                        let att_pid = ctx.get("attackerPlayerId").and_then(|v| v.as_str()).unwrap_or("");
                        let def_pid = ctx.get("defenderPlayerId").and_then(|v| v.as_str()).unwrap_or("");
                        let actor_id = ctx.get("actorId").and_then(|v| v.as_str()).unwrap_or("");
                        let target_id = ctx.get("targetId").and_then(|v| v.as_str()).unwrap_or("");

                        let att_ap = engine
                            .players
                            .get(att_pid)
                            .and_then(|ps| ps.field.iter().find(|c| c.instance_id == actor_id))
                            .map(|c| engine.get_effective_ap(att_pid, c))
                            .unwrap_or(3000);

                        let def_ap = engine
                            .players
                            .get(def_pid)
                            .and_then(|ps| ps.field.iter().find(|c| c.instance_id == target_id))
                            .map(|c| engine.get_effective_ap(def_pid, c))
                            .unwrap_or(0);

                        let is_case = my_ps.r#case.as_ref().map(|c| c.instance_id == target_id).unwrap_or(false);
                        (att_ap, def_ap, is_case)
                    })
                    .unwrap_or((3000, 0, false));
                let i_am_attacker = engine
                    .action_context
                    .as_ref()
                    .and_then(|ctx| ctx.get("attackerPlayerId").and_then(|v| v.as_str()))
                    == Some(self.player_id.as_str());

                if let (Some(disguises), Some(target_id)) = (
                    opts.get("disguiseOptions").and_then(|v| v.as_array()),
                    opts.get("disguiseTargetId").and_then(|v| v.as_str()),
                ) {
                    if let Some(cur_card) = my_ps.field.iter().find(|c| c.instance_id == target_id) {
                        let cur_ap = engine.get_effective_ap(&self.player_id, cur_card);
                        for d_val in disguises {
                            if let (Some(did), Some(def_id)) = (
                                d_val.get("instanceId").and_then(|v| v.as_str()),
                                d_val.get("definitionId").and_then(|v| v.as_str()),
                            ) {
                                let new_ap = engine.catalog.get(def_id).map(|d| d.ap).unwrap_or(0);
                                if new_ap - cur_ap >= self.weights.disguise_ap_threshold || (cur_ap <= attacker_ap && new_ap > attacker_ap) {
                                    return json!({"value": {"disguiseCardId": did, "targetId": target_id}});
                                }
                            }
                        }
                    }
                }

                if let Some(cutins) = opts.get("cutinCardIds").and_then(|v| v.as_array()) {
                    if !cutins.is_empty() {
                        let need_cutin = if i_am_attacker {
                            defender_ap > 0 && attacker_ap < defender_ap
                        } else {
                            !is_case_attack && defender_ap > 0 && attacker_ap >= defender_ap && (attacker_ap - defender_ap) <= 3000
                        };

                        if need_cutin {
                            if let Some(first_cutin) = cutins.first().and_then(|v| {
                                v.get("instanceId").and_then(|s| s.as_str()).or_else(|| v.as_str())
                            }) {
                                return json!({"value": {"cutinCardId": first_cutin}});
                            }
                        }
                    }
                }

                if let Some(guards) = opts.get("guardCardIds").and_then(|v| v.as_array()) {
                    let mut best_win_guard: Option<(&str, i32)> = None;
                    let mut best_trade_guard: Option<(&str, i32)> = None;
                    let mut weakest_guard: Option<(&str, i32)> = None;

                    for g_val in guards {
                        if let Some(gid) = g_val.as_str() {
                            if let Some(gc) = my_ps.field.iter().find(|c| c.instance_id == gid) {
                                let g_ap = engine.get_effective_ap(&self.player_id, gc);
                                let g_lp = engine.get_effective_lp(&self.player_id, gc);
                                if g_ap > attacker_ap {
                                    if best_win_guard.map(|(_, b_ap)| g_ap < b_ap).unwrap_or(true) {
                                        best_win_guard = Some((gid, g_ap));
                                    }
                                } else if self.difficulty == CpuDifficulty::Hard && g_ap == attacker_ap && g_lp <= 1 {
                                    if best_trade_guard.map(|(_, t_ap)| g_ap < t_ap).unwrap_or(true) {
                                        best_trade_guard = Some((gid, g_ap));
                                    }
                                }
                                if weakest_guard.map(|(_, w_ap)| g_ap < w_ap).unwrap_or(true) {
                                    weakest_guard = Some((gid, g_ap));
                                }
                            }
                        }
                    }

                    if let Some((gid, _)) = best_win_guard {
                        return json!({"value": {"guardCardId": gid}});
                    }
                    if let Some((gid, _)) = best_trade_guard {
                        return json!({"value": {"guardCardId": gid}});
                    }
                    let opp_near_lethal = opp_ps
                        .map(|o| o.case_phase == CasePhase::RESOLUTION && o.evidence.len() >= 5)
                        .unwrap_or(false);
                    if is_case_attack && (my_ps.evidence.len() <= 1 || opp_near_lethal) {
                        if let Some((gid, _)) = weakest_guard {
                            return json!({"value": {"guardCardId": gid}});
                        }
                    }
                    if opts.get("mustGuard").and_then(|v| v.as_bool()).unwrap_or(false) {
                        if let Some((gid, _)) = weakest_guard {
                            return json!({"value": {"guardCardId": gid}});
                        }
                    }
                }
                json!({"value": {"guardCardId": null}})
            }
            "targetSelect" => self.choose_target_select_with_engine(engine, my_ps, opp_ps, &opp_id, &opts),
            "deckSearch" => self.choose_deck_search_with_engine(my_ps, &opts),
            _ => self.choose_action(decision),
        }
    }

    /// 最強難易度向けの研究版戦略的意思決定
    fn choose_action_strongest(&self, engine: &MatchEngine, decision: &Value) -> Value {
        let kind = decision.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        let opts = decision.get("options").cloned().unwrap_or(json!({}));

        let my_ps = match engine.players.get(&self.player_id) {
            Some(ps) => ps,
            None => return self.choose_action(decision),
        };
        let opp_id = engine
            .player_order
            .iter()
            .find(|pid| *pid != &self.player_id)
            .cloned()
            .unwrap_or_default();
        let opp_ps = engine.players.get(&opp_id);

        match kind {
            "mulligan" => {
                // 第1部 1.1節: 先攻・後攻ごとのマリガン基準＋超幾何分布に基づくLv4単体キープ定石
                let is_first = self.is_first_player(engine);
                let mut return_ids = vec![];
                let mut seen_names = HashSet::new();

                let mut hand_cards: Vec<(&str, i32, &str, bool, bool, bool)> = Vec::new();
                if let Some(hand_ids) = opts.get("handCardIds").and_then(|v| v.as_array()) {
                    for id_val in hand_ids {
                        if let Some(cid) = id_val.as_str() {
                            if let Some(c) = my_ps.hand.iter().find(|h| h.instance_id == cid) {
                                let def = engine.catalog.get(&c.definition_id);
                                let lvl = def.map(|d| d.level).unwrap_or(5);
                                let name = def
                                    .and_then(|d| d.names.first())
                                    .map(|s| s.as_str())
                                    .unwrap_or("");
                                let is_char = def
                                    .map(|d| d.card_type == CardType::CHARACTER)
                                    .unwrap_or(false);
                                let is_search_ev = name.contains("烏丸蓮耶の影")
                                    || name.contains("小さくなった名探偵")
                                    || (def.map(|d| d.card_type == CardType::EVENT && d.level <= 1 && Self::is_info_gain_card(d)).unwrap_or(false));
                                let is_lv5_interact = lvl == 5
                                    && def
                                        .map(|d| {
                                            Self::has_rush(d)
                                                || d.description.contains("リムーブ")
                                                || d.description.contains("スタン")
                                        })
                                        .unwrap_or(false);
                                hand_cards.push((cid, lvl, name, is_char, is_search_ev, is_lv5_interact));
                            }
                        }
                    }
                }

                let has_lv2 = hand_cards.iter().any(|&(_, lvl, _, is_char, _, _)| is_char && lvl <= 2);
                let has_lv4 = hand_cards.iter().any(|&(_, lvl, _, is_char, _, _)| is_char && lvl == 4);
                let mut kept_count = 0usize;
                let mut kept_finisher = false;

                for &(cid, lvl, name, is_char, is_search_ev, is_lv5_interact) in &hand_cards {
                    let is_dup = !name.is_empty() && seen_names.contains(name) && !is_search_ev;
                    let keep = if is_dup {
                        false
                    } else if is_char && lvl <= 2 {
                        true
                    } else if is_search_ev {
                        true
                    } else if is_char && lvl == 4 {
                        true
                    } else if !is_first && is_char && lvl == 3 {
                        !has_lv2
                    } else if !is_first && is_lv5_interact {
                        has_lv2 || has_lv4
                    } else if !is_first && has_lv2 && has_lv4 && is_char && lvl >= 8 && !kept_finisher {
                        kept_finisher = true;
                        true
                    } else if is_first && has_lv2 && is_char && lvl == 3 && !has_lv4 {
                        true
                    } else {
                        false
                    };

                    if keep && kept_count < 3 {
                        kept_count += 1;
                        if !name.is_empty() {
                            seen_names.insert(name.to_string());
                        }
                    } else {
                        return_ids.push(json!(cid));
                    }
                }

                json!({"value": {"cardIds": return_ids}})
            }
            "mainAction" => {
                let empty_vec = vec![];
                let actions = opts.get("actions").and_then(|v| v.as_array()).unwrap_or(&empty_vec);
                let find_act = |t: &str| {
                    actions
                        .iter()
                        .find(|a| a.get("type").and_then(|v| v.as_str()) == Some(t))
                };

                // 1. 勝てるなら即座に事件解決！
                if find_act("RESOLVE_CASE").is_some() {
                    return json!({"value": {"type": "RESOLVE_CASE"}});
                }

                let eff_file = my_ps.effective_file_count();
                let phase = self.compute_game_phase(engine, &opp_id);
                let lethal_mode = self.can_reach_lethal_this_turn(engine, &opp_id);
                let profile = self.analyze_opponent_profile(engine, &opp_id);
                let board_eval = self.evaluate_board_state(engine, &opp_id);
                let desperate_mode = board_eval < -2500.0;
                let ahead_safe_mode = board_eval > 3000.0;

                // 2. 情報獲得・除去・バフ系の高優先宣言を先に切る
                if let Some(act) = find_act("DECLARE") {
                    if let Some(actors) = act.get("actors").and_then(|v| v.as_array()) {
                        for actor in actors {
                            if let Some(aid) = actor.get("actorId").and_then(|v| v.as_str()) {
                                if self.is_high_priority_declare(engine, aid)
                                    && self.is_declare_beneficial(engine, aid, &opp_id)
                                {
                                    return json!({"value": {"type": "DECLARE", "actorId": aid}});
                                }
                            }
                        }
                    }
                }

                // 3. ターン開始時の ASSIST 先行化 (リーサルターンは温存)
                if find_act("ASSIST").is_some() && self.should_assist_before_hand_play_strongest(engine, &opp_id) {
                    return json!({"value": {"type": "ASSIST"}});
                }

                // 4. 手札登場・イベント使用 (同一CardDefIdの重複評価排除 + 局面適応スコアリング)
                if let Some(act) = find_act("HAND_PLAY") {
                    if let Some(card_ids) = act.get("cardIds").and_then(|v| v.as_array()) {
                        let mut best_cid: Option<&str> = None;
                        let mut best_score = i32::MIN;
                        let opp_field_count = opp_ps.map(|o| o.field.len()).unwrap_or(0);
                        let my_field_count = my_ps.field.len();
                        let can_assist_later = find_act("ASSIST").is_some() && !lethal_mode;
                        let mut evaluated_defs: HashSet<&str> = HashSet::new();

                        for cid_val in card_ids {
                            if let Some(cid) = cid_val.as_str() {
                                if let Some(c) = my_ps.hand.iter().find(|h| h.instance_id == cid) {
                                    if !evaluated_defs.insert(c.definition_id.as_str()) {
                                        continue;
                                    }
                                    if let Some(def) = engine.catalog.get(&c.definition_id) {
                                        let name = def.names.first().map(|s| s.as_str()).unwrap_or("");
                                        let desc = &def.description;

                                        let mut score_f = (def.level as f64) * self.weights.level_weight
                                            + (def.ap as f64) * self.weights.ap_weight
                                            + (def.lp as f64) * self.weights.lp_weight
                                            + (def.ap as f64) * (def.lp as f64) * self.weights.ap_lp_cross_weight.eval(phase);

                                        if lethal_mode {
                                            if Self::has_swift(def) {
                                                score_f += 5500.0;
                                            }
                                            if Self::has_rush(def) {
                                                score_f += 4200.0;
                                            }
                                            if Self::gains_evidence_directly(def) > 0 {
                                                score_f += 4500.0;
                                            }
                                        } else if Self::has_rush(def) || Self::has_swift(def) {
                                            if opp_field_count > 0 || phase >= 0.6 {
                                                score_f += self.weights.rush_bonus;
                                            } else {
                                                score_f -= 600.0;
                                            }
                                        }

                                        if desc.contains("【登場時】") {
                                            score_f += self.weights.on_play_bonus;
                                        }
                                        if Self::is_info_gain_card(def) {
                                            score_f += 850.0;
                                        }
                                        if def.level >= 9 {
                                            score_f += 2200.0;
                                        }

                                        for req_f in [5usize, 6, 7] {
                                            let tag1 = format!("【FILE{}】", req_f);
                                            let tag2 = format!("【FILE {}】", req_f);
                                            if eff_file < req_f && (desc.contains(&tag1) || desc.contains(&tag2)) {
                                                score_f -= if can_assist_later { 9000.0 } else { 4500.0 };
                                            }
                                        }

                                        if desc.contains("自分の現場に登場したとき") {
                                            score_f += 2500.0;
                                        }

                                        if name == "ラム" && desc.contains("リムーブエリア") {
                                            let has_good_target = my_ps.remove.iter().any(|rc| {
                                                engine
                                                    .catalog
                                                    .get(&rc.definition_id)
                                                    .map(|d| d.card_type == CardType::CHARACTER && d.level >= 4)
                                                    .unwrap_or(false)
                                            });
                                            if has_good_target && my_field_count < 4 {
                                                score_f += 2200.0;
                                            } else {
                                                score_f -= 3500.0;
                                            }
                                        }

                                        if def.card_type == CardType::EVENT {
                                            score_f += self.weights.event_penalty;

                                            if name.contains("烏丸蓮耶の影") || name.contains("小さくなった名探偵") {
                                                score_f += 3800.0;
                                            } else if name == "少年探偵団" {
                                                if my_ps.hand.len() >= 2 && my_field_count < 4 {
                                                    score_f += 3200.0;
                                                } else {
                                                    score_f -= 5000.0;
                                                }
                                            } else if name.contains("五人囃子") || desc.contains("リムーブエリアから") {
                                                let has_revivable = my_ps.remove.iter().any(|rc| {
                                                    engine
                                                        .catalog
                                                        .get(&rc.definition_id)
                                                        .map(|d| d.card_type == CardType::CHARACTER && d.level <= 4)
                                                        .unwrap_or(false)
                                                });
                                                if has_revivable && my_field_count < 4 {
                                                    score_f += 2200.0;
                                                } else {
                                                    score_f -= 9000.0;
                                                }
                                            } else if name == "カッ" || desc.contains("すべてをリムーブ") {
                                                if opp_field_count >= 2 && opp_field_count > my_field_count {
                                                    score_f += 3500.0;
                                                } else {
                                                    score_f -= 7000.0;
                                                }
                                            } else if desc.contains("リムーブ") || desc.contains("スリープ") || desc.contains("スタン") {
                                                if opp_field_count == 0 {
                                                    score_f -= 7000.0;
                                                } else {
                                                    score_f += self.weights.removal_bonus
                                                        + if desperate_mode { 600.0 } else { 0.0 };
                                                }
                                            }
                                        }

                                        let score = score_f as i32;
                                        if score > best_score {
                                            best_score = score;
                                            best_cid = Some(cid);
                                        }
                                    }
                                }
                            }
                        }
                        if let Some(cid) = best_cid {
                            if best_score > -1000 {
                                return json!({"value": {"type": "HAND_PLAY", "cardId": cid}});
                            }
                        }
                    }
                }

                // 5. 残りの有効な宣言効果 (DECLARE)
                if let Some(act) = find_act("DECLARE") {
                    if let Some(actors) = act.get("actors").and_then(|v| v.as_array()) {
                        for actor in actors {
                            if let Some(aid) = actor.get("actorId").and_then(|v| v.as_str()) {
                                if self.is_declare_beneficial(engine, aid, &opp_id) {
                                    return json!({"value": {"type": "DECLARE", "actorId": aid}});
                                }
                            }
                        }
                    }
                }

                // 6. 変装 (DISGUISE)
                if let Some(act) = find_act("DISGUISE") {
                    if let Some(ans) = self.choose_best_disguise(engine, my_ps, act) {
                        return ans;
                    }
                }

                // 7. 攻撃 (ACTION) vs 推理 (DEDUCE) の統合ルックアヘッド評価
                let mut best_combat_or_deduce: Option<(i32, Value)> = None;

                if let Some(act) = find_act("ACTION") {
                    let actor_ids = act.get("actorIds").and_then(|v| v.as_array());
                    let target_ids = act.get("targetIds").and_then(|v| v.as_array());
                    let actor_target_map = act.get("actorTargetMap").and_then(|v| v.as_object());

                    if let (Some(aids), Some(opp)) = (actor_ids, opp_ps) {
                        let max_opp_active_blocker_ap = opp
                            .field
                            .iter()
                            .filter(|c| c.orientation == CardOrientation::ACTIVE)
                            .map(|c| engine.get_effective_ap(&opp_id, c))
                            .max()
                            .unwrap_or(0);

                        let no_ci_obs = self.opp_no_cutin_observations.load(Ordering::Relaxed);
                        let ci_used = self.opp_cutin_used_count.load(Ordering::Relaxed);
                        let cutin_margin = if ci_used > no_ci_obs {
                            2000
                        } else if no_ci_obs >= 2 && ci_used == 0 {
                            0
                        } else {
                            1000
                        };

                        for aid_v in aids {
                            let Some(aid) = aid_v.as_str() else { continue };
                            let Some(my_card) = my_ps.field.iter().find(|c| c.instance_id == aid) else { continue };
                            let my_ap = engine.get_effective_ap(&self.player_id, my_card);
                            let my_lp = engine.get_effective_lp(&self.player_id, my_card);
                            let can_deduce = find_act("DEDUCE")
                                .and_then(|d| d.get("actorIds"))
                                .and_then(|v| v.as_array())
                                .map(|arr| arr.iter().any(|x| x.as_str() == Some(aid)))
                                .unwrap_or(false);

                            let sleep_risk = self.estimate_sleep_punish_risk(engine, &opp_id, my_ap, my_lp)
                                + if ahead_safe_mode { 400 } else { 0 };
                            let preserve_blocker = self.should_preserve_active_blocker(engine, &opp_id, aid);

                            let valid_tids = actor_target_map
                                .and_then(|m| m.get(aid))
                                .and_then(|v| v.as_array())
                                .or(target_ids);

                            if let Some(tids) = valid_tids {
                                for tid_v in tids {
                                    let Some(tid) = tid_v.as_str() else { continue };
                                    if let Some(opp_c) = opp.field.iter().find(|c| c.instance_id == tid) {
                                        let opp_ap = engine.get_effective_ap(&opp_id, opp_c);
                                        let opp_lp = engine.get_effective_lp(&opp_id, opp_c);
                                        let opp_is_mislead = engine
                                            .catalog
                                            .get(&opp_c.definition_id)
                                            .map(Self::has_mislead)
                                            .unwrap_or(false);

                                        if my_ap >= opp_ap {
                                            let base_rem = (self.weights.attack_remove_weight * 3.5) as i32;
                                            let lp_threat = opp_lp * 1350;
                                            let mislead_bonus = if opp_is_mislead { 2200 } else { 0 };
                                            let disguise_order_bonus = if profile.disguise_risk && my_ap >= 7000 { 900 } else { 0 };
                                            let safe_ci_bonus = if my_ap >= opp_ap + cutin_margin { 400 } else { 0 };
                                            let blocker_pen = if preserve_blocker && opp_lp <= 1 { 2600 } else { 0 };
                                            let comeback_boost = if desperate_mode { 600 } else { 0 };

                                            let score = base_rem
                                                + opp_ap
                                                + lp_threat
                                                + mislead_bonus
                                                + disguise_order_bonus
                                                + safe_ci_bonus
                                                + comeback_boost
                                                - (my_ap - opp_ap) / 4
                                                - sleep_risk / 2
                                                - blocker_pen;

                                            if score > 0
                                                && best_combat_or_deduce
                                                    .as_ref()
                                                    .map(|(bs, _)| score > *bs)
                                                    .unwrap_or(true)
                                            {
                                                best_combat_or_deduce = Some((
                                                    score,
                                                    json!({"value": {"type": "ACTION", "actorId": aid, "targetId": tid}}),
                                                ));
                                            }
                                        }
                                    } else if opp.r#case.as_ref().map(|c| c.instance_id == tid).unwrap_or(false)
                                        && !opp.evidence.is_empty()
                                    {
                                        if max_opp_active_blocker_ap > my_ap && can_deduce {
                                            continue;
                                        }
                                        if preserve_blocker && !lethal_mode {
                                            continue;
                                        }

                                        let urgency = if opp.case_phase == CasePhase::RESOLUTION { 2100 } else { 0 };
                                        let case_bonus = self.weights.case_attack_bonus as i32;
                                        let lp_penalty = if can_deduce {
                                            (my_lp as f64 * self.weights.deduce_lp_weight * 0.65) as i32
                                        } else {
                                            0
                                        };
                                        let low_ap_first = if profile.disguise_risk && my_ap <= 4000 && max_opp_active_blocker_ap <= my_ap {
                                            600
                                        } else {
                                            0
                                        };
                                        let lethal_boost = if lethal_mode { 2500 } else { 0 };
                                        let score = 3900 + case_bonus + urgency + low_ap_first + lethal_boost
                                            - lp_penalty
                                            - sleep_risk;

                                        if score > 0
                                            && best_combat_or_deduce
                                                .as_ref()
                                                .map(|(bs, _)| score > *bs)
                                                .unwrap_or(true)
                                        {
                                            best_combat_or_deduce = Some((
                                                score,
                                                json!({"value": {"type": "ACTION", "actorId": aid, "targetId": tid}}),
                                            ));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // 7b. 推理候補の評価
                if let Some(ded_act) = find_act("DEDUCE") {
                    if let Some(actor_ids) = ded_act.get("actorIds").and_then(|v| v.as_array()) {
                        let opp_active_mislead = opp_ps
                            .map(|o| {
                                o.field
                                    .iter()
                                    .filter(|c| {
                                        c.orientation == CardOrientation::ACTIVE
                                            && engine
                                                .catalog
                                                .get(&c.definition_id)
                                                .map(Self::has_mislead)
                                                .unwrap_or(false)
                                    })
                                    .count() as i32
                            })
                            .unwrap_or(0);

                        for aid_v in actor_ids {
                            let Some(aid) = aid_v.as_str() else { continue };
                            let is_partner = my_ps
                                .partner
                                .as_ref()
                                .map(|p| p.instance_id == aid)
                                .unwrap_or(false);

                            if is_partner {
                                if lethal_mode {
                                    continue;
                                }
                                if my_ps.case_phase != CasePhase::RESOLUTION && eff_file == 6 {
                                    continue;
                                }
                                if profile.avoid_early_p_deduce
                                    && my_ps.case_phase != CasePhase::RESOLUTION
                                    && my_ps.evidence.is_empty()
                                {
                                    continue;
                                }
                                let lp = my_ps
                                    .partner
                                    .as_ref()
                                    .and_then(|p| engine.catalog.get(&p.definition_id))
                                    .map(|d| d.lp)
                                    .unwrap_or(1);
                                let score = 3400 + lp * (self.weights.deduce_lp_weight as i32);
                                if best_combat_or_deduce
                                    .as_ref()
                                    .map(|(bs, _)| score > *bs)
                                    .unwrap_or(true)
                                {
                                    best_combat_or_deduce =
                                        Some((score, json!({"value": {"type": "DEDUCE", "actorId": aid}})));
                                }
                            } else if let Some(fc) = my_ps.field.iter().find(|c| c.instance_id == aid) {
                                if self.should_preserve_active_blocker(engine, &opp_id, aid) {
                                    continue;
                                }
                                let lp = engine.get_effective_lp(&self.player_id, fc);
                                let ap = engine.get_effective_ap(&self.player_id, fc);
                                let eff_lp = (lp - opp_active_mislead).max(0);
                                if eff_lp == 0 && !lethal_mode {
                                    continue;
                                }
                                let sleep_risk = self.estimate_sleep_punish_risk(engine, &opp_id, ap, lp)
                                    + if ahead_safe_mode { 400 } else { 0 };
                                let lethal_boost = if lethal_mode { 3500 } else { 0 };
                                let score = 2600
                                    + eff_lp * (self.weights.deduce_lp_weight as i32)
                                    + lethal_boost
                                    - sleep_risk
                                    - (ap / 10);

                                if score > 500
                                    && best_combat_or_deduce
                                        .as_ref()
                                        .map(|(bs, _)| score > *bs)
                                        .unwrap_or(true)
                                {
                                    best_combat_or_deduce =
                                        Some((score, json!({"value": {"type": "DEDUCE", "actorId": aid}})));
                                }
                            }
                        }
                    }
                }

                if let Some((_, best_val)) = best_combat_or_deduce {
                    return best_val;
                }

                // 8. 解決編(FILE 7)到達、または「アシスト挟みネクストヒント」(FILE 8→9→NHでLv8連打)
                if find_act("ASSIST").is_some() && !lethal_mode {
                    if my_ps.case_phase != CasePhase::RESOLUTION && eff_file == 6 {
                        return json!({"value": {"type": "ASSIST"}});
                    }
                    if my_ps.normal_hand_played_this_turn
                        && my_ps.case_phase == CasePhase::RESOLUTION
                        && eff_file >= 7
                        && my_ps.field.len() < 5
                    {
                        let boosted_after_nh = eff_file as i32;
                        let has_big_play = my_ps.hand.iter().any(|c| {
                            engine
                                .catalog
                                .get(&c.definition_id)
                                .map(|d| d.card_type == CardType::CHARACTER && d.level == boosted_after_nh && d.level >= 7)
                                .unwrap_or(false)
                        });
                        if has_big_play {
                            return json!({"value": {"type": "ASSIST"}});
                        }
                    }
                }

                // 9. ネクストヒント (HINT_TAKE) の厳密判定 (第1部 1.3節)
                if find_act("HINT_TAKE").is_some() && self.should_use_next_hint(engine, &opp_id) {
                    let used = self.hints_used_this_turn.load(Ordering::Relaxed);
                    self.hints_used_this_turn.store(used + 1, Ordering::Relaxed);
                    return json!({"value": {"type": "HINT_TAKE"}});
                }

                json!({"value": {"type": "END_TURN"}})
            }
            "hintPlay" => {
                let eff_file = my_ps.effective_file_count();
                let lethal_mode = self.can_reach_lethal_this_turn(engine, &opp_id);
                if let Some(playable) = opts.get("playableCardIds").and_then(|v| v.as_array()) {
                    let mut best_cid: Option<&str> = None;
                    let mut best_score = i32::MIN;
                    for cid_v in playable {
                        if let Some(cid) = cid_v.as_str() {
                            if let Some(c) = my_ps.hand.iter().find(|h| h.instance_id == cid) {
                                if let Some(def) = engine.catalog.get(&c.definition_id) {
                                    if def.card_type == CardType::EVENT {
                                        let name = def.names.first().map(|s| s.as_str()).unwrap_or("");
                                        if name.contains("五人囃子") && my_ps.remove.len() < 2 {
                                            continue;
                                        }
                                    }
                                    let mut score = def.level * 1000 + def.ap + def.lp * 450;
                                    if Self::has_swift(def) {
                                        score += if lethal_mode { 5000 } else { 1800 };
                                    }
                                    if Self::has_rush(def) {
                                        score += if lethal_mode { 4000 } else { 1500 };
                                    }
                                    if eff_file < 5
                                        && (def.description.contains("【FILE 5】") || def.description.contains("【FILE5】"))
                                    {
                                        score -= 3500;
                                    }
                                    if score > best_score {
                                        best_score = score;
                                        best_cid = Some(cid);
                                    }
                                }
                            }
                        }
                    }
                    if let Some(cid) = best_cid {
                        self.last_hint_skipped.store(false, Ordering::Relaxed);
                        return json!({"value": {"cardId": cid}});
                    }
                }
                self.last_hint_skipped.store(true, Ordering::Relaxed);
                json!({"value": {"cardId": null}})
            }
            "guard" => {
                // 第1部 1.4節・1.5節: 優先権往復・最小コストカットイン・定石ガード判定
                let (attacker_ap, defender_ap, defender_lv, is_case_attack, i_am_attacker, opp_used_cutin_now) = engine
                    .action_context
                    .as_ref()
                    .map(|ctx| {
                        let att_pid = ctx.get("attackerPlayerId").and_then(|v| v.as_str()).unwrap_or("");
                        let def_pid = ctx.get("defenderPlayerId").and_then(|v| v.as_str()).unwrap_or("");
                        let actor_id = ctx.get("actorId").and_then(|v| v.as_str()).unwrap_or("");
                        let target_id = ctx.get("targetId").and_then(|v| v.as_str()).unwrap_or("");

                        let att_ap = engine
                            .players
                            .get(att_pid)
                            .and_then(|ps| ps.field.iter().find(|c| c.instance_id == actor_id))
                            .map(|c| engine.get_effective_ap_with_contact(att_pid, c, true))
                            .unwrap_or(3000);

                        let def_card = engine
                            .players
                            .get(def_pid)
                            .and_then(|ps| ps.field.iter().find(|c| c.instance_id == target_id));
                        let def_ap = def_card
                            .map(|c| engine.get_effective_ap_with_contact(def_pid, c, true))
                            .unwrap_or(0);
                        let def_lv = def_card
                            .and_then(|c| engine.catalog.get(&c.definition_id))
                            .map(|d| d.level)
                            .unwrap_or(0);

                        let is_case = my_ps.r#case.as_ref().map(|c| c.instance_id == target_id).unwrap_or(false);
                        let i_am_att = att_pid == self.player_id;
                        let att_ci = ctx.get("attackerUsedCutin").and_then(|v| v.as_bool()).unwrap_or(false);
                        let def_ci = ctx.get("defenderUsedCutin").and_then(|v| v.as_bool()).unwrap_or(false);
                        let opp_ci = (!i_am_att && att_ci) || (i_am_att && def_ci);
                        (att_ap, def_ap, def_lv, is_case, i_am_att, opp_ci)
                    })
                    .unwrap_or((3000, 0, 0, false, false, false));

                if opp_used_cutin_now {
                    self.opp_cutin_used_count.fetch_add(1, Ordering::Relaxed);
                }

                let opp_lethal_danger = self.estimate_opponent_lethal_excess(engine, &opp_id) >= 0;

                // 1. 変装（AP逆転または大幅強化時のみ）
                if let (Some(disguises), Some(target_id)) = (
                    opts.get("disguiseOptions").and_then(|v| v.as_array()),
                    opts.get("disguiseTargetId").and_then(|v| v.as_str()),
                ) {
                    if let Some(cur_card) = my_ps.field.iter().find(|c| c.instance_id == target_id) {
                        let cur_ap = engine.get_effective_ap_with_contact(&self.player_id, cur_card, true);
                        for d_val in disguises {
                            if let (Some(did), Some(def_id)) = (
                                d_val.get("instanceId").and_then(|v| v.as_str()),
                                d_val.get("definitionId").and_then(|v| v.as_str()),
                            ) {
                                let new_ap = engine.catalog.get(def_id).map(|d| d.ap).unwrap_or(0);
                                let flips_combat = if i_am_attacker {
                                    cur_ap < defender_ap && new_ap >= defender_ap
                                } else {
                                    cur_ap <= attacker_ap && new_ap > attacker_ap
                                };
                                if flips_combat || new_ap - cur_ap >= self.weights.disguise_ap_threshold {
                                    return json!({"value": {"disguiseCardId": did, "targetId": target_id}});
                                }
                            }
                        }
                    }
                }

                // 2. カットイン (第1部 1.5節: 勝っている側からは切らない＋勝敗が覆る最小ボーナスを選択)
                if let Some(cutins) = opts.get("cutinCardIds").and_then(|v| v.as_array()) {
                    if !cutins.is_empty() {
                        let currently_losing = if i_am_attacker {
                            defender_ap > 0 && attacker_ap < defender_ap
                        } else {
                            !is_case_attack && defender_ap > 0 && attacker_ap >= defender_ap
                        };

                        if currently_losing {
                            let required_bonus = if i_am_attacker {
                                defender_ap - attacker_ap
                            } else {
                                (attacker_ap - defender_ap) + 1
                            };

                            let worth_defending = i_am_attacker
                                || defender_lv >= 5
                                || defender_ap >= 5000
                                || opp_lethal_danger;

                            if worth_defending {
                                let mut best_ci: Option<(&str, i32)> = None;
                                for ci_v in cutins {
                                    let ci_id = ci_v
                                        .get("instanceId")
                                        .and_then(|s| s.as_str())
                                        .or_else(|| ci_v.as_str());
                                    if let Some(cid) = ci_id {
                                        let bonus = my_ps
                                            .hand
                                            .iter()
                                            .find(|c| c.instance_id == cid)
                                            .and_then(|c| engine.catalog.get(&c.definition_id))
                                            .map(Self::parse_cutin_bonus)
                                            .unwrap_or(2000);
                                        if bonus >= required_bonus
                                            && best_ci.map(|(_, b)| bonus < b).unwrap_or(true)
                                        {
                                            best_ci = Some((cid, bonus));
                                        }
                                    }
                                }
                                if let Some((cid, _)) = best_ci {
                                    return json!({"value": {"cutinCardId": cid}});
                                }
                            }
                        } else if !i_am_attacker && !opp_used_cutin_now {
                            self.opp_no_cutin_observations.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }

                // 3. ガード (第1部 1.4節: 4条件＋mustGuard強制)
                if let Some(guards) = opts.get("guardCardIds").and_then(|v| v.as_array()) {
                    let mut best_win_guard: Option<(&str, i32)> = None;
                    let mut best_trade_guard: Option<(&str, i32)> = None;
                    let mut weakest_guard: Option<(&str, i32)> = None;

                    for g_val in guards {
                        if let Some(gid) = g_val.as_str() {
                            if let Some(gc) = my_ps.field.iter().find(|c| c.instance_id == gid) {
                                let g_ap = engine.get_effective_ap_with_contact(&self.player_id, gc, true);
                                let g_lp = engine.get_effective_lp(&self.player_id, gc);
                                if g_ap > attacker_ap {
                                    if best_win_guard.map(|(_, b_ap)| g_ap < b_ap).unwrap_or(true) {
                                        best_win_guard = Some((gid, g_ap));
                                    }
                                } else if g_ap == attacker_ap && g_lp <= 1 {
                                    if best_trade_guard.map(|(_, t_ap)| g_ap < t_ap).unwrap_or(true) {
                                        best_trade_guard = Some((gid, g_ap));
                                    }
                                }
                                if weakest_guard.map(|(_, w_ap)| g_ap < w_ap).unwrap_or(true) {
                                    weakest_guard = Some((gid, g_ap));
                                }
                            }
                        }
                    }

                    if let Some((gid, _)) = best_win_guard {
                        return json!({"value": {"guardCardId": gid}});
                    }
                    if !is_case_attack && (defender_lv >= 5 || defender_ap >= 5000) && attacker_ap >= defender_ap {
                        if let Some((gid, w_ap)) = weakest_guard {
                            if w_ap <= 3000 {
                                return json!({"value": {"guardCardId": gid}});
                            }
                        }
                    }
                    if let Some((gid, _)) = best_trade_guard {
                        if attacker_ap >= 5000 || opp_lethal_danger {
                            return json!({"value": {"guardCardId": gid}});
                        }
                    }
                    let opp_near_lethal = opp_ps
                        .map(|o| {
                            o.case_phase == CasePhase::RESOLUTION
                                && o.evidence.len() + 1 >= self.required_evidence_for(engine, &opp_id)
                        })
                        .unwrap_or(false);
                    if is_case_attack && (opp_near_lethal || opp_lethal_danger) {
                        if let Some((gid, _)) = weakest_guard {
                            return json!({"value": {"guardCardId": gid}});
                        }
                    }
                    if opts.get("mustGuard").and_then(|v| v.as_bool()).unwrap_or(false) {
                        if let Some((gid, _)) = weakest_guard {
                            return json!({"value": {"guardCardId": gid}});
                        }
                    }
                }
                json!({"value": {"guardCardId": null}})
            }
            "targetSelect" => self.choose_target_select_with_engine(engine, my_ps, opp_ps, &opp_id, &opts),
            "deckSearch" => self.choose_deck_search_with_engine(my_ps, &opts),
            "modeSelect" => {
                let modes = opts.get("modes").and_then(|v| v.as_array());
                let modes_len = modes.map(|a| a.len()).unwrap_or(0);
                if modes_len == 0 {
                    return json!({"value": {"mode": 0, "modeIndex": 0, "modes": []}});
                }
                let min_pick = opts.get("minPick").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
                let max_pick = opts.get("maxPick").and_then(|v| v.as_u64()).unwrap_or(1).max(1) as usize;
                let pick_count = min_pick.max(1).min(max_pick).min(modes_len);

                let actor_id = opts.get("actorId").and_then(|v| v.as_str()).unwrap_or("");
                let actor_card = my_ps.field.iter().find(|c| c.instance_id == actor_id);
                let actor_sleeping = actor_card
                    .map(|c| c.orientation != CardOrientation::ACTIVE)
                    .unwrap_or(false);
                let actor_can_use_rush = actor_card
                    .map(|c| !c.has_rush && c.entered_turn == engine.turn)
                    .unwrap_or(false);
                let opp_field_len = opp_ps.map(|o| o.field.len()).unwrap_or(0);
                let lethal_mode = self.can_reach_lethal_this_turn(engine, &opp_id);

                let mut scored_modes: Vec<(usize, i32)> = modes
                    .unwrap()
                    .iter()
                    .enumerate()
                    .map(|(idx, v)| {
                        let txt = v.as_str().unwrap_or("");
                        let mut score = 0i32;
                        if txt.contains("証拠を") {
                            score += if lethal_mode { 6000 } else { 3500 };
                        }
                        if txt.contains("カードを引") || txt.contains("手札に加え") {
                            score += 2800;
                        }
                        if txt.contains("リムーブ") || txt.contains("デッキの下") {
                            score += if opp_field_len > 0 { 4000 } else { -3000 };
                        }
                        if txt.contains("スタン") {
                            score += if opp_field_len > 0 { 3200 } else { -3000 };
                        }
                        if txt.contains("スリープ") {
                            score += if opp_field_len > 0 { 2500 } else { -3000 };
                        }
                        if txt.contains("アクティブに") {
                            score += if actor_sleeping { 4000 } else { 800 };
                        }
                        if txt.contains("突撃") || txt.contains("迅速") {
                            score += if actor_can_use_rush { 3800 } else { 600 };
                        }
                        if txt.contains("AP＋") || txt.contains("AP+") {
                            score += 1200;
                        }
                        (idx, score)
                    })
                    .collect();

                scored_modes.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
                let chosen: Vec<usize> = scored_modes.into_iter().take(pick_count).map(|(i, _)| i).collect();
                let first_idx = chosen.first().copied().unwrap_or(0);
                json!({
                    "value": {
                        "mode": first_idx,
                        "modeIndex": first_idx,
                        "modes": chosen
                    }
                })
            }
            _ => self.choose_action(decision),
        }
    }

    pub fn choose_action(&self, decision: &Value) -> Value {
        let kind = decision.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        let opts = decision.get("options").cloned().unwrap_or(json!({}));
        let mut rng_guard = self.rng.lock().unwrap_or_else(|e| e.into_inner());
        let mut rng: &mut StdRng = &mut rng_guard;

        match kind {
            "mulligan" => {
                let hand = opts.get("handCardIds").and_then(|v| v.as_array());
                if let Some(cards) = hand {
                    if !cards.is_empty() && rng.gen_bool(0.3) {
                        let chosen = cards.choose(&mut rng).unwrap();
                        return json!({"value": {"cardIds": [chosen]}});
                    }
                }
                json!({"value": {"cardIds": []}})
            }
            "mainAction" => {
                let empty_vec = vec![];
                let actions = opts.get("actions").and_then(|v| v.as_array()).unwrap_or(&empty_vec);
                let types: Vec<&str> = actions
                    .iter()
                    .filter_map(|a| a.get("type").and_then(|t| t.as_str()))
                    .collect();

                if types.contains(&"RESOLVE_CASE") {
                    return json!({"value": {"type": "RESOLVE_CASE"}});
                }
                if types.contains(&"HAND_PLAY") {
                    if let Some(act) = actions.iter().find(|a| a.get("type").and_then(|t| t.as_str()) == Some("HAND_PLAY")) {
                        if let Some(card_ids) = act.get("cardIds").and_then(|v| v.as_array()) {
                            if let Some(cid) = card_ids.choose(&mut rng) {
                                return json!({"value": {"type": "HAND_PLAY", "cardId": cid}});
                            }
                        }
                    }
                }
                if types.contains(&"DEDUCE") {
                    if let Some(act) = actions.iter().find(|a| a.get("type").and_then(|t| t.as_str()) == Some("DEDUCE")) {
                        if let Some(actor_ids) = act.get("actorIds").and_then(|v| v.as_array()) {
                            if let Some(aid) = actor_ids.choose(&mut rng) {
                                return json!({"value": {"type": "DEDUCE", "actorId": aid}});
                            }
                        }
                    }
                }
                if types.contains(&"ACTION") {
                    if let Some(act) = actions.iter().find(|a| a.get("type").and_then(|t| t.as_str()) == Some("ACTION")) {
                        let actor_ids = act.get("actorIds").and_then(|v| v.as_array());
                        let target_ids = act.get("targetIds").and_then(|v| v.as_array());
                        let actor_target_map = act.get("actorTargetMap").and_then(|v| v.as_object());
                        if let Some(aids) = actor_ids {
                            if let Some(aid_val) = aids.choose(&mut rng) {
                                let aid_str = aid_val.as_str().unwrap_or("");
                                let valid_tids = actor_target_map
                                    .and_then(|m| m.get(aid_str))
                                    .and_then(|v| v.as_array())
                                    .or(target_ids);
                                if let Some(tids) = valid_tids {
                                    if let Some(tid) = tids.choose(&mut rng) {
                                        return json!({"value": {"type": "ACTION", "actorId": aid_val, "targetId": tid}});
                                    }
                                }
                            }
                        }
                    }
                }
                if types.contains(&"DECLARE") && rng.gen_bool(0.7) {
                    if let Some(act) = actions.iter().find(|a| a.get("type").and_then(|t| t.as_str()) == Some("DECLARE")) {
                        if let Some(actors) = act.get("actors").and_then(|v| v.as_array()) {
                            if let Some(actor) = actors.choose(&mut rng) {
                                if let Some(aid) = actor.get("actorId").and_then(|v| v.as_str()) {
                                    return json!({"value": {"type": "DECLARE", "actorId": aid}});
                                }
                            }
                        }
                    }
                }
                if types.contains(&"DISGUISE") && rng.gen_bool(0.5) {
                    if let Some(act) = actions.iter().find(|a| a.get("type").and_then(|t| t.as_str()) == Some("DISGUISE")) {
                        let card_ids = act.get("cardIds").and_then(|v| v.as_array());
                        let target_ids = act.get("targetIds").and_then(|v| v.as_array());
                        if let (Some(cids), Some(tids)) = (card_ids, target_ids) {
                            if let (Some(cid), Some(tid)) = (cids.choose(&mut rng), tids.choose(&mut rng)) {
                                return json!({"value": {"type": "DISGUISE", "cardId": cid, "targetId": tid}});
                            }
                        }
                    }
                }
                if types.contains(&"ASSIST") && rng.gen_bool(0.5) {
                    return json!({"value": {"type": "ASSIST"}});
                }
                if types.contains(&"HINT_TAKE") && rng.gen_bool(0.3) {
                    return json!({"value": {"type": "HINT_TAKE"}});
                }

                json!({"value": {"type": "END_TURN"}})
            }
            "hintPlay" => {
                if let Some(playable) = opts.get("playableCardIds").and_then(|v| v.as_array()) {
                    if let Some(cid) = playable.choose(&mut rng) {
                        return json!({"value": {"cardId": cid}});
                    }
                }
                json!({"value": {"cardId": null}})
            }
            "guard" => {
                let must_guard = opts.get("mustGuard").and_then(|v| v.as_bool()).unwrap_or(false);
                if let Some(guards) = opts.get("guardCardIds").and_then(|v| v.as_array()) {
                    if !guards.is_empty() && (must_guard || rng.gen_bool(0.5)) {
                        let chosen = guards.choose(&mut rng).unwrap();
                        return json!({"value": {"guardCardId": chosen}});
                    }
                }
                json!({"value": {"guardCardId": null}})
            }
            "targetSelect" => {
                let actor_id = opts.get("actorId").and_then(|v| v.as_str()).unwrap_or("");
                let effect_type = opts.get("effectType").and_then(|v| v.as_str()).unwrap_or("");
                let max_select = opts.get("maxSelect").and_then(|v| v.as_u64()).unwrap_or(1).max(1) as usize;
                let distinct_names = opts.get("distinctNames").and_then(|v| v.as_bool()).unwrap_or(false);
                let distinct_levels = opts.get("distinctLevels").and_then(|v| v.as_bool()).unwrap_or(false);
                let distinct_colors = opts.get("distinctColors").and_then(|v| v.as_bool()).unwrap_or(false);
                let max_total_level = opts.get("maxTotalLevel").and_then(|v| v.as_i64());

                if let Some(candidates) = opts.get("candidates").and_then(|v| v.as_array()) {
                    let mut shuffled: Vec<&Value> = candidates.iter().collect();
                    shuffled.shuffle(&mut rng);

                    let mut chosen_ids: Vec<&str> = Vec::new();
                    let mut used_names = HashSet::new();
                    let mut used_levels = HashSet::new();
                    let mut used_colors = HashSet::new();
                    let mut sum_level: i64 = 0;

                    for c in shuffled {
                        if chosen_ids.len() >= max_select {
                            break;
                        }
                        let Some(iid) = c.get("instanceId").and_then(|v| v.as_str()) else {
                            continue;
                        };
                        let clv = c.get("level").and_then(|v| v.as_i64()).unwrap_or(0);
                        let ccols: Vec<&str> = c
                            .get("colors")
                            .and_then(|v| v.as_array())
                            .map(|arr| arr.iter().filter_map(|x| x.as_str()).collect())
                            .unwrap_or_default();

                        let cnames = Self::candidate_distinct_names(c);
                        if distinct_names && !cnames.is_empty() && cnames.iter().any(|n| used_names.contains(n)) {
                            continue;
                        }
                        if distinct_levels && used_levels.contains(&clv) {
                            continue;
                        }
                        if distinct_colors && ccols.iter().any(|col| used_colors.contains(*col)) {
                            continue;
                        }
                        if let Some(cap) = max_total_level {
                            if sum_level + clv > cap {
                                continue;
                            }
                        }

                        if distinct_names {
                            for n in cnames {
                                used_names.insert(n);
                            }
                        }
                        if distinct_levels {
                            used_levels.insert(clv);
                        }
                        if distinct_colors {
                            for col in ccols {
                                used_colors.insert(col.to_string());
                            }
                        }
                        sum_level += clv;
                        chosen_ids.push(iid);
                    }
                    if let Some(&first_id) = chosen_ids.first() {
                        return json!({
                            "value": {
                                "targetId": first_id,
                                "targetIds": chosen_ids,
                                "actorId": actor_id,
                                "effectType": effect_type
                            }
                        });
                    }
                }
                json!({
                    "value": {
                        "targetId": null,
                        "targetIds": [],
                        "actorId": actor_id,
                        "effectType": effect_type
                    }
                })
            }
            "deckSearch" => {
                let actor_id = opts.get("actorId").and_then(|v| v.as_str()).unwrap_or("");
                let max_select = opts.get("maxSelect").and_then(|v| v.as_u64()).unwrap_or(1).max(1) as usize;
                let distinct_colors = opts.get("distinctColors").and_then(|v| v.as_bool()).unwrap_or(false);
                if let Some(cards) = opts.get("revealedCards").and_then(|v| v.as_array()) {
                    let mut selectable: Vec<&Value> = cards
                        .iter()
                        .filter(|c| c.get("selectable").and_then(|v| v.as_bool()).unwrap_or(true))
                        .collect();
                    selectable.shuffle(&mut rng);
                    let mut chosen_ids: Vec<&str> = Vec::new();
                    let mut used_colors = HashSet::new();
                    for c in selectable {
                        if chosen_ids.len() >= max_select {
                            break;
                        }
                        let Some(cid) = c.get("instanceId").and_then(|v| v.as_str()) else {
                            continue;
                        };
                        let ccols: Vec<&str> = c
                            .get("colors")
                            .and_then(|v| v.as_array())
                            .map(|arr| arr.iter().filter_map(|x| x.as_str()).collect())
                            .unwrap_or_default();
                        if distinct_colors && ccols.iter().any(|col| used_colors.contains(*col)) {
                            continue;
                        }
                        if distinct_colors {
                            for col in ccols {
                                used_colors.insert(col.to_string());
                            }
                        }
                        chosen_ids.push(cid);
                    }
                    if let Some(&first_id) = chosen_ids.first() {
                        return json!({
                            "value": {
                                "chosenCardId": first_id,
                                "chosenCardIds": chosen_ids,
                                "actorId": actor_id
                            }
                        });
                    }
                }
                json!({
                    "value": {
                        "chosenCardId": null,
                        "chosenCardIds": [],
                        "actorId": actor_id
                    }
                })
            }
            "triggerOrder" => {
                json!({
                    "value": { "index": 0 }
                })
            }
            "modeSelect" => {
                let modes_len = opts.get("modes").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
                let min_pick = opts.get("minPick").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
                let max_pick = opts.get("maxPick").and_then(|v| v.as_u64()).unwrap_or(1).max(1) as usize;
                let pick_count = min_pick.max(1).min(max_pick).min(modes_len);
                let mut indices: Vec<usize> = (0..modes_len).collect();
                indices.shuffle(&mut rng);
                let chosen: Vec<usize> = indices.into_iter().take(pick_count).collect();
                let first_idx = chosen.first().copied().unwrap_or(0);
                json!({
                    "value": {
                        "mode": first_idx,
                        "modeIndex": first_idx,
                        "modes": chosen
                    }
                })
            }
            _ => json!({"value": {}}),
        }
    }
}

/// 第2部 2.5節: Paired Trinomial モデルに基づく GSPRT (Generalized SPRT) 統計器
#[derive(Debug, Default)]
struct PairedGsprt {
    ll: usize,
    wl: usize,
    ww: usize,
    first_wins: usize,
    first_games: usize,
    second_wins: usize,
    second_games: usize,
}

impl PairedGsprt {
    fn record_pair(&mut self, game1_win: bool, game2_win: bool) {
        self.first_games += 1;
        if game1_win {
            self.first_wins += 1;
        }
        self.second_games += 1;
        if game2_win {
            self.second_wins += 1;
        }
        match (game1_win, game2_win) {
            (true, true) => self.ww += 1,
            (false, false) => self.ll += 1,
            _ => self.wl += 1,
        }
    }

    fn total_pairs(&self) -> usize {
        self.ll + self.wl + self.ww
    }

    /// Stockfish Fishtest の正規化Eloベース LLR (対数尤度比) を計算
    fn compute_llr(&self, elo0: f64, elo1: f64) -> f64 {
        let p = self.total_pairs() as f64;
        if p < 3.0 {
            return 0.0;
        }
        let n_games = 2.0 * p;
        let mean = (self.ww as f64 + 0.5 * self.wl as f64) / p;
        let var_pair = ((self.ll as f64) * (0.0 - mean).powi(2)
            + (self.wl as f64) * (0.5 - mean).powi(2)
            + (self.ww as f64) * (1.0 - mean).powi(2))
            / (p - 1.0);
        let sigma_pg = (2.0 * var_pair).sqrt().max(1e-4);

        let c_et = 800.0 / 10.0_f64.ln(); // ≈ 347.43
        let t_hat = (mean - 0.5) / sigma_pg;
        let t0 = elo0 / c_et;
        let t1 = elo1 / c_et;

        let raw_llr = 0.5 * n_games * ((1.0 + (t_hat - t0).powi(2)) / (1.0 + (t_hat - t1).powi(2))).ln();
        let bound = 0.5 * (t1 - t0).abs() * n_games;
        raw_llr.clamp(-bound, bound)
    }
}

/// 2つの重みセットで1試合シミュレーションし、(勝者, p1証拠 - p2証拠, 勝者の場・リムーブに出たカード定義ID, 敗者の終了時手札に残ったカード定義ID) を返す
fn simulate_match(
    catalog: &CardCatalog,
    p1_deck: &DeckDefinition,
    p2_deck: &DeckDefinition,
    w1: &CpuWeights,
    w2: &CpuWeights,
    seed: u64,
) -> (Option<String>, i32, Vec<String>, Vec<String>) {
    let mut engine = MatchEngine::new_with_seed(catalog.clone(), seed);
    engine.add_player("p1", p1_deck);
    engine.add_player("p2", p2_deck);
    engine.start_match();

    let a1 = RandomAgent::with_seed("p1", w1.clone(), seed ^ 0x9E37_79B9_7F4A_7C15);
    let a2 = RandomAgent::with_seed("p2", w2.clone(), seed ^ 0xBF58_476D_1CE4_E5B9);

    for _ in 0..800 {
        if engine.winner_id.is_some() {
            break;
        }
        let Some(dec) = engine.pending_decision.clone() else {
            break;
        };
        let pid = dec.get("playerId").and_then(|v| v.as_str()).unwrap_or("");
        let ans = if pid == "p1" {
            a1.choose_action_with_engine(&engine, &dec)
        } else if pid == "p2" {
            a2.choose_action_with_engine(&engine, &dec)
        } else {
            break;
        };
        if engine.submit_decision(pid, &ans).is_err() {
            break;
        }
    }

    let p1_ev = engine.players.get("p1").map(|p| p.evidence.len() as i32).unwrap_or(0);
    let p2_ev = engine.players.get("p2").map(|p| p.evidence.len() as i32).unwrap_or(0);

    let mut winner_active_cards = Vec::new();
    let mut loser_stranded_hand = Vec::new();
    if let Some(ref w_id) = engine.winner_id {
        let l_id = if w_id == "p1" { "p2" } else { "p1" };
        if let Some(w_ps) = engine.players.get(w_id) {
            for c in w_ps.field.iter().chain(w_ps.remove.iter()) {
                winner_active_cards.push(c.definition_id.clone());
            }
        }
        if let Some(l_ps) = engine.players.get(l_id) {
            for c in &l_ps.hand {
                loser_stranded_hand.push(c.definition_id.clone());
            }
        }
    }

    (engine.winner_id, p1_ev - p2_ev, winner_active_cards, loser_stranded_hand)
}

/// 固定シード・シミュレーションの集計サマリ
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SimulationSummary {
    pub games: usize,
    pub seed: u64,
    pub p1_wins: usize,
    pub p2_wins: usize,
    pub draws: usize,
    /// 1試合平均の (p1証拠 - p2証拠)
    pub avg_evidence_diff: f64,
}

/// 固定シードで N 試合同士打戦をシミュレーションし、勝敗を集計する。
/// 各試合のシードは base seed から決定的に導出するため、
/// 同一 (catalog, decks, weights, games, seed) なら結果が完全に再現される。
pub fn run_fixed_seed_simulation(
    catalog: &CardCatalog,
    p1_deck: &DeckDefinition,
    p2_deck: &DeckDefinition,
    w1: &CpuWeights,
    w2: &CpuWeights,
    games: usize,
    seed: u64,
) -> SimulationSummary {
    let mut p1_wins = 0usize;
    let mut p2_wins = 0usize;
    let mut draws = 0usize;
    let mut ev_diff_sum: i64 = 0;

    for i in 0..games {
        let game_seed = seed ^ ((i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let (winner, diff, _, _) = simulate_match(catalog, p1_deck, p2_deck, w1, w2, game_seed);
        match winner.as_deref() {
            Some("p1") => p1_wins += 1,
            Some("p2") => p2_wins += 1,
            _ => draws += 1,
        }
        ev_diff_sum += diff as i64;
    }

    SimulationSummary {
        games,
        seed,
        p1_wins,
        p2_wins,
        draws,
        avg_evidence_diff: ev_diff_sum as f64 / games.max(1) as f64,
    }
}

/// 第2部 2.4・2.5節: Rademacher SPSA + 先後入替ペア対戦 + Paired Trinomial GSPRT による自己対戦学習
pub fn train_cpu_self_play(
    catalog: &CardCatalog,
    p1_deck: &DeckDefinition,
    p2_deck: &DeckDefinition,
    generations: usize,
    games_per_gen: usize,
    save_path: &str,
) -> CpuWeights {
    let mut best = CpuWeights::load_or_default(save_path);
    let mut rng = rand::thread_rng();
    let scales = CpuWeights::spsa_scales();

    let mut win_card_counts: HashMap<String, usize> = HashMap::new();
    let mut stranded_hand_counts: HashMap<String, usize> = HashMap::new();

    let pairs_per_gen = (games_per_gen / 2).max(1);

    println!(
        "🧠 CPU自己対戦学習 (SPSA + 先後入替ペア対戦 + GSPRT) を開始します (世代数: {}, 1世代あたりペア数: {}, 保存先: {})",
        generations, pairs_per_gen, save_path
    );

    for gen in 1..=generations {
        // SPSA: 減衰ステップ幅 c_n, 学習率 r_n
        let n_f = gen as f64;
        let c_decay = 1.0 / n_f.powf(0.101);
        let r_n = 0.25 / (n_f + 4.0).powf(0.602);

        let base_theta = best.to_vec();
        // Rademacher 分布 (+1.0 / -1.0 を確率0.5で生成)
        let delta: Vec<f64> = (0..base_theta.len())
            .map(|_| if rng.gen_bool(0.5) { 1.0 } else { -1.0 })
            .collect();

        let theta_plus: Vec<f64> = base_theta
            .iter()
            .enumerate()
            .map(|(i, &v)| v + c_decay * scales[i] * delta[i])
            .collect();
        let theta_minus: Vec<f64> = base_theta
            .iter()
            .enumerate()
            .map(|(i, &v)| v - c_decay * scales[i] * delta[i])
            .collect();

        let w_plus = best.from_perturbed(&theta_plus);
        let w_minus = best.from_perturbed(&theta_minus);

        let mut gsprt = PairedGsprt::default();
        let mut plus_wins = 0usize;
        let mut minus_wins = 0usize;
        let mut ev_diff_sum = 0i32;

        for _ in 0..pairs_per_gen {
            // Game 1: w_plus(先攻 p1) vs w_minus(後攻 p2)
            let seed1 = rng.gen::<u64>();
            let (w1, diff1, wc1, lh1) = simulate_match(catalog, p1_deck, p2_deck, &w_plus, &w_minus, seed1);
            // Game 2: w_minus(先攻 p1) vs w_plus(後攻 p2)
            let seed2 = rng.gen::<u64>();
            let (w2, diff2, wc2, lh2) = simulate_match(catalog, p1_deck, p2_deck, &w_minus, &w_plus, seed2);

            for cid in wc1.into_iter().chain(wc2.into_iter()) {
                *win_card_counts.entry(cid).or_insert(0) += 1;
            }
            for cid in lh1.into_iter().chain(lh2.into_iter()) {
                *stranded_hand_counts.entry(cid).or_insert(0) += 1;
            }

            let g1_plus_win = w1.as_deref() == Some("p1");
            let g2_plus_win = w2.as_deref() == Some("p2");

            if g1_plus_win {
                plus_wins += 1;
            } else if w1.as_deref() == Some("p2") {
                minus_wins += 1;
            }
            if g2_plus_win {
                plus_wins += 1;
            } else if w2.as_deref() == Some("p1") {
                minus_wins += 1;
            }

            ev_diff_sum += diff1 - diff2;
            gsprt.record_pair(g1_plus_win, g2_plus_win);

            // GSPRT 早期終了判定 (α = β = 0.05 -> ±2.944)
            let llr = gsprt.compute_llr(0.0, 25.0);
            if gsprt.total_pairs() >= 6 && (llr >= 2.944 || llr <= -2.944) {
                break;
            }
        }

        let win_diff = (plus_wins as f64) - (minus_wins as f64);
        let llr = gsprt.compute_llr(0.0, 25.0);
        let first_wr = if gsprt.first_games > 0 {
            100.0 * gsprt.first_wins as f64 / gsprt.first_games as f64
        } else {
            0.0
        };
        let second_wr = if gsprt.second_games > 0 {
            100.0 * gsprt.second_wins as f64 / gsprt.second_games as f64
        } else {
            0.0
        };

        if win_diff != 0.0 {
            // SPSA 勾配ステップ更新: θ_{n+1, i} = θ_{n, i} + r_n * c_i * (wins - losses) * Δ_{n, i}
            let norm_diff = win_diff / (gsprt.total_pairs().max(1) as f64);
            let next_theta: Vec<f64> = base_theta
                .iter()
                .enumerate()
                .map(|(i, &v)| v + r_n * scales[i] * norm_diff * delta[i])
                .collect();
            best = best.from_perturbed(&next_theta);
            let _ = best.save(save_path);
            println!(
                "  [世代 {}/{}] ✅ SPSA更新 (θ+ {}勝 vs θ- {}勝 | ペア WW:{} WL:{} LL:{} | 先攻勝率:{:.1}% 後攻勝率:{:.1}% | LLR:{:+.2} 証拠差:{:+})",
                gen, generations, plus_wins, minus_wins, gsprt.ww, gsprt.wl, gsprt.ll, first_wr, second_wr, llr, ev_diff_sum
            );
        } else {
            println!(
                "  [世代 {}/{}] ── 据置 (θ+ {}勝 vs θ- {}勝 | ペア WW:{} WL:{} LL:{} | LLR:{:+.2} 証拠差:{:+})",
                gen, generations, plus_wins, minus_wins, gsprt.ww, gsprt.wl, gsprt.ll, llr, ev_diff_sum
            );
        }
    }

    let mut top_win: Vec<_> = win_card_counts.into_iter().collect();
    top_win.sort_by(|a, b| b.1.cmp(&a.1));
    println!("\n📊 【構築分析】勝利時に場・リムーブで活躍したカード TOP5:");
    for (cid, cnt) in top_win.into_iter().take(5) {
        let name = catalog
            .get(&cid)
            .and_then(|d| d.names.first())
            .map(|s| s.as_str())
            .unwrap_or(&cid);
        println!("   - {} ({}): {}回", name, cid, cnt);
    }

    let mut top_stranded: Vec<_> = stranded_hand_counts.into_iter().collect();
    top_stranded.sort_by(|a, b| b.1.cmp(&a.1));
    println!("⚠️ 【構築分析】敗北時に手札で腐っていた（使えなかった）カード TOP5:");
    for (cid, cnt) in top_stranded.into_iter().take(5) {
        let name = catalog
            .get(&cid)
            .and_then(|d| d.names.first())
            .map(|s| s.as_str())
            .unwrap_or(&cid);
        println!("   - {} ({}): {}回", name, cid, cnt);
    }

    recommend_tech_cards(catalog, p1_deck, p2_deck);

    best
}

/// 環境デッキ（P2）の傾向を分析し、P1デッキ外の有効な対策カード（テックカード）を推薦する
fn recommend_tech_cards(
    catalog: &CardCatalog,
    p1_deck: &DeckDefinition,
    p2_deck: &DeckDefinition,
) {
    let p1_colors: HashSet<String> = catalog
        .get(&p1_deck.partner_id)
        .map(|d| d.colors.iter().cloned().collect())
        .unwrap_or_default();

    let mut p1_canonical_ids: HashSet<String> = HashSet::new();
    for item in &p1_deck.main_deck {
        if let Some(def) = catalog.get(&item.definition_id) {
            p1_canonical_ids.insert(def.canonical_id.clone());
        } else {
            p1_canonical_ids.insert(item.definition_id.clone());
        }
    }

    let mut p2_high_lv = 0usize;
    let mut p2_rush = 0usize;
    let mut p2_removal = 0usize;
    let mut p2_events = 0usize;
    for item in &p2_deck.main_deck {
        if let Some(def) = catalog.get(&item.definition_id) {
            let cnt = item.count.max(0) as usize;
            if def.level >= 6 {
                p2_high_lv += cnt;
            }
            if def.description.contains("突撃") {
                p2_rush += cnt;
            }
            if def.description.contains("リムーブ") || def.description.contains("スタン") || def.description.contains("手札に戻す") {
                p2_removal += cnt;
            }
            if def.card_type == CardType::EVENT {
                p2_events += cnt;
            }
        }
    }

    let mut seen_canonical: HashSet<String> = HashSet::new();
    let mut candidates: Vec<(i32, String, String, String, Vec<String>)> = Vec::new();

    for (id, def) in &catalog.definitions {
        if def.card_type != CardType::CHARACTER && def.card_type != CardType::EVENT {
            continue;
        }
        if p1_canonical_ids.contains(&def.canonical_id) || !seen_canonical.insert(def.canonical_id.clone()) {
            continue;
        }
        let color_ok = p1_colors.is_empty()
            || def.colors.is_empty()
            || def.colors.iter().any(|c| p1_colors.contains(c));
        if !color_ok {
            continue;
        }

        let mut score: i32 = 0;
        let mut reasons: Vec<String> = Vec::new();
        let desc = &def.description;
        let lv = def.level;
        let ap = def.ap;
        let lp = def.lp;

        if desc.contains("リムーブ") || desc.contains("デッキの下") {
            let bonus = if p2_high_lv >= 8 { 45 } else { 30 };
            score += bonus;
            reasons.push("高Lv・脅威キャラの除去".to_string());
        } else if desc.contains("スタン") || desc.contains("手札に戻す") {
            score += 25;
            reasons.push("盤面テンポ阻害（スタン/バウンス）".to_string());
        }

        if desc.contains("突撃") || desc.contains("迅速") {
            let bonus = if p2_removal >= 6 { 35 } else { 25 };
            score += bonus;
            reasons.push("突撃・迅速による即時打点・証拠奪取".to_string());
        }

        if desc.contains("カードを引く") || desc.contains("1枚引く") || desc.contains("2枚引く") || desc.contains("手札に加える") {
            score += 25;
            reasons.push("手札補充・安定化".to_string());
        }

        if desc.contains("カットイン") || desc.contains("アタックできない") {
            let bonus = if p2_rush >= 4 { 35 } else { 20 };
            score += bonus;
            reasons.push("相手の速攻・アタック対策".to_string());
        }

        if def.card_type == CardType::CHARACTER {
            if lv <= 4 && lp >= 2 {
                score += 20;
                reasons.push(format!("軽量高LP(Lv{}/LP{})", lv, lp));
            }
            if lv >= 5 && ap >= 6000 {
                score += 15;
            }
        }

        if score >= 35 && !reasons.is_empty() {
            let name = def.names.first().map(|s| s.as_str()).unwrap_or(id).to_string();
            let lv_str = if lv > 0 { format!("Lv{}", lv) } else { "Event".to_string() };
            candidates.push((score, name, id.clone(), lv_str, reasons));
        }
    }

    candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.2.cmp(&b.2)));

    println!("\n💡 【環境対策】相手デッキ（{}）に有効そうなデッキ外カード推薦 TOP5:", p2_deck.deck_name.as_deref().unwrap_or("名称未設定"));
    println!(
        "   (相手デッキ傾向: Lv6以上={}枚, 突撃={}枚, 除去={}枚, イベント={}枚)",
        p2_high_lv, p2_rush, p2_removal, p2_events
    );
    for (score, name, id, lv_str, reasons) in candidates.into_iter().take(5) {
        println!("   - {} ({} / {}) [評価:{}]: {}", name, id, lv_str, score, reasons.join("・"));
    }
}
