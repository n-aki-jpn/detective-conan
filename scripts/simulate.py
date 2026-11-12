import argparse
import json
import sys
from pathlib import Path

# Add src to sys.path
sys.path.insert(0, str(Path(__file__).parent.parent / "src"))

from conan_engine.catalog import CardCatalog
from conan_engine.engine import MatchEngine
from conan_engine.agent import RandomAgent
from conan_engine.models import MatchPhase

def run_simulation(games: int = 10, max_turns: int = 100, seed: int = 42):
    catalog_path = Path("data/dummy/cards.json")
    deck_path = Path("data/dummy/deck.json")
    
    catalog = CardCatalog.load_from_json(catalog_path)
    with open(deck_path, "r", encoding="utf-8") as f:
        deck_def = json.load(f)

    stats = {
        "total": games,
        "completed": 0,
        "truncated": 0,
        "wins": {"playerA": 0, "playerB": 0},
        "reasons": {}
    }

    print(f"=== Starting Conan Card Game Simulation ({games} games) ===")
    for g in range(games):
        game_seed = seed + g
        engine = MatchEngine(catalog, seed=game_seed)
        engine.add_player("playerA", deck_def)
        engine.add_player("playerB", deck_def)

        agent_a = RandomAgent("playerA", seed=game_seed * 2 + 1)
        agent_b = RandomAgent("playerB", seed=game_seed * 2 + 2)
        agents = {"playerA": agent_a, "playerB": agent_b}

        engine.start_match()

        while engine.phase != MatchPhase.FINISHED and engine.turn <= max_turns:
            dec = engine.pending_decision
            if not dec:
                break
            agent = agents[dec["playerId"]]
            ans = agent.choose_action(dec)
            engine.submit_decision(dec["playerId"], ans)

        if engine.phase == MatchPhase.FINISHED:
            stats["completed"] += 1
            stats["wins"][engine.winner_id] = stats["wins"].get(engine.winner_id, 0) + 1
            stats["reasons"][engine.finish_reason] = stats["reasons"].get(engine.finish_reason, 0) + 1
        else:
            stats["truncated"] += 1

    print("\n=== Simulation Results ===")
    print(f"Games played: {stats['total']}")
    print(f"Completed: {stats['completed']} ({(stats['completed']/stats['total'])*100:.1f}%)")
    print(f"Truncated (max turns {max_turns}): {stats['truncated']}")
    print(f"Wins: Player A: {stats['wins']['playerA']}, Player B: {stats['wins']['playerB']}")
    print(f"Reasons: {stats['reasons']}")

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Run headless simulation of Conan Card Game")
    parser.add_argument("--games", type=int, default=10, help="Number of games to simulate")
    parser.add_argument("--seed", type=int, default=42, help="Base random seed")
    parser.add_argument("--max-turns", type=int, default=100, help="Max turns before truncation")
    args = parser.parse_args()

    run_simulation(games=args.games, max_turns=args.max_turns, seed=args.seed)
