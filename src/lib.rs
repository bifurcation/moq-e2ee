use clap::Parser;
use std::net;
use url::Url;

mod client;
mod delivery_service;
mod messages;

pub use client::Client;
pub use delivery_service::DeliveryService;

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
