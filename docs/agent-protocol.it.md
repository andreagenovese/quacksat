# quacksat agent protocol — specifica di wire v1

Stato: v1, implementato da `backends/agent`, `backends/direct` e dal bridge di riferimento in `bridge/`. Questa è la traduzione
italiana; la copia inglese `agent-protocol.md` è quella canonica.
Destinatari: implementatori di bridge/agenti (il bridge di riferimento
in `bridge/`, [Arkimede](https://arkimede.ai/), o qualunque altra cosa).

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

`satellite.name` viene da `[agent] name` nella config del satellite e
identifica l'anatra presso il server. Con più anatre su un bridge,
dai a ciascuna un nome unico: fa da chiave nel registro delle sessioni
del bridge e diventa l'argomento `duck` dei tool MCP del bridge.

### `wake`
La wake word locale è scattata. Il satellite inizia lo streaming
dell'audio del microfono subito dopo questo evento (pre-roll incluso).

```json
{"type": "wake", "model": "hey_daffy", "score": 0.93}
```

`score` è la confidenza del rilevatore (null per i rilevatori che non
ne hanno una). I server con più anatre connesse lo usano per
l'arbitraggio del wake: i wake che arrivano in una piccola finestra
competono, vince lo score più alto, le perdenti ricevono
`listen.stop`.

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

I server dovrebbero inviarne uno ogni volta che un turno non produce
risposta (trascrizione vuota, STT/LLM falliti): il satellite lo tratta
come fine dell'attesa — abbandona la posa pensierosa e suona il "tock"
di resa invece di restare in silenzio fino al suo timeout di risposta.

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
| `robot.look` | `{x, y?, z?}` metri, trunk frame, con clamp | punta lo sguardo su un punto via l'IK di `robot.look` di robotd; il risultato riporta `clamped` se il punto è fuori portata |
| `robot.head` | `{pitch?, yaw?, roll?}` rad, con clamp | posa espressiva della testa (per guardare qualcosa c'è `robot.look`); gli angoli omessi tornano al centro |
| `robot.skill` | `{name}` ∈ ciò che ha riportato `robot.skills` (stock: ground_pick, kick_left, kick_right, sit_toggle, roulade) | skill one-shot via `robot.do`; dal demone 0.14 la tabella delle skill è config, quindi l'enum annunciato è la lista del robot e un nome fuori da quella è rifiutato prima del filo |
| `robot.move` | `{vx?, vy?, vyaw?, duration_s}` (duration ≤ 3.0 s) | camminata a tempo: intenti pompati a ≥20 Hz per la durata, poi silenzio — il deadman resta la rete di sicurezza |
| `robot.state` | `{}` | sintesi di `robot.state`/`robot.health`: posa, caduto, batteria, modalità |
| `robot.get_frame` | `{}` | **v1: sempre `ok: false, error: "unsupported"`** (in attesa dell'accesso camera via mediad) |

> I tool da `robot.where_am_i` a `robot.go_to` sono del demone di
> navigazione (`quack-navd`, ADR 0006). Compaiono nel catalogo di
> `session.start` solo quando un demone risponde su `[nav] socket`;
> senza, il satellite annuncia i suoi sette e la papera non può essere
> mandata da nessuna parte.

| `robot.where_am_i` | `{}` | il luogo memorizzato più vicino e la distanza (`at_place` se dentro il suo raggio), dalla mappa dal vivo di robotd (`robot.map`, ADR 0005); `known: false` con un motivo finché la posa non è fidata (seduto, in ricerca, senza mappa); errore se il robot non mappa affatto |
| `robot.remember_place` | `{name, radius_m?}` (0,3–6 m, default 1,5) | insegna la posa corrente come `name`; lo stesso nome da un altro punto aggiunge un'ancora; rifiutato finché la posa non è fidata |
| `robot.forget_place` | `{name}` | dimentica un luogo |
| `robot.list_places` | `{}` | tutti i luoghi con ancore, raggio, `stale` (insegnati prima di un reset della mappa) e la distanza quando la posa è nota |
| `robot.map_status` | `{}` | la mappa in numeri (mappatura on/off e modalità, tracking, seduto, finestre, submap, loop, conteggi di celle, posa), la distanza libera davanti/sinistra/destra/dietro prima di un muro noto (`clearance`, con ciò che ferma ogni raggio: muro, ignoto, bordo, aperto), la vista del guardiano del vuoto (`cliff`: se vede, e il dislivello più vicino — scale o buca, invisibile alla mappa — con distanza e direzione), più un suggerimento in una riga su cosa fare dopo |
| `robot.map_step` | `{vx?, vy?, vyaw?, walk_s?, stop_s?, centre?, gap?}` (cammino ≤ 3 s, sosta ≤ 10 s, default 6) | una tappa di un giro di mappatura stop-and-scan: una camminata a tempo, poi una sosta perché la fermata raggiunga la mappa; riferisce `new_windows`, lo spazio libero dopo la tappa e un suggerimento. Rifiutata se l'anatra è seduta o la mappatura è spenta; una camminata che finirebbe a meno di 25 cm da un muro mappato o da qualcosa visto dal sensore di profondità viene accorciata a quanto ci sta (`shortened` dice cosa l'ha limitata) e rifiutata solo se ci sta meno di un secondo di cammino; rifiutata in spazio non mappato a un palmo dal becco (il sensore non vede sotto i 10 cm), o verso un dislivello visto dal guardiano del vuoto (margine 35 cm) — l'errore indica i lati liberi. Con `centre: true` la tappa si tiene al centro tra i muri mappati (lo chiedono le tappe dell'esploratore); un muro a meno di 20 cm su un lato fa sterzare qualunque tappa dall'altra parte (`steered`) |
| `robot.map_explore` | `{stop?, max_s?}` | mappa tutto: un lavoro in background porta l'anatra alla frontiera raggiungibile più promettente (pavimento libero a contatto con l'ignoto, pesato per costo del percorso per cella di frontiera, così un'apertura larga una stanza più in là batte un ritaglio vicino; percorsi tenuti a 15 cm dai muri), la fa sostare per mapparla, e ripete finché non resta nulla di raggiungibile o scade il budget; ogni passo è un `robot.map_step` con tutti i suoi guardiani; quando la via è chiusa gira sempre dallo stesso lato (`[map] explore_turn` di quack-nav, destra di default — la regola della mano destra). Torna subito; `robot.map_status` porta `explore` (stato, tappe, frontiere rimaste, bersaglio). Mentre gira, `robot.move` e `robot.map_step` sono rifiutati. Raggiunta una zona senza nome il satellite chiede "qui dove siamo?" a voce (backend `direct`; `[map] ask_phrase` di quack-nav) e la risposta è attesa come `robot.remember_place` |
| `robot.map_save` | `{name}` | conserva la mappa che l'anatra ha in mano, con un nome, nella libreria del robot (da 1 a 64 caratteri fra lettere, cifre, `-` e `_`; salvare di nuovo con lo stesso nome sostituisce). Serve un robotd con la libreria di mappe — uno più vecchio risponde "questo robot non ha ancora una libreria di mappe" |
| `robot.map_list` | `{}` | le mappe salvate: nome, dimensione e quando è stata scritta ciascuna |
| `robot.map_match` | `{name?}` | è una delle case che l'anatra ha già mappato? Confronta la mappa che ha in mano con ogni mappa salvata (o con quella indicata) e risponde con i candidati, il migliore per primo: nome, dove la mappa viva si colloca dentro quella salvata (`x`, `y`, `yaw`), il residuo sui muri, la quota di pavimento vivo posato su muri salvati e un punteggio (più basso è meglio), più `live_cells`, quanta mappa c'era per chiedere. Non è un verdetto: con un ToF 8×8 un appartamento e la sua immagine speculare valgono uguale, quindi un nome si crede solo se torna lo stesso qualche minuto dopo con una mappa più grande (è quello che fa `[homecoming]`) |
| `robot.map_load` | `{name}` | restituisce all'anatra una mappa salvata. Torna la mappa, non la posizione: il mapper parte perso dentro di essa e cerca, quindi `robot.map_status` mostra `tracking: false` finché una sosta non conferma un luogo |
| `robot.go_to` | `{place?, x?, y?, stop?, max_s?}` | va in un luogo che la papera conosce (per nome, da `robot.list_places`) o in un punto in metri di mappa: la via più economica sulla mappa che ha già — senza esplorare — percorsa con le tappe e le guardie del lavoro di mappatura, quindi scale e ostacoli non mappati la fermano come fermerebbero un `robot.map_step`. Rifiuta subito, prima di camminare, se la mappa non mostra alcuna via. Ritorna subito; si segue con `robot.map_status` (`explore.state`, `explore.target_distance_m`); `stop: true` la ferma. Mentre va, `robot.move` e `robot.map_step` sono rifiutati |

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
