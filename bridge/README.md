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
- **Arkimede**: LLM/STT/TTS all on `http://<server>:3000/api/openai/v1`
  with an `ak_` key; set `tool_calling = false` (its shim keeps tools
  internal — robot tools reach it via MCP, phase 2).
- **Bare LLM**: works too; voice-only.

The MCP server (exposing the satellite's tools to MCP-native agents
like Arkimede) is the next planned piece — see
`docs/agent-backend-plan.md`.
