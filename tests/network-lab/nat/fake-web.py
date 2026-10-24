#!/usr/bin/env python3
"""Backend TCP sin TLS: el banner permite identificar la ruta de HAProxy."""
import argparse
import socketserver


class Web(socketserver.BaseRequestHandler):
    def handle(self):
        self.request.settimeout(6)
        try:
            self.request.sendall(b"WEB\n")
            self.request.recv(65536)
        except (OSError, TimeoutError):
            pass


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("port", type=int)
    args = parser.parse_args()
    socketserver.ThreadingTCPServer.allow_reuse_address = True
    with socketserver.ThreadingTCPServer(("127.0.0.1", args.port), Web) as server:
        print("READY", flush=True)
        server.serve_forever()
