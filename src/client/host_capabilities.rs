//! Capability profile for the terminal hosting the rendered Herdr client.
//!
//! Configuration expresses which optional features the user wants. Runtime
//! protocol negotiation establishes which requested extensions are safe to
//! enable. Before that negotiation completes, Windows starts conservatively.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct HostTerminalCapabilities {
    pub(super) kitty_graphics: bool,
    pub(super) synchronized_output: bool,
}

impl HostTerminalCapabilities {
    pub(super) fn initial() -> Self {
        Self::initial_for_platform(cfg!(windows))
    }

    fn initial_for_platform(is_windows: bool) -> Self {
        if is_windows {
            Self {
                kitty_graphics: false,
                synchronized_output: false,
            }
        } else {
            // Preserve the existing Unix behavior. Its host probes and direct
            // graphics transport already provide the established compatibility
            // boundary for Unix terminal stacks.
            Self {
                kitty_graphics: true,
                synchronized_output: true,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::HostTerminalCapabilities;

    #[test]
    fn windows_starts_conservatively() {
        assert_eq!(
            HostTerminalCapabilities::initial_for_platform(true),
            HostTerminalCapabilities {
                kitty_graphics: false,
                synchronized_output: false,
            }
        );
    }

    #[test]
    fn unix_preserves_established_extensions() {
        assert_eq!(
            HostTerminalCapabilities::initial_for_platform(false),
            HostTerminalCapabilities {
                kitty_graphics: true,
                synchronized_output: true,
            }
        );
    }
}
