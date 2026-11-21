use super::{HasRunId, Runs};

struct Entry {
    id: &'static str,
}

impl HasRunId for Entry {
    fn run_id(&self) -> &str {
        self.id
    }
}

fn runs_with(workspace: &str, id: &'static str) -> Runs<Entry> {
    let mut runs = Runs::default();
    assert!(runs.insert(workspace, Entry { id }).is_ok());
    runs
}

#[test]
fn a_lookup_names_its_workspace_and_run() {
    let mut runs = runs_with("/a", "run-1");
    let cases: [(&str, &str, Option<&str>); 4] = [
        ("/a", "run-1", None),
        ("/a", "run-0", Some("stale_workspace")),
        ("/b", "run-1", Some("not_running")),
        ("/a/", "run-1", Some("not_running")),
    ];
    for (workspace, run_id, expected) in cases {
        let code = runs.exact(workspace, run_id).err().map(|e| e.code);
        assert_eq!(code, expected, "{workspace} {run_id}");
    }
}

#[test]
fn an_old_run_cannot_remove_a_newer_run_for_the_same_workspace() {
    let mut runs = runs_with("/a", "run-1");
    assert!(runs.remove_exact("/a", "run-1").is_some());
    assert!(runs.insert("/a", Entry { id: "run-2" }).is_ok());
    // A late pump or handshake from run-1 finds run-2 in the slot and leaves it alone.
    assert!(runs.remove_exact("/a", "run-1").is_none());
    assert_eq!(runs.get("/a").map(|run| run.id), Some("run-2"));
}

#[test]
fn a_second_run_for_a_workspace_is_refused_and_returned() {
    let mut runs = runs_with("/a", "run-1");
    let rejected = runs
        .insert("/a", Entry { id: "run-2" })
        .expect_err("a second run for the same workspace must be refused");
    assert_eq!(rejected.id, "run-2");
    assert_eq!(runs.get("/a").map(|run| run.id), Some("run-1"));
}

#[test]
fn workspaces_do_not_share_runs() {
    let mut runs = runs_with("/a", "run-1");
    assert!(runs.insert("/b", Entry { id: "run-2" }).is_ok());
    assert!(runs.remove_exact("/b", "run-1").is_none());
    assert_eq!(runs.drain().len(), 2);
    assert!(!runs.contains("/a"));
}
