# Mastertech MCP reconnecting proxy

A tiny persistent stdio MCP server that Claude Desktop (or Cursor / Claude Code)
spawns **once**. It forwards every `tools/list` and `tools/call` to the running
Mastertech app's Streamable-HTTP MCP at `http://127.0.0.1:9004/mcp` and
**auto-reconnects** whenever Mastertech is recompiled and restarted.

## Why

Mastertech hosts its MCP server inside the GUI process. Recompiling + restarting
the app drops the MCP session, and Claude Desktop won't re-establish it until you
reopen Claude Desktop. This proxy is a **separate process** (Node, not the Rust
binary), so:

- Recompiling Mastertech never locks or kills it.
- Claude Desktop's stdio connection to the proxy stays alive across restarts.
- When Mastertech goes away (mid-recompile) and comes back, the proxy mints a
  fresh downstream session and keeps serving — no reopening required.

## Setup (once)

Requires Node 18+.

```sh
cd Mastertech4.0/mcp-proxy
npm install
npm run build
```

## Claude Desktop config

Edit `claude_desktop_config.json` (Settings → Developer → Edit Config) and point
the `mastertech` server at the proxy instead of the Mastertech binary. Keep the
key `mastertech` so tool names stay `mcp__mastertech__*`:

```json
{
  "mcpServers": {
    "mastertech": {
      "command": "node",
      "args": [
        "C:\\Path\\To\\mcp-proxy\\dist\\index.js"
      ]
    }
  }
}
```

Restart Claude Desktop once. After that, recompile + run Mastertech freely — the
tools keep working without reopening Claude Desktop.

## Notes

- Downstream URL override: set `MASTERTECH_MCP_URL` (default
  `http://127.0.0.1:9004/mcp`).
- Reconnect/refresh poll interval: `MASTERTECH_MCP_POLL_MS` (default `8000`).
- The proxy adds one extra tool, `mastertech_proxy_status`, which reports whether
  Mastertech is currently reachable — handy when a tool call says the app is down.
- If Mastertech is **not** running when Claude Desktop starts, only the proxy's
  own tools list initially; once you launch Mastertech the proxy detects it and
  emits `tools/list_changed` so the full tool set appears.
