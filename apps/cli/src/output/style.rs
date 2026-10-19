//! The five status tones of spec 0012 as terminal colors, and which tone
//! each state gets (spec 0008, *Output manusia*). A state is always written
//! as its wire value; the color only adds to it.

use anstyle::{AnsiColor, Style};
use stackly_protocol::daemon::{DaemonHealthStatus, SubsystemStatus};
use stackly_protocol::operation::{OperationState, OperationSummary};

/// A status tone (spec 0012).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// Things work: green.
    Positive,
    /// Look at it soon: yellow.
    Caution,
    /// Something broke: red.
    Critical,
    /// Work in progress: bright blue, since plain blue is hard to read on
    /// dark themes.
    Informative,
    /// Nothing to flag: no color.
    Neutral,
}

impl Tone {
    /// The terminal style of this tone.
    #[must_use]
    pub fn style(self) -> Style {
        let color = match self {
            Self::Positive => AnsiColor::Green,
            Self::Caution => AnsiColor::Yellow,
            Self::Critical => AnsiColor::Red,
            Self::Informative => AnsiColor::BrightBlue,
            Self::Neutral => return Style::new(),
        };
        Style::new().fg_color(Some(color.into()))
    }
}

/// The tone of a daemon health status.
#[must_use]
pub const fn health_tone(status: DaemonHealthStatus) -> Tone {
    match status {
        DaemonHealthStatus::Ready => Tone::Positive,
        DaemonHealthStatus::Starting
        | DaemonHealthStatus::Degraded
        | DaemonHealthStatus::Stopping
        | DaemonHealthStatus::Unrecognized => Tone::Caution,
    }
}

/// The tone of a subsystem status.
#[must_use]
pub const fn subsystem_tone(status: SubsystemStatus) -> Tone {
    match status {
        SubsystemStatus::Ok => Tone::Positive,
        SubsystemStatus::Degraded | SubsystemStatus::Unrecognized => Tone::Caution,
        SubsystemStatus::Unavailable => Tone::Critical,
        SubsystemStatus::NotBuilt => Tone::Neutral,
    }
}

/// The tone of an operation state; `succeeded` with partial items is
/// caution.
#[must_use]
pub fn operation_tone(operation: &OperationSummary) -> Tone {
    match operation.state {
        OperationState::Succeeded
            if operation
                .result
                .as_ref()
                .is_some_and(|result| !result.partial.is_empty()) =>
        {
            Tone::Caution
        }
        state => state_tone(state),
    }
}

/// The tone of an operation state on its own.
#[must_use]
pub const fn state_tone(state: OperationState) -> Tone {
    match state {
        OperationState::Running => Tone::Informative,
        OperationState::CancelRequested => Tone::Caution,
        OperationState::Failed | OperationState::NeedsRecovery => Tone::Critical,
        OperationState::Queued | OperationState::Succeeded | OperationState::Cancelled => {
            Tone::Neutral
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_tone_has_its_spec_0012_color() {
        let fg = |tone: Tone| tone.style().get_fg_color();
        assert_eq!(fg(Tone::Positive), Some(AnsiColor::Green.into()));
        assert_eq!(fg(Tone::Caution), Some(AnsiColor::Yellow.into()));
        assert_eq!(fg(Tone::Critical), Some(AnsiColor::Red.into()));
        assert_eq!(fg(Tone::Informative), Some(AnsiColor::BrightBlue.into()));
        assert_eq!(Tone::Neutral.style(), Style::new());
    }

    #[test]
    fn states_get_the_tones_of_the_spec_table() {
        let tones: Vec<_> = OperationState::ALL
            .iter()
            .map(|state| (*state, state_tone(*state)))
            .collect();
        assert_eq!(
            tones,
            [
                (OperationState::Queued, Tone::Neutral),
                (OperationState::Running, Tone::Informative),
                (OperationState::CancelRequested, Tone::Caution),
                (OperationState::Succeeded, Tone::Neutral),
                (OperationState::Failed, Tone::Critical),
                (OperationState::Cancelled, Tone::Neutral),
                (OperationState::NeedsRecovery, Tone::Critical),
            ]
        );
        assert_eq!(health_tone(DaemonHealthStatus::Ready), Tone::Positive);
        assert_eq!(health_tone(DaemonHealthStatus::Unrecognized), Tone::Caution);
        assert_eq!(subsystem_tone(SubsystemStatus::Unavailable), Tone::Critical);
        assert_eq!(subsystem_tone(SubsystemStatus::NotBuilt), Tone::Neutral);
        assert_eq!(subsystem_tone(SubsystemStatus::Unrecognized), Tone::Caution);
    }
}
