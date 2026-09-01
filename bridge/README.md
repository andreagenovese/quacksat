# bridge

Reference server for the quacksat agent protocol
(`docs/agent-protocol.md`): WebSocket in, STT → LLM (tool calling) →
TTS out, all three as OpenAI-dialect url+key endpoints. Minimal by
design — bring your own agent. Italian copy: `README.it.md`.

## Run

```sh
python3 -m venv .venv && .venv/bin/pip install websockets
.venv/bin/python bridge.py --fake            # no AI services needed
.venv/bin/python bridge.py --config config.toml
```

Point the satellite at it:

```toml
backend = "agent"
[agent]
url = "ws://<bridge-host>:8765"
```

`--fake` answers every utterance with a canned reply and a test tone,
and exercises the tool path by calling `robot.sound` once per turn —
the full protocol loop with zero dependencies.

## Profiles

- **Generic provider with tool calling** (`tool_calling = true`): the
  bridge runs the loop; the satellite's tool catalog is declared as
  OpenAI tools and `tool_calls` are forwarded as `tool.call`.
- **[Arkimede](https://arkimede.ai/)**: LLM/STT/TTS all on `http://<server>:3000/api/openai/v1`
  with an `ak_` key; set `tool_calling = false` (its shim keeps tools
  internal — robot tools reach it via the MCP server below).
- **Bare LLM**: works too; voice-only.

## MCP server (robot tools for MCP-native agents)

With `[mcp] enabled = true` (requires `pip install "mcp>=2" uvicorn`)
the bridge exposes the satellite's tool catalog as an MCP server at
`http://<host>:8766/mcp` (Streamable HTTP). Register it in your agent
platform (e.g. Arkimede's MCP servers) and the agent calls the robot
from inside its own loop — tool names use underscores (`robot_move`);
results carry the satellite's `tool.result` JSON. With no satellite
connected, calls answer `no satellite connected`. Like the WebSocket,
the endpoint is meant for a trusted LAN (no auth of its own).

Several ducks can share one bridge: give each a unique `[agent] name`
in its satellite config. Wakes landing within `[behavior] wake_window`
(default 250 ms) compete and the highest score wins — the losers get
`listen.stop`. With more than one duck connected, every MCP tool gains
a mandatory `duck` argument (an enum of the connected names); a call
without it is refused with the list.
