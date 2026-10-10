# Provare quacksat senza la papera

Ogni backend gira oggi su un portatile, senza un Microduck nella
stanza: un `robotd --fake` vero fa da corpo, microfono e altoparlanti
del portatile fanno da quelli della papera. Questa guida percorre le tre
modalità su un Mac (su Linux è uguale, con `arecord`/`aplay` al posto di
sox). Copia inglese, quella di riferimento:
[try-without-a-duck.md](try-without-a-duck.md). Il perché di ogni passo,
e cosa dimostra ogni risposta, è in
[study/live-without-a-duck.it.md](study/live-without-a-duck.it.md).

Cosa un portatile non può darti: le gambe vere (il robot finto risponde,
non cammina), una policy (le skill vengono rifiutate con "the policy is
not driving", che è il robot che risponde), e il percorso audio della
papera.

## 1. Comune a tutte le modalità

**Strumenti:** Rust (1.89 o successivo), `sox` (`brew install sox`), git.

**Il robot, finto.** Compila robotd dal repo di Pollen al tag a cui è
fissato quacksat, e avvialo su un percorso di socket corto (macOS limita
il percorso di un socket unix a circa cento caratteri):

```sh
git clone --branch daemon-v0.14.4 https://github.com/pollen-robotics/microduck.git
cd microduck && cargo build --release -p robotd
mkdir -p /tmp/qs && target/release/robotd --fake --socket /tmp/qs/robotd.sock
```

**Il satellite.** In questo checkout:

```sh
scripts/fetch-wake-models.sh models hey_daffy.onnx   # modelli di base condivisi + "hey Daffy"
cargo build -p quacksat
```

**La configurazione da cui parte ogni modalità**, in un file tuo
(`quacksat-dev.toml`, fuori da git se conterrà chiavi):

```toml
robotd_socket = "/tmp/qs/robotd.sock"

[audio]
capture_command = ["sox", "-q", "-d", "-t", "raw", "-r", "48000", "-c", "2", "-e", "signed-integer", "-b", "16", "-"]
playback_program = "scripts/aplay-shim-macos.sh"

[wake]
mode = "openwakeword"
models_dir = "models"
model = "hey_daffy.onnx"
threshold = 0.5

[announce]
language = "it"          # o "en"
```

Avvia con `target/debug/quacksat quacksat-dev.toml`; il log dice cosa
ha risposto (le skill del robot, la navigazione, il backend). macOS
chiede una volta l'accesso al microfono per il terminale.

Senza il demone di navigazione (`quack-navd`, repo
[quacknav](https://github.com/andreagenovese/quacknav)) la papera
risponde e si muove ma non può essere mandata in un posto, e lo dice;
metti `[nav] enabled = false` perché smetta di chiederlo.

## 2. wyoming — la papera in Assist di Home Assistant

Cosa serve: un Home Assistant con una pipeline di Assist (STT, TTS e
agente qualsiasi), raggiungibile dal portatile.

```toml
backend = "wyoming"

[wyoming]
bind = "0.0.0.0:10700"
name = "quacksat"
```

In Home Assistant: Impostazioni → Dispositivi e servizi → Aggiungi
integrazione → **Wyoming Protocol**, host = l'IP del portatile, porta
10700. Scegli la pipeline sul nuovo dispositivo satellite, poi di' "hey
Daffy" e una domanda. Il log mostra `wake`, la trascrizione e `tts
played`.

**Anche il corpo** (ADR 0007): aggiungi `[mqtt]` con il broker e un
utente suo, e la papera compare come dispositivo con pulsanti, cursori e
sensori; le automazioni d'esempio la fanno camminare a voce. È tutto in
[home-assistant/README.it.md](home-assistant/README.it.md); la sezione
"Provarlo senza la papera" lì ha la parte di Home Assistant.

```toml
[mqtt]
enabled = true
host = "homeassistant.local"
username = "quacksat"
password = "…"
```

## 3. agent — attraverso il bridge di riferimento

Cosa serve: Python 3.11+. Comincia dalla modalità finta del bridge, che
non ha bisogno di nessun servizio AI:

```sh
cd bridge
python3 -m venv .venv && .venv/bin/pip install websockets
.venv/bin/python bridge.py --fake
```

```toml
backend = "agent"

[agent]
url = "ws://127.0.0.1:8765"
name = "quacksat"
```

Di' "hey Daffy" e qualunque cosa: il bridge finto risponde con una frase
fissa e un tono, e chiama `robot.sound` una volta per turno — il "this
robot has no voice" del robot è la prova che il percorso dei tool è
arrivato a robotd. Per una conversazione vera, `bridge.py --config
config.toml` con i tuoi endpoint STT, LLM e TTS (`bridge/README.md`).

## 4. direct — il satellite chiama i servizi da solo

Cosa serve: tre endpoint in dialetto OpenAI — un LLM `/chat/completions`
(va bene un ollama locale), uno STT `/audio/transcriptions` (per esempio
speaches / faster-whisper-server) e un TTS `/audio/speech` che
restituisce WAV (per esempio openedai-speech con Piper).

```toml
backend = "direct"

[direct.llm]
base_url = "http://localhost:11434/v1"
model = "qwen3:8b"
tool_calling = true

[direct.stt]
base_url = "http://localhost:9000/v1"
language = "it"

[direct.tts]
base_url = "http://localhost:9100/v1"
voice = "it_IT-paola-medium"
```

Di' "hey Daffy, guarda a sinistra": l'LLM chiama `robot.head` e il robot
finto risponde. Per pilotare i tool senza nessun servizio vocale, accendi
il server MCP della papera e chiamalo con curl:

```toml
[direct.mcp]
enabled = true
port = 8767
token = "change-me"
```

```sh
curl -s -X POST http://127.0.0.1:8767/mcp -H "Authorization: Bearer change-me" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}'
```

## Quando qualcosa tace

- **Non si sveglia niente:** controlla che nessun `sox` vecchio tenga il
  microfono (`pgrep -fl sox`): fermare il satellite con `pkill` può
  lasciarlo orfano. Alza il guadagno d'ingresso, o abbassa `[wake]
  threshold`.
- **La papera sente la propria risposta:** abbassa il volume; altoparlante
  e microfono del portatile sono più vicini di quelli della papera.
- **"the policy is not driving", "this robot has no voice":** non sono
  errori. Il robot finto ha risposto, quindi la linea funziona.
