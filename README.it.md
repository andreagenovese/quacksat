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
scripts/            build, pacchetto e installazione per l'anatra (Radxa Zero 3)
docs/study/         study notes on the Microduck software stack
docs/adr/           architecture decision records
docs/todo.it.md     lavoro aperto
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
- La navigazione, dal 2026-09-22, è un demone a sé (ADR 0006):
  `quack-navd`, nel repo [quacknav](https://github.com/andreagenovese/quacknav),
  possiede il client della mappa, la guardia del dirupo, il planner, il
  registro dei posti, l'esploratore e l'homecoming, e risponde per loro
  su `/run/quack-nav/nav.sock` (NDJSON JSON-RPC, il filo di robotd). quacksat
  sonda quel socket all'avvio: se un demone risponde, i suoi tool vengono
  annunciati accanto a quelli del satellite; se non risponde nessuno, la
  papera ascolta e risponde ma non può essere mandata da nessuna parte, e
  lo dice. I due repo non condividono codice — solo il protocollo di
  robotd — quindi nessuno dei due va clonato per installare l'altro. Cosa
  sa fare la navigazione, e tre settimane di misure sul gemello MuJoCo,
  sono documentati in quel repo.
- Una pagina nel browser per i demoni della papera è un terzo repo,
  [quack-control](https://github.com/andreagenovese/quack-control): oggi
  mappa, luoghi, esplorazione e telecamera; quacksat ci compare come "non
  disponibile" finché non espone un suo socket di controllo (stato,
  configurazione, poi la chat — vedi [docs/todo.it.md](docs/todo.it.md)).
- Luoghi, mappatura e viaggi: l'anatra impara il nome del posto in cui si
  trova ("questa è la cucina"), risponde a "dove sei", mappa una casa da
  sola chiedendo "qui dove siamo?" nelle zone senza nome, e cammina fino
  a un posto che conosce — tutto attraverso i tool del demone
  (`robot.where_am_i`, `robot.remember_place`, `robot.map_status`,
  `robot.map_step`, `robot.map_explore`, `robot.go_to`, la libreria
  delle mappe), innestati nel catalogo che l'agente vede. I nomi sono
  legati a coordinate della mappa, mai riconosciuti a vista; un reset
  della mappa li rende stantii finché non vengono reinsegnati.
- Ciò che la papera dice da sé (`[announce]`, 2026-10-03): un viaggio
  risponde subito e prosegue in quack-navd, quindi il satellite segue
  `robot.map_status` (ogni 2 s mentre qualcosa si muove, ogni 10 s
  altrimenti, su una corsia tutta sua) e dice com'è finito un lavoro
  che ha avviato lui, con una frase fissa e breve — "Sono arrivata in
  cucina", "Non trovo una strada per la cucina", "Mi sono fermata", "Ho
  esplorato per 10 minuti, la casa è mappata al 72 per cento" — e
  quando quack-navd muove la papera da sola: la ricerca del ritorno a
  casa all'avvio ("Non sono sicura di dove sono: mi guardo intorno" …
  "Mi sono ritrovata"), la rilocalizzazione prima di un viaggio ("Prima
  di partire mi guardo intorno per ritrovarmi"). Italiano e inglese.
  Detto tra un turno e l'altro, mai sopra qualcuno, una volta sola:
  `direct` parla con il suo TTS, `agent` passa la frase al bridge come
  `say` (protocollo v1.1), che un bridge senza quella feature non
  riceve mai. Uno stop mandato dall'agente stesso non viene ripetuto:
  lo dice già la sua risposta.
- Gestione del turno che sopravvive alla voce dell'anatra stessa: un
  microfono, nessuna cancellazione d'eco e l'altoparlante di fianco (ADR
  0003), quindi dopo il verso di conferma il satellite butta via la coda
  che le sue casse stanno ancora emettendo e poi tiene il microfono
  aperto per tre secondi qualunque cosa senta — se parli, aspetta la fine
  della frase. Trovato parlandoci: prima il turno si chiudeva sul quack e
  il trascrittore rispondeva al silenzio con un'allucinazione.
- Pin su microduck `daemon-v0.14.4` (2026-09-23): compila ed è testato
  contro la release corrente. Dalla 0.14 le skill di un robot sono sua
  config, quindi `robot.skill` annuncia e fa rispettare la lista che il
  robot riporta (`robot.skills`) invece di cinque nomi compilati dentro —
  riletta ogni volta che la lane delle richieste si riaggancia, cosa che
  ora succede: una sola `robotd::Lane` dietro ogni backend, così robotd
  che riparte sotto il satellite non lascia più i tool del robot a dire
  "robot unreachable" — né, sul percorso Home Assistant, l'anatra muta e
  immobile mentre la pipeline va avanti. Quanto è costato quel salto
  è scritto in
  `docs/study/microduck-ipc-and-packaging.it.md`.

## Installare da una release

Ogni release porta un pacchetto d'installazione per la scheda
dell'anatra: `quacksat-<versione>-aarch64-linux.tar.gz` col suo
`.sha256`, dalla [pagina delle release](https://github.com/andreagenovese/quacksat/releases).
Si scarica, si verifica, si scompatta, poi dal tuo computer:

```sh
./install-on-duck.sh microduck@<anatra>   # prima --dry-run stampa cosa farebbe
```

Niente copia del repository e niente compilazione: installa il binario,
la unit systemd, l'account di servizio e la config d'esempio, e scarica
i modelli della wake word sull'anatra. Ogni passo, e cosa impostare nella
config (il backend, gli endpoint e le loro chiavi, la wake word, l'audio,
il socket di quack-navd, gli annunci dell'anatra), è nel
[README-install.it.md](scripts/package/README-install.it.md) del
pacchetto. Il bridge non c'è: gira su un server, da una copia del
repository allo stesso tag (sotto).

## Come iniziare

### 1. Build e installazione sull'anatra (da una copia del repository)

La board dell'anatra è una Radxa Zero 3 (Rockchip RK3566 aarch64) con
Armbian e lo userland Debian 13. La cross-build non chiede Docker:
cargo-zigbuild linka con `zig cc` contro una soglia glibc 2.31, così il
binario si carica sulla board qualunque glibc abbia la macchina che
compila (su un Mac: `brew install rustup zig cargo-zigbuild`, poi
`rustup toolchain install stable --target aarch64-unknown-linux-gnu`):

```sh
scripts/cross-build.sh                          # cross-build del binario release
scripts/install-on-duck.sh microduck@<anatra>   # installa tutto via ssh
```

L'installer (lo stesso script del pacchetto di release; `--dry-run`
stampa ogni comando e non si collega a niente) installa il binario
(`/usr/local/bin/quacksat`), la unit systemd col suo account di servizio
non privilegiato, una config di default in `/etc/robot/quacksat.toml`
(solo se non c'è — modificala lì; modo 0640 root:quacksat, perché
conterrà delle API key), e i modelli wake word in
`/var/lib/quacksat/models` — inclusa **«hey Daffy»**, la wake word
propria di quacksat, che è nel repo (`models/hey_daffy.onnx`); ogni
altro modello nella tua cartella `models/` locale (ad es. allenato
secondo `docs/custom-wake-word.md`) viaggia insieme, e i modelli di
feature condivisi che lì mancano li scarica sull'anatra
`scripts/fetch-wake-models.sh`, verificati col loro sha256.
`scripts/package.sh <versione> <binario> <dir>` prepara il pacchetto di
release in locale. Poi:

```sh
ssh microduck@<anatra> journalctl -u quacksat -f
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
