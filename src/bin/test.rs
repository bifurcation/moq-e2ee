use anyhow::Context;
use clap::Parser;
use moq_native_ietf::quic;
use moq_transport::{
    coding::Tuple,
    serve::{self, TrackWriter, TracksReader},
    session::{Publisher, Session, Subscriber},
};
use url::Url;

use moq_e2ee::*;

async fn relay_session(config: &Args) -> anyhow::Result<(Session, Publisher, Subscriber)> {
    let tls = config.tls.load()?;
    let quic = quic::Endpoint::new(quic::Config {
        bind: config.bind,
        tls,
    })?;

    let quic_session = quic.client.connect(&config.relay_url).await?;
    Ok(Session::connect(quic_session).await?)
}

struct DeliveryServiceInstance {
    ds: DeliveryService,
    session: Session,
    publisher: Publisher,
    reader: TracksReader,
}

impl DeliveryServiceInstance {
    async fn new(http_bind: String, config: &Args) -> anyhow::Result<Self> {
        let (session, publisher, _) = relay_session(&config).await?;

        let (mut writer, _, reader) = serve::Tracks {
            namespace: Tuple::from_utf8_path(&config.namespace),
        }
        .produce();

        let group_track = writer.create(&config.group_track).unwrap();
        let ds = DeliveryService::new(http_bind, group_track.groups()?);

        Ok(Self {
            ds,
            session,
            publisher,
            reader,
        })
    }

    async fn run(mut self) -> anyhow::Result<()> {
        tokio::select! {
            res = self.session.run() => res.context("session error"),
            res = self.ds.run() => res.context("clock error"),
            res = self.publisher.announce(self.reader) => res.context("publish error"),
        }
    }
}

struct ClientInstance {
    client: Client,
    session: Session,
    subscriber: Subscriber,
    prod: TrackWriter,
}

impl ClientInstance {
    async fn new(
        name: &str,
        join_url: &Url,
        commit_url: &Url,
        config: &Args,
    ) -> anyhow::Result<Self> {
        let (session, _, subscriber) = relay_session(config)
            .await
            .context("failed to create MoQ Transport session")?;

        let (prod, sub) = serve::Track::new(
            Tuple::from_utf8_path(&config.namespace),
            config.group_track.clone(),
        )
        .produce();

        let client = Client::new(name.to_string(), join_url.clone(), commit_url.clone(), sub);

        Ok(Self {
            client,
            session,
            subscriber,
            prod,
        })
    }

    async fn run(mut self) -> anyhow::Result<()> {
        tokio::select! {
            res = self.session.run() => res.context("session error"),
            res = self.client.run() => res.context("client error"),
            res = self.subscriber.subscribe(self.prod) => res.context("group subscribe error"),
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    let port = 3000;
    let http_bind = format!("[::]:{}", port);
    let join_url = Url::parse(&format!("http://localhost:{}/join", port))?;
    let commit_url = Url::parse(&format!("http://localhost:{}/commit", port))?;

    // Disable tracing so we don't get a bunch of Quinn spam.
    let tracer = tracing_subscriber::FmtSubscriber::builder()
        .with_max_level(tracing::Level::WARN)
        .finish();
    tracing::subscriber::set_global_default(tracer).unwrap();

    let config = Args::parse();

    // Create the DS
    let ds = DeliveryServiceInstance::new(http_bind, &config).await?;
    let ds_task = tokio::spawn(ds.run());

    // epoch 0
    let mut client_a = ClientInstance::new("alice", &join_url, &commit_url, &config).await?;
    let client_a_epochs = client_a.client.epochs();
    let client_a_task = tokio::spawn(client_a.run());

    let client_a_epoch = client_a_epochs.recv()?;
    assert_eq!(client_a_epoch, 0);

    // epoch 1
    let mut client_b = ClientInstance::new("bob", &join_url, &commit_url, &config).await?;
    let client_b_epochs = client_b.client.epochs();
    let client_b_task = tokio::spawn(client_b.run());

    let client_a_epoch = client_a_epochs.recv()?;
    assert_eq!(client_a_epoch, 1);

    let client_b_epoch = client_b_epochs.recv()?;
    assert_eq!(client_b_epoch, 1);

    // epoch 2
    let mut client_c = ClientInstance::new("carol", &join_url, &commit_url, &config).await?;
    let client_c_epochs = client_c.client.epochs();
    let client_c_task = tokio::spawn(client_c.run());

    let client_a_epoch = client_a_epochs.recv()?;
    assert_eq!(client_a_epoch, 2);

    let client_b_epoch = client_b_epochs.recv()?;
    assert_eq!(client_b_epoch, 2);

    let client_c_epoch = client_c_epochs.recv()?;
    assert_eq!(client_c_epoch, 2);

    println!("ok");

    ds_task.abort();
    client_a_task.abort();
    client_b_task.abort();
    client_c_task.abort();
    Ok(())
}
