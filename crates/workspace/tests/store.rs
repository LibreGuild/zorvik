//! Workspace file operations on a real temp directory.

use zorvik_formats::{
    Auth, Environment, FolderMeta, ImportedCollection, ImportedItem, KeyValue, NodeKind, Request, RequestKind,
    TreeNode, Variable,
};
use zorvik_workspace::{ErrorCode, Workspace};

fn names(nodes: &[TreeNode]) -> Vec<String> {
    nodes.iter().map(|n| n.name.clone()).collect()
}

fn req(name: &str) -> Request {
    let mut r = Request::new(name, RequestKind::Http);
    r.url = "https://example.com".into();
    r
}

#[test]
fn create_open_and_tree_order() {
    let dir = tempfile::tempdir().unwrap();
    let ws = Workspace::create(dir.path(), "Demo").unwrap();
    assert!(Workspace::create(dir.path(), "Again").is_err());
    let reopened = Workspace::open(dir.path()).unwrap();
    assert_eq!(reopened.meta().name, "Demo");
    assert_eq!(reopened.meta().id, ws.meta().id);

    let a = ws.create_request("", req("Zeta")).unwrap();
    let folder = ws.create_folder("", "Users").unwrap();
    let b = ws.create_request(&folder, req("List users")).unwrap();
    let c = ws.create_request("", req("alpha")).unwrap();
    assert_eq!(a, "Zeta.yaml");
    assert_eq!(b, "Users/List users.yaml");
    let tree = ws.tree().unwrap();
    // Creation order (seq), not alphabetical.
    assert_eq!(names(&tree), ["Zeta", "Users", "alpha"]);
    assert_eq!(tree[1].kind, NodeKind::Folder);
    assert_eq!(names(&tree[1].children), ["List users"]);
    assert_eq!(ws.read_request(&c).unwrap().name, "alpha");

    let not_ws = tempfile::tempdir().unwrap();
    assert_eq!(Workspace::open(not_ws.path()).unwrap_err().code, ErrorCode::NotAWorkspace);
}

#[test]
fn unsafe_paths_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let ws = Workspace::create(dir.path(), "Demo").unwrap();
    for bad in ["../zorvik.yaml", "a/../../x.yaml", "C:\\x.yaml", "..", "a\\..\\b.yaml", "a/./b.yaml"] {
        assert!(ws.read_request(bad).is_err(), "{bad}");
        assert!(ws.resolve_path(bad).is_err(), "{bad}");
    }
    assert!(ws.resolve_path("../x").is_err());
    // A leading slash is still relative to requests/.
    assert!(ws.resolve_path("/etc/passwd").unwrap().starts_with(dir.path().join("requests")));
    assert_eq!(ws.read_request("missing.yaml").unwrap_err().code, ErrorCode::NotFound);
    assert!(ws.read_request("_folder.yaml").is_err());
}

#[test]
fn names_are_sanitized_and_unique() {
    let dir = tempfile::tempdir().unwrap();
    let ws = Workspace::create(dir.path(), "Demo").unwrap();
    let a = ws.create_request("", req("Get: user?")).unwrap();
    let b = ws.create_request("", req("get: USER?")).unwrap();
    assert_eq!(a, "Get- user-.yaml");
    assert_eq!(b, "get- USER- 2.yaml");
    assert_eq!(ws.read_request(&b).unwrap().name, "get: USER?");
    assert!(ws.create_request("", req("  ")).is_err());
    assert!(ws.create_request("nope", req("x")).is_err());
}

#[test]
fn rename_duplicate_move_and_reorder() {
    let dir = tempfile::tempdir().unwrap();
    let ws = Workspace::create(dir.path(), "Demo").unwrap();
    let one = ws.create_request("", req("One")).unwrap();
    let two = ws.create_request("", req("Two")).unwrap();
    let folder = ws.create_folder("", "Folder").unwrap();

    // Case-only rename works on case-insensitive file systems.
    let one = ws.rename(&one, "ONE").unwrap();
    assert_eq!(one, "ONE.yaml");
    assert_eq!(ws.read_request(&one).unwrap().name, "ONE");

    let copy = ws.duplicate(&two).unwrap();
    assert_eq!(ws.read_request(&copy).unwrap().name, "Two copy");

    // Move into folder, then reorder to the front of root.
    let moved = ws.move_item(&two, &folder, None).unwrap();
    assert_eq!(moved, "Folder/Two.yaml");
    let back = ws.move_item(&moved, "", Some(0)).unwrap();
    assert_eq!(back, "Two.yaml");
    assert_eq!(names(&ws.tree().unwrap()), ["Two", "ONE", "Folder", "Two copy"]);

    // Reorder within the same parent.
    ws.move_item(&copy, "", Some(1)).unwrap();
    assert_eq!(names(&ws.tree().unwrap()), ["Two", "Two copy", "ONE", "Folder"]);

    // Folder into itself is refused; folder rename keeps children.
    let sub = ws.create_folder(&folder, "Sub").unwrap();
    assert!(ws.move_item(&folder, &sub, None).is_err());
    ws.create_request(&sub, req("Deep")).unwrap();
    let renamed = ws.rename(&folder, "Renamed").unwrap();
    assert_eq!(renamed, "Renamed");
    assert_eq!(ws.read_request("Renamed/Sub/Deep.yaml").unwrap().name, "Deep");
    let dup = ws.duplicate(&renamed).unwrap();
    assert_eq!(ws.read_folder(&dup).unwrap().name, "Renamed copy");
    assert!(ws.read_request(&format!("{dup}/Sub/Deep.yaml")).is_ok());
}

#[test]
fn folder_meta_and_inheritance_chain() {
    let dir = tempfile::tempdir().unwrap();
    let ws = Workspace::create(dir.path(), "Demo").unwrap();
    let a = ws.create_folder("", "A").unwrap();
    let b = ws.create_folder(&a, "B").unwrap();
    let mut meta = ws.read_folder(&a).unwrap();
    meta.auth = Auth::Bearer { token: "t".into(), prefix: "Bearer".into() };
    meta.headers = vec![KeyValue::new("X-A", "1")];
    ws.save_folder(&a, &meta).unwrap();
    let r = ws.create_request(&b, req("R")).unwrap();
    let chain = ws.ancestors(&r);
    assert_eq!(chain.len(), 2);
    assert_eq!(chain[0].name, "A");
    assert!(matches!(chain[0].auth, Auth::Bearer { .. }));
    assert_eq!(chain[1].name, "B");
    assert!(ws.save_folder("", &FolderMeta::default()).is_err());
}

#[test]
fn broken_files_show_up_with_errors() {
    let dir = tempfile::tempdir().unwrap();
    let ws = Workspace::create(dir.path(), "Demo").unwrap();
    std::fs::write(dir.path().join("requests/broken.yaml"), "name: [unclosed").unwrap();
    std::fs::write(dir.path().join("requests/notes.txt"), "ignored").unwrap();
    std::fs::write(dir.path().join("requests/.hidden.yaml"), "name: x").unwrap();
    let tree = ws.tree().unwrap();
    assert_eq!(tree.len(), 1);
    assert!(tree[0].error.is_some());
    assert_eq!(tree[0].name, "broken");
}

#[test]
fn environments_crud() {
    let dir = tempfile::tempdir().unwrap();
    let ws = Workspace::create(dir.path(), "Demo").unwrap();
    let env = Environment {
        name: "Dev".into(),
        variables: vec![Variable {
            key: "base".into(),
            value: "http://localhost".into(),
            enabled: true,
            secret: false,
        }],
    };
    let id = ws.create_environment(&env).unwrap();
    assert_eq!(id, "Dev");
    let id2 = ws.create_environment(&Environment { name: "dev".into(), variables: vec![] }).unwrap();
    assert_eq!(id2, "dev 2");
    let renamed = ws.save_environment(&id, &Environment { name: "Production".into(), ..env.clone() }).unwrap();
    assert_eq!(renamed, "Production");
    let list = ws.list_environments().unwrap();
    assert_eq!(list.iter().map(|e| e.environment.name.as_str()).collect::<Vec<_>>(), ["dev", "Production"]);
    assert_eq!(ws.read_environment("Production").unwrap().variables.len(), 1);
    assert!(ws.read_environment("../zorvik").is_err());
}

#[test]
fn yaml_is_readable_and_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let ws = Workspace::create(dir.path(), "Demo").unwrap();
    let mut r = req("Create user");
    r.method = "POST".into();
    r.body = zorvik_formats::Body {
        body_type: zorvik_formats::BodyType::Json,
        text: "{\n  \"a\": 1\n}".into(),
        ..Default::default()
    };
    let path = ws.create_request("", r.clone()).unwrap();
    let text = std::fs::read_to_string(dir.path().join("requests").join(&path)).unwrap();
    assert!(text.contains("method: POST"), "{text}");
    assert!(!text.contains("auth:"), "defaults are omitted: {text}");
    let back = ws.read_request(&path).unwrap();
    assert_eq!(back.body, r.body);
}

#[test]
fn websocket_options_are_short_yaml() {
    use zorvik_formats::{ConnectStep, MatchKind, MessageFormat, RepeatStep, SavedMessage, WaitStep};
    let dir = tempfile::tempdir().unwrap();
    let ws = Workspace::create(dir.path(), "Demo").unwrap();
    let mut r = req("Chat");
    r.kind = zorvik_formats::RequestKind::Websocket;
    r.websocket.subprotocols = vec!["chat.v1".into()];
    r.websocket.messages = vec![
        SavedMessage {
            name: "Login".into(),
            format: MessageFormat::Json,
            text: "{\"token\": \"{{token}}\"}".into(),
            ..Default::default()
        },
        SavedMessage { name: "Ping".into(), format: MessageFormat::Text, text: "ping".into(), ..Default::default() },
    ];
    r.websocket.on_connect = vec![
        ConnectStep::Send { message: "Login".into() },
        ConnectStep::Wait(WaitStep {
            matcher: MatchKind::Json,
            pattern: "$.type == welcome".into(),
            ..Default::default()
        }),
        ConnectStep::Repeat(RepeatStep { message: "Ping".into(), every_ms: 30000 }),
    ];
    let path = ws.create_request("", r.clone()).unwrap();
    let text = std::fs::read_to_string(dir.path().join("requests").join(&path)).unwrap();
    for line in [
        "  - type: send\n    message: Login",
        "    match: json",
        "    pattern: $.type == welcome",
        "    everyMs: 30000",
        "  onConnect:",
    ] {
        assert!(text.contains(line), "{line} in:\n{text}");
    }
    // Defaults stay out: the text format and the 5 s wait.
    assert!(!text.contains("format: text") && !text.contains("timeoutMs"), "{text}");
    assert_eq!(ws.read_request(&path).unwrap().websocket, r.websocket);
}

#[test]
fn delete_moves_to_trash() {
    // Opt-in: this really moves a file to the OS trash (CI sets it).
    if std::env::var("ZORVIK_TEST_TRASH").map_or(true, |v| v.is_empty()) {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let ws = Workspace::create(dir.path(), "Demo").unwrap();
    let p = ws.create_request("", req("Temp")).unwrap();
    ws.delete(&p).unwrap();
    assert!(ws.tree().unwrap().is_empty());
    assert_eq!(ws.delete(&p).unwrap_err().code, ErrorCode::NotFound);
}

#[cfg(unix)]
#[test]
fn symlinks_are_not_followed() {
    use std::os::unix::fs::symlink;
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret.yaml"), "name: Secret").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let ws = Workspace::create(dir.path(), "Demo").unwrap();
    let requests = dir.path().join("requests");
    ws.create_request("", req("Real")).unwrap();
    // Two self-loops used to make the tree walk branch exponentially (hang / out of memory).
    symlink(&requests, requests.join("loop1")).unwrap();
    symlink(&requests, requests.join("loop2")).unwrap();
    symlink(outside.path(), requests.join("outside")).unwrap();
    symlink(outside.path().join("secret.yaml"), requests.join("linked.yaml")).unwrap();
    assert_eq!(names(&ws.tree().unwrap()), ["Real"]);
    for bad in ["outside/secret.yaml", "linked.yaml", "loop1/Real.yaml"] {
        assert!(ws.read_request(bad).is_err(), "{bad}");
    }
    assert!(ws.create_request("outside", req("x")).is_err());
    assert!(ws.save_request("linked.yaml", &req("x")).is_err());
    assert!(ws.ancestors("outside/secret.yaml").is_empty());
    assert!(!outside.path().join("x.yaml").exists());

    // Duplicating a folder never copies what a link points to.
    let folder = ws.create_folder("", "F").unwrap();
    symlink(outside.path().join("secret.yaml"), requests.join("F/key.yaml")).unwrap();
    let dup = ws.duplicate(&folder).unwrap();
    assert!(!requests.join(&dup).join("key.yaml").exists());

    // Linked environment files and zorvik.yaml are refused too.
    std::fs::write(outside.path().join("env.yaml"), "name: Stolen").unwrap();
    symlink(outside.path().join("env.yaml"), dir.path().join("environments/Stolen.yaml")).unwrap();
    assert!(ws.list_environments().unwrap().is_empty());
    assert!(ws.read_environment("Stolen").is_err());
    let envs = dir.path().join("environments");
    std::fs::remove_dir_all(&envs).unwrap();
    symlink(outside.path(), &envs).unwrap();
    assert!(ws.create_environment(&Environment { name: "Dev".into(), variables: vec![] }).is_err());
    assert!(!outside.path().join("Dev.yaml").exists());
    let other = tempfile::tempdir().unwrap();
    std::fs::create_dir(other.path().join("requests")).unwrap();
    symlink(dir.path().join("zorvik.yaml"), other.path().join("zorvik.yaml")).unwrap();
    assert!(Workspace::open(other.path()).is_err());
}

#[test]
fn unsafe_workspace_ids_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::create(dir.path(), "Demo").unwrap();
    let meta_path = dir.path().join("zorvik.yaml");
    // The id names the local cookie file, so it must not be a path.
    for bad in ["../../settings", "a/b", "CON", "x y"] {
        std::fs::write(&meta_path, format!("version: 1\nid: '{bad}'\nname: Demo\n")).unwrap();
        assert!(Workspace::open(dir.path()).is_err(), "{bad}");
        assert!(ws.reload_meta().is_err(), "{bad}");
    }
    std::fs::write(&meta_path, "version: 99\nid: abc\nname: Demo\n").unwrap();
    assert!(ws.reload_meta().is_err());
    std::fs::write(&meta_path, "version: 1\nid: ''\nname: Demo\n").unwrap();
    ws.reload_meta().unwrap();
    assert!(!ws.meta().id.is_empty());
    assert_eq!(Workspace::open(dir.path()).unwrap().meta().id, ws.meta().id);
}

#[test]
fn item_operations_only_touch_requests_and_folders() {
    let dir = tempfile::tempdir().unwrap();
    let ws = Workspace::create(dir.path(), "Demo").unwrap();
    let folder = ws.create_folder("", "F").unwrap();
    std::fs::write(dir.path().join("requests/ab"), "name: x").unwrap();
    std::fs::write(dir.path().join("requests/日本.txt"), "name: x").unwrap();
    // These used to panic slicing off a ".yaml" the name did not have.
    assert!(ws.move_item("ab", &folder, None).is_err());
    assert!(ws.move_item("日本.txt", &folder, None).is_err());
    assert!(ws.rename("F/_folder.yaml", "x").is_err());
    assert!(ws.duplicate("ab").is_err());
    assert!(ws.read_folder(&folder).is_ok());
    for bad in ["...", ". ", "a/ /b"] {
        assert!(ws.resolve_path(bad).is_err(), "{bad}");
    }

    // Upper-case extensions are listed and can be opened.
    std::fs::write(dir.path().join("requests/Upper.YAML"), "name: Upper").unwrap();
    let tree = ws.tree().unwrap();
    let upper = tree.iter().find(|n| n.name == "Upper").unwrap();
    assert_eq!(ws.read_request(&upper.path).unwrap().name, "Upper");
}

#[test]
fn reordering_keeps_broken_folder_files_and_saves_keep_disk_order() {
    let dir = tempfile::tempdir().unwrap();
    let ws = Workspace::create(dir.path(), "Demo").unwrap();
    let a = ws.create_folder("", "A").unwrap();
    let b = ws.create_folder("", "B").unwrap();
    let r = ws.create_request("", req("R")).unwrap();
    // A merge conflict in A's settings must survive a drag-and-drop in the same folder.
    let conflicted = "<<<<<<< HEAD\nname: A\n=======\nname: A2\n>>>>>>> branch\n";
    std::fs::write(dir.path().join("requests/A/_folder.yaml"), conflicted).unwrap();
    ws.move_item(&r, "", Some(0)).unwrap();
    assert_eq!(std::fs::read_to_string(dir.path().join("requests/A/_folder.yaml")).unwrap(), conflicted);
    std::fs::write(dir.path().join("requests/A/_folder.yaml"), "name: A\nseq: 5\n").unwrap();

    // Saving folder settings from a stale copy does not undo a reorder.
    let stale = ws.read_folder(&b).unwrap();
    ws.move_item(&b, "", Some(0)).unwrap();
    let moved_seq = ws.read_folder(&b).unwrap().seq;
    ws.save_folder(&b, &FolderMeta { docs: "notes".into(), ..stale }).unwrap();
    let saved = ws.read_folder(&b).unwrap();
    assert_eq!((saved.seq, saved.docs.as_str()), (moved_seq, "notes"));
    assert_eq!(names(&ws.tree().unwrap())[0], "B");
    assert!(ws.read_folder(&a).is_ok());
}

#[test]
fn request_named_like_folder_file_is_kept_apart() {
    let dir = tempfile::tempdir().unwrap();
    let ws = Workspace::create(dir.path(), "Demo").unwrap();
    std::fs::create_dir(dir.path().join("requests/Plain")).unwrap();
    // On case-insensitive disks `_Folder.yaml` would be the folder settings file.
    let p = ws.create_request("Plain", req("_Folder")).unwrap();
    assert_eq!(p, "Plain/untitled.yaml");
    ws.save_folder("Plain", &FolderMeta { name: "Plain".into(), ..Default::default() }).unwrap();
    assert_eq!(ws.read_request(&p).unwrap().name, "_Folder");
}

#[test]
fn import_keeps_order_and_nesting() {
    let dir = tempfile::tempdir().unwrap();
    let ws = Workspace::create(dir.path(), "Demo").unwrap();
    ws.create_request("", req("Existing")).unwrap();
    let items = vec![
        ImportedItem::Request(req("Zed")),
        ImportedItem::Folder {
            meta: FolderMeta { name: "Sub".into(), headers: vec![KeyValue::new("X-Sub", "1")], ..Default::default() },
            children: (0..50).map(|_| ImportedItem::Request(req("Same"))).collect(),
        },
        ImportedItem::Request(req("")),
    ];
    let collection = ImportedCollection {
        name: "API".into(),
        items,
        auth: Auth::Bearer { token: "t".into(), prefix: "Bearer".into() },
        headers: vec![KeyValue::new("X-Api", "1")],
        scripts: zorvik_formats::Scripts { pre_request: "pm.variables.set('a', 1);".into(), ..Default::default() },
        variables: vec![],
        workspace_variables: vec![],
        warnings: vec![],
        docs: "What the API does.".into(),
    };
    let summary = ws.write_imported("", &collection).unwrap();
    assert_eq!((summary.requests, summary.folders), (52, 2));
    let tree = ws.tree().unwrap();
    assert_eq!(names(&tree), ["Existing", "API"]);
    assert_eq!(names(&tree[1].children), ["Zed", "Sub", "Untitled request"]);
    assert_eq!(tree[1].children[1].children.len(), 50);
    assert_eq!(tree[1].children[1].children[49].path, "API/Sub/Same 50.yaml");
    let api = ws.read_folder("API").unwrap();
    assert!(matches!(api.auth, Auth::Bearer { .. }));
    assert_eq!(api.headers[0].key, "X-Api");
    assert_eq!(api.scripts.pre_request, "pm.variables.set('a', 1);");
    assert_eq!(api.docs, "What the API does.");
    assert_eq!(ws.read_folder("API/Sub").unwrap().headers[0].key, "X-Sub");
}

#[test]
fn imported_names_that_are_sentences_are_cut_short() {
    let dir = tempfile::tempdir().unwrap();
    let ws = Workspace::create(dir.path(), "Demo").unwrap();
    let sentence = "Retrieve the deferred completion for a request that was sent earlier, ".repeat(4);
    let collection = ImportedCollection {
        name: "API".into(),
        items: vec![
            ImportedItem::Request(req(&sentence)),
            ImportedItem::Folder { meta: FolderMeta { name: "x".repeat(300), ..Default::default() }, children: vec![] },
        ],
        ..Default::default()
    };
    ws.write_imported("", &collection).unwrap();
    let tree = ws.tree().unwrap();
    let names = names(&tree[0].children);
    assert!(names.iter().all(|n| n.chars().count() <= 120 && n.ends_with('…')), "{names:?}");
    assert!(names[0].starts_with("Retrieve the deferred completion") && !names[0].ends_with(" …"), "{names:?}");
}

// Unix only: paths this long need special handling on Windows, where Git
// cannot check such a tree out by default either.
#[cfg(unix)]
#[test]
fn very_deep_folders_do_not_overflow_the_stack() {
    let dir = tempfile::tempdir().unwrap();
    let ws = Workspace::create(dir.path(), "Demo").unwrap();
    let mut p = dir.path().join("requests");
    for _ in 0..200 {
        p.push("a");
    }
    std::fs::create_dir_all(&p).unwrap();
    // Uncapped, 200 levels need more than 1 MB of stack in a debug build (and
    // ~2000 levels are possible on Linux); tokio workers have 2 MB.
    let depth = std::thread::Builder::new()
        .stack_size(1024 * 1024)
        .spawn(move || {
            let tree = ws.tree().unwrap();
            let mut depth = 0;
            let mut level = &tree;
            while let Some(node) = level.first() {
                depth += 1;
                level = &node.children;
            }
            depth
        })
        .unwrap()
        .join()
        .unwrap();
    assert!((2..=40).contains(&depth), "{depth}");
}

#[test]
fn servers_crud_order_and_broken_files() {
    use zorvik_formats::{Server, ServerKind};
    let dir = tempfile::tempdir().unwrap();
    let ws = Workspace::create(dir.path(), "Demo").unwrap();
    assert!(ws.list_servers().unwrap().is_empty());

    let mock = ws.create_server(&Server::new("Payments mock", ServerKind::Http)).unwrap();
    let echo = ws.create_server(&Server::new("Echo: tcp?", ServerKind::Tcp)).unwrap();
    assert_eq!((mock.as_str(), echo.as_str()), ("Payments mock", "Echo- tcp-"));
    let list = ws.list_servers().unwrap();
    assert_eq!(
        list.iter().map(|n| (n.name.as_str(), n.seq)).collect::<Vec<_>>(),
        [("Payments mock", 0), ("Echo: tcp?", 1)]
    );
    assert_eq!(list[1].kind, ServerKind::Tcp);

    // Rename follows the file; reorder; duplicate.
    let mut server = ws.read_server(&echo).unwrap();
    server.name = "Echo".into();
    let echo = ws.save_server(&echo, &server).unwrap();
    assert_eq!(echo, "Echo");
    ws.reorder_servers(&[echo.clone(), mock.clone()]).unwrap();
    let copy = ws.duplicate_server(&mock).unwrap();
    let names: Vec<_> = ws.list_servers().unwrap().into_iter().map(|n| n.name).collect();
    assert_eq!(names, ["Echo", "Payments mock", "Payments mock copy"]);
    assert!(ws.read_server(&copy).is_ok());

    // A broken file is listed with its error; bad ids are refused.
    std::fs::write(dir.path().join("servers/Broken.yaml"), "name: [").unwrap();
    let broken = ws.list_servers().unwrap().into_iter().find(|n| n.id == "Broken").unwrap();
    assert!(broken.error.is_some());
    assert!(ws.read_server("../zorvik").is_err());
    assert_eq!(ws.read_server("Missing").unwrap_err().code, ErrorCode::NotFound);
}

#[test]
fn server_files_too_big_to_read_back_are_refused() {
    use zorvik_formats::{MockRoute, Server, ServerKind};
    let dir = tempfile::tempdir().unwrap();
    let ws = Workspace::create(dir.path(), "Demo").unwrap();
    let mut server = Server::new("Huge", ServerKind::Http);
    server.http.routes.push(MockRoute { body: "a".repeat(51 << 20), ..Default::default() });
    let err = ws.create_server(&server).unwrap_err();
    assert!(err.message.contains("larger than 50 MB"), "{}", err.message);
    assert!(ws.list_servers().unwrap().is_empty(), "nothing was written");
}

#[test]
fn server_fingerprints_ignore_order_and_auto_start() {
    use zorvik_formats::{Server, ServerKind};
    use zorvik_workspace::store::server_fingerprint;
    let server = Server::new("Echo", ServerKind::Tcp);
    let print = server_fingerprint(&server);
    assert_eq!(print.len(), 64);
    assert_eq!(server_fingerprint(&Server { seq: 7, auto_start: true, ..server.clone() }), print);
    assert_ne!(server_fingerprint(&Server { host: "0.0.0.0".into(), ..server.clone() }), print);
    assert_ne!(server_fingerprint(&Server { port: 9999, ..server }), print);
}

#[test]
fn load_tests_crud_and_order() {
    use zorvik_formats::LoadTest;
    let dir = tempfile::tempdir().unwrap();
    let ws = Workspace::create(dir.path(), "Demo").unwrap();
    assert!(ws.list_load_tests().unwrap().is_empty());
    let a = ws.create_load_test(&LoadTest::new("Smoke", vec!["a.yaml".into()])).unwrap();
    let b = ws.create_load_test(&LoadTest::new("Soak: 1h", vec![])).unwrap();
    assert_eq!((a.as_str(), b.as_str()), ("Smoke", "Soak- 1h"));
    let list = ws.list_load_tests().unwrap();
    assert_eq!(
        list.iter().map(|n| (n.name.as_str(), n.targets, n.duration_secs)).collect::<Vec<_>>(),
        [("Smoke", 1, 60), ("Soak: 1h", 0, 60)]
    );

    let mut test = ws.read_load_test(&a).unwrap();
    test.name = "Smoke test".into();
    let a = ws.save_load_test(&a, &test).unwrap();
    assert_eq!(a, "Smoke test");
    ws.reorder_load_tests(&[b.clone(), a.clone()]).unwrap();
    let copy = ws.duplicate_load_test(&a).unwrap();
    let names: Vec<_> = ws.list_load_tests().unwrap().into_iter().map(|n| n.name).collect();
    assert_eq!(names, ["Soak: 1h", "Smoke test", "Smoke test copy"]);
    ws.delete_load_test(&copy).ok(); // the OS trash may be unavailable in CI
    assert!(ws.read_load_test("../zorvik").is_err());
    std::fs::write(dir.path().join("loadtests/Broken.yaml"), "targets: [").unwrap();
    assert!(ws.list_load_tests().unwrap().iter().any(|n| n.id == "Broken" && n.error.is_some()));
}

#[test]
fn scenarios_crud_and_order() {
    use zorvik_formats::{Scenario, Step, StepAction, StepParallel, StepValue};
    let dir = tempfile::tempdir().unwrap();
    let ws = Workspace::create(dir.path(), "Demo").unwrap();
    assert!(ws.list_scenarios().unwrap().is_empty());
    let a = ws.create_scenario("", &Scenario::new("Checkout", vec!["a.yaml".into(), "b.yaml".into()])).unwrap();
    let b = ws.create_scenario("", &Scenario::new("Sign up: new user", vec![])).unwrap();
    assert_eq!((a.as_str(), b.as_str()), ("Checkout", "Sign up- new user"));
    let list = ws.list_scenarios().unwrap();
    assert_eq!(
        list.iter().map(|n| (n.name.as_str(), n.steps)).collect::<Vec<_>>(),
        [("Checkout", 2), ("Sign up: new user", 0)]
    );

    let mut scenario = ws.read_scenario(&a).unwrap();
    scenario.name = "Checkout flow".into();
    scenario.docs = "Buys one thing.".into();
    let a = ws.save_scenario(&a, &scenario).unwrap();
    assert_eq!(a, "Checkout flow");
    assert_eq!(ws.read_scenario(&a).unwrap().docs, "Buys one thing.");
    ws.move_scenario(&b, zorvik_workspace::ScenarioItem::Scenario, "", Some(0)).unwrap();
    ws.duplicate_scenario(&a).unwrap();
    let names: Vec<_> = ws.list_scenarios().unwrap().into_iter().map(|n| n.name).collect();
    assert_eq!(names, ["Sign up: new user", "Checkout flow", "Checkout flow copy"]);
    assert!(ws.read_scenario("../zorvik").is_err());

    // What can't be used is refused when saved, and listed with why when it comes from a file.
    scenario.variables = vec![StepValue::new("two words", "1")];
    assert!(ws.save_scenario(&a, &scenario).unwrap_err().message.contains("two words"));
    std::fs::write(dir.path().join("scenarios/Broken.yaml"), "steps: [").unwrap();
    let mut deep = Step::request("a.yaml");
    for _ in 0..40 {
        deep = Step::new(StepAction::Parallel(StepParallel { steps: vec![deep] }));
    }
    let deep = Scenario { steps: vec![deep], ..Scenario::new("Deep", vec![]) };
    std::fs::write(dir.path().join("scenarios/Deep.yaml"), serde_yaml_ng::to_string(&deep).unwrap()).unwrap();
    let listed = ws.list_scenarios().unwrap();
    assert!(listed.iter().any(|n| n.id == "Broken" && n.error.is_some()));
    assert!(listed.iter().any(|n| n.id == "Deep" && n.error.as_deref().is_some_and(|e| e.contains("nested"))));
    assert!(ws.read_scenario("Deep").is_err());
}

#[test]
fn scenarios_in_folders() {
    use zorvik_formats::{Scenario, Step, StepAction, StepCall};
    use zorvik_workspace::ScenarioItem;
    let dir = tempfile::tempdir().unwrap();
    let ws = Workspace::create(dir.path(), "Demo").unwrap();
    ws.create_scenario("", &Scenario::new("Smoke", vec![])).unwrap();
    let shop = ws.create_scenario_folder("", "Shop: EU").unwrap();
    let cart = ws.create_scenario_folder(&shop, "Cart").unwrap();
    assert_eq!((shop.as_str(), cart.as_str()), ("Shop- EU", "Shop- EU/Cart"));
    let login = ws.create_scenario(&shop, &Scenario::new("Log in", vec!["a.yaml".into()])).unwrap();
    let mut checkout = Scenario::new("Checkout", vec![]);
    checkout.steps = vec![Step::new(StepAction::Scenario(StepCall { scenario: login.clone(), ..Default::default() }))];
    let checkout = ws.create_scenario(&cart, &checkout).unwrap();
    assert_eq!((login.as_str(), checkout.as_str()), ("Shop- EU/Log in", "Shop- EU/Cart/Checkout"));
    assert!(dir.path().join("scenarios/Shop- EU/Cart/Checkout.yaml").is_file());
    assert_eq!(ws.read_scenario(&checkout).unwrap().calls(), [login.as_str()]);

    // The tree: folders with their names and what is in them, in order; the flat list, the
    // scenarios only.
    let shape = |nodes: &[zorvik_formats::ScenarioNode]| -> Vec<String> {
        fn go(nodes: &[zorvik_formats::ScenarioNode], depth: usize, out: &mut Vec<String>) {
            for n in nodes {
                out.push(format!("{}{}{} ({})", "  ".repeat(depth), n.name, if n.folder { "/" } else { "" }, n.id));
                go(&n.children, depth + 1, out);
            }
        }
        let mut out = Vec::new();
        go(nodes, 0, &mut out);
        out
    };
    assert_eq!(
        shape(&ws.scenario_tree().unwrap()),
        [
            "Smoke (Smoke)",
            "Shop: EU/ (Shop- EU)",
            "  Cart/ (Shop- EU/Cart)",
            "    Checkout (Shop- EU/Cart/Checkout)",
            "  Log in (Shop- EU/Log in)"
        ]
    );
    let ids: Vec<String> = ws.list_scenarios().unwrap().into_iter().map(|n| n.id).collect();
    assert_eq!(ids, ["Smoke", "Shop- EU/Cart/Checkout", "Shop- EU/Log in"]);
    assert_eq!(ws.scenario_folders().unwrap(), ["Shop- EU", "Shop- EU/Cart"]);

    // Saved under a new name it stays in its folder; a copy goes next to it.
    let mut renamed = ws.read_scenario(&login).unwrap();
    renamed.name = "Sign in".into();
    let login = ws.save_scenario(&login, &renamed).unwrap();
    assert_eq!(login, "Shop- EU/Sign in");
    assert_eq!(ws.duplicate_scenario(&login).unwrap(), "Shop- EU/Sign in copy");

    // Moving: a scenario out to the top, first; a folder into another; never into itself.
    let moved = ws.move_scenario(&login, ScenarioItem::Scenario, "", Some(0)).unwrap();
    assert_eq!(moved, "Sign in");
    let archive = ws.create_scenario_folder("", "Archive").unwrap();
    let cart = ws.move_scenario(&cart, ScenarioItem::Folder, &archive, None).unwrap();
    assert_eq!(cart, "Archive/Cart");
    assert!(ws.read_scenario("Archive/Cart/Checkout").is_ok());
    assert!(ws.move_scenario(&archive, ScenarioItem::Folder, &cart, None).unwrap_err().message.contains("into itself"));
    // Within a folder: its place changes.
    ws.move_scenario(&archive, ScenarioItem::Folder, "", Some(0)).unwrap();
    assert_eq!(
        shape(&ws.scenario_tree().unwrap()),
        [
            "Archive/ (Archive)",
            "  Cart/ (Archive/Cart)",
            "    Checkout (Archive/Cart/Checkout)",
            "Sign in (Sign in)",
            "Smoke (Smoke)",
            "Shop: EU/ (Shop- EU)",
            "  Sign in copy (Shop- EU/Sign in copy)",
        ]
    );

    // Renamed: the folder's name and its file name; made as needed by path.
    let shop = ws.rename_scenario_folder(&shop, "Shop").unwrap();
    assert_eq!(shop, "Shop");
    assert_eq!(ws.ensure_scenario_folder("shop/Payments/Cards").unwrap(), "Shop/Payments/Cards");
    assert_eq!(ws.ensure_scenario_folder("Shop/Payments").unwrap(), "Shop/Payments");
    assert_eq!(ws.ensure_scenario_folder("").unwrap(), "");

    // Ids that climb out, hide or name nothing are refused.
    for bad in ["../zorvik", "Shop/../../zorvik", ".git/config", "Shop/.hidden"] {
        assert!(ws.read_scenario(bad).is_err(), "{bad}");
    }
    assert!(ws.create_scenario("Nope", &Scenario::new("X", vec![])).is_err());
    assert!(ws.rename_scenario_folder("", "X").is_err());
    // A folder written by hand (no `_folder.yaml`) is named after itself.
    std::fs::create_dir_all(dir.path().join("scenarios/By hand")).unwrap();
    std::fs::write(dir.path().join("scenarios/By hand/Probe.yaml"), "name: Probe\nsteps: []\n").unwrap();
    assert!(ws.list_scenarios().unwrap().iter().any(|n| n.id == "By hand/Probe" && n.name == "Probe"));
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(dir.path().join("requests"), dir.path().join("scenarios/Linked")).unwrap();
        assert!(!ws.scenario_tree().unwrap().iter().any(|n| n.id == "Linked"));
        assert!(ws.create_scenario("Linked", &Scenario::new("X", vec![])).is_err());
    }
}

#[test]
fn graphql_requests_are_flagged_in_the_tree() {
    use zorvik_formats::{Body, BodyType, GraphqlBody};
    let dir = tempfile::tempdir().unwrap();
    let ws = Workspace::create(dir.path(), "Demo").unwrap();
    let mut gql = req("Viewer");
    gql.method = "POST".into();
    gql.body = Body {
        body_type: BodyType::Graphql,
        graphql: GraphqlBody { query: "{ viewer { id } }".into(), ..Default::default() },
        ..Default::default()
    };
    ws.create_request("", gql).unwrap();
    ws.create_request("", req("Plain")).unwrap();
    let tree = ws.tree().unwrap();
    assert_eq!(names(&tree), ["Viewer", "Plain"]);
    assert!(tree[0].graphql && !tree[1].graphql);
    let json = serde_json::to_value(&tree[1]).unwrap();
    assert!(json.get("graphql").is_none(), "false is omitted: {json}");
}

#[test]
fn saving_a_scenario_keeps_its_place() {
    use zorvik_formats::Scenario;
    use zorvik_workspace::ScenarioItem;
    let dir = tempfile::tempdir().unwrap();
    let ws = Workspace::create(dir.path(), "Demo").unwrap();
    let a = ws.create_scenario("", &Scenario::new("A", vec![])).unwrap();
    let b = ws.create_scenario("", &Scenario::new("B", vec![])).unwrap();
    // A copy read before the move is saved after it (an open tab, an agent): B stays first.
    let stale = ws.read_scenario(&b).unwrap();
    ws.move_scenario(&b, ScenarioItem::Scenario, "", Some(0)).unwrap();
    ws.save_scenario(&b, &Scenario { docs: "edited".into(), ..stale.clone() }).unwrap();
    ws.update_scenario(&a, &Scenario { seq: 0, ..ws.read_scenario(&a).unwrap() }).unwrap();
    let names: Vec<_> = ws.list_scenarios().unwrap().into_iter().map(|n| n.name).collect();
    assert_eq!(names, ["B", "A"]);
}
