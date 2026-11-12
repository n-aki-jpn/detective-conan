import json
import unittest
from pathlib import Path
from conan_engine.catalog import CardCatalog
from conan_engine.engine import MatchEngine
from conan_engine.models import MatchPhase, Zone, CardOrientation, CasePhase

class TestConanEngine(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        catalog_path = Path("data/dummy/cards.json")
        cls.catalog = CardCatalog.load_from_json(catalog_path)
        with open("data/dummy/deck.json", "r", encoding="utf-8") as f:
            cls.deck_def = json.load(f)

    def test_mulligan_and_first_turn_resources(self):
        # T001, T002, T003
        engine = MatchEngine(self.catalog, seed=123)
        engine.add_player("player1", self.deck_def)
        engine.add_player("player2", self.deck_def)
        engine.start_match()

        first_player = engine.pending_decision["playerId"]
        self.assertEqual(engine.pending_decision["kind"], "mulligan")
        
        # Player 1 mulligans 1 card
        p1_hand_card = engine.pending_decision["options"]["handCardIds"][0]
        engine.submit_decision(first_player, {"value": {"cardIds": [p1_hand_card]}})

        # Player 2 passes mulligan
        second_player = engine.pending_decision["playerId"]
        self.assertEqual(engine.pending_decision["kind"], "mulligan")
        engine.submit_decision(second_player, {"value": {"cardIds": []}})

        # Now Turn 1 for First Player
        self.assertEqual(engine.turn, 1)
        self.assertEqual(engine.turn_player_id, first_player)
        p1 = engine.players[first_player]
        # First turn: drew 1 (5 - 1 + 1 = 5? wait, start: 5 cards, mulligan 1 replaced -> 5 cards, auto draw 1 -> 6 cards)
        self.assertEqual(len(p1.hand), 6)
        # First turn, first player: 1 file
        self.assertEqual(len(p1.file), 1)

    def test_assist_to_resolution_phase(self):
        # T013
        engine = MatchEngine(self.catalog, seed=42)
        engine.add_player("p1", self.deck_def)
        engine.add_player("p2", self.deck_def)
        engine.start_match()
        engine.submit_decision(engine.pending_decision["playerId"], {"value": {"cardIds": []}})
        engine.submit_decision(engine.pending_decision["playerId"], {"value": {"cardIds": []}})

        p1 = engine.players[engine.turn_player_id]
        # Add cards to file to make it 6 cards
        while len(p1.file) < 6:
            c = p1.deck.pop(0)
            c.zone = Zone.FILE
            p1.file.append(c)

        # Do ASSIST
        engine.submit_decision(p1.player_id, {"value": {"type": "ASSIST"}})
        self.assertEqual(p1.case_phase, CasePhase.RESOLUTION)
        self.assertEqual(p1.effective_file_count(), 7)

    def test_deduce_gains_evidence(self):
        engine = MatchEngine(self.catalog, seed=42)
        engine.add_player("p1", self.deck_def)
        engine.add_player("p2", self.deck_def)
        engine.start_match()
        engine.submit_decision(engine.pending_decision["playerId"], {"value": {"cardIds": []}})
        engine.submit_decision(engine.pending_decision["playerId"], {"value": {"cardIds": []}})

        p1 = engine.players[engine.turn_player_id]
        initial_ev = len(p1.evidence)
        # Partner lp=1, can deduce
        engine.submit_decision(p1.player_id, {"value": {"type": "DEDUCE", "actorId": p1.partner.instanceId}})
        self.assertEqual(len(p1.evidence), initial_ev + 1)
        self.assertEqual(p1.partner.orientation, CardOrientation.SLEEP)

if __name__ == "__main__":
    unittest.main()
