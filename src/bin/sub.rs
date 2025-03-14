use moq_native_ietf::quic;
use std::net;
use url::Url;

use clap::Parser;

use moq_transport::{coding::Tuple, serve, session::Subscriber as MoqSubscriber};

use anyhow::Context;
use moq_transport::serve::{
    DatagramsReader, StreamReader, SubgroupsReader, TrackReader, TrackReaderMode,
};

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

    /// The name of the clock track.
    #[arg(long, default_value = "clock")]
    pub namespace: String,

    /// The name of the clock track.
    #[arg(long, default_value = "now")]
    pub track: String,
}

pub struct Subscriber {
    track: TrackReader,
}

impl Subscriber {
    pub fn new(track: TrackReader) -> Self {
        Self { track }
    }

    pub async fn run(self) -> anyhow::Result<()> {
        match self.track.mode().await.context("failed to get mode")? {
            TrackReaderMode::Stream(stream) => Self::recv_stream(stream).await,
            TrackReaderMode::Subgroups(subgroups) => Self::recv_subgroups(subgroups).await,
            TrackReaderMode::Datagrams(datagrams) => Self::recv_datagrams(datagrams).await,
        }
    }

    async fn recv_stream(mut track: StreamReader) -> anyhow::Result<()> {
        while let Some(mut subgroup) = track.next().await? {
            while let Some(object) = subgroup.read_next().await? {
                let str = String::from_utf8_lossy(&object);
                println!("{}", str);
            }
        }

        Ok(())
    }

    async fn recv_subgroups(mut subgroups: SubgroupsReader) -> anyhow::Result<()> {
        while let Some(mut subgroup) = subgroups.next().await? {
            let base = subgroup
                .read_next()
                .await
                .context("failed to get first object")?
                .context("empty subgroup")?;

            let base = String::from_utf8_lossy(&base);

            while let Some(object) = subgroup.read_next().await? {
                let str = String::from_utf8_lossy(&object);
                println!("{}{}", base, str);
            }
        }

        Ok(())
    }

    async fn recv_datagrams(mut datagrams: DatagramsReader) -> anyhow::Result<()> {
        while let Some(datagram) = datagrams.read().await? {
            let str = String::from_utf8_lossy(&datagram.payload);
            println!("{}", str);
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
    let tls = config.tls.load()?;

    let quic = quic::Endpoint::new(quic::Config {
        bind: config.bind,
        tls,
    })?;

    log::info!("connecting to server: url={}", config.url);

    let session = quic.client.connect(&config.url).await?;

    let (session, mut subscriber) = MoqSubscriber::connect(session)
        .await
        .context("failed to create MoQ Transport session")?;

    let (prod, sub) =
        serve::Track::new(Tuple::from_utf8_path(&config.namespace), config.track).produce();

    let clock = Subscriber::new(sub);

    tokio::select! {
        res = session.run() => res.context("session error")?,
        res = clock.run() => res.context("clock error")?,
        res = subscriber.subscribe(prod) => res.context("failed to subscribe to track")?,
    }

    Ok(())
}
