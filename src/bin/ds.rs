use anyhow::Context;
use clap::Parser;
use moq_native_ietf::quic;
use moq_transport::{coding::Tuple, serve, session::Publisher};
use std::net;

use moq_e2ee::*;

#[derive(Parser, Clone)]
pub struct Cli {
    /// Configuration options that are common to both the DS and the clients
    #[command(flatten)]
    pub common: moq_e2ee::Args,

    /// Listen for HTTP connections on the given address.
    #[arg(long, default_value = "[::]:3000")]
    pub http_bind: net::SocketAddr,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    // Disable tracing so we don't get a bunch of Quinn spam.
    let tracer = tracing_subscriber::FmtSubscriber::builder()
        .with_max_level(tracing::Level::WARN)
        .finish();
    tracing::subscriber::set_global_default(tracer).unwrap();

    let config = Cli::parse();
    let tls = config.common.tls.load()?;

    let quic = quic::Endpoint::new(quic::Config {
        bind: config.common.bind,
        tls,
    })?;

    log::info!("connecting to server: url={}", config.common.relay_url);

    let session = quic.client.connect(&config.common.relay_url).await?;

    let (session, mut publisher) = Publisher::connect(session)
        .await
        .context("failed to create MoQ Transport session")?;

    let (mut writer, _, reader) = serve::Tracks {
        namespace: Tuple::from_utf8_path(&config.common.namespace),
    }
    .produce();

    let group_track = writer.create(&config.common.group_track).unwrap();
    let ds = DeliveryService::new(group_track.groups()?);

    tokio::select! {
        res = session.run() => res.context("session error")?,
        res = ds.run() => res.context("clock error")?,
        res = publisher.announce(reader) => res.context("publish error")?,
    }

    Ok(())
}
