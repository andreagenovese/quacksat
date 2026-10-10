# To do

Open work on quacksat. Italian copy: [todo.it.md](todo.it.md).

- [ ] Spoken outcomes, what is left (after 2026-10-03): the agent
      phrasing the outcome instead of a fixed phrase (the `say` could
      carry the event and let the bridge's model word it); Italian
      prepositions before a place name (always "in": "Sono arrivata in
      cucina" is right, "in divano" is not); `untrusted` alone (the duck
      may have been moved, nothing moves yet) is not said; the `wyoming`
      backend says nothing on its own (Home Assistant has an announce
      action for satellites — worth wiring); languages beyond Italian
      and English.
- [x] 2026-10-02 → done 2026-10-03: **say how a navigation job ended.**
      `quacksat_core::announce`: a thread with its own lane to
      quack-navd reads `robot.map_status` (every 2 s while a job is
      followed or the duck moves, every 10 s otherwise; at once after a
      tool call started a job), and a state machine follows every job a
      tool call of the satellite started (`robot.go_to` by place or
      point, `robot.map_explore`) to its end: arrived ("Sono arrivata in
      cucina" / "Sono arrivata"), failed with the reason in brief (no way,
      not mapped, too long, moved and lost, fallen, battery, a drop),
      stopped ("Mi sono fermata" — not when the agent itself sent the
      stop), an exploration mapped or on its budget ("Ho esplorato per 10
      minuti, la casa è mappata al 72 per cento"). Fixed phrases, Italian
      and English (`[announce] language`), queued until the backend is
      between turns, each said once. Live on the MuJoCo twin (casa_grande,
      `direct`, qwen3:8b): "vai in cucina" → 81 s later "Sono arrivata in
      cucina."; a STOP from the nav socket mid-way → "Mi sono fermata.";
      a point off the map is refused before any job, and the model says
      why.
- [x] 2026-10-02 → done 2026-10-03: **say when the duck moves on its
      own.** The same tracker: `explore.self_started` with `searching`
      ("Non sono sicura di dove sono: mi guardo intorno."), `relocalizing`
      ("Prima di partire mi guardo intorno per ritrovarmi."), the
      homecoming's own exploring ("Riprendo a esplorare la casa."), and
      the end: found ("Mi sono ritrovata."), not found, stopped by the
      user, or the exploration's outcome. Live: at quacksat's start and
      after `twin.sh restart-navd`, the homecoming's search was announced
      and so was its end.
- [x] Both items with the `agent` backend (2026-10-03): protocol v1.1
      `say` (`docs/agent-protocol.md`): the satellite sends
      `{"type": "say", "id", "text", "lang"}` between turns to a bridge
      that lists `say` in `session.ready`'s `features`; the bridge speaks
      it as an ordinary `tts.start` … `tts.end` clip (or answers `error`
      with the id). An older bridge never gets one. The reference bridge
      implements it and reports its `[stt] language`. Live on the twin
      (the bridge on fake STT/TTS, qwen3:8b): the duck carried off its
      spot, "vai al punto x 1.0, y 2.2" → `say` "Prima di partire mi
      guardo intorno per ritrovarmi.", then "Mi sono ritrovata.", then
      "Sono arrivata." — each one a clip from the bridge's TTS.
- [ ] 2026-10-01: **a control socket for quack-control** (the local page,
      github.com/andreagenovese/quack-control): `sat.status` (backend,
      wake word, the LLM/STT/TTS in use, the bridge link), `sat.config`
      (read, later edit), and later the chat (`chat.sock`: text in,
      streamed answer out, the tools the agent called), with a turn lock
      shared with the microphone. The design is in quack-nav's
      `docs/study/map-app.md`.
- [ ] 2026-10-10: **the Home Assistant device, what is left** (ADR 0007,
      open questions): where the Speech-to-Phrase add-on reads its own
      sentence files from and when it retrains (a new area or blueprint
      sentence may need an add-on restart), checked on a real add-on;
      and the manual steps' lack of a cliff guard, once the duck has
      legs.
