use tokio_util::sync::CancellationToken;

use crate::{config::Config, database, reactor::events::EventSubscriber};

#[derive(Clone)]
pub struct Env {
    pub config: Config,
    pub cancel_token: CancellationToken,
    pub reader: database::Reader,
    pub event_subscriber: EventSubscriber,
}
