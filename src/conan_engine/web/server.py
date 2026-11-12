import json
import http.server
import socketserver
from pathlib import Path
from urllib.parse import urlparse, parse_qs

from conan_engine.catalog import CardCatalog
from conan_engine.engine import MatchEngine
from conan_engine.observation import build_player_observation
from conan_engine.agent import RandomAgent

class MatchManager:
    def __init__(self):
        self.catalog_path = Path("data/dummy/cards.json")
        self.deck_path = Path("data/dummy/deck.json")
        self.catalog = CardCatalog.load_from_json(self.catalog_path)
        self.load_deck()
        self.engine = None
        self.p2_is_cpu = True
        self.cpu_agent = None
        self.reset_match()

    def load_deck(self):
        with open(self.deck_path, "r", encoding="utf-8") as f:
            self.deck_def = json.load(f)

    def save_deck(self, new_deck_def):
        self.deck_def = new_deck_def
        with open(self.deck_path, "w", encoding="utf-8") as f:
            json.dump(new_deck_def, f, ensure_ascii=False, indent=2)

    def reset_match(self):
        self.load_deck()
        self.engine = MatchEngine(self.catalog)
        self.engine.add_player("player1", self.deck_def)
        self.engine.add_player("player2", self.deck_def)
        self.cpu_agent = RandomAgent("player2") if self.p2_is_cpu else None
        self.engine.start_match()
        self.process_cpu_if_needed()

    def process_cpu_if_needed(self):
        max_steps = 50
        steps = 0
        while (self.p2_is_cpu and self.cpu_agent and 
               self.engine.pending_decision and 
               self.engine.pending_decision["playerId"] == "player2" and 
               steps < max_steps):
            dec = self.engine.pending_decision
            ans = self.cpu_agent.choose_action(dec)
            self.engine.submit_decision("player2", ans)
            steps += 1

manager = MatchManager()
STATIC_DIR = Path(__file__).parent / "static"

class ConanHttpHandler(http.server.SimpleHTTPRequestHandler):
    def do_GET(self):
        parsed = urlparse(self.path)
        
        # 1. State API
        if parsed.path == "/api/state":
            manager.process_cpu_if_needed()
            query = parse_qs(parsed.query)
            player_id = query.get("player_id", ["player1"])[0]
            if player_id not in manager.engine.players:
                player_id = "player1"
            obs = build_player_observation(manager.engine, player_id)
            obs["p2IsCpu"] = manager.p2_is_cpu
            self.send_response(200)
            self.send_header("Content-Type", "application/json; charset=utf-8")
            self.end_headers()
            self.wfile.write(json.dumps(obs, ensure_ascii=False).encode("utf-8"))
            
        # 2. Catalog & Deck API for Deck Builder
        elif parsed.path == "/api/catalog":
            with open(manager.catalog_path, "r", encoding="utf-8") as f:
                cat_data = json.load(f)
            self.send_response(200)
            self.send_header("Content-Type", "application/json; charset=utf-8")
            self.end_headers()
            self.wfile.write(json.dumps(cat_data, ensure_ascii=False).encode("utf-8"))

        elif parsed.path == "/api/deck":
            with open(manager.deck_path, "r", encoding="utf-8") as f:
                deck_data = json.load(f)
            self.send_response(200)
            self.send_header("Content-Type", "application/json; charset=utf-8")
            self.end_headers()
            self.wfile.write(json.dumps(deck_data, ensure_ascii=False).encode("utf-8"))

        elif parsed.path == "/" or parsed.path == "/index.html":
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.end_headers()
            with open(STATIC_DIR / "index.html", "rb") as f:
                self.wfile.write(f.read())
        else:
            super().do_GET()

    def do_POST(self):
        parsed = urlparse(self.path)
        if parsed.path == "/api/reset":
            manager.reset_match()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(b'{"status":"ok"}')
            
        elif parsed.path == "/api/set_cpu":
            content_length = int(self.headers.get("Content-Length", 0))
            body = self.rfile.read(content_length)
            req = json.loads(body.decode("utf-8"))
            manager.p2_is_cpu = bool(req.get("isCpu", True))
            manager.cpu_agent = RandomAgent("player2") if manager.p2_is_cpu else None
            manager.process_cpu_if_needed()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(b'{"status":"ok"}')

        elif parsed.path == "/api/save_deck":
            content_length = int(self.headers.get("Content-Length", 0))
            body = self.rfile.read(content_length)
            req = json.loads(body.decode("utf-8"))
            try:
                # Validate 40 cards, max 3 copies, partner and case
                main_deck = req.get("mainDeck", [])
                total_cards = sum(item.get("count", 0) for item in main_deck)
                if total_cards != 40:
                    raise ValueError(f"メインデッキはちょうど40枚である必要があります（現在: {total_cards}枚）")
                for item in main_deck:
                    if item.get("count", 0) > 3:
                        raise ValueError(f"同一カードは最大3枚までです: {item.get('definitionId')}")
                if not req.get("partnerId"):
                    raise ValueError("パートナーカードが選択されていません")
                if not req.get("caseId"):
                    raise ValueError("事件カードが選択されていません")

                manager.save_deck(req)
                manager.reset_match() # restart match with new deck
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.end_headers()
                self.wfile.write(b'{"status":"ok"}')
            except Exception as e:
                self.send_response(400)
                self.send_header("Content-Type", "application/json")
                self.end_headers()
                self.wfile.write(json.dumps({"error": str(e)}).encode("utf-8"))

        elif parsed.path == "/api/decision":
            content_length = int(self.headers.get("Content-Length", 0))
            body = self.rfile.read(content_length)
            req = json.loads(body.decode("utf-8"))
            player_id = req.get("playerId")
            value = req.get("value")
            
            try:
                manager.engine.submit_decision(player_id, {"value": value})
                manager.process_cpu_if_needed()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.end_headers()
                self.wfile.write(b'{"status":"ok"}')
            except Exception as e:
                self.send_response(400)
                self.send_header("Content-Type", "application/json")
                self.end_headers()
                self.wfile.write(json.dumps({"error": str(e)}).encode("utf-8"))
        else:
            self.send_response(404)
            self.end_headers()

def run_server(port: int = 8000):
    handler = ConanHttpHandler
    with socketserver.TCPServer(("", port), handler) as httpd:
        print(f"Detective Conan Web App running at http://localhost:{port}")
        httpd.serve_forever()

if __name__ == "__main__":
    import sys
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8000
    run_server(port)
