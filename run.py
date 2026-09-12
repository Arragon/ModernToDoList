import os
import webbrowser
import http.server
import socket
import socketserver
import threading
import sys
import time

PORT = int(os.environ.get("PORT", "8000"))
PORT_SCAN_RANGE = 20

class ExclusiveTCPServer(socketserver.TCPServer):
    allow_reuse_address = False

    def server_bind(self):
        if os.name == "nt":
            try:
                self.socket.setsockopt(socket.SOL_SOCKET, socket.SO_EXCLUSIVEADDRUSE, 1)
            except Exception:
                pass
        super().server_bind()


def create_server(port: int):
    handler = http.server.SimpleHTTPRequestHandler
    return ExclusiveTCPServer(("", port), handler)


def start_server(httpd):
    os.chdir(os.path.dirname(os.path.abspath(__file__)))
    httpd.serve_forever()

if __name__ == "__main__":
    httpd = None
    last_err = None
    for p in range(PORT, PORT + PORT_SCAN_RANGE):
        try:
            httpd = create_server(p)
            break
        except OSError as e:
            last_err = e
            if getattr(e, "winerror", None) == 10048 or getattr(e, "errno", None) == 10048:
                continue
            raise

    if httpd is None:
        raise last_err

    server_thread = threading.Thread(target=start_server, args=(httpd,), daemon=True)
    server_thread.start()

    url = f"http://localhost:{httpd.server_address[1]}/"
    print(f"Starting server at {url}", flush=True)
    webbrowser.open(url)

    try:
        while True:
            time.sleep(1)
    except KeyboardInterrupt:
        try:
            httpd.shutdown()
            httpd.server_close()
        except Exception:
            pass
        print("\nServer stopped.")
        sys.exit(0)
