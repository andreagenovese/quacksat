# Allenare una wake word personalizzata (es. "hey Daffy")

Il rilevatore di quacksat esegue qualunque modello openWakeWord: la frase
è solo un file `.onnx` da mettere nella directory dei modelli. I modelli
preallenati (hey jarvis, alexa, …) arrivano da
`scripts/fetch-wake-models.sh`; una frase personalizzata si allena una
volta sola con la pipeline ufficiale di openWakeWord — gratis su Google
Colab, senza GPU locale. (I siti a pagamento che offrono lo stesso
training impacchettano esattamente questo notebook.)

## Procedura (~1 ora, in gran parte non presidiata)

1. Apri il notebook ufficiale di training su Colab (serve un account
   Google):

   <https://colab.research.google.com/github/dscripka/openWakeWord/blob/main/notebooks/automatic_model_training.ipynb>

2. Runtime → Cambia tipo di runtime → **GPU** (la T4 gratuita basta).

3. Nella cella di configurazione imposta:
   - `target_word` / `target_phrase`: `"hey daffy"` (grafia inglese — le
     voci sintetiche sono inglesi, e corrisponde abbastanza bene anche
     alla pronuncia di un non madrelingua)
   - nome del modello: `hey_daffy`
   Al primo giro lascia i default per numero di campioni e passi di
   training.

4. Esegui tutte le celle. La pipeline sintetizza migliaia di pronunce
   della frase con molte voci TTS, le aumenta con rumore e riverbero,
   aggiunge dati negativi (parlato che *non* è la frase) e allena il
   classificatore. Gli stadi mel + embedding sono i modelli condivisi
   preallenati che quacksat ha già.

5. Scarica il file `hey_daffy.onnx` risultante e mettilo nella directory
   dei modelli (`/var/lib/quacksat/models` sul robot, `models/` in dev).

6. Punta la config al modello:

   ```toml
   [wake]
   mode = "openwakeword"
   model = "hey_daffy.onnx"
   threshold = 0.5
   ```

## Taratura

- I modelli solo-sintetici sono un po' meno robusti dei preallenati
  curati. Se non ti sente, abbassa `threshold` (0.4, poi 0.35); se scatta
  con la TV o le conversazioni, alzala (0.6+). Osserva gli score con
  `RUST_LOG=quacksat_core=debug` — ogni rilevamento logga il suo score.
- Se la tua pronuncia differisce dalle voci sintetiche (accento), di
  solito aiuta riallenare con grafie aggiuntive della frase (es.
  `"hey daffy"`, `"ei daffy"`) nella lista dei target.
- Se scatta anche una frase *simile* (in pratica: un primo modello
  "hey Daffy" partiva anche con "hey Jarvis"), riallena aggiungendo le
  frasi confondibili ai **negativi avversari** custom — quella
  separazione non si sistema con la sola soglia.
- Verifica rapida senza hardware, su macOS:

  ```sh
  say -v Samantha -o /tmp/daffy.aiff "hey daffy"
  afconvert -f WAVE -d LEI16@16000 -c 1 /tmp/daffy.aiff /tmp/daffy.wav
  ```

  poi passa `/tmp/daffy.wav` al rilevatore (lo schema in
  `quacksat-core/tests/wake_oww.rs` fa esattamente questo per hey jarvis).
