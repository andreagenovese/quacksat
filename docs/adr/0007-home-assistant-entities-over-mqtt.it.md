# ADR 0007: Il corpo della papera come entità di Home Assistant, via MQTT

- Stato: accettata
- Data: 2026-10-09, rivista il 2026-10-10 (il caso base è l'agente
  di Home Assistant stesso, senza LLM; §7 riscritto su ciò che è stato
  misurato)
- Ingressi: ADR 0002 (backend intercambiabili), ADR 0004 §4 (la
  superficie dei tool), ADR 0006 §4 (un solo pilota alla volta),
  `docs/study/quacksat-ha-vs-agent.it.md` ("controllo robot: assente"
  sul percorso A), la domanda dell'utente del 2026-10-09
- Copia canonica in inglese: [0007-home-assistant-entities-over-mqtt.md](0007-home-assistant-entities-over-mqtt.md)

## Contesto

Sul percorso `wyoming` la papera è un satellite di Assist e nient'altro.
Il protocollo Wyoming trasporta audio ed eventi della pipeline; non ha
modo di dichiarare un'entità né di ricevere un comando. Home Assistant
sente "papera, vai avanti", lo passa ai suoi intent o al suo agente
conversazionale, e non trova niente da chiamare. Anche con Arkimede come
agente conversazionale la risposta è la stessa: Arkimede comanda Home
Assistant via MCP, e Home Assistant della papera non sa nulla. La lane
di robotd che il backend wyoming tiene aperta porta i segnali della
papera (il cinguettio, l'ondeggiare mentre pensa, il tock triste) e
nessun tool.

Il percorso A è quindi mezzo robot: ha una voce e non ha un corpo. Lo
studio di agosto 2026 lo annotava come "assente (o via automazioni HA →
RPC)" e lì si era fermato.

Il caso per cui questa ADR è pensata è il più semplice: l'agente
conversazionale di Home Assistant stesso, senza nessun LLM dietro.
Quell'agente non ragiona; confronta il testo trascritto con un elenco
di frasi — quelle di serie applicate ai nomi delle entità esposte, e
quelle che aggiunge l'utente — ed esegue ciò che corrisponde, oppure
dice che non ha capito. Tutto ciò che la papera sa fare a voce sul
percorso A deve essere raggiungibile così. Un agente LLM, dove c'è, è
un'aggiunta sopra (§7), mai un requisito.

I comandi del corpo esistono già come tabella: `quacksat_core::tools`,
una sola allowlist con i limiti applicati dal satellite, eseguita per il
bridge agent, per l'LLM del backend direct e per il suo server MCP.
Manca una porta verso Home Assistant.

## Decisione

### 1. MQTT discovery, dentro quacksat

quacksat si collega al broker MQTT di casa (in un'installazione HAOS,
l'add-on Mosquitto) e pubblica un payload di discovery: Home Assistant
crea un dispositivo, "la papera", con le entità del §3, e da lì in poi
premerne una è un messaggio su un topic che quacksat esegue.

Considerate e scartate:

- **L'API nativa di ESPHome** al posto di Wyoming. È quella che parla
  il Voice PE e darebbe un solo dispositivo che è insieme satellite ed
  entità, ma significa un server protobuf-su-TCP con cifratura Noise in
  Rust: in pratica un quarto backend, per il risultato che il §3 ottiene
  con un client.
- **Un'integrazione custom di Home Assistant** (Python, HACS). Una
  seconda base di codice in una seconda lingua, rilasciata al ritmo di
  Home Assistant, per dire quello che la discovery dice in un payload
  JSON.
- **Un server MCP sul percorso wyoming.** Piccolo (il `mcp.rs` del
  backend direct spostato nel core), ma raggiunge solo un agente MCP
  come Arkimede; gli intent di Home Assistant, Speech-to-Phrase, le
  automazioni e le dashboard restano senza corpo. Completa questa ADR e
  non viene sostituito da essa.

Il client è scritto a mano (`quacksat_core::ha::mqtt`), come il server
MCP del backend direct: MQTT 3.1.1, QoS 0, sessione pulita, last will,
utente e password — pochi pacchetti incorniciati e circa trecento righe
con i loro test. (Scritto all'inizio come `rumqttc` con le feature di
default spente; all'implementazione è risultato che porta un runtime
tokio sotto la sua API sincrona, e la sua feature TLS porta `ring`, che
è C e assembly. Nessuno dei due vale la pena per questo sottoinsieme.)
Il TLS aspetta un provider in Rust puro o un bisogno vero; su una LAN
di casa il recinto sono la password del broker e la sua ACL (§5).

### 2. Opzionale, su ogni backend, su un thread suo

`[mqtt] enabled = true` lo accende. È scritto per il percorso A ma non
è chiuso lì: le entità sono utili anche su `agent` e `direct` per
automazioni e dashboard, e il codice non bada a quale backend tenga il
microfono.

Gira su thread suoi, accanto al loop vocale: uno tiene il socket del
broker, l'altro — il worker — tiene un `Robot` suo, con le sue lane
verso robotd e quack-navd. (Scritto all'inizio come condivisione del
`Robot` del loop vocale dietro il mutex del backend direct;
all'implementazione una camminata di tre secondi con quel mutex in mano
avrebbe bloccato i segnali e l'audio del loop wyoming, e il backend
wyoming tiene una `Lane` nuda. Lane separate non costano niente a
robotd, che serve più client.) La regola della camminata unica, che il
mutex avrebbe dato, si è spostata dove ogni camminata viene pompata:
`body::timed_move` rifiuta una camminata mentre un'altra viene pompata
nel processo, chiunque l'abbia chiesta — un tool di un turno vocale, un
client MCP, un pulsante — e `body::halt` ferma quella in corso (§4,
§5). I backend chiamano solo `ha::start`, passando il loro
annunciatore.

### 3. Il catalogo delle entità

MQTT è un quarto chiamante della tabella dei tool, non una superficie
nuova: ogni entità di comando diventa una chiamata a `tools::execute`
con argomenti fissi o limitati. A robotd non arriva niente che i tool
dell'LLM non potessero già mandare, e i limiti sono le stesse costanti.

| Entità | Tipo | Diventa | Note |
|---|---|---|---|
| Avanti | `button` | `robot.move {vx: 0.3, duration_s: step_s}` | `step_s` di default 2 s, massimo 3 (il limite del tool); ≈ 20 cm |
| Gira a sinistra / destra | `button` ×2 | `robot.move {vx: 0.3, vyaw: ±0.7, duration_s: turn_s}` | la papera non gira sul posto; `turn_s` di default 2 s ≈ 90° |
| Stop | `button` | ferma la camminata in corso, chiunque l'abbia avviata | vedi §4 |
| Testa pitch / yaw / roll | `number` ×3 | `robot.head` | min/max sono i limiti del tool; lo stato è l'ultimo valore inviato |
| Testa al centro | `button` | `robot.head {}` | |
| Uno per skill | `button` | `robot.skill {name}` | costruiti da `robot.skills` alla connessione; ripubblicati se il robot torna con una lista diversa |
| Uno per suono | `button` | `robot.sound {tag}` | `enabled_by_default: false`, sei pulsanti affollerebbero la pagina del dispositivo |
| Uno per posto | `button` | `robot.go_to {place}` | solo se quack-navd risponde; costruiti da `robot.list_places`, ripubblicati quando un posto viene memorizzato o dimenticato |
| Batteria | `sensor` | `robot.state` → `battery` | come la riporta robotd |
| In salute | `binary_sensor` (`problem`) | `robot.state` → `healthy`, `reason` come attributo | |
| Modo | `sensor` | `robot.state` → `mode` | |
| Dove | `sensor` | `robot.where_am_i` / `robot.map_status` | solo con la navigazione: il nome del posto più vicino |
| Viaggio | `sensor` | `robot.map_status` | solo con la navigazione: lo stato di quack-navd così com'è (idle, running, relocalizing, searching, done, stopped, failed), con il motivo |
| Ultima risposta | `sensor` | i topic di risultato | la risposta all'ultimo comando come la direbbe la papera; tiene anche Home Assistant sempre sottoscritto a `result/+` — senza, un'automazione che preme e poi aspetta perdeva una risposta più veloce della sua stessa sottoscrizione (misurato: `go_to` ha risposto in meno di un millisecondo) |

**I posti: pulsanti per la mano, un comando per nome per la voce.** Un
pulsante per posto serve a dashboard e automazioni, costruito da
`robot.list_places` e ripubblicato quando un posto viene memorizzato o
dimenticato. La voce non passa da lì: come misurato (§7), nessuna frase
di serie di Home Assistant preme un pulsante, né in italiano né in
inglese, e Speech-to-Phrase non impara i nomi delle entità fuori dalle
frasi accendi/spegni per luci, interruttori e ventilatori. Quindi un
posto si può chiedere anche per nome: `cmd/go_to` riceve il posto così
come è stato detto, quacksat lo confronta con `robot.list_places`
ignorando maiuscole e un articolo iniziale (il, lo, la, l', i, gli,
le), e risponde su `result/go_to` con `{ok: true, place}` oppure
`{ok: false, error, detail}` — "nessun posto chiamato garage". È l'unico comando non legato a un'entità; chi lo chiama è
un'automazione a frase (§7).

Non una `select`: se l'agente di Home Assistant imposti un'opzione a
voce non è stato verificato, e il comando per nome rende la domanda
superflua.

Non le scene, anche se sono state la sorpresa delle misure: l'agente di
Home Assistant attiva una scena dicendone solo il nome ("papera vai in
salotto" → attivata), quindi le scene MQTT darebbero i comandi dei
posti senza nessun file. Due cose pesano contro: le frasi italiane di
Speech-to-Phrase lasciano fuori le scene (la stessa frase, detta a
voce, è tornata vuota), e la risposta è quella fissa di Home Assistant,
"Ho attivato la scena…", che non dice niente quando quacksat rifiuta.

**Nomi nella lingua dell'utente.** I nomi visibili seguono `[announce]
language` ("Papera avanti", "Papera vai: cucina"): sono ciò che
mostra una dashboard e ciò che legge un agente LLM. Gli id delle entità
restano in inglese e stabili (§6), così un'automazione sopravvive a un
cambio di lingua.

Lasciati fuori dalla v1, di proposito:

- **Camminare all'indietro.** L'andatura del gemello non arretra dritta
  (arretra solo con un po' di yaw); un pulsante che promette "indietro"
  e cammina in curva è peggio di nessun pulsante. Torna dopo le misure
  di dicembre sull'andatura vera.
- **`robot.look`.** Vuole un punto in metri nel sistema della papera,
  che nessuno digita in una dashboard.
- **`robot.map_explore`.** Un lavoro autonomo lungo non è un pulsante da
  premere per sbaglio dal telefono. Può arrivare più avanti, disattivato
  di default.
- **`robot.get_frame`.** Non supportato sul robot fino a mediad.

I sensori sono letti con la cadenza che usa già l'annunciatore: ogni
2 s mentre la papera si muove o un viaggio è in corso, ogni 10 s
altrimenti.

### 4. Ogni movimento è un impulso

Non esiste un'entità "cammina finché non dico stop". Una pressione è un
solo `timed_move` limitato — la stessa pompa che usano i tool, con il
deadman di robotd dietro — e quando finisce la papera sta ferma.
Conseguenze per i topic:

- **I topic di comando non sono mai retained, e un messaggio retained
  su uno di essi viene scartato.** MQTT consegna a chi si sottoscrive il
  messaggio retained a ogni (ri)connessione: un "avanti" retained farebbe
  camminare la papera dopo ogni riavvio del broker. (Un messaggio
  retained pubblicato mentre quacksat è connesso gli arriva come uno
  normale, con il flag spento, ed è eseguito una volta: è MQTT 3.1.1, ed
  è un comando dal vivo.)
- **Un comando vuoto viene scartato.** Cancellare un messaggio retained
  ne manda uno vuoto a ogni abbonato; trovato dal vivo, quando
  cancellare l'"avanti" retained della prova ha fatto camminare la
  papera per due secondi. I pulsanti di Home Assistant mandano "PRESS".
- **QoS 0 e sessione pulita.** Un comando mandato mentre quacksat era
  offline va perso, non rieseguito ore dopo.
- **Una pressione durante una camminata viene scartata**, e scritta nel
  log. Le pressioni non si accodano: cinque tocchi impazienti sono un
  passo, non un metro.
- **Lo stop vince sempre.** Alza un flag che la pompa controlla a ogni
  tick (40 ms), così la camminata finisce al tick dopo e il deadman fa
  il resto. Non chiama `robot.stop` di robotd, che è un cambio di modo
  e non "stai ferma".

### 5. Un solo pilota, e chi può premere

L'ADR 0006 §4 vale: le gambe le guida una cosa alla volta.

- Una camminata da MQTT viene rifiutata mentre è in corso un viaggio
  della navigazione (il worker legge prima `robot.map_status`) o mentre
  un'altra camminata viene pompata in qualunque punto del processo
  (`body::timed_move`, §2), compresa quella di un turno vocale; una
  pressione mentre il worker è ancora occupato con la precedente riceve
  "occupata". Il rifiuto è pubblicato sul topic di risultato del
  comando (§6) e scritto nel log.
- Un viaggio partito da un pulsante di posto viene seguito dall'annunciatore come
  uno partito a voce, quindi su `agent` e `direct` la papera dice
  comunque com'è finito.
- L'albero dei topic guida un robot che può cadere da un tavolo: i
  passi manuali non hanno la guardia del dirupo (la guardia viaggia con
  i viaggi della navigazione, esattamente come per il tool
  `robot.move`). quacksat si rifiuta di avviare il thread MQTT senza
  `username` e `password`, e la documentazione d'installazione dà l'ACL
  di Mosquitto che lascia scrivere sotto il prefisso della papera solo
  a quell'utente e a quello di Home Assistant.

### 6. Topic e disponibilità

Con `node = "quacksat"` (default: il nome del backend attivo,
`[wyoming] name` o `[agent] name`, altrimenti `quacksat`; `[wyoming]
area` diventa l'area suggerita del dispositivo, così la papera e il suo
satellite finiscono nella stessa stanza):

- discovery: un payload di dispositivo su
  `homeassistant/device/<node>/config` (discovery per dispositivo,
  Home Assistant 2024.11+), retained;
- comandi: `quacksat/<node>/cmd/<entità>`;
- l'unico comando senza entità: `quacksat/<node>/cmd/go_to`, con il
  nome del posto come contenuto (§3);
- risultati: `quacksat/<node>/result/<entità>` (e `result/go_to`),
  `{ok, error?, detail?}`, non retained — ciò su cui un'automazione può
  aspettare;
- stato: `quacksat/<node>/state`, un solo documento JSON, retained,
  letto da ogni sensore con un value template;
- disponibilità, due topic con `availability_mode: all`: quella di
  quacksat (`online`, e `offline` come last will MQTT) e quella del
  robot (`offline` finché la lane di robotd è giù). In entrambi i casi
  Home Assistant mostra la papera in grigio invece di accettare
  pressioni che non possono arrivare.

### 7. Come un comando vocale arriva alle gambe

quacksat non capisce mai il parlato: sul percorso A la papera manda
l'audio, Home Assistant lo trascrive e decide. MQTT dà soltanto a Home
Assistant qualcosa da premere.

**Cosa è stato misurato.** Il 2026-10-10, su Home Assistant 2026.10.0
in un container e Speech-to-Phrase 1.4.3 addestrato su di esso, con
audio italiano dalla voce Alice di macOS inviato via Wyoming, e le
trascrizioni passate poi all'agente di Home Assistant:

| Strada | Agente di Home Assistant (testo) | Speech-to-Phrase (audio) |
|---|---|---|
| frase di serie su un pulsante ("premi / accendi papera avanti") | no: nessuna frase italiana o inglese copre i pulsanti; "accendi {name}" vale per light, switch, fan, media_player, input_boolean, climate | no |
| frase di serie su un sensore ("qual è lo stato di batteria papera") | sì, solo con quella formulazione | no: le sue frasi italiane sui sensori sono disattivate |
| nome nudo di una scena o di uno script | sì | no: le sue frasi italiane su scene e script sono disattivate |
| `custom_sentences/` + `intent_script` | sì | no: non legge i file di frasi di Home Assistant |
| automazione con trigger `conversation` | sì | sì: chiede a Home Assistant le frasi dei trigger |
| la stessa con un segnaposto libero (`{posto}`) | sì, qualunque nome | solo i nomi che conosce per altra via (sotto) |
| il suo file di frasi, con `{area}` | — | sì: ogni area di Home Assistant, una nuova dopo il riaddestramento |
| il suo file di frasi, con `{name}` | — | sì, i nomi nudi delle entità; il filtro per dominio viene ignorato |

La forma che funziona per entrambi è l'automazione con trigger a
frase. Gli esempi nel repo sono automazioni, mai `intent_script`.

**Il caso base: l'agente di Home Assistant, un vocabolario chiuso.** Il
repo fornisce gli esempi (`docs/home-assistant/`, italiano e inglese),
un'automazione per famiglia di comandi —

```yaml
triggers:
  - trigger: conversation
    command:
      - "papera (avanza|vai avanti|cammina)"
actions:
  - action: button.press
    target: {entity_id: button.quacksat_forward}
  - wait_for_trigger:
      - trigger: mqtt
        topic: quacksat/quacksat/result/forward
    timeout: 5
  - set_conversation_response: >-
      {% if not wait.trigger %}Il robot non risponde.
      {% elif wait.trigger.payload_json.ok %}Vado!
      {% else %}Non posso: {{ wait.trigger.payload_json.error }}{% endif %}
```

— così "papera avanza", "papera gira a destra", "papera fermati",
"papera fai la capriola", "papera guarda a sinistra", "papera quanta
batteria hai" ricevono come risposta ciò che è successo davvero, non un
"Vado!" detto prima che quacksat rifiutasse. È a questo che servono i
topic di risultato (§6).

Regole che gli esempi seguono, per via di chi ascolta:

- **Solo liste chiuse**, scritte per esteso nel trigger (skill,
  direzioni, pose della testa): un segnaposto libero arriva a
  Speech-to-Phrase solo per i nomi che conosce già.
- **Gli angoli della testa sono parole, non numeri.** "Guarda a sinistra / a
  destra / avanti" corrispondono a pose fisse.
- **I sensori hanno una frase loro.** La domanda di serie sui sensori
  non sopravvive a Speech-to-Phrase in italiano, quindi "papera quanta
  batteria hai" è un'automazione che legge il sensore e lo dice.
- **La corsia veloce resta veloce.** Queste frasi sono riconosciute in
  locale; con un LLM configurato come ripiego, Home Assistant le prova
  comunque per prime.

**I posti a voce: tre pezzi.** I posti sono l'unica lista aperta, e
cambiano man mano che la papera impara la casa.

1. **Un'automazione con un segnaposto libero** raccoglie ogni "papera
   vai in …" e passa il nome a quacksat, che dice se lo conosce:

   ```yaml
   triggers:
     - trigger: conversation
       command:
         - "papera (vai|va) [in|nel|nella|nello|al|alla|allo|sul|sulla|sullo] {posto}"
   actions:
     - action: mqtt.publish
       data:
         topic: quacksat/quacksat/cmd/go_to
         payload: "{{ trigger.slots.posto }}"
     - wait_for_trigger:
         - trigger: mqtt
           topic: quacksat/quacksat/result/go_to
       timeout: 5
     - set_conversation_response: >-
         {% if not wait.trigger %}Il robot non risponde.
         {% elif wait.trigger.payload_json.ok %}Ci vado.
         {% else %}{{ wait.trigger.payload_json.error }}{% endif %}
   ```

   Con faster-whisper basta questa per qualunque posto. Misurato: le
   trascrizioni "papera va in Cucina", "papera vai nella Camera da
   letto", "papera vai in garage" sono arrivate tutte all'automazione
   con il nome giusto.
2. **Un file di frasi di Speech-to-Phrase sulle aree**, fornito come
   `docs/home-assistant/speech-to-phrase/it/quacksat.yaml`:

   ```yaml
   language: it
   intents:
     QuacksatGoArea:
       data:
         - sentences:
             - "papera (vai|va) (in|nel|nella|nello|al|alla|allo|sul|sulla|sullo) {area}"
   ```

   Ogni stanza di Home Assistant diventa un posto che Speech-to-Phrase
   sa sentire, senza liste da mantenere: un'area creata durante la
   prova ("Studio") è stata sentita dopo il riaddestramento. Da qui il
   consiglio della documentazione d'installazione: **dare ai posti
   della papera i nomi delle aree di Home Assistant.**
3. **Un blueprint per i posti che non sono stanze** ("Papera, vai in un posto
   (elenco di frasi)", un elenco di frasi come "papera vai in terrazza").
   Speech-to-Phrase le impara attraverso le frasi dei trigger, senza un
   file suo; l'elenco si aggiorna a mano quando la papera impara o
   dimentica un posto.

Cosa questo caso non sa fare: comporre ("vai in cucina e poi
siediti"), capire una formulazione che nessuno ha scritto, o rispondere
a qualunque cosa fuori dall'elenco. Dà un vocabolario fisso — una
ventina di frasi, più i posti — veloce, locale, e che copre i comandi
brevi, quelli per cui serve la voce.

**Un'aggiunta: un agente LLM.** Dove l'agente di Home Assistant è un
LLM, o Arkimede raggiunge Home Assistant tramite il suo server MCP, i
pulsanti esposti gli sono visibili così come sono, e diventano possibili
le richieste libere. Per tutto ciò che ha un parametro va bene uno
script di Home Assistant (per esempio `script.papera_vai_al_posto` con
il suo posto, o `script.papera_testa` con i suoi angoli; qui non ne
viene fornito nessuno): uno script
esposto è un tool con un nome, una descrizione e dei campi, che un LLM
chiama in modo affidabile e che aspetta il topic di risultato come le
automazioni. In quacksat non cambia niente per questo.

**Senza voce: automazioni e dashboard.** Le stesse entità sono pulsanti
e sensori normali: una card sul telefono, "alle otto vai in cucina",
"batteria sotto il 20 %, avvisami", e gli esiti detti a voce del
percorso A (Domande aperte).

**Su un'installazione vera, senza la papera.** Il 2026-10-10 tutto
questo ha girato contro un'installazione di Home Assistant in uso ogni
giorno (con l'add-on Mosquitto), con quacksat su un Mac di sviluppo:
microfono e altoparlanti del Mac tramite `sox`, un robotd `--fake` vero
come corpo, un demone di navigazione finto. Il dispositivo è comparso
per discovery con le sue 28 entità; pulsanti e cursori sono arrivati a
robotd e sono tornati con le sue risposte (una skill rifiutata con "the
policy is not driving", il robot che risponde); e la voce ha chiuso il
giro da sola: "hey Daffy" sul satellite, "papera cammina" trascritto
dalla pipeline di casa, l'automazione a frase che preme il pulsante,
robotd che cammina due secondi, "Fatto." detto dal Mac — due secondi e
mezzo dalla trascrizione alla risposta. Ciò che aggiungerà la papera
fisica sono l'audio della scheda e le gambe vere (dicembre 2026).

### 8. Configurazione

```toml
[mqtt]
enabled = false
host = "homeassistant.local"
port = 1883
username = ""            # obbligatorio se abilitato
password = ""            # obbligatorio se abilitato
node = ""                # default: il nome del backend, altrimenti "quacksat"
device_name = ""         # default: "Papera" o "Duck", secondo [announce] language
discovery_prefix = "homeassistant"
base_topic = "quacksat"
step_s = 2.0             # una pressione di avanti, massimo 3
turn_s = 2.0             # una pressione di gira, massimo 3
```

## Conseguenze

- Il percorso A diventa un robot intero: la voce attraverso Assist, il
  corpo attraverso le entità, e lo stesso corpo raggiungibile da
  automazioni, dashboard e da qualunque agente che già comandi Home
  Assistant.
- Una superficie di rete in più su una macchina che cammina. È
  opzionale, autenticata, recintata da ACL, solo a impulsi, e non può
  mandare niente che la tabella dei tool non potesse; resta comunque
  una porta, e la documentazione d'installazione lo dice.
- Un broker diventa una dipendenza per chi lo accende. Chi usa HAOS ce
  l'ha a un clic; senza, il satellite funziona esattamente come prima.
- Il payload di discovery è un contratto con Home Assistant: gli id
  delle entità derivano da `node` e dalla chiave dell'entità e non
  devono cambiare dopo la pubblicazione, o le automazioni si rompono.
  I pulsanti dei posti sono l'eccezione per natura: dimenticare un
  posto rimuove il suo pulsante, e l'automazione che lo premeva.
- Il percorso A a voce è un vocabolario chiuso, tranne i posti, che
  sono aperti con faster-whisper e ampi quanto le aree di Home
  Assistant con Speech-to-Phrase. Basta per i comandi brevi ed è tutto
  locale; le richieste libere richiedono un agente LLM, che questa ADR
  non richiede e non impedisce.
- quack-control (la pagina nel browser) e questa ADR rispondono a
  persone diverse: la pagina è il pannello di controllo della papera
  sulla LAN, MQTT è la porta verso Home Assistant. Entrambi passano per
  la regola del pilota unico.
- Provabile senza la papera: robotd `--fake`, un Mosquitto locale,
  `mosquitto_sub`/`mosquitto_pub` per i topic, e un container di Home
  Assistant per la discovery; i rifiuti che provano che la lane è viva
  sono quelli di `docs/study/live-without-a-duck.it.md`.

## Domande aperte

- **Esiti detti a voce sul percorso A.** Il backend wyoming non dice
  niente di sua iniziativa (`docs/todo.it.md`). Il sensore del viaggio
  dà a Home Assistant ciò che serve per annunciare "sono arrivata in
  cucina" attraverso il satellite; se un satellite Wyoming accetti
  `assist_satellite.announce` è da verificare prima di scegliere fra
  quello e un'automazione d'esempio.
- **Speech-to-Phrase come add-on.** Due cose a cui il container non
  poteva rispondere e l'installazione di casa sì: da dove l'add-on
  legge i suoi file di frasi (`/share/speech-to-phrase/custom_sentences/`
  è un ricordo, non una verifica), e quando si riaddestra. Il programma
  si riaddestra all'avvio, a intervalli o a ogni connessione, secondo
  come viene lanciato; se l'add-on fa solo la prima, una nuova area o
  una nuova frase del blueprint si sente dopo averlo riavviato.
