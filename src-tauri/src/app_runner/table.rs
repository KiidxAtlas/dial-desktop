//! The runs table: at most one run per workspace, and every lookup names the run it expects.
//!
//! Generic over the entry so the rules are tested without a guest. A stale caller with an old run
//! id gets `stale_workspace` from [`Runs::exact`] and cannot remove a newer run through
//! [`Runs::remove_exact`].

use std::collections::HashMap;

use super::AppRunError;

/// An entry that names its run.
pub trait HasRunId {
    fn run_id(&self) -> &str;
}

pub struct Runs<T> {
    by_workspace: HashMap<String, T>,
}

impl<T> Default for Runs<T> {
    fn default() -> Self {
        Runs {
            by_workspace: HashMap::new(),
        }
    }
}

impl<T: HasRunId> Runs<T> {
    pub fn contains(&self, workspace: &str) -> bool {
        self.by_workspace.contains_key(workspace)
    }

    pub(super) fn entries(&self) -> impl Iterator<Item = &T> {
        self.by_workspace.values()
    }

    /// Add `run` for `workspace`. Returns it back when the workspace already has a run.
    pub fn insert(&mut self, workspace: &str, run: T) -> Result<(), T> {
        if self.by_workspace.contains_key(workspace) {
            return Err(run);
        }
        self.by_workspace.insert(workspace.to_owned(), run);
        Ok(())
    }

    pub fn get(&self, workspace: &str) -> Option<&T> {
        self.by_workspace.get(workspace)
    }

    /// The run for `workspace`, which must be `run_id`.
    pub fn exact(&mut self, workspace: &str, run_id: &str) -> Result<&mut T, AppRunError> {
        let run = self
            .by_workspace
            .get_mut(workspace)
            .ok_or_else(|| AppRunError::new("not_running", "No run is open for this workspace."))?;
        if run.run_id() != run_id {
            return Err(AppRunError::new(
                "stale_workspace",
                "That run is no longer open.",
            ));
        }
        Ok(run)
    }

    /// Remove the run for `workspace` only if it is `run_id`.
    pub fn remove_exact(&mut self, workspace: &str, run_id: &str) -> Option<T> {
        if self
            .by_workspace
            .get(workspace)
            .is_some_and(|run| run.run_id() == run_id)
        {
            self.by_workspace.remove(workspace)
        } else {
            None
        }
    }

    pub fn drain(&mut self) -> Vec<T> {
        self.by_workspace.drain().map(|(_, run)| run).collect()
    }
}

#[cfg(test)]
mod tests;
