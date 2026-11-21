use super::*;
use crate::cli::Preview;
use crate::preview;
use serde_json::json;

fn workspace() -> Workspace {
    Workspace::new("/home/ada/work/app", Some("/home/ada"))
}

fn image_call(arguments: &str, result: Option<dial_client::ToolOutcome>) -> ToolEntry {
    ToolEntry {
        call_id: "g1".into(),
        name: "image_gen".into(),
        arguments: arguments.into(),
        result,
        ..ToolEntry::default()
    }
}

fn outcome(ok: Option<bool>, changed: &[&str]) -> dial_client::ToolOutcome {
    dial_client::ToolOutcome {
        is_error: ok != Some(true),
        outcome_known: ok.is_some(),
        changed_paths: changed.iter().map(|p| (*p).to_owned()).collect(),
        ..dial_client::ToolOutcome::default()
    }
}

#[test]
fn an_edit_shows_its_hunks_instead_of_the_patch_and_receipt() {
    let edit = |text: &str| {
        let tool = ToolEntry {
            call_id: "e1".into(),
            name: "edit".into(),
            arguments: r#"{"input":"[src/calc.py#E1A8]\nPUT 2.=2:\n+    return a + b"}"#.into(),
            result: Some(dial_client::ToolOutcome {
                text: text.into(),
                ..outcome(Some(true), &["/home/ada/work/app/src/calc.py"])
            }),
            ..ToolEntry::default()
        };
        ToolView::new(0, &tool, &workspace())
    };
    let view = edit(
        "[src/calc.py#BA1A]\nupdated (+1 -1 lines)\n1:def add(a, b):\n2:    return a + b\n\n\
         change preview (+1 -1 lines)\n@@ -1,2 +1,2 @@\n def add(a, b):\n-    return a - b\n\
         +    return a + b\n[final newline added]\n\n[Edit churn: run a check]",
    );
    assert_eq!(view.input, None);
    assert_eq!(
        view.output,
        Some(Block {
            caption: "diff",
            text: "@@ -1,2 +1,2 @@\n def add(a, b):\n-    return a - b\n+    return a + b\n\
                   [final newline added]"
                .into(),
        })
    );

    // Several files, or no hunks: the full result, which names each file, and the arguments.
    for text in [
        "[a.py#1]\n\nchange preview (+1 -0 lines)\n@@ -1 +1,2 @@\n+x\n[b.py#2]\n\
         \nchange preview (+1 -0 lines)\n@@ -1 +1,2 @@\n+y",
        "[a.py#1]\nno changes",
    ] {
        let view = edit(text);
        assert_eq!(view.input.unwrap().caption, "arguments");
        assert_eq!(view.output.unwrap().caption, "result");
    }
}

#[test]
fn generated_images_show_inline_from_changed_paths_else_the_output_argument() {
    let w = workspace();
    let args = r#"{"prompt":"a kite","output":"art/kite.png"}"#;
    // Live result: every written image inside the workspace, in order; other files are skipped.
    let live = image_call(
        args,
        Some(outcome(
            Some(true),
            &[
                "/home/ada/work/app/art/kite.png",
                "/home/ada/work/app/art/kite-2.png",
                "/home/ada/work/app/notes.txt",
                "/elsewhere/kite.png",
            ],
        )),
    );
    assert_eq!(
        ToolView::new(0, &live, &w).images,
        [
            "/home/ada/work/app/art/kite.png",
            "/home/ada/work/app/art/kite-2.png"
        ]
    );
    // Resumed history without changed paths or a known outcome still shows the requested output.
    let resumed = image_call(args, Some(outcome(None, &[])));
    assert_eq!(
        ToolView::new(0, &resumed, &w).images,
        ["/home/ada/work/app/art/kite.png"]
    );
    // Running, failed, escaping or non-image outputs, and other tools, show nothing.
    assert!(
        ToolView::new(0, &image_call(args, None), &w)
            .images
            .is_empty()
    );
    let failed = image_call(args, Some(outcome(Some(false), &[])));
    assert!(ToolView::new(0, &failed, &w).images.is_empty());
    for output in ["../x.png", "/etc/x.png", "art/kite.txt"] {
        let call = image_call(
            &format!(r#"{{"output":"{output}"}}"#),
            Some(outcome(Some(true), &[])),
        );
        assert!(ToolView::new(0, &call, &w).images.is_empty(), "{output}");
    }
    // Any tool shows the image files it wrote; only `image_gen` falls back to its output argument.
    let mut write = live.clone();
    write.name = "write".into();
    assert_eq!(
        ToolView::new(0, &write, &w).images,
        [
            "/home/ada/work/app/art/kite.png",
            "/home/ada/work/app/art/kite-2.png"
        ]
    );
    let mut resumed_write = resumed.clone();
    resumed_write.name = "write".into();
    assert!(ToolView::new(0, &resumed_write, &w).images.is_empty());
}

#[test]
fn a_read_shows_the_images_it_viewed() {
    let w = workspace();
    let read = |ok: Option<bool>, text: &str| ToolEntry {
        call_id: "r1".into(),
        name: "read".into(),
        arguments: r#"{"paths":["shots/a.png","shots/b (1).jpg"]}"#.into(),
        result: Some(dial_client::ToolOutcome {
            text: text.into(),
            ..outcome(ok, &[])
        }),
        ..ToolEntry::default()
    };
    let receipt = "Image: shots/a.png (1298289 bytes; attached to the next model request when vision is available)\n\
         Image: shots/b (1).jpg (52 bytes; attached to the next model request when vision is available)\n\
         Image: shots/a.png (1298289 bytes; attached to the next model request when vision is available)\n\
         Image: /elsewhere/c.png (9 bytes; attached to the next model request when vision is available)\n\
         Image: art/logo.svg rasterized from SVG; attached to the next model request when vision is available\n\
         Image: notes.png (not a receipt)";
    // Each viewed image inside the workspace, once, in order; converted sources and prose are not.
    assert_eq!(
        ToolView::new(0, &read(Some(true), receipt), &w).images,
        ["/home/ada/work/app/shots/a.png", "/home/ada/work/app/shots/b (1).jpg"]
    );
    assert!(ToolView::new(0, &read(Some(false), receipt), &w).images.is_empty());
    // Text from other tools that looks like a receipt shows nothing.
    let mut bash = read(Some(true), receipt);
    bash.name = "bash".into();
    assert!(ToolView::new(0, &bash, &w).images.is_empty());
    // Many images: the row keeps the first few.
    let many: String = (0..20)
        .map(|i| format!("Image: shots/{i}.png (1 bytes; attached)\n"))
        .collect();
    assert_eq!(ToolView::new(0, &read(Some(true), &many), &w).images.len(), 8);
}

#[test]
fn workspace_abbreviates_home_and_relativizes_paths() {
    let w = workspace();
    assert_eq!(w.display, "~/work/app");
    assert_eq!(w.name, "app");
    assert_eq!(w.relative("/home/ada/work/app/src/a.rs"), "src/a.rs");
    assert_eq!(
        w.relative("/home/ada/work/apple/a.rs"),
        "/home/ada/work/apple/a.rs"
    );
    assert_eq!(w.relative("/home/ada/work/app"), "/home/ada/work/app");
    assert_eq!(Workspace::new("/srv/x/", None).name, "x");
    assert_eq!(Workspace::new("/srv/x", None).display, "/srv/x");
}

#[test]
fn sidebar_shows_each_session_once_even_if_history_repeats_it() {
    let w = workspace();
    let mut model = preview::model(Preview::Session, &w.root);
    let count = model.sessions.len();
    model.sessions.push(model.sessions[0].clone());
    assert_eq!(Snapshot::new(&model, Some(&w), 0).sessions.len(), count);
}

#[test]
fn sessions_show_what_runs_in_other_tabs() {
    let w = workspace();
    let model = preview::model(Preview::Session, &w.root);
    let mut snap = Snapshot::new(&model, Some(&w), crate::time::now());
    let listed = snap.sessions[2].id.clone();
    let activity = HashMap::from([
        (
            listed.clone(),
            Activity {
                title: "ignored".into(),
                running: true,
                waiting: true,
            },
        ),
        (
            "fresh".to_owned(),
            Activity {
                title: "First turn".into(),
                running: true,
                waiting: false,
            },
        ),
        (
            "idle".to_owned(),
            Activity {
                title: "Idle".into(),
                running: false,
                waiting: false,
            },
        ),
    ]);
    let count = snap.sessions.len();
    snap.show_activity(&activity);
    // A running session the list lacks is added on top; an idle one is not.
    assert_eq!(snap.sessions.len(), count + 1);
    assert_eq!(
        (
            snap.sessions[0].id.as_str(),
            snap.sessions[0].title.as_str()
        ),
        ("fresh", "First turn")
    );
    assert!(snap.sessions[0].running && !snap.sessions[0].waiting);
    let row = snap.sessions.iter().find(|s| s.id == listed).unwrap();
    assert!(row.running && row.waiting);
    assert!(!snap.sessions.iter().any(|s| s.id == "idle"));
}

/// The UI's side of [`patch`]: replace the changed fields and the transcript's tail.
fn apply(prev: &Snapshot, patch: &serde_json::Map<String, Value>) -> Value {
    let mut out = serde_json::to_value(prev).unwrap();
    let object = out.as_object_mut().unwrap();
    for (key, value) in patch {
        if key == "entriesFrom" || key == "entriesTail" {
            continue;
        }
        object.insert(key.clone(), value.clone());
    }
    if let Some(from) = patch.get("entriesFrom").and_then(Value::as_u64) {
        let entries = object["entries"].as_array_mut().unwrap();
        entries.truncate(from as usize);
        entries.extend(patch["entriesTail"].as_array().unwrap().iter().cloned());
    }
    out
}

#[test]
fn published_snapshots_send_only_what_changed() {
    let w = workspace();
    let model = preview::model(Preview::Session, &w.root);
    let first = Snapshot::new(&model, Some(&w), 0);
    let mut published = Published::default();

    let event = published.advance(first.clone()).unwrap();
    assert_eq!((event.seq, event.base), (1, 0));
    assert_eq!(event.full.as_ref(), Some(&first));
    assert!(published.advance(first.clone()).is_none(), "nothing changed");

    // A reply streams: only the last entry and the title change.
    let mut next = first.clone();
    next.title = "Renamed".into();
    let last = next.entries.len() - 1;
    next.entries[last] = Arc::new(EntryView::Assistant {
        text: "still streaming".into(),
        reasoning: String::new(),
        streaming: true,
        abandoned: false,
    });
    let event = published.advance(next.clone()).unwrap();
    assert_eq!((event.seq, event.base), (2, 1));
    let patch = event.patch.unwrap();
    let mut keys: Vec<&str> = patch.keys().map(String::as_str).collect();
    keys.sort();
    assert_eq!(keys, ["entriesFrom", "entriesTail", "title"]);
    assert_eq!(patch["entriesFrom"], last);
    assert_eq!(apply(&first, &patch), serde_json::to_value(&next).unwrap());

    // Undoing a turn shortens the transcript.
    let mut shorter = next.clone();
    shorter.entries.truncate(2);
    let patch = published.advance(shorter.clone()).unwrap().patch.unwrap();
    assert_eq!(patch["entriesFrom"], 2);
    assert_eq!(patch["entriesTail"], json!([]));
    assert_eq!(apply(&next, &patch), serde_json::to_value(&shorter).unwrap());

    let at = published.current(|| unreachable!("already published"));
    assert_eq!((at.seq, &at.snapshot), (3, &shorter));
}

#[test]
fn the_first_request_publishes_a_snapshot() {
    let w = workspace();
    let model = preview::model(Preview::Session, &w.root);
    let snap = Snapshot::new(&model, Some(&w), 0);
    let mut published = Published::default();
    let at = published.current(|| snap.clone());
    assert_eq!((at.seq, &at.snapshot), (1, &snap));
    assert!(published.advance(snap).is_none());
}

#[test]
fn the_session_preview_snapshot_covers_every_row_kind() {
    let w = workspace();
    let model = preview::model(Preview::Session, &w.root);
    let snap = Snapshot::new(&model, Some(&w), crate::time::now());

    assert_eq!(snap.connection, ConnectionView::Preview);
    assert!(snap.turn_running);
    assert!(!snap.can_send);
    assert_eq!(snap.title, "Rate limiting for the public API");
    assert_eq!(snap.approvals.len(), 1);
    assert_eq!(snap.plan.len(), 4);
    assert_eq!(snap.plan[2].status, "in_progress");
    assert_eq!(
        snap.stats,
        StatsView {
            running: 1,
            succeeded: 7,
            failed: 1
        }
    );
    assert_eq!(
        snap.changes
            .iter()
            .map(|c| (c.dir.as_str(), c.name.as_str()))
            .collect::<Vec<_>>(),
        [
            ("src/server/", "limiter.rs"),
            ("src/server/", "router.rs"),
            ("src-tauri/icons/", "128x128@2x.png")
        ]
    );
    assert!(
        snap.sessions
            .iter()
            .any(|s| s.selected && s.meta.ends_with("· 24 messages"))
    );

    let tools: Vec<&ToolView> = snap
        .entries
        .iter()
        .filter_map(|e| match &**e {
            EntryView::Tool(t) => Some(&**t),
            _ => None,
        })
        .collect();
    let edit = tools.iter().find(|t| t.key == "tool:c3").unwrap();
    assert_eq!(edit.state, ToolState::Succeeded);
    assert_eq!(edit.label, "Edit");
    assert_eq!(edit.changed_paths, ["src/server/limiter.rs"]);
    let image = tools.iter().find(|t| t.key == "tool:c6g").unwrap();
    assert_eq!(
        image.images,
        ["/home/ada/work/app/src-tauri/icons/128x128@2x.png"]
    );
    assert!(edit.images.is_empty());
    let failed = tools.iter().find(|t| t.key == "tool:c5").unwrap();
    assert_eq!(failed.state, ToolState::Failed);
    assert_eq!(failed.facts, ["status: error"]);
    let running = tools.iter().find(|t| t.key == "tool:c7").unwrap();
    assert_eq!(running.state, ToolState::Running);
    assert_eq!(running.progress.as_deref(), Some("8 of 12 tests passed"));
    assert_eq!(running.output.as_ref().unwrap().caption, "output");
}

#[test]
fn snapshots_serialize_with_camel_case_tags() {
    let w = workspace();
    let model = preview::model(Preview::Session, &w.root);
    let value = serde_json::to_value(Snapshot::new(&model, Some(&w), 0)).unwrap();
    assert_eq!(value["connection"], json!({"state": "preview"}));
    assert_eq!(value["turnRunning"], json!(true));
    assert_eq!(value["workspace"]["display"], json!("~/work/app"));
    let kinds: Vec<&str> = value["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap())
        .collect();
    for kind in ["user", "assistant", "tool", "notice"] {
        assert!(kinds.contains(&kind), "{kind} missing from {kinds:?}");
    }
    let tool = value["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["kind"] == "tool")
        .unwrap();
    assert!(tool.get("changedPaths").is_some());
    assert_eq!(
        value["approvals"][0]["approvalId"],
        json!("preview-approval")
    );

    let live = serde_json::to_value(ConnectionView::Live {
        model: "m".into(),
        engine_version: Some("1.0".into()),
    })
    .unwrap();
    assert_eq!(
        live,
        json!({"state": "live", "model": "m", "engineVersion": "1.0"})
    );
}

#[test]
fn questions_become_answerable_items_or_plain_text() {
    let request = json!({
        "question_id": "q1", "generation": 2, "session_id": "s1",
        "questions": [
            {"id": "a", "header": "Push", "question": " Which remote? ",
             "options": [{"label": "origin", "description": "fork"}, {"label": ""}, {"label": "up"}],
             "recommended": 1},
            {"id": "b", "question": "Also?", "options": [], "multi": true, "recommended": 3},
            {"question": "no id is dropped", "options": []}
        ]
    });
    let view = question_view(&request);
    assert_eq!(view.id, "q1");
    assert_eq!(view.items.len(), 2);
    let first = &view.items[0];
    assert_eq!(
        (first.header.as_deref(), first.text.as_str()),
        (Some("Push"), "Which remote?")
    );
    let labels: Vec<_> = first.options.iter().map(|o| o.label.as_str()).collect();
    assert_eq!(labels, ["origin", "up"]);
    assert_eq!(first.options[0].description.as_deref(), Some("fork"));
    assert_eq!(first.recommended, Some(1));
    assert!(view.items[1].multi);
    assert_eq!(view.items[1].recommended, None, "out of range");

    // Without its identity, or in an older shape, the text shows but is not answerable.
    let old = question_view(&json!({"question": "Which?"}));
    assert_eq!(
        (old.id.as_str(), old.items[0].text.as_str()),
        ("", "Which?")
    );
    let anonymous = question_view(&json!({"questions": [{"id": "a", "question": "First?"}]}));
    assert_eq!(
        (anonymous.id.as_str(), anonymous.items[0].text.as_str()),
        ("", "First?")
    );
    assert_eq!(
        question_view(&json!({})).items[0].text,
        "Dial is asking a question."
    );
}

#[test]
fn the_model_view_prefers_state_and_labels_from_the_list() {
    use crate::model::{CurrentModel, ModelOption};
    let mut model = Model::starting();
    assert_eq!(ModelView::of(&model), None);
    model.connection = Some(crate::model::Connection::Live {
        model: "claude-opus".into(),
        engine_version: None,
    });
    let named = ModelView::of(&model).unwrap();
    assert_eq!(
        (named.provider.as_str(), named.label.as_str()),
        ("", "claude-opus")
    );

    model.current_model = Some(CurrentModel {
        provider: "anthropic".into(),
        model_id: "claude-opus".into(),
        thinking: Some("high".into()),
        levels: vec!["low".into(), "high".into()],
        thinking_settable: true,
        ..CurrentModel::default()
    });
    model.models = vec![ModelOption {
        provider: "anthropic".into(),
        model_id: "claude-opus".into(),
        label: "Claude Opus".into(),
        context_window: None,
        reasoning: true,
        vision: Some(true),
        configured: true,
        local: false,
        listed: false,
        catalog_only: false,
    }];
    model.approval = Some("ask".into());
    let value = serde_json::to_value(Snapshot::new(&model, None, 0)).unwrap();
    assert_eq!(value["workspace"], json!(null));
    assert_eq!(value["approval"], json!("ask"));
    assert_eq!(value["model"]["label"], json!("Claude Opus"));
    assert_eq!(value["model"]["modelId"], json!("claude-opus"));
    assert_eq!(value["model"]["thinking"], json!("high"));
    assert_eq!(value["model"]["thinkingSettable"], json!(true));
    assert_eq!(value["model"]["acceptsImages"], json!(true));
}

#[test]
fn image_support_comes_from_the_list_else_from_ready_for_the_starting_model() {
    use crate::model::CurrentModel;
    let mut model = Model::starting();
    model.connection = Some(crate::model::Connection::Live {
        model: "m1".into(),
        engine_version: None,
    });
    model.ready_image_input = Some(true);
    assert_eq!(ModelView::of(&model).unwrap().accepts_images, Some(true));
    let current = |id: &str| CurrentModel {
        provider: "p".into(),
        model_id: id.into(),
        ..CurrentModel::default()
    };
    model.current_model = Some(current("m1"));
    let row = |id: &str| json!({"provider":"p", "model_id":id});
    model.models = [row("m1"), row("m2")]
        .iter()
        .filter_map(crate::model::ModelOption::from_wire)
        .collect();
    assert_eq!(ModelView::of(&model).unwrap().accepts_images, Some(true));
    // After a switch, `ready` says nothing about the new model.
    model.current_model = Some(current("m2"));
    assert_eq!(ModelView::of(&model).unwrap().accepts_images, None);
    let mut text_only = row("m2");
    text_only["vision"] = json!(false);
    model.models = vec![crate::model::ModelOption::from_wire(&text_only).unwrap()];
    assert_eq!(ModelView::of(&model).unwrap().accepts_images, Some(false));
}

#[test]
fn the_empty_preview_has_no_selected_session() {
    let w = workspace();
    let model = preview::model(Preview::Empty, &w.root);
    let snap = Snapshot::new(&model, Some(&w), 0);
    assert!(snap.entries.is_empty());
    assert!(!snap.sessions.is_empty());
    assert!(snap.sessions.iter().all(|s| !s.selected));
}

fn feed(model: &mut Model, body: Value) {
    let kind = body["event"].as_str().unwrap_or_default().to_owned();
    model.conversation.apply_event(&dial_client::Event { kind, body });
}

/// A session with `turns` turns, each an answer and three settled `read` calls.
fn long_session(w: &Workspace, turns: usize, output: &str) -> Model {
    let mut model = preview::model(Preview::Empty, &w.root);
    feed(&mut model, json!({"event": "ready", "session_id": "long"}));
    for turn in 0..turns {
        model.conversation.push_user(&format!("prompt {turn}"), 0);
        feed(&mut model, json!({"event": "text", "delta": "word ".repeat(400)}));
        for call in 0..3 {
            let id = format!("c{turn}-{call}");
            feed(&mut model, json!({"event": "tool_call", "call_id": id, "name": "read", "summary": "src/lib.rs"}));
            feed(&mut model, json!({"event": "tool_result", "call_id": id, "name": "read", "text": output, "is_error": false, "status": "ok", "changed_paths": []}));
        }
    }
    model
}

#[test]
fn reused_rows_match_a_fresh_build_and_only_changed_rows_are_rebuilt() {
    let w = workspace();
    let mut model = long_session(&w, 2, "output");
    feed(&mut model, json!({"event": "tool_call", "call_id": "run", "name": "bash", "summary": "cargo test"}));
    let mut views = EntryViews::default();
    let first = Snapshot::reusing(&model, Some(&w), 0, &mut views);
    assert_eq!(first, Snapshot::new(&model, Some(&w), 0));
    let running = first.entries.len() - 1;

    // The running call settles and an answer starts: earlier rows are shared, the rest rebuilt.
    feed(&mut model, json!({"event": "tool_result", "call_id": "run", "name": "bash", "text": "ok", "is_error": false, "status": "ok", "changed_paths": []}));
    feed(&mut model, json!({"event": "text", "delta": "All tests pass."}));
    let next = Snapshot::reusing(&model, Some(&w), 0, &mut views);
    assert_eq!(next, Snapshot::new(&model, Some(&w), 0));
    assert_eq!(next.entries.len(), running + 2);
    assert!((0..running).all(|i| Arc::ptr_eq(&first.entries[i], &next.entries[i])));
    assert!(!Arc::ptr_eq(&first.entries[running], &next.entries[running]));

    // A shorter transcript (an undone turn) drops the rows past its end.
    model.conversation.load_history(Some("long"), &[json!({"role": "user", "content": "only this"})]);
    let shorter = Snapshot::reusing(&model, Some(&w), 0, &mut views);
    assert_eq!(shorter, Snapshot::new(&model, Some(&w), 0));
    assert_eq!(shorter.entries.len(), 1);

    // Rows show paths relative to the workspace, so a different workspace rebuilds every row.
    let mut model = long_session(&w, 1, "Image: /home/ada/work/app/shot.png (10 bytes; image/png)");
    let mut views = EntryViews::default();
    let here = Snapshot::reusing(&model, Some(&w), 0, &mut views);
    let other = Workspace::new("/home/ada/elsewhere", Some("/home/ada"));
    feed(&mut model, json!({"event": "text", "delta": "."}));
    let there = Snapshot::reusing(&model, Some(&other), 0, &mut views);
    assert_eq!(there, Snapshot::new(&model, Some(&other), 0));
    assert!(here.entries.iter().zip(&there.entries).all(|(a, b)| !Arc::ptr_eq(a, b)));
    assert_ne!(here.entries[2], there.entries[2], "the image is outside the other workspace");
}

#[test]
#[ignore = "timing benchmark: cargo test --release snapshot_cost -- --ignored --nocapture"]
fn snapshot_cost_on_a_long_session() {
    let w = workspace();
    let mut model = long_session(&w, 300, &"line of tool output\n".repeat(300));
    model.conversation.push_user("last prompt", 0);
    let frames = 60;
    let mut views = EntryViews::default();
    for (name, reuse) in [("rebuild every row", false), ("reuse unchanged rows", true)] {
        let mut published = Published::default();
        published.advance(Snapshot::reusing(&model, Some(&w), 0, &mut views));
        let start = std::time::Instant::now();
        for i in 0..frames {
            feed(&mut model, json!({"event": "text", "delta": format!("token{i} ")}));
            let snap = if reuse {
                Snapshot::reusing(&model, Some(&w), 0, &mut views)
            } else {
                Snapshot::new(&model, Some(&w), 0)
            };
            assert!(published.advance(snap).is_some());
        }
        let per_frame = start.elapsed().as_secs_f64() * 1000.0 / f64::from(frames);
        println!("{} entries, {name}: {per_frame:.2} ms per frame", model.conversation.entries().len());
    }
}
