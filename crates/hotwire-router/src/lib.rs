//! Platform-neutral binding routing.
//!
//! This crate composes the platform-neutral boundaries into the runtime that
//! decides what happens when a normalized key event arrives:
//!
//! 1. [`BindingRouter`] matches a [`PhysicalKeyEvent`] against a validated
//!    [`Profile`], runs the press/hold/double-press state machines, applies
//!    layer and capture-mode gating, and decides whether the original key
//!    event is consumed.
//! 2. [`AdapterRegistry`] holds the registered [`Adapter`]s by id and is the
//!    only way the runtime reaches an adapter.
//! 3. [`HotwireRuntime`] wires the two together: it executes fired actions,
//!    ends and cancels hold interactions, and publishes [`ActionReceipt`]s
//!    for the live board.
//!
//! The router is deliberately pure: it never touches the OS and never awaits
//! an adapter. It only produces routing decisions. Dispatch happens in
//! [`HotwireRuntime`], which callers must drive from an async task — never
//! from a native input callback.

pub mod registry;
pub mod runtime;

use std::collections::HashMap;
use std::time::Duration;

use hotwire_core::{
    should_route, ActionReceipt, ActionStatus, KeyState, PhysicalKeyEvent, Trigger,
};
use hotwire_input::{TriggerDetector, TriggerEvent};
use hotwire_profile::{Binding, CaptureMode, Profile};
use thiserror::Error;

pub use hotwire_adapter_sdk::{ActionInvocation, ActiveApplication, ExecutionContext};

pub use registry::{AdapterRegistry, RegistryError};
pub use runtime::{HotwireRuntime, RuntimeError};

/// Tuning knobs for routing behavior.
#[derive(Clone, Copy, Debug)]
pub struct RouterConfig {
    /// Window within which a second press of a `double_press` binding counts.
    pub double_press_window: Duration,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            double_press_window: Duration::from_millis(250),
        }
    }
}

/// Errors produced while constructing or driving a [`BindingRouter`].
#[derive(Debug, Error)]
pub enum RouterError {
    /// The profile failed schema validation; routing refuses to start.
    #[error("cannot route an invalid profile: {0}")]
    InvalidProfile(#[from] hotwire_profile::ProfileError),
    /// The profile is disabled and must not produce actions.
    #[error("profile is disabled")]
    ProfileDisabled,
    /// The profile has no enabled bindings to route.
    #[error("profile has no enabled bindings to route")]
    EmptyProfile,
}

/// A hold interaction that ended because its key was released.
#[derive(Clone, Debug)]
pub struct ReleaseRequest {
    /// The execution the adapter started on key-down.
    pub execution_id: String,
    /// The adapter that owns the execution.
    pub adapter_id: String,
    pub profile_id: String,
    pub binding_id: String,
    pub physical_code: String,
    pub action_id: String,
}

/// What feeding one key event to the router produced.
#[derive(Clone, Debug, Default)]
pub struct RouteOutcome {
    /// Whether the original OS key event must be suppressed.
    pub consume_original: bool,
    /// Actions that fired on this event and must be started.
    pub invocations: Vec<ActionInvocation>,
    /// Hold interactions whose key was released and must be ended.
    pub releases: Vec<ReleaseRequest>,
    /// Receipts produced synchronously by this event (always `Started`).
    pub receipts: Vec<ActionReceipt>,
}

/// Matches normalized key events against a validated profile.
///
/// One detector runs per enabled binding; a physical code with several
/// bindings (different triggers, or a layer/normal pair) is disambiguated by
/// the layer key and profile order. The router tracks hold state per code so
/// a held key never re-fires and always releases exactly once.
#[derive(Debug)]
pub struct BindingRouter {
    profile: Profile,
    codes: HashMap<String, CodeState>,
    layer_held: bool,
    next_execution: u64,
}

#[derive(Debug)]
struct CodeState {
    detectors: Vec<BindingDetector>,
    active: Option<ActiveInteraction>,
    consuming: bool,
}

#[derive(Debug)]
struct BindingDetector {
    binding: Binding,
    detector: TriggerDetector,
}

#[derive(Debug)]
struct ActiveInteraction {
    execution_id: String,
    binding_id: String,
    adapter_id: String,
    action_id: String,
    is_hold: bool,
}

#[derive(Debug)]
struct Fire {
    binding: Binding,
    trigger: Trigger,
}

impl BindingRouter {
    /// Builds a router from a validated profile.
    ///
    /// A disabled profile is rejected so it can never produce actions;
    /// enabling a profile means constructing a router for it. Enabled
    /// bindings are pre-grouped per physical code; a binding whose `enabled`
    /// flag is `false` is never routed and never matches. Profiles without
    /// any enabled binding are rejected.
    ///
    /// # Errors
    ///
    /// Returns [`RouterError::InvalidProfile`] when the profile fails
    /// validation, [`RouterError::ProfileDisabled`] when the profile's
    /// `enabled` flag is `false`, and [`RouterError::EmptyProfile`] when it
    /// has no enabled bindings.
    pub fn new(profile: Profile, config: RouterConfig) -> Result<Self, RouterError> {
        profile.validate()?;
        if !profile.enabled {
            return Err(RouterError::ProfileDisabled);
        }

        let mut codes: HashMap<String, CodeState> = HashMap::new();
        for binding in &profile.bindings {
            if !binding.enabled {
                continue;
            }
            let code = codes
                .entry(binding.physical_code.clone())
                .or_insert_with(|| CodeState {
                    detectors: Vec::new(),
                    active: None,
                    consuming: false,
                });
            code.detectors.push(BindingDetector {
                binding: binding.clone(),
                detector: TriggerDetector::new(
                    binding.trigger,
                    u64::try_from(config.double_press_window.as_nanos()).unwrap_or(u64::MAX),
                ),
            });
        }
        if codes.is_empty() {
            return Err(RouterError::EmptyProfile);
        }

        Ok(Self {
            profile,
            codes,
            layer_held: false,
            next_execution: 0,
        })
    }

    /// Feeds one normalized key event and returns the routing outcome.
    ///
    /// Injected and repeat events are ignored up front (generated-event
    /// filtering), so a held key never re-fires. Unassigned keys pass through
    /// with `consume_original == false`.
    #[must_use]
    pub fn on_event(&mut self, event: &PhysicalKeyEvent) -> RouteOutcome {
        let mut outcome = RouteOutcome::default();
        if !should_route(event) {
            return outcome;
        }
        self.expire_stale_waits(event.timestamp_ns);
        handle_event(self, event, &mut outcome);
        outcome
    }

    /// Advances time to `now_ns`, expiring stale double-press waits.
    ///
    /// Expired waits never start an execution; they only clear state. The
    /// router does this implicitly on every event; call it directly to drive
    /// time that passes without input.
    pub fn on_tick(&mut self, now_ns: u64) {
        self.expire_stale_waits(now_ns);
    }

    /// Returns whether the profile's layer key is currently held.
    #[must_use]
    pub fn layer_held(&self) -> bool {
        self.layer_held
    }

    /// Resets all interaction state: the layer key, per-code active
    /// interactions, consumption, and every trigger detector.
    ///
    /// Recovery surfaces (pausing capture, shutting down) call this so a held
    /// or partially-completed interaction that the runtime cancelled does not
    /// linger and mis-route the next press after capture resumes.
    pub fn reset(&mut self) {
        self.layer_held = false;
        for code in self.codes.values_mut() {
            code.active = None;
            code.consuming = false;
            for detector in &mut code.detectors {
                detector.detector.reset();
            }
        }
    }

    fn expire_stale_waits(&mut self, now_ns: u64) {
        for code in self.codes.values_mut() {
            for detector in &mut code.detectors {
                let _ = detector.detector.on_tick(now_ns);
            }
        }
    }
}

fn handle_event(router: &mut BindingRouter, event: &PhysicalKeyEvent, outcome: &mut RouteOutcome) {
    if router.profile.layer_key.as_deref() == Some(event.physical_code.as_str()) {
        router.layer_held = event.state == KeyState::Down;
    }

    let Some(code) = router.codes.get_mut(&event.physical_code) else {
        return;
    };
    let layer_key_is_set = router.profile.layer_key.is_some();
    let layer_held = router.layer_held;
    let capture_mode = router.profile.capture_mode;

    match event.state {
        KeyState::Down => handle_down(
            &mut router.next_execution,
            &router.profile,
            event,
            code,
            outcome,
            layer_key_is_set,
            layer_held,
            capture_mode,
        ),
        KeyState::Up => handle_up(event, code, outcome, &router.profile.id),
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_down(
    next_execution: &mut u64,
    profile: &Profile,
    event: &PhysicalKeyEvent,
    code: &mut CodeState,
    outcome: &mut RouteOutcome,
    layer_key_is_set: bool,
    layer_held: bool,
    capture_mode: CaptureMode,
) {
    let fire = collect_fire(code, event, layer_key_is_set, layer_held, capture_mode);
    let Some(fire) = fire else {
        // A double-press binding that armed on this down (or was already
        // armed) keeps the key consumed, so its first press never leaks to
        // the OS while the second press is still expected. Passthrough and
        // inactive modified-capture profiles never consume here.
        let armed_consumes = code.detectors.iter().any(|detector| {
            detector.binding.trigger == Trigger::DoublePress
                && detector.detector.is_armed()
                && fire_allowed(
                    &detector.binding,
                    layer_key_is_set,
                    layer_held,
                    capture_mode,
                )
                && consume_allowed(&detector.binding, layer_held, capture_mode)
        });
        if armed_consumes {
            code.consuming = true;
            outcome.consume_original = true;
        } else {
            outcome.consume_original = code.consuming;
            if code.active.is_none() {
                code.consuming = false;
            }
        }
        return;
    };

    let consume = consume_allowed(&fire.binding, layer_held, capture_mode);
    *next_execution += 1;
    let execution_id = format!("exec-{next_execution}");
    let invocation = build_invocation(
        &profile.id,
        &fire.binding,
        fire.trigger,
        &execution_id,
        event,
    );

    code.consuming = consume;
    code.active = Some(ActiveInteraction {
        execution_id: execution_id.clone(),
        binding_id: fire.binding.id.clone(),
        adapter_id: fire.binding.adapter_id.clone(),
        action_id: fire.binding.action_id.clone(),
        is_hold: fire.trigger == Trigger::Hold,
    });

    outcome.consume_original = consume;
    outcome.invocations.push(invocation);
    outcome.receipts.push(ActionReceipt {
        execution_id,
        profile_id: profile.id.clone(),
        binding_id: fire.binding.id.clone(),
        physical_code: fire.binding.physical_code.clone(),
        action_id: fire.binding.action_id.clone(),
        adapter_id: fire.binding.adapter_id.clone(),
        status: ActionStatus::Started,
        message: None,
    });
}

fn handle_up(
    event: &PhysicalKeyEvent,
    code: &mut CodeState,
    outcome: &mut RouteOutcome,
    profile_id: &str,
) {
    for detector in &mut code.detectors {
        let _ = detector.detector.on_event(event);
    }

    outcome.consume_original = code.consuming;
    code.consuming = false;

    let Some(active) = code.active.take() else {
        return;
    };
    if active.is_hold {
        outcome.releases.push(ReleaseRequest {
            execution_id: active.execution_id,
            adapter_id: active.adapter_id,
            profile_id: profile_id.to_string(),
            binding_id: active.binding_id,
            physical_code: event.physical_code.clone(),
            action_id: active.action_id,
        });
    }
}

fn collect_fire(
    code: &mut CodeState,
    event: &PhysicalKeyEvent,
    layer_key_is_set: bool,
    layer_held: bool,
    capture_mode: CaptureMode,
) -> Option<Fire> {
    let mut layer_fire: Option<Fire> = None;
    let mut normal_fire: Option<Fire> = None;

    for detector in &mut code.detectors {
        for trigger_event in detector.detector.on_event(event) {
            let TriggerEvent::Down(trigger) = trigger_event else {
                continue;
            };
            if !fire_allowed(
                &detector.binding,
                layer_key_is_set,
                layer_held,
                capture_mode,
            ) {
                continue;
            }
            let fire = Fire {
                binding: detector.binding.clone(),
                trigger,
            };
            if layer_key_is_set && detector.binding.layer {
                if layer_fire.is_none() {
                    layer_fire = Some(fire);
                }
            } else if normal_fire.is_none() {
                normal_fire = Some(fire);
            }
        }
    }

    layer_fire.or(normal_fire)
}

/// Whether a binding may fire in the current layer/capture state.
fn fire_allowed(
    binding: &Binding,
    layer_key_is_set: bool,
    layer_held: bool,
    capture_mode: CaptureMode,
) -> bool {
    if capture_mode == CaptureMode::ModifiedCapture && !layer_held {
        return false;
    }
    if layer_key_is_set && binding.layer {
        return layer_held;
    }
    true
}

/// Whether the original key event should be suppressed for a firing binding.
fn consume_allowed(binding: &Binding, layer_held: bool, capture_mode: CaptureMode) -> bool {
    match capture_mode {
        CaptureMode::Passthrough => false,
        CaptureMode::ModifiedCapture => layer_held && binding.consume_original,
        CaptureMode::Capture => binding.consume_original,
    }
}

fn build_invocation(
    profile_id: &str,
    binding: &Binding,
    trigger: Trigger,
    execution_id: &str,
    event: &PhysicalKeyEvent,
) -> ActionInvocation {
    ActionInvocation {
        execution_id: execution_id.to_string(),
        action_id: binding.action_id.clone(),
        adapter_id: binding.adapter_id.clone(),
        profile_id: profile_id.to_string(),
        binding_id: binding.id.clone(),
        trigger,
        config: binding.config.clone(),
        context: ExecutionContext {
            active_application: None,
            cwd: None,
            profile_id: profile_id.to_string(),
            binding_id: binding.id.clone(),
            trigger,
            timestamp: event.timestamp_ns.to_string(),
        },
    }
}
