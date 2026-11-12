import json
from pathlib import Path
from typing import Dict
from .models import CardDefinition, CardType

class CardCatalog:
    def __init__(self, catalog_version: str = "official-raw-v1"):
        self.catalog_version = catalog_version
        self.definitions: Dict[str, CardDefinition] = {}

    def get(self, definition_id: str) -> CardDefinition:
        if definition_id not in self.definitions:
            raise KeyError(f"Card definition not found: {definition_id}")
        return self.definitions[definition_id]

    @classmethod
    def load_from_json(cls, file_path: str | Path) -> "CardCatalog":
        path = Path(file_path)
        with open(path, "r", encoding="utf-8") as f:
            data = json.load(f)
        
        catalog = cls(catalog_version=data.get("catalogVersion", "official-raw-v1"))
        for card_data in data.get("cards", []):
            raw_type = card_data.get("cardType", card_data.get("type", "CHARACTER")).upper()
            card_type = CardType[raw_type]
            
            c_def = CardDefinition(
                definitionId=card_data["definitionId"],
                canonicalId=card_data.get("canonicalId", card_data["definitionId"]),
                cardType=card_type,
                colors=card_data.get("colors", []),
                level=card_data.get("level", 0),
                ap=card_data.get("ap", 0),
                lp=card_data.get("lp", 0),
                names=card_data.get("names", [card_data.get("title", card_data.get("dummyName", ""))]),
                description=card_data.get("description", ""),
                traits=card_data.get("traits", []),
                abilities=card_data.get("abilities", []),
                caseLevels=card_data.get("caseLevels"),
                mainThumb=card_data.get("mainThumb"),
                mainPath=card_data.get("mainPath")
            )
            catalog.definitions[c_def.definitionId] = c_def
        return catalog
