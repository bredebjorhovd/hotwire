//! Profile model and validation boundary.
//!
//! Profiles are the human-readable, versioned documents that bind physical
//! keys to semantic actions. Imported profiles must be validated against the
//! current schema before they are activated. This crate owns the canonical
//! Rust model and the parse/validate entry points; the TypeScript mirror lives
//! in `packages/schema`.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use hotwire_core::Trigger;

/// The only profile format version Hotwire v0.1 understands.
pub const SCHEMA_VERSION: u32 = 1;

/// Errors produced while parsing or validating a profile.
#[derive(Debug, Error)]
pub enum ProfileError {
    /// The document was not valid YAML.
    #[error("invalid profile YAML: {0}")]
    InvalidYaml(#[from] serde_yaml::Error),
    /// The document was not valid JSON.
    #[error("invalid profile JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),
    /// The document declares a schema version this build cannot handle.
    #[error("unsupported schema version {0}; this build supports version {SCHEMA_VERSION}")]
    UnsupportedSchemaVersion(u32),
    /// The document parsed but failed semantic validation.
    #[error("profile validation failed: {0}")]
    Invalid(String),
}

/// The kind of physical control surface a profile targets.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlSurface {
    /// A dedicated number pad.
    Numpad,
    /// The top function-key row.
    FunctionRow,
    /// A manually selected set of keys.
    Manual,
}

/// How a profile treats key events that match its bindings.
///
/// Mirrors the three modes of spec §9.3: assigned keys are captured and
/// consumed, captured only while a Hotwire layer key is held, or observed
/// without ever being consumed.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureMode {
    /// Assigned keys are consumed according to each binding's `consumeOriginal`.
    #[default]
    Capture,
    /// Keys only become Hotwire keys while the profile's layer key is held.
    ModifiedCapture,
    /// Observe/test without consuming; bindings still fire.
    Passthrough,
}

/// A single physical-key to semantic-action mapping.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Binding {
    pub id: String,
    pub physical_code: String,
    pub trigger: Trigger,
    pub action_id: String,
    pub adapter_id: String,
    #[serde(default = "default_config")]
    pub config: Value,
    pub consume_original: bool,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// When `true`, this binding only fires while the profile's layer key is
    /// held (spec §9.2 alternate-action model). Inert when the profile has no
    /// `layerKey`.
    #[serde(default)]
    pub layer: bool,
}

/// A validated, activate-ready profile.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub control_surface: ControlSurface,
    #[serde(default)]
    pub bindings: Vec<Binding>,
    pub layer_key: Option<String>,
    #[serde(default = "default_capture_mode")]
    pub capture_mode: CaptureMode,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

impl Profile {
    /// Validates the profile against the current schema.
    ///
    /// Mirrors the constraints in `packages/schema`: `schemaVersion` must be
    /// [`SCHEMA_VERSION`]; `id` and `name` must be non-empty; `layerKey`, when
    /// present, must be non-empty; every binding needs a non-empty `id`,
    /// `physicalCode`, `actionId`, and `adapterId`; and binding `config` must
    /// be a JSON object.
    ///
    /// # Errors
    ///
    /// Returns [`ProfileError::UnsupportedSchemaVersion`] when the profile
    /// declares a schema version other than [`SCHEMA_VERSION`], and
    /// [`ProfileError::Invalid`] when any required field is empty or a
    /// binding's `config` is not an object.
    pub fn validate(&self) -> Result<(), ProfileError> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(ProfileError::UnsupportedSchemaVersion(self.schema_version));
        }
        if self.id.is_empty() {
            return Err(ProfileError::Invalid("profile id must not be empty".into()));
        }
        if self.name.is_empty() {
            return Err(ProfileError::Invalid(
                "profile name must not be empty".into(),
            ));
        }
        if self.layer_key.as_ref().is_some_and(String::is_empty) {
            return Err(ProfileError::Invalid(
                "layerKey must not be empty when present".into(),
            ));
        }
        for (index, binding) in self.bindings.iter().enumerate() {
            if binding.id.is_empty() {
                return Err(ProfileError::Invalid(format!(
                    "binding at index {index} has an empty id"
                )));
            }
            for (field, value) in [
                ("physicalCode", binding.physical_code.as_str()),
                ("actionId", binding.action_id.as_str()),
                ("adapterId", binding.adapter_id.as_str()),
            ] {
                if value.is_empty() {
                    return Err(ProfileError::Invalid(format!(
                        "binding {} has an empty {field}",
                        binding.id
                    )));
                }
            }
            if !binding.config.is_object() {
                return Err(ProfileError::Invalid(format!(
                    "binding {} config must be an object",
                    binding.id
                )));
            }
        }
        Ok(())
    }
}

/// Parses and validates a profile from its YAML representation.
///
/// # Errors
///
/// Returns [`ProfileError::InvalidYaml`] on a malformed document, and the
/// same errors as [`Profile::validate`] on semantic problems.
pub fn parse_yaml(input: &str) -> Result<Profile, ProfileError> {
    let profile: Profile = serde_yaml::from_str(input)?;
    profile.validate()?;
    Ok(profile)
}

/// Parses and validates a profile from its JSON representation.
///
/// # Errors
///
/// Returns [`ProfileError::InvalidJson`] on a malformed document, and the
/// same errors as [`Profile::validate`] on semantic problems.
pub fn parse_json(input: &str) -> Result<Profile, ProfileError> {
    let profile: Profile = serde_json::from_str(input)?;
    profile.validate()?;
    Ok(profile)
}

/// Serializes a profile to readable, shareable YAML.
///
/// # Errors
///
/// Returns [`ProfileError::InvalidYaml`] if the profile cannot be rendered to
/// YAML.
pub fn export_yaml(profile: &Profile) -> Result<String, ProfileError> {
    Ok(serde_yaml::to_string(profile)?)
}

/// Serializes a profile to JSON.
///
/// # Errors
///
/// Returns [`ProfileError::InvalidJson`] if the profile cannot be rendered to
/// JSON.
pub fn export_json(profile: &Profile) -> Result<String, ProfileError> {
    Ok(serde_json::to_string(profile)?)
}

fn default_config() -> Value {
    Value::Object(serde_json::Map::new())
}

const fn default_enabled() -> bool {
    true
}

const fn default_capture_mode() -> CaptureMode {
    CaptureMode::Capture
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE_YAML: &str = r#"
schemaVersion: 1
id: brede-ai-numpad
name: Brede AI Numpad
controlSurface: numpad
enabled: true
bindings:
  - id: open-herdr
    physicalCode: Numpad5
    trigger: press
    actionId: app.open_or_focus
    adapterId: herdr
    consumeOriginal: true
    config:
      whenFocused: new_task
  - id: voice-input
    physicalCode: Numpad0
    trigger: hold
    actionId: voice.input
    adapterId: papegoye
    consumeOriginal: true
    config:
      shortcut: "fn+space"
"#;

    #[test]
    fn parses_the_example_profile() {
        let profile = parse_yaml(EXAMPLE_YAML).expect("example profile should parse");

        assert_eq!(profile.schema_version, SCHEMA_VERSION);
        assert_eq!(profile.control_surface, ControlSurface::Numpad);
        assert_eq!(profile.capture_mode, CaptureMode::Capture);
        assert_eq!(profile.bindings.len(), 2);
        assert_eq!(profile.bindings[0].adapter_id, "herdr");
        assert!(!profile.bindings[0].layer);
        assert_eq!(profile.bindings[1].trigger, Trigger::Hold);
    }

    #[test]
    fn omitted_layer_and_capture_mode_default_safely() {
        let profile = parse_yaml(EXAMPLE_YAML).expect("example profile should parse");

        assert_eq!(profile.capture_mode, CaptureMode::Capture);
        for binding in &profile.bindings {
            assert!(!binding.layer);
        }
    }

    #[test]
    fn capture_modes_parse_and_round_trip() {
        // Table-driven: every capture mode must parse and survive an export.
        let cases: &[(&str, CaptureMode)] = &[
            ("capture", CaptureMode::Capture),
            ("modified_capture", CaptureMode::ModifiedCapture),
            ("passthrough", CaptureMode::Passthrough),
        ];

        for (yaml_value, expected) in cases {
            let profile = parse_yaml(&format!(
                "schemaVersion: 1\nid: p\nname: P\ncontrolSurface: numpad\ncaptureMode: {yaml_value}\nbindings: []"
            ))
            .expect("profile should parse");
            assert_eq!(profile.capture_mode, *expected);

            let exported = export_yaml(&profile).expect("profile should export");
            let restored = parse_yaml(&exported).expect("exported profile should re-parse");
            assert_eq!(restored.capture_mode, *expected);
        }
    }

    #[test]
    fn layer_bindings_parse_and_round_trip() {
        let profile = parse_yaml(
            "schemaVersion: 1\nid: p\nname: P\ncontrolSurface: numpad\nlayerKey: NumLock\nbindings:\n  - id: alternate\n    physicalCode: Numpad7\n    trigger: press\n    actionId: app.alternate\n    adapterId: herdr\n    consumeOriginal: true\n    layer: true\n",
        )
        .expect("profile should parse");

        assert_eq!(profile.layer_key.as_deref(), Some("NumLock"));
        assert!(profile.bindings[0].layer);

        let exported = export_yaml(&profile).expect("profile should export");
        let restored = parse_yaml(&exported).expect("exported profile should re-parse");
        assert_eq!(profile, restored);
    }

    #[test]
    fn rejects_unknown_schema_versions() {
        let unsupported = EXAMPLE_YAML.replace("schemaVersion: 1", "schemaVersion: 2");

        assert!(matches!(
            parse_yaml(&unsupported),
            Err(ProfileError::UnsupportedSchemaVersion(2))
        ));
    }

    #[test]
    fn rejects_bindings_without_an_action() {
        let broken = EXAMPLE_YAML.replace("actionId: app.open_or_focus", "unknownField: x");

        assert!(matches!(
            parse_yaml(&broken),
            Err(ProfileError::InvalidYaml(_))
        ));
    }

    #[test]
    fn json_round_trips_without_loss() {
        let profile = parse_yaml(EXAMPLE_YAML).expect("example profile should parse");
        let json = export_json(&profile).expect("profile should serialize");
        let restored = parse_json(&json).expect("profile should parse back");

        assert_eq!(profile, restored);
    }

    #[test]
    fn yaml_export_round_trips_without_loss() {
        let profile = parse_yaml(EXAMPLE_YAML).expect("example profile should parse");
        let exported = export_yaml(&profile).expect("profile should serialize");
        let restored = parse_yaml(&exported).expect("profile should parse back");

        assert_eq!(profile, restored);
    }

    #[test]
    fn validate_rejects_empty_and_non_object_fields() {
        use serde_json::json;

        type CaseMutator = fn(&mut Profile);

        // Mirrors the `packages/schema` constraints: every profile/binding
        // identifier must be non-empty and binding `config` must be an object.
        let cases: &[(&str, CaseMutator)] = &[
            ("empty profile id", |profile| profile.id.clear()),
            ("empty profile name", |profile| profile.name.clear()),
            ("empty layerKey", |profile| {
                profile.layer_key = Some(String::new());
            }),
            ("empty binding id", |profile| profile.bindings[0].id.clear()),
            ("empty binding physicalCode", |profile| {
                profile.bindings[0].physical_code.clear();
            }),
            ("empty binding actionId", |profile| {
                profile.bindings[0].action_id.clear();
            }),
            ("empty binding adapterId", |profile| {
                profile.bindings[0].adapter_id.clear();
            }),
            ("non-object binding config", |profile| {
                profile.bindings[0].config = json!("not-an-object");
            }),
        ];

        for (label, mutate) in cases {
            let mut profile = parse_yaml(EXAMPLE_YAML).expect("example profile should parse");
            mutate(&mut profile);

            assert!(
                matches!(profile.validate(), Err(ProfileError::Invalid(_))),
                "expected {label} to be rejected"
            );

            let json = serde_json::to_string(&profile).expect("profile should serialize");
            assert!(
                parse_yaml(&json).is_err(),
                "expected {label} to be rejected at parse time"
            );
        }
    }
}
