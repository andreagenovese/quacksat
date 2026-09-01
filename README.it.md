# quacksat

Satellite vocale mobile per Home Assistant e agenti AI, in esecuzione sul
Microduck di Pollen Robotics.

> **Disclaimer**: quacksat è un progetto indipendente. Non è affiliato a,
> approvato da o supportato da Pollen Robotics o Hugging Face.
> "Microduck" è usato solo per identificare l'hardware di destinazione.

## Cosa fa

quacksat trasforma il Microduck in un assistente vocale itinerante. Cattura
l'audio a bordo dell'anatra, rileva una wake word e affida la conversazione
a uno di tre backend intercambiabili selezionati in `/etc/robot/quacksat.toml`:

- **`wyoming`** — l'anatra diventa un satellite [Home Assistant Assist](https://www.home-assistant.io/voice_control/)
  tramite il protocollo Wyoming: STT, gestione degli intenti e TTS girano
  nella pipeline HA esistente.
- **`agent`** — l'anatra invia in stream audio ed eventi via WebSocket a un
  bridge che esegue STT → LLM (con tool calling) → TTS. Il protocollo è
  neutrale rispetto all'agente; un bridge di riferimento minimale vive in
  [`bridge/`](bridge/), quindi puoi portare il tuo agente.
- **`direct`** — autosufficiente: è l'anatra stessa a chiamare tre
  endpoint in dialetto OpenAI (chat completions, transcriptions,
  speech) — una chiave cloud o un server locale, senza bridge e senza
  server in casa. Può anche servire il proprio endpoint MCP così gli
  agenti MCP-capable guidano il robot direttamente.

In entrambe le modalità quacksat è un client non privilegiato di `robotd`,
il demone di sistema del Microduck: invia intenti e RPC (move, head, skill)
sul socket JSON-RPC e non tocca mai direttamente il bus hardware. Se
quacksat va in crash o si blocca, il deadman di robotd mantiene il robot al
sicuro.

## Struttura del repository

```
quacksat/           il binario: caricamento config, cattura, dispatch dei backend
quacksat-core/      cattura mic, wake word, VAD, speaker, tool robot → robotd
backends/wyoming/   Home Assistant Assist satellite backend
backends/agent/     backend agente AI (WebSocket verso un bridge)
backends/direct/    backend autosufficiente (STT/LLM/TTS in dialetto OpenAI, senza bridge)
bridge/             minimal reference bridge for the agent backend
systemd/            quacksat.service unit
scripts/            deploy scripts for the Radxa Zero 3 / the duck
docs/study/         study notes on the Microduck software stack
docs/adr/           architecture decision records
```

## Stato

Funzionante, validato su un Mac di sviluppo contro servizi reali (un
Home Assistant, una piattaforma agente, LLM locali). La validazione sul
robot è in attesa — dicembre 2026.

- Wake word locale (modelli openWakeWord sul runtime pure-Rust tract;
  frasi custom supportate — vedi `docs/custom-wake-word.md`),
  segmentazione dei turni via VAD, riproduzione half-duplex, client
  robotd sul modello di padd, e un segnale di pensiero: una lenta
  oscillazione della testa mentre la risposta viene calcolata, un
  "tock" basso di resa su timeout o errore (config `[thinking]`).
- `wyoming`: si registra in Home Assistant ed esegue il giro Assist
  completo (wake → STT → intent → TTS).
- `agent`: il protocollo WebSocket neutro (`docs/agent-protocol.md`)
  più il bridge di riferimento in `bridge/` — STT/LLM/TTS come endpoint
  url+key in dialetto OpenAI, tool robot dietro una allowlist
  esaustiva, e un server MCP che li espone agli agenti MCP-native. Multi-anatra: più satelliti su un bridge, arbitraggio
  del wake per score (risponde l'anatra che ti ha sentito meglio),
  indirizzamento per-anatra dei tool MCP.
- `direct`: il satellite autosufficiente — chiama da sé i tre endpoint
  in dialetto OpenAI, senza bridge, e può servire il proprio endpoint
  MCP così gli agenti guidano il robot direttamente.

## Come iniziare

### 1. Build e installazione sull'anatra

La board dell'anatra è un Rockchip RK3566 aarch64 con Armbian; macOS
non ha un sysroot aarch64-linux, quindi la build gira in un container
Linux (serve Docker):

```sh
scripts/build-aarch64.sh          # cross-build del binario release
scripts/deploy.sh <host-anatra>   # installa tutto via ssh
```

Il deploy installa il binario (`/usr/local/bin/quacksat`), la unit
systemd col suo account di servizio non privilegiato, una config di
default in `/etc/robot/quacksat.toml` (conservata ai redeploy —
modificala lì), e i modelli wake word in `/var/lib/quacksat/models` — inclusa
**«hey Daffy»**, la wake word propria di quacksat, che è nel repo
(`models/hey_daffy.onnx`); ogni altro modello nella tua cartella
`models/` locale (ad es. allenato secondo `docs/custom-wake-word.md`)
viaggia insieme.
Poi:

```sh
ssh <host-anatra> journalctl -u quacksat -f
```

Scegli il backend nella config: `wyoming` non richiede altro da questa
lista; `agent` richiede un bridge attivo (sotto); `direct` richiede gli
URL di tre endpoint in dialetto OpenAI.

### 2. Avviare il bridge (backend agent)

Su una macchina qualunque con Python 3.11+ (tipicamente il server
sempre acceso):

```sh
cd bridge
cp config.example.toml config.toml   # poi modificalo: url + chiavi LLM/STT/TTS
python3 -m venv .venv && .venv/bin/pip install websockets "mcp>=2" uvicorn
.venv/bin/python bridge.py --config config.toml
```

Punta il satellite verso il bridge (`[agent] url =
"ws://<host-bridge>:8765"`). `--fake` al posto di `--config` esercita
l'intero protocollo senza servizi AI. Dettagli e profili dei provider:
`bridge/README.md`.

### 3. Il bridge in Docker

```sh
cd bridge
cp config.example.toml config.toml   # poi modificalo
docker compose up -d --build
docker compose logs -f bridge
```

Porte: 8765 (WebSocket del satellite), 8766 (server MCP quando `[mcp]`
è abilitato). Smoke test del protocollo senza servizi AI:
`docker compose run --rm --service-ports bridge python bridge.py --fake`.

### Sviluppare senza il robot

`robotd --fake` (da un checkout di `pollen-robotics/microduck`) fa le
veci del robot vero, oppure si inoltra il socket reale:

```sh
ssh -L /tmp/robotd.sock:/run/robotd.sock <anatra>
```

Su macOS mic e speaker funzionano via sox — vedi gli hook
`capture_command` / `playback_program` in `quacksat.example.toml`.

## Licenza

Apache-2.0 — vedi [LICENSE](LICENSE) e [NOTICE](NOTICE).
