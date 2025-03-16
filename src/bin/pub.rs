use moq_native_ietf::quic;
use std::net;

use clap::Parser;

use moq_transport::{coding::Tuple, serve, session::Publisher as MoqPublisher};

use anyhow::Context;
use moq_transport::serve::{Subgroup, SubgroupsWriter};

use axum::{extract::State, http::StatusCode, routing::post, Router};
use std::sync::{Arc, Mutex};

#[derive(Parser, Clone)]
pub struct Cli {
    /// Configuration options that are common to both the DS and the clients
    #[command(flatten)]
    pub common: moq_e2ee::Args,

    /// Listen for HTTP connections on the given address.
    /// TODO(RLB): Allow HTTPS configuration.
    #[arg(long, default_value = "[::]:3000")]
    pub http_bind: net::SocketAddr,
}

struct PublisherState {
    track: SubgroupsWriter,
    seq: u64,
}

impl PublisherState {
    fn send_string(&mut self, message: &str) {
        let group_id = self.seq;
        self.seq += 1;

        let mut group = self
            .track
            .create(Subgroup {
                group_id,
                subgroup_id: 0,
                priority: 0,
            })
            .context("failed to create minute segment")
            .unwrap();

        group
            .write(message.to_string().into())
            .context("failed to write")
            .unwrap();

        println!("send: {}", message);
    }
}

pub struct Publisher {
    state: Arc<Mutex<PublisherState>>,
}

impl Publisher {
    pub fn new(track: SubgroupsWriter) -> Self {
        Self {
            state: Arc::new(Mutex::new(PublisherState { track, seq: 0 })),
        }
    }

    pub async fn run(&self) -> anyhow::Result<()> {
        let app = Router::new()
            .route("/join", post(join))
            .route("/commit", post(commit))
            .with_state(self.state.clone());

        let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
        axum::serve(listener, app).await?;
        Ok(())
    }
}

async fn join(State(state): State<Arc<Mutex<PublisherState>>>) -> StatusCode {
    let Ok(mut state) = state.lock() else {
        todo!();
    };

    state.send_string("join");

    StatusCode::OK
}

async fn commit(State(state): State<Arc<Mutex<PublisherState>>>) -> StatusCode {
    let Ok(mut state) = state.lock() else {
        todo!();
    };

    state.send_string("commit");

    StatusCode::OK
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

    let (session, mut publisher) = MoqPublisher::connect(session)
        .await
        .context("failed to create MoQ Transport session")?;

    let (mut writer, _, reader) = serve::Tracks {
        namespace: Tuple::from_utf8_path(&config.common.namespace),
    }
    .produce();

    let track = writer.create(&config.common.welcome_track).unwrap();
    let clock = Publisher::new(track.groups()?);

    tokio::select! {
        res = session.run() => res.context("session error")?,
        res = clock.run() => res.context("clock error")?,
        res = publisher.announce(reader) => res.context("failed to serve tracks")?,
    }

    Ok(())
}
