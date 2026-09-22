use serde::Deserialize;

/// Which voice backend quacksat runs with. Selected in the config file,
/// never at compile time, so the same binary ships for both setups.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    /// Bring-up mode: run the audio pipeline (capture, VAD, wake) and the
    /// robotd client, log events, chirp on wake. No conversation.
    None,
    /// Home Assistant Assist satellite over the Wyoming protocol.
    Wyoming,
    /// WebSocket bridge to an STT → LLM (tool calling) → TTS agent.
    Agent,
    /// Self-contained: the satellite itself speaks the OpenAI dialect —
    /// STT → LLM (tool calling) → TTS over HTTP, no bridge.
    Direct,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub backend: Backend,
    #[serde(default = "default_robotd_socket")]
    pub robotd_socket: String,
    #[serde(default)]
    pub audio: AudioConfig,
    #[serde(default)]
    pub wake: WakeConfig,
    #[serde(default)]
    pub wyoming: WyomingConfig,
    #[serde(default)]
    pub agent: AgentConfig,
    #[serde(default)]
    pub direct: DirectConfig,
    #[serde(default)]
    pub thinking: ThinkingConfig,
    /// Where the navigation daemon listens. The map, the places and
    /// the journeys moved to `quack-navd` (2026-09-22); `[map]` and
    /// `[homecoming]` are that daemon's config now.
    #[serde(default)]
    pub nav: NavConfig,
    #[serde(default)]
    pub gait: GaitConfig,
}



/// Where the navigation daemon listens (`quack-navd`). Enabled by
/// default: if nothing answers there the satellite says so once and
/// carries on as a voice assistant.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NavConfig {
    pub enabled: bool,
    pub socket: String,
}

impl Default for NavConfig {
    fn default() -> Self {
        Self { enabled: true, socket: "/run/quack-nav.sock".into() }
    }
}

/// The gait's limits (`[gait]`): the navigation has its own copy of the
/// same numbers in its own config (ADR 0006).
pub use crate::gait::GaitConfig;

/// The thinking cue: body language while the duck waits for its answer.
/// Timeline: utterance closed → nothing; after `delay_s` a slow head sway
/// (robot.head) until the reply arrives, then the head recenters; on
/// timeout or error a low tock replaces the silence.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ThinkingConfig {
    pub enabled: bool,
    /// Seconds of waiting before the pose starts — replies faster than
    /// this deserve no theatrics.
    pub delay_s: f32,
    /// Give up waiting for the agent after this long: sad tock, back to
    /// idle. Applies where the protocol has no timeout of its own.
    pub timeout_s: f32,
}

impl Default for ThinkingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            delay_s: 1.0,
            timeout_s: 30.0,
        }
    }
}

/// Settings for the `direct` backend: the satellite itself speaks the
/// OpenAI dialect — no bridge, no home server. All three services are
/// url+key endpoints, exactly like the reference bridge's.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DirectConfig {
    pub llm: LlmService,
    pub stt: SttService,
    pub tts: TtsService,
    /// Reopen the mic after each reply (multi-turn without wake word).
    pub follow_up: bool,
    /// The duck's own MCP server: the robot tool catalog served over
    /// Streamable HTTP so MCP-capable agents can drive the body directly.
    pub mcp: DirectMcpConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DirectMcpConfig {
    pub enabled: bool,
    pub bind: String,
    pub port: u16,
    /// Mandatory when enabled: an HTTP server accepting motion commands
    /// on the robot does not run open.
    pub token: String,
}

impl Default for DirectMcpConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bind: "0.0.0.0".to_string(),
            port: 8767,
            token: String::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LlmService {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub system_prompt: String,
    pub tool_calling: bool,
    pub max_tool_rounds: u32,
    pub history_max_messages: usize,
}

impl Default for LlmService {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:11434/v1".to_string(),
            api_key: String::new(),
            model: "qwen3:8b".to_string(),
            system_prompt: "You are quacksat, a small robot duck. Reply in one or \
                            two spoken sentences, no formatting. Use your robot tools \
                            when asked to move, look, quack, or act. If you have the \
                            navigation tools: when you have just asked where you are and \
                            the user names the place, remember it with \
                            robot.remember_place; when the user tells you to go somewhere \
                            by name — the kitchen, the bedroom, back to the desk — call \
                            robot.go_to with that name as `place`; robot.map_explore is \
                            for mapping a house, never for going to a room you know."
                .to_string(),
            tool_calling: true,
            max_tool_rounds: 5,
            history_max_messages: 20,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SttService {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub language: String,
}

impl Default for SttService {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:9000/v1".to_string(),
            api_key: String::new(),
            model: String::new(),
            language: String::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TtsService {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub voice: String,
}

impl Default for TtsService {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:9100/v1".to_string(),
            api_key: String::new(),
            model: "piper".to_string(),
            voice: String::new(),
        }
    }
}

/// Settings for the `agent` backend (WebSocket bridge, ADR 0004).
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AgentConfig {
    /// Bridge WebSocket URL (`ws://` or `wss://`).
    pub url: String,
    /// Optional bearer token sent on the WebSocket upgrade.
    pub token: Option<String>,
    /// Satellite name announced in `session.start` — give each duck its
    /// own (kitchen, studio...) so the bridge can tell them apart.
    pub name: String,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            url: "ws://127.0.0.1:8765".to_string(),
            token: None,
            name: "quacksat".to_string(),
        }
    }
}

/// Settings for the `wyoming` backend (Home Assistant Assist satellite).
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WyomingConfig {
    /// Where the satellite listens for Home Assistant; HA's Wyoming
    /// integration is pointed at this host:port.
    pub bind: String,
    /// Satellite name shown in Home Assistant.
    pub name: String,
    /// Optional Home Assistant area hint.
    pub area: Option<String>,
}

impl Default for WyomingConfig {
    fn default() -> Self {
        Self {
            bind: "0.0.0.0:10700".to_string(),
            name: "quacksat".to_string(),
            area: None,
        }
    }
}

/// ALSA device names (ADR 0003). Capture is 2ch/48kHz on the aic3104 codec;
/// the single mic sits on the right channel only.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AudioConfig {
    pub playback_device: String,
    pub capture_device: String,
    /// Development hook: replace `arecord` with any command that writes raw
    /// S16_LE 2ch 48kHz audio to stdout (e.g. sox on macOS, a file feeder in
    /// CI). Unset on the robot, where arecord + capture_device is the path.
    pub capture_command: Option<Vec<String>>,
    /// Development hook: the program spawned for playback instead of
    /// `aplay`, invoked with aplay-style arguments (see
    /// scripts/aplay-shim-macos.sh for a sox-based shim).
    pub playback_program: Option<String>,
    /// Wake acknowledgement wav played locally when robotd cannot chirp
    /// (refused or unreachable). Unset = a built-in synthesized quack.
    pub wake_sound: Option<String>,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            playback_device: "plughw:aic3104".to_string(),
            capture_device: "plughw:aic3104,0".to_string(),
            capture_command: None,
            playback_program: None,
            wake_sound: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WakeConfig {
    pub mode: WakeMode,
    /// Directory holding the openWakeWord feature models
    /// (melspectrogram.onnx, embedding_model.onnx) and wake models.
    /// Populate with scripts/fetch-wake-models.sh.
    pub models_dir: String,
    /// Wake model file name inside `models_dir`.
    pub model: String,
    /// Detection threshold on the model's 0..1 score.
    pub threshold: f32,
}

impl Default for WakeConfig {
    fn default() -> Self {
        Self {
            mode: WakeMode::Energy,
            models_dir: "/var/lib/quacksat/models".to_string(),
            model: "hey_daffy.onnx".to_string(),
            threshold: 0.5,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WakeMode {
    /// openWakeWord models via the tract ONNX runtime (pure Rust).
    Openwakeword,
    /// Bring-up detector: any speech onset after a stretch of silence
    /// counts as a wake. Fires on every utterance — not for production.
    Energy,
    /// Never wake (backend-driven or push-to-talk setups).
    Disabled,
}

fn default_robotd_socket() -> String {
    "/run/robotd.sock".to_string()
}

impl Config {
    pub const DEFAULT_PATH: &'static str = "/etc/robot/quacksat.toml";

    pub fn load(path: &str) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&text)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shipped example is the first thing a new user feeds the
    /// binary, and `deny_unknown_fields` makes a stale section fatal:
    /// after the navigation left, `[map]` sat there for a day and the
    /// example would not start (2026-09-22).
    #[test]
    fn the_shipped_example_parses() {
        let text = include_str!("../../quacksat.example.toml");
        let config: Config = toml::from_str(text).expect("quacksat.example.toml must parse");
        assert_eq!(config.backend, Backend::None);
        assert_eq!(config.nav.socket, "/run/quack-nav.sock");
    }

    /// The default prompt is a multi-line Rust string: a line that
    /// forgets its trailing backslash ships the source indentation to
    /// the model (public main sent it runs of 29 spaces).
    #[test]
    fn the_default_prompt_has_no_source_indentation() {
        let prompt = LlmService::default().system_prompt;
        assert!(!prompt.contains("  "), "double space in: {prompt}");
    }

    #[test]
    fn parses_minimal_config() {
        let config: Config = toml::from_str("backend = \"wyoming\"").unwrap();
        assert_eq!(config.backend, Backend::Wyoming);
        assert_eq!(config.robotd_socket, "/run/robotd.sock");
        assert_eq!(config.audio.capture_device, "plughw:aic3104,0");
        assert_eq!(config.wake.mode, WakeMode::Energy);
        assert!(!config.direct.mcp.enabled);
    }

    #[test]
    fn parses_full_config() {
        let config: Config = toml::from_str(
            "backend = \"none\"\n\
             robotd_socket = \"/tmp/robotd.sock\"\n\
             [audio]\n\
             playback_device = \"default\"\n\
             capture_device = \"default\"\n\
             [wake]\n\
             mode = \"disabled\"\n",
        )
        .unwrap();
        assert_eq!(config.backend, Backend::None);
        assert_eq!(config.audio.playback_device, "default");
        assert_eq!(config.audio.capture_command, None);
        assert_eq!(config.wake.mode, WakeMode::Disabled);
    }

    #[test]
    fn parses_capture_command_hook() {
        let config: Config = toml::from_str(
            "backend = \"none\"\n\
             [audio]\n\
             capture_command = [\"sox\", \"-q\", \"-d\"]\n",
        )
        .unwrap();
        assert_eq!(
            config.audio.capture_command.as_deref(),
            Some(["sox", "-q", "-d"].map(String::from).as_slice())
        );
    }

    #[test]
    fn rejects_unknown_keys() {
        assert!(toml::from_str::<Config>("backend = \"agent\"\ntypo = 1").is_err());
        assert!(toml::from_str::<Config>("backend = \"agent\"\n[audio]\ntypo = 1").is_err());
    }

    /// The correction is trim plus gain, clamped at the configured top —
    /// a velstand calibration (gain 1.63, yaw_max 1.7) must be allowed to
    /// send what alpha's 0.9 clamp would have cut.
    #[test]
    fn the_yaw_clamp_is_the_configured_one() {
        let alpha = GaitConfig { yaw_trim: 0.08, yaw_gain_left: 1.34, yaw_gain_right: 1.58, ..Default::default() };
        assert!((alpha.yaw(0.3, 0.9) - 0.9).abs() < 1e-9, "alpha stays under 0.9");
        let velstand = GaitConfig { yaw_trim: 0.16, yaw_gain_left: 1.63, yaw_gain_right: 1.58, yaw_max: 1.7 };
        assert!((velstand.yaw(0.3, 0.9) - (0.9 * 1.63 + 0.16)).abs() < 1e-9, "1.63 sent, under 1.7");
        assert!((velstand.yaw(0.3, -0.9) - (-0.9 * 1.58 + 0.16)).abs() < 1e-9);
        assert!((velstand.yaw(0.3, 1.5) - 1.7).abs() < 1e-9, "and 1.7 is the top");
        assert_eq!(velstand.yaw(0.0, 0.7), 0.7, "no correction when not walking forward");
    }
}
