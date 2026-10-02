# To do

Open work on quacksat. Italian copy: [todo.it.md](todo.it.md).

- [ ] 2026-10-02: **say how a navigation job ended.** `robot.go_to` and
      `robot.map_explore` answer at once ("started") and the journey runs
      on in quack-navd; the turn ends there, so the duck never says it
      arrived (a general test on the MuJoCo twin, voice through Arkimede:
      "vai in cucina" → arrived in 2 min 14 s, silently). quack-navd has
      no channel to push events to quacksat. After a job quacksat itself
      started, follow `robot.map_status`'s `explore` (`state`, `reason`,
      `goal`) the way `nav.take_question` is already polled, and speak the
      end in a short fixed phrase in the user's language: arrived ("Sono
      arrivata in cucina"), failed with the reason in brief ("Non trovo
      una strada per la cucina"), stopped ("Mi sono fermata").
      Fixed phrases first (fast, no model round trip); the agent phrasing
      them is a later option.
- [ ] 2026-10-02: **say when the duck moves on its own.** quack-navd marks
      self-started motion in `robot.map_status` — `explore.self_started`,
      `explore.state` `searching` / `relocalizing`, the reason ("the duck
      may have been moved: finding where it is first"), `untrusted` — and
      a STOP holds it (`stopped_by_user`). Speak its start ("Non sono
      sicura di dove sono, mi guardo intorno") and its end ("Mi sono
      ritrovata"), so a person near the duck knows why it walks.
- [ ] Both items with the `agent` backend: speech is the bridge's (its
      TTS), so quacksat needs a way to have a sentence spoken outside a
      turn — a protocol message to the bridge (e.g. `say` → `tts.start`
      … `tts.end`), documented in `docs/agent-protocol.md`. With the
      `direct` backend quacksat speaks through its own TTS.
- [ ] 2026-10-01: **a control socket for quack-control** (the local page,
      github.com/andreagenovese/quack-control): `sat.status` (backend,
      wake word, the LLM/STT/TTS in use, the bridge link), `sat.config`
      (read, later edit), and later the chat (`chat.sock`: text in,
      streamed answer out, the tools the agent called), with a turn lock
      shared with the microphone. The design is in quack-nav's
      `docs/study/map-app.md`.
