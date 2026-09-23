# quacksat — handoff per Claude Code

Brief da incollare come primo messaggio (o da salvare come `CLAUDE.md` nel repo).

## Politica della lingua

L'inglese è la lingua primaria per tutto (codice, commenti, docs, commit,
conversazione). Ogni documento in docs/ (e CLAUDE.md/README.md) è tenuto in
coppia: il file inglese è canonico; una copia italiana con lo stesso basename
più un suffisso `.it` prima dell'estensione esiste per l'utente. Quando si
crea o si modifica un documento, aggiornare entrambe le copie, prima
l'inglese.

## Cos'è

**quacksat**: satellite vocale mobile per Home Assistant e per agenti AI,
in esecuzione a bordo del Microduck (Pollen Robotics / Hugging Face).
Progetto indipendente, non affiliato a Pollen Robotics. Licenza Apache-2.0.
Descrizione repo: "Mobile voice satellite for Home Assistant and AI agents,
running on the Pollen Robotics Microduck".

## Decisioni prese

1. **Repo separato**, non fork di `pollen-robotics/microduck`. Si dipende dai
   loro crate (`duck-ipc-proto`) e si patcha upstream via PR solo se serve.
2. **Due backend intercambiabili**, selezionati da `/etc/robot/quacksat.toml`:
   - `wyoming` → strada A: satellite per HA Assist (Wyoming/ESPHome).
   - `agent` → strada B: WebSocket verso un bridge STT → LLM (tool calling) → TTS.
   Ordine: prima core + wyoming (valida l'hardware contro una catena HA già
   collaudata), poi agent + bridge di riferimento "bring your own agent".
3. **Pattern padd**: quacksat è un client NON privilegiato di robotd (gruppi
   minimi), manda solo intenti/RPC, non tocca mai il bus. Se tace, il deadman
   protegge il robot.
4. Protocollo `agent` neutro (audio streaming + eventi + tool call/result); il
   bridge nel repo è un riferimento minimale; l'integrazione con [Arkimede](https://arkimede.ai/) vive
   in Arkimede.

## Vincoli tecnici noti (da robotd-design.md e architecture.md)

- Board: Rockchip RK3566, 1 GB RAM, Armbian, systemd. Solo Rust nello stack
  Pollen; preferire crate pure-Rust (una C-dep costa cross-build e board).
- Cross-build da macOS non ha il sysroot aarch64: usare Docker/Linux per
  buildare per la board.
- IPC: JSON-RPC 2.0 / NDJSON su unix socket, `/run/robotd.sock`; intenti come
  notifiche (robot.move, robot.head), richieste con risposta (robot.stop,
  enable, skill). Namespace maintenance escluso ai client remoti.
- **Speaker: PCM esclusivo** — `sound.rs` tiene un solo figlio `aplay`, un
  suono nuovo uccide il vecchio. Il TTS deve coordinarsi con sound.rs, non
  aprire ALSA in parallelo. Da verificare sul codice come esporlo.
- **Mic: già in cattura continua da `pet-detect/`** (CNN log-mel 40 bande).
  Da verificare: ALSA dsnoop condivisibile o accesso esclusivo?
- Codec audio su I²C condiviso col ToF. Pipeline camera/mic prevista in
  migrazione verso `mediad` (roadmap M5): seguire i commit prima di fissare
  il design audio.
- Mic singolo, nessuna echo cancellation hardware (a differenza di Voice PE).
- Batteria NP-F550, ~1 h. Update firmati via updaterd: quacksat va
  pacchettizzato come unit separata, reinstallabile, fuori da `releases/`.
- Dev remoto: `ssh -L /tmp/robotd.sock:/run/robotd.sock` → quacksat gira sul
  Mac contro il robot vero. `robotd --fake` per lavorare senza hardware.

## Stato (2026-09-01) e prossimi passi

Il piano originale (scaffold → studio di microduck → ADR 0003 → core →
wyoming → agent/bridge) è completato, più un terzo backend `direct`,
il server MCP lato anatra, il supporto multi-anatra nel bridge
(arbitraggio del wake per score, indirizzamento per-anatra dei tool
MCP) e il segnale di pensiero (oscillazione della testa in attesa,
tock di resa a timeout — `[thinking]`). Tutto validato dal vivo su un
Mac di sviluppo contro servizi reali; vedi README → Stato.

Il 2026-09-23 il pin è passato da `daemon-v0.10.0` a `daemon-v0.14.4`:
le quattro release upstream ci sfiorano appena (il percorso audio, il
socket e robotd.service sono invariati), ma `proto::Skill` ora è una
stringa perché le skill di un robot sono config, quindi `robot.skill`
chiede `robot.skills` alla connessione e fa rispettare quella lista — e
la lane delle richieste (`robotd::Lane`, condivisa da tutti i backend) si
riaggancia quando muore, così un riavvio di robotd non azzittisce più per
sempre i tool del robot, né l'anatra stessa sul percorso wyoming. Il resoconto
completo è in `docs/study/microduck-ipc-and-packaging.it.md`.

Prossimi passi:

1. **Dicembre 2026**: arriva l'anatra fisica — validazione
   sull'hardware (cattura/riproduzione aic3104 secondo ADR 0003,
   starnazzi veri, cross-build + deploy via `scripts/`, budget CPU
   della wake word sull'RK3566). Due numeri hanno una data e vanno
   rimisurati là: `listen::TAIL_FRAMES`, i 320 ms della propria voce che
   l'anatra butta via, tarati sull'altoparlante e sul microfono di un
   portatile (l'ADR 0003 §4 dice come misurarli sulla board), e i trim
   `[gait]`, presi contro `alpha_walking` mentre lo slot walk di serie
   ora è `velstand.onnx`.
   Tutto ciò che si prova senza anatra sta in
   `docs/study/live-without-a-duck.it.md`: un robotd `--fake` vero su un
   Mac, il satellite contro di lui, e quali rifiuti dimostrano che una
   lane è viva.
2. Opzionale, nel repo di Arkimede: la fase 2 (gateway `/voice` nativo,
   tool passthrough sullo shim OpenAI) — i brief vivono lì.
3. La traccia di mappatura (la mappa, i posti, i viaggi) è cresciuta e
   se n'è andata di casa il 2026-09-22: è `quack-navd` nel repo
   quacknav (ADR 0006), e il satellite la raggiunge via
   `[nav] socket`. Qui ne restano la lane e lo splice.

## Contesto privato

I dettagli dell'impianto domestico dell'autore (rilevanti per la strada B)
stanno in `CLAUDE.local.it.md`, non tracciato. Altro materiale privato sta
nella cartella non tracciata `private/`. Nessuno dei due viene mai committato.
