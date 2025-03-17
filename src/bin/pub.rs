use anyhow::Context;
use axum::{extract::State, http::StatusCode, routing::post, Json, Router};
use clap::Parser;
use moq_native_ietf::quic;
use moq_transport::{
    coding::Tuple,
    serve,
    serve::{Subgroup, SubgroupsWriter},
    session::Publisher as MoqPublisher,
};
use serde::Serialize;
use std::net;
use std::sync::{Arc, Mutex};

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

struct TrackWithSeq {
    track: SubgroupsWriter,
    seq: u64,
}

impl TrackWithSeq {
    fn send(&mut self, message: impl Serialize) {
        let group_id = self.seq;
        self.seq += 1;

        let message = serde_json::to_string(&message).unwrap();

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
            .write(message.clone().into())
            .context("failed to write")
            .unwrap();

        println!("send: {}", message);
    }
}

#[derive(Clone)]
struct PublisherState {
    epoch: Arc<Mutex<Option<u64>>>,
    group_track: Arc<Mutex<TrackWithSeq>>,
}

struct Publisher {
    state: PublisherState,
}

impl Publisher {
    pub fn new(group_track: SubgroupsWriter) -> Self {
        Self {
            state: PublisherState {
                epoch: Arc::new(Mutex::new(None)),
                group_track: Arc::new(Mutex::new(TrackWithSeq {
                    track: group_track,
                    seq: 0,
                })),
            },
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

async fn join(
    State(state): State<PublisherState>,
    Json(join_request): Json<JoinRequest>,
) -> StatusCode {
    // If the group has not been initialized, tell the requestor to create it
    let mut epoch = state.epoch.lock().unwrap();
    if let None = epoch.as_mut() {
        *epoch = Some(0);
        return StatusCode::CREATED;
    }

    // Otherwise, ask the membership to add the new user
    let mut group_track = state.group_track.lock().unwrap();
    group_track.send(GroupEvent::JoinRequest(join_request.name));
    StatusCode::ACCEPTED
}

async fn commit(
    State(state): State<PublisherState>,
    Json(commit_request): Json<CommitRequest>,
) -> StatusCode {
    let mut epoch = state.epoch.lock().unwrap();
    let Some(epoch) = epoch.as_mut() else {
        return StatusCode::NOT_FOUND;
    };

    if commit_request.commit.epoch != *epoch {
        return StatusCode::BAD_REQUEST;
    }

    *epoch += 1;

    let mut group_track = state.group_track.lock().unwrap();
    group_track.send(GroupEvent::Commit(
        commit_request.commit,
        commit_request.welcome,
    ));

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

    let group_track = writer.create(&config.common.group_track).unwrap();
    let ds = Publisher::new(group_track.groups()?);

    tokio::select! {
        res = session.run() => res.context("session error")?,
        res = ds.run() => res.context("clock error")?,
        res = publisher.announce(reader) => res.context("publish error")?,
    }

    Ok(())
}
