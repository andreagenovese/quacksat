#!/usr/bin/env python3
"""Reference bridge for the quacksat agent protocol.

Implements the server side of docs/agent-protocol.md: accepts the
satellite's WebSocket, segments turns (the satellite's local VAD closes
utterances), and orchestrates STT -> LLM (tool calling) -> TTS through
three OpenAI-dialect endpoints configured as url+key. `--fake` replaces
all three with canned local stand-ins so the protocol can be exercised
with no AI services at all.

Deliberately minimal and readable: one file, one dependency
(`websockets`); HTTP via the standard library. Bring your own agent.

Usage:
    python3 bridge.py --config config.toml
    python3 bridge.py --fake
"""

import argparse
import asyncio
import io
import json
import logging
import math
import re
import struct
import sys
import tomllib
import urllib.request
import uuid
import wave

import websockets

log = logging.getLogger("bridge")

DEFAULTS = {
    "server": {"bind": "0.0.0.0", "port": 8765, "token": ""},
    "llm": {
        "base_url": "http://localhost:11434/v1",
        "api_key": "",
        "model": "arkimede",
        "system_prompt": "You are a small robot duck called quacksat. Answer briefly, in the language the user speaks. Use your robot tools when asked to move, look, or act.",
        "tool_calling": True,
        "max_tool_rounds": 5,
    },
    "stt": {"base_url": "http://localhost:9000/v1", "api_key": "", "model": "", "language": ""},
    "tts": {"base_url": "http://localhost:9100/v1", "api_key": "", "model": "piper", "voice": ""},
    "behavior": {"follow_up": False, "history_max_messages": 20, "wake_window": 0.25},
    "mcp": {"enabled": False, "bind": "0.0.0.0", "port": 8766},
}

# The MCP server (mcp_server.py) forwards tool calls to connected
# satellites, keyed by the name each announces in session.start.
REGISTRY = {"sessions": {}}

class WakeArbiter:
    """Multi-duck wake arbitration: when several ducks hear the wake word
    within a short window, the highest score (the closest duck) wins and
    the others are told to stop listening (listen.stop)."""

    def __init__(self, window_s):
        self.window = window_s
        self.candidates = []
        self.task = None

    def wake(self, session, score):
        self.candidates.append((score if score is not None else 0.0, session))
        if self.task is None or self.task.done():
            self.task = asyncio.create_task(self._decide())

    async def _decide(self):
        await asyncio.sleep(self.window)
        candidates, self.candidates = self.candidates, []
        if len(candidates) <= 1:
            return
        winner = max(candidates, key=lambda c: c[0])[1]
        for score, session in candidates:
            if session is not winner:
                log.info("wake arbitration: %s (%.2f) loses to %s", session.name, score, winner.name)
                session.audio.clear()
                await session.send({"type": "listen.stop"})

ARBITER = WakeArbiter(0.25)


def load_config(path):
    config = {section: dict(values) for section, values in DEFAULTS.items()}
    if path:
        with open(path, "rb") as f:
            for section, values in tomllib.load(f).items():
                config.setdefault(section, {}).update(values)
    return config


# ── OpenAI-dialect HTTP clients (stdlib, run in a worker thread) ─────────────


def http_json(url, payload, api_key):
    request = urllib.request.Request(
        url,
        data=json.dumps(payload).encode(),
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    if api_key:
        request.add_header("Authorization", f"Bearer {api_key}")
    with urllib.request.urlopen(request, timeout=120) as response:
        return json.loads(response.read())


def http_multipart(url, fields, file_field, filename, file_bytes, api_key):
    boundary = uuid.uuid4().hex
    body = io.BytesIO()
    for name, value in fields.items():
        if not value:
            continue
        body.write(
            f"--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n".encode()
        )
    body.write(
        f"--{boundary}\r\nContent-Disposition: form-data; name=\"{file_field}\"; "
        f"filename=\"{filename}\"\r\nContent-Type: audio/wav\r\n\r\n".encode()
    )
    body.write(file_bytes)
    body.write(f"\r\n--{boundary}--\r\n".encode())
    request = urllib.request.Request(
        url,
        data=body.getvalue(),
        headers={"Content-Type": f"multipart/form-data; boundary={boundary}"},
        method="POST",
    )
    if api_key:
        request.add_header("Authorization", f"Bearer {api_key}")
    with urllib.request.urlopen(request, timeout=120) as response:
        return response.read()


def speakable(text):
    """Strip markdown, emoji, and symbols a TTS voice would read aloud.

    The agent's prompt should already ask for plain spoken text; this is
    the safety net for the asterisks and emoji that slip through anyway.
    """
    text = re.sub(r"<think>.*?</think>", " ", text, flags=re.DOTALL)  # reasoning models
    text = re.sub(r"```.*?```", " ", text, flags=re.DOTALL)  # code blocks
    text = re.sub(r"`([^`]*)`", r"\1", text)                 # inline code
    text = re.sub(r"\*\*|__|\*|_|~~|#+ ", "", text)          # md emphasis/headers
    text = re.sub(r"\[([^\]]*)\]\([^)]*\)", r"\1", text)     # links -> label
    text = re.sub(r"^\s*[-*•]\s+", " ", text, flags=re.MULTILINE)  # bullets
    text = re.sub(                                           # emoji & symbols
        "["
        "\U0001f000-\U0001ffff"
        "←-⇿"   # arrows
        "⌀-➿"   # misc technical, dingbats
        "⬀-⯿"
        "️"
        "]+",
        " ",
        text,
    )
    return re.sub(r"\s+", " ", text).strip()


def pcm_to_wav(pcm, rate, channels):
    out = io.BytesIO()
    with wave.open(out, "wb") as w:
        w.setnchannels(channels)
        w.setsampwidth(2)
        w.setframerate(rate)
        w.writeframes(pcm)
    return out.getvalue()


# ── The three services, real and fake ────────────────────────────────────────


class Services:
    def __init__(self, config, fake):
        self.config = config
        self.fake = fake

    async def transcribe(self, pcm, rate):
        if self.fake:
            return "fake utterance"
        stt = self.config["stt"]
        fields = {"model": stt["model"], "language": stt["language"], "response_format": "json"}
        data = await asyncio.to_thread(
            http_multipart,
            f"{stt['base_url'].rstrip('/')}/audio/transcriptions",
            fields,
            "file",
            "utterance.wav",
            pcm_to_wav(pcm, rate, 1),
            stt["api_key"],
        )
        return json.loads(data).get("text", "").strip()

    async def complete(self, messages, tools):
        if self.fake:
            # First round: exercise the tool path; then a canned reply.
            if not any(m.get("role") == "tool" for m in messages):
                return None, [{"id": "fake-1", "function": {"name": "robot.sound", "arguments": '{"tag": "chirp"}'}}]
            return "Ciao! Sono il bridge di prova: ti ho sentito.", None
        llm = self.config["llm"]
        payload = {"model": llm["model"], "messages": messages}
        if tools and llm["tool_calling"]:
            payload["tools"] = tools
        data = await asyncio.to_thread(
            http_json,
            f"{llm['base_url'].rstrip('/')}/chat/completions",
            payload,
            llm["api_key"],
        )
        message = data["choices"][0]["message"]
        return message.get("content"), message.get("tool_calls")

    async def synthesize(self, text):
        """Return (pcm_bytes, rate, channels)."""
        if self.fake:
            rate = 22050
            pcm = b"".join(
                struct.pack("<h", int(8000 * math.sin(2 * math.pi * 440 * i / rate)))
                for i in range(rate // 2)
            )
            return pcm, rate, 1
        tts = self.config["tts"]
        payload = {"input": text, "response_format": "wav"}
        if tts["model"]:
            payload["model"] = tts["model"]
        if tts["voice"]:
            payload["voice"] = tts["voice"]
        data = await asyncio.to_thread(
            http_json_bytes,
            f"{tts['base_url'].rstrip('/')}/audio/speech",
            payload,
            tts["api_key"],
        )
        with wave.open(io.BytesIO(data), "rb") as w:
            return w.readframes(w.getnframes()), w.getframerate(), w.getnchannels()


def http_json_bytes(url, payload, api_key):
    request = urllib.request.Request(
        url,
        data=json.dumps(payload).encode(),
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    if api_key:
        request.add_header("Authorization", f"Bearer {api_key}")
    with urllib.request.urlopen(request, timeout=120) as response:
        return response.read()


# ── One satellite session ────────────────────────────────────────────────────


class Session:
    def __init__(self, ws, config, services):
        self.ws = ws
        self.config = config
        self.services = services
        self.name = "quacksat"
        self.audio = bytearray()
        self.audio_rate = 16000
        self.tools = []
        self.history = []
        self.pending_tools = {}

    def openai_tools(self):
        # Dots become underscores (many providers reject dots in function
        # names); run_turn maps them back before calling the satellite.
        return [
            {"type": "function", "function": {
                "name": t["name"].replace(".", "_"),
                "description": t.get("description", ""),
                "parameters": t.get("parameters", {"type": "object", "properties": {}}),
            }}
            for t in self.tools
        ]

    async def send(self, event):
        await self.ws.send(json.dumps(event))

    async def call_tool(self, name, args):
        call_id = uuid.uuid4().hex[:8]
        future = asyncio.get_running_loop().create_future()
        self.pending_tools[call_id] = future
        await self.send({"type": "tool.call", "id": call_id, "name": name, "args": args})
        try:
            return await asyncio.wait_for(future, timeout=30)
        except asyncio.TimeoutError:
            return {"ok": False, "error": "tool timed out"}
        finally:
            self.pending_tools.pop(call_id, None)

    async def run_turn_safe(self, pcm):
        # A failing STT/LLM must reach the satellite as a protocol error,
        # not vanish inside a fire-and-forget task: the duck is holding
        # its thinking pose and deserves to be told to stop.
        try:
            await self.run_turn(pcm)
        except Exception as e:  # noqa: BLE001
            log.exception("turn failed")
            try:
                await self.send({"type": "error", "message": f"turn failed: {e}"})
            except Exception:  # noqa: BLE001 — the socket may be gone too
                pass

    async def run_turn(self, pcm):
        text = await self.services.transcribe(bytes(pcm), self.audio_rate)
        if not text:
            log.info("turn: nothing recognized")
            # Tell the satellite: it is waiting for an answer and would
            # otherwise sit in its thinking pose until the reply timeout.
            await self.send({"type": "error", "message": "nothing recognized"})
            return
        log.info("user: %s", text)
        self.history.append({"role": "user", "content": text})

        llm = self.config["llm"]
        reply = None
        for _ in range(llm["max_tool_rounds"]):
            messages = [{"role": "system", "content": llm["system_prompt"]}] + self.history
            content, tool_calls = await self.services.complete(messages, self.openai_tools())
            if not tool_calls:
                reply = content
                break
            self.history.append(
                {"role": "assistant", "content": content, "tool_calls": tool_calls}
            )
            for call in tool_calls:
                name = call["function"]["name"]
                if "." not in name:
                    name = name.replace("_", ".", 1)
                try:
                    args = json.loads(call["function"].get("arguments") or "{}")
                except json.JSONDecodeError:
                    args = {}
                log.info("tool call: %s %s", name, args)
                result = await self.call_tool(name, args)
                log.info("tool result: %s", result)
                self.history.append({
                    "role": "tool",
                    "tool_call_id": call.get("id", ""),
                    "content": json.dumps(result),
                })
        else:
            reply = "I could not finish that action."

        if reply:
            log.info("assistant: %s", reply)
            self.history.append({"role": "assistant", "content": reply})
            await self.speak(reply)
        else:
            # The satellite is holding its thinking pose: an empty reply
            # must end the wait, not leave it to the reply timeout.
            await self.send({"type": "error", "message": "the agent had nothing to say"})
        limit = self.config["behavior"]["history_max_messages"]
        self.history = self.history[-limit:]
        if self.config["behavior"]["follow_up"]:
            await self.send({"type": "listen.start"})

    async def speak(self, text):
        text = speakable(text)
        if not text:
            # Nothing sayable survived the sanitizer: same as no reply.
            await self.send({"type": "error", "message": "the agent had nothing to say"})
            return
        try:
            pcm, rate, channels = await self.services.synthesize(text)
        except Exception as e:  # noqa: BLE001 — a dead TTS must not kill the session
            log.warning("tts failed: %s", e)
            await self.send({"type": "error", "message": f"tts failed: {e}"})
            return
        await self.send({"type": "tts.start", "rate": rate, "channels": channels, "format": "s16le"})
        for i in range(0, len(pcm), 8192):
            await self.ws.send(pcm[i:i + 8192])
        await self.send({"type": "tts.end"})

    async def handle(self):
        async for message in self.ws:
            if isinstance(message, (bytes, bytearray)):
                self.audio.extend(message)
                continue
            event = json.loads(message)
            kind = event.get("type", "")
            if kind == "session.start":
                self.tools = event.get("tools", [])
                self.audio_rate = event.get("audio", {}).get("rate", 16000)
                self.name = event.get("satellite", {}).get("name") or "quacksat"
                previous = REGISTRY["sessions"].get(self.name)
                if previous is not None and previous is not self:
                    log.warning("duck %s reconnected — replacing the old session", self.name)
                REGISTRY["sessions"][self.name] = self
                log.info(
                    "session: %s v%s, %d tools",
                    self.name,
                    event.get("satellite", {}).get("version", "?"),
                    len(self.tools),
                )
                await self.send({"type": "session.ready", "version": 1, "agent": {"name": "bridge"}})
            elif kind == "wake":
                log.info("wake %s (%s, score %s)", self.name, event.get("model", "?"), event.get("score"))
                self.audio.clear()
                ARBITER.wake(self, event.get("score"))
            elif kind == "utterance.end":
                pcm = bytes(self.audio)
                self.audio.clear()
                log.info("utterance: %.1fs of audio", len(pcm) / 2 / self.audio_rate)
                self.turn_task = asyncio.create_task(self.run_turn_safe(pcm))
            elif kind == "tool.result":
                future = self.pending_tools.get(event.get("id", ""))
                if future and not future.done():
                    future.set_result({
                        "ok": event.get("ok", False),
                        "data": event.get("data"),
                        "error": event.get("error"),
                    })
            elif kind == "ping":
                pong = dict(event)
                pong["type"] = "pong"
                await self.send(pong)
            elif kind == "pong":
                pass
            else:
                log.debug("ignored event: %s", kind)


async def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", help="TOML config file (see config.example.toml)")
    parser.add_argument("--fake", action="store_true", help="canned STT/LLM/TTS, no AI services")
    parser.add_argument("-v", "--verbose", action="store_true")
    args = parser.parse_args()
    logging.basicConfig(
        level=logging.DEBUG if args.verbose else logging.INFO,
        format="%(asctime)s %(levelname)s %(name)s: %(message)s",
    )

    config = load_config(args.config)
    services = Services(config, args.fake)
    ARBITER.window = config["behavior"]["wake_window"]
    token = config["server"]["token"]

    async def handler(ws):
        if token:
            got = ws.request.headers.get("Authorization", "")
            if got != f"Bearer {token}":
                await ws.close(code=4401, reason="unauthorized")
                log.warning("rejected connection: bad token")
                return
        log.info("satellite connected: %s", ws.remote_address)
        session = Session(ws, config, services)
        try:
            await session.handle()
        except websockets.ConnectionClosed:
            pass
        finally:
            if REGISTRY["sessions"].get(session.name) is session:
                del REGISTRY["sessions"][session.name]
        log.info("satellite disconnected: %s", session.name)

    mcp_task = None
    if config["mcp"]["enabled"]:
        try:
            import mcp_server

            async def run_mcp():
                try:
                    await mcp_server.serve(REGISTRY, config["mcp"]["bind"], config["mcp"]["port"])
                except Exception:
                    log.exception("mcp server failed")

            # Keep the reference: an unreferenced task can be collected.
            mcp_task = asyncio.get_running_loop().create_task(run_mcp())
        except ImportError as e:
            log.warning("mcp server disabled (pip install mcp uvicorn): %s", e)
    _ = mcp_task

    bind, port = config["server"]["bind"], config["server"]["port"]
    mode = "fake services" if args.fake else "configured services"
    async with websockets.serve(handler, bind, port):
        log.info("bridge listening on ws://%s:%s (%s)", bind, port, mode)
        await asyncio.Future()


if __name__ == "__main__":
    try:
        asyncio.run(main())
    except KeyboardInterrupt:
        sys.exit(0)
