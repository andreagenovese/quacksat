"""MCP server: the bridge's tool core exposed to MCP-native agents.

Serves the connected satellite's declared tools over MCP (Streamable
HTTP) so an agent platform — Arkimede, or anything MCP-capable — can
call the robot from inside its own loop (ADR 0004 §5). Tool listings
mirror the satellite's `session.start` catalog verbatim; calls are
forwarded over the satellite WebSocket and answered with the
`tool.result` payload as JSON text.

Optional: requires `pip install "mcp>=2" uvicorn`. Enabled by the
`[mcp]` section in config.toml; the endpoint is `http://host:port/mcp`.
"""

import json
import logging

import mcp.types as types
import uvicorn
from mcp.server import Server
from mcp.server.transport_security import TransportSecuritySettings

log = logging.getLogger("bridge.mcp")
# Stateless streamable-http tears a session down per request; the INFO
# noise ("Terminating session: None") is not worth reading.
logging.getLogger("mcp.server.streamable_http").setLevel(logging.WARNING)


def build_server(registry):
    server = Server("quacksat-robot")

    async def list_tools(ctx, params):
        session = registry.get("session")
        tools = (
            []
            if session is None
            else [
                types.Tool(
                    # MCP tool names avoid dots; the wire names use them.
                    name=tool["name"].replace(".", "_"),
                    description=tool.get("description", ""),
                    inputSchema=tool.get("parameters", {"type": "object", "properties": {}}),
                )
                for tool in session.tools
            ]
        )
        return types.ListToolsResult(tools=tools)

    async def call_tool(ctx, params):
        session = registry.get("session")
        if session is None:
            payload = {"ok": False, "error": "no satellite connected"}
        else:
            wire_name = params.name.replace("_", ".", 1)
            log.info("mcp tool call: %s %s", wire_name, params.arguments)
            payload = await session.call_tool(wire_name, params.arguments or {})
        return types.CallToolResult(
            content=[types.TextContent(type="text", text=json.dumps(payload))],
            is_error=not payload.get("ok", False),
        )

    server.add_request_handler("tools/list", types.RequestParams, list_tools)
    server.add_request_handler("tools/call", types.CallToolRequestParams, call_tool)
    return server


async def serve(registry, bind, port):
    server = build_server(registry)
    app = server.streamable_http_app(
        streamable_http_path="/mcp",
        stateless_http=True,
        # LAN service called by hostname/IP: the Host allow-list would
        # reject anything but localhost otherwise.
        transport_security=TransportSecuritySettings(enable_dns_rebinding_protection=False),
    )
    config = uvicorn.Config(app, host=bind, port=port, log_level="warning")
    log.info("mcp server listening on http://%s:%s/mcp", bind, port)
    await uvicorn.Server(config).serve()
