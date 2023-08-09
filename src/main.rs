use std::net::UdpSocket;

use anyhow::Result;
use cadence::{BufferedUdpMetricSink, QueuingMetricSink, StatsdClient};
use clap::Parser;
use tracing_subscriber::{EnvFilter, FmtSubscriber};

use cmdprobe::CommandProbe;

#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    #[clap(short, long, default_value = "0.0.0.0:0", env = "CMDPROBE_STATSD_ADDR")]
    // By default it will send stats nowhere
    pub statsd_address: String,
}

fn metrics_client(addr: String) -> StatsdClient {
    let prefix = "cmdprobe";
    let socket = UdpSocket::bind("0.0.0.0:0").unwrap();
    socket.set_nonblocking(true).unwrap();
    let host = {
        if let Some((host, port)) = addr.split_once(":") {
            (host, port.parse().unwrap())
        } else {
            panic!("Invalid statsd host supplied, please use <address>:<port>");
        }
    };
    let udp_sink = BufferedUdpMetricSink::from(host, socket).unwrap();
    let queuing_sink = QueuingMetricSink::from(udp_sink);
    StatsdClient::from_sink(prefix, queuing_sink)
}

fn main() -> Result<()> {
    let args = Args::parse();
    let statsd_client = metrics_client(args.statsd_address);

    FmtSubscriber::builder()
        .with_env_filter(EnvFilter::from_default_env())
        .compact()
        .init();

    let probe = CommandProbe::new("config.yaml".into(), statsd_client);
    probe.run_checks()
}
