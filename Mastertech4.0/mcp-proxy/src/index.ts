#!/usr/bin/env node
/**
 * Mastertech MCP reconnecting proxy.
 *
 * Claude Desktop spawns this once over stdio. It forwards tools/list and
 * tools/call to the running Mastertech app's Streamable-HTTP MCP at
 * http://127.0.0.1:9004/mcp, transparently reconnecting whenever Mastertech is
 * recompiled/restarted. The stdio session to Claude Desktop stays alive across
 * those restarts, so Claude Desktop never needs to be reopened.
 *
 * Because this process is NOT the Mastertech binary, recompiling Mastertech
 * never locks or kills it.
 *
 * Everything logs to stderr — stdout carries the MCP stdio framing and must
 * stay clean.
 */
import { Server } from "@modelcontextprotocol/sdk/server/index.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StreamableHTTPClientTransport } from "@modelcontextprotocol/sdk/client/streamableHttp.js";
import {
  CallToolRequestSchema,
  ListToolsRequestSchema,
  type CallToolResult,
  type Tool,
} from "@modelcontextprotocol/sdk/types.js";

const UPSTREAM_URL = process.env.MASTERTECH_MCP_URL ?? "http://127.0.0.1:9004/mcp";
const POLL_MS = Number(process.env.MASTERTECH_MCP_POLL_MS ?? 8000);

const log = (...args: unknown[]) => console.error("[mastertech-proxy]", ...args);
const describe = (err: unknown): string => (err instanceof Error ? err.message : String(err));

/**
 * Lazily-(re)connected downstream client to Mastertech's :9004 MCP. The session
 * dies whenever Mastertech restarts; `run` recovers by tearing down the stale
 * client and re-initializing a fresh session, then retrying the call once.
 */
class Upstream {
  private client: Client | null = null;
  private connecting: Promise<Client> | null = null;

  get url(): string {
    return UPSTREAM_URL;
  }

  private async connect(): Promise<Client> {
    const transport = new StreamableHTTPClientTransport(new URL(UPSTREAM_URL));
    const client = new Client({ name: "mastertech-proxy", version: "1.0.0" }, { capabilities: {} });
    await client.connect(transport); // performs initialize + notifications/initialized
    this.client = client;
    log(`connected to ${UPSTREAM_URL}`);
    return client;
  }

  private async ensure(): Promise<Client> {
    if (this.client) return this.client;
    if (!this.connecting) {
      this.connecting = this.connect().finally(() => {
        this.connecting = null;
      });
    }
    return this.connecting;
  }

  private async reset(): Promise<void> {
    const c = this.client;
    this.client = null;
    if (c) {
      try {
        await c.close();
      } catch {
        /* already gone */
      }
    }
  }

  /** Run `fn` against the live client; on failure, reconnect once and retry. */
  async run<T>(fn: (c: Client) => Promise<T>): Promise<T> {
    try {
      return await fn(await this.ensure());
    } catch (err) {
      log(`downstream call failed (${describe(err)}); reconnecting…`);
      await this.reset();
      return await fn(await this.ensure());
    }
  }
}

const upstream = new Upstream();

/** Tools the proxy serves itself; always available even when Mastertech is down. */
const PROXY_TOOLS: Tool[] = [
  {
    name: "mastertech_proxy_status",
    description:
      "Report whether the proxy can currently reach the running Mastertech app's MCP server, and the upstream URL. Works even when Mastertech is not running.",
    inputSchema: { type: "object", properties: {}, additionalProperties: false },
  },
];

const server = new Server(
  { name: "mastertech", version: "1.0.0" },
  { capabilities: { tools: { listChanged: true } } },
);

server.setRequestHandler(ListToolsRequestSchema, async () => {
  try {
    const { tools } = await upstream.run((c) => c.listTools());
    return { tools: [...tools, ...PROXY_TOOLS] };
  } catch (err) {
    log(`listTools: Mastertech unreachable (${describe(err)}); advertising proxy tools only`);
    return { tools: PROXY_TOOLS };
  }
});

server.setRequestHandler(CallToolRequestSchema, async (req): Promise<CallToolResult> => {
  const { name, arguments: args } = req.params;

  if (name === "mastertech_proxy_status") {
    try {
      await upstream.run((c) => c.listTools());
      return { content: [{ type: "text", text: `Mastertech MCP reachable at ${upstream.url}.` }] };
    } catch (err) {
      return {
        content: [
          {
            type: "text",
            text: `Mastertech MCP NOT reachable at ${upstream.url} (${describe(err)}). Start or recompile-run the Mastertech app; the proxy reconnects automatically.`,
          },
        ],
      };
    }
  }

  try {
    return (await upstream.run((c) => c.callTool({ name, arguments: args ?? {} }))) as CallToolResult;
  } catch (err) {
    return {
      isError: true,
      content: [
        {
          type: "text",
          text: `Mastertech MCP unavailable for tool '${name}' at ${upstream.url} (${describe(err)}). The app may be recompiling — it will reconnect automatically once it is running again.`,
        },
      ],
    };
  }
});

/**
 * Background watcher: when Mastertech comes up after being down (e.g. it wasn't
 * running when Claude Desktop started), tell the client to re-list tools so the
 * full Mastertech tool set appears without reopening Claude Desktop.
 */
function startToolWatcher(): void {
  let lastCount = -1;
  const tick = async () => {
    let count = 0;
    try {
      const { tools } = await upstream.run((c) => c.listTools());
      count = tools.length;
    } catch {
      count = 0; // Mastertech down → only proxy tools effectively available.
    }
    if (count !== lastCount && lastCount !== -1) {
      log(`tool set changed (${lastCount} → ${count}); notifying client`);
      try {
        await server.notification({ method: "notifications/tools/list_changed" });
      } catch (err) {
        log(`list_changed notify failed: ${describe(err)}`);
      }
    }
    lastCount = count;
  };
  setInterval(() => void tick(), POLL_MS).unref();
}

async function main(): Promise<void> {
  const transport = new StdioServerTransport();
  await server.connect(transport);
  log(`stdio proxy ready → ${UPSTREAM_URL}`);
  startToolWatcher();
}

main().catch((err) => {
  log(`fatal: ${describe(err)}`);
  process.exit(1);
});
