# Piano strada B: backend agent, bridge e protocollo (task 6)

Stato: **implementato** (2026-09-01) — conservato come registro di
design della strada B; il contratto wire vive in ADR 0004 +
`docs/agent-protocol.md`. Sostituisce lo schizzo in
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
   - **Profilo 1 — agente MCP-native** (l'[Arkimede](https://arkimede.ai/) di oggi): l'agente
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
   Arkimede li esporrà. Zero codice specifico per Arkimede nel bridge.

## Ordine dei lavori (tutto consegnato)

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

## Implementato: un backend `direct` (anatra autosufficiente)

Un terzo backend, `backend = "direct"`, in cui è il satellite stesso a
parlare il dialetto OpenAI — STT → LLM (tool calling) → TTS su
semplice HTTP, tool eseguiti in-process dietro la stessa allowlist,
niente bridge e niente salto WebSocket. In Rust il costo in risorse
sulla board è trascurabile (l'orchestrazione non calcola nulla; i
modelli pesanti restano dietro gli URL).

Perché conta: **un'anatra completamente autosufficiente è ciò che la
maggior parte delle persone vuole** — inserisci una chiave API (o un
qualunque endpoint in dialetto OpenAI) e parli, senza server in casa,
senza container, senza un bridge da gestire. L'architettura attuale lo
accoglie già per costruzione: la selezione dei backend a runtime
dell'ADR 0002 più i servizi url+key dell'ADR 0004 fanno sì che
`backends/direct` si affianchi a `wyoming` e `agent` senza toccare
altro.

Compromessi noti rispetto al bridge: la memoria di conversazione muore
con la batteria, e nulla è condiviso tra più anatre. Il bridge resta
la forma giusta per una casa con un server sempre acceso; `direct` è
la forma giusta per tutti gli altri.

**Implementato: un server MCP sull'anatra stessa.** Il backend
direct può esporre il catalogo dei tool robot come proprio server MCP
(Streamable HTTP stateless, config `[direct.mcp]`, spento di default) —
stessa allowlist e stessi clamp, serviti dal robot. Questo completa la
matrice (direct + Arkimede = voce + domotica + corpo, senza bridge) e
rende l'anatra registrabile da *qualunque* client MCP (Claude Desktop,
Claude Code, altri agenti) indipendentemente dalla voce. Postura di
sicurezza: il bearer token è **obbligatorio** quando abilitato (un
server HTTP che accetta comandi di movimento sul robot è più delicato
dello stesso server sul bridge); sotto restano allowlist, clamp e il
deadman di robotd. I limiti noti rimangono: batteria e DHCP fanno
dell'anatra un host MCP intermittente — bene per sperimentare, mentre
il bridge resta il bersaglio di registrazione solido per l'uso
quotidiano con Arkimede.

## Implementato: multi-satellite (un bridge, più anatre)

Il protocollo già lo permetteva: ogni anatra apre il suo WebSocket, si
presenta in `session.start` (nome incluso) e ottiene una sessione
indipendente — buffer audio, turni e memoria propri. Il bridge di
riferimento ora ha la parte in cui *una* anatra va scelta:

1. **Identità**: ogni satellite imposta `[agent] name` nella sua
   config ("duck-cucina", "duck-studio", ...); il nome viaggia in
   `session.start` e fa da chiave nel registro delle sessioni del
   bridge (una riconnessione con lo stesso nome sostituisce la
   sessione vecchia).
2. **Tool con indirizzo**: con più di un'anatra connessa, i tool MCP
   del bridge guadagnano un argomento `duck` obbligatorio (un enum dei
   nomi connessi, iniettato in ogni schema); una chiamata senza viene
   rifiutata con la lista dei nomi. Con una sola anatra non cambia
   nulla — niente argomento, niente attrito.
3. **Arbitraggio del wake**: due anatre in stanze adiacenti sentono
   entrambe la wake word. Come fa Home Assistant coi suoi satelliti,
   il bridge raccoglie gli eventi `wake` in una piccola finestra
   (`[behavior] wake_window`, default 250 ms), **vince lo score più
   alto** (l'anatra più vicina a chi parla), e le altre ricevono
   `listen.stop` e lo svuotamento dell'audio bufferizzato. L'evento
   `wake` porta lo score esattamente per questo —
   `WakeDetector::last_score()` lato satellite.
4. **Memoria di casa** (con una piattaforma agente): la conversazione
   appartiene alla casa, non a un'anatra — la memoria vive già
   nell'agente; il bridge instrada ogni risposta all'anatra che ha
   catturato l'ultima frase.

È anche il *perché* il multi-anatra pretende il bridge lato server:
arbitraggio del wake e memoria condivisa richiedono un punto che veda
tutte le anatre insieme — impossibile con un bridge a bordo o col
backend `direct`. Il "chorale" di upstream (anatre che cantano in
armonia) è lo stesso istinto; la versione quacksat è la presenza
vocale distribuita. È costato esattamente quanto previsto: mappa nel
registro più arbitraggio del wake nel bridge, un campo di config sul
satellite, zero modifiche al protocollo.

## Implementato: il segnale di pensiero (linguaggio del corpo in attesa)

Tra un comando e la risposta possono passare parecchi secondi con
l'anatra muta e immobile — indistinguibile da un'anatra che non ha
sentito. Voice PE lo risolve col suo anello LED; l'anatra ha qualcosa
di meglio: un corpo. La timeline, comune ai tre backend:

- frase chiusa → niente (le risposte rapide non meritano teatro);
- dopo `[thinking] delay_s` (default 1 s) → la posa pensierosa: una
  leggera inclinazione della testa con una lenta oscillazione dello
  yaw, un reinvio strozzato di `robot.head` della stessa forma del
  pump di `robot.move`;
- arriva la risposta (`tts.start` / `audio-start`) → la testa torna al
  centro, l'anatra parla;
- timeout (`[thinking] timeout_s`, default 30 s) o un errore di
  protocollo → un "tock" basso invece del silenzio (sintetizzato in
  locale quando robotd non può suonarlo).

Se l'agente inizia ad agire durante l'attesa (arriva una tool call),
la posa cede il corpo senza ricentrare — un ricentraggio
calpesterebbe un movimento della testa comandato da un tool — mentre
il cronometro continua a correre verso il timeout; finito il tool
l'oscillazione riprende, a meno che il tool stesso non abbia messo in
posa la testa (`robot.head` / `robot.look`). Il bridge ora
segnala trascrizione vuota e turno fallito come eventi `error` di
protocollo, così il satellite smette subito di aspettare invece di
scoprire il silenzio al timeout.

## Fuori scope (deliberato)

- Barge-in (serve l'AEC — l'ADR 0003 lo rimanda).
- Speech-to-speech realtime (strada C): un bridge diverso, stesso
  protocollo satellite.
- mDNS/discovery.
