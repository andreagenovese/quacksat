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
  robotd sul modello di padd.
- `wyoming`: si registra in Home Assistant ed esegue il giro Assist
  completo (wake → STT → intent → TTS).
- `agent`: il protocollo WebSocket neutro (`docs/agent-protocol.md`)
  più il bridge di riferimento in `bridge/` — STT/LLM/TTS come endpoint
  url+key in dialetto OpenAI, tool robot dietro una allowlist
  esaustiva, e un server MCP che li espone agli agenti MCP-native.
- `direct`: il satellite autosufficiente — chiama da sé i tre endpoint
  in dialetto OpenAI, senza bridge, e può servire il proprio endpoint
  MCP così gli agenti guidano il robot direttamente.

## Compilazione

La board dell'anatra è un Rockchip RK3566 aarch64 con Armbian. macOS non ha
un sysroot aarch64-linux, quindi la cross-build va fatta in Docker/Linux
(vedi `scripts/`). Per sviluppare senza il robot, `robotd --fake` esegue il
demone senza hardware, oppure si può inoltrare il socket reale:

```sh
ssh -L /tmp/robotd.sock:/run/robotd.sock <duck>
```

## Licenza

Apache-2.0 — vedi [LICENSE](LICENSE) e [NOTICE](NOTICE).
