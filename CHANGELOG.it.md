# Changelog

Tutte le modifiche rilevanti a quacksat. Il formato segue
[Keep a Changelog](https://keepachangelog.com/it-IT/1.1.0/); le versioni
seguono il [Semantic Versioning](https://semver.org/lang/it/) (prima della
1.0 una versione minore può rompere qualcosa). Copia inglese:
[CHANGELOG.md](CHANGELOG.md).

quacksat ha girato su Mac di sviluppo contro servizi reali, contro un
robotd vero (`--fake`, o il socket del robot inoltrato) e col gemello
MuJoCo del Microduck — mai ancora su un'anatra fisica.

## [Unreleased]

## [0.1.0-rc1] - 2026-10-03

La prima release candidate: il satellite vocale come l'hanno lasciato
cinque settimane di storia, impacchettato per installarsi sull'anatra.
Note di rilascio:
[docs/release-notes-v0.1.0-rc1.it.md](docs/release-notes-v0.1.0-rc1.it.md).

### Aggiunto

- **La pipeline vocale** (`quacksat-core`). Cattura continua tramite un
  figlio `arecord` sul codec dell'anatra (il canale destro del microfono,
  48 kHz ricampionati a 16 kHz mono; ADR 0003), un VAD a energia con
  soglia di rumore adattiva che segmenta i turni, riproduzione
  half-duplex con un solo figlio `aplay` alla volta, e 320 ms di pre-roll
  inviati al risveglio perché il trascrittore senta il primo fonema. Gli
  agganci `capture_command` e `playback_program` (uno shim sox per macOS)
  fanno girare tutta la pipeline fuori dal robot.
- **Una wake word locale**: modelli ONNX alla openWakeWord (i modelli di
  feature mel ed embedding condivisi più una testa di risveglio) su
  tract, il runtime ONNX in puro Rust — nessuna dipendenza C. «Hey
  Daffy», la frase di quacksat, addestrata con la pipeline di openWakeWord
  e tenuta nel repository, è il default; qualunque modello openWakeWord
  si aggiunge così com'è (`docs/custom-wake-word.it.md`). Il rilevatore
  si azzera quando l'ascolto riprende, e il microfono è escluso mentre
  suona la conferma del risveglio.
- **L'anatra smette di ascoltare sé stessa**: un microfono, niente
  cancellazione d'eco, un altoparlante accanto. Dopo la propria voce il
  satellite butta una coda (`listen::TAIL_FRAMES`, ~320 ms, un numero da
  portatile finché non si misura sulla scheda) e tiene il microfono
  aperto almeno tre secondi; il parlato si aspetta fino alla fine.
  `direct` e `agent` la condividono; il percorso `wyoming` applica la
  stessa coda.
- **Tre backend intercambiabili** (ADR 0002), scelti da `backend` in
  `/etc/robot/quacksat.toml`, più `none` per il bring-up (risveglio e
  cinguettio):
  - **`wyoming`**: un satellite di Home Assistant Assist sul protocollo
    Wyoming — STT, intenti e TTS restano nella pipeline di Home
    Assistant.
  - **`agent`**: audio ed eventi su un WebSocket verso un bridge che fa
    STT → LLM (tool calling) → TTS, con un protocollo neutro
    (`docs/agent-protocol.it.md`, ADR 0004).
  - **`direct`**: autosufficiente — l'anatra chiama da sé tre endpoint in
    dialetto OpenAI (chat completions, trascrizioni, sintesi), una
    chiave cloud o server locali, nessun bridge. I blocchi di
    ragionamento dei modelli che li producono non vengono pronunciati.
- **Il bridge di riferimento** (`bridge/`, Python): il lato server del
  protocollo agent, LLM/STT/TTS come endpoint URL + chiave in dialetto
  OpenAI, i tool del robot dietro una allowlist esaustiva, `--fake` per
  esercitare tutto il protocollo senza servizi AI, un Dockerfile e un
  file compose. **Più anatre su un bridge**: ciascuna annuncia il suo
  `[agent] name`, i risvegli entro una finestra di 250 ms competono e
  risponde il punteggio più alto.
- **Protocollo agent v1.1** (2026-10-03): `say`, una frase dell'anatra
  pronunciata dal bridge come una normale clip TTS, mandata solo tra un
  turno e l'altro e solo a un bridge che elenca `say` nelle `features`
  di `session.ready`; un bridge più vecchio non ne riceve mai.
- **MCP**: il bridge offre il catalogo dei tool del satellite come
  server MCP (Streamable HTTP, porta 8766), coi tool indirizzati per
  anatra quando ne sono collegate più d'una; il backend `direct` offre il
  suo (`[direct.mcp]`, porta 8767, token bearer obbligatorio), così un
  agente che parla MCP guida il corpo senza bridge.
- **I tool del robot attraverso robotd**, sul modello di padd: un client
  non privilegiato del socket JSON-RPC di robotd che manda intenti e
  richieste e non tocca mai il bus (`robot.move`, `robot.head`,
  `robot.look`, `robot.sound`, `robot.skill`, `robot.state`,
  `robot.get_frame`, …); il deadman di robotd tiene il robot al sicuro
  se il satellite si blocca. Fissato a microduck `daemon-v0.14.4`:
  `robot.skill` fa rispettare le skill che il robot stesso elenca. Una
  sola corsia di richieste (`robotd::Lane`) dietro ogni backend si
  ricollega quando robotd riparte, invece di lasciare i tool «robot
  unreachable» (o, sul percorso Home Assistant, l'anatra muta) fino al
  riavvio del satellite. Correzioni `[gait]` per un passo che devia.
- **Il segnale di pensiero**: dopo `[thinking] delay_s` di attesa della
  risposta la testa ondeggia piano, e riprende dopo i tool che lasciano
  stare il corpo; a `timeout_s` un tock basso dice che l'anatra ha
  rinunciato.
- **La navigazione attraverso quack-navd** (`[nav] socket`,
  `/run/quack-nav/nav.sock`). La mappa, i luoghi, il pianificatore,
  l'esploratore e il ritorno a casa sono passati a un demone proprio nel
  repository quacknav il 2026-09-22 (ADR 0006); il satellite sonda il suo
  socket all'avvio e annuncia i suoi tool (`robot.where_am_i`,
  `robot.remember_place`, `robot.go_to`, `robot.map_explore`, …) accanto
  ai propri. Nessuno in ascolto non è un errore: l'anatra risponde ma non
  si può mandare da nessuna parte, e lo dice. I due repository non
  condividono codice.
- **Cosa dice l'anatra da sola** (`[announce]`, 2026-10-03): il satellite
  segue `robot.map_status` di quack-navd (ogni 2 s mentre qualcosa si
  muove, ogni 10 s altrimenti) e dice, con una breve frase fissa in
  italiano o in inglese, com'è finito un viaggio o un'esplorazione che ha
  avviato («Sono arrivata in cucina», «Mi sono fermata», «Ho esplorato
  per 10 minuti, la casa è mappata al 72 per cento») e perché l'anatra
  cammina quando nessuno gliel'ha chiesto (la ricerca del ritorno a casa,
  la rilocalizzazione prima di un viaggio). Tra un turno e l'altro, una
  volta ciascuna: `direct` con la sua voce, `agent` col `say` del bridge.
- **La unit systemd**: un account `quacksat` non privilegiato
  (`SupplementaryGroups=robot audio`), `After`/`Wants` su robotd e
  quack-navd invece di richiederli, un recinto di risorse (Nice 10,
  CPUWeight 50, MemoryHigh 192M, MemoryMax 256M) perché il satellite non
  costi mai a robotd il suo ciclo a 50 Hz, e il blocco di hardening di
  padd. I log non portano codici colore quando stderr non è un terminale
  (il journal).
- **La release**: `scripts/cross-build.sh` compila il binario della
  scheda con cargo-zigbuild (soglia glibc 2.31, senza Docker);
  `scripts/package.sh` impacchetta `quacksat-<versione>-aarch64-linux.tar.gz`
  con la unit, l'account, la config d'esempio, l'installer e un
  `README-install.md` passo per passo (e `.it.md`);
  `scripts/install-on-duck.sh` installa o aggiorna via ssh, dal pacchetto
  o da una copia del repository, con `--dry-run`; tiene una config
  esistente e ne installa una nuova 0640 root:quacksat. I modelli della
  wake word non sono nel pacchetto (openWakeWord pubblica i suoi modelli
  pre-addestrati sotto CC BY-NC-SA 4.0): l'installer li scarica
  sull'anatra, ciascuno verificato col suo sha256. La CI esegue i test e
  la cross-build a ogni push e allega il pacchetto, il binario nudo e i
  loro checksum alla release di un tag `v*`.

[Unreleased]: https://github.com/andreagenovese/quacksat/compare/v0.1.0-rc1...HEAD
[0.1.0-rc1]: https://github.com/andreagenovese/quacksat/releases/tag/v0.1.0-rc1
