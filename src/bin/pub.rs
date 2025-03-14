use moq_native_ietf::quic;
use std::net;
use url::Url;

use clap::Parser;

use moq_transport::{coding::Tuple, serve, session::Publisher as MoqPublisher};

use anyhow::Context;
use moq_transport::serve::{Subgroup, SubgroupWriter, SubgroupsWriter};

use chrono::prelude::*;
#[derive(Parser, Clone)]
pub struct Cli {
    /// Listen for UDP packets on the given address.
    #[arg(long, default_value = "[::]:0")]
    pub bind: net::SocketAddr,

    /// Connect to the given URL starting with https://
    #[arg()]
    pub url: Url,

    /// The TLS configuration.
    #[command(flatten)]
    pub tls: moq_native_ietf::tls::Args,

    /// Publish the current time to the relay, otherwise only subscribe.
    #[arg(long)]
    pub publish: bool,

    /// The name of the clock track.
    #[arg(long, default_value = "clock")]
    pub namespace: String,

    /// The name of the clock track.
    #[arg(long, default_value = "now")]
    pub track: String,
}

pub struct Publisher {
    track: SubgroupsWriter,
}

impl Publisher {
    pub fn new(track: SubgroupsWriter) -> Self {
        Self { track }
    }

    pub async fn run(mut self) -> anyhow::Result<()> {
        let start = Utc::now();
        let mut now = start;

        // Just for fun, don't start at zero.
        let mut sequence = start.minute();

        loop {
            let segment = self
                .track
                .create(Subgroup {
                    group_id: sequence as u64,
                    subgroup_id: 0,
                    priority: 0,
                })
                .context("failed to create minute segment")?;

            sequence += 1;

            tokio::spawn(async move {
                if let Err(err) = Self::send_segment(segment, now).await {
                    log::warn!("failed to send minute: {:?}", err);
                }
            });

            let next = now + chrono::Duration::try_minutes(1).unwrap();
            let next = next.with_second(0).unwrap().with_nanosecond(0).unwrap();

            let delay = (next - now).to_std().unwrap();
            tokio::time::sleep(delay).await;

            now = next; // just assume we didn't undersleep
        }
    }

    async fn send_segment(
        mut segment: SubgroupWriter,
        mut now: DateTime<Utc>,
    ) -> anyhow::Result<()> {
        // Everything but the second.
        let base = now.format("%Y-%m-%d %H:%M:").to_string();

        segment
            .write(base.clone().into())
            .context("failed to write base")?;

        loop {
            let delta = now.format("%S").to_string();
            segment
                .write(delta.clone().into())
                .context("failed to write delta")?;

            println!("{}{}", base, delta);

            let next = now + chrono::Duration::try_seconds(1).unwrap();
            let next = next.with_nanosecond(0).unwrap();

            let delay = (next - now).to_std().unwrap();
            tokio::time::sleep(delay).await;

            // Get the current time again to check if we overslept
            let next = Utc::now();
            if next.minute() != now.minute() {
                return Ok(());
            }

            now = next;
        }
    }
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
    let tls = config.tls.load()?;

    let quic = quic::Endpoint::new(quic::Config {
        bind: config.bind,
        tls,
    })?;

    log::info!("connecting to server: url={}", config.url);

    let session = quic.client.connect(&config.url).await?;

    let (session, mut publisher) = MoqPublisher::connect(session)
        .await
        .context("failed to create MoQ Transport session")?;

    let (mut writer, _, reader) = serve::Tracks {
        namespace: Tuple::from_utf8_path(&config.namespace),
    }
    .produce();

    let track = writer.create(&config.track).unwrap();
    let clock = Publisher::new(track.groups()?);

    tokio::select! {
        res = session.run() => res.context("session error")?,
        res = clock.run() => res.context("clock error")?,
        res = publisher.announce(reader) => res.context("failed to serve tracks")?,
    }

    Ok(())
}
