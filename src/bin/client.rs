use anyhow::Context;
use clap::Parser;
use moq_native_ietf::quic;
use moq_transport::{coding::Tuple, serve, session::Subscriber};
use url::Url;

use moq_e2ee::*;

#[derive(Parser, Clone)]
pub struct Cli {
    /// Configuration options that are common to both the DS and the clients
    #[command(flatten)]
    pub common: moq_e2ee::Args,

    /// A user name for this client
    #[arg(long)]
    pub name: String,

    /// Use the following URL to request to join
    #[arg(long)]
    pub join_url: Url,

    /// Use the following URL to request to commit
    #[arg(long)]
    pub commit_url: Url,
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

    // Connect to the relay
    let quic = quic::Endpoint::new(quic::Config {
        bind: config.common.bind,
        tls,
    })?;

    println!("connecting to server: url={}", config.common.relay_url);
    let session = quic.client.connect(&config.common.relay_url).await?;

    let (session, mut subscriber) = Subscriber::connect(session)
        .await
        .context("failed to create MoQ Transport session")?;

    // Create the two subscriptions
    let (prod, sub) = serve::Track::new(
        Tuple::from_utf8_path(&config.common.namespace),
        config.common.group_track,
    )
    .produce();

    let client = Client::new(config.name, config.join_url, config.commit_url, sub);

    println!("running...");
    tokio::select! {
        res = session.run() => res.context("session error")?,
        res = client.run() => res.context("client error")?,
        res = subscriber.subscribe(prod) => res.context("group subscribe error")?,
    }

    Ok(())
}
