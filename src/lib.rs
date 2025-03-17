use clap::Parser;
use serde::{Deserialize, Serialize};
use std::net;
use url::Url;

#[derive(Parser, Clone)]
#[group(id = "moqls")]
pub struct Args {
    /// Listen for UDP packets on the given address.
    #[arg(long, default_value = "[::]:0")]
    pub bind: net::SocketAddr,

    /// Connect to the given URL starting with https://
    #[arg(long)]
    pub relay_url: Url,

    /// The TLS configuration.
    #[command(flatten)]
    pub tls: moq_native_ietf::tls::Args,

    /// The namespace for the group
    #[arg(long, default_value = "moqls")]
    pub namespace: String,

    /// The track name for the group track
    #[arg(long, default_value = "group")]
    pub group_track: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct JoinRequest {
    pub name: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CommitRequest {
    pub commit: Commit,
    pub welcome: Option<Welcome>,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum Proposal {
    Add(String),
    Remove(String),
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Commit {
    pub epoch: u64,
    pub proposal: Proposal,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Welcome {
    pub name: String,
    pub epoch: u64,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum GroupEvent {
    JoinRequest(String),
    LeaveRequest(String),
    Commit(Commit, Option<Welcome>),
}
