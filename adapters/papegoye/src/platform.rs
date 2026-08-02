//! Platform seam for the Papegøye adapter.
//!
//! The adapter reproduces Papegøye's push-to-talk shortcut as a true hold
//! (spec §13.5): every OS-level key press and release happens behind
//! [`PapegoyePlatform`], so the hold state machine is exercised in mocked
//! integration tests without touching a real keyboard.

use async_trait::async_trait;
use thiserror::Error;

/// Errors produced by [`PapegoyePlatform`] operations.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum PapegoyeError {
    /// A synthetic key event could not be posted.
    #[error("key injection failed: {0}")]
    Inject(String),
    /// A configured shortcut name does not resolve to any physical key.
    #[error("shortcut `{0}` does not resolve to a key")]
    ShortcutResolve(String),
}

/// Everything the Papegøye adapter needs to hold and release a shortcut.
#[async_trait]
pub trait PapegoyePlatform: Send + Sync {
    /// Resolves a canonical physical-code name to a keycode.
    fn resolve_key(&self, name: &str) -> Option<u16>;
    /// Resolves a modifier name (`"shift"`, `"fn"`, …) to a keycode.
    fn resolve_modifier(&self, name: &str) -> Option<u16>;
    /// Whether the Papegøye application is installed on this machine.
    ///
    /// The adapter never probes or touches the microphone; this is a pure
    /// app-presence check so detection stays explicit without any audio.
    fn app_available(&self) -> bool;
    /// Sends a synthetic key-down.
    async fn key_down(&self, keycode: u16) -> Result<(), PapegoyeError>;
    /// Sends a synthetic key-up.
    async fn key_up(&self, keycode: u16) -> Result<(), PapegoyeError>;
    /// Releases every key the platform currently holds (fail-open on shutdown).
    async fn release_all(&self) -> Vec<u16>;
}

/// Returns the default platform for the current OS.
///
/// macOS gets a real implementation backed by Quartz injection; other
/// platforms get a stub that resolves nothing so the crate still compiles and
/// reports failures explicitly.
#[must_use]
pub fn default_platform() -> std::sync::Arc<dyn PapegoyePlatform> {
    #[cfg(target_os = "macos")]
    {
        std::sync::Arc::new(macos::MacPapegoyePlatform::new())
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::sync::Arc::new(UnsupportedPapegoyePlatform)
    }
}

#[cfg(target_os = "macos")]
pub mod macos {
    use std::path::Path;

    use async_trait::async_trait;
    use hotwire_input_macos::{InjectError, MacEventInjector};

    use super::{PapegoyeError, PapegoyePlatform};

    /// Common install paths probed by [`PapegoyePlatform::app_available`].
    pub const PAPEGOYE_APP_PATHS: &[&str] = &[
        "/Applications/Papegøye.app",
        "/Applications/Papegoye.app",
        "/Applications/Papegøye Dictation.app",
        "/Applications/Papegoye Dictation.app",
    ];

    /// Resolves a modifier name to its macOS keycode (left-hand variant).
    #[must_use]
    pub fn resolve_modifier(name: &str) -> Option<u16> {
        match name {
            "shift" => Some(0x38),
            "control" | "ctrl" => Some(0x3B),
            "option" | "alt" => Some(0x3A),
            "command" | "cmd" => Some(0x37),
            "fn" => Some(0x3F),
            _ => None,
        }
    }

    /// A real Papegøye platform backed by Quartz keyboard injection.
    pub struct MacPapegoyePlatform {
        injector: MacEventInjector,
    }

    impl MacPapegoyePlatform {
        /// Creates a platform that tags injected events as Hotwire's own.
        #[must_use]
        pub fn new() -> Self {
            Self {
                injector: MacEventInjector::new(hotwire_input_macos::INJECTED_MARKER),
            }
        }
    }

    impl Default for MacPapegoyePlatform {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Drop for MacPapegoyePlatform {
        /// Fail-open on process exit: any key still held is released so a quit
        /// or crash cannot leave a logical push-to-talk key down (spec §15.5).
        fn drop(&mut self) {
            let _ = self.injector.release_all();
        }
    }

    #[async_trait]
    impl PapegoyePlatform for MacPapegoyePlatform {
        fn resolve_key(&self, name: &str) -> Option<u16> {
            hotwire_input_macos::from_physical_name(name)
        }

        fn resolve_modifier(&self, name: &str) -> Option<u16> {
            resolve_modifier(name)
        }

        fn app_available(&self) -> bool {
            PAPEGOYE_APP_PATHS
                .iter()
                .any(|path| Path::new(path).exists())
        }

        async fn key_down(&self, keycode: u16) -> Result<(), PapegoyeError> {
            self.injector
                .key_down(keycode)
                .map_err(|error| inject_error(&error))
        }

        async fn key_up(&self, keycode: u16) -> Result<(), PapegoyeError> {
            self.injector
                .key_up(keycode)
                .map_err(|error| inject_error(&error))
        }

        async fn release_all(&self) -> Vec<u16> {
            self.injector.release_all()
        }
    }

    fn inject_error(error: &InjectError) -> PapegoyeError {
        PapegoyeError::Inject(error.to_string())
    }
}

/// A stub platform used on operating systems without keyboard injection yet.
///
/// Nothing resolves and every synthetic event fails, so no key is ever
/// reported as held.
#[cfg(not(target_os = "macos"))]
pub struct UnsupportedPapegoyePlatform;

#[cfg(not(target_os = "macos"))]
#[async_trait]
impl PapegoyePlatform for UnsupportedPapegoyePlatform {
    fn resolve_key(&self, _name: &str) -> Option<u16> {
        None
    }

    fn resolve_modifier(&self, _name: &str) -> Option<u16> {
        None
    }

    fn app_available(&self) -> bool {
        false
    }

    async fn key_down(&self, keycode: u16) -> Result<(), PapegoyeError> {
        Err(PapegoyeError::Inject(format!(
            "key injection is unsupported on this platform (keycode {keycode})"
        )))
    }

    async fn key_up(&self, keycode: u16) -> Result<(), PapegoyeError> {
        Err(PapegoyeError::Inject(format!(
            "key injection is unsupported on this platform (keycode {keycode})"
        )))
    }

    async fn release_all(&self) -> Vec<u16> {
        Vec::new()
    }
}
