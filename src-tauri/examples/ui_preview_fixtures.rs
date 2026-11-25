//! Writes the design-preview snapshots (the same fixtures as `--preview`) as JSON for the browser
//! UI preview, `npm run preview:ui`. The output directory is `$OUT`.
use dial_desktop::{cli::Preview, preview, time, view::{Snapshot, Workspace}};

fn main() {
    let out = std::env::var("OUT").expect("set OUT to the output directory");
    let root = "/Users/demo/code/app";
    let workspace = Workspace::new(root, Some("/Users/demo"));
    for (name, kind) in [("session", Preview::Session), ("empty", Preview::Empty), ("question", Preview::Question)] {
        let model = preview::model(kind, root);
        let snapshot = Snapshot::new(&model, Some(&workspace), time::now());
        let json = serde_json::to_string(&snapshot).expect("snapshot serializes");
        std::fs::write(format!("{out}/{name}.json"), json).expect("write fixture");
    }
}
