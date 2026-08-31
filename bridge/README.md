# bridge

Minimal reference bridge for the `agent` backend: a server that accepts the
quacksat WebSocket protocol (audio streaming + events + tool call/result)
and runs STT → LLM (tool calling) → TTS.

Not implemented yet — it comes after `quacksat-core` and
`backends/wyoming` (see ADR 0002). This reference stays intentionally
minimal ("bring your own agent"); the Arkimede integration lives in the
Arkimede repository.
