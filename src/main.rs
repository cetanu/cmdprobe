use std::net::UdpSocket;
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use cadence::{BufferedUdpMetricSink, NopMetricSink, QueuingMetricSink, StatsdClient};
use clap::Parser;
use tracing_subscriber::{EnvFilter, FmtSubscriber};

use cmdprobe::probe::CommandProbe;

#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    #[clap(
        short,
        long,
        default_value = "cmdprobe.yaml",
        env = "CMDPROBE_CONFIG_FILE"
    )]
    pub config_file: PathBuf,

    #[clap(short, long, env = "CMDPROBE_STATSD_ADDR", value_name = "HOST:PORT")]
    /// Send StatsD metrics to this address. Metrics are disabled by default.
    pub statsd_address: Option<String>,
}

fn metrics_client(addr: Option<&str>) -> Result<StatsdClient> {
    let prefix = "cmdprobe";
    let Some(addr) = addr else {
        return Ok(StatsdClient::from_sink(prefix, NopMetricSink));
    };
    let (host, port) = addr
        .rsplit_once(':')
        .ok_or_else(|| anyhow!("invalid StatsD address {addr:?}; use <host>:<port>"))?;
    if host.is_empty() {
        return Err(anyhow!("invalid StatsD address {addr:?}; host is empty"));
    }
    let port = port
        .parse::<u16>()
        .with_context(|| format!("invalid StatsD port {port:?}"))?;
    let socket = UdpSocket::bind("0.0.0.0:0").context("failed to bind StatsD socket")?;
    socket
        .set_nonblocking(true)
        .context("failed to configure StatsD socket")?;
    let udp_sink = BufferedUdpMetricSink::from((host, port), socket)
        .map_err(|error| anyhow!(error))
        .context("failed to create StatsD sink")?;
    let queuing_sink = QueuingMetricSink::from(udp_sink);
    Ok(StatsdClient::from_sink(prefix, queuing_sink))
}

fn main() -> Result<()> {
    let args = Args::parse();
    let statsd_client = metrics_client(args.statsd_address.as_deref())?;

    FmtSubscriber::builder()
        .with_env_filter(EnvFilter::from_default_env())
        .compact()
        .init();

    let probe = CommandProbe::new(args.config_file, statsd_client)?;
    probe.run_checks()
}
