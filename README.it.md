# quacksat

Satellite vocale mobile per Home Assistant e agenti AI, in esecuzione sul
Microduck di Pollen Robotics.

> **Disclaimer**: quacksat è un progetto indipendente. Non è affiliato a,
> approvato da o supportato da Pollen Robotics o Hugging Face.
> "Microduck" è usato solo per identificare l'hardware di destinazione.

## Cosa fa

quacksat trasforma il Microduck in un assistente vocale itinerante. Cattura
l'audio a bordo dell'anatra, rileva una wake word e affida la conversazione
a uno di due backend intercambiabili selezionati in `/etc/robot/quacksat.toml`:

- **`wyoming`** — l'anatra diventa un satellite [Home Assistant Assist](https://www.home-assistant.io/voice_control/)
  tramite il protocollo Wyoming: STT, gestione degli intenti e TTS girano
  nella pipeline HA esistente.
- **`agent`** — l'anatra invia in stream audio ed eventi via WebSocket a un
  bridge che esegue STT → LLM (con tool calling) → TTS. Il protocollo è
  neutrale rispetto all'agente; un bridge di riferimento minimale vive in
  [`bridge/`](bridge/), quindi puoi portare il tuo agente.

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
backends/agent/     AI agent backend (WebSocket to a bridge)
bridge/             minimal reference bridge for the agent backend
systemd/            quacksat.service unit
scripts/            deploy scripts for the Radxa Zero 3 / the duck
docs/study/         study notes on the Microduck software stack
docs/adr/           architecture decision records
```

## Stato

Scaffold iniziale. L'ordine di sviluppo è: prima core + wyoming (validare
l'hardware contro una catena HA collaudata), poi agent + bridge di
riferimento.

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
