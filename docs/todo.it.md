# Da fare

Lavoro aperto su quacksat. Copia inglese: [todo.md](todo.md).

- [ ] Esiti a voce, ciò che resta (dopo il 2026-10-03): far formulare
      l'esito all'agente invece di una frase fissa (il `say` potrebbe
      portare l'evento e lasciare le parole al modello del bridge); le
      preposizioni italiane davanti al nome di un posto (sempre "in":
      "Sono arrivata in cucina" va bene, "in divano" no); `untrusted` da
      solo (la papera forse è stata spostata, ma niente si muove ancora)
      non viene detto; il backend `wyoming` non dice niente da sé (Home
      Assistant ha un'azione announce per i satelliti — vale la pena
      collegarla); lingue oltre italiano e inglese.
- [x] 2026-10-02 → fatto il 2026-10-03: **dire come è finito un lavoro
      di navigazione.** `quacksat_core::announce`: un thread con una sua
      corsia verso quack-navd legge `robot.map_status` (ogni 2 s mentre
      segue un lavoro o la papera si muove, ogni 10 s altrimenti; subito
      dopo una chiamata che ha avviato un lavoro), e una macchina a stati
      segue fino alla fine ogni lavoro avviato da un tool del satellite
      (`robot.go_to` per posto o per punto, `robot.map_explore`): arrivata
      ("Sono arrivata in cucina" / "Sono arrivata"), fallita col motivo
      in breve (nessuna strada, non mappato, troppo lungo, spostata e
      persa, caduta, batteria, un dislivello), fermata ("Mi sono fermata"
      — non quando lo stop l'ha mandato l'agente stesso), un'esplorazione
      finita o a budget esaurito ("Ho esplorato per 10 minuti, la casa è
      mappata al 72 per cento"). Frasi fisse, italiano e inglese
      (`[announce] language`), in coda finché il backend non è tra un
      turno e l'altro, ognuna detta una volta. Dal vivo sul gemello
      MuJoCo (casa_grande, `direct`, qwen3:8b): "vai in cucina" → 81 s
      dopo "Sono arrivata in cucina."; uno STOP dal socket di navigazione
      a metà strada → "Mi sono fermata."; un punto fuori mappa è
      rifiutato prima di ogni lavoro, e il modello dice perché.
- [x] 2026-10-02 → fatto il 2026-10-03: **dire quando la papera si
      muove da sola.** Lo stesso tracker: `explore.self_started` con
      `searching` ("Non sono sicura di dove sono: mi guardo intorno."),
      `relocalizing` ("Prima di partire mi guardo intorno per
      ritrovarmi."), l'esplorazione del ritorno a casa ("Riprendo a
      esplorare la casa."), e la fine: ritrovata ("Mi sono ritrovata."),
      non ritrovata, fermata dall'utente, o l'esito dell'esplorazione. Dal
      vivo: all'avvio di quacksat e dopo `twin.sh restart-navd` la
      ricerca del ritorno a casa è stata annunciata, e così la sua fine.
- [x] Entrambi i punti col backend `agent` (2026-10-03): protocollo v1.1
      `say` (`docs/agent-protocol.it.md`): il satellite manda
      `{"type": "say", "id", "text", "lang"}` tra un turno e l'altro a un
      bridge che elenca `say` nelle `features` di `session.ready`; il
      bridge lo pronuncia come una normale clip `tts.start` … `tts.end`
      (oppure risponde `error` con l'id). Un bridge più vecchio non ne
      riceve mai. Il bridge di riferimento lo implementa e riporta il suo
      `[stt] language`. Dal vivo sul gemello (il bridge su STT/TTS
      finti, qwen3:8b): la papera portata via dal suo posto, "vai al
      punto x 1.0, y 2.2" → `say` "Prima di partire mi guardo intorno per
      ritrovarmi.", poi "Mi sono ritrovata.", poi "Sono arrivata." —
      ognuna una clip dal TTS del bridge.
- [ ] 2026-10-01: **un socket di controllo per quack-control** (la pagina
      locale, github.com/andreagenovese/quack-control): `sat.status`
      (backend, parola di attivazione, LLM/STT/TTS in uso, il collegamento
      al bridge), `sat.config` (lettura, poi modifica), e più avanti la
      chat (`chat.sock`: testo in ingresso, risposta in streaming, i tool
      chiamati dall'agente), con un lock del turno condiviso col
      microfono. Il progetto è nel `docs/study/map-app.md` di quack-nav.
