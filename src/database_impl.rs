use tracing::Level;

use crate::storage::{HasStorage, StorageBuilder};
use crate::zalsa::HasJar;
use crate::{Database, Event, Storage};

/// Default database implementation that you can use if you don't
/// require any custom user data.
#[derive(Clone)]
pub struct DatabaseImpl {
    storage: Storage<Self>,
}

impl Default for DatabaseImpl {
    fn default() -> Self {
        Self {
            // Default behavior: trace events at DEBUG when detailed tracing is enabled.
            storage: Storage::new(
                if cfg!(feature = "detailed-trace") && tracing::enabled!(Level::DEBUG) {
                    Some(Box::new(|event| {
                        crate::tracing::debug!("salsa_event({:?})", event)
                    }))
                } else {
                    None
                },
            ),
        }
    }
}

impl DatabaseImpl {
    /// Create a new database; equivalent to `Self::default`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a builder that supports explicit ingredient registration.
    pub fn builder() -> DatabaseImplBuilder {
        DatabaseImplBuilder {
            storage: Storage::builder(),
        }
    }

    pub fn storage(&self) -> &Storage<Self> {
        &self.storage
    }
}

/// Builder for the default Salsa database implementation.
pub struct DatabaseImplBuilder {
    storage: StorageBuilder<DatabaseImpl>,
}

impl DatabaseImplBuilder {
    /// Manually registers an ingredient.
    ///
    /// This is required for generic Salsa structs when static inventory is disabled.
    pub fn ingredient<I: HasJar>(mut self) -> Self {
        self.storage = self.storage.ingredient::<I>();
        self
    }

    /// Sets the database event callback.
    pub fn event_callback(mut self, callback: Box<dyn Fn(Event) + Send + Sync + 'static>) -> Self {
        self.storage = self.storage.event_callback(callback);
        self
    }

    /// Builds the database.
    pub fn build(self) -> DatabaseImpl {
        DatabaseImpl {
            storage: self.storage.build(),
        }
    }
}

impl Database for DatabaseImpl {}

// SAFETY: The `storage` and `storage_mut` fields return a reference to the same storage field owned by `self`.
unsafe impl HasStorage for DatabaseImpl {
    #[inline(always)]
    fn storage(&self) -> &Storage<Self> {
        &self.storage
    }

    #[inline(always)]
    fn storage_mut(&mut self) -> &mut Storage<Self> {
        &mut self.storage
    }
}
