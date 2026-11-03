//! Shared application state.

use std::sync::Arc;

use crate::clock::Clock;
use crate::config::Config;
use crate::keys::KeyMaterial;
use crate::roles::RoleSource;
use crate::store::Store;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub keys: KeyMaterial,
    pub roles: Arc<dyn RoleSource>,
    pub clock: Arc<dyn Clock>,
    pub store: Arc<Store>,
}

impl AppState {
    pub fn new(
        config: Config,
        keys: KeyMaterial,
        roles: Arc<dyn RoleSource>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            config: Arc::new(config),
            keys,
            roles,
            clock,
            store: Arc::new(Store::new()),
        }
    }
}
