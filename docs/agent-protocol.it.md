# quacksat agent protocol — specifica di wire v1

Stato: bozza che implementa l'ADR 0004. Questa è la traduzione
italiana; la copia inglese `agent-protocol.md` è quella canonica.
Destinatari: implementatori di bridge/agenti (il bridge di riferimento
in `bridge/`, Arkimede, o qualunque altra cosa).

## Trasporto

- WebSocket, `ws://` o `wss://`. Il satellite è il client.
- Autenticazione opzionale: `Authorization: Bearer <token>` sulla
  richiesta di upgrade. Un server che rifiuta l'autenticazione chiude
  con HTTP 401/403.
- I **messaggi di testo** sono singoli oggetti JSON con un `"type"`
  obbligatorio.
- I **messaggi binari** sono payload audio raw, con significato
  dipendente dalla direzione: satellite→server è audio del microfono;
  server→satellite è audio TTS. Nessun header — il formato è fissato
  da `session.start` (microfono) e `tts.start` (TTS).
- I tipi di evento sconosciuti DEVONO essere ignorati (log-and-skip),
  mai trattati come errori. I campi sconosciuti in eventi noti DEVONO
  essere ignorati. Questa è la regola di compatibilità; non c'è un
  gate di versione.
- Ciascun lato può inviare `ping`; il peer risponde `pong` facendo eco
  al payload.
- Una connessione chiusa termina la sessione. Il satellite si
  riconnette con un backoff fisso (2 s) e avvia una sessione nuova.

## Ciclo di vita della sessione

```
satellite                              server (bridge/agent)
    │ ── session.start ──────────────────► │
    │ ◄────────────────── session.ready ── │
    │            (idle: wake word armed)   │
    │ ── wake ───────────────────────────► │
    │ ── [binary mic audio] ─────────────► │   streaming
    │ ── utterance.end ──────────────────► │   (or ◄─ listen.stop)
    │ ◄─────────────────────── tts.start ─ │
    │ ◄───────────────── [binary tts] ──── │   speaking (half-duplex)
    │ ◄───────────────────────── tts.end ─ │
    │ ◄────────────────────── listen.start │   follow-up turn (no wake)
    │ ── [binary mic audio] ─────────────► │
    │              ...                     │
```

Stati del microfono sul satellite: **idle** (wake armata, nessuno
streaming) → **streaming** (dopo un wake locale o `listen.start`) →
ritorno a idle su `utterance.end` (VAD locale) o `listen.stop`. Mentre
il TTS è in riproduzione il satellite è sordo (half-duplex, ADR 0003);
un `listen.start` ricevuto durante la riproduzione ha effetto quando
la riproduzione termina.

Le tool call possono arrivare in qualsiasi momento mentre la sessione
è aperta, anche durante lo streaming o la riproduzione.

## Eventi: satellite → server

### `session.start`
Primo messaggio su ogni connessione.

```json
{
  "type": "session.start",
  "version": 1,
  "satellite": {"name": "quacksat", "version": "0.1.0"},
  "audio": {"rate": 16000, "channels": 1, "format": "s16le"},
  "tools": [
    {
      "name": "robot.move",
      "description": "Move the robot for a bounded time. Speeds are clamped.",
      "parameters": {
        "type": "object",
        "properties": {
          "vx": {"type": "number", "description": "m/s forward"},
          "vy": {"type": "number", "description": "m/s left"},
          "vyaw": {"type": "number", "description": "rad/s counterclockwise"},
          "duration_s": {"type": "number", "maximum": 3.0}
        },
        "required": ["duration_s"]
      }
    }
  ]
}
```

`tools` è l'intera superficie offerta; è vuoto quando il robot non è
raggiungibile. La forma dello schema è JSON Schema standard,
utilizzabile direttamente come `tools[].function.parameters` di OpenAI
o come listing di tool MCP.

### `wake`
La wake word locale è scattata. Il satellite inizia lo streaming
dell'audio del microfono subito dopo questo evento (pre-roll incluso).

```json
{"type": "wake", "model": "hey_daffy", "score": 0.93}
```

### frame binari
Audio del microfono nel formato di `session.start`, ~32 ms per frame.
Inviati solo nello stato streaming.

### `utterance.end`
Il VAD locale ha chiuso l'enunciato; lo streaming si ferma.

```json
{"type": "utterance.end"}
```

### `tool.result`
Risposta a esattamente una `tool.call`, abbinata tramite `id`.

```json
{"type": "tool.result", "id": "t1", "ok": true, "data": {"fallen": false}}
{"type": "tool.result", "id": "t2", "ok": false, "error": "unknown tool"}
```

`ok: false` è un esito normale (rifiutato dalla allowlist, robot non
raggiungibile, unsupported); `error` ne spiega il motivo, in testo
pensato per essere letto dall'LLM.

### `pong`
Eco di un `ping` ricevuto, payload incluso.

## Eventi: server → satellite

### `session.ready`
Ack di `session.start`.

```json
{"type": "session.ready", "version": 1, "agent": {"name": "bridge"}}
```

### `listen.start` / `listen.stop`
Aprono/chiudono il microfono del satellite senza wake word. Un
`listen.start` durante la riproduzione TTS viene onorato al termine
della riproduzione. Un `listen.stop` in stato idle è un no-op.

```json
{"type": "listen.start"}
```

### `tts.start`, frame binari, `tts.end`
Una risposta parlata. L'audio è PCM raw nel formato dichiarato, in
streaming; il satellite lo riproduce attraverso il proprio player
half-duplex e nel frattempo scarta l'input del microfono. `tts.end`
chiude la clip; il satellite completa la riproduzione prima di
processare altri eventi che toccano l'audio.

```json
{"type": "tts.start", "rate": 22050, "channels": 1, "format": "s16le"}
```

Un nuovo `tts.start` prima che la clip precedente sia terminata uccide
la riproduzione precedente (regola del figlio unico, ADR 0003).

### `tool.call`
```json
{"type": "tool.call", "id": "t1", "name": "robot.state", "args": {}}
```

`id` è una stringa opaca scelta dal server, unica per ogni chiamata in
volo. Il satellite esegue in modo sequenziale nell'ordine di arrivo e
risponde sempre con un `tool.result` corrispondente. I server
dovrebbero applicare un timeout (suggerito: 30 s) e trattare un
risultato mancante come `ok: false`.

### `error`
Informativo; la sessione continua.

```json
{"type": "error", "message": "stt failed: connection refused"}
```

### `ping`

```json
{"type": "ping", "t": 1725100000}
```

## Superficie dei tool v1

Dichiarata dal satellite; tutti gli argomenti sono limitati (clamp)
lato satellite.

| Tool | Argomenti | Effetto |
|---|---|---|
| `robot.sound` | `{tag}` ∈ insieme SoundTag di robotd | verso espressivo dell'anatra via `robot.sound` |
| `robot.head` | `{pitch?, yaw?, roll?}` rad, con clamp | intento di posa della testa |
| `robot.skill` | `{name}` ∈ {ground_pick, kick_left, kick_right, sit_toggle, roulade} | skill one-shot via `robot.do` |
| `robot.move` | `{vx?, vy?, vyaw?, duration_s}` (duration ≤ 3.0 s) | camminata a tempo: intenti pompati a ≥20 Hz per la durata, poi silenzio — il deadman resta la rete di sicurezza |
| `robot.state` | `{}` | sintesi di `robot.state`/`robot.health`: posa, caduto, batteria, modalità |
| `robot.get_frame` | `{}` | **v1: sempre `ok: false, error: "unsupported"`** (in attesa dell'accesso camera via mediad) |

Ciò che non è elencato non esiste sul filo; la allowlist del satellite
è esaustiva per costruzione.

## Compatibilità ed evoluzione

- `version` è informativo (dottrina delle versioni API di microduck:
  ciò che rompe davvero un peer è una forma che è cambiata, e si
  rifiuta da sola per nome). Le modifiche additive (nuovi eventi,
  nuovi campi, nuovi tool) sono minor e sicure grazie alle regole di
  ignore.
- La separazione eventi/binario mappa 1:1 su datachannel/track WebRTC
  per un futuro binding remoto/full-duplex (ADR 0004 §1).
