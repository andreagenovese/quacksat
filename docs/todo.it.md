# Da fare

Lavoro aperto su quacksat. Copia inglese: [todo.md](todo.md).

- [ ] 2026-10-02: **dire come è finito un lavoro di navigazione.**
      `robot.go_to` e `robot.map_explore` rispondono subito ("partita") e
      il viaggio continua in quack-navd; il turno finisce lì, quindi la
      papera non dice mai di essere arrivata (un test generale sul gemello
      MuJoCo, voce tramite Arkimede: "vai in cucina" → arrivata in 2 min
      14 s, in silenzio). quack-navd non ha un canale per spingere eventi a
      quacksat. Dopo un lavoro avviato da quacksat stesso, seguire
      l'`explore` di `robot.map_status` (`state`, `reason`, `goal`) come
      già si interroga `nav.take_question`, e dire la fine con una frase
      breve e fissa nella lingua dell'utente: arrivata ("Sono arrivata in
      cucina"), fallita col motivo in breve ("Non trovo una strada per la
      cucina"), fermata ("Mi sono fermata"). Prima le frasi fisse (veloci,
      senza passare dal modello); farle formulare all'agente è
      un'opzione per dopo.
- [ ] 2026-10-02: **dire quando la papera si muove da sola.** quack-navd
      segna il movimento partito da sé in `robot.map_status` —
      `explore.self_started`, `explore.state` `searching` /
      `relocalizing`, il motivo ("the duck may have been moved: finding
      where it is first"), `untrusted` — e uno STOP lo blocca
      (`stopped_by_user`). Dirne l'inizio ("Non sono sicura di dove sono,
      mi guardo intorno") e la fine ("Mi sono ritrovata"), così chi è
      vicino alla papera sa perché cammina.
- [ ] Entrambi i punti col backend `agent`: la voce è del bridge (il suo
      TTS), quindi quacksat ha bisogno di un modo per far dire una frase
      fuori da un turno — un messaggio del protocollo verso il bridge (per
      esempio `say` → `tts.start` … `tts.end`), documentato in
      `docs/agent-protocol.it.md`. Col backend `direct` quacksat parla col
      proprio TTS.
- [ ] 2026-10-01: **un socket di controllo per quack-control** (la pagina
      locale, github.com/andreagenovese/quack-control): `sat.status`
      (backend, parola di attivazione, LLM/STT/TTS in uso, il collegamento
      al bridge), `sat.config` (lettura, poi modifica), e più avanti la
      chat (`chat.sock`: testo in ingresso, risposta in streaming, i tool
      chiamati dall'agente), con un lock del turno condiviso col
      microfono. Il progetto è nel `docs/study/map-app.md` di quack-nav.
