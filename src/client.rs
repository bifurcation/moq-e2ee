use anyhow::Context;
use moq_transport::serve::{TrackReader, TrackReaderMode};
use reqwest::StatusCode;
use std::sync::mpsc;
use url::Url;

use crate::messages::*;

pub struct Client {
    name: String,
    join_url: Url,
    commit_url: Url,
    track: TrackReader,
    epoch: Option<u64>,
    epochs: Option<mpsc::Sender<u64>>,
}

impl Client {
    pub fn new(name: String, join_url: Url, commit_url: Url, track: TrackReader) -> Self {
        Self {
            name,
            join_url,
            commit_url,
            track,
            epoch: None,
            epochs: None,
        }
    }

    pub fn epochs(&mut self) -> mpsc::Receiver<u64> {
        let (send, recv) = mpsc::channel();
        self.epochs = Some(send);
        recv
    }

    pub fn update_epoch(&mut self, epoch: u64) -> anyhow::Result<()> {
        self.epoch = Some(epoch);

        if let Some(epochs) = &self.epochs {
            epochs.send(epoch)?;
        }

        Ok(())
    }

    pub async fn run(mut self) -> anyhow::Result<()> {
        let http_client = reqwest::Client::new();
        let res = http_client
            .post(self.join_url.clone())
            .json(&JoinRequest {
                name: self.name.clone(),
            })
            .send()
            .await?;

        match res.status() {
            StatusCode::CREATED => {
                self.update_epoch(0)?;
            }
            StatusCode::ACCEPTED => {}
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
                                welcome: None,
                            })
                            .send()
                            .await?;
                    }
                    GroupEvent::Commit(commit, welcome) => match (self.epoch, welcome) {
                        (Some(epoch), _) if epoch == commit.epoch => {
                            self.update_epoch(epoch + 1)?;
                        }

                        (None, Some(welcome)) if self.name == welcome.name => {
                            self.update_epoch(welcome.epoch)?;
                        }
                        _ => { /* ignore commit event */ }
                    },
                }
            }
        }

        Ok(())
    }
}
