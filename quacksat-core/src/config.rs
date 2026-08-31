use serde::Deserialize;

/// Which voice backend quacksat runs with. Selected in the config file,
/// never at compile time, so the same binary ships for both setups.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
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
    }

    #[test]
    fn rejects_unknown_keys() {
        assert!(toml::from_str::<Config>("backend = \"agent\"\ntypo = 1").is_err());
    }
}
