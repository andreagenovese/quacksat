# quacksat v0.1.0-rc2 — note di rilascio

2026-10-10. Copia inglese, quella di riferimento:
[release-notes-v0.1.0-rc2.md](release-notes-v0.1.0-rc2.md). L'elenco
completo delle modifiche è nel [CHANGELOG.it.md](../CHANGELOG.it.md); cosa
ha portato la v0.1.0-rc1, e i limiti che ha ancora, nelle
[release-notes-v0.1.0-rc1.it.md](release-notes-v0.1.0-rc1.it.md).

## Cosa c'è di nuovo

**Il corpo della papera in Home Assistant.** Con `[mqtt]` acceso, su
qualunque backend, quacksat pubblica la papera come dispositivo di Home
Assistant tramite la discovery MQTT
([ADR 0007](adr/0007-home-assistant-entities-over-mqtt.it.md)):

- pulsanti per camminare avanti, girare a sinistra e a destra, fermarsi,
  centrare la testa, eseguire ogni skill che il robot elenca, suonare
  ogni suono (disabilitati di default) e, con la navigazione, andare in
  ogni posto conosciuto;
- la testa come tre cursori (su e giù, destra e sinistra, inclinata);
- sensori per la batteria, la salute del robot, il suo modo, la risposta
  all'ultimo comando e, con la navigazione, dove si trova e come va il
  suo viaggio.

Sul percorso `wyoming` è questo che fa della papera un robot intero:
Assist le dà la voce, queste entità le danno il corpo. Automazioni a
frase d'esempio ("papera cammina", "papera gira a sinistra", "papera vai
in cucina"), frasi di Speech-to-Phrase sulle aree e un blueprint per i
posti, in italiano e in inglese, permettono all'agente di Home Assistant
stesso di guidarla a voce senza LLM, e di rispondere con ciò che la
papera ha fatto davvero.

Ogni movimento è un impulso — una pressione cammina per qualche secondo
e si ferma — e in tutto il satellite gira una camminata alla volta,
chiunque l'abbia chiesta (un turno vocale, un agente MCP, un pulsante);
lo **stop** vince sempre.

**Guide.** [Provare quacksat senza la papera](try-without-a-duck.it.md):
le tre modalità su un portatile, con un `robotd --fake` vero come corpo.
E [la guida per Home Assistant](home-assistant/setup-guide.it.md): il
satellite Wyoming e il dispositivo MQTT configurati insieme, passo per
passo, con le schermate.

## Stato

- **Non ancora su una papera fisica.** Arriva a dicembre 2026.
- **Il dispositivo in Home Assistant** è stato verificato su Home
  Assistant 2026.10 e Speech-to-Phrase 1.4.3 in container (discovery,
  ogni comando, i rifiuti, la disponibilità, le automazioni a frase per
  testo e per audio), poi da capo a fondo su un'installazione di Home
  Assistant in uso ogni giorno, con quacksat su un Mac: "hey Daffy" sul
  satellite, "papera cammina" trascritto dalla pipeline di casa,
  l'automazione che preme il pulsante, la camminata, "Fatto." detto a
  voce.
- **Nessuna regressione sugli altri percorsi**, verificato dal vivo su un
  Mac contro un robotd `--fake` vero prima del tag: `direct` pilotato dal
  suo server MCP con MQTT acceso accanto (uno stop da Home Assistant
  ferma una camminata avviata da un agente MCP, e la camminata dopo
  dura per intero), `agent` contro la modalità finta del bridge con e
  senza MQTT, il bring-up `none`. 96 test automatici, clippy pulito, la
  cross-compilazione per la scheda.

## Installazione

Come per la rc1: la release contiene
`quacksat-0.1.0-rc2-aarch64-linux.tar.gz` con il suo `.sha256`, e il
binario `quacksat-aarch64-linux` con il suo. Vedi [Installare da una
release](../README.it.md#installare-da-una-release) e il
[README-install.it.md](../scripts/package/README-install.it.md) del
pacchetto, che ora ha la sezione `[mqtt]`. L'aggiornamento conserva
`/etc/robot/quacksat.toml`: `[mqtt]` resta spento finché non lo aggiungi.

## Configurazione

```toml
[mqtt]
enabled = true
host = "homeassistant.local"   # il broker MQTT
port = 1883
username = "quacksat"          # obbligatori: i topic comandano un robot che cammina
password = "…"
# node = "quacksat"            # l'id nei topic e negli id delle entità; vuoto: il nome del backend
# device_name = ""             # vuoto: "Papera" o "Duck", secondo [announce] language
step_s = 2.0                   # una pressione di avanti, secondi (al massimo 3)
turn_s = 2.0                   # una pressione di gira
```

L'utente del broker, una ACL, le automazioni d'esempio e i topic sono in
[docs/home-assistant/README.it.md](home-assistant/README.it.md).

## Limiti noti

Nuovi con questa release:

- **I passi manuali non hanno la guardia del dirupo.** La guardia
  viaggia con i viaggi della navigazione; una pressione di avanti non
  guarda in basso. Tieni la papera lontana dal bordo di un tavolo.
- **Solo MQTT in chiaro**: niente TLS verso il broker, QoS 0. Pensato
  per un broker nella rete di casa, con un utente suo per la papera.
- **Speech-to-Phrase come add-on di Home Assistant**: da dove legge i
  suoi file di frasi, e se si riaddestra da solo quando si aggiunge
  un'area o una frase del blueprint, è ancora da verificare; fino ad
  allora, riavvia l'add-on dopo averne aggiunta una.
- **L'agente di Home Assistant non preme i pulsanti per nome**: la voce
  passa dalle automazioni d'esempio; un agente LLM in Home Assistant
  vede i pulsanti così come sono.

Ancora veri dalla rc1: niente socket di controllo per quack-control; il
backend `wyoming` non dice niente da sé (il sensore del viaggio ora dà a
Home Assistant ciò che serve per annunciarlo, non ancora collegato);
l'italiano mette "in" davanti a ogni posto; CPU e memoria sull'RK3566
non misurate, la coda d'eco e le correzioni `[gait]` da rimisurare sulla
papera; "hey Daffy" e l'accento italiano; robotd fissato a
`daemon-v0.14.4`.

## Cosa serve per la v0.1.0 definitiva

Una prima sessione sulla papera vera: cattura e riproduzione del codec,
la CPU della wake word sull'RK3566, la coda d'eco misurata lì, una
conversazione che la porta in cucina — e, ora, un pulsante in Home
Assistant che fa lo stesso.
