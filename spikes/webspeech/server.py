"""Serves the Web Speech probe page and records what the page reports.

Usage: python server.py <sample.wav> <log-file>
"""

import http.server
import pathlib
import sys

PAGE = pathlib.Path(__file__).with_name("index.html")
SAMPLE = pathlib.Path(sys.argv[1])
LOG = pathlib.Path(sys.argv[2])
PORT = 8765


class ProbeHandler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        route = self.path.split("?")[0]
        if route == "/":
            self._send(PAGE.read_bytes(), "text/html; charset=utf-8")
        elif route == "/sample.wav":
            self._send(SAMPLE.read_bytes(), "audio/wav")
        else:
            self.send_error(404)

    def do_POST(self):
        length = int(self.headers.get("Content-Length", 0))
        line = self.rfile.read(length).decode("utf-8", errors="replace")
        with LOG.open("a", encoding="utf-8") as log:
            log.write(line + "\n")
        self.send_response(204)
        self.end_headers()

    def _send(self, body, content_type):
        self.send_response(200)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass


http.server.ThreadingHTTPServer(("127.0.0.1", PORT), ProbeHandler).serve_forever()
