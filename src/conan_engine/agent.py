import random
from typing import Dict, Any

class RandomAgent:
    def __init__(self, player_id: str, seed: int = 42):
        self.player_id = player_id
        self.rng = random.Random(seed)

    def choose_action(self, decision: Dict[str, Any]) -> Dict[str, Any]:
        kind = decision["kind"]
        opts = decision.get("options", {})

        if kind == "mulligan":
            # 50% chance to mulligan 0 or 1 card
            hand = opts.get("handCardIds", [])
            if hand and self.rng.random() < 0.3:
                chosen = self.rng.sample(hand, 1)
                return {"value": {"cardIds": chosen}}
            return {"value": {"cardIds": []}}

        elif kind == "mainAction":
            actions = opts.get("actions", [])
            # Priority: RESOLVE_CASE > HAND_PLAY > DEDUCE > ACTION > ASSIST > HINT_TAKE > END_TURN
            types = [a["type"] for a in actions]
            if "RESOLVE_CASE" in types:
                return {"value": {"type": "RESOLVE_CASE"}}
            if "HAND_PLAY" in types:
                act = next(a for a in actions if a["type"] == "HAND_PLAY")
                card_id = self.rng.choice(act["cardIds"])
                return {"value": {"type": "HAND_PLAY", "cardId": card_id}}
            if "DEDUCE" in types:
                act = next(a for a in actions if a["type"] == "DEDUCE")
                actor_id = self.rng.choice(act["actorIds"])
                return {"value": {"type": "DEDUCE", "actorId": actor_id}}
            if "ACTION" in types:
                act = next(a for a in actions if a["type"] == "ACTION")
                actor_id = self.rng.choice(act["actorIds"])
                target_id = self.rng.choice(act["targetIds"])
                return {"value": {"type": "ACTION", "actorId": actor_id, "targetId": target_id}}
            if "ASSIST" in types and self.rng.random() < 0.5:
                return {"value": {"type": "ASSIST"}}
            if "HINT_TAKE" in types and self.rng.random() < 0.3:
                return {"value": {"type": "HINT_TAKE"}}
            
            return {"value": {"type": "END_TURN"}}

        elif kind == "hintPlay":
            playable = opts.get("playableCardIds", [])
            if playable:
                return {"value": {"cardId": self.rng.choice(playable)}}
            return {"value": {"cardId": None}}

        elif kind == "guard":
            guards = opts.get("guardCardIds", [])
            if guards and self.rng.random() < 0.5:
                return {"value": {"guardCardId": self.rng.choice(guards)}}
            return {"value": {"guardCardId": None}}

        return {"value": {}}
