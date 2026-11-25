use super::*;
use serde_json::json;

fn blueprint(value: Value) -> TypeDef {
    serde_json::from_value(value).unwrap()
}

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n0000rest-of-a-tiny-file";

#[test]
fn tool_descriptions_fit_engine_mcp_limit() {
    for tool in tools() {
        let description = tool["description"].as_str().unwrap();
        assert!(description.len() <= 2048, "{} exceeds the MCP cap", tool["name"]);
    }
    assert!(APP_GUIDE.contains("window.dial") && APP_GUIDE.contains("types_screenshot"));
}

#[test]
fn build_progress_reports_only_what_the_patch_set() {
    let mut def = normalize(blueprint(json!({"name":"Cookbook",
        "fields":[{"key":"recipe", "label":"Recipe", "kind":"text"}]})), 1).unwrap();
    assert_eq!(build_step(&def, &json!({"icon":"✦"})), json!({}));
    assert_eq!(build_step(&def, &json!({"fields":[]})), json!({"fields":["Recipe"]}));
    def.html = "x".repeat(1500);
    assert_eq!(build_step(&def, &json!({"html": "…"})), json!({"app": 2}));
}

#[test]
fn normalize_cleans_names_keys_and_colours() {
    let def = normalize(
        blueprint(json!({
            "name": "  Cookbook ",
            "accent": "red",
            "fields": [
                {"label": "Recipe Name"},
                {"key": "Recipe name", "kind": "longtext"},
                {"key": "photo", "kind": "image"},
                {"key": "where", "kind": "json"}
            ],
            "view": {"layout": "book"}
        })),
        7,
    )
    .unwrap();
    assert_eq!(def.name, "Cookbook");
    assert_eq!(def.icon, "✦");
    assert_eq!(def.created, 7);
    assert_eq!(def.accent, "", "not a hex colour");
    let keys: Vec<&str> = def.fields.iter().map(|f| f.key.as_str()).collect();
    assert_eq!(keys, ["recipe_name", "photo", "where"]);
    assert!(!def.has_app());
    assert_eq!(normalize(blueprint(json!({"name": "A", "accent": " #ab12cd "})), 1).unwrap().accent, "#ab12cd");
}

#[test]
fn normalize_refuses_bad_types() {
    assert!(normalize(blueprint(json!({"name": " "})), 1).is_err());
    let huge = json!({"name": "X", "html": "x".repeat(MAX_APP + 1)});
    assert!(normalize(blueprint(huge), 1).is_err());
}

#[test]
fn sanitize_shapes_each_kind_and_drops_unknown_fields() {
    let def = normalize(
        blueprint(json!({"name": "T", "fields": [
            {"key": "title"}, {"key": "steps", "kind": "steps"}, {"key": "stars", "kind": "rating"},
            {"key": "pic", "kind": "image"}, {"key": "on", "kind": "toggle"}, {"key": "n", "kind": "number"},
            {"key": "at", "kind": "json"}
        ]})),
        1,
    )
    .unwrap();
    let out = sanitize_fields(
        &def,
        json!({
            "title": "  Soup ",
            "steps": "1. Chop\n2) Boil\n\n- Serve",
            "stars": 9,
            "pic": "/etc/passwd",
            "on": "yes",
            "n": "2.5",
            "at": {"lat": 51.5, "lon": -0.1},
            "other": "dropped"
        })
        .as_object()
        .unwrap(),
    );
    assert_eq!(
        Value::Object(out),
        json!({"title": "Soup", "steps": ["Chop", "Boil", "Serve"], "stars": 5, "on": true, "n": 2.5, "at": {"lat": 51.5, "lon": -0.1}})
    );
}

#[test]
fn a_type_without_fields_keeps_free_form_items() {
    let def = normalize(blueprint(json!({"name": "Scores"})), 1).unwrap();
    let out = sanitize_fields(
        &def,
        json!({"Player Name": "Ada", "score": 120, "skip": null, "big": "x".repeat(MAX_LONG + 10)})
            .as_object()
            .unwrap(),
    );
    assert_eq!(Value::Object(out), json!({"player_name": "Ada", "score": 120}));
}

#[test]
fn store_round_trip_with_images_and_patches() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(Some(dir.path().join("types")));
    assert!(store.list().unwrap().is_empty());
    let def = store
        .create(blueprint(json!({"name": "Cookbook", "icon": "🍳", "fields": [
            {"key": "title"}, {"key": "photo", "kind": "image"}, {"key": "ingredients", "kind": "list"}
        ]})))
        .unwrap();
    assert!(def.id.starts_with("cookbook-"), "{}", def.id);
    assert_eq!(store.find("COOKBOOK").unwrap(), def.id);
    assert!(store.find("nope").unwrap_err().contains("Cookbook"));

    let image = dir.path().join("pic.png");
    std::fs::write(&image, PNG).unwrap();
    let (item, problems) = store
        .add_item(
            &def.id,
            json!({"title": "Soup", "photo": image.to_str().unwrap(), "ingredients": ["Leek", "Salt"]})
                .as_object()
                .unwrap()
                .clone(),
            Some(Source { text: "# Soup".into(), ..Source::default() }),
        )
        .unwrap();
    assert!(problems.is_empty(), "{problems:?}");
    let asset = item.fields["photo"].as_str().unwrap().to_owned();
    assert!(valid_asset(&asset) && asset.ends_with(".png"));
    let full = store.get(&def.id).unwrap();
    assert!(Path::new(&full.assets).join(&asset).is_file());
    assert_eq!(full.items.len(), 1);

    let (_, problems) = store
        .add_item(
            &def.id,
            json!({"title": "Bread", "photo": "https://x/y.png"})
                .as_object()
                .unwrap()
                .clone(),
            None,
        )
        .unwrap();
    assert_eq!(problems.len(), 1);
    assert!(store.add_item(&def.id, Map::new(), None).is_err());

    let patched = store
        .update_item(
            &def.id,
            &item.id,
            ItemPatch {
                fields: Some(
                    json!({"ingredients": null, "title": "Leek soup"})
                        .as_object()
                        .unwrap()
                        .clone(),
                ),
                done: Some(true),
            },
        )
        .unwrap();
    assert_eq!(patched.fields.get("ingredients"), None);
    assert_eq!(patched.fields["title"], "Leek soup");
    assert!(patched.done);

    let second = store.get(&def.id).unwrap().items[1].id.clone();
    store.move_item(&def.id, &second, 0).unwrap();
    assert_eq!(store.get(&def.id).unwrap().items[0].id, second);

    let restyled = store
        .update(&def.id, &json!({"accent": "#fff8ee", "description": "Family recipes", "id": "hijack"}))
        .unwrap();
    assert_eq!(restyled.id, def.id);
    assert_eq!(restyled.accent, "#fff8ee");
    assert_eq!(restyled.fields.len(), 3, "members not named stay");
    assert_eq!(store.get(&def.id).unwrap().items.len(), 2, "items stay");

    let listed = store.list().unwrap();
    assert_eq!((listed[0].name.as_str(), listed[0].count), ("Cookbook", 2));
    store.remove_item(&def.id, &second).unwrap();
    store.delete(&def.id).unwrap();
    assert!(store.list().unwrap().is_empty());
}

#[test]
fn ids_never_leave_the_store() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(Some(dir.path().to_path_buf()));
    assert!(store.get("../etc").is_err());
    assert!(store.delete("a/b").is_err());
    assert!(!valid_asset("../x.png"));
    assert!(!valid_asset("0123456789abcdef.svg"));
}

#[test]
fn a_named_type_has_no_app_and_its_sessions_are_replaced_whole() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(Some(dir.path().to_path_buf()));
    let def = store.create(blueprint(json!({"name": "Cookbook"}))).unwrap();
    assert!(!def.has_app());
    assert!(describe(&def, 0).contains("NO APP YET"));

    store
        .update(&def.id, &json!({"sessions": {"/a": "s1", "/b": "s2"}}))
        .unwrap();
    let def = store
        .update(&def.id, &json!({"sessions": {"/b": "s3"}}))
        .unwrap();
    assert_eq!(def.sessions.len(), 1);
    assert_eq!(def.sessions["/b"], "s3");

    let def = store.update(&def.id, &json!({"html": "<p>app</p>"})).unwrap();
    assert!(def.has_app());
    assert_eq!(def.sessions["/b"], "s3", "a new app keeps the sessions");
    let summary = &store.list().unwrap()[0];
    assert!(summary.has_app);
    assert_eq!(summary.sessions, ["s3"]);
    assert!(!describe(&def, 0).contains("NO APP"));
}

#[test]
fn claiming_a_type_chat_preserves_other_projects_and_refuses_replacement() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(Some(dir.path().to_path_buf()));
    let id = store.create(blueprint(json!({"name":"Notes"}))).unwrap().id;
    for (root, session) in [("/a", "one"), ("/b", "two")] {
        store.claim_session(&id, root.into(), session.into()).unwrap();
    }
    assert!(store.claim_session(&id, "/a".into(), "other".into()).is_err());
    assert_eq!(store.list().unwrap()[0].sessions, ["one", "two"]);
}

#[test]
fn the_app_lives_in_its_own_file_and_is_served_with_the_runtime() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(Some(dir.path().to_path_buf()));
    let app = store
        .create(blueprint(json!({"name": "Weather", "html": "<!doctype html><html><head><title>W</title></head><body>hi</body></html>"})))
        .unwrap();
    let bare = store.create(blueprint(json!({"name": "Book"}))).unwrap();
    let folder = dir.path().join(&app.id);
    assert!(folder.join("app.html").is_file());
    let saved = std::fs::read_to_string(folder.join("type.json")).unwrap();
    assert!(!saved.contains("<body>"), "type.json stays small: {saved}");

    // The UI gets everything but the HTML; the scheme serves it.
    let full = store.get(&app.id).unwrap();
    assert!(full.has_app && full.def.html.is_empty());
    assert_eq!(full.state, json!({}));

    let page = custom_page(&store, &format!("/{}?v=3", app.id));
    assert_eq!(page.status(), 200);
    let body = String::from_utf8(page.body().clone()).unwrap();
    assert!(body.starts_with("<!doctype html><html><head><script>"), "{}", &body[..80]);
    assert!(body.contains("window.dial = dial") && body.ends_with("<title>W</title></head><body>hi</body></html>"));
    assert_eq!(page.headers()["Content-Security-Policy"], CUSTOM_CSP);
    assert!(CUSTOM_CSP.contains("connect-src https:") && !CUSTOM_CSP.contains("ipc"));
    assert_eq!(custom_page(&store, &format!("/{}", bare.id)).status(), 404);
    assert_eq!(custom_page(&store, "/../x").status(), 404);

    // Clearing the app removes its file.
    store.update(&app.id, &json!({"html": ""})).unwrap();
    assert!(!folder.join("app.html").exists());
}

#[test]
fn a_legacy_custom_page_becomes_the_app() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(Some(dir.path().to_path_buf()));
    let folder = dir.path().join("old-1234");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join("type.json"),
        json!({"version": 1, "id": "old-1234", "name": "Old", "view": {"layout": "custom"},
            "theme": {"accent": "#123456"}, "customHtml": "<p>old</p>", "designed": true}).to_string(),
    )
    .unwrap();
    assert!(store.list().unwrap()[0].has_app);
    assert_eq!(store.app_html("old-1234").unwrap(), "<p>old</p>");
    store.update("old-1234", &json!({"name": "Older"})).unwrap();
    assert_eq!(std::fs::read_to_string(folder.join("app.html")).unwrap(), "<p>old</p>");
}

#[test]
fn the_runtime_goes_first_in_the_head() {
    let at = |html: &str| with_runtime(html).find("<script>").unwrap();
    assert_eq!(at("<HTML lang=en><HEAD>\n<script>x</script>"), "<HTML lang=en><HEAD>".len());
    assert_eq!(at("<!DOCTYPE html>\n<header>no</header>"), "<!DOCTYPE html>".len(), "<header> is not <head>");
    assert_eq!(at("<div>fragment</div>"), 0);
}

#[test]
fn edits_apply_all_or_none() {
    let html = "<h1>Hi</h1><p>a</p><p>a</p>";
    let ok = apply_edits(html, &[json!({"find": "Hi", "replace": "Hello"}), json!({"find": "<p>a</p>", "replace": "<p>b</p>", "all": true})]);
    assert_eq!(ok.unwrap(), "<h1>Hello</h1><p>b</p><p>b</p>");
    assert!(apply_edits(html, &[json!({"find": "<p>a</p>", "replace": "x"})]).unwrap_err().contains("2 times"));
    assert!(apply_edits(html, &[json!({"find": "Hi", "replace": "Yo"}), json!({"find": "nope", "replace": ""})]).unwrap_err().contains("Edit 2"));
    assert!(apply_edits(html, &[]).is_err());
}

#[test]
fn the_app_keeps_its_own_state() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(Some(dir.path().to_path_buf()));
    let id = store.create(blueprint(json!({"name": "Snake"}))).unwrap().id;
    store.set_state(&id, &json!({"best": 42})).unwrap();
    assert_eq!(store.get(&id).unwrap().state, json!({"best": 42}));
    assert!(store.set_state(&id, &json!("x".repeat(MAX_STATE))).is_err());
    assert!(store.set_state("nope-1", &json!({})).is_err());
}

#[test]
fn new_types_go_where_the_user_chose_and_both_places_are_listed() {
    let dir = tempfile::tempdir().unwrap();
    let app = dir.path().join("app").join("types");
    let project = dir.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let store = Store::new(Some(app.clone()));
    store.set_project(Some(&project));

    let shared = store.create(blueprint(json!({"name": "Recipes"}))).unwrap();
    assert!(app.join(&shared.id).join("type.json").is_file(), "the app is the default");

    store.set_save_in(Scope::Project).unwrap();
    let local = store
        .create(blueprint(json!({"name": "Moodboard"})))
        .unwrap();
    let local_dir = project.join(".dial").join("types").join(&local.id);
    assert!(local_dir.join("type.json").is_file());
    assert!(!app.join(&local.id).exists());

    // Items and images of a project type stay in the project.
    let fields = json!({"title": "Sketch"}).as_object().unwrap().clone();
    store.add_item(&local.id, fields, None).unwrap();
    assert!(local_dir.join("items.json").is_file());
    let png = dir.path().join("pic.png");
    std::fs::write(&png, PNG).unwrap();
    let asset = store.import_image(&local.id, &png).unwrap();
    assert!(local_dir.join("assets").join(asset).is_file());

    // Both places are listed, each marked with where it is, whatever the setting is.
    store.set_save_in(Scope::App).unwrap();
    let list = store.list().unwrap();
    let scope_of = |id: &str| list.iter().find(|s| s.id == id).unwrap().scope;
    assert_eq!(list.len(), 2);
    assert_eq!(scope_of(&shared.id), Scope::App);
    assert_eq!(scope_of(&local.id), Scope::Project);
    assert_eq!(store.get(&local.id).unwrap().items.len(), 1);

    // Another project sees only the app's types; with no project open "project" means the app.
    store.set_project(None);
    assert_eq!(store.list().unwrap().len(), 1);
    assert!(store.get(&local.id).is_err());
    store.set_save_in(Scope::Project).unwrap();
    let fallback = store.create(blueprint(json!({"name": "Notes"}))).unwrap();
    assert!(app.join(&fallback.id).join("type.json").is_file());

    store.set_project(Some(&project));
    store.delete(&local.id).unwrap();
    assert!(!local_dir.exists());
}

#[test]
fn the_choice_of_where_to_save_is_remembered() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::for_home(Some(dir.path()));
    assert_eq!(store.location().save_in, Scope::App);
    assert_eq!(store.location().project, None);
    store.set_save_in(Scope::Project).unwrap();
    store.set_project(Some(Path::new("/work/site")));
    let again = Store::for_home(Some(dir.path()));
    assert_eq!(again.location().save_in, Scope::Project);
    assert_eq!(store.location().project.as_deref(), Some("/work/site/.dial/types"));
}

#[test]
fn a_type_prompt_carries_what_the_type_is() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(Some(dir.path().to_path_buf()));
    let def = store
        .create(blueprint(json!({
            "name": "Image board", "icon": "🖼", "description": "Pictures that set a mood"
        })))
        .unwrap();
    let sent = match type_scope(&store, Some(def.id.as_str()), false) {
        Some(crate::engine::TypeScope::Framed(context)) => legacy_prompt(&context, "make it warmer"),
        other => panic!("{other:?}"),
    };
    assert!(sent.starts_with(&format!("[[type:{}]]\n{CONTEXT_OPEN}", def.id)));
    assert!(sent.contains("Image board") && sent.contains("Pictures that set a mood"));
    assert!(sent.ends_with("make it warmer"));
    // A modern engine gets the Type beside the user's own text.
    match type_scope(&store, Some(def.id.as_str()), true) {
        Some(crate::engine::TypeScope::Typed(context)) => {
            assert_eq!(context.id, def.id);
            assert!(context.briefing.starts_with(CONTEXT_OPEN) && context.briefing.contains("no app yet") && context.briefing.contains("window.dial"));
        }
        other => panic!("{other:?}"),
    }
    // A modern prompt in no Type says so explicitly; an older engine gets nothing for it.
    assert_eq!(type_scope(&store, None, true), Some(crate::engine::TypeScope::Plain));
    assert_eq!(type_scope(&store, Some("nope-1234"), true), Some(crate::engine::TypeScope::Plain));
    assert_eq!(type_scope(&store, None, false), None);
}

#[test]
fn allow_import_false_never_reads_host_image_paths() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(Some(dir.path().join("types")));
    let def = store
        .create(blueprint(json!({
            "name": "Custom Board",
            "fields": [{"key": "title"}, {"key": "photo", "kind": "image"}, {"key": "gallery", "kind": "images"}]
        })))
        .unwrap();
    let secret = dir.path().join("secret.png");
    std::fs::write(&secret, PNG).unwrap();

    let (item, _) = store
        .add_item_with(
            &def.id,
            json!({"title": "Card", "photo": secret.to_str().unwrap(), "gallery": [secret.to_str().unwrap()]})
                .as_object()
                .unwrap()
                .clone(),
            None,
            false,
        )
        .unwrap();
    assert_eq!(item.fields.get("photo"), None);
    assert_eq!(item.fields.get("gallery"), None);
    assert!(
        std::fs::read_dir(&store.get(&def.id).unwrap().assets)
            .map(|mut d| d.next().is_none())
            .unwrap_or(true),
        "no host image may be copied into assets when allow_import is false"
    );
    // Updating from a sandboxed view cannot smuggle a host path either, while existing asset names stay valid.
    let legit = store.import_image(&def.id, &secret).unwrap();
    let updated = store
        .update_item_with(
            &def.id,
            &item.id,
            ItemPatch {
                fields: Some(
                    json!({"photo": legit, "gallery": [secret.to_str().unwrap()]})
                        .as_object()
                        .unwrap()
                        .clone(),
                ),
                ..ItemPatch::default()
            },
            false,
        )
        .unwrap();
    assert_eq!(updated.fields["photo"], legit);
    assert_eq!(updated.fields.get("gallery"), None);
}

#[test]
fn resolve_asset_allows_only_store_assets_and_refuses_outside_files() {
    let dir = tempfile::tempdir().unwrap();
    let app = dir.path().join("app").join("types");
    let project = dir.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let store = Store::new(Some(app));
    store.set_project(Some(&project));

    let def = store.create(blueprint(json!({"name": "Board"}))).unwrap();
    let src = dir.path().join("pic.png");
    std::fs::write(&src, PNG).unwrap();
    let asset = store.import_image(&def.id, &src).unwrap();
    let full = store.get(&def.id).unwrap();
    let asset_path = Path::new(&full.assets).join(&asset);

    assert!(store.resolve_asset(&asset_path).is_ok());
    assert!(store.resolve_asset(&src).is_err(), "files outside the store are rejected");
    assert!(
        store
            .resolve_asset(&Path::new(&full.assets).parent().unwrap().join("type.json"))
            .is_err(),
        "non-asset files in the type folder are rejected"
    );
}

#[test]
fn two_windows_see_save_location_changes_immediately() {
    let dir = tempfile::tempdir().unwrap();
    let first = Store::for_home(Some(dir.path()));
    let second = Store::for_home(Some(dir.path()));
    assert_eq!(second.location().save_in, Scope::App);
    first.set_save_in(Scope::Project).unwrap();
    assert_eq!(second.location().save_in, Scope::Project);
}
