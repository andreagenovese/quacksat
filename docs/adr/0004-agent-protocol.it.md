# ADR 0004: Il protocollo agent (strada B)

- Stato: accettata
- Data: 2026-08-31
- Input: ADR 0002 (backend intercambiabili), ADR 0003 (audio
  half-duplex), `docs/agent-backend-plan.md`, ricognizione della
  piattaforma [Arkimede](https://arkimede.ai/)

## Contesto

La strada B collega il satellite a un agente AI: audio in uscita, audio
di ritorno, e l'agente in grado di agire tramite tool — sia la domotica
(propria dell'agente) sia il corpo del robot. Il protocollo deve essere
neutro ("bring your own agent"): implementabile da Arkimede, da un
bridge di riferimento, o da qualunque altra cosa, senza che quacksat
sappia quale.

## Decisioni

### 1. WebSocket, non WebRTC (per ora)

I frame di testo trasportano eventi JSON; i frame binari trasportano
audio PCM raw. Motivazione: la conversazione è a turni e half-duplex
per progetto (niente AEC — ADR 0003), gira sulla LAN di casa, e un
protocollo neutro deve essere implementabile con la libreria standard
di qualunque linguaggio. I vantaggi reali di WebRTC (audio su reti con
perdite, NAT traversal, budget di jitter sotto i 50 ms, tracce video)
qui non comprano ancora nulla e costringerebbero ogni implementatore
alla complessità di ICE/SDP/DTLS.

Il protocollo è deliberatamente agnostico rispetto al trasporto: gli
eventi mappano 1:1 su un datachannel WebRTC e l'audio su una track.
L'operatività remota fuori dalla LAN, il barge-in full-duplex (quando
esisterà un AEC) o il video live sono i trigger dichiarati per
aggiungere un binding WebRTC. Non in v0.

### 2. Wake word e VAD restano sul satellite

Il bridge non vede mai audio always-on: il satellite fa streaming solo
dopo un wake locale (pre-roll incluso) o quando il bridge riapre
esplicitamente il microfono per un turno di follow-up (`listen.start`
— è questo che dà alla strada B conversazioni multi-turno senza
ripetere la wake word, cosa che la strada Wyoming non può fare). Il
satellite chiude ogni enunciato con il proprio VAD locale
(`utterance.end`); anche il bridge può troncarlo (`listen.stop`).

### 3. Riproduzione half-duplex

Il TTS va in streaming nel player a figlio unico del satellite (ADR
0003); i frame del microfono vengono scartati mentre l'anatra parla, e
il rilevatore di wake viene resettato quando l'ascolto riprende.
Niente barge-in finché non esiste un AEC.

### 4. I tool vengono eseguiti sul satellite, dietro una allowlist esplicita

Il satellite dichiara i propri tool in `session.start` (nome,
descrizione, parametri JSON-Schema — proiettabili direttamente sia
sugli schemi tool di OpenAI sia sui listing di tool MCP). Il
bridge/agente ne richiede l'esecuzione con `tool.call`; il satellite
valida contro una allowlist esaustiva (il pattern `permits()` di btd:
un tool nuovo non compila finché non viene classificato) e traduce in
RPC verso robotd.

Regole di sicurezza cablate nella semantica dei tool, non lasciate
all'agente:

- **`robot.move` è a tempo**: `{vx, vy, vyaw, duration_s}` con durata
  massima limitata (clamp). Il satellite pompa intenti a ≥20 Hz per la
  durata indicata, poi si ferma. Un LLM non tiene mai un acceleratore
  aperto; il deadman di robotd resta l'ultima linea di difesa.
- Gli intervalli degli argomenti sono limitati (clamp) lato satellite
  (velocità, angoli).
- `robot.shutdown`, `robot.enable` e tutto ciò che non è offerto
  esplicitamente semplicemente non esiste sul filo.
- `robot.get_frame` è dichiarato ma restituisce `unsupported` finché
  mediad non espone la camera (tiene il contratto pronto per la
  roadmap del mapping).

### 5. Il cuore tool del bridge è un unico server MCP

Il bridge di riferimento espone i tool dichiarati dal satellite come
server MCP (HTTP/SSE) e ogni consumatore passa di lì: gli agenti
MCP-nativi (Arkimede) lo chiamano dall'interno del proprio loop; per i
provider chat-completions semplici con tool calling, il bridge esegue
il loop e smaltisce i `tool_calls` attraverso lo stesso layer MCP; gli
LLM puri ottengono chat solo vocale. Un solo esecutore, una sola
allowlist, un solo audit trail.

### 6. I servizi AI sono url+key nel dialetto OpenAI

Il bridge non incorpora ML: LLM (`/chat/completions`), STT
(`/audio/transcriptions`), TTS (`/audio/speech`) sono tre endpoint
configurabili in dialetto OpenAI. I deployment locali rispettosi della
privacy usano server in dialetto OpenAI già esistenti; il preset
Arkimede è pura configurazione una volta che Arkimede esporrà le sue
rotte audio.

### 7. Le sessioni sono stateless sul filo

Ogni (ri)connessione inizia con `session.start`; il satellite si
riconnette con un backoff fisso e si riannuncia. Memoria di
conversazione, storico dei turni e politica multi-turno sono affari
del bridge/agente. L'autenticazione è un bearer token sull'upgrade
WebSocket, opzionale su una LAN fidata.

Il contratto di wire completo vive in `docs/agent-protocol.md`.

## Conseguenze

- Qualunque piattaforma agente raggiungibile via WebSocket + tre URL
  in dialetto OpenAI può guidare l'anatra; nessuna può guidarla fuori
  dalla allowlist.
- Il satellite resta leggero (nessun ML oltre la wake word) e il
  budget della board da 1 GB regge.
- Niente barge-in e niente operatività fuori LAN in v0 — entrambi
  hanno percorsi di evoluzione dichiarati (AEC → full duplex; binding
  WebRTC → remoto).
- Il bridge di riferimento si porta la complessità di orchestrazione
  (turni VAD, server MCP, profili provider), mantenendo semplici sia
  il satellite sia gli agenti.
