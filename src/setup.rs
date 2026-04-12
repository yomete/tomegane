use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Map, Value, json};
use toml_edit::{Array, DocumentMut, Item, Table, value};

const SERVER_NAME: &str = "tomegane";

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum SetupScope {
    User,
    Project,
}

impl SetupScope {
    pub fn as_claude_scope(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Project => "project",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Project => "project",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClientKind {
    Claude,
    Cursor,
    Codex,
}

impl ClientKind {
    fn name(self) -> &'static str {
        match self {
            Self::Claude => "Claude Code",
            Self::Cursor => "Cursor",
            Self::Codex => "Codex",
        }
    }
}

#[derive(Debug)]
struct Detection {
    kind: ClientKind,
    available: bool,
    configured: bool,
    detail: String,
}

pub fn run_setup(scope: SetupScope, yes: bool) -> Result<(), String> {
    let exe =
        env::current_exe().map_err(|e| format!("Failed to locate current executable: {e}"))?;
    let cwd =
        env::current_dir().map_err(|e| format!("Failed to determine current directory: {e}"))?;

    println!("tomegane setup");
    println!("Using {} scope.", scope.label());
    println!();

    let detections = vec![
        detect_claude(scope),
        detect_cursor(scope, &cwd),
        detect_codex(scope),
    ];

    let available = detections.iter().filter(|d| d.available).count();
    if available == 0 {
        println!("No supported MCP clients were detected.");
        println!("Supported clients right now: Claude Code, Cursor, and Codex.");
        return Ok(());
    }

    for detection in detections {
        if !detection.available {
            println!(
                "{} not detected: {}",
                detection.kind.name(),
                detection.detail
            );
            continue;
        }

        println!("{} detected: {}", detection.kind.name(), detection.detail);

        if detection.configured {
            println!("{} is already configured there.", SERVER_NAME);
            println!();
            continue;
        }

        let should_install = if yes {
            true
        } else {
            prompt_yes_no(&format!(
                "Add {} to {} ({})? [y/N]: ",
                SERVER_NAME,
                detection.kind.name(),
                scope.label()
            ))?
        };

        if !should_install {
            println!("Skipped {}.", detection.kind.name());
            println!();
            continue;
        }

        match detection.kind {
            ClientKind::Claude => install_claude(scope, &exe)?,
            ClientKind::Cursor => install_cursor(scope, &cwd, &exe)?,
            ClientKind::Codex => install_codex(scope, &exe)?,
        }

        println!("Added {} to {}.", SERVER_NAME, detection.kind.name());
        println!();
    }

    Ok(())
}

fn detect_claude(scope: SetupScope) -> Detection {
    if !command_available("claude", &["--version"]) {
        return Detection {
            kind: ClientKind::Claude,
            available: false,
            configured: false,
            detail: "the `claude` CLI is not on PATH".to_string(),
        };
    }

    let configured = claude_has_server(scope);
    Detection {
        kind: ClientKind::Claude,
        available: true,
        configured,
        detail: format!("`claude` CLI found on PATH; scope {}", scope.label()),
    }
}

fn detect_cursor(scope: SetupScope, cwd: &Path) -> Detection {
    let cli_available = command_available("cursor-agent", &["--version"]);
    let config_path = cursor_config_path(scope, cwd);
    let configured = cursor_has_server(&config_path).unwrap_or(false);

    let detail = if cli_available {
        format!(
            "`cursor-agent` found on PATH; config {}",
            config_path.display()
        )
    } else {
        format!(
            "`cursor-agent` not found, but config can still be written to {}",
            config_path.display()
        )
    };

    Detection {
        kind: ClientKind::Cursor,
        available: true,
        configured,
        detail,
    }
}

fn detect_codex(scope: SetupScope) -> Detection {
    if scope == SetupScope::Project {
        return Detection {
            kind: ClientKind::Codex,
            available: false,
            configured: false,
            detail: "Codex MCP is currently supported only in user scope".to_string(),
        };
    }

    let config_path = codex_config_path();
    let available = config_path.exists() || home_dir().join(".codex").exists();
    let configured = codex_has_server(&config_path).unwrap_or(false);

    Detection {
        kind: ClientKind::Codex,
        available,
        configured,
        detail: format!("config {}", config_path.display()),
    }
}

fn install_claude(scope: SetupScope, exe: &Path) -> Result<(), String> {
    let output = Command::new("claude")
        .args([
            "mcp",
            "add",
            SERVER_NAME,
            "--scope",
            scope.as_claude_scope(),
            "--",
            exe.to_str()
                .ok_or("Executable path contains invalid UTF-8")?,
            "mcp",
        ])
        .output()
        .map_err(|e| format!("Failed to run `claude mcp add`: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("`claude mcp add` failed: {stderr}"));
    }

    Ok(())
}

fn install_cursor(scope: SetupScope, cwd: &Path, exe: &Path) -> Result<(), String> {
    let config_path = cursor_config_path(scope, cwd);
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            format!(
                "Failed to create config directory {}: {e}",
                parent.display()
            )
        })?;
    }

    let mut root = read_json_object_or_default(&config_path)?;
    let root_obj = root.as_object_mut().ok_or_else(|| {
        format!(
            "Cursor config at {} is not a JSON object",
            config_path.display()
        )
    })?;

    let mcp_servers = root_obj
        .entry("mcpServers".to_string())
        .or_insert_with(|| Value::Object(Map::new()));

    let servers_obj = mcp_servers.as_object_mut().ok_or_else(|| {
        format!(
            "Cursor config at {} has a non-object `mcpServers` field",
            config_path.display()
        )
    })?;

    servers_obj.insert(
        SERVER_NAME.to_string(),
        json!({
            "command": exe.to_string_lossy().to_string(),
            "args": ["mcp"]
        }),
    );

    let serialized = serde_json::to_string_pretty(&root)
        .map_err(|e| format!("Failed to serialize Cursor config: {e}"))?;
    fs::write(&config_path, format!("{serialized}\n")).map_err(|e| {
        format!(
            "Failed to write Cursor config {}: {e}",
            config_path.display()
        )
    })?;

    Ok(())
}

fn install_codex(scope: SetupScope, exe: &Path) -> Result<(), String> {
    if scope != SetupScope::User {
        return Err("Codex setup currently supports only user scope".to_string());
    }

    let config_path = codex_config_path();
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            format!(
                "Failed to create config directory {}: {e}",
                parent.display()
            )
        })?;
    }

    let mut doc = read_toml_document_or_default(&config_path)?;
    if !doc.as_table().contains_key("mcp_servers") {
        doc["mcp_servers"] = Item::Table(Table::new());
    }

    let servers = doc["mcp_servers"].as_table_mut().ok_or_else(|| {
        format!(
            "Codex config at {} has a non-table `mcp_servers` field",
            config_path.display()
        )
    })?;

    let mut server = Table::new();
    let mut args = Array::new();
    args.push("mcp");
    server["command"] = value(exe.to_string_lossy().to_string());
    server["args"] = value(args);
    servers[SERVER_NAME] = Item::Table(server);

    fs::write(&config_path, doc.to_string()).map_err(|e| {
        format!(
            "Failed to write Codex config {}: {e}",
            config_path.display()
        )
    })?;

    Ok(())
}

fn claude_has_server(scope: SetupScope) -> bool {
    Command::new("claude")
        .args([
            "mcp",
            "get",
            SERVER_NAME,
            "--scope",
            scope.as_claude_scope(),
        ])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn cursor_has_server(path: &Path) -> Result<bool, String> {
    if !path.exists() {
        return Ok(false);
    }

    let value = read_json_object_or_default(path)?;
    Ok(value
        .get("mcpServers")
        .and_then(Value::as_object)
        .is_some_and(|servers| servers.contains_key(SERVER_NAME)))
}

fn codex_has_server(path: &Path) -> Result<bool, String> {
    if !path.exists() {
        return Ok(false);
    }

    let doc = read_toml_document_or_default(path)?;
    Ok(doc["mcp_servers"]
        .as_table()
        .is_some_and(|servers| servers.contains_key(SERVER_NAME)))
}

fn read_json_object_or_default(path: &Path) -> Result<Value, String> {
    if !path.exists() {
        return Ok(Value::Object(Map::new()));
    }

    let raw =
        fs::read_to_string(path).map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
    serde_json::from_str(&raw)
        .map_err(|e| format!("Failed to parse JSON in {}: {e}", path.display()))
}

fn read_toml_document_or_default(path: &Path) -> Result<DocumentMut, String> {
    if !path.exists() {
        return Ok(DocumentMut::new());
    }

    let raw =
        fs::read_to_string(path).map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
    raw.parse::<DocumentMut>()
        .map_err(|e| format!("Failed to parse TOML in {}: {e}", path.display()))
}

fn cursor_config_path(scope: SetupScope, cwd: &Path) -> PathBuf {
    match scope {
        SetupScope::User => home_dir().join(".cursor").join("mcp.json"),
        SetupScope::Project => cwd.join(".cursor").join("mcp.json"),
    }
}

fn codex_config_path() -> PathBuf {
    home_dir().join(".codex").join("config.toml")
}

fn home_dir() -> PathBuf {
    env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn command_available(cmd: &str, args: &[&str]) -> bool {
    Command::new(cmd)
        .args(args)
        .output()
        .map(|_| true)
        .unwrap_or(false)
}

fn prompt_yes_no(prompt: &str) -> Result<bool, String> {
    print!("{prompt}");
    io::stdout()
        .flush()
        .map_err(|e| format!("Failed to flush stdout: {e}"))?;

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .map_err(|e| format!("Failed to read input: {e}"))?;

    let answer = input.trim().to_ascii_lowercase();
    Ok(matches!(answer.as_str(), "y" | "yes"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_project_path_uses_workspace() {
        let cwd = PathBuf::from("/tmp/project");
        let path = cursor_config_path(SetupScope::Project, &cwd);
        assert_eq!(path, PathBuf::from("/tmp/project/.cursor/mcp.json"));
    }

    #[test]
    fn cursor_has_server_returns_false_for_missing_config() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("missing.json");
        assert!(!cursor_has_server(&path).unwrap());
    }

    #[test]
    fn cursor_has_server_detects_existing_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("mcp.json");
        fs::write(
            &path,
            r#"{
  "mcpServers": {
    "tomegane": {
      "command": "/bin/tomegane",
      "args": ["mcp"]
    }
  }
}"#,
        )
        .unwrap();

        assert!(cursor_has_server(&path).unwrap());
    }

    #[test]
    fn codex_has_server_detects_existing_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        fs::write(
            &path,
            r#"[mcp_servers.tomegane]
command = "/bin/tomegane"
args = ["mcp"]
"#,
        )
        .unwrap();

        assert!(codex_has_server(&path).unwrap());
    }

    #[test]
    fn install_cursor_merges_config() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path().join("workspace");
        fs::create_dir_all(&cwd).unwrap();
        let config_path = cwd.join(".cursor").join("mcp.json");
        fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        fs::write(
            &config_path,
            r#"{
  "mcpServers": {
    "other": {
      "command": "echo"
    }
  }
}"#,
        )
        .unwrap();

        install_cursor(
            SetupScope::Project,
            &cwd,
            Path::new("/usr/local/bin/tomegane"),
        )
        .unwrap();

        let value: Value =
            serde_json::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
        let servers = value["mcpServers"].as_object().unwrap();
        assert!(servers.contains_key("other"));
        assert_eq!(servers["tomegane"]["args"][0], "mcp");
    }

    #[test]
    fn install_codex_merges_config() {
        let tmp = tempfile::tempdir().unwrap();
        let codex_dir = tmp.path().join(".codex");
        fs::create_dir_all(&codex_dir).unwrap();
        let config_path = codex_dir.join("config.toml");
        fs::write(
            &config_path,
            r#"model = "gpt-5.4"

[mcp_servers.other]
command = "echo"
"#,
        )
        .unwrap();

        let original_home = env::var_os("HOME");
        unsafe { env::set_var("HOME", tmp.path()) };

        let result = install_codex(SetupScope::User, Path::new("/usr/local/bin/tomegane"));

        if let Some(home) = original_home {
            unsafe { env::set_var("HOME", home) };
        } else {
            unsafe { env::remove_var("HOME") };
        }

        result.unwrap();

        let doc = fs::read_to_string(&config_path).unwrap();
        assert!(doc.contains("[mcp_servers.other]"));
        assert!(doc.contains("[mcp_servers.tomegane]"));
        assert!(doc.contains(r#"args = ["mcp"]"#));
    }
}
