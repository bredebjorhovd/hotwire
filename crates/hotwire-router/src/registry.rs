//! Adapter registry.
//!
//! The registry is the only path from the runtime to an [`Adapter`]. Adapters
//! register by their manifest id once; routing an invocation or a cancellation
//! to an unregistered id fails cleanly instead of silently dropping the
//! action.

use std::collections::HashMap;
use std::sync::Arc;

use hotwire_adapter_sdk::{
    ActionInvocation, ActionResult, Adapter, AdapterError, DetectionResult, ValidationResult,
};
use hotwire_core::ActionStatus;
use serde_json::Value;
use thiserror::Error;

/// Errors produced while registering or looking up adapters.
#[derive(Debug, Error)]
pub enum RegistryError {
    /// An adapter with the same manifest id is already registered.
    #[error("adapter `{0}` is already registered")]
    Duplicate(String),
    /// No adapter is registered under the requested id.
    #[error("adapter `{0}` is not registered")]
    NotFound(String),
}

/// Holds every registered [`Adapter`] keyed by manifest id.
#[derive(Default)]
pub struct AdapterRegistry {
    adapters: HashMap<String, Arc<dyn Adapter>>,
}

impl AdapterRegistry {
    /// Creates an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers an adapter under its manifest id.
    ///
    /// # Errors
    ///
    /// Returns [`RegistryError::Duplicate`] when an adapter with the same id
    /// is already registered.
    pub fn register(&mut self, adapter: Arc<dyn Adapter>) -> Result<(), RegistryError> {
        let id = adapter.manifest().id.clone();
        if self.adapters.contains_key(&id) {
            return Err(RegistryError::Duplicate(id));
        }
        self.adapters.insert(id, adapter);
        Ok(())
    }

    /// Returns the adapter registered under `id`, if any.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<Arc<dyn Adapter>> {
        self.adapters.get(id).cloned()
    }

    /// Returns the ids of all registered adapters.
    #[must_use]
    pub fn ids(&self) -> Vec<String> {
        self.adapters.keys().cloned().collect()
    }

    /// Returns whether any adapters are registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.adapters.is_empty()
    }

    /// Executes an invocation through the adapter named by the invocation.
    ///
    /// An unregistered adapter yields a failed [`ActionResult`] rather than
    /// panicking or dropping the execution.
    pub async fn execute(&self, invocation: &ActionInvocation) -> ActionResult {
        match self.get(&invocation.adapter_id) {
            Some(adapter) => adapter.execute(invocation).await,
            None => ActionResult {
                execution_id: invocation.execution_id.clone(),
                status: ActionStatus::Failed,
                message: Some(format!(
                    "adapter `{}` is not registered",
                    invocation.adapter_id
                )),
            },
        }
    }

    /// Cancels an in-flight execution owned by `adapter_id`.
    ///
    /// # Errors
    ///
    /// Returns an [`AdapterError`] when the adapter is not registered or does
    /// not own the execution.
    pub async fn cancel(&self, adapter_id: &str, execution_id: &str) -> Result<(), AdapterError> {
        self.get(adapter_id)
            .ok_or_else(|| {
                AdapterError::Other(format!("adapter `{adapter_id}` is not registered"))
            })?
            .cancel(execution_id)
            .await
    }

    /// Ends a hold interaction owned by `adapter_id`.
    ///
    /// # Errors
    ///
    /// Returns an [`AdapterError`] when the adapter is not registered or does
    /// not own the execution.
    pub async fn release(&self, adapter_id: &str, execution_id: &str) -> Result<(), AdapterError> {
        self.get(adapter_id)
            .ok_or_else(|| {
                AdapterError::Other(format!("adapter `{adapter_id}` is not registered"))
            })?
            .release(execution_id)
            .await
    }

    /// Probes every registered adapter and returns its detection result.
    pub async fn detect_all(&self) -> Vec<DetectionResult> {
        let mut results = Vec::with_capacity(self.adapters.len());
        for adapter in self.adapters.values() {
            results.push(adapter.detect().await);
        }
        results
    }

    /// Validates a binding configuration against a registered adapter.
    ///
    /// # Errors
    ///
    /// Returns [`RegistryError::NotFound`] when no adapter is registered
    /// under `adapter_id`.
    pub async fn validate_config(
        &self,
        adapter_id: &str,
        config: &Value,
    ) -> Result<ValidationResult, RegistryError> {
        let adapter = self
            .adapters
            .get(adapter_id)
            .ok_or_else(|| RegistryError::NotFound(adapter_id.to_string()))?;
        Ok(adapter.validate(config).await)
    }
}
