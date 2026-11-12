from typing import Dict, Any, List
from .models import CardOrientation, Zone, MatchPhase
from .engine import MatchEngine, PlayerState

def build_player_observation(engine: MatchEngine, viewing_player_id: str) -> Dict[str, Any]:
    catalog = engine.catalog
    
    def serialize_card(c, is_secret: bool):
        if is_secret:
            return {
                "instanceId": c.instanceId,
                "faceUp": False,
                "orientation": c.orientation.value
            }
        c_def = catalog.get(c.definitionId)
        return {
            "instanceId": c.instanceId,
            "definitionId": c.definitionId,
            "name": c_def.names[0] if c_def.names else c.definitionId,
            "description": c_def.description,
            "colors": c_def.colors,
            "level": c_def.level,
            "ap": c_def.ap,
            "lp": c_def.lp,
            "cardType": c_def.cardType.value,
            "orientation": c.orientation.value,
            "enteredTurn": c.enteredTurn,
            "faceUp": True,
            "mainThumb": c_def.mainThumb,
            "mainPath": c_def.mainPath
        }

    def serialize_player(ps: PlayerState, is_me: bool):
        return {
            "playerId": ps.player_id,
            "isFirstPlayer": ps.is_first_player,
            "casePhase": ps.case_phase.value,
            "fileCount": ps.effective_file_count(),
            "deckCount": len(ps.deck),
            "evidenceCount": len(ps.evidence),
            "removeCount": len(ps.remove),
            "handCount": len(ps.hand),
            "hand": [serialize_card(c, is_secret=(not is_me)) for c in ps.hand],
            "field": [serialize_card(c, is_secret=False) for c in ps.field],
            "partner": serialize_card(ps.partner, is_secret=False) if ps.partner else None,
            "case": serialize_card(ps.case, is_secret=False) if ps.case else None,
            "remove": [serialize_card(c, is_secret=False) for c in ps.remove],
            "normalHandPlayed": ps.normal_hand_played_this_turn,
            "hintPlayed": ps.hint_played_this_turn
        }

    me_ps = engine.players[viewing_player_id]
    opp_pid = [pid for pid in engine.player_order if pid != viewing_player_id][0]
    opp_ps = engine.players[opp_pid]

    pending_dec = None
    if engine.pending_decision and engine.pending_decision["playerId"] == viewing_player_id:
        pending_dec = engine.pending_decision

    return {
        "matchId": "match_live",
        "revision": engine.revision,
        "phase": engine.phase.value,
        "turn": engine.turn,
        "turnPlayerId": engine.turn_player_id,
        "decisionPlayerId": engine.pending_decision["playerId"] if engine.pending_decision else None,
        "viewingPlayerId": viewing_player_id,
        "me": serialize_player(me_ps, is_me=True),
        "opponent": serialize_player(opp_ps, is_me=False),
        "pendingDecision": pending_dec,
        "winnerId": engine.winner_id,
        "finishReason": engine.finish_reason
    }
