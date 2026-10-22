"""Fact 3 of spec 0023 step 0: does NATS turn JetStream off while it runs?
(a) SIGUSR2 (lame duck), (b) a full disk. Loopback only."""
import shutil, json, os, signal, socket, subprocess, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import startup_facts as f

server, store_dir, mode = sys.argv[1:4]
os.makedirs(store_dir, exist_ok=True)
port = f.free_port()
conf = os.path.join(os.path.dirname(store_dir), "rt-%s.conf" % mode)
log = os.path.join(os.path.dirname(store_dir), "rt-%s.log" % mode)
open(conf, "w").write(
    "server_name: golden\nlisten: 127.0.0.1:%d\njetstream {\n  store_dir: \"%s\"\n  max_file_store: 1073741824\n}\n" % (port, store_dir))
if os.path.exists(log):
    os.remove(log)
proc = subprocess.Popen([server, "--config", conf, "-l", log], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
t0 = time.monotonic()
while f.attempt(port, t0) is None or "js_api" not in (f.attempt(port, t0) or {}):
    time.sleep(0.05)
out = {"mode": mode, "before": f.attempt(port, t0)}
try:
    if mode == "lameduck":
        sock = socket.create_connection(("127.0.0.1", port), timeout=5)
        buf = b""
        line, buf = f.read_line(sock, buf, time.monotonic() + 3)
        sock.sendall(f.CONNECT + b"PING\r\n")
        while f.read_line(sock, buf, time.monotonic() + 3)[0] != b"PONG":
            pass
        proc.send_signal(signal.SIGUSR2)
        t1 = time.monotonic()
        samples = []
        for _ in range(8):
            time.sleep(0.25)
            seen = f.attempt(port, t1)
            samples.append(seen if seen is None else {k: seen.get(k) for k in ("info_jetstream", "js_api", "error")})
        out["after_sigusr2_new_connections"] = samples
        try:
            sock.settimeout(1)
            out["old_connection_js_api"], _ = f.js_api_info(sock, b"", 2)
        except Exception as exc:
            out["old_connection_js_api"] = repr(exc)
    else:
        sock = socket.create_connection(("127.0.0.1", port), timeout=5)
        buf = b""
        line, buf = f.read_line(sock, buf, time.monotonic() + 3)
        sock.sendall(f.CONNECT.replace(b'"echo":false', b'"echo":true') + b"SUB _INBOX.fill 1\r\n")
        cfg = json.dumps({"name": "fill", "subjects": ["fill.>"], "storage": "file"}).encode()
        sock.sendall(("PUB $JS.API.STREAM.CREATE.fill _INBOX.fill %d\r\n" % len(cfg)).encode() + cfg + b"\r\n")
        line, buf = f.read_line(sock, buf, time.monotonic() + 5)
        n = int(line.split()[3]); buf = buf[n + 2:] if len(buf) >= n + 2 else buf
        payload = b"y" * 65536
        sent = 0
        errors = []
        deadline = time.monotonic() + 60
        while time.monotonic() < deadline and len(errors) < 3 and sent < 400:
            sock.sendall(("PUB fill.x _INBOX.fill %d\r\n" % len(payload)).encode() + payload + b"\r\n")
            sent += 1
            time.sleep(0.01)
            sock.setblocking(False)
            try:
                data = sock.recv(1 << 20)
                if data:
                    buf += data
            except BlockingIOError:
                pass
            sock.setblocking(True)
            while b"\r\n" in buf:
                line, _, buf = buf.partition(b"\r\n")
                if line.startswith(b"MSG "):
                    n = int(line.split()[3])
                    if len(buf) < n + 2:
                        buf = line + b"\r\n" + buf
                        break
                    body, buf = buf[:n], buf[n + 2:]
                    if b"error" in body:
                        errors.append(body.decode()[:200])
        out["messages_sent"] = sent
        out["first_publish_errors"] = errors
        out["disk_free_kib"] = shutil.disk_usage(store_dir).free // 1024
        time.sleep(1)
        out["after_fill"] = f.attempt(port, time.monotonic())
        out["after_fill_second_connection"] = f.attempt(port, time.monotonic())
finally:
    proc.terminate()
    proc.wait(timeout=30)
out["log_tail"] = [l.strip()[:200] for l in open(log).readlines()[-12:]]
print(json.dumps(out, indent=1))
