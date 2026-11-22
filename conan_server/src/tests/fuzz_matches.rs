use super::*;
use crate::agent::{CpuDifficulty, RandomAgent};
use crate::models::{CardType, DeckCardItem, DeckDefinition, MatchPhase};
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::SeedableRng;
use std::collections::HashMap;

fn build_random_deck(catalog: &CardCatalog, rng: &mut StdRng) -> DeckDefinition {
    let mut cases: Vec<_> = catalog
        .definitions
        .values()
        .filter(|d| d.card_type == CardType::CASE && !d.colors.is_empty())
        .collect();
    cases.sort_by(|a, b| a.definition_id.cmp(&b.definition_id));
    let case_def = cases.choose(rng).expect("case card must exist");

    let mut partners: Vec<_> = catalog
        .definitions
        .values()
        .filter(|d| {
            d.card_type == CardType::PARTNER
                && d.colors.iter().any(|c| case_def.colors.contains(c))
        })
        .collect();
    if partners.is_empty() {
        partners = catalog
            .definitions
            .values()
            .filter(|d| d.card_type == CardType::PARTNER)
            .collect();
    }
    partners.sort_by(|a, b| a.definition_id.cmp(&b.definition_id));
    let partner_def = partners.choose(rng).expect("partner card must exist");

    let mut pool: Vec<_> = catalog
        .definitions
        .values()
        .filter(|d| {
            (d.card_type == CardType::CHARACTER || d.card_type == CardType::EVENT)
                && (d.colors.is_empty() || d.colors.iter().any(|c| case_def.colors.contains(c)))
        })
        .collect();
    pool.sort_by(|a, b| a.definition_id.cmp(&b.definition_id));
    pool.shuffle(rng);

    let mut counts: HashMap<String, i32> = HashMap::new();
    let mut main_deck: Vec<DeckCardItem> = Vec::new();
    let mut total = 0;

    for def in pool.iter().cycle() {
        if total >= 40 {
            break;
        }
        let key = if def.canonical_id.is_empty() {
            def.definition_id.clone()
        } else {
            def.canonical_id.clone()
        };
        let cur = counts.get(&key).copied().unwrap_or(0);
        if cur >= 3 {
            continue;
        }
        counts.insert(key, cur + 1);
        if let Some(existing) = main_deck.iter_mut().find(|x| x.definition_id == def.definition_id) {
            existing.count += 1;
        } else {
            main_deck.push(DeckCardItem {
                definition_id: def.definition_id.clone(),
                count: 1,
            });
        }
        total += 1;
    }

    DeckDefinition {
        deck_id: None,
        deck_name: Some("fuzz".to_string()),
        catalog_version: None,
        partner_id: partner_def.definition_id.clone(),
        case_id: case_def.definition_id.clone(),
        main_deck,
    }
}

fn recent_logs(engine: &MatchEngine) -> Vec<String> {
    let len = engine.logs.len();
    engine.logs[len.saturating_sub(20)..].to_vec()
}

fn run_fuzz_matches(num_matches: usize) {
    let catalog = load_test_catalog();

    for match_idx in 0..num_matches {
        let seed = 0xC09A_0000_0000_0000u64 + match_idx as u64;
        let mut rng = StdRng::seed_from_u64(seed);
        let d1 = build_random_deck(&catalog, &mut rng);
        let d2 = build_random_deck(&catalog, &mut rng);

        let mut engine = MatchEngine::new(catalog.clone());
        engine.add_player("p1", &d1);
        engine.add_player("p2", &d2);
        engine.start_match();

        let a1 = RandomAgent::with_difficulty("p1", CpuDifficulty::Hard);
        let a2 = RandomAgent::with_difficulty("p2", CpuDifficulty::Normal);

        let mut last_state_key = (-1i32, -1i32);
        let mut same_state_streak = 0usize;

        for step in 0..300 {
            if engine.phase == MatchPhase::FINISHED || engine.winner_id.is_some() {
                break;
            }

            let dec = engine.pending_decision.clone().unwrap_or_else(|| {
                panic!(
                    "Fuzz invariant failed (pending_decision is None while not FINISHED): match={} seed=0x{:x} step={} turn={} phase={:?}\nlogs:\n{}",
                    match_idx,
                    seed,
                    step,
                    engine.turn,
                    engine.phase,
                    recent_logs(&engine).join("\n")
                );
            });

            let state_key = (engine.turn, engine.revision);
            if state_key == last_state_key {
                same_state_streak += 1;
                assert!(
                    same_state_streak < 5,
                    "Fuzz invariant failed (infinite loop on same turn/revision): match={} seed=0x{:x} step={} turn={} rev={}\ndec={}\nlogs:\n{}",
                    match_idx,
                    seed,
                    step,
                    engine.turn,
                    engine.revision,
                    dec,
                    recent_logs(&engine).join("\n")
                );
            } else {
                last_state_key = state_key;
                same_state_streak = 1;
            }

            if dec.get("kind").and_then(|v| v.as_str()) == Some("mainAction") {
                for pid in ["p1", "p2"] {
                    if let Some(ps) = engine.players.get(pid) {
                        assert!(
                            ps.field.len() <= 5,
                            "Fuzz invariant failed (field.len() > 5 at mainAction): match={} seed=0x{:x} step={} turn={} pid={} len={}\nlogs:\n{}",
                            match_idx,
                            seed,
                            step,
                            engine.turn,
                            pid,
                            ps.field.len(),
                            recent_logs(&engine).join("\n")
                        );
                    }
                }
            }

            let pid = dec
                .get("playerId")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let ans = if pid == "p1" {
                a1.choose_action_with_engine(&engine, &dec)
            } else if pid == "p2" {
                a2.choose_action_with_engine(&engine, &dec)
            } else {
                panic!(
                    "Fuzz invariant failed (unknown playerId '{}'): match={} seed=0x{:x} turn={}\nlogs:\n{}",
                    pid,
                    match_idx,
                    seed,
                    engine.turn,
                    recent_logs(&engine).join("\n")
                );
            };

            let res = engine.submit_decision(&pid, &ans);
            assert!(
                res.is_ok(),
                "Fuzz invariant failed (submit_decision error {:?}): match={} seed=0x{:x} step={} turn={} pid={}\ndec={}\nans={}\nlogs:\n{}",
                res.err(),
                match_idx,
                seed,
                step,
                engine.turn,
                pid,
                dec,
                ans,
                recent_logs(&engine).join("\n")
            );
        }
    }
}

#[test]
fn fuzz_100_cpu_matches() {
    run_fuzz_matches(100);
}

#[test]
#[ignore]
fn fuzz_1000_cpu_matches() {
    run_fuzz_matches(1000);
}
