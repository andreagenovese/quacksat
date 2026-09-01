# ADR 0002: Due backend vocali intercambiabili selezionati a runtime

- Stato: accettato
- Data: 2026-08-31

## Contesto

L'anatra deve servire due configurazioni: una casa con Home Assistant che
vuole un satellite Assist, e una configurazione con agente AI in cui le
conversazioni sono gestite da un LLM esterno con tool calling (per es.
[Arkimede](https://arkimede.ai/)). Costruire solo una delle due avrebbe vincolato il progetto ad HA
oppure costretto ogni utente a gestire uno stack agente custom.

## Decisione

quacksat distribuisce un solo binario con due backend, selezionati dalla
chiave `backend` in `/etc/robot/quacksat.toml`:

- `wyoming` — si registra come satellite Home Assistant Assist tramite il
  protocollo Wyoming; STT, gestione degli intenti e TTS girano nella
  pipeline HA.
- `agent` — invia in stream audio ed eventi via WebSocket a un bridge che
  esegue STT → LLM (tool calling) → TTS.

L'impianto condiviso (cattura del mic, wake word, VAD, uscita speaker,
client robotd) vive in `quacksat-core`; ogni backend è un crate proprio
sotto `backends/`.

Ordine di sviluppo: prima core + wyoming, per validare l'hardware contro
una catena HA già collaudata; poi agent più un bridge di riferimento
minimale.

Il protocollo agent è neutrale rispetto all'agente (audio streaming +
eventi + tool call/result). Il bridge in questo repo è un riferimento
minimale ("bring your own agent"); l'integrazione con Arkimede vive in
Arkimede.

## Conseguenze

- La selezione a runtime (configurazione, non feature a compile-time)
  significa un solo artefatto da compilare, firmare e distribuire tramite
  `updaterd` per entrambe le configurazioni.
- La separazione core/backend impone una API interna pulita per la
  pipeline audio e i tool robot, consumata da entrambi i backend.
- Il percorso Wyoming fa anche da banco di validazione dell'hardware:
  qualunque bug audio trovato lì è un bug del core, non del protocollo
  agent.
- Mantenere neutrale il protocollo agent significa che quacksat non
  acquisirà mai una dipendenza specifica da Arkimede.
