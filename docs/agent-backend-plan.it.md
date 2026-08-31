# Piano strada B: backend agent, bridge e protocollo (task 6)

Stato: design concordato, pre-implementazione. Diventerà ADR 0004
(protocollo) più codice. Sostituisce lo schizzo in
`quacksat-ha-vs-agent.md` per i dettagli della strada B. Copia inglese
(canonica): `agent-backend-plan.md`.

## Forma

```
┌──────────── anatra ────────────┐      ┌───────────────── server ─────────────────┐
│ backends/agent (Rust)          │      │ bridge/ (Python, riferimento)            │
│ mic → wake → VAD → audio ────── WS ───► orchestratore: turni segmentati dal VAD  │
│ speaker ◄── stream tts ─────────────── │  ├─ STT  → {stt_base_url}/audio/transcriptions
│ esecuzione tool → allowlist →  │      │  ├─ LLM  → {llm_base_url}/chat/completions
│   robotd                       │      │  ├─ TTS  → {tts_base_url}/audio/speech   │
└────────────────────────────────┘      │  └─ server MCP "robot" (il cuore dei tool)│
                                        └──────────────────────────────────────────┘
```

## Decisioni

1. **Trasporto: WebSocket, non WebRTC** (per ora). Frame binari = audio
   raw 16 kHz mono S16LE; frame testo = eventi JSON. Motivazione: la
   conversazione è a turni e half-duplex per design (niente AEC, ADR
   0003); LAN domestica; un protocollo neutro "bring your own agent"
   deve essere implementabile con la libreria standard di qualunque
   linguaggio. Il protocollo è agnostico rispetto al trasporto: eventi ↔
   datachannel WebRTC e audio ↔ track mappano 1:1 se un domani
   remoto/full-duplex/video lo richiederanno (evoluzione documentata,
   non v0).

2. **Protocollo (ADR 0004, da speccare per primo).**
   - satellite → bridge: `session.start` (nome, formato audio, tool
     robot offerti), `wake` (modello, score), audio binario durante lo
     streaming, `utterance.end` (VAD locale), `tool.result {id, ok,
     data}`.
   - bridge → satellite: `listen.start` / `listen.stop` (controllo del
     mic — abilita il multi-turno senza ripetere la wake word),
     `tts.start {rate}` + audio binario + `tts.end` (in streaming nel
     Player half-duplex), `tool.call {id, name, args}`, `error`,
     ping/pong.

3. **I tool si eseguono sul satellite.** Il satellite dichiara i suoi
   tool in `session.start`; quacksat applica una allowlist esplicita
   (pattern del match esaustivo di btd) e traduce in RPC robotd.
   Superficie v0: `robot.sound`, `robot.head`, `robot.skill`,
   `robot.state`, e un `robot.move` *a tempo* `{vx, vy, vyaw,
   duration}` (quacksat pompa gli intenti per la durata, poi tace — un
   LLM non tiene mai un rubinetto aperto; il deadman resta l'ultima
   difesa). `robot.get_frame` è dichiarato ma `unsupported` finché
   mediad non espone la camera (roadmap mapping).

4. **Il server MCP è il cuore dei tool del bridge, non un'aggiunta.**
   Un unico server MCP (HTTP/SSE) espone i tool dichiarati dal
   satellite; ogni consumatore ci passa:
   - **Profilo 1 — agente MCP-native** (l'Arkimede di oggi): l'agente
     registra il server MCP del bridge e chiama i tool robot dentro il
     proprio loop. Nessuna modifica ad Arkimede — il suo shim OpenAI
     non vede mai le tool call (prendono la porta laterale MCP).
   - **Profilo 2 — provider chat-completions con tool calling**
     (OpenAI, Groq, llama.cpp…): il loop lo fa il bridge, dichiara i
     tool nella request ed esegue i `tool_calls` restituiti attraverso
     lo stesso livello MCP interno.
   - **Profilo 3 — LLM nudo** (né tool né MCP): solo chat vocale.
   Stesso esecutore, stessa allowlist, stesso audit in tutti i profili.

5. **STT/TTS/LLM sono tutti url+key in dialetto OpenAI.** Il bridge non
   contiene ML: `{llm,stt,tts}_base_url` + chiavi, parlando
   `/chat/completions`, `/audio/transcriptions`, `/audio/speech`. Le
   installazioni locali usano server in dialetto OpenAI già esistenti
   (speaches / faster-whisper-server, openedai-speech per Piper,
   LocalAI); il repo include un docker-compose d'esempio. Il **preset
   Arkimede è pura configurazione**: LLM su `/api/openai/v1` con chiave
   `ak_` (funziona oggi), STT sulla sua rotta audio e TTS quando
   Arkimede li esporrà — vedi `docs/VOICE_AUDIO_SERVICES.md` nel repo
   di Arkimede per quel lavoro. Zero codice specifico per Arkimede nel
   bridge.

## Ordine dei lavori

1. ADR 0004 + documento di spec del protocollo (bilingue).
2. `backends/agent` (Rust): client tungstenite + rustls (sincrono,
   thread std come il resto), stessa forma `Deps` del backend wyoming,
   reconnect con backoff; test con server scriptato speculari alla
   suite wyoming (conversazione completa inclusa una tool call).
3. `bridge/` (Python): server WS, segmentazione VAD, i tre client
   OpenAI, server MCP, profili; modalità `--fake` (risposte
   predefinite, niente modelli) per i test di protocollo;
   docker-compose d'esempio per STT/TTS locali.
4. Test dal vivo sul Mac: voce → bridge (fake) → tool.call → chirp su
   `robotd --fake`; poi bridge → preset Arkimede → conversazione vera
   con l'agente di casa (domotica via i tool MCP interni di Arkimede).
5. Fase 2 (repo Arkimede, opzionale): rotte audio + piper-service (doc
   sopra), registrazione MCP robot, eventualmente un gateway `/voice`
   nativo che sostituisce il bridge.

## Fuori scope (deliberato)

- Barge-in (serve l'AEC — l'ADR 0003 lo rimanda).
- Speech-to-speech realtime (strada C): un bridge diverso, stesso
  protocollo satellite.
- mDNS/discovery, sessioni multi-satellite.
