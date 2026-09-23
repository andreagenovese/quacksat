# Studio: un satellite dal vivo su un Mac, senza anatra nella stanza

Cosa serve per far girare la cosa vera — un `robotd` vero, il binario
`quacksat` vero, un microfono vero e casse vere — mesi prima che arrivi
l'hardware, e cosa dimostra ogni risposta. Scritto dalla sessione del
2026-09-23, che è come sono stati validati l'allineamento alla 0.14.4 e
la finestra di ascolto.

## robotd, quello vero, su macOS

`robotd` compila e gira su un Mac: lo dice il demone stesso ("no bus on
this platform; use `--fake`"). Da un worktree di
`pollen-robotics/microduck` sul tag di release:

```sh
cargo build --release -p robotd        # ~20 s
robotd --fake --socket /tmp/qsl/robotd.sock --params /tmp/qsl/robotd.toml
```

- **Il path del socket dev'essere corto.** macOS limita il path di un
  socket unix a circa cento caratteri, e una directory di scratch sepolta
  sotto `/private/tmp/...` lo sfonda: il demone rifiuta con "path must be
  shorter than SUN_LEN". `/tmp/<corto>/robotd.sock` è tutto il rimedio.
- `--sim host:porta` fa girare lo stesso demone contro il gemello MuJoCo,
  quando il corpo si deve muovere davvero.

**Cosa quel demone non può fare su un portatile, e perché è utile.** Non
ha ONNX runtime, quindi nessuna policy guida e `robot.do` risponde "the
policy is not driving — press Start on the pad". Non ha banco suoni,
quindi `robot.sound` risponde "this robot has no voice". Sono entrambe
*risposte*: un rifiuto che torna indietro sul filo dimostra che la
chiamata ha raggiunto il robot, il che ne fa il testimone più economico
che una lane è viva. `robot.head`, `robot.look` e `robot.state` riescono
normalmente.

**La tabella delle skill è config**, quindi una voce `[[policy.skill]]`
nel file `--params` compare in `robot.skills` — ed è così che un satellite
che chiede al robot cosa sa fare (dalla 0.14) può essere provato contro
una skill che nessuna release ha mai distribuito. Un robotd `--fake` di
serie ne elenca sei.

## Microfono e casse del satellite su un Mac

`[audio] capture_command` accetta qualsiasi comando che scriva raw S16_LE
2ch 48 kHz su stdout, e `playback_program` qualsiasi programma in stile
aplay:

```toml
capture_command = ["sox", "-q", "-d", "-t", "raw", "-r", "48000", "-c", "2", "-e", "signed-integer", "-b", "16", "-"]
playback_program = "scripts/aplay-shim-macos.sh"
```

- Per una stanza che non ha bisogno di esseri umani, un generatore python
  che alterna due secondi di silenzio e uno di tono pilota
  `wake.mode = "energy"`, che vuole un attacco dopo il silenzio e non
  rumore costante.
- **`pkill` sul satellite lascia orfano il figlio della cattura**, che
  continua a tenersi il microfono. Il processo successivo legge un device
  che tiene qualcun altro e non sente niente — senza un errore da nessuna
  parte. Uccidi anche il comando di cattura, e controlla con `pgrep` prima
  di dare la colpa all'audio.
- Il guadagno d'ingresso è una variabile vera: a 27/100 una stanza
  tranquilla legge RMS 0,001, a 75 legge 0,018. Alzarlo non aiuta e basta
  — solleva sopra la soglia del VAD anche la voce dell'anatra stessa.

## Guidare un turno senza STT, LLM o TTS

Il backend `direct` serve il proprio endpoint MCP, che è un percorso
completo dei tool e non ha bisogno di nessun servizio vocale:

```sh
curl -s -X POST http://127.0.0.1:8767/mcp -H "Authorization: Bearer <token>" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}'
```

`tools/list` mostra esattamente ciò che vedrebbe l'agente (compreso l'enum
delle skill che ha riportato il robot), e `tools/call` esegue contro il
demone vivo. Basta a provare tutta la superficie dei tool, un riavvio di
robotd a metà sessione, e la lane che torna su.

Per il percorso Home Assistant, una sessantina di righe di python che
parlano Wyoming (describe → info, run-satellite, ping/pong, poi detection)
bastano a far svegliare l'anatra e chiudere un turno. È il satellite che
ascolta; è Home Assistant che si connette.

## Cosa può trovare solo una persona con una voce vera

Tutto quanto sopra è automatizzabile, e niente di tutto ciò ha trovato i
due guasti che una serata passata a parlare davvero con l'anatra ha
trovato:

- **L'anatra sente il proprio verso di conferma.** Ogni turno si chiudeva
  due secondi dopo il risveglio, prima che qualcuno avesse parlato, e la
  trascrizione tornava come allucinazione di whisper sul silenzio invece
  che come errore. Vedi ADR 0003 §4 — e nota che i test automatici non
  potevano prenderlo, perché frame date a raffica non modellano un
  altoparlante che continua a suonare dopo che il suo programma è uscito.
- **Una wake word addestrata su voci inglesi non risponde a una bocca
  italiana** (`docs/custom-wake-word.it.md`).

La regola che ne è uscita: un guasto che appare solo alla velocità di una
conversazione vera ha bisogno di una conversazione vera. Metti in conto
una serata a parlare con l'anatra prima di credere che un percorso vocale
funzioni.
