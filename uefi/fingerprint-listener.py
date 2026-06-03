#!/usr/bin/env python3
"""Tiny listener for the Mastertech UEFI fingerprint POST.
Reads the full HTTP request, prints the JSON body, and replies 200.
Usage: python3 fingerprint-listener.py [port]   (default 8080)
"""
import socket, sys
port = int(sys.argv[1]) if len(sys.argv) > 1 else 8080
srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
srv.bind(("0.0.0.0", port))
srv.listen(5)
print(f"listening on 0.0.0.0:{port} ...")
while True:
    conn, addr = srv.accept()
    conn.settimeout(1.0)
    data = b""
    try:
        while True:
            chunk = conn.recv(4096)
            if not chunk:
                break
            data += chunk
    except socket.timeout:
        pass
    text = data.decode("utf-8", "replace")
    body = text.split("\r\n\r\n", 1)[1] if "\r\n\r\n" in text else text
    print(f"\n=== POST from {addr[0]} ===")
    print(body.strip() or "(no body)")
    conn.sendall(b"HTTP/1.0 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\nok\n")
    conn.close()
