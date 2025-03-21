use anyhow::Context;
use moq_transport::serve::{TrackReader, TrackReaderMode};
use reqwest::StatusCode;
use url::Url;

use crate::messages::*;

pub struct Client {
    name: String,
    join_url: Url,
    commit_url: Url,
    track: TrackReader,
    epoch: Option<u64>,
}

impl Client {
    pub fn new(name: String, join_url: Url, commit_url: Url, track: TrackReader) -> Self {
        Self {
            name,
            join_url,
            commit_url,
            track,
            epoch: None,
        }
    }

    pub async fn run(mut self) -> anyhow::Result<()> {
        println!("sending join request");
        let http_client = reqwest::Client::new();
        let res = http_client
            .post(self.join_url)
            .json(&JoinRequest {
                name: self.name.clone(),
            })
            .send()
            .await?;

        self.epoch = match res.status() {
            StatusCode::CREATED => {
                println!("creating group");
                Some(0)
            }
            StatusCode::ACCEPTED => {
                println!("awaiting Welcome");
                None
            }
            _ => panic!("Unexpected status code in join request"),
        };

        let mode = self.track.mode().await.context("failed to get mode")?;
        let TrackReaderMode::Subgroups(mut subgroups) = mode else {
            panic!("unexpected stream mode");
        };

        while let Some(mut subgroup) = subgroups.next().await? {
            while let Some(object) = subgroup.read_next().await? {
                let event_str = String::from_utf8_lossy(&object);
                let event: GroupEvent = serde_json::from_str(&event_str)?;

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
                                    name: name.clone(),
                                    epoch: epoch + 1,
                                }),
                            })
                            .send()
                            .await?;

                        println!("sent Commit(Add({})) => {}", name, res.status());
                    }
                    GroupEvent::LeaveRequest(name) => {
                        let Some(epoch) = self.epoch else {
                            break;
                        };

                        let res = http_client
                            .post(self.commit_url.clone())
                            .json(&CommitRequest {
                                commit: Commit {
                                    epoch,
                                    proposal: Proposal::Remove(name.clone()),
                                },
                                welcome: None,
                            })
                            .send()
                            .await?;

                        println!("sent Commit(Remove({})) => {}", name, res.status());
                    }
                    GroupEvent::Commit(commit, welcome) => match (self.epoch, welcome) {
                        (Some(epoch), _) if epoch == commit.epoch => {
                            // Handle the commit
                            println!("handling commit {} => {}", epoch, epoch + 1);
                            self.epoch = Some(epoch + 1);
                        }

                        (None, Some(welcome)) if self.name == welcome.name => {
                            println!("joining via welcome at {}", welcome.epoch);
                            self.epoch = Some(welcome.epoch);
                        }
                        _ => {
                            println!("Ignored commit event");
                        }
                    },
                }
            }
        }

        Ok(())
    }
}
