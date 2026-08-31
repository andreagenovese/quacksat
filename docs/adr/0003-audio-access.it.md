# ADR 0003: Accesso audio sull'anatra (mic e speaker)

- Stato: accettato
- Data: 2026-08-31
- Input: `docs/study/microduck-speaker-path.md`,
  `docs/study/microduck-mic-path.md`,
  `docs/study/microduck-client-pattern.md`,
  `docs/study/microduck-ipc-and-packaging.md`

## Contesto

quacksat ha bisogno di cattura microfono continua (wake word + streaming
STT) e di uscita speaker (TTS) su un hardware il cui unico codec audio
(TLV320AIC3104, scheda `aic3104`) è usato da robotd con una premessa
esplicita di esclusività:

- **Riproduzione**: `robotd/src/sound.rs` lancia un figlio `aplay` per
  suono su `plughw:aic3104` ("un solo figlio in riproduzione, un suono
  nuovo lo uccide"). Il device è libero tra un suono e l'altro;
  `robot.theremin`/`robot.chorale` possono tenerlo indefinitamente. Se
  `aplay` non riesce ad aprire il device robotd logga una riga a livello
  debug e va avanti — il fallimento è soft in entrambe le direzioni.
- **Cattura**: `pet-detect` (un thread dentro robotd) tiene aperto in
  continuo `arecord -D plughw:aic3104,0` — ma **solo con
  `audio.pet_detect = true`, che di default è off**. Su un robot di
  fabbrica il PCM di cattura è inutilizzato.
- Nel deploy di microduck **non esiste alcun dsnoop/dmix/asound.conf**:
  entrambe le direzioni sono davvero single-client.
- L'RPC `robot.sound` non può trasportare TTS: accetta un enum chiuso di
  7 tag che seleziona un wav casuale da una directory bank di root. Non
  esiste alcuna API file/stream/volume.
- Il mic singolo è cablato su **Mic3R → solo PGA destro**; una cattura
  mono via `plug` media un canale sinistro morto dentro il segnale
  (~metà ampiezza). La frequenza nativa è 48 kHz.
- **Non esiste echo cancellation da nessuna parte** (e nessuna AEC
  hardware, a differenza di Voice PE): il mic sente ogni suono che
  l'anatra riproduce.

## Decisione

### 1. Accesso diretto ed esclusivo al device, via figli CLI ALSA

quacksat apre il codec direttamente, lanciando sottoprocessi
`arecord`/`aplay` esattamente come fanno robotd e pet-detect — nessun
binding ALSA in-process, nessuna dipendenza C, stesse caratteristiche di
fallimento del resto dello stack.

- **Cattura**: `arecord -D plughw:aic3104,0 -f S16_LE -c 2 -r 48000 -t raw`,
  tenuto aperto in continuo. quacksat prende il **canale destro** e
  ricampiona in-process a ciò che servono wake-word engine e backend
  (tipicamente 16 kHz mono). Non copiamo il `-c 1 -r 16000` di
  pet-detect, che dimezza il segnale del mic e butta qualità utile
  allo STT.
- **Riproduzione**: il TTS va su `plughw:aic3104` attraverso un singolo
  figlio `aplay`, serializzato lato quacksat (un solo figlio, una nuova
  frase uccide la vecchia — stessa policy di sound.rs). Su `-EBUSY`
  (robotd sta starnazzando), retry con breve backoff; se lo stream di
  stato mostra un theremin/chorale attivo, aspettare che finisca invece
  di girare a vuoto.

### 2. Convivenza con pet-detect per policy, non per idraulica

quacksat richiede `audio.pet_detect = false` — **il default di fabbrica**.
La cosa è documentata, e quacksat rileva il conflitto all'avvio (apertura
cattura fallita / config robotd) e lo dice in una riga di log chiara,
invece di entrare in un duello silenzioso di retry. Rilevamento coccole e
satellite vocale sono mutuamente esclusivi su questo hardware, oggi.

### 3. Versi dell'anatra via robotd, voce via quacksat

I suoni espressivi non vocali (chirp di conferma sulla wake word, inquire,
alarm) si richiedono con l'RPC `robot.sound` e restano compito di robotd —
zero contesa, serializzati da robotd, personalità dell'anatra coerente. Il
TTS è esclusivamente l'aplay di quacksat. Mentre parla, quacksat può
animare il becco con notifiche `robot.mouth`.

### 4. Audio half-duplex come risposta all'eco (v0)

Senza AEC, quacksat è **half-duplex**: mentre il figlio TTS riproduce (e,
best effort, mentre suona un suono robotd da lui richiesto), l'elaborazione
di wake word e STT è sospesa; la cattura resta aperta ma i frame vengono
scartati. Nessun barge-in nella v0. Config robot consigliata per il ruolo
satellite: `audio.greet = false` (evita che lo starnazzo di boot faccia a
gara con la prima frase).

### 5. Privilegi e ordinamento della unit

quacksat gira come utente proprio con `SupplementaryGroups=robot audio`
(`/dev/snd/*` è root:audio 0660; nulla in microduck gestisce il gruppo
audio — usiamo quello della distro). La unit ordina
`After=aic3104-init.service robotd.service` con `Wants=robotd.service`
(After, non Requires/BindsTo: un robotd giù non deve impedire al satellite
di partire, riprovare, o fare da semplice speaker).

## Alternative considerate

- **Idraulica ALSA condivisa (dsnoop + dmix via un `/etc/asound.conf`
  additivo)**: concorrenza vera, ma richiede di ripuntare anche
  pet-detect (il device di cattura è *derivato* dal parametro di
  riproduzione — serve una PR upstream `audio.capture_device`, ~10
  righe), impone parametri slave fissi, aggiunge latenza al percorso
  synth di robotd tarato stretto, e rende poco finché pet_detect è off di
  default. **Rimandata**: è il percorso di evoluzione documentato se mai
  servisse la convivenza.
- **Instradare il TTS attraverso robotd**: impossibile oggi (enum chiuso,
  nessuna API stream). Un RPC upstream "riproduci PCM arbitrario" è una
  conversazione di design più grossa (fuori scope, di proposito, per un
  daemon di sicurezza).
- **ALSA in-process (crate `alsa`)**: latenza più bassa e controllo più
  fine, ma è un binding C — costa la cross-build e diverge dallo stile
  della casa. Rivalutabile se la latenza dei sottoprocessi si rivelasse
  inadeguata per il barge-in.
- **AEC software (es. speexdsp/webrtc-audio-processing)**: l'unica strada
  verso il barge-in full-duplex; tutti i candidati sono dipendenze C e
  non provati su 1 GB di RAM accanto all'inferenza. Rimandata a un ADR
  dedicato quando l'half-duplex si dimostrerà troppo limitante.

## Conseguenze

- Per la v0 non serve alcuna modifica a microduck. Le collisioni con i
  suoni di robotd falliscono soft in entrambe le direzioni e sono
  limitate (~centinaia di ms), tranne theremin/chorale, a cui quacksat
  deve accorgersi di cedere il passo.
- Un robot con il rilevamento coccole attivo non può usare il mic del
  satellite; il compromesso è esplicito e loggato.
- Half-duplex significa che l'anatra non può essere interrotta a metà
  frase; il barge-in è sacrificato consapevolmente finché non esiste
  un'AEC.
- Lo stato del mixer è impostato al boot da `aic3104-init.service`; se il
  TTS avesse bisogno di PGA/volume diversi, quacksat dovrebbe impostarli
  da sé (`amixer -c aic3104`) accettando di cambiare anche il volume di
  robotd — evitato nella v0.
- Lo speaker taglia sotto i ~300 Hz: scegliere/filtrare la voce TTS di
  conseguenza.
