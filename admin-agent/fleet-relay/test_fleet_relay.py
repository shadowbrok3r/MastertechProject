import json
import os
import socket
import sqlite3
import tempfile
import threading
import time
import unittest
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import fleet_relay as fr

TOKEN = "test-relay-token-placeholder"
SPAN = (
    "session_loop{{thread_id={t}}}:submission_dispatch{{otel.name=\"op.dispatch.turn_input\"}}:turn{{thread.id={t} "
    "model=zc-heavy}}:stream_request:model_client.stream_responses{{model=zc-heavy wire_api=responses}}: "
)
SCHEMA = """
CREATE TABLE logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    ts INTEGER NOT NULL,
    ts_nanos INTEGER NOT NULL,
    level TEXT NOT NULL,
    target TEXT NOT NULL,
    feedback_log_body TEXT,
    module_path TEXT,
    file TEXT,
    line INTEGER,
    thread_id TEXT,
    process_uuid TEXT,
    estimated_bytes INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_logs_ts ON logs(ts DESC, ts_nanos DESC, id DESC);
CREATE INDEX idx_logs_thread_id ON logs(thread_id);
CREATE INDEX idx_logs_thread_id_ts ON logs(thread_id, ts DESC, ts_nanos DESC, id DESC);
"""


def completed(thread, backend, route):
    headers = {
        "date": "Wed, 14 Oct 2026 20:00:51 GMT",
        "x-litellm-call-id": "c0ffee",
        "x-litellm-model-group": "zc-heavy",
        "x-fleet-backend": backend,
        "x-fleet-route": route,
        "cache-control": "no-cache",
        "content-type": "text/event-stream",
    }
    return (
        SPAN.format(t=thread)
        + "Request completed method=POST url=http://127.0.0.1:4000/v1/responses status=200 OK headers="
        + json.dumps(headers)
        + " version=HTTP/1.1"
    )


def failed(thread):
    return SPAN.format(t=thread) + "Request failed method=POST url=http://127.0.0.1:4000/v1/responses error=timeout"


def add_row(conn, ts, thread, body, target="codex_http_client::client", line=252, level="DEBUG"):
    conn.execute(
        "INSERT INTO logs (ts, ts_nanos, level, target, feedback_log_body, module_path, file, line, thread_id) "
        "VALUES (?, ?, ?, ?, ?, ?, 'http-client/src/client.rs', ?, ?)",
        (ts, 500_000_000, level, target, body, target, line, thread),
    )


T_A = "01a13b91-0c66-7d52-b071-306b8cc3f350"
T_B = "01a13bab-d0fe-7511-8dc5-4166e6005de8"
T_C = "01a13b71-de5f-76e3-84ab-b6a07b43a71d"
T_D = "01a13b2a-2389-7cc2-b4da-371a00d2abc1"
T_E = "01a13741-490f-7961-a2e8-052af2a929f8"
BASE_TS = 1792008000


def build_fixture(path):
    conn = sqlite3.connect(path)
    conn.executescript(SCHEMA)
    add_row(conn, BASE_TS + 1, T_A, completed(T_A, "r9700", "compaction"))
    add_row(conn, BASE_TS + 51, T_A, completed(T_A, "node-128", "pinned"))
    add_row(conn, BASE_TS + 10, T_B, completed(T_B, "node-7900", "pinned"))
    add_row(conn, BASE_TS + 20, T_B, failed(T_B), line=267)
    add_row(conn, BASE_TS + 30, T_B, "turn finished", target="codex_core::session::turn", level="TRACE")
    add_row(conn, BASE_TS + 5, T_C, failed(T_C), line=267)
    add_row(conn, BASE_TS + 5, T_D, completed(T_D, "node-7900", "pinned"))
    add_row(conn, BASE_TS + 60, T_D, completed(T_D, "r9700", "compaction"))
    add_row(conn, BASE_TS + 70, T_E, completed(T_E, "node-gone", "spill"))
    add_row(conn, BASE_TS + 80, None, completed("none", "node-7900", "pinned"))
    conn.commit()
    conn.close()


LLAMA_ROUTES = {
    "/props": (
        200,
        {
            "model_path": "/home/x/models/Qwen3.8-27B-GGUF/Qwen3.8-27B-Q8_0.gguf",
            "build_info": "b142-d1d3c33",
            "model_ftype": "Q8_0",
            "default_generation_settings": {"n_ctx": 204800},
            "chat_template": "{{ secret template }}",
        },
    ),
    "/v1/models": (
        200,
        {"data": [{"id": "qwen3.8-uncensored:27b-agent", "owned_by": "llamacpp", "meta": {"n_ctx": 204800, "ftype": "Q8_0"}}]},
    ),
}
LLAMA_P_ROUTES = {
    "/props": (
        200,
        {
            "model_path": "/home/x/llm-models/Qwen3.8-27B-HauhauCS-Aggressive-Q6_K_P.gguf",
            "build_info": "b1-22397c31a",
            "default_generation_settings": {"n_ctx": 262144},
        },
    ),
    "/v1/models": (200, {"data": [{"id": "agent", "owned_by": "llamacpp", "meta": {"n_ctx": 262144, "ftype": "Q6_K"}}]}),
}
VLLM_ROUTES = {
    "/props": (404, {"detail": "Not Found"}),
    "/v1/models": (
        200,
        {
            "data": [
                {
                    "id": "qwen3.8-uncensored:27b-agent",
                    "owned_by": "vllm",
                    "root": "/models/Qwen3.8-27B-Uncensored-MXFP4-awq",
                    "max_model_len": 204800,
                }
            ]
        },
    ),
    "/version": (200, {"version": "0.27.1"}),
}
MODELS_ONLY_ROUTES = {
    "/v1/models": (200, {"data": [{"id": "gemma-3-27b", "owned_by": "llamacpp", "meta": {"n_ctx": 8192, "ftype": "Q4_K - Medium"}}]}),
}


def serve(routes):
    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            status, body = routes.get(self.path, (404, {"error": "nf"}))
            data = json.dumps(body).encode()
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        def log_message(self, *args):
            pass

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server, "http://127.0.0.1:{}".format(server.server_address[1])


def raw_server(reply):
    """Answers every connection with the raw bytes `reply`, then closes it."""
    sock = socket.socket()
    sock.bind(("127.0.0.1", 0))
    sock.listen()

    def run():
        while True:
            try:
                conn, _ = sock.accept()
            except OSError:
                return
            with conn:
                conn.recv(65536)
                conn.sendall(reply)

    threading.Thread(target=run, daemon=True).start()
    return sock, "http://127.0.0.1:{}".format(sock.getsockname()[1])


def dead_url():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        port = s.getsockname()[1]
    return "http://127.0.0.1:{}".format(port)


class QuantTests(unittest.TestCase):
    def test_names(self):
        cases = {
            "Qwen3.8-27B-Q8_0.gguf": "Q8_0",
            "Qwen3.8-27B-HauhauCS-Aggressive-Q6_K_P.gguf": "Q6_K_P",
            "Qwen3.8-27B-Uncensored-MXFP4-awq": "MXFP4 AWQ",
            "model-Q4_K_M.gguf": "Q4_K_M",
            "model-UD-Q4_K_XL.gguf": "Q4_K_XL",
            "model-Q5_K_S.gguf": "Q5_K_S",
            "model-q6_k.gguf": "Q6_K",
            "model-IQ4_XS.gguf": "IQ4_XS",
            "model.IQ3_XXS.gguf": "IQ3_XXS",
            "model-BF16.gguf": "BF16",
            "model-f16.gguf": "F16",
            "Qwen3-32B-FP8": "FP8",
            "Qwen3-32B-AWQ": "AWQ",
            "Qwen2.5-72B-Instruct-GPTQ-Int4": "GPTQ",
            "gpt-oss-20b-mxfp4.gguf": "MXFP4",
            "Qwen3.8-27B": None,
            "": None,
            None: None,
        }
        for name, want in cases.items():
            self.assertEqual(fr.quant_from_name(name), want, name)

    def test_ftype(self):
        self.assertEqual(fr.normalize_ftype("Q6_K"), "Q6_K")
        self.assertEqual(fr.normalize_ftype("Q4_K - Medium"), "Q4_K")
        self.assertEqual(fr.normalize_ftype("weird"), "weird")
        self.assertIsNone(fr.normalize_ftype(None))


class ExtractTests(unittest.TestCase):
    def test_completed(self):
        self.assertEqual(fr.extract_route(completed(T_A, "node-128", "pinned")), ("node-128", "pinned"))

    def test_failed(self):
        self.assertIsNone(fr.extract_route(failed(T_A)))

    def test_rejects_odd_values(self):
        self.assertIsNone(fr.extract_route(completed(T_A, "node 128<script>", "pinned")))

    def test_ignores_span_text_before_headers(self):
        body = 'x "x-fleet-backend": "fake"' + completed(T_A, "node-7900", "spill")
        self.assertEqual(fr.extract_route(body), ("node-7900", "spill"))


class FixtureCase(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.db = os.path.join(self.dir.name, "logs_2.sqlite")
        build_fixture(self.db)
        self.log = fr.CodexLog(self.db)

    def tearDown(self):
        self.dir.cleanup()


class CodexLogTests(FixtureCase):
    def test_latest(self):
        found = self.log.latest([T_A, T_B, T_C, T_D, "missing"])
        self.assertEqual(found[T_A][1:], ("node-128", "pinned"))
        self.assertAlmostEqual(found[T_A][0], BASE_TS + 51.5)
        self.assertEqual(found[T_B][1:], ("node-7900", "pinned"))
        self.assertNotIn(T_C, found)
        self.assertEqual(found[T_D][1:], ("r9700", "compaction"))
        self.assertNotIn("missing", found)

    def test_read_only(self):
        uri = "file:{}?mode=ro".format(self.db)
        conn = sqlite3.connect(uri, uri=True)
        with self.assertRaises(sqlite3.OperationalError):
            conn.execute("DELETE FROM logs")
        conn.close()

    def test_missing_file(self):
        with self.assertRaises(sqlite3.Error):
            fr.CodexLog(os.path.join(self.dir.name, "nope.sqlite")).latest([T_A])


class CodexHomeTests(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.home = self.dir.name
        self.logged = []
        self.saved_log = fr.log
        fr.log = self.logged.append

    def tearDown(self):
        fr.log = self.saved_log
        self.dir.cleanup()

    def test_newest_logs_db(self):
        self.assertIsNone(fr.newest_logs_db(self.home))
        for name in ("logs_2.sqlite", "logs_10.sqlite", "logs_11.sqlite-wal", "state_5.sqlite", "logs_x.sqlite"):
            open(os.path.join(self.home, name), "w").close()
        self.assertEqual(fr.newest_logs_db(self.home), os.path.join(self.home, "logs_10.sqlite"))
        self.assertIsNone(fr.newest_logs_db(os.path.join(self.home, "gone")))

    def test_reads_the_newest_file_and_logs_a_switch(self):
        build_fixture(os.path.join(self.home, "logs_2.sqlite"))
        codex_log = fr.CodexLog(home=self.home)
        self.assertEqual(codex_log.latest([T_B])[T_B][1:], ("node-7900", "pinned"))
        newer = os.path.join(self.home, "logs_3.sqlite")
        conn = sqlite3.connect(newer)
        conn.executescript(SCHEMA)
        add_row(conn, BASE_TS + 200, T_B, completed(T_B, "r9700", "spill"))
        conn.commit()
        conn.close()
        self.assertEqual(codex_log.latest([T_B])[T_B][1:], ("r9700", "spill"))
        self.assertEqual(self.logged, ["codex log switched to " + newer])

    def test_empty_home_is_unavailable(self):
        with self.assertRaises(sqlite3.Error):
            fr.CodexLog(home=self.home).latest([T_A])


class ProbeTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.servers = []
        cls.urls = {}
        for name, routes in (("llama", LLAMA_ROUTES), ("llama_p", LLAMA_P_ROUTES), ("vllm", VLLM_ROUTES), ("models", MODELS_ONLY_ROUTES)):
            server, url = serve(routes)
            cls.servers.append(server)
            cls.urls[name] = url

    @classmethod
    def tearDownClass(cls):
        for server in cls.servers:
            server.shutdown()
            server.server_close()

    def test_llama(self):
        got = fr.probe_node(self.urls["llama"], 2)
        self.assertEqual(
            got,
            {"engine": "llama.cpp", "build": "b142-d1d3c33", "model": "Qwen3.8-27B-Q8_0.gguf", "quant": "Q8_0", "context": 204800, "ok": True},
        )

    def test_llama_quant_from_file_beats_ftype(self):
        got = fr.probe_node(self.urls["llama_p"], 2)
        self.assertEqual(got["quant"], "Q6_K_P")
        self.assertEqual(got["context"], 262144)

    def test_vllm(self):
        got = fr.probe_node(self.urls["vllm"], 2)
        self.assertEqual(
            got,
            {"engine": "vllm", "build": "0.27.1", "model": "Qwen3.8-27B-Uncensored-MXFP4-awq", "quant": "MXFP4 AWQ", "context": 204800, "ok": True},
        )

    def test_models_only_uses_ftype(self):
        got = fr.probe_node(self.urls["models"], 2)
        self.assertEqual((got["engine"], got["model"], got["quant"], got["context"]), ("llama.cpp", "gemma-3-27b", "Q4_K", 8192))

    def test_dead(self):
        got = fr.probe_node(dead_url(), 1)
        self.assertFalse(got["ok"])
        self.assertIsNone(got["quant"])

    def test_broken_http_is_a_failed_probe(self):
        replies = (
            b"SSH-2.0-OpenSSH_9.6\r\n",
            b'HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n5\r\n{"a":',
        )
        for reply in replies:
            sock, url = raw_server(reply)
            try:
                self.assertEqual(fr.probe_node(url, 2), fr.failed_probe(), reply)
            finally:
                sock.close()


class NodeTableTests(unittest.TestCase):
    def test_one_failing_probe_does_not_skip_the_others(self):
        calls, logged = [], []

        def fake_probe(url, timeout):
            calls.append(url)
            if url == "http://a":
                raise RuntimeError("boom")
            return dict(fr.failed_probe(), ok=True, quant="Q8_0")

        saved = fr.probe_node, fr.log
        fr.probe_node, fr.log = fake_probe, logged.append
        try:
            table = fr.NodeTable({"a": "http://a", "b": "http://b"})
            table.probe_all()
        finally:
            fr.probe_node, fr.log = saved
        self.assertEqual(calls, ["http://a", "http://b"])
        self.assertFalse(table.get("a")["ok"])
        self.assertIsNotNone(table.get("a")["probed_at"])
        self.assertEqual(table.get("b")["quant"], "Q8_0")
        self.assertEqual(logged, ["probe a error: RuntimeError"])


def call(base, path, token=None):
    req = urllib.request.Request(base + path)
    if token:
        req.add_header("Authorization", "Bearer " + token)
    try:
        with urllib.request.urlopen(req, timeout=5) as resp:
            return resp.status, json.loads(resp.read())
    except urllib.error.HTTPError as err:
        with err:
            return err.code, json.loads(err.read())


class RelayHttpTests(FixtureCase):
    def setUp(self):
        super().setUp()
        self.node_servers = []
        nodes = {}
        for name, routes in (("node-7900", LLAMA_ROUTES), ("node-128", LLAMA_P_ROUTES), ("r9700", VLLM_ROUTES)):
            server, url = serve(routes)
            self.node_servers.append(server)
            nodes[name] = url
        nodes["comfyqwen"] = dead_url()
        self.table = fr.NodeTable(nodes, timeout=1)
        self.table.probe_all()
        self.relay = fr.Relay(TOKEN, self.log, self.table)
        self.server = fr.make_server(self.relay, "127.0.0.1", 0)
        threading.Thread(target=self.server.serve_forever, daemon=True).start()
        self.base = "http://127.0.0.1:{}".format(self.server.server_address[1])
        self.logged = []
        self.saved_log = fr.log
        fr.log = self.logged.append

    def tearDown(self):
        fr.log = self.saved_log
        self.server.shutdown()
        self.server.server_close()
        for server in self.node_servers:
            server.shutdown()
            server.server_close()
        super().tearDown()

    def test_health_needs_no_token(self):
        self.assertEqual(call(self.base, "/health"), (200, {"ok": True}))

    def test_auth(self):
        self.assertEqual(call(self.base, "/v1/routes?threads=" + T_A)[0], 401)
        self.assertEqual(call(self.base, "/v1/routes?threads=" + T_A, "wrong-token")[0], 401)
        self.assertEqual(call(self.base, "/v1/nodes")[0], 401)

    def test_validation(self):
        self.assertEqual(call(self.base, "/v1/routes?threads=abc,d%27e", TOKEN)[0], 400)
        self.assertEqual(call(self.base, "/v1/routes?threads=" + "a" * 65, TOKEN)[0], 400)
        many = ",".join("t{}".format(i) for i in range(201))
        self.assertEqual(call(self.base, "/v1/routes?threads=" + many, TOKEN)[0], 400)
        exactly = ",".join("t{}".format(i) for i in range(200))
        self.assertEqual(call(self.base, "/v1/routes?threads=" + exactly, TOKEN)[0], 200)

    def test_routes(self):
        status, body = call(self.base, "/v1/routes?threads={},{},{},{},{},{}".format(T_A, T_B, T_C, T_D, T_E, T_A), TOKEN)
        self.assertEqual(status, 200)
        self.assertRegex(body["checked_at"], r"^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ$")
        by = {t["thread"]: t for t in body["threads"]}
        self.assertEqual([t["thread"] for t in body["threads"]], [T_A, T_B, T_D, T_E])
        self.assertEqual(
            by[T_A],
            {"thread": T_A, "node": "node-128", "route": "pinned", "served_at": fr.rfc3339(BASE_TS + 51), "quant": "Q6_K_P", "model": "Qwen3.8-27B-HauhauCS-Aggressive-Q6_K_P.gguf"},
        )
        self.assertEqual((by[T_B]["node"], by[T_B]["quant"], by[T_B]["model"]), ("node-7900", "Q8_0", "Qwen3.8-27B-Q8_0.gguf"))
        self.assertEqual((by[T_D]["node"], by[T_D]["route"], by[T_D]["quant"]), ("r9700", "compaction", "MXFP4 AWQ"))
        self.assertEqual((by[T_E]["node"], by[T_E]["quant"], by[T_E]["model"]), ("node-gone", None, None))

    def test_unreachable_node_has_null_quant(self):
        conn = sqlite3.connect(self.db)
        add_row(conn, BASE_TS + 90, T_C, completed(T_C, "comfyqwen", "new"))
        conn.commit()
        conn.close()
        status, body = call(self.base, "/v1/routes?threads=" + T_C, TOKEN)
        self.assertEqual(status, 200)
        self.assertEqual(body["threads"][0]["node"], "comfyqwen")
        self.assertIsNone(body["threads"][0]["quant"])
        self.assertIsNone(body["threads"][0]["model"])

    def test_empty(self):
        self.assertEqual(call(self.base, "/v1/routes?threads=", TOKEN)[1]["threads"], [])

    def test_nodes(self):
        status, body = call(self.base, "/v1/nodes", TOKEN)
        self.assertEqual(status, 200)
        nodes = {n["node"]: n for n in body["nodes"]}
        self.assertEqual(set(nodes), {"node-7900", "node-128", "r9700", "comfyqwen"})
        self.assertEqual(set(nodes["r9700"]), {"node", "url", "engine", "build", "model", "quant", "context", "ok", "probed_at"})
        self.assertTrue(nodes["node-7900"]["ok"])
        self.assertEqual(nodes["node-7900"]["engine"], "llama.cpp")
        self.assertEqual(nodes["r9700"]["build"], "0.27.1")
        self.assertFalse(nodes["comfyqwen"]["ok"])
        self.assertIsNone(nodes["comfyqwen"]["quant"])
        self.assertIsNotNone(nodes["comfyqwen"]["probed_at"])

    def test_not_found_and_method(self):
        self.assertEqual(call(self.base, "/v1/other", TOKEN)[0], 404)
        req = urllib.request.Request(self.base + "/v1/routes", data=b"{}", method="POST")
        with self.assertRaises(urllib.error.HTTPError) as ctx:
            urllib.request.urlopen(req, timeout=5)
        ctx.exception.close()
        self.assertEqual(ctx.exception.code, 405)

    def test_log_lines_omit_token(self):
        call(self.base, "/v1/routes?threads=" + T_A, TOKEN)
        call(self.base, "/v1/nodes", TOKEN)
        deadline = time.monotonic() + 2
        while len(self.logged) < 2 and time.monotonic() < deadline:
            time.sleep(0.01)
        self.assertEqual(len(self.logged), 2)
        self.assertTrue(all(TOKEN not in line for line in self.logged))
        self.assertIn("GET /v1/routes 200", self.logged[0])

    def test_db_unavailable(self):
        self.relay.codex_log = fr.CodexLog(os.path.join(self.dir.name, "gone.sqlite"))
        self.assertEqual(call(self.base, "/v1/routes?threads=" + T_A, TOKEN)[0], 503)


class CacheTests(FixtureCase):
    def test_route_cache(self):
        now = [100.0]
        relay = fr.Relay(TOKEN, self.log, fr.NodeTable({}), clock=lambda: now[0])
        first = relay.routes([T_B])
        self.assertEqual(first["threads"][0]["node"], "node-7900")
        conn = sqlite3.connect(self.db)
        add_row(conn, BASE_TS + 99, T_B, completed(T_B, "r9700", "spill"))
        conn.commit()
        conn.close()
        now[0] += 2
        self.assertIs(relay.routes([T_B]), first)
        now[0] += 2
        self.assertEqual(relay.routes([T_B])["threads"][0]["node"], "r9700")


class ConfigTests(unittest.TestCase):
    def test_nodes_file(self):
        with tempfile.TemporaryDirectory() as d:
            path = os.path.join(d, "nodes.json")
            with open(path, "w", encoding="utf-8") as fh:
                json.dump({"a": "http://10.0.0.1:1/"}, fh)
            self.assertEqual(fr.load_nodes(path), {"a": "http://10.0.0.1:1"})
            with open(path, "w", encoding="utf-8") as fh:
                json.dump({"a": "ftp://x"}, fh)
            with self.assertRaises(ValueError):
                fr.load_nodes(path)
        self.assertEqual(fr.load_nodes(None), fr.DEFAULT_NODES)

    def test_token_file(self):
        with tempfile.TemporaryDirectory() as d:
            path = os.path.join(d, "token")
            with open(path, "w", encoding="utf-8") as fh:
                fh.write("short\n")
            with self.assertRaises(ValueError):
                fr.load_token(path)
            with open(path, "w", encoding="utf-8") as fh:
                fh.write(TOKEN + "\n")
            self.assertEqual(fr.load_token(path), TOKEN)

    def test_authorized(self):
        relay = fr.Relay(TOKEN, None, fr.NodeTable({}))
        self.assertTrue(relay.authorized("Bearer " + TOKEN))
        self.assertFalse(relay.authorized(TOKEN))
        self.assertFalse(relay.authorized("Bearer " + TOKEN + "x"))
        self.assertFalse(relay.authorized(None))


if __name__ == "__main__":
    unittest.main()
