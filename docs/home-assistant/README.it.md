# La papera in Home Assistant

quacksat può pubblicare il corpo della papera in Home Assistant come un
dispositivo: pulsanti per farla camminare, girare, fermare ed eseguire
le skill, cursori per la testa, e sensori per la batteria, la salute e,
con la navigazione, dove si trova e come va il viaggio. Sul percorso
`wyoming` è questo che rende la papera un robot intero: Assist le dà la
voce, queste entità le danno il corpo. Funziona su ogni backend. Copia
canonica in inglese: [README.md](README.md). La decisione e le misure
che la sostengono: [ADR 0007](../adr/0007-home-assistant-entities-over-mqtt.it.md).

## Cosa serve

- Un broker MQTT. In Home Assistant OS: l'add-on **Mosquitto broker**, e
  l'integrazione **MQTT** collegata a esso.
- Un utente del broker per la papera. I topic guidano un robot che
  cammina, quindi quacksat si rifiuta di avviare MQTT senza utente e
  password.

ACL di Mosquitto che lascia scrivere sotto il prefisso della papera solo
alla papera e a Home Assistant (la cartella `customize` dell'add-on, o un
tuo `acl_file`):

```text
user quacksat
topic readwrite quacksat/#
topic write homeassistant/#

user homeassistant
topic readwrite #
```

## Dalla parte di quacksat

In `/etc/robot/quacksat.toml`:

```toml
[mqtt]
enabled = true
host = "homeassistant.local"
port = 1883
username = "quacksat"
password = "…"
# node = "quacksat"        # vuoto: il nome del backend ([wyoming] name)
# device_name = "Papera"   # vuoto: "Papera" o "Duck", secondo [announce] language
step_s = 2.0               # una pressione di avanti, in secondi (al massimo 3)
turn_s = 2.0               # una pressione di gira, circa 90°
```

Riavvia quacksat: il dispositivo compare da solo in Home Assistant (MQTT
discovery), nella stessa area del satellite se `[wyoming] area` è
impostato.

## Cosa compare

Con il nodo `quacksat` e i nomi in italiano:

| Entità | Cosa fa |
|---|---|
| `button.quacksat_forward` | cammina in avanti per `step_s` (≈ 20 cm in 2 s) |
| `button.quacksat_turn_left` / `_turn_right` | cammina in curva per `turn_s` (la papera non gira sul posto) |
| `button.quacksat_stop` | ferma subito la camminata |
| `number.quacksat_head_pitch` / `_yaw` / `_roll` | la testa, in radianti, entro i limiti del tool |
| `button.quacksat_head_center` | testa di nuovo al centro |
| `button.quacksat_skill_<nome>` | una per skill che il robot elenca |
| `button.quacksat_sound_<tag>` | i suoni, disattivati finché non li attivi |
| `button.quacksat_place_<nome>` | uno per posto, con la navigazione |
| `sensor.quacksat_battery`, `binary_sensor.quacksat_problem`, `sensor.quacksat_mode` | batteria %, salute (il motivo come attributo), modo |
| `sensor.quacksat_where`, `sensor.quacksat_journey` | con la navigazione: il posto più vicino, lo stato del viaggio |
| `sensor.quacksat_last_answer` | la risposta all'ultimo comando, come la direbbe la papera |

Ogni movimento è un impulso: una pressione cammina per qualche secondo e
si ferma. Una pressione mentre la papera cammina, o mentre è in corso un
viaggio, viene rifiutata ("sto già camminando"), e lo **stop** vince
sempre. I passi manuali non hanno la guardia del dirupo (la guardia
viaggia con i viaggi della navigazione): tieni la papera lontana dal
bordo di un tavolo quando premi avanti.

## A voce

L'agente di Home Assistant non preme i pulsanti per nome, e
Speech-to-Phrase impara solo le frasi delle automazioni. Misurato il
2026-10-10, vedi l'ADR 0007 §7. Quindi la voce viene da tre file in
questa cartella:

1. **[automations.it.yaml](automations.it.yaml)** (inglese:
   [automations.yaml](automations.yaml)). Un'automazione per famiglia di
   comandi, con un trigger a frase: "papera avanza", "papera gira a
   sinistra", "papera fermati", "papera fai la capriola", "papera guarda
   a sinistra", "papera quanta batteria hai", "papera vai in cucina".
   Ognuna aspetta la risposta di quacksat e dice cosa è successo
   davvero. Incollale in `automations.yaml` (cambia `quacksat` nei topic
   e negli id se il tuo nodo è diverso) e ricarica le automazioni.
2. **[speech-to-phrase/](speech-to-phrase/)**, solo se usi
   Speech-to-Phrase. Un file di frasi che gli insegna "papera vai in
   <area>" per ogni area di Home Assistant. Va nella cartella delle frasi
   personalizzate dell'add-on, sotto la lingua (`it/` o `en/`). Il
   percorso esatto nell'add-on è ancora da verificare; dopo aver aggiunto
   un'area può servire riavviare l'add-on perché si riaddestri. **Dai ai
   posti della papera i nomi delle tue aree** e ogni stanza funziona
   senza liste da mantenere.
3. **[blueprints/quacksat_go_to.it.yaml](blueprints/quacksat_go_to.it.yaml)**
   (inglese: [quacksat_go_to.yaml](blueprints/quacksat_go_to.yaml)). Per i
   posti che non sono stanze ("il divano"): una frase per posto, che
   Speech-to-Phrase impara dal trigger. Copialo in
   `config/blueprints/automation/quacksat/` e crea un'automazione da esso.

Con faster-whisper basta il primo file, posti compresi: l'automazione
"vai" accetta qualunque nome e quacksat dice se lo conosce.

Gli errori che quacksat rimanda sono nella lingua della papera
(`[announce] language`), perché le automazioni li leggono ad alta voce.

## I topic, per le tue automazioni

Con `base_topic = "quacksat"` e il nodo `quacksat`:

| Topic | Verso | Contenuto |
|---|---|---|
| `quacksat/quacksat/cmd/<comando>` | alla papera | `forward`, `turn_left`, `turn_right`, `stop`, `head_center`: qualunque cosa non vuota; `head_pitch`/`_yaw`/`_roll`: un numero; `skill`: il nome di una skill; `sound`: un tag; `go_to`: un posto così come è stato detto |
| `quacksat/quacksat/result/<comando>` | dalla papera | `{"ok": true, …}` oppure `{"ok": false, "error": "…", "detail": "…"}`, non retained |
| `quacksat/quacksat/state` | dalla papera | un solo documento JSON, retained: `battery`, `volts`, `healthy`, `reason`, `mode`, `walking`, e con la navigazione `place`, `at_place`, `journey`, `journey_reason` |
| `quacksat/quacksat/availability`, `…/robot` | dalla papera | `online` / `offline`: quacksat stesso, e la sua linea verso il robot |

I comandi non vengono mai eseguiti se retained, né se vuoti (è un
messaggio retained che viene cancellato), e le pressioni non si accodano.

## Provarlo senza la papera

Basta quacksat su un portatile con un robotd `--fake` vero come corpo;
[docs/try-without-a-duck.it.md](../try-without-a-duck.it.md) prepara
quella parte. Per Home Assistant, o un container (con accanto un
container di Mosquitto) o quello che hai già: il 2026-10-10 l'intero
giro ha girato contro un'installazione di Home Assistant in uso ogni
giorno, con quacksat su un Mac — il dispositivo per discovery, i
pulsanti e i cursori, e "hey Daffy, papera cammina" attraverso il
satellite Wyoming, l'automazione, la camminata e il "Fatto." detto a
voce (ADR 0007 §7).

Sul tuo Home Assistant, con l'add-on Mosquitto e l'integrazione MQTT già
configurati:

1. Crea un utente del broker per la papera (va bene un utente di Home
   Assistant: l'add-on li accetta).
2. Aggiungi `[mqtt]` alla configurazione del portatile, con `host` =
   l'indirizzo di Home Assistant, e avvia quacksat: il dispositivo
   compare da solo.
3. Per la voce, metti `[wyoming] bind = "0.0.0.0:10700"` e aggiungi
   l'integrazione Wyoming Protocol con l'IP del portatile; poi incolla
   le automazioni.

Un rifiuto come "the policy is not driving" su una skill è il robot che
risponde, e dimostra che la linea funziona.

**Pulire dopo.** Fermare quacksat lascia il dispositivo in Home
Assistant, non disponibile. Eliminalo dalla sua pagina, poi svuota i
messaggi retained lasciati sul broker (`-r -n` pubblica un messaggio
retained vuoto, che lo cancella):

```sh
for t in homeassistant/device/quacksat/config quacksat/quacksat/state \
         quacksat/quacksat/availability quacksat/quacksat/robot; do
  mosquitto_pub -h homeassistant.local -u quacksat -P '…' -r -n -t "$t"
done
```
