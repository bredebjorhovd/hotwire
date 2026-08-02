//! Execution runtime.
//!
//! [`HotwireRuntime`] drives a [`BindingRouter`] and an [`AdapterRegistry`]
//! together: it turns routing decisions into adapter calls, tracks in-flight
//! executions so they can be cancelled, and publishes every [`ActionReceipt`]
//! to subscribers for the live board and logs.
//!
//! The event path is async because adapters are async. Callers must feed
//! events from a task — never from a native input callback, which must only
//! normalize and enqueue.

use std::collections::HashMap;

use hotwire_adapter_sdk::{ActionInvocation, AdapterError};
use hotwire_core::{ActionReceipt, ActionStatus, PhysicalKeyEvent};
use hotwire_profile::Profile;
use thiserror::Error;
use tokio::sync::broadcast;

use crate::{
    AdapterRegistry, BindingRouter, ReleaseRequest, RouteOutcome, RouterConfig, RouterError,
};

/// Errors produced while driving the runtime.
#[derive(Debug, Error)]
pub enum RuntimeError {
    /// The runtime was asked to cancel an execution it is not tracking.
    #[error("no execution is active for `{0}`")]
    UnknownExecution(String),
    /// The adapter rejected the operation.
    #[error("adapter operation failed: {0}")]
    Adapter(#[from] AdapterError),
}

/// What the runtime needs to cancel or finish a tracked execution.
#[derive(Clone, Debug)]
struct ActiveExecution {
    adapter_id: String,
    profile_id: String,
    binding_id: String,
    physical_code: String,
    action_id: String,
}

/// Composes routing, dispatch, and receipt publication.
pub struct HotwireRuntime {
    router: BindingRouter,
    registry: AdapterRegistry,
    receipts: broadcast::Sender<ActionReceipt>,
    active: HashMap<String, ActiveExecution>,
    paused: bool,
    stopped: bool,
}

impl HotwireRuntime {
    /// Builds a runtime around a validated profile.
    ///
    /// # Errors
    ///
    /// Returns [`RouterError`] when the profile fails validation or has no
    /// enabled bindings.
    pub fn new(profile: Profile, config: RouterConfig) -> Result<Self, RouterError> {
        let (receipts, _) = broadcast::channel(64);
        Ok(Self {
            router: BindingRouter::new(profile, config)?,
            registry: AdapterRegistry::new(),
            receipts,
            active: HashMap::new(),
            paused: false,
            stopped: false,
        })
    }

    /// Returns the adapter registry, for looking adapters up.
    #[must_use]
    pub fn registry(&self) -> &AdapterRegistry {
        &self.registry
    }

    /// Returns the adapter registry mutably, for registering adapters.
    #[must_use]
    pub fn registry_mut(&mut self) -> &mut AdapterRegistry {
        &mut self.registry
    }

    /// Returns the underlying router, e.g. to drive time without input.
    #[must_use]
    pub fn router(&mut self) -> &mut BindingRouter {
        &mut self.router
    }

    /// Subscribes to every [`ActionReceipt`] the runtime publishes.
    ///
    /// Receipts are broadcast: any number of subscribers (live board, logs,
    /// diagnostics) can listen independently.
    #[must_use]
    pub fn subscribe_receipts(&self) -> broadcast::Receiver<ActionReceipt> {
        self.receipts.subscribe()
    }

    /// Feeds one key event through routing and dispatches anything that fired.
    ///
    /// A paused or stopped runtime ignores events entirely (fail-open): no
    /// action fires and nothing is consumed.
    pub async fn on_event(&mut self, event: &PhysicalKeyEvent) -> RouteOutcome {
        if self.paused || self.stopped {
            return RouteOutcome::default();
        }
        let outcome = self.router.on_event(event);

        for receipt in &outcome.receipts {
            if receipt.status == ActionStatus::Started {
                self.active.insert(
                    receipt.execution_id.clone(),
                    ActiveExecution {
                        adapter_id: receipt.adapter_id.clone(),
                        profile_id: receipt.profile_id.clone(),
                        binding_id: receipt.binding_id.clone(),
                        physical_code: receipt.physical_code.clone(),
                        action_id: receipt.action_id.clone(),
                    },
                );
            }
            self.publish(receipt.clone());
        }

        for invocation in &outcome.invocations {
            self.dispatch_start(invocation.clone()).await;
        }
        for release in &outcome.releases {
            self.dispatch_release(release.clone()).await;
        }

        outcome
    }

    /// Cancels a tracked in-flight execution and publishes a `Cancelled`
    /// receipt.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::UnknownExecution`] when the runtime is not
    /// tracking `execution_id`, and [`RuntimeError::Adapter`] when the owning
    /// adapter rejects the cancellation.
    pub async fn cancel(&mut self, execution_id: &str) -> Result<(), RuntimeError> {
        let Some(info) = self.active.get(execution_id).cloned() else {
            return Err(RuntimeError::UnknownExecution(execution_id.to_string()));
        };
        self.registry.cancel(&info.adapter_id, execution_id).await?;
        self.finish(execution_id, ActionStatus::Cancelled, None);
        Ok(())
    }

    /// Cancels every in-flight execution and returns how many were cancelled.
    ///
    /// Used when a profile is deactivated or the app is shutting down, so no
    /// key stays logically held down.
    #[must_use]
    pub async fn cancel_active(&mut self) -> usize {
        let ids: Vec<String> = self.active.keys().cloned().collect();
        let mut cancelled = 0;
        for id in &ids {
            if self.cancel(id).await.is_ok() {
                cancelled += 1;
            }
        }
        cancelled
    }

    /// Returns whether the runtime is paused.
    #[must_use]
    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Returns whether the runtime has been shut down.
    #[must_use]
    pub fn is_stopped(&self) -> bool {
        self.stopped
    }

    /// Pauses the runtime: no further events route, every in-flight execution
    /// is cancelled so no key stays logically held down, and the router's
    /// interaction state is reset so the next press after resume starts fresh.
    ///
    /// Returns how many executions were cancelled.
    #[must_use]
    pub async fn pause(&mut self) -> usize {
        self.paused = true;
        self.router.reset();
        self.cancel_active().await
    }

    /// Resumes the runtime after a pause.
    pub fn resume(&mut self) {
        self.paused = false;
    }

    /// Shuts the runtime down: pauses it, cancels every in-flight execution,
    /// and permanently stops it from accepting events.
    ///
    /// This is the clean-shutdown surface (spec §15.5): after it returns, no
    /// action can fire and no key is left held. Returns how many executions
    /// were cancelled.
    #[must_use]
    pub async fn shutdown(&mut self) -> usize {
        self.stopped = true;
        self.pause().await
    }

    async fn dispatch_start(&mut self, invocation: ActionInvocation) {
        let execution_id = invocation.execution_id.clone();
        let result = self.registry.execute(&invocation).await;
        match result.status {
            ActionStatus::Succeeded | ActionStatus::Failed => {
                self.finish(&execution_id, result.status, result.message);
            }
            // Started keeps the execution tracked: a hold is released on
            // key-up and any running execution stays cancellable by id.
            ActionStatus::Started => {}
            ActionStatus::Cancelled => {
                self.finish(&execution_id, ActionStatus::Cancelled, result.message);
            }
        }
    }

    async fn dispatch_release(&mut self, release: ReleaseRequest) {
        let result = self
            .registry
            .release(&release.adapter_id, &release.execution_id)
            .await;
        let (status, message) = match result {
            Ok(()) => (ActionStatus::Succeeded, None),
            Err(error) => (ActionStatus::Failed, Some(error.to_string())),
        };
        self.finish(&release.execution_id, status, message);
    }

    fn finish(&mut self, execution_id: &str, status: ActionStatus, message: Option<String>) {
        if let Some(info) = self.active.remove(execution_id) {
            self.publish(ActionReceipt {
                execution_id: execution_id.to_string(),
                profile_id: info.profile_id,
                binding_id: info.binding_id,
                physical_code: info.physical_code,
                action_id: info.action_id,
                adapter_id: info.adapter_id,
                status,
                message,
            });
        }
    }

    fn publish(&self, receipt: ActionReceipt) {
        let _ = self.receipts.send(receipt);
    }
}
