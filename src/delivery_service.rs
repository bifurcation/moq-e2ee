use anyhow::Context;
use axum::{extract::State, http::StatusCode, routing::post, Json, Router};
use moq_transport::serve::{Subgroup, SubgroupsWriter};
use serde::Serialize;
use std::sync::{Arc, Mutex};

use crate::messages::*;

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
struct DeliveryState {
    epoch: Arc<Mutex<Option<u64>>>,
    track: Arc<Mutex<TrackWithSeq>>,
}

pub struct DeliveryService {
    state: DeliveryState,
}

impl DeliveryService {
    pub fn new(track: SubgroupsWriter) -> Self {
        Self {
            state: DeliveryState {
                epoch: Arc::new(Mutex::new(None)),
                track: Arc::new(Mutex::new(TrackWithSeq {
                    track: track,
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
    State(state): State<DeliveryState>,
    Json(join_request): Json<JoinRequest>,
) -> StatusCode {
    // If the group has not been initialized, tell the requestor to create it
    let mut epoch = state.epoch.lock().unwrap();
    if let None = epoch.as_mut() {
        *epoch = Some(0);
        return StatusCode::CREATED;
    }

    // Otherwise, ask the membership to add the new user
    println!("PUB {:?}", join_request);
    let mut track = state.track.lock().unwrap();
    track.send(GroupEvent::JoinRequest(join_request.name));
    StatusCode::ACCEPTED
}

async fn commit(
    State(state): State<DeliveryState>,
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

    let mut track = state.track.lock().unwrap();
    track.send(GroupEvent::Commit(
        commit_request.commit,
        commit_request.welcome,
    ));

    StatusCode::OK
}
