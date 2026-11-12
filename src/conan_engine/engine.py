import random
from typing import List, Dict, Optional, Any
from .models import (
    CardInstance, CardDefinition, CardOrientation, Zone,
    MatchPhase, CasePhase, MoveCause
)
from .catalog import CardCatalog

class PlayerState:
    def __init__(self, player_id: str):
        self.player_id = player_id
        self.deck: List[CardInstance] = []
        self.hand: List[CardInstance] = []
        self.field: List[CardInstance] = []
        self.partner: Optional[CardInstance] = None
        self.case: Optional[CardInstance] = None
        self.file: List[CardInstance] = []
        self.evidence: List[CardInstance] = []
        self.remove: List[CardInstance] = []
        
        self.case_phase: CasePhase = CasePhase.INTRO
        self.is_first_player: bool = False
        self.normal_hand_played_this_turn: bool = False
        self.hint_played_this_turn: bool = False

    def effective_file_count(self) -> int:
        count = len(self.file)
        if self.partner and self.partner.zone == Zone.FILE:
            count += 1
        return count

class MatchEngine:
    def __init__(self, catalog: CardCatalog, seed: Optional[int] = None):
        self.catalog = catalog
        self.rng = random.Random(seed)
        self.players: Dict[str, PlayerState] = {}
        self.player_order: List[str] = []
        self.phase: MatchPhase = MatchPhase.WAITING
        self.turn: int = 0
        self.turn_player_id: str = ""
        self.revision: int = 0
        self.next_instance_id: int = 1
        self.next_decision_id: int = 1
        
        self.pending_decision: Optional[Dict[str, Any]] = None
        self.winner_id: Optional[str] = None
        self.finish_reason: Optional[str] = None

        # Ongoing action context
        self.action_context: Optional[Dict[str, Any]] = None

    def _generate_instance_id(self) -> str:
        inst_id = f"inst_{self.next_instance_id}"
        self.next_instance_id += 1
        return inst_id

    def _generate_decision_id(self) -> str:
        d_id = f"dec_{self.next_decision_id}"
        self.next_decision_id += 1
        return d_id

    def add_player(self, player_id: str, deck_def: Dict[str, Any]):
        if player_id in self.players:
            raise ValueError(f"Player {player_id} already exists")
        ps = PlayerState(player_id)
        
        partner_def_id = deck_def["partnerId"]
        p_inst = CardInstance(
            instanceId=self._generate_instance_id(),
            definitionId=partner_def_id,
            ownerId=player_id,
            zone=Zone.PARTNER,
            orientation=CardOrientation.ACTIVE,
            faceUp=False
        )
        ps.partner = p_inst
        
        case_def_id = deck_def["caseId"]
        c_inst = CardInstance(
            instanceId=self._generate_instance_id(),
            definitionId=case_def_id,
            ownerId=player_id,
            zone=Zone.CASE,
            orientation=CardOrientation.ACTIVE,
            faceUp=False
        )
        ps.case = c_inst
        
        for item in deck_def["mainDeck"]:
            def_id = item["definitionId"]
            count = item["count"]
            for _ in range(count):
                inst = CardInstance(
                    instanceId=self._generate_instance_id(),
                    definitionId=def_id,
                    ownerId=player_id,
                    zone=Zone.DECK,
                    faceUp=False
                )
                ps.deck.append(inst)

        self.players[player_id] = ps
        self.player_order.append(player_id)

    def start_match(self):
        if len(self.player_order) != 2:
            raise ValueError("Exactly 2 players required")
        
        # Determine first player
        first_idx = self.rng.randint(0, 1)
        if first_idx == 1:
            self.player_order = [self.player_order[1], self.player_order[0]]
        
        self.players[self.player_order[0]].is_first_player = True
        self.players[self.player_order[1]].is_first_player = False

        # Shuffle main decks
        for ps in self.players.values():
            self.rng.shuffle(ps.deck)

        # Deal 5 cards
        for ps in self.players.values():
            for _ in range(5):
                card = ps.deck.pop(0)
                card.zone = Zone.HAND
                card.faceUp = True
                ps.hand.append(card)

        self.phase = MatchPhase.MULLIGAN_FIRST
        self._prompt_mulligan(self.player_order[0])

    def _prompt_mulligan(self, player_id: str):
        ps = self.players[player_id]
        options = [c.instanceId for c in ps.hand]
        self.pending_decision = {
            "decisionId": self._generate_decision_id(),
            "revision": self.revision,
            "playerId": player_id,
            "kind": "mulligan",
            "options": {"handCardIds": options}
        }

    def submit_decision(self, player_id: str, answer: Dict[str, Any]):
        if not self.pending_decision:
            raise ValueError("No pending decision")
        if player_id != self.pending_decision["playerId"]:
            raise ValueError(f"Expected decision from {self.pending_decision['playerId']}, got {player_id}")
        
        kind = self.pending_decision["kind"]
        val = answer.get("value", {})
        
        self.pending_decision = None
        self.revision += 1

        if kind == "mulligan":
            self._handle_mulligan_answer(player_id, val)
        elif kind == "mainAction":
            self._handle_main_action(player_id, val)
        elif kind == "hintPlay":
            self._handle_hint_play_answer(player_id, val)
        elif kind == "guard":
            self._handle_guard_answer(player_id, val)
        elif kind == "contactSupport":
            self._handle_contact_support_answer(player_id, val)
        else:
            raise ValueError(f"Unknown decision kind: {kind}")

    def _handle_mulligan_answer(self, player_id: str, val: Dict[str, Any]):
        cards_to_replace = val.get("cardIds", [])
        ps = self.players[player_id]
        if cards_to_replace:
            kept = []
            returned = []
            for c in ps.hand:
                if c.instanceId in cards_to_replace:
                    returned.append(c)
                else:
                    kept.append(c)
            
            for c in returned:
                c.zone = Zone.DECK
                ps.deck.append(c)
            
            self.rng.shuffle(ps.deck)
            for _ in range(len(returned)):
                draw_c = ps.deck.pop(0)
                draw_c.zone = Zone.HAND
                draw_c.faceUp = True
                kept.append(draw_c)
            ps.hand = kept

        if self.phase == MatchPhase.MULLIGAN_FIRST:
            self.phase = MatchPhase.MULLIGAN_SECOND
            self._prompt_mulligan(self.player_order[1])
        elif self.phase == MatchPhase.MULLIGAN_SECOND:
            self.phase = MatchPhase.REVEAL
            for p in self.players.values():
                if p.partner: p.partner.faceUp = True
                if p.case: p.case.faceUp = True
            
            self._start_turn(1, self.player_order[0])

    def _start_turn(self, turn: int, turn_player_id: str):
        self.turn = turn
        self.turn_player_id = turn_player_id
        ps = self.players[turn_player_id]
        
        ps.normal_hand_played_this_turn = False
        ps.hint_played_this_turn = False

        # AUTO_RESTORE_PARTNER
        if ps.partner and ps.partner.zone == Zone.FILE:
            ps.partner.zone = Zone.PARTNER
            ps.partner.orientation = CardOrientation.ACTIVE

        # AUTO_ACTIVATE
        for c in ps.field:
            if c.orientation == CardOrientation.STUN:
                c.orientation = CardOrientation.SLEEP
            else:
                c.orientation = CardOrientation.ACTIVE
        if ps.partner and ps.partner.zone == Zone.PARTNER:
            if ps.partner.orientation == CardOrientation.STUN:
                ps.partner.orientation = CardOrientation.SLEEP
            else:
                ps.partner.orientation = CardOrientation.ACTIVE

        # AUTO_DRAW: 1 card
        self._draw_cards(ps, 1)
        if self.phase == MatchPhase.FINISHED:
            return

        # AUTO_FILE: 1 if turn 1 and first player, else 2
        file_count = 1 if (self.turn == 1 and ps.is_first_player) else 2
        for _ in range(file_count):
            self._file_card_from_deck(ps)
            if self.phase == MatchPhase.FINISHED:
                return

        self.phase = MatchPhase.MAIN
        self._prompt_main_action(ps.player_id)

    def _draw_cards(self, ps: PlayerState, count: int):
        for _ in range(count):
            if not ps.deck:
                self._trigger_refresh(ps)
                if self.phase == MatchPhase.FINISHED:
                    return
            card = ps.deck.pop(0)
            card.zone = Zone.HAND
            card.faceUp = True
            ps.hand.append(card)

    def _file_card_from_deck(self, ps: PlayerState):
        if not ps.deck:
            self._trigger_refresh(ps)
            if self.phase == MatchPhase.FINISHED:
                return
        card = ps.deck.pop(0)
        card.zone = Zone.FILE
        ps.file.append(card)

    def _gain_evidence(self, ps: PlayerState, count: int):
        for _ in range(count):
            if not ps.deck:
                self._trigger_refresh(ps)
                if self.phase == MatchPhase.FINISHED:
                    return
            card = ps.deck.pop(0)
            card.zone = Zone.EVIDENCE
            ps.evidence.append(card)

    def _trigger_refresh(self, ps: PlayerState):
        if not ps.remove:
            # Immediate loss if remove is empty [rules.md line 65]
            other_id = [pid for pid in self.player_order if pid != ps.player_id][0]
            self.finish_match(other_id, "REFRESH_EMPTY_REMOVE")
            return
        
        while ps.remove:
            c = ps.remove.pop(0)
            c.zone = Zone.DECK
            ps.deck.append(c)
        self.rng.shuffle(ps.deck)
        
        # Opponent gains 1 evidence
        other_ps = [p for p in self.players.values() if p.player_id != ps.player_id][0]
        self._gain_evidence(other_ps, 1)

    def finish_match(self, winner_id: str, reason: str):
        self.phase = MatchPhase.FINISHED
        self.winner_id = winner_id
        self.finish_reason = reason
        self.pending_decision = None

    def _prompt_main_action(self, player_id: str):
        if self.phase == MatchPhase.FINISHED:
            return
        ps = self.players[player_id]
        opp_ps = [p for p in self.players.values() if p.player_id != player_id][0]
        effective_file = ps.effective_file_count()
        case_def = self.catalog.get(ps.case.definitionId) if ps.case else None
        case_colors = set(case_def.colors if case_def else [])

        actions = []

        # 1. End turn
        actions.append({"type": "END_TURN"})

        # 2. Hand Play (Normal)
        if not ps.normal_hand_played_this_turn and not ps.hint_played_this_turn:
            playable_cards = []
            for c in ps.hand:
                c_def = self.catalog.get(c.definitionId)
                if c_def.level <= effective_file and set(c_def.colors).issubset(case_colors):
                    playable_cards.append(c.instanceId)
            if playable_cards and len(ps.field) < 5:
                actions.append({"type": "HAND_PLAY", "cardIds": playable_cards})

        # 3. Hint Take
        if ps.file:
            actions.append({"type": "HINT_TAKE"})

        # 4. Assist
        if ps.partner and ps.partner.zone == Zone.PARTNER and ps.partner.orientation == CardOrientation.ACTIVE:
            actions.append({"type": "ASSIST"})

        # 5. Resolve Case
        if (ps.case_phase == CasePhase.RESOLUTION and ps.partner and 
            ps.partner.zone == Zone.PARTNER and ps.partner.orientation == CardOrientation.ACTIVE):
            target_evidence = case_def.caseLevels["first" if ps.is_first_player else "second"] if (case_def and case_def.caseLevels) else 6
            if len(ps.evidence) >= target_evidence:
                actions.append({"type": "RESOLVE_CASE"})

        # 6. Deduce
        deduce_actors = []
        if ps.partner and ps.partner.zone == Zone.PARTNER and ps.partner.orientation == CardOrientation.ACTIVE:
            deduce_actors.append(ps.partner.instanceId)
        for c in ps.field:
            if c.orientation == CardOrientation.ACTIVE and c.enteredTurn != self.turn:
                deduce_actors.append(c.instanceId)
        if deduce_actors:
            actions.append({"type": "DEDUCE", "actorIds": deduce_actors})

        # 7. Action
        action_actors = []
        for c in ps.field:
            if c.orientation == CardOrientation.ACTIVE and c.enteredTurn != self.turn:
                action_actors.append(c.instanceId)
        
        target_ids = []
        for opp_c in opp_ps.field:
            if opp_c.orientation in (CardOrientation.SLEEP, CardOrientation.STUN):
                target_ids.append(opp_c.instanceId)
        if opp_ps.evidence and opp_ps.case:
            target_ids.append(opp_ps.case.instanceId)

        if action_actors and target_ids:
            actions.append({"type": "ACTION", "actorIds": action_actors, "targetIds": target_ids})

        self.pending_decision = {
            "decisionId": self._generate_decision_id(),
            "revision": self.revision,
            "playerId": player_id,
            "kind": "mainAction",
            "options": {"actions": actions}
        }

    def _handle_main_action(self, player_id: str, val: Dict[str, Any]):
        action_type = val.get("type")
        ps = self.players[player_id]

        if action_type == "END_TURN":
            next_turn = self.turn + 1
            next_player_id = [pid for pid in self.player_order if pid != player_id][0]
            self._start_turn(next_turn, next_player_id)
        
        elif action_type == "HAND_PLAY":
            card_id = val.get("cardId")
            card = next((c for c in ps.hand if c.instanceId == card_id), None)
            if not card:
                raise ValueError("Card not in hand")
            ps.hand.remove(card)
            card.zone = Zone.FIELD
            card.orientation = CardOrientation.ACTIVE
            card.enteredTurn = self.turn
            card.faceUp = True
            ps.field.append(card)
            ps.normal_hand_played_this_turn = True
            self._prompt_main_action(player_id)

        elif action_type == "HINT_TAKE":
            top_file = ps.file.pop()
            top_file.zone = Zone.HAND
            top_file.faceUp = True
            ps.hand.append(top_file)
            ps.hint_played_this_turn = True
            
            # Now offer hintPlay decision
            effective_file = ps.effective_file_count()
            case_def = self.catalog.get(ps.case.definitionId) if ps.case else None
            case_colors = set(case_def.colors if case_def else [])
            playable_cards = []
            if len(ps.field) < 5:
                for c in ps.hand:
                    c_def = self.catalog.get(c.definitionId)
                    if c_def.level <= effective_file and set(c_def.colors).issubset(case_colors):
                        playable_cards.append(c.instanceId)
            
            self.pending_decision = {
                "decisionId": self._generate_decision_id(),
                "revision": self.revision,
                "playerId": player_id,
                "kind": "hintPlay",
                "options": {"playableCardIds": playable_cards}
            }

        elif action_type == "ASSIST":
            if ps.partner and ps.partner.zone == Zone.PARTNER:
                ps.partner.orientation = CardOrientation.SLEEP
                ps.partner.zone = Zone.FILE
                if ps.effective_file_count() >= 7:
                    ps.case_phase = CasePhase.RESOLUTION
            self._prompt_main_action(player_id)

        elif action_type == "RESOLVE_CASE":
            self.finish_match(player_id, "CASE_RESOLVED")

        elif action_type == "DEDUCE":
            actor_id = val.get("actorId")
            actor = None
            if ps.partner and ps.partner.instanceId == actor_id:
                actor = ps.partner
            else:
                actor = next((c for c in ps.field if c.instanceId == actor_id), None)
            
            if not actor:
                raise ValueError("Invalid actor")
            actor.orientation = CardOrientation.SLEEP
            c_def = self.catalog.get(actor.definitionId)
            lp = c_def.lp
            if lp > 0:
                self._gain_evidence(ps, lp)
            self._prompt_main_action(player_id)

        elif action_type == "ACTION":
            actor_id = val.get("actorId")
            target_id = val.get("targetId")
            actor = next((c for c in ps.field if c.instanceId == actor_id), None)
            if not actor:
                raise ValueError("Invalid actor")
            actor.orientation = CardOrientation.SLEEP

            opp_ps = [p for p in self.players.values() if p.player_id != player_id][0]
            self.action_context = {
                "actorId": actor_id,
                "targetId": target_id,
                "attackerPlayerId": player_id,
                "defenderPlayerId": opp_ps.player_id
            }

            # Check if defender can guard
            guards = [c.instanceId for c in opp_ps.field if c.orientation == CardOrientation.ACTIVE]
            if guards:
                self.pending_decision = {
                    "decisionId": self._generate_decision_id(),
                    "revision": self.revision,
                    "playerId": opp_ps.player_id,
                    "kind": "guard",
                    "options": {"guardCardIds": guards}
                }
            else:
                self._proceed_to_action_resolution(None)

    def _handle_hint_play_answer(self, player_id: str, val: Dict[str, Any]):
        ps = self.players[player_id]
        card_id = val.get("cardId")
        if card_id:
            card = next((c for c in ps.hand if c.instanceId == card_id), None)
            if card and len(ps.field) < 5:
                ps.hand.remove(card)
                card.zone = Zone.FIELD
                card.orientation = CardOrientation.ACTIVE
                card.enteredTurn = self.turn
                card.faceUp = True
                ps.field.append(card)
        self._prompt_main_action(player_id)

    def _handle_guard_answer(self, player_id: str, val: Dict[str, Any]):
        guard_id = val.get("guardCardId")
        self._proceed_to_action_resolution(guard_id)

    def _proceed_to_action_resolution(self, guard_id: Optional[str]):
        ctx = self.action_context
        attacker_ps = self.players[ctx["attackerPlayerId"]]
        defender_ps = self.players[ctx["defenderPlayerId"]]
        actor = next((c for c in attacker_ps.field if c.instanceId == ctx["actorId"]), None)

        if guard_id:
            guard_card = next((c for c in defender_ps.field if c.instanceId == guard_id), None)
            if guard_card:
                guard_card.orientation = CardOrientation.SLEEP
            defender_card = guard_card
            is_case_action = False
        else:
            if defender_ps.case and ctx["targetId"] == defender_ps.case.instanceId:
                is_case_action = True
                defender_card = None
            else:
                is_case_action = False
                defender_card = next((c for c in defender_ps.field if c.instanceId == ctx["targetId"]), None)

        if is_case_action:
            if defender_ps.evidence:
                ev_card = defender_ps.evidence.pop()
                ev_card.zone = Zone.REMOVE
                defender_ps.remove.append(ev_card)
                self._gain_evidence(attacker_ps, 1)
            self.action_context = None
            self._prompt_main_action(attacker_ps.player_id)
        else:
            # Contact AP comparison
            if actor and defender_card:
                actor_def = self.catalog.get(actor.definitionId)
                def_def = self.catalog.get(defender_card.definitionId)
                if actor_def.ap >= def_def.ap:
                    defender_ps.field.remove(defender_card)
                    defender_card.zone = Zone.REMOVE
                    defender_ps.remove.append(defender_card)
            self.action_context = None
            self._prompt_main_action(attacker_ps.player_id)
