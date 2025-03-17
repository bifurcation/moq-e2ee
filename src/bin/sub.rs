use anyhow::Context;
use clap::Parser;
use moq_native_ietf::quic;
use moq_transport::serve::{TrackReader, TrackReaderMode};
use moq_transport::{coding::Tuple, serve, session::Subscriber as MoqSubscriber};
use reqwest::StatusCode;
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

pub struct Subscriber {
    name: String,
    join_url: Url,
    commit_url: Url,
    group_track: TrackReader,
    epoch: Option<u64>,
}

impl Subscriber {
    pub fn new(name: String, join_url: Url, commit_url: Url, group_track: TrackReader) -> Self {
        Self {
            name,
            join_url,
            commit_url,
            group_track,
            epoch: None,
        }
    }

    pub async fn run(mut self) -> anyhow::Result<()> {
        println!("Subscriber::run");
        let mode = self
            .group_track
            .mode()
            .await
            .context("failed to get mode")?;
        let TrackReaderMode::Subgroups(mut group_subgroups) = mode else {
            panic!("Unexpected track mode");
        };

        println!("sending join request");
        let http_client = reqwest::Client::new();
        let res = http_client
            .post(self.join_url)
            .json(&JoinRequest {
                name: self.name.clone(),
            })
            .send()
            .await?;

        match res.status() {
            StatusCode::CREATED => {
                println!("creating group");
                // Create the group at epoch 0
                self.epoch = Some(0);
            }
            StatusCode::ACCEPTED => {
                println!("awaiting welcome");
                // No epoch at start
                self.epoch = None;
            }
            _ => panic!("Unexpected status code in join request"),
        }

        while let Some(mut subgroup) = group_subgroups.next().await? {
            let object = subgroup.read_next().await?.unwrap();
            let event_str = String::from_utf8_lossy(&object);
            let event: GroupEvent = serde_json::from_str(&event_str)?;

            println!("received group event: {:?}", event);

            match event {
                GroupEvent::JoinRequest(name) => {
                    let Some(epoch) = self.epoch else {
                        break;
                    };

                    http_client
                        .post(self.commit_url.clone())
                        .json(&CommitRequest {
                            commit: Commit {
                                epoch,
                                proposal: Proposal::Add(name.clone()),
                            },
                            welcome: Some(Welcome {
                                name,
                                epoch: epoch + 1,
                            }),
                        })
                        .send()
                        .await?;
                }
                GroupEvent::LeaveRequest(name) => {
                    let Some(epoch) = self.epoch else {
                        break;
                    };

                    http_client
                        .post(self.commit_url.clone())
                        .json(&CommitRequest {
                            commit: Commit {
                                epoch,
                                proposal: Proposal::Remove(name.clone()),
                            },
                            welcome: Some(Welcome {
                                name,
                                epoch: epoch + 1,
                            }),
                        })
                        .send()
                        .await?;
                }
                GroupEvent::Commit(commit, welcome) => match (self.epoch, welcome) {
                    (Some(epoch), _) if epoch == commit.epoch => {
                        // Handle the commit
                        self.epoch = Some(epoch + 1);
                    }
                    (None, Some(welcome)) if self.name == welcome.name => {
                        self.epoch = Some(welcome.epoch);
                    }
                    _ => {
                        println!("Ignored commit event");
                    }
                },
            }
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

    // Connect to the relay
    let quic = quic::Endpoint::new(quic::Config {
        bind: config.common.bind,
        tls,
    })?;

    println!("connecting to server: url={}", config.common.relay_url);
    let session = quic.client.connect(&config.common.relay_url).await?;

    let (session, mut subscriber) = MoqSubscriber::connect(session)
        .await
        .context("failed to create MoQ Transport session")?;

    // Create the two subscriptions
    let (group_prod, group_sub) = serve::Track::new(
        Tuple::from_utf8_path(&config.common.namespace),
        config.common.group_track,
    )
    .produce();

    let client = Subscriber::new(config.name, config.join_url, config.commit_url, group_sub);

    println!("running...");
    tokio::select! {
        res = session.run() => res.context("session error")?,
        res = client.run() => res.context("client error")?,
        res = subscriber.subscribe(group_prod) => res.context("group subscribe error")?,
    }

    Ok(())
}
