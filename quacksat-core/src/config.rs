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
}

/// Settings for the `agent` backend (WebSocket bridge, ADR 0004).
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AgentConfig {
    /// Bridge WebSocket URL (`ws://` or `wss://`).
    pub url: String,
    /// Optional bearer token sent on the WebSocket upgrade.
    pub token: Option<String>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            url: "ws://127.0.0.1:8765".to_string(),
            token: None,
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
            model: "hey_jarvis_v0.1.onnx".to_string(),
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

    #[test]
    fn parses_minimal_config() {
        let config: Config = toml::from_str("backend = \"wyoming\"").unwrap();
        assert_eq!(config.backend, Backend::Wyoming);
        assert_eq!(config.robotd_socket, "/run/robotd.sock");
        assert_eq!(config.audio.capture_device, "plughw:aic3104,0");
        assert_eq!(config.wake.mode, WakeMode::Energy);
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
}
