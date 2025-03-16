use moq_native_ietf::quic;
use url::Url;

use clap::Parser;

use moq_transport::{coding::Tuple, serve, session::Subscriber as MoqSubscriber};

use anyhow::Context;
use moq_transport::serve::{SubgroupsReader, TrackReader, TrackReaderMode};

#[derive(Parser, Clone)]
pub struct Cli {
    /// Configuration options that are common to both the DS and the clients
    #[command(flatten)]
    pub common: moq_e2ee::Args,

    /// Use the following URL to request to join
    #[arg(long)]
    pub join_url: Url,

    /// Use the following URL to request to commit
    #[arg(long)]
    pub commit_url: Url,
}

pub struct Subscriber {
    track: TrackReader,
}

impl Subscriber {
    pub fn new(track: TrackReader) -> Self {
        Self { track }
    }

    pub async fn run(self) -> anyhow::Result<()> {
        let mode = self.track.mode().await.context("failed to get mode")?;
        let TrackReaderMode::Subgroups(subgroups) = mode else {
            panic!("Unexpected track mode");
        };

        Self::recv_subgroups(subgroups).await
    }

    async fn recv_subgroups(mut subgroups: SubgroupsReader) -> anyhow::Result<()> {
        println!("receiving as subgroups");
        while let Some(mut subgroup) = subgroups.next().await? {
            let base = subgroup
                .read_next()
                .await
                .context("failed to get first object")?
                .context("empty subgroup")?;

            let base = String::from_utf8_lossy(&base);
            println!("{}", base);
        }

        Ok(())
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
    let tls = config.common.tls.load()?;

    let quic = quic::Endpoint::new(quic::Config {
        bind: config.common.bind,
        tls,
    })?;

    log::info!("connecting to server: url={}", config.common.relay_url);

    let session = quic.client.connect(&config.common.relay_url).await?;

    let (session, mut subscriber) = MoqSubscriber::connect(session)
        .await
        .context("failed to create MoQ Transport session")?;

    let (prod, sub) = serve::Track::new(
        Tuple::from_utf8_path(&config.common.namespace),
        config.common.welcome_track,
    )
    .produce();

    let clock = Subscriber::new(sub);

    tokio::select! {
        res = session.run() => res.context("session error")?,
        res = clock.run() => res.context("clock error")?,
        res = subscriber.subscribe(prod) => res.context("failed to subscribe to track")?,
    }

    Ok(())
}
