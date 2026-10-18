use gray_core::message::ToolDef;
use gray_plugin::{Manifest, merge_manifests};

fn tool_def(name: &str) -> ToolDef {
    ToolDef::new(name, format!("sidecar tool {name}"), serde_json::json!({}))
}

fn manifest_a() -> Manifest {
    Manifest {
        name: "plugin-a".to_string(),
        version: "0.1.0".to_string(),
        tools: vec![tool_def("read")],
        ..Manifest::default()
    }
}

fn manifest_b() -> Manifest {
    Manifest {
        name: "plugin-b".to_string(),
        version: "0.1.0".to_string(),
        tools: vec![tool_def("read")],
        ..Manifest::default()
    }
}

#[test]
fn later_entry_wins_on_name_conflict() {
    let merged = merge_manifests(vec![manifest_a(), manifest_b()]);
    assert_eq!(merged["read"], "plugin-b");
}

#[test]
fn manifest_result_with_tool_schema_round_trips_def() {
    let v = serde_json::json!({
        "name": "echo",
        "version": "0.1.0",
        "tools": [{"name": "echo", "description": "Echo text back",
                   "parameters": {"type": "object"},
                   "snippet": "echo <text> — echo text back"}],
        "commands": ["/echo"],
        "hooks": ["prompt/context", "tool/before", "turn/end"],
    });
    let m = Manifest::from_result(&v);
    assert_eq!(m.name, "echo");
    assert_eq!(m.commands, vec!["/echo".to_string()]);
    assert_eq!(m.hooks.len(), 3);
    assert_eq!(m.tools.len(), 1);
    let def = &m.tools[0];
    assert_eq!(def.name, "echo");
    assert_eq!(def.description, "Echo text back");
    assert_eq!(def.parameters, serde_json::json!({"type": "object"}));
}

#[test]
fn tool_preview_parses_and_stays_model_invisible() {
    let v = serde_json::json!({
        "name": "x", "version": "0.1.0",
        "tools": [{"name": "send_card", "description": "d",
                   "parameters": {"type": "object"}, "preview": "document.title"}],
    });
    let m = Manifest::from_result(&v);
    assert_eq!(m.tools[0].preview.as_deref(), Some("document.title"));

    let v2 = serde_json::json!({
        "name": "x", "version": "0.1.0",
        "tools": [{"name": "t", "description": "d",
                   "parameters": {"type": "object"}, "preview": "  "}],
    });
    assert!(Manifest::from_result(&v2).tools[0].preview.is_none());

    let def = gray_core::message::ToolDef::new("t", "d", serde_json::json!({}));
    assert!(
        !serde_json::to_value(&def)
            .unwrap()
            .as_object()
            .unwrap()
            .contains_key("preview")
    );
}

#[test]
fn legacy_string_tool_entries_still_parse() {
    let v = serde_json::json!({"name": "echo", "version": "0.1.0", "tools": ["echo"]});
    let m = Manifest::from_result(&v);
    assert_eq!(m.tools.len(), 1);
    assert_eq!(m.tools[0].name, "echo");
    assert!(m.commands.is_empty() && m.hooks.is_empty());
}

#[test]
fn subcommands_parse_lenient() {
    let v = serde_json::json!({
        "name": "cron", "version": "0.1.0", "tools": [],
        "subcommands": ["/cron"],
    });
    let m = Manifest::from_result(&v);
    assert_eq!(m.subcommands, vec!["/cron".to_string()]);

    let m2 = Manifest::from_result(&serde_json::json!({"name": "x", "tools": []}));
    assert!(m2.subcommands.is_empty());
}

#[test]
fn tool_label_parses_and_stays_model_invisible() {
    let v = serde_json::json!({
        "name": "x", "version": "0.1.0",
        "tools": [{"name": "notify_owner", "description": "d",
                   "parameters": {"type": "object"}, "label": "Notify Owner"}],
    });
    let m = Manifest::from_result(&v);
    assert_eq!(m.tools[0].label.as_deref(), Some("Notify Owner"));

    let v2 = serde_json::json!({
        "name": "x", "version": "0.1.0",
        "tools": [{"name": "t", "description": "d",
                   "parameters": {"type": "object"}, "label": "  "}],
    });
    assert!(Manifest::from_result(&v2).tools[0].label.is_none());

    let def = gray_core::message::ToolDef::new("t", "d", serde_json::json!({}));
    assert!(
        !serde_json::to_value(&def)
            .unwrap()
            .as_object()
            .unwrap()
            .contains_key("label")
    );
}

struct ManifestPlugin(Manifest);

#[async_trait::async_trait]
impl gray_plugin::Plugin for ManifestPlugin {
    fn manifest(&self) -> Manifest {
        self.0.clone()
    }
}

#[test]
fn turn_local_parses_and_stays_within_declared_commands() {
    let v = serde_json::json!({
        "name": "x", "version": "0.1.0", "tools": [],
        "commands": ["/subagents", "/usage"],
        "turn_local": {
            "/subagents": ["", "status", "view", "list", "open"],
            "/usage": ["*"],
            "/other": ["status"],
        },
    });
    let m = Manifest::from_result(&v);
    assert_eq!(
        m.turn_local["/subagents"],
        ["", "status", "view", "list", "open"]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>()
    );
    assert_eq!(m.turn_local["/usage"], vec!["*".to_string()]);
    assert!(
        !m.turn_local.contains_key("/other"),
        "keys outside the plugin's own commands are ignored"
    );

    let m2 = Manifest::from_result(&serde_json::json!({
        "name": "x", "tools": [], "commands": ["/a"],
    }));
    assert!(m2.turn_local.is_empty());
}

#[test]
fn hook_adapter_carries_declared_turn_local_verbs() {
    let m = Manifest::from_result(&serde_json::json!({
        "name": "x", "version": "0.1.0", "tools": [],
        "commands": ["/subagents", "/usage", "/plain"],
        "turn_local": {"/subagents": ["", "status"], "/usage": ["*"]},
    }));
    let adapter =
        gray_plugin::PluginHookAdapter::new(std::sync::Arc::new(ManifestPlugin(m)), "/tmp");
    let cmds = gray_core::agent::PluginHooks::commands(&adapter);
    let get = |name: &str| cmds.iter().find(|c| c.name == name).unwrap();
    assert_eq!(
        get("/subagents").turn_local_verbs,
        vec![String::new(), "status".to_string()]
    );
    assert_eq!(get("/usage").turn_local_verbs, vec!["*".to_string()]);
    assert!(get("/plain").turn_local_verbs.is_empty());
}
