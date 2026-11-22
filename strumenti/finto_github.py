"""Un finto GitHub in locale per provare l'aggiornamento dall'app senza toccare le release vere (solo 127.0.0.1).

  python strumenti/finto_github.py <exe da servire> <modo>
  modi: giusto | sha-sbagliato | exe-404 | interrotto | lento (latest.json risponde dopo 8 s)

Risponde come `releases/latest` con la build 99 e serve l'.exe dato, lento (64 KB ogni 40 ms) per vedere lo stato
"scarico". aimpro va compilato con `AIMPRO_BUILD=5` e `AIMPRO_API_PROVA=http://127.0.0.1:8765/latest.json`
(vedi CLAUDE.md, "L'aggiornamento dall'app").
"""
import hashlib
import http.server
import json
import sys
import time

EXE = open(sys.argv[1], "rb").read()
MODO = sys.argv[2]
BASE = "http://127.0.0.1:8765"


class H(http.server.BaseHTTPRequestHandler):
    def manda(self, codice, corpo, tipo="application/octet-stream"):
        self.send_response(codice)
        self.send_header("Content-Type", tipo)
        self.send_header("Content-Length", str(len(corpo)))
        self.end_headers()
        self.wfile.write(corpo)

    def do_GET(self):
        print(self.path, self.headers.get("User-Agent"), self.headers.get("Accept"), flush=True)
        if self.path == "/latest.json":
            if MODO == "lento":
                time.sleep(8)
            r = {
                "tag_name": "v0.1.0-build.99",
                "draft": False,
                "prerelease": False,
                "assets": [
                    {"name": "aimpro.exe", "browser_download_url": BASE + "/exe"},
                    {"name": "aimpro.exe.sha256", "browser_download_url": BASE + "/sha"},
                ],
            }
            self.manda(200, json.dumps(r).encode(), "application/json")
        elif self.path == "/sha":
            h = hashlib.sha256(EXE).hexdigest()
            if MODO == "sha-sbagliato":
                h = hashlib.sha256(b"un altro file").hexdigest()
            self.manda(200, f"{h}  aimpro.exe\r\n".encode())
        elif self.path == "/exe":
            if MODO == "exe-404":
                self.manda(404, b"Not Found", "text/plain")
                return
            self.send_response(200)
            self.send_header("Content-Length", str(len(EXE)))
            self.end_headers()
            fine = len(EXE) // 2 if MODO == "interrotto" else len(EXE)
            for i in range(0, fine, 65536):
                self.wfile.write(EXE[i:min(i + 65536, fine)])
                self.wfile.flush()
                time.sleep(0.04)
            if MODO == "interrotto":
                self.close_connection = True
        else:
            self.manda(404, b"", "text/plain")


http.server.ThreadingHTTPServer(("127.0.0.1", 8765), H).serve_forever()
