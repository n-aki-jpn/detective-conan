from dataclasses import dataclass, field
from enum import Enum
from typing import List, Dict, Optional, Any

class CardType(str, Enum):
    CHARACTER = "CHARACTER"
    EVENT = "EVENT"
    PARTNER = "PARTNER"
    CASE = "CASE"

class CardOrientation(str, Enum):
    ACTIVE = "ACTIVE"
    SLEEP = "SLEEP"
    STUN = "STUN"

class Zone(str, Enum):
    DECK = "DECK"
    HAND = "HAND"
    FIELD = "FIELD"
    PARTNER = "PARTNER"
    CASE = "CASE"
    FILE = "FILE"
    EVIDENCE = "EVIDENCE"
    REMOVE = "REMOVE"
    RESOLVING = "RESOLVING"
    PENDING_EVIDENCE_REMOVAL = "PENDING_EVIDENCE_REMOVAL"

class MoveCause(str, Enum):
    RULE = "RULE"
    COST = "COST"
    EFFECT = "EFFECT"
    ACTION_RESULT = "ACTION_RESULT"
    SWITCH = "SWITCH"
    REFRESH = "REFRESH"

class MatchPhase(str, Enum):
    WAITING = "WAITING"
    SETUP_HIDDEN = "SETUP_HIDDEN"
    MULLIGAN_FIRST = "MULLIGAN_FIRST"
    MULLIGAN_SECOND = "MULLIGAN_SECOND"
    REVEAL = "REVEAL"
    AUTO_RESTORE_PARTNER = "AUTO_RESTORE_PARTNER"
    AUTO_ACTIVATE = "AUTO_ACTIVATE"
    AUTO_DRAW = "AUTO_DRAW"
    AUTO_FILE = "AUTO_FILE"
    MAIN = "MAIN"
    DEDUCE = "DEDUCE"
    ACTION_GUARD = "ACTION_GUARD"
    ACTION_CONTACT = "ACTION_CONTACT"
    ACTION_CASE = "ACTION_CASE"
    END_TRIGGERS = "END_TRIGGERS"
    END_EXPIRE = "END_EXPIRE"
    FINISHED = "FINISHED"

class CasePhase(str, Enum):
    INTRO = "INTRO"
    RESOLUTION = "RESOLUTION"

@dataclass
class CardDefinition:
    definitionId: str
    canonicalId: str
    cardType: CardType
    colors: List[str]
    level: int = 0
    ap: int = 0
    lp: int = 0
    names: List[str] = field(default_factory=list)
    description: str = ""
    traits: List[str] = field(default_factory=list)
    abilities: List[Dict[str, Any]] = field(default_factory=list)
    caseLevels: Optional[Dict[str, int]] = None
    mainThumb: Optional[str] = None
    mainPath: Optional[str] = None

@dataclass
class CardInstance:
    instanceId: str
    definitionId: str
    ownerId: str
    zone: Zone
    orientation: CardOrientation = CardOrientation.ACTIVE
    enteredTurn: int = -1
    faceUp: bool = False
    modifiers: Dict[str, Any] = field(default_factory=dict)
