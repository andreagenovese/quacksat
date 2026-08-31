# bridge

Server di riferimento per il protocollo agent di quacksat
(`docs/agent-protocol.md`): WebSocket in ingresso, STT → LLM (tool
calling) → TTS in uscita, tutti e tre come endpoint url+key in dialetto
OpenAI. Minimale di proposito — bring your own agent. Copia inglese
(canonica): `README.md`.

## Avvio

```sh
python3 -m venv .venv && .venv/bin/pip install websockets
.venv/bin/python bridge.py --fake            # senza servizi AI
.venv/bin/python bridge.py --config config.toml
```

Punta il satellite verso il bridge:

```toml
backend = "agent"
[agent]
url = "ws://<host-del-bridge>:8765"
```

`--fake` risponde a ogni utterance con una frase predefinita e un tono
di prova, ed esercita il percorso dei tool chiamando `robot.sound` una
volta per turno — l'intero giro del protocollo senza dipendenze.

## Profili

- **Provider generico con tool calling** (`tool_calling = true`): il
  loop lo fa il bridge; il catalogo tool del satellite è dichiarato
  come tool OpenAI e i `tool_calls` sono inoltrati come `tool.call`.
- **Arkimede**: LLM/STT/TTS tutti su
  `http://<server>:3000/api/openai/v1` con chiave `ak_`; imposta
  `tool_calling = false` (il suo shim tiene i tool interni — i tool
  robot arriveranno via MCP, fase 2).
- **LLM nudo**: funziona comunque; solo voce.

Il server MCP (che esporrà i tool del satellite agli agenti MCP-native
come Arkimede) è il prossimo pezzo pianificato — vedi
`docs/agent-backend-plan.md`.
