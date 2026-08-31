use anyhow::Context;
use quacksat_core::config::{Backend, Config};

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| Config::DEFAULT_PATH.to_string());
    let config = Config::load(&path).with_context(|| format!("loading config from {path}"))?;

    tracing::info!(backend = ?config.backend, "quacksat starting");

    match config.backend {
        Backend::Wyoming => anyhow::bail!("wyoming backend not implemented yet"),
        Backend::Agent => anyhow::bail!("agent backend not implemented yet"),
    }
}
