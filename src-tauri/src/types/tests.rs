use super::*;
use serde_json::json;

fn blueprint(value: Value) -> TypeDef {
    serde_json::from_value(value).unwrap()
}

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n0000rest-of-a-tiny-file";

#[test]
fn blueprint_descriptions_fit_engine_mcp_limit() {
    for tool in tools() {
        if matches!(tool["name"].as_str(), Some("types_create" | "types_update")) {
            let description = tool["description"].as_str().unwrap();
            assert!(description.len() <= 2048, "{} exceeds the MCP cap", tool["name"]);
            assert!(description.ends_with("illustration."));
        }
    }
}

#[test]
fn design_progress_reports_only_committed_choices_including_explicit_defaults() {
    let def = normalize(blueprint(json!({"name":"Cookbook", "view":{"layout":"book", "style":"cookbook"},
        "fields":[{"key":"recipe", "label":"Recipe", "kind":"text"}], "theme":{"font":"serif"}})), 1).unwrap();
    assert_eq!(design_change(&def, &json!({"icon":"✦"}))["choices"], json!({}));
    assert_eq!(design_change(&def, &json!({"fields":[]}))["choices"], json!({"fields":["Recipe"]}));
    assert_eq!(design_change(&def, &json!({"view":{"layout":"book"}, "theme":{"font":"serif"}})),
        json!({"id":def.id, "choices":{"layout":{"kind":"book", "style":"cookbook"},
            "look":{"font":"serif", "texture":def.theme.texture, "accent":def.theme.accent}}}));
}

#[test]
fn normalize_fills_fields_and_view_from_the_layout() {
    let def = normalize(
        blueprint(json!({"name": "  Routine ", "view": {"layout": "checklist"}})),
        7,
    )
    .unwrap();
    assert_eq!(def.name, "Routine");
    assert_eq!(def.icon, "✦");
    assert_eq!(def.created, 7);
    let keys: Vec<&str> = def.fields.iter().map(|f| f.key.as_str()).collect();
    assert_eq!(keys, ["title", "time", "notes"]);
    assert_eq!(def.view.title_field.as_deref(), Some("title"));
    assert_eq!(def.view.time_field.as_deref(), Some("time"));
    assert_eq!(def.view.lead_minutes, [15, 0]);
}

#[test]
fn a_calendar_always_has_a_date_to_place_items_by() {
    // The user's own case: a calendar drawn as a table, its date kept as text.
    let def = normalize(
        blueprint(json!({"name": "calendar", "view": {"layout": "calendar"}, "fields": [
            {"key": "title", "kind": "text"}, {"key": "when", "kind": "text"}, {"key": "where", "kind": "text"}
        ]})),
        1,
    )
    .unwrap();
    assert_eq!(def.fields[1].kind, FieldKind::Datetime);
    assert_eq!(def.view.time_field.as_deref(), Some("when"));

    // No date-like field: one is added. A time of day alone cannot place an item on a day.
    let def = normalize(
        blueprint(json!({"name": "Plans", "view": {"layout": "calendar", "timeField": "at"}, "fields": [
            {"key": "title", "kind": "text"}, {"key": "at", "kind": "time"}
        ]})),
        1,
    )
    .unwrap();
    assert_eq!(def.fields[1].kind, FieldKind::Datetime, "the named time field becomes a datetime");
    assert_eq!(def.view.time_field.as_deref(), Some("at"));

    let def = normalize(blueprint(json!({"name": "Plans", "view": {"layout": "calendar"}, "fields": [{"key": "title", "kind": "text"}]})), 1).unwrap();
    let keys: Vec<&str> = def.fields.iter().map(|f| f.key.as_str()).collect();
    assert_eq!(keys, ["title", "when"]);
    assert_eq!(def.view.time_field.as_deref(), Some("when"));

    // A date field already there is used as is.
    let def = normalize(blueprint(json!({"name": "Days", "view": {"layout": "calendar"}, "fields": [{"key": "on", "kind": "date"}]})), 1).unwrap();
    assert_eq!(def.fields.len(), 1);
    assert_eq!(def.view.time_field.as_deref(), Some("on"));
}

#[test]
fn normalize_cleans_keys_colours_and_references() {
    let def = normalize(
        blueprint(json!({
            "name": "Cookbook",
            "fields": [
                {"label": "Recipe Name"},
                {"key": "Recipe name", "kind": "longtext"},
                {"key": "photo", "kind": "image"},
                {"key": "ingredients", "kind": "list"}
            ],
            "view": {"layout": "book", "title_field": "missing", "image_field": "ingredients", "lead_minutes": [5000, 10, 10]},
            "theme": {"accent": "#ab12cd", "paper": "red", "radius": 90}
        })),
        1,
    )
    .unwrap();
    let keys: Vec<&str> = def.fields.iter().map(|f| f.key.as_str()).collect();
    assert_eq!(keys, ["recipe_name", "photo", "ingredients"]);
    assert_eq!(def.view.title_field.as_deref(), Some("recipe_name"));
    assert_eq!(
        def.view.image_field.as_deref(),
        Some("photo"),
        "a list cannot be the image"
    );
    assert_eq!(def.view.lead_minutes, [1440, 10]);
    assert_eq!(def.theme.accent, "#ab12cd");
    assert_eq!(def.theme.paper, "");
    assert_eq!(def.theme.radius, 28);
}

#[test]
fn a_book_keeps_only_a_style_it_draws() {
    let style = |layout: &str, style: &str| {
        normalize(blueprint(json!({"name": "B", "view": {"layout": layout, "style": style}})), 1)
            .unwrap()
            .view
            .style
    };
    assert_eq!(style("book", " Cookbook "), "cookbook");
    assert_eq!(style("book", "journal"), "journal");
    assert_eq!(style("book", "scroll"), "");
    assert_eq!(style("cards", "cookbook"), "", "only a book has styles");
}

#[test]
fn normalize_refuses_bad_blueprints() {
    assert!(normalize(blueprint(json!({"name": " "})), 1).is_err());
    assert!(
        normalize(
            blueprint(json!({"name": "X", "view": {"layout": "custom"}})),
            1
        )
        .is_err()
    );
    assert!(
        normalize(
            blueprint(
                json!({"name": "X", "view": {"layout": "custom"}, "customHtml": "<p>hi</p>"})
            ),
            1
        )
        .is_ok()
    );
}

#[test]
fn sanitize_shapes_each_kind_and_drops_unknown_fields() {
    let def = normalize(
        blueprint(json!({"name": "T", "fields": [
            {"key": "title"}, {"key": "steps", "kind": "steps"}, {"key": "stars", "kind": "rating"},
            {"key": "pic", "kind": "image"}, {"key": "on", "kind": "toggle"}, {"key": "n", "kind": "number"}
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
            "other": "dropped"
        })
        .as_object()
        .unwrap(),
    );
    assert_eq!(
        Value::Object(out),
        json!({"title": "Soup", "steps": ["Chop", "Boil", "Serve"], "stars": 5, "on": true, "n": 2.5})
    );
}

#[test]
fn store_round_trip_with_images_and_patches() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(Some(dir.path().join("types")));
    assert!(store.list().unwrap().is_empty());
    let def = store
        .create(blueprint(json!({"name": "Cookbook", "icon": "🍳", "fields": [
            {"key": "title"}, {"key": "photo", "kind": "image"}, {"key": "ingredients", "kind": "list"}
        ], "view": {"layout": "book"}})))
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
                done_on: Some("2025-01-02".into()),
                ..ItemPatch::default()
            },
        )
        .unwrap();
    assert_eq!(patched.fields.get("ingredients"), None);
    assert_eq!(patched.fields["title"], "Leek soup");
    assert_eq!(patched.done_on.as_deref(), Some("2025-01-02"));

    let second = store.get(&def.id).unwrap().items[1].id.clone();
    store.move_item(&def.id, &second, 0).unwrap();
    assert_eq!(store.get(&def.id).unwrap().items[0].id, second);

    let restyled = store
        .update(&def.id, &json!({"theme": {"paper": "#fff8ee"}, "view": {"tagline": "Family recipes"}, "id": "hijack"}))
        .unwrap();
    assert_eq!(restyled.id, def.id);
    assert_eq!(restyled.view.layout, Layout::Book, "view members merge");
    assert_eq!(restyled.theme.paper, "#fff8ee");
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
fn a_named_type_is_undesigned_and_its_sessions_are_replaced_whole() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(Some(dir.path().to_path_buf()));
    let def = store.create(blueprint(json!({"name": "Cookbook"}))).unwrap();
    assert!(!def.designed);
    assert!(describe(&def, 0).contains("NOT DESIGNED YET"));

    store
        .update(&def.id, &json!({"sessions": {"/a": "s1", "/b": "s2"}}))
        .unwrap();
    let def = store
        .update(&def.id, &json!({"sessions": {"/b": "s3"}}))
        .unwrap();
    assert_eq!(def.sessions.len(), 1);
    assert_eq!(def.sessions["/b"], "s3");

    let def = store
        .update(&def.id, &json!({"designed": true, "view": {"layout": "book"}}))
        .unwrap();
    assert!(def.designed);
    assert_eq!(def.sessions["/b"], "s3", "a design change keeps the sessions");
    let summary = &store.list().unwrap()[0];
    assert!(summary.designed);
    assert_eq!(summary.sessions, ["s3"]);
    assert!(!describe(&def, 0).contains("NOT DESIGNED"));
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
fn custom_pages_are_served_only_for_custom_layouts() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(Some(dir.path().to_path_buf()));
    let custom = store
        .create(blueprint(
            json!({"name": "Mine", "view": {"layout": "custom"}, "customHtml": "<p>hi</p>"}),
        ))
        .unwrap();
    let book = store
        .create(blueprint(
            json!({"name": "Book", "view": {"layout": "book"}}),
        ))
        .unwrap();
    let page = custom_page(&store, &format!("/{}?v=3", custom.id));
    assert_eq!(page.status(), 200);
    assert_eq!(page.body(), b"<p>hi</p>");
    assert_eq!(page.headers()["Content-Security-Policy"], CUSTOM_CSP);
    assert_eq!(custom_page(&store, &format!("/{}", book.id)).status(), 404);
    assert_eq!(custom_page(&store, "/../x").status(), 404);
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
        .create(blueprint(json!({"name": "Moodboard", "view": {"layout": "board"}})))
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
            "name": "Image board", "icon": "🖼", "description": "Pictures that set a mood",
            "view": {"layout": "board"}
        })))
        .unwrap();
    let sent = with_context(&store, &format!("[[type:{}]]\nmake it warmer", def.id));
    assert!(sent.starts_with(&format!("[[type:{}]]\n{CONTEXT_OPEN}", def.id)));
    assert!(sent.contains("Image board") && sent.contains("Pictures that set a mood"));
    assert!(sent.contains("board layout") && sent.contains("not about the project's code"));
    assert!(sent.ends_with("make it warmer"));
    // Already briefed, plain, or about an unknown type: unchanged.
    assert_eq!(with_context(&store, &sent), sent);
    assert_eq!(with_context(&store, "fix the build"), "fix the build");
    let unknown = "[[type:nope-1234]]\nhello";
    assert_eq!(with_context(&store, unknown), unknown);
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
