use serde::{Deserialize, Serialize};

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
