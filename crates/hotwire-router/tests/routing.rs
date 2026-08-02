//! Table-driven routing tests: consumeOriginal decisions, hold/double-press
//! state machines, layers, disabled bindings, passthrough, and
//! generated-event filtering.

mod common;

use hotwire_core::{KeyState, Trigger};
use hotwire_profile::{CaptureMode, Profile, ProfileError};
use hotwire_router::{BindingRouter, RouterConfig, RouterError};

use crate::common::{binding, event, profile, profile_with, DOUBLE_PRESS_NS};

fn router(bindings: Vec<hotwire_profile::Binding>) -> BindingRouter {
    BindingRouter::new(profile(None, bindings), RouterConfig::default())
        .expect("router should build")
}

#[allow(clippy::too_many_lines, clippy::struct_excessive_bools)]
#[test]
fn consume_decisions_follow_capture_mode_and_binding_flag() {
    struct Case {
        name: &'static str,
        capture_mode: CaptureMode,
        consume_original: bool,
        layer_key: Option<&'static str>,
        layer: bool,
        hold_layer: bool,
        expect_consume: bool,
        expect_fire: bool,
    }

    let cases: &[Case] = &[
        Case {
            name: "capture consumes by default",
            capture_mode: CaptureMode::Capture,
            consume_original: true,
            layer_key: None,
            layer: false,
            hold_layer: false,
            expect_consume: true,
            expect_fire: true,
        },
        Case {
            name: "capture respects consumeOriginal=false",
            capture_mode: CaptureMode::Capture,
            consume_original: false,
            layer_key: None,
            layer: false,
            hold_layer: false,
            expect_consume: false,
            expect_fire: true,
        },
        Case {
            name: "passthrough never consumes but still fires",
            capture_mode: CaptureMode::Passthrough,
            consume_original: true,
            layer_key: None,
            layer: false,
            hold_layer: false,
            expect_consume: false,
            expect_fire: true,
        },
        Case {
            name: "modified capture consumes while layer held",
            capture_mode: CaptureMode::ModifiedCapture,
            consume_original: true,
            layer_key: Some("NumLock"),
            layer: false,
            hold_layer: true,
            expect_consume: true,
            expect_fire: true,
        },
        Case {
            name: "modified capture is inert without the layer",
            capture_mode: CaptureMode::ModifiedCapture,
            consume_original: true,
            layer_key: Some("NumLock"),
            layer: false,
            hold_layer: false,
            expect_consume: false,
            expect_fire: false,
        },
        Case {
            name: "layer binding fires and consumes while layer held",
            capture_mode: CaptureMode::Capture,
            consume_original: true,
            layer_key: Some("NumLock"),
            layer: true,
            hold_layer: true,
            expect_consume: true,
            expect_fire: true,
        },
        Case {
            name: "layer binding is inert without the layer",
            capture_mode: CaptureMode::Capture,
            consume_original: true,
            layer_key: Some("NumLock"),
            layer: true,
            hold_layer: false,
            expect_consume: false,
            expect_fire: false,
        },
        Case {
            name: "layer flag is inert when the profile has no layer key",
            capture_mode: CaptureMode::Capture,
            consume_original: true,
            layer_key: None,
            layer: true,
            hold_layer: false,
            expect_consume: true,
            expect_fire: true,
        },
    ];

    for case in cases {
        let mut binding = binding(
            "b",
            "Numpad5",
            Trigger::Press,
            "app.x",
            case.consume_original,
        );
        binding.layer = case.layer;
        let mut router = BindingRouter::new(
            profile_with(case.capture_mode, case.layer_key, vec![binding]),
            RouterConfig::default(),
        )
        .expect("router should build");

        if case.hold_layer {
            let _ = router.on_event(&event(0, "NumLock", KeyState::Down));
        }
        let outcome = router.on_event(&event(1, "Numpad5", KeyState::Down));

        assert_eq!(
            outcome.consume_original, case.expect_consume,
            "{}: unexpected consume decision",
            case.name
        );
        assert_eq!(
            outcome.invocations.len(),
            usize::from(case.expect_fire),
            "{}: unexpected fire count",
            case.name
        );
    }
}

#[test]
fn disabled_bindings_never_fire_or_consume() {
    struct Case {
        name: &'static str,
        enabled: bool,
        expect_fire: bool,
    }

    let cases: &[Case] = &[
        Case {
            name: "enabled binding fires",
            enabled: true,
            expect_fire: true,
        },
        Case {
            name: "disabled binding stays silent",
            enabled: false,
            expect_fire: false,
        },
    ];

    for case in cases {
        let mut case_binding = binding("b", "Numpad5", Trigger::Press, "app.x", true);
        case_binding.enabled = case.enabled;
        // Keep at least one enabled binding so the router builds; the case
        // binding lives on its own key.
        let anchor = binding("anchor", "Numpad6", Trigger::Press, "app.anchor", true);
        let mut router = router(vec![anchor, case_binding]);

        let outcome = router.on_event(&event(0, "Numpad5", KeyState::Down));

        assert_eq!(
            outcome.invocations.len(),
            usize::from(case.expect_fire),
            "{}: unexpected fire count",
            case.name
        );
        assert_eq!(
            outcome.consume_original, case.expect_fire,
            "{}: consume must track fire",
            case.name
        );
    }
}

#[test]
fn hold_fires_once_without_repeats_and_releases_exactly_once() {
    let mut router = router(vec![binding(
        "voice",
        "Numpad0",
        Trigger::Hold,
        "voice.input",
        true,
    )]);

    let down = router.on_event(&event(0, "Numpad0", KeyState::Down));
    assert_eq!(down.invocations.len(), 1);
    assert!(down.consume_original);

    let mut repeat = event(100, "Numpad0", KeyState::Down);
    repeat.is_repeat = true;
    let repeat_outcome = router.on_event(&repeat);
    assert!(repeat_outcome.invocations.is_empty());
    assert!(!repeat_outcome.consume_original);

    let held_down = router.on_event(&event(200, "Numpad0", KeyState::Down));
    assert!(held_down.invocations.is_empty());
    assert!(
        held_down.consume_original,
        "consuming stays armed while held"
    );

    let up = router.on_event(&event(1_000, "Numpad0", KeyState::Up));
    assert_eq!(up.releases.len(), 1);
    assert_eq!(up.releases[0].action_id, "voice.input");
    assert!(
        up.consume_original,
        "the key-up of a consumed hold is consumed"
    );

    let second_up = router.on_event(&event(1_001, "Numpad0", KeyState::Up));
    assert!(second_up.releases.is_empty());
}

#[test]
fn double_press_fires_only_on_a_fast_second_press() {
    struct Case {
        name: &'static str,
        timing: &'static [(u64, KeyState)],
        expect_fires: bool,
        expect_first_consume: bool,
    }

    let cases: &[Case] = &[
        Case {
            name: "two quick presses fire on the second down",
            timing: &[
                (0, KeyState::Down),
                (50_000_000, KeyState::Up),
                (150_000_000, KeyState::Down),
            ],
            expect_fires: true,
            expect_first_consume: true,
        },
        Case {
            name: "a single press never fires",
            timing: &[(0, KeyState::Down), (50_000_000, KeyState::Up)],
            expect_fires: false,
            expect_first_consume: true,
        },
        Case {
            name: "a slow second press starts over instead of firing",
            timing: &[
                (0, KeyState::Down),
                (50_000_000, KeyState::Up),
                (400_000_000, KeyState::Down),
            ],
            expect_fires: false,
            expect_first_consume: true,
        },
    ];

    for case in cases {
        let mut router = router(vec![binding(
            "dbl",
            "Numpad0",
            Trigger::DoublePress,
            "app.double",
            true,
        )]);

        let mut total_fires = 0;
        let mut first_consume = None;
        for (ns, state) in case.timing {
            let outcome = router.on_event(&event(*ns, "Numpad0", *state));
            total_fires += outcome.invocations.len();
            if *state == KeyState::Down && first_consume.is_none() {
                first_consume = Some(outcome.consume_original);
            }
        }

        assert_eq!(
            total_fires,
            usize::from(case.expect_fires),
            "{}: unexpected fire count",
            case.name
        );
        assert_eq!(
            first_consume,
            Some(case.expect_first_consume),
            "{}: first press consume must be handled",
            case.name
        );
    }
}

#[allow(clippy::too_many_lines)]
#[test]
fn double_press_consumption_follows_capture_mode_and_binding_flag() {
    struct Step {
        state: KeyState,
        at_ns: u64,
        consume: bool,
        fires: usize,
    }
    struct Case {
        name: &'static str,
        capture_mode: CaptureMode,
        consume_original: bool,
        layer_key: Option<&'static str>,
        hold_layer: bool,
        expect_fires: usize,
        steps: &'static [Step],
    }

    const QUICK: &[Step] = &[
        Step {
            state: KeyState::Down,
            at_ns: 0,
            consume: true,
            fires: 0,
        },
        Step {
            state: KeyState::Up,
            at_ns: 50_000_000,
            consume: true,
            fires: 0,
        },
        Step {
            state: KeyState::Down,
            at_ns: 150_000_000,
            consume: true,
            fires: 1,
        },
        Step {
            state: KeyState::Up,
            at_ns: 200_000_000,
            consume: true,
            fires: 0,
        },
    ];
    const UNCONSUMED: &[Step] = &[
        Step {
            state: KeyState::Down,
            at_ns: 0,
            consume: false,
            fires: 0,
        },
        Step {
            state: KeyState::Up,
            at_ns: 50_000_000,
            consume: false,
            fires: 0,
        },
        Step {
            state: KeyState::Down,
            at_ns: 150_000_000,
            consume: false,
            fires: 1,
        },
        Step {
            state: KeyState::Up,
            at_ns: 200_000_000,
            consume: false,
            fires: 0,
        },
    ];
    const INERT: &[Step] = &[
        Step {
            state: KeyState::Down,
            at_ns: 0,
            consume: false,
            fires: 0,
        },
        Step {
            state: KeyState::Up,
            at_ns: 50_000_000,
            consume: false,
            fires: 0,
        },
        Step {
            state: KeyState::Down,
            at_ns: 150_000_000,
            consume: false,
            fires: 0,
        },
        Step {
            state: KeyState::Up,
            at_ns: 200_000_000,
            consume: false,
            fires: 0,
        },
    ];

    let cases: &[Case] = &[
        Case {
            name: "capture consumes the armed first press through completion",
            capture_mode: CaptureMode::Capture,
            consume_original: true,
            layer_key: None,
            hold_layer: false,
            expect_fires: 1,
            steps: QUICK,
        },
        Case {
            name: "passthrough observes without consuming",
            capture_mode: CaptureMode::Passthrough,
            consume_original: true,
            layer_key: None,
            hold_layer: false,
            expect_fires: 1,
            steps: UNCONSUMED,
        },
        Case {
            name: "modified capture consumes the armed first press while the layer is held",
            capture_mode: CaptureMode::ModifiedCapture,
            consume_original: true,
            layer_key: Some("NumLock"),
            hold_layer: true,
            expect_fires: 1,
            steps: QUICK,
        },
        Case {
            name: "modified capture is inert without the layer",
            capture_mode: CaptureMode::ModifiedCapture,
            consume_original: true,
            layer_key: Some("NumLock"),
            hold_layer: false,
            expect_fires: 0,
            steps: INERT,
        },
        Case {
            name: "consumeOriginal=false passes the first press through",
            capture_mode: CaptureMode::Capture,
            consume_original: false,
            layer_key: None,
            hold_layer: false,
            expect_fires: 1,
            steps: UNCONSUMED,
        },
    ];

    for case in cases {
        let dp = binding(
            "dbl",
            "Numpad0",
            Trigger::DoublePress,
            "app.double",
            case.consume_original,
        );
        let mut router = BindingRouter::new(
            profile_with(case.capture_mode, case.layer_key, vec![dp]),
            RouterConfig::default(),
        )
        .expect("router should build");

        if case.hold_layer {
            let _ = router.on_event(&event(0, "NumLock", KeyState::Down));
        }

        let mut total_fires = 0;
        for step in case.steps {
            let outcome = router.on_event(&event(step.at_ns, "Numpad0", step.state));
            total_fires += outcome.invocations.len();
            assert_eq!(
                outcome.invocations.len(),
                step.fires,
                "{} at t={} {:?}: unexpected fire count",
                case.name,
                step.at_ns,
                step.state
            );
            assert_eq!(
                outcome.consume_original, step.consume,
                "{} at t={} {:?}: unexpected consume decision",
                case.name, step.at_ns, step.state
            );
        }
        assert_eq!(
            total_fires, case.expect_fires,
            "{}: unexpected total fire count",
            case.name
        );
    }
}

#[test]
fn double_press_expiry_consumes_the_armed_first_press() {
    let mut router = router(vec![binding(
        "dbl",
        "Numpad0",
        Trigger::DoublePress,
        "app.double",
        true,
    )]);

    let first = router.on_event(&event(0, "Numpad0", KeyState::Down));
    assert!(first.consume_original, "armed first press is consumed");

    let first_up = router.on_event(&event(50_000_000, "Numpad0", KeyState::Up));
    assert!(
        first_up.consume_original,
        "the first key-up of an armed double press is consumed"
    );

    router.on_tick(DOUBLE_PRESS_NS + 1);

    let fresh = router.on_event(&event(400_000_000, "Numpad0", KeyState::Down));
    assert!(
        fresh.invocations.is_empty(),
        "an expired wait must not fire on the next press"
    );
    assert!(
        fresh.consume_original,
        "the next lone press re-arms and stays consumed"
    );
}

#[test]
fn double_press_window_expires_when_time_passes_without_input() {
    let mut router = router(vec![binding(
        "dbl",
        "Numpad0",
        Trigger::DoublePress,
        "app.double",
        true,
    )]);

    let _ = router.on_event(&event(0, "Numpad0", KeyState::Down));
    let _ = router.on_event(&event(50, "Numpad0", KeyState::Up));
    router.on_tick(DOUBLE_PRESS_NS + 1);

    let late = router.on_event(&event(DOUBLE_PRESS_NS + 10, "Numpad0", KeyState::Down));
    assert!(
        late.invocations.is_empty(),
        "an expired wait must not fire on the next press"
    );

    let _ = router.on_event(&event(DOUBLE_PRESS_NS + 20, "Numpad0", KeyState::Up));
    let inside = router.on_event(&event(DOUBLE_PRESS_NS + 21, "Numpad0", KeyState::Down));
    assert_eq!(inside.invocations.len(), 1);
}

#[test]
fn layer_switches_alternate_action_on_the_same_key() {
    struct Case {
        name: &'static str,
        hold_layer: bool,
        expected_action: &'static str,
    }

    let cases: &[Case] = &[
        Case {
            name: "without the layer the normal action fires",
            hold_layer: false,
            expected_action: "app.normal",
        },
        Case {
            name: "with the layer the alternate action fires",
            hold_layer: true,
            expected_action: "app.alternate",
        },
    ];

    for case in cases {
        let mut normal = binding("normal", "Numpad7", Trigger::Press, "app.normal", true);
        normal.layer = false;
        let mut alternate = binding(
            "alternate",
            "Numpad7",
            Trigger::Press,
            "app.alternate",
            true,
        );
        alternate.layer = true;

        let mut router = BindingRouter::new(
            profile(Some("NumLock"), vec![normal, alternate]),
            RouterConfig::default(),
        )
        .expect("router should build");

        if case.hold_layer {
            let _ = router.on_event(&event(0, "NumLock", KeyState::Down));
        }
        let outcome = router.on_event(&event(1, "Numpad7", KeyState::Down));

        assert_eq!(
            outcome.invocations.len(),
            1,
            "{}: exactly one action should fire",
            case.name
        );
        assert_eq!(
            outcome.invocations[0].action_id, case.expected_action,
            "{}: wrong action fired",
            case.name
        );
    }
}

#[test]
fn generated_and_repeat_events_are_filtered() {
    struct Case {
        name: &'static str,
        is_injected: bool,
        is_repeat: bool,
    }

    let cases: &[Case] = &[
        Case {
            name: "injected events are ignored",
            is_injected: true,
            is_repeat: false,
        },
        Case {
            name: "repeat events are ignored",
            is_injected: false,
            is_repeat: true,
        },
    ];

    for case in cases {
        let mut router = router(vec![binding("b", "Numpad5", Trigger::Press, "app.x", true)]);

        let mut key = event(0, "Numpad5", KeyState::Down);
        key.is_injected = case.is_injected;
        key.is_repeat = case.is_repeat;

        let outcome = router.on_event(&key);

        assert!(
            outcome.invocations.is_empty(),
            "{}: no action should fire",
            case.name
        );
        assert!(
            !outcome.consume_original,
            "{}: nothing to consume",
            case.name
        );
        assert!(outcome.receipts.is_empty());
    }
}

#[test]
fn unassigned_keys_pass_through() {
    let mut router = router(vec![binding("b", "Numpad5", Trigger::Press, "app.x", true)]);

    let outcome = router.on_event(&event(0, "KeyA", KeyState::Down));
    assert!(outcome.invocations.is_empty());
    assert!(!outcome.consume_original);
}

#[test]
fn construction_rejects_invalid_and_empty_profiles() {
    let invalid = Profile {
        schema_version: 2,
        ..profile(
            None,
            vec![binding("b", "Numpad5", Trigger::Press, "app.x", true)],
        )
    };
    assert!(matches!(
        BindingRouter::new(invalid, RouterConfig::default()),
        Err(RouterError::InvalidProfile(
            ProfileError::UnsupportedSchemaVersion(2)
        ))
    ));

    let no_bindings = profile(None, Vec::new());
    assert!(matches!(
        BindingRouter::new(no_bindings, RouterConfig::default()),
        Err(RouterError::EmptyProfile)
    ));

    let disabled = Profile {
        enabled: false,
        ..profile(
            None,
            vec![binding("b", "Numpad5", Trigger::Press, "app.x", true)],
        )
    };
    assert!(matches!(
        BindingRouter::new(disabled, RouterConfig::default()),
        Err(RouterError::ProfileDisabled)
    ));

    let all_disabled = profile(
        None,
        vec![{
            let mut b = binding("b", "Numpad5", Trigger::Press, "app.x", true);
            b.enabled = false;
            b
        }],
    );
    assert!(matches!(
        BindingRouter::new(all_disabled, RouterConfig::default()),
        Err(RouterError::EmptyProfile)
    ));
}

#[test]
fn on_tick_clears_state_without_side_effects() {
    let mut router = router(vec![binding("b", "Numpad5", Trigger::Press, "app.x", true)]);
    router.on_tick(1_000_000);
    assert!(!router.layer_held());
}
