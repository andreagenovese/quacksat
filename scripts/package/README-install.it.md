# Installare quacksat sull'anatra

Questo pacchetto installa `quacksat`, il satellite vocale del Microduck,
sulla scheda dell'anatra (Radxa Zero 3, Linux aarch64, Debian 13) dal tuo
computer, via ssh. Non servono né una copia del repository né una
compilazione. Copia inglese: [README-install.md](README-install.md).

Cosa serve:

- un'anatra preparata da microduck: robotd in funzione, il gruppo `robot`
  a cui appartiene il suo socket, il gruppo `audio`, e `arecord`/`aplay`
  (alsa-utils);
- internet sull'anatra una volta, durante l'installazione: i modelli
  della wake word si scaricano lì (vedi la sezione 3, «Wake word»);
- accesso ssh all'anatra con un account che ha `sudo` (`microduck`
  sull'immagine della scheda; le immagini più vecchie avevano `radxa`);
- sul tuo computer: `bash`, `ssh`, `scp`, `tar` e `shasum` o `sha256sum`
  (macOS e Linux li hanno tutti).

Facoltativo, accanto: `quack-navd` (il demone di navigazione,
<https://github.com/andreagenovese/quacknav>) perché l'anatra si possa
mandare da qualche parte; senza, l'anatra ascolta e risponde, e dice che
non può muoversi per casa.

## 1. Scaricare e verificare

Da <https://github.com/andreagenovese/quacksat/releases>, il pacchetto e
il suo checksum. Questo pacchetto è `@VERSION@`; per uno più nuovo si mette
in `V` la sua versione (il tag senza la `v`):

```sh
V=@VERSION@
gh release download "v$V" --repo andreagenovese/quacksat \
    --pattern "quacksat-$V-aarch64-linux.tar.gz*"
# oppure, senza gh:
curl -LO "https://github.com/andreagenovese/quacksat/releases/download/v$V/quacksat-$V-aarch64-linux.tar.gz"
curl -LO "https://github.com/andreagenovese/quacksat/releases/download/v$V/quacksat-$V-aarch64-linux.tar.gz.sha256"

shasum -a 256 -c "quacksat-$V-aarch64-linux.tar.gz.sha256"   # oppure sha256sum -c
```

Deve stampare `OK`. Poi si scompatta:

```sh
tar xzf "quacksat-$V-aarch64-linux.tar.gz"
cd "quacksat-$V"
```

## 2. Installare

```sh
./install-on-duck.sh --dry-run microduck@192.168.1.42   # facoltativo: stampa cosa farebbe
./install-on-duck.sh microduck@192.168.1.42
```

Copia i file sull'anatra, poi lì con `sudo`:

| sull'anatra | da questo pacchetto |
|---|---|
| `/usr/local/bin/quacksat` | `bin/quacksat` |
| `/etc/systemd/system/quacksat.service` | `systemd/quacksat.service` |
| `/etc/sysusers.d/quacksat.conf` (utente `quacksat`) | `systemd/sysusers.d/quacksat.conf` |
| `/etc/robot/quacksat.toml`, modo 0640 root:quacksat — **solo se non c'è** | `quacksat.example.toml` |
| `/var/lib/quacksat/models/` — i file già presenti restano | scaricati da `fetch-wake-models.sh` |

Poi abilita e riavvia il servizio, stampando ogni comando.
`SSH_OPTS="-p 2222"` passa opzioni a ssh e scp;
`WAKE_MODELS="hey_daffy.onnx hey_jarvis_v0.1.onnx"` scarica più modelli
di wake word del solo `hey_daffy.onnx` predefinito.

La config d'esempio avvia il satellite in **modalità bring-up**
(`backend = "none"`): ascolta, si sveglia su «hey Daffy» e cinguetta, e
non parla con nessuno. Il passo dopo è scegliere un backend.

## 3. La config: /etc/robot/quacksat.toml

Si modifica sull'anatra (`sudo nano /etc/robot/quacksat.toml`), poi
`sudo systemctl restart quacksat`. Ogni chiave omessa prende il suo
default; una chiave sconosciuta ferma il satellite con un messaggio che
la nomina.

**Conterrà delle API key.** Va tenuta leggibile solo da root e dal
servizio, come la lascia l'installer:

```sh
sudo chown root:quacksat /etc/robot/quacksat.toml
sudo chmod 640 /etc/robot/quacksat.toml
```

(Non 600 di proprietà di root: il servizio gira come `quacksat` e non
potrebbe più leggerla.)

### Il backend

`backend` sceglie con chi parla l'anatra:

| backend | cosa gira dove | cosa impostare |
|---|---|---|
| `"none"` | bring-up: solo wake word e cinguettio | niente |
| `"direct"` | l'anatra stessa chiama un endpoint LLM, uno di trascrizione e uno di sintesi vocale, in dialetto OpenAI (una chiave cloud o server locali) | `[direct.llm]`, `[direct.stt]`, `[direct.tts]` |
| `"agent"` | l'anatra manda l'audio a un bridge su un server, che fa STT → LLM → TTS | `[agent] url` (e il bridge, sotto) |
| `"wyoming"` | l'anatra è un satellite di Home Assistant Assist | `[wyoming] bind`, poi aggiungerla in Home Assistant; `[mqtt]` (sotto) aggiunge il corpo |

`direct`, il più semplice senza un server:

```toml
backend = "direct"

[direct.llm]
base_url = "https://api.example.com/v1"   # qualunque endpoint /chat/completions
api_key = "LA-TUA-CHIAVE-LLM"
model = "il-tuo-modello"
tool_calling = true                        # serve ai tool del robot

[direct.stt]
base_url = "https://api.example.com/v1"   # qualunque endpoint /audio/transcriptions
api_key = "LA-TUA-CHIAVE-STT"
language = "it"                            # sceglie anche le frasi dell'anatra

[direct.tts]
base_url = "https://api.example.com/v1"   # qualunque endpoint /audio/speech che renda WAV
api_key = "LA-TUA-CHIAVE-TTS"
voice = "la-tua-voce"
```

`[direct.mcp]` (spento di default) offre i tool del robot agli agenti
che parlano MCP sulla porta 8767; il suo `token` è obbligatorio.

`agent`:

```toml
backend = "agent"

[agent]
url = "ws://<host-del-bridge>:8765"
# token = "..."           # se il bridge ne chiede uno
name = "anatra-cucina"    # unico per anatra quando più anatre condividono un bridge
```

Il **bridge** non è in questo pacchetto: gira su un server (Python 3.11+
o Docker), non sull'anatra. Si prende dallo stesso tag, o dall'archivio
«Source code» della release o così:

```sh
git clone --branch v@VERSION@ --depth 1 https://github.com/andreagenovese/quacksat
cd quacksat/bridge
cp config.example.toml config.toml   # poi modificala: url e chiavi di LLM/STT/TTS
docker compose up -d --build         # oppure i passi con il venv in bridge/README.it.md
```

`wyoming`: si punta l'integrazione Wyoming di Home Assistant a
`<anatra>:10700` (Impostazioni → Dispositivi e servizi → Aggiungi
integrazione → Wyoming Protocol). STT, intenti e TTS sono quelli di Home
Assistant.

### Wake word

```toml
[wake]
mode = "openwakeword"
models_dir = "/var/lib/quacksat/models"
model = "hey_daffy.onnx"
threshold = 0.5
```

I modelli **non** sono in questo pacchetto: openWakeWord pubblica i suoi
modelli pre-addestrati sotto CC BY-NC-SA 4.0 (non commerciale), quindi
l'installer li scarica sull'anatra, dalla loro fonte, ciascuno
verificato col suo sha256: i due modelli di feature condivisi
(`melspectrogram.onnx`, `embedding_model.onnx`) dalla release di
openWakeWord, e «hey Daffy», la wake word di quacksat, da questo
repository al commit del pacchetto. Senza internet sull'anatra, si
scaricano altrove con `fetch-wake-models.sh <dir> hey_daffy.onnx` e si
copiano in `/var/lib/quacksat/models/`. Un'altra frase: si mette lì il
suo `.onnx` e si imposta `model`. Se non ti sente, si abbassa
`threshold` (0.4, 0.35); se scatta sulla TV, si alza.

### Audio

```toml
[audio]
playback_device = "plughw:aic3104"
capture_device = "plughw:aic3104,0"
```

Il codec dell'anatra, coi nomi che gli dà microduck. Il microfono deve
essere libero: `audio.pet_detect` di robotd deve restare `false` in
`/etc/robot/robotd.toml`, il default di fabbrica; lì è consigliato anche
`audio.greet = false` (il quack all'avvio farebbe a gara con la prima
frase). L'altoparlante è condiviso coi suoni di robotd: i cinguettii
dell'anatra sono di robotd, la voce è di quacksat, mai insieme. Non c'è
cancellazione d'eco: l'anatra non ascolta mentre parla.

### Navigazione e cosa dice l'anatra da sola

```toml
[nav]
enabled = true
socket = "/run/quack-nav/nav.sock"   # il socket di quack-navd

[announce]
enabled = true
language = ""      # "it" o "en"; vuoto: [direct.stt] language, poi quella del bridge, poi inglese
journeys = true    # "Sono arrivata in cucina", "Mi sono fermata", ...
own_motion = true  # perché l'anatra cammina quando nessuno gliel'ha chiesto
```

Con quack-navd che risponde su `socket`, i suoi tool (dove sono, ricorda
questo posto, vai in cucina, mappa la casa) si aggiungono a quelli del
satellite. L'anatra dice com'è finito un viaggio su cui è stata mandata,
tra un turno e l'altro: con `direct` con la sua voce, con `agent`
attraverso il bridge. Il backend `wyoming` per ora non dice niente da sé.

### Il corpo in Home Assistant (facoltativo, ogni backend)

```toml
[mqtt]
enabled = true
host = "homeassistant.local"   # il broker MQTT (l'add-on Mosquitto di Home Assistant)
port = 1883
username = "quacksat"          # obbligatori: i topic comandano un robot che cammina
password = "…"
```

La papera compare da sola in Home Assistant, come dispositivo con
pulsanti per camminare, girare, fermarsi ed eseguire le skill, la testa
come cursori, e sensori di batteria, salute e viaggio. L'utente del
broker, le automazioni a frase d'esempio e i topic sono in
[docs/home-assistant/README.it.md](https://github.com/andreagenovese/quacksat/blob/main/docs/home-assistant/README.it.md).

## 4. Controllare

Sull'anatra:

```sh
systemctl status quacksat
journalctl -u quacksat -f
```

Un avvio sano scrive `quacksat starting`, `listening` sul dispositivo di
cattura, la connessione a robotd e, quando quack-navd risponde, i suoi
tool. Senza robotd o quack-navd parte lo stesso, lo dice nel log e
riprova. Si dice «hey Daffy»: l'anatra cinguetta.

## 5. Aggiornare

Si scarica il pacchetto più nuovo, si verifica, si scompatta e si lancia
il suo `./install-on-duck.sh` allo stesso modo. Il binario, la unit e
l'account vengono sostituiti; la tua `/etc/robot/quacksat.toml` e i
modelli in `/var/lib/quacksat/models` restano. Una release nuova può
aggiungere chiavi alla config (prendono il loro default): si confronta
con il `quacksat.example.toml` del pacchetto.

## 6. Disinstallare

Sull'anatra:

```sh
sudo systemctl disable --now quacksat
sudo rm /usr/local/bin/quacksat /etc/systemd/system/quacksat.service /etc/sysusers.d/quacksat.conf
sudo systemctl daemon-reload
# lasciati apposta: /etc/robot/quacksat.toml (le tue chiavi) e
# /var/lib/quacksat/ (i modelli). Rimuoverli, e `sudo userdel quacksat`,
# per non lasciare niente.
```

Altro: il README del progetto, <https://github.com/andreagenovese/quacksat>.
