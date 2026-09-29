//! Background operation kinds (activation / update / store repair / generation switch):
//! titles and the ordered progress steps the monitor shows.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OpKind {
    Activation,
    Update,
    Repair,
    GenSwitch,
}

/// Ordered UI steps for activation / update. Last label is always "Finished" so the
/// final working phase is never shown as complete while it still runs.
const ACTIVATION_STEPS: &[&str] = &[
    "Start",
    "Write flake",
    "Build",
    "Save checkpoints",
    "Switch",
    "Finished",
];

const UPDATE_STEPS: &[&str] = &[
    "Start",
    "Reinitialize",
    "Write flake",
    "Update dependencies",
    "Migrate",
    "Finished",
];

impl OpKind {
    /// Kind from an op id prefix (`activation_…`, `update_…`, `repair_…`, `genswitch_…`).
    pub(crate) fn from_id(id: &str) -> Option<Self> {
        let (prefix, _) = id.split_once('_')?;
        match prefix {
            "activation" => Some(OpKind::Activation),
            "update" => Some(OpKind::Update),
            "repair" => Some(OpKind::Repair),
            "genswitch" => Some(OpKind::GenSwitch),
            _ => None,
        }
    }

    pub(crate) fn title(self) -> &'static str {
        match self {
            OpKind::Activation => "Activation",
            OpKind::Update => "Update",
            OpKind::Repair => "Nix store repair",
            OpKind::GenSwitch => "Generation switch",
        }
    }

    /// Progress steps; empty for kinds that only report a free-form phase.
    pub(crate) fn steps(self) -> &'static [&'static str] {
        match self {
            OpKind::Activation => ACTIVATION_STEPS,
            OpKind::Update => UPDATE_STEPS,
            OpKind::Repair | OpKind::GenSwitch => &[],
        }
    }

    /// Map the JSON `phase` string written by activate/update onto a step index.
    /// Terminal "complete*" phases land on the final "Finished" step.
    pub(crate) fn step_index(self, phase: &str) -> usize {
        match self {
            OpKind::Activation => match phase {
                "triggered" | "starting" => 0,
                "write-flake" | "write-flake-done" => 1,
                "toplevel-build" | "toplevel-built" => 2,
                "git-add" | "build-branch" | "build-commit" | "branches-created" | "amend-add"
                | "amend-commit" | "branch-failed" => 3,
                "pre-rebuild" | "rebuild-failed" => 4,
                "completed" | "completed-with-warnings" | "complete" => 5,
                _ => 0,
            },
            OpKind::Update => match phase {
                "triggered" | "starting" => 0,
                "flake init" | "post-init restore" => 1,
                "write-flake" => 2,
                "flake update" => 3,
                "migrate" => 4,
                "complete" => 5,
                _ => 0,
            },
            OpKind::Repair | OpKind::GenSwitch => 0,
        }
    }
}

/// Timestamp part of an op id (after the kind prefix), for the dialog subtitle.
pub(crate) fn id_timestamp(id: &str) -> &str {
    id.split_once('_').map(|(_, ts)| ts).unwrap_or(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_from_ids() {
        assert_eq!(
            OpKind::from_id("activation_20260928-1"),
            Some(OpKind::Activation)
        );
        assert_eq!(OpKind::from_id("update_x"), Some(OpKind::Update));
        assert_eq!(OpKind::from_id("repair_x"), Some(OpKind::Repair));
        assert_eq!(
            OpKind::from_id("genswitch_switch-3-x"),
            Some(OpKind::GenSwitch)
        );
        assert_eq!(OpKind::from_id("nope"), None);
        assert_eq!(id_timestamp("genswitch_switch-3-x"), "switch-3-x");
    }

    #[test]
    fn terminal_phase_is_last_step() {
        let k = OpKind::Activation;
        assert_eq!(k.step_index("completed"), k.steps().len() - 1);
        let k = OpKind::Update;
        assert_eq!(k.step_index("complete"), k.steps().len() - 1);
    }
}
