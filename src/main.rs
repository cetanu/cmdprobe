use anyhow::Result;
use tracing_subscriber::{EnvFilter, FmtSubscriber};

use cmdprobe::run_checks_from_file;

fn main() -> Result<()> {
    FmtSubscriber::builder()
        .with_env_filter(EnvFilter::from_default_env())
        .compact()
        .init();

    run_checks_from_file("config.yaml".into())
}
