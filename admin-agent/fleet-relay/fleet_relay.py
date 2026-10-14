#!/usr/bin/env python3
"""Read-only relay: which fleet node served each Codex thread, and which quant that node runs."""

import hmac
import http.client
import json
import os
import re
import sqlite3
import sys
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
from datetime import datetime, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

DEFAULT_BIND = "192.168.22.7:7431"
DEFAULT_TOKEN_FILE = "~/.config/fleet-relay/token"
DEFAULT_CODEX_HOME = "~/.config/zc-codexd/codex-home"
DEFAULT_NODES = {
    "node-7900": "http://100.80.202.35:11436",
    "node-128": "http://100.116.134.128:11436",
    "comfyqwen": "http://100.74.123.108:11436",
    "r9700": "http://100.96.121.37:11436",
}
PROBE_INTERVAL_SECS = 60
PROBE_TIMEOUT_SECS = 3
ROUTE_CACHE_SECS = 3
ROUTE_CACHE_MAX = 64
MAX_THREADS = 200

THREAD_ID = re.compile(r"[A-Za-z0-9-]{1,64}")
LOGS_DB = re.compile(r"logs_(\d{1,6})\.sqlite")
HEADER_TARGET = "codex_http_client::client"
HEADER_VALUE = r'"{}":\s*"([A-Za-z0-9._:-]{{1,64}})"'
BACKEND_RE = re.compile(HEADER_VALUE.format("x-fleet-backend"))
ROUTE_RE = re.compile(HEADER_VALUE.format("x-fleet-route"))
LATEST_HEADER_SQL = (
    "SELECT ts, ts_nanos, feedback_log_body FROM logs "
    "WHERE thread_id = ? AND target = ? AND feedback_log_body LIKE '%x-fleet-backend%' "
    "ORDER BY ts DESC, ts_nanos DESC, id DESC LIMIT 5"
)

GGUF_QUANT = re.compile(
    r"(?:^|[-_.\s/])(IQ\d_[A-Z]{1,3}|Q\d_K(?:_(?:XL|S|M|L|P))?|Q\d_[01]|BF16|F16|F32)(?=$|[-_.\s])",
    re.IGNORECASE,
)
FLOAT_FORMAT = re.compile(r"(?:^|[-_.\s/])(MXFP4|NVFP4|FP8|FP16)(?=$|[-_.\s])", re.IGNORECASE)
QUANT_METHOD = re.compile(r"(?:^|[-_.\s/])(AWQ|GPTQ)(?=$|[-_.\s])", re.IGNORECASE)


def rfc3339(epoch):
    return datetime.fromtimestamp(int(epoch), timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def quant_from_name(name):
    """Quant label from a model file or directory name, or None."""
    if not name:
        return None
    m = GGUF_QUANT.search(name)
    if m:
        return m.group(1).upper()
    parts = [m.group(1).upper() for m in (FLOAT_FORMAT.search(name), QUANT_METHOD.search(name)) if m]
    return " ".join(parts) or None


def normalize_ftype(ftype):
    """Quant label from llama.cpp's ftype string, or None."""
    if not isinstance(ftype, str) or not ftype.strip():
        return None
    return quant_from_name(ftype.strip().split(" ")[0]) or clean(ftype)


def clean(value, limit=128):
    """Printable string capped at limit, or None."""
    if value is None:
        return None
    text = "".join(ch for ch in str(value) if ch.isprintable()).strip()
    return text[:limit] or None


def basename(path):
    return clean(str(path).rstrip("/").rsplit("/", 1)[-1]) if path else None


def as_int(value):
    return value if isinstance(value, int) and not isinstance(value, bool) else None


def extract_route(body):
    """(backend, route) from a Codex 'Request completed' log body, or None."""
    if not body:
        return None
    at = body.find("headers={")
    headers = body[at:] if at >= 0 else body
    backend = BACKEND_RE.search(headers)
    if not backend:
        return None
    route = ROUTE_RE.search(headers)
    return backend.group(1), route.group(1) if route else None


def newest_logs_db(home):
    """Path of the highest-numbered logs_<n>.sqlite in home, or None."""
    try:
        names = os.listdir(home)
    except OSError:
        return None
    found = [(int(m.group(1)), name) for name in names if (m := LOGS_DB.fullmatch(name))]
    return os.path.join(home, max(found)[1]) if found else None


class CodexLog:
    """Read-only view of one Codex log file, or of the newest logs_<n>.sqlite in a Codex home."""

    def __init__(self, path=None, home=None):
        self.path = path
        self.home = home
        self.lock = threading.Lock()
        self.current = None

    def resolve(self):
        """The file to read; logs a switch to another file."""
        path = self.path or (newest_logs_db(self.home) if self.home else None)
        if not path:
            raise sqlite3.OperationalError("no logs_<n>.sqlite in codex home")
        with self.lock:
            previous, self.current = self.current, path
        if previous and previous != path:
            log("codex log switched to {}".format(path))
        return path

    def latest(self, thread_ids):
        """Most recent fleet header row per thread id."""
        uri = "file:{}?mode=ro".format(urllib.parse.quote(self.resolve()))
        conn = sqlite3.connect(uri, uri=True, timeout=2)
        try:
            conn.execute("PRAGMA query_only = 1")
            found = {}
            for thread in thread_ids:
                for ts, ts_nanos, body in conn.execute(LATEST_HEADER_SQL, (thread, HEADER_TARGET)).fetchall():
                    hit = extract_route(body)
                    if hit:
                        found[thread] = (ts + ts_nanos / 1e9, hit[0], hit[1])
                        break
            return found
        finally:
            conn.close()


def fetch_json(url, timeout):
    """(status, parsed JSON or None); status None when the node did not answer."""
    req = urllib.request.Request(url, headers={"Accept": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            raw = resp.read(4 * 1024 * 1024)
            status = resp.status
    except urllib.error.HTTPError as err:
        return err.code, None
    except (OSError, ValueError, http.client.HTTPException):
        return None, None
    try:
        return status, json.loads(raw)
    except ValueError:
        return status, None


def first_model(models):
    data = models.get("data") if isinstance(models, dict) else None
    return data[0] if isinstance(data, list) and data and isinstance(data[0], dict) else {}


def failed_probe():
    return {"engine": None, "build": None, "model": None, "quant": None, "context": None, "ok": False}


def probe_node(url, timeout=PROBE_TIMEOUT_SECS):
    """Engine, build, model, quant and context of one inference server."""
    out = failed_probe()
    status, props = fetch_json(url + "/props", timeout)
    if status == 200 and isinstance(props, dict) and props.get("model_path"):
        settings = props.get("default_generation_settings") or {}
        _, models = fetch_json(url + "/v1/models", timeout)
        entry = first_model(models)
        meta = entry.get("meta") if isinstance(entry.get("meta"), dict) else {}
        model = basename(props["model_path"])
        out.update(
            engine="llama.cpp",
            build=clean(props.get("build_info")),
            model=model,
            quant=quant_from_name(model) or normalize_ftype(meta.get("ftype")) or normalize_ftype(props.get("model_ftype")),
            context=as_int(settings.get("n_ctx")) or as_int(meta.get("n_ctx")),
            ok=True,
        )
        return out
    status, models = fetch_json(url + "/v1/models", timeout)
    if status != 200:
        return out
    entry = first_model(models)
    if entry.get("owned_by") == "vllm" or "root" in entry or "max_model_len" in entry:
        _, version = fetch_json(url + "/version", timeout)
        model = basename(entry.get("root") or entry.get("id"))
        out.update(
            engine="vllm",
            build=clean(version.get("version")) if isinstance(version, dict) else None,
            model=model,
            quant=quant_from_name(model),
            context=as_int(entry.get("max_model_len")),
            ok=True,
        )
        return out
    meta = entry.get("meta") if isinstance(entry.get("meta"), dict) else {}
    model = basename(entry.get("id"))
    out.update(
        engine="llama.cpp" if entry.get("owned_by") == "llamacpp" else clean(entry.get("owned_by")),
        model=model,
        quant=quant_from_name(model) or normalize_ftype(meta.get("ftype")),
        context=as_int(meta.get("n_ctx")),
        ok=True,
    )
    return out


class NodeTable:
    """Cached probe results per node."""

    def __init__(self, nodes, timeout=PROBE_TIMEOUT_SECS):
        self.nodes = dict(nodes)
        self.timeout = timeout
        self.lock = threading.Lock()
        self.state = {name: None for name in self.nodes}

    def probe_all(self):
        for name, url in self.nodes.items():
            try:
                result = probe_node(url, self.timeout)
            except Exception as err:
                log("probe {} error: {}".format(name, type(err).__name__))
                result = failed_probe()
            result["probed_at"] = rfc3339(time.time())
            with self.lock:
                self.state[name] = result

    def run(self, interval=PROBE_INTERVAL_SECS, stop=None):
        stop = stop or threading.Event()
        while not stop.is_set():
            try:
                self.probe_all()
            except Exception as err:
                log("probe error: {}".format(type(err).__name__))
            stop.wait(interval)

    def get(self, name):
        with self.lock:
            return self.state.get(name)

    def listing(self):
        rows = []
        with self.lock:
            for name, url in self.nodes.items():
                found = self.state.get(name) or {}
                rows.append(
                    {
                        "node": name,
                        "url": url,
                        "engine": found.get("engine"),
                        "build": found.get("build"),
                        "model": found.get("model"),
                        "quant": found.get("quant"),
                        "context": found.get("context"),
                        "ok": bool(found.get("ok")),
                        "probed_at": found.get("probed_at"),
                    }
                )
        return rows


class Relay:
    """Request handling independent of the HTTP server."""

    def __init__(self, token, codex_log, nodes, clock=time.monotonic):
        self.token = token.encode()
        self.codex_log = codex_log
        self.nodes = nodes
        self.clock = clock
        self.cache = {}
        self.cache_lock = threading.Lock()

    def authorized(self, header):
        if not header or not header.startswith("Bearer "):
            return False
        return hmac.compare_digest(header[7:].strip().encode(), self.token)

    @staticmethod
    def parse_threads(query):
        """Ordered unique thread ids, or an error message."""
        raw = ",".join(urllib.parse.parse_qs(query).get("threads", []))
        ids = []
        for part in raw.split(","):
            part = part.strip()
            if not part:
                continue
            if not THREAD_ID.fullmatch(part):
                return None, "invalid thread id"
            if part not in ids:
                ids.append(part)
        if len(ids) > MAX_THREADS:
            return None, "too many thread ids (max {})".format(MAX_THREADS)
        return ids, None

    def routes(self, ids):
        key = tuple(sorted(ids))
        now = self.clock()
        with self.cache_lock:
            hit = self.cache.get(key)
            if hit and now - hit[0] < ROUTE_CACHE_SECS:
                return hit[1]
        found = self.codex_log.latest(ids) if ids else {}
        threads = []
        for thread in ids:
            if thread not in found:
                continue
            served, node, route = found[thread]
            probe = self.nodes.get(node) or {}
            ok = bool(probe.get("ok"))
            threads.append(
                {
                    "thread": thread,
                    "node": node,
                    "route": route,
                    "served_at": rfc3339(served),
                    "quant": probe.get("quant") if ok else None,
                    "model": probe.get("model") if ok else None,
                }
            )
        answer = {"checked_at": rfc3339(time.time()), "threads": threads}
        with self.cache_lock:
            self.cache = {k: v for k, v in self.cache.items() if now - v[0] < ROUTE_CACHE_SECS}
            if len(self.cache) < ROUTE_CACHE_MAX:
                self.cache[key] = (now, answer)
        return answer

    def node_listing(self):
        return {"checked_at": rfc3339(time.time()), "nodes": self.nodes.listing()}

    def handle(self, path, query, auth_header):
        """(status, body dict, extra headers) for a GET."""
        if path == "/health":
            return 200, {"ok": True}, {}
        if path not in ("/v1/routes", "/v1/nodes"):
            return 404, {"error": "not found"}, {}
        if not self.authorized(auth_header):
            return 401, {"error": "unauthorized"}, {"WWW-Authenticate": "Bearer"}
        if path == "/v1/nodes":
            return 200, self.node_listing(), {}
        ids, error = self.parse_threads(query)
        if error:
            return 400, {"error": error}, {}
        try:
            return 200, self.routes(ids), {}
        except sqlite3.Error as err:
            log("codex log error: {}".format(err))
            return 503, {"error": "codex log unavailable"}, {}


def log(line):
    sys.stderr.write("{} {}\n".format(rfc3339(time.time()), line))
    sys.stderr.flush()


def make_handler(relay):
    class Handler(BaseHTTPRequestHandler):
        server_version = "fleet-relay"
        sys_version = ""
        protocol_version = "HTTP/1.1"
        timeout = 10

        def do_GET(self):
            started = time.perf_counter()
            parsed = urllib.parse.urlsplit(self.path)
            status, body, headers = relay.handle(parsed.path, parsed.query, self.headers.get("Authorization"))
            self.reply(status, body, headers)
            count = len(body.get("threads") or body.get("nodes") or [])
            log(
                "{} GET {} {} {}ms items={}".format(
                    self.client_address[0], parsed.path, status, round((time.perf_counter() - started) * 1000), count
                )
            )

        def reply(self, status, body, headers):
            data = json.dumps(body, separators=(",", ":")).encode()
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Cache-Control", "no-store")
            self.send_header("Content-Length", str(len(data)))
            for name, value in headers.items():
                self.send_header(name, value)
            self.end_headers()
            self.wfile.write(data)

        def do_POST(self):
            self.close_connection = True
            self.reply(405, {"error": "method not allowed"}, {"Allow": "GET", "Connection": "close"})

        do_PUT = do_DELETE = do_PATCH = do_POST

        def log_message(self, format, *args):
            pass

    return Handler


def make_server(relay, host, port):
    server = ThreadingHTTPServer((host, port), make_handler(relay))
    server.daemon_threads = True
    return server


def load_nodes(path):
    if not path:
        return dict(DEFAULT_NODES)
    with open(os.path.expanduser(path), encoding="utf-8") as fh:
        nodes = json.load(fh)
    if not isinstance(nodes, dict) or not nodes:
        raise ValueError("nodes file must be a JSON object of name -> url")
    for name, url in nodes.items():
        if not isinstance(url, str) or not url.startswith(("http://", "https://")):
            raise ValueError("bad url for node {}".format(name))
    return {name: url.rstrip("/") for name, url in nodes.items()}


def load_token(path):
    with open(os.path.expanduser(path), encoding="utf-8") as fh:
        token = fh.read().strip()
    if len(token) < 16:
        raise ValueError("token file holds fewer than 16 characters")
    return token


def main():
    bind = os.environ.get("FLEET_RELAY_BIND", DEFAULT_BIND)
    host, _, port = bind.rpartition(":")
    try:
        token = load_token(os.environ.get("FLEET_RELAY_TOKEN_FILE", DEFAULT_TOKEN_FILE))
        nodes = NodeTable(load_nodes(os.environ.get("FLEET_RELAY_NODES")))
    except (OSError, ValueError) as err:
        log("startup failed: {}".format(err))
        return 1
    explicit = os.environ.get("FLEET_RELAY_CODEX_LOG")
    codex_log = CodexLog(
        os.path.expanduser(explicit) if explicit else None,
        os.path.expanduser(os.environ.get("FLEET_RELAY_CODEX_HOME", DEFAULT_CODEX_HOME)),
    )
    relay = Relay(token, codex_log, nodes)
    try:
        server = make_server(relay, host, int(port))
    except OSError as err:
        log("bind {} failed: {}".format(bind, err))
        return 1
    threading.Thread(target=nodes.run, name="probe", daemon=True).start()
    log(
        "listening on {}:{} nodes={} codex={}".format(
            host, port, ",".join(nodes.nodes), codex_log.path or newest_logs_db(codex_log.home)
        )
    )
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
