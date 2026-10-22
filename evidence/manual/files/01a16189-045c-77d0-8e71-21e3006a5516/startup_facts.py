"""Facts 1 and 2 of spec 0023 step 0: what the first INFO says while JetStream
starts, and whether the client listener opens before or after JetStream.

Usage: python3 -I startup_facts.py <nats-server> <store-dir> <log-file> [populate N M]
Prints one JSON object. Loopback only, a free port, no network.
"""
import json
import os
import re
import socket
import subprocess
import sys
import time

CONNECT = (
    b'CONNECT {"verbose":false,"pedantic":false,"tls_required":false,'
    b'"name":"stackly-readiness","lang":"rust","version":"0.1.0","protocol":1,'
    b'"echo":false,"headers":false,"no_responders":false}\r\n'
)


def free_port():
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    port = s.getsockname()[1]
    s.close()
    return port


def read_line(sock, buf, deadline):
    while b"\r\n" not in buf:
        sock.settimeout(max(0.01, deadline - time.monotonic()))
        chunk = sock.recv(65536)
        if not chunk:
            raise EOFError("closed")
        buf += chunk
    line, _, rest = buf.partition(b"\r\n")
    return line, rest


def js_api_info(sock, buf, timeout):
    """Sends $JS.API.INFO and returns (reply type or error, buf)."""
    inbox = "_INBOX.facts.%d" % int(time.monotonic() * 1e6)
    sock.sendall(f"SUB {inbox} 1\r\nPUB $JS.API.INFO {inbox} 0\r\n\r\n".encode())
    deadline = time.monotonic() + timeout
    while True:
        line, buf = read_line(sock, buf, deadline)
        if line.startswith(b"MSG "):
            n = int(line.split()[3])
            while len(buf) < n + 2:
                sock.settimeout(max(0.01, deadline - time.monotonic()))
                buf += sock.recv(65536)
            payload, buf = buf[:n], buf[n + 2 :]
            doc = json.loads(payload)
            if "error" in doc:
                return "error %s %s" % (doc["error"].get("code"), doc["error"].get("description")), buf
            return doc.get("type"), buf


def attempt(port, t0):
    """One full probe: INFO, CONNECT, PING, PONG, $JS.API.INFO."""
    try:
        sock = socket.create_connection(("127.0.0.1", port), timeout=0.2)
    except OSError:
        return None
    seen = {"connected_ms": round((time.monotonic() - t0) * 1000, 1)}
    try:
        deadline = time.monotonic() + 3
        buf = b""
        line, buf = read_line(sock, buf, deadline)
        info = json.loads(line.split(b" ", 1)[1])
        seen["info_jetstream"] = info.get("jetstream", False)
        seen["info_server_name"] = info.get("server_name")
        sock.sendall(CONNECT + b"PING\r\n")
        while True:
            line, buf = read_line(sock, buf, deadline)
            if line == b"PONG":
                break
        seen["pong_ms"] = round((time.monotonic() - t0) * 1000, 1)
        seen["js_api"], buf = js_api_info(sock, buf, 3)
        seen["js_api_ms"] = round((time.monotonic() - t0) * 1000, 1)
    except Exception as exc:  # noqa: BLE001 - a recording script
        seen["error"] = repr(exc)
    finally:
        sock.close()
    return seen


def measure(server, store, log):
    port = free_port()
    if os.path.exists(log):
        os.remove(log)
    t0 = time.monotonic()
    proc = subprocess.Popen(
        [server, "-a", "127.0.0.1", "-p", str(port), "-n", "golden", "-js", "-sd", store, "-l", log],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    attempts = []
    try:
        while time.monotonic() - t0 < 60:
            seen = attempt(port, t0)
            if seen is None:
                time.sleep(0.001)
                continue
            attempts.append(seen)
            if seen.get("js_api", "").startswith("io.nats.jetstream.api.v1.account_info_response"):
                break
            time.sleep(0.005)
    finally:
        proc.terminate()
        proc.wait(timeout=30)
    log_lines = []
    with open(log) as handle:
        for line in handle:
            if re.search(r"JetStream|Listening for client|Server is ready|Starting|Recovering|Enabled", line):
                log_lines.append(line.strip()[:160])
    return {
        "store_bytes": sum(
            os.path.getsize(os.path.join(d, f)) for d, _, fs in os.walk(store) for f in fs
        ),
        "first_connect_ms": attempts[0]["connected_ms"] if attempts else None,
        "attempts": attempts[:6],
        "attempt_count": len(attempts),
        "log": log_lines,
    }


def populate(server, store, streams, messages):
    port = free_port()
    proc = subprocess.Popen(
        [server, "-a", "127.0.0.1", "-p", str(port), "-n", "golden", "-js", "-sd", store],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    try:
        t0 = time.monotonic()
        while attempt(port, t0) is None or "js_api" not in (attempt(port, t0) or {}):
            time.sleep(0.05)
        sock = socket.create_connection(("127.0.0.1", port), timeout=5)
        buf = b""
        line, buf = read_line(sock, buf, time.monotonic() + 3)
        sock.sendall(CONNECT.replace(b'"echo":false', b'"echo":true') + b"SUB _INBOX.pop 1\r\n")
        payload = b"x" * 1024
        for i in range(streams):
            cfg = json.dumps({"name": f"s{i}", "subjects": [f"s{i}.>"], "storage": "file"}).encode()
            sock.sendall(f"PUB $JS.API.STREAM.CREATE.s{i} _INBOX.pop {len(cfg)}\r\n".encode() + cfg + b"\r\n")
            deadline = time.monotonic() + 5
            while True:
                line, buf = read_line(sock, buf, deadline)
                if line.startswith(b"MSG "):
                    n = int(line.split()[3])
                    while len(buf) < n + 2:
                        buf += sock.recv(65536)
                    buf = buf[n + 2 :]
                    break
            batch = b"".join(
                f"PUB s{i}.x {len(payload)}\r\n".encode() + payload + b"\r\n" for _ in range(messages)
            )
            sock.sendall(batch + b"PING\r\n")
            while True:
                line, buf = read_line(sock, buf, time.monotonic() + 30)
                if line == b"PONG":
                    break
        sock.close()
        time.sleep(1)
    finally:
        proc.terminate()
        proc.wait(timeout=60)


if __name__ == "__main__":
    server, store, log = sys.argv[1:4]
    os.makedirs(store, exist_ok=True)
    if len(sys.argv) > 4 and sys.argv[4] == "populate":
        populate(server, store, int(sys.argv[5]), int(sys.argv[6]))
    print(json.dumps(measure(server, store, log), indent=1))
