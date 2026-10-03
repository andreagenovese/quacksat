# quacksat v0.1.0-rc1 — note di rilascio

2026-10-03. Copia inglese: [release-notes-v0.1.0-rc1.md](release-notes-v0.1.0-rc1.md).
L'elenco completo di cosa contiene è nel [CHANGELOG.it.md](../CHANGELOG.it.md).

## Cos'è

quacksat è il satellite vocale del Microduck: gira sull'anatra, ascolta
la sua wake word («hey Daffy») e affida la conversazione a Home
Assistant, a un agente AI dietro un bridge, o direttamente a endpoint di
voce e linguaggio in dialetto OpenAI — e dà a quella conversazione il
corpo dell'anatra: i movimenti, la testa, i suoni e le skill attraverso
robotd, e, attraverso il demone di navigazione quack-navd, i luoghi
della casa.

Questa è la **prima release candidate**: ciò che hanno lasciato cinque
settimane di lavoro, impacchettato perché si installi sull'anatra da un
download.

## Stato

- **Non ancora su un'anatra fisica.** L'anatra arriva a dicembre 2026;
  fino a una prima sessione su di lei, niente di questo ha girato sul
  RK3566.
- **Su Mac di sviluppo, contro servizi reali**: un'installazione di Home
  Assistant (il percorso `wyoming`, il giro completo di Assist), una
  piattaforma agente self-hosted e LLM locali (i percorsi `agent` e
  `direct`), un robotd vero — `--fake`, o il socket di un robot
  inoltrato via ssh — che risponde ai tool del robot.
- **Sul gemello MuJoCo** del Microduck, con quack-navd: «vai in cucina»
  ha camminato fino alla cucina e l'anatra l'ha detto all'arrivo; uno
  STOP a metà strada è stato detto; la ricerca del ritorno a casa
  all'avvio e la rilocalizzazione prima di un viaggio sono state
  annunciate, con `direct` (la sua voce) e con `agent` (attraverso il
  `say` del bridge).
- **L'installazione** è stata provata in un container Debian 13 arm64
  avviato con systemd: dal pacchetto scompattato e di nuovo da una copia
  del repository, col servizio che parte nella sandbox della sua unit e
  scrive nel log cosa gli manca (niente scheda audio, niente robotd,
  niente quack-navd). Non ancora su una scheda.

## Cosa funziona

- **Wake word, in locale**: modelli ONNX alla openWakeWord su tract (puro
  Rust); «hey Daffy» di default, qualunque modello openWakeWord per nome
  ([custom-wake-word.it.md](custom-wake-word.it.md)).
- **Turni senza cancellazione d'eco**: riproduzione half-duplex, una coda
  della voce dell'anatra buttata via, una finestra d'ascolto minima, il
  parlato aspettato fino alla fine.
- **Quattro backend**: `none` (bring-up: risveglio e cinguettio),
  `wyoming` (un satellite di Home Assistant Assist), `agent` (un
  WebSocket verso un bridge, [agent-protocol.it.md](agent-protocol.it.md)
  v1.1), `direct` (l'anatra chiama da sé gli endpoint, nessun server).
- **Il bridge di riferimento** in `bridge/`, più anatre su un bridge
  (risponde quella che ti ha sentito meglio), e **MCP**: il server del
  bridge per gli agenti che parlano MCP, e quello dell'anatra in `direct`.
- **Il corpo**: i tool del robot attraverso il socket di robotd come
  client non privilegiato (movimento, testa, sguardo, suoni, skill,
  stato, un fotogramma della camera), che sopravvivono a un riavvio di
  robotd; una posa pensierosa mentre la risposta si calcola.
- **La casa**: i tool di quack-navd aggiunti quando il suo socket
  risponde — dove sono, ricorda questo posto, vai in cucina, mappa la
  casa — e l'anatra dice da sola com'è finito un viaggio e perché
  cammina quando nessuno gliel'ha chiesto (`[announce]`, italiano e
  inglese).

## Installare

La release porta `quacksat-0.1.0-rc1-aarch64-linux.tar.gz` col suo
`.sha256`, e il binario nudo `quacksat-aarch64-linux` col proprio. Si
scarica il pacchetto, si verifica, si scompatta e si lancia
`./install-on-duck.sh microduck@<anatra>` dal proprio computer: vedi
[Installare da una release](../README.it.md#installare-da-una-release) e
il [README-install.it.md](../scripts/package/README-install.it.md) passo
per passo del pacchetto. All'anatra servono robotd, i gruppi `robot` e
`audio`, alsa-utils, e internet una volta (i modelli della wake word si
scaricano lì: i modelli preallenati di openWakeWord sono CC BY-NC-SA
4.0, quindi nessuno è nel pacchetto). Il bridge non è nel pacchetto: gira
su un server, da una copia del repository al tag.

## La configurazione in breve

`/etc/robot/quacksat.toml`, installata da `quacksat.example.toml` solo se
non c'è, modo 0640 root:quacksat — conterrà delle API key.

- `backend`: `none` nell'esempio; `direct` chiede `[direct.llm]`,
  `[direct.stt]`, `[direct.tts]` (`base_url`, `api_key`, modello o voce);
  `agent` chiede `[agent] url` (e un `name` unico per anatra su un bridge
  condiviso); `wyoming` ascolta su `[wyoming] bind` (`0.0.0.0:10700`).
- `[wake]`: `models_dir = "/var/lib/quacksat/models"`,
  `model = "hey_daffy.onnx"`, `threshold = 0.5`.
- `[audio]`: il codec dell'anatra, `plughw:aic3104`; `audio.pet_detect`
  di robotd deve restare `false` (il default di fabbrica).
- `[nav] socket = "/run/quack-nav/nav.sock"`: quello di quack-navd;
  nessuno in ascolto non è un errore.
- `[announce]`: acceso, `language` `"it"` o `"en"` (vuoto: quella
  dell'STT, poi quella del bridge, poi inglese), `journeys` e
  `own_motion`.
- `[direct.mcp]`: spento; acceso, il `token` è obbligatorio.
- Ogni chiave omessa prende il suo default; una chiave sconosciuta ferma
  il satellite con un messaggio che la nomina.

## Limiti noti

- **Nessun socket di controllo, ancora**: quack-control, la pagina
  locale per i demoni dell'anatra, mostra quacksat come «non
  disponibile» finché non espone il suo stato, la sua configurazione e
  più avanti una chat ([todo.it.md](todo.it.md)).
- **Gli annunci aspettano una pausa, non l'ordine della storia**: una
  frase dell'anatra si dice solo tra un turno e l'altro, così può
  arrivare dopo una risposta tardiva del modello su qualcosa accaduto
  dopo, o prima di essa.
- **L'italiano mette «in» davanti a ogni luogo**: «Sono arrivata in
  cucina» va bene, «in divano» no.
- **Una posa inaffidabile da sola non si dice**: l'anatra può essere
  stata spostata, ma finché niente si muove il satellite non ne parla.
- **Il backend `wyoming` non dice niente da solo**: nessun annuncio sul
  percorso Home Assistant, per ora (l'azione announce di HA non è
  collegata).
- **Frasi fisse**: le frasi dell'anatra sono frasi fisse in italiano o
  inglese, non formulate dall'agente.
- **La CPU sul RK3566 non è misurata**: il costo della wake word, e i
  tetti di memoria della unit (192M high, 256M max), sono prime stime per
  una scheda da 1 GB. La coda d'eco di 320 ms e le correzioni `[gait]`
  sono state tarate su un portatile e sul gemello; vanno misurate di
  nuovo sull'anatra.
- **«Hey Daffy» e l'accento italiano**: chi parla italiano ha fatto
  0.20–0.37 contro la soglia di 0.5, dove `hey_jarvis` di serie si
  svegliava subito; si abbassa la soglia o si allena la frase che si
  direbbe davvero ([custom-wake-word.it.md](custom-wake-word.it.md)).
- **robotd è fissato a `daemon-v0.14.4`**: un robotd più nuovo sulla
  scheda non è provato da qui.

## Cosa serve per la v0.1.0 finale

Una prima sessione sull'anatra vera: cattura e riproduzione del codec,
la CPU della wake word sul RK3566, la coda d'eco misurata lì, e una
conversazione che la porti in cucina.
