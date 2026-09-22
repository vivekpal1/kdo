//! Move a conversation between coding harnesses.
//!
//! `kdo chats export` reads Claude Code, Codex, OpenCode, or Grok Build and
//! writes a `kdo-chat` JSON file: visible user and assistant text only.
//! `kdo chats import` writes that file back into a harness. DeepSeek Harness
//! imports the same file through the `kdo` plugin (`kdo setup dsh`).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use miette::{miette, IntoDiagnostic, Result};
use owo_colors::OwoColorize;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Harness {
    Claude,
    Codex,
    OpenCode,
    Grok,
    Dsh,
}

impl Harness {
    pub fn parse(name: &str) -> Result<Self> {
        Ok(match name {
            "claude" | "claude-code" => Self::Claude,
            "codex" => Self::Codex,
            "opencode" => Self::OpenCode,
            "grok" | "grok-build" => Self::Grok,
            "dsh" | "deepseek" => Self::Dsh,
            other => {
                return Err(miette!(
                    "unknown harness `{other}` (claude, codex, opencode, grok, dsh)"
                ))
            }
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::OpenCode => "opencode",
            Self::Grok => "grok",
            Self::Dsh => "dsh",
        }
    }

    pub fn all() -> &'static [Self] {
        &[
            Self::Claude,
            Self::Codex,
            Self::OpenCode,
            Self::Grok,
            Self::Dsh,
        ]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatDoc {
    pub version: u32,
    pub harness: String,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    pub messages: Vec<ChatMessage>,
}

#[derive(Debug, Clone)]
pub struct ChatSummary {
    pub harness: Harness,
    pub id: String,
    pub title: String,
    pub path: PathBuf,
}

pub fn cmd_list(harness: Option<&str>) -> Result<()> {
    let home = home_dir()?;
    let harnesses = selected(harness)?;
    let mut any = false;
    for harness in harnesses {
        match list(&home, harness) {
            Ok(rows) => {
                for row in rows {
                    any = true;
                    println!(
                        "{}\t{}\t{}\t{}",
                        row.harness.as_str(),
                        row.id,
                        one_line(&row.title, 72),
                        row.path.display()
                    );
                }
            }
            Err(err) => eprintln!("  {} {}: {err}", "warn".yellow(), harness.as_str()),
        }
    }
    if !any {
        eprintln!(
            "{} no chats found. Pass --harness if a directory is missing.",
            "kdo chats".cyan().bold()
        );
    }
    Ok(())
}

pub fn cmd_export(harness: &str, id: Option<&str>, all: bool, out: Option<&Path>) -> Result<()> {
    let home = home_dir()?;
    let harness = Harness::parse(harness)?;
    if all {
        let dir = out.ok_or_else(|| miette!("--all needs -o <directory>"))?;
        fs::create_dir_all(dir).into_diagnostic()?;
        let rows = list(&home, harness)?;
        for row in rows {
            let doc = export_one(&home, harness, &row.id)?;
            let path = dir.join(format!("{}-{}.kdo.json", harness.as_str(), sanitize(&row.id)));
            write_doc(&path, &doc)?;
            eprintln!("{} {}", "exported".green(), path.display());
        }
        return Ok(());
    }
    let id = id.ok_or_else(|| miette!("pass --id <session> or --all -o <dir>"))?;
    let doc = export_one(&home, harness, id)?;
    match out {
        Some(path) if path == Path::new("-") => {
            println!("{}", serde_json::to_string(&doc).into_diagnostic()?);
        }
        Some(path) => {
            write_doc(path, &doc)?;
            eprintln!(
                "{} {} ({} messages)",
                "exported".green(),
                path.display(),
                doc.messages.len()
            );
        }
        None => println!("{}", serde_json::to_string_pretty(&doc).into_diagnostic()?),
    }
    Ok(())
}

pub fn cmd_import(to: &str, file: &Path) -> Result<()> {
    let home = home_dir()?;
    let to = Harness::parse(to)?;
    let doc = read_doc(file)?;
    let path = import_doc(&home, to, &doc)?;
    eprintln!(
        "{} {} → {}",
        "imported".green(),
        doc.id.yellow(),
        path.display()
    );
    if to == Harness::Dsh {
        eprintln!(
            "  {} open DeepSeek Harness → Settings → kdo, and import this file.",
            "next".dimmed()
        );
    }
    Ok(())
}

pub fn list(home: &Path, harness: Harness) -> Result<Vec<ChatSummary>> {
    match harness {
        Harness::Claude => list_jsonl(&home.join(".claude").join("projects"), harness, ".jsonl"),
        Harness::Codex => {
            let mut rows = list_jsonl(&home.join(".codex").join("sessions"), harness, ".jsonl")?;
            rows.extend(list_jsonl(
                &home.join(".codex").join("archived_sessions"),
                harness,
                ".jsonl",
            )?);
            Ok(rows)
        }
        Harness::Grok => list_grok(&home.join(".grok").join("sessions")),
        Harness::OpenCode => list_opencode(home),
        Harness::Dsh => list_dsh_inbox(&home.join(".kdo").join("chats")),
    }
}

pub fn export_one(home: &Path, harness: Harness, id: &str) -> Result<ChatDoc> {
    match harness {
        Harness::Claude => {
            let path = find_file(&home.join(".claude").join("projects"), id, ".jsonl")
                .ok_or_else(|| miette!("claude session `{id}` not found"))?;
            parse_claude(&path, id)
        }
        Harness::Codex => {
            let path = find_file(&home.join(".codex").join("sessions"), id, ".jsonl")
                .or_else(|| find_file(&home.join(".codex").join("archived_sessions"), id, ".jsonl"))
                .ok_or_else(|| miette!("codex session `{id}` not found"))?;
            parse_codex(&path, id)
        }
        Harness::Grok => {
            let path = find_grok(&home.join(".grok").join("sessions"), id)
                .ok_or_else(|| miette!("grok session `{id}` not found"))?;
            parse_grok(&path, id)
        }
        Harness::OpenCode => export_opencode(home, id),
        Harness::Dsh => {
            let path = home
                .join(".kdo")
                .join("chats")
                .join(format!("{id}.kdo.json"));
            if path.exists() {
                return read_doc(&path);
            }
            Err(miette!(
                "dsh inbox has no {id}.kdo.json. Export from the source harness first."
            ))
        }
    }
}

pub fn import_doc(home: &Path, to: Harness, doc: &ChatDoc) -> Result<PathBuf> {
    if doc.version != 1 {
        return Err(miette!("unsupported kdo-chat version {}", doc.version));
    }
    match to {
        Harness::Claude => import_claude(home, doc),
        Harness::Codex => import_codex(home, doc),
        Harness::Grok => import_grok(home, doc),
        Harness::OpenCode => import_opencode(home, doc),
        Harness::Dsh => {
            let dir = home.join(".kdo").join("chats");
            fs::create_dir_all(&dir).into_diagnostic()?;
            let path = dir.join(format!("{}.kdo.json", sanitize(&doc.id)));
            write_doc(&path, doc)?;
            Ok(path)
        }
    }
}

fn selected(name: Option<&str>) -> Result<Vec<Harness>> {
    match name {
        Some(name) => Ok(vec![Harness::parse(name)?]),
        None => Ok(Harness::all().to_vec()),
    }
}

fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .ok_or_else(|| miette!("HOME is not set"))
}

fn write_doc(path: &Path, doc: &ChatDoc) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).into_diagnostic()?;
    }
    let text = serde_json::to_string_pretty(doc).into_diagnostic()?;
    let mut file = fs::File::create(path).into_diagnostic()?;
    file.write_all(text.as_bytes()).into_diagnostic()?;
    file.write_all(b"\n").into_diagnostic()?;
    Ok(())
}

fn read_doc(path: &Path) -> Result<ChatDoc> {
    let text = fs::read_to_string(path).into_diagnostic()?;
    serde_json::from_str(&text).into_diagnostic()
}

fn list_jsonl(root: &Path, harness: Harness, suffix: &str) -> Result<Vec<ChatSummary>> {
    let mut rows = Vec::new();
    if !root.exists() {
        return Ok(rows);
    }
    walk(root, &mut |path| {
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(suffix))
        {
            let id = session_id(path);
            let title = title_from_file(path).unwrap_or_else(|| id.clone());
            rows.push(ChatSummary {
                harness,
                id,
                title,
                path: path.to_path_buf(),
            });
        }
    })?;
    rows.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(rows)
}

fn list_grok(root: &Path) -> Result<Vec<ChatSummary>> {
    let mut rows = Vec::new();
    if !root.exists() {
        return Ok(rows);
    }
    walk(root, &mut |path| {
        if path.file_name().and_then(|n| n.to_str()) == Some("chat_history.jsonl") {
            let id = path
                .parent()
                .and_then(|dir| dir.file_name())
                .and_then(|name| name.to_str())
                .unwrap_or("session")
                .to_string();
            let title = path
                .parent()
                .and_then(|dir| fs::read_to_string(dir.join("summary.json")).ok())
                .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
                .and_then(|value| {
                    value
                        .get("title")
                        .or_else(|| value.get("summary"))
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                })
                .unwrap_or_else(|| id.clone());
            rows.push(ChatSummary {
                harness: Harness::Grok,
                id,
                title,
                path: path.to_path_buf(),
            });
        }
    })?;
    rows.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(rows)
}

fn list_dsh_inbox(dir: &Path) -> Result<Vec<ChatSummary>> {
    let mut rows = Vec::new();
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return Ok(rows),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        if let Ok(doc) = read_doc(&path) {
            rows.push(ChatSummary {
                harness: Harness::Dsh,
                id: doc.id,
                title: doc.title.unwrap_or_else(|| "imported".into()),
                path,
            });
        }
    }
    rows.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(rows)
}

fn list_opencode(home: &Path) -> Result<Vec<ChatSummary>> {
    let db = opencode_db(home);
    if !db.exists() {
        return Ok(Vec::new());
    }
    let out = sqlite(
        &db,
        "SELECT id, title, directory FROM session ORDER BY time_created DESC;",
    )?;
    let mut rows = Vec::new();
    for line in out.lines().filter(|line| !line.is_empty()) {
        let mut parts = line.split('\t');
        let id = parts.next().unwrap_or("").to_string();
        if id.is_empty() {
            continue;
        }
        let title = parts.next().unwrap_or("").to_string();
        rows.push(ChatSummary {
            harness: Harness::OpenCode,
            id,
            title: if title.is_empty() {
                "untitled".into()
            } else {
                title
            },
            path: db.clone(),
        });
    }
    Ok(rows)
}

fn export_opencode(home: &Path, id: &str) -> Result<ChatDoc> {
    let db = opencode_db(home);
    let header = sqlite(
        &db,
        &format!(
            "SELECT title, directory FROM session WHERE id = {};",
            sql_quote(id)
        ),
    )?;
    let mut header_parts = header.lines().next().unwrap_or("").split('\t');
    let title = header_parts.next().filter(|s| !s.is_empty()).map(str::to_string);
    let cwd = header_parts.next().filter(|s| !s.is_empty()).map(str::to_string);
    let body = sqlite(
        &db,
        &format!(
            "SELECT m.data, p.data FROM message m LEFT JOIN part p ON p.message_id = m.id WHERE m.session_id = {} ORDER BY m.time_created;",
            sql_quote(id)
        ),
    )?;
    let mut messages = Vec::new();
    for line in body.lines().filter(|line| !line.is_empty()) {
        let mut cols = line.split('\t');
        let message: serde_json::Value = serde_json::from_str(cols.next().unwrap_or("{}")).unwrap_or(json!({}));
        let part: serde_json::Value = serde_json::from_str(cols.next().unwrap_or("{}")).unwrap_or(json!({}));
        let role = message
            .get("role")
            .and_then(|v| v.as_str())
            .unwrap_or("user");
        if role != "user" && role != "assistant" {
            continue;
        }
        let text = part
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if text.is_empty() {
            continue;
        }
        push_message(&mut messages, role, text);
    }
    if messages.is_empty() && title.is_none() {
        return Err(miette!("opencode session `{id}` not found"));
    }
    Ok(ChatDoc {
        version: 1,
        harness: "opencode".into(),
        id: id.into(),
        title,
        cwd,
        messages,
    })
}

fn import_opencode(home: &Path, doc: &ChatDoc) -> Result<PathBuf> {
    let db = opencode_db(home);
    if !db.exists() {
        return Err(miette!(
            "OpenCode database not found at {}. Open OpenCode once, then import again.",
            db.display()
        ));
    }
    let now = chrono_millis();
    let project = "kdo-import";
    let _ = sqlite(
        &db,
        &format!(
            "INSERT OR IGNORE INTO project (id, worktree, name, time_created, time_updated, sandboxes) VALUES ({project}, {dir}, 'kdo-import', {now}, {now}, '[]');",
            project = sql_quote(project),
            dir = sql_quote(doc.cwd.as_deref().unwrap_or(".")),
            now = now,
        ),
    );
    sqlite(
        &db,
        &format!(
            "INSERT INTO session (id, project_id, slug, directory, title, version, time_created, time_updated) VALUES ({id}, {project}, {slug}, {dir}, {title}, '1', {now}, {now});",
            id = sql_quote(&format!("kdo-{}", sanitize(&doc.id))),
            project = sql_quote(project),
            slug = sql_quote(&sanitize(&doc.id)),
            dir = sql_quote(doc.cwd.as_deref().unwrap_or(".")),
            title = sql_quote(doc.title.as_deref().unwrap_or("imported from kdo")),
            now = now,
        ),
    )?;
    let session_id = format!("kdo-{}", sanitize(&doc.id));
    for (index, message) in doc.messages.iter().enumerate() {
        let message_id = format!("{session_id}-{index}");
        let data = json!({"role": message.role, "time": {"created": now}});
        sqlite(
            &db,
            &format!(
                "INSERT INTO message (id, session_id, time_created, time_updated, data) VALUES ({id}, {sid}, {now}, {now}, {data});",
                id = sql_quote(&message_id),
                sid = sql_quote(&session_id),
                now = now + index as i64,
                data = sql_quote(&data.to_string()),
            ),
        )?;
        let part = json!({"type": "text", "text": message.text});
        sqlite(
            &db,
            &format!(
                "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) VALUES ({id}, {mid}, {sid}, {now}, {now}, {data});",
                id = sql_quote(&format!("{message_id}-p")),
                mid = sql_quote(&message_id),
                sid = sql_quote(&session_id),
                now = now + index as i64,
                data = sql_quote(&part.to_string()),
            ),
        )?;
    }
    Ok(db)
}

fn import_claude(home: &Path, doc: &ChatDoc) -> Result<PathBuf> {
    let dir = home.join(".claude").join("projects").join("-kdo-import");
    fs::create_dir_all(&dir).into_diagnostic()?;
    let path = dir.join(format!("{}.jsonl", sanitize(&doc.id)));
    refuse_existing(&path)?;
    let mut lines = String::new();
    for message in &doc.messages {
        let row = json!({
            "type": message.role,
            "cwd": doc.cwd,
            "message": {
                "role": message.role,
                "content": [{"type": "text", "text": message.text}]
            }
        });
        lines.push_str(&row.to_string());
        lines.push('\n');
    }
    fs::write(&path, lines).into_diagnostic()?;
    Ok(path)
}

fn import_codex(home: &Path, doc: &ChatDoc) -> Result<PathBuf> {
    let dir = home.join(".codex").join("sessions");
    fs::create_dir_all(&dir).into_diagnostic()?;
    let path = dir.join(format!("rollout-kdo-{}.jsonl", sanitize(&doc.id)));
    refuse_existing(&path)?;
    let mut lines = String::new();
    lines.push_str(
        &json!({
            "type": "session_meta",
            "payload": {"cwd": doc.cwd, "id": doc.id}
        })
        .to_string(),
    );
    lines.push('\n');
    for message in &doc.messages {
        let kind = if message.role == "assistant" {
            "output_text"
        } else {
            "input_text"
        };
        lines.push_str(
            &json!({
                "type": "response_item",
                "payload": {
                    "type": "message",
                    "role": message.role,
                    "content": [{"type": kind, "text": message.text}]
                }
            })
            .to_string(),
        );
        lines.push('\n');
    }
    fs::write(&path, lines).into_diagnostic()?;
    Ok(path)
}

fn import_grok(home: &Path, doc: &ChatDoc) -> Result<PathBuf> {
    let dir = home
        .join(".grok")
        .join("sessions")
        .join("%2Fkdo-import")
        .join(sanitize(&doc.id));
    fs::create_dir_all(&dir).into_diagnostic()?;
    let history = dir.join("chat_history.jsonl");
    refuse_existing(&history)?;
    let mut lines = String::new();
    for (index, message) in doc.messages.iter().enumerate() {
        lines.push_str(
            &json!({
                "id": format!("{}-{index}", doc.id),
                "type": message.role,
                "content": message.text
            })
            .to_string(),
        );
        lines.push('\n');
    }
    fs::write(&history, lines).into_diagnostic()?;
    fs::write(
        dir.join("summary.json"),
        serde_json::to_string_pretty(&json!({
            "title": doc.title.clone().unwrap_or_else(|| "imported from kdo".into()),
            "session_id": doc.id
        }))
        .into_diagnostic()?,
    )
    .into_diagnostic()?;
    Ok(history)
}

fn parse_claude(path: &Path, id: &str) -> Result<ChatDoc> {
    let mut messages = Vec::new();
    let mut cwd = None;
    let mut title = None;
    for value in json_lines(path)? {
        if cwd.is_none() {
            cwd = value.get("cwd").and_then(|v| v.as_str()).map(str::to_string);
        }
        if value.get("type").and_then(|v| v.as_str()) == Some("ai-title") {
            title = value.get("title").and_then(|v| v.as_str()).map(str::to_string);
        }
        let kind = value.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if kind != "user" && kind != "assistant" {
            continue;
        }
        if value.get("isMeta").and_then(|v| v.as_bool()) == Some(true) {
            continue;
        }
        let role = value
            .pointer("/message/role")
            .and_then(|v| v.as_str())
            .unwrap_or(kind);
        if role != "user" && role != "assistant" {
            continue;
        }
        let text = claude_text(value.get("message"));
        push_message(&mut messages, role, text);
    }
    Ok(ChatDoc {
        version: 1,
        harness: "claude".into(),
        id: id.to_string(),
        title,
        cwd,
        messages,
    })
}

fn parse_codex(path: &Path, id: &str) -> Result<ChatDoc> {
    let mut messages = Vec::new();
    let mut cwd = None;
    for value in json_lines(path)? {
        if value.get("type").and_then(|v| v.as_str()) == Some("session_meta") {
            cwd = value
                .pointer("/payload/cwd")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            continue;
        }
        let item = value.get("payload").filter(|_| {
            value.get("type").and_then(|v| v.as_str()) == Some("response_item")
        });
        let Some(item) = item else { continue };
        if item.get("type").and_then(|v| v.as_str()) != Some("message") {
            continue;
        }
        let role = item.get("role").and_then(|v| v.as_str()).unwrap_or("");
        if role != "user" && role != "assistant" {
            continue;
        }
        let mut text = text_blocks(item.get("content"));
        if role == "user" {
            text = text
                .lines()
                .filter(|line| {
                    let trimmed = line.trim_start();
                    !trimmed.starts_with("<environment_context>")
                        && !trimmed.starts_with("<app-context>")
                        && !trimmed.starts_with("# AGENTS.md")
                })
                .collect::<Vec<_>>()
                .join("\n");
        }
        push_message(&mut messages, role, text);
    }
    Ok(ChatDoc {
        version: 1,
        harness: "codex".into(),
        id: id.to_string(),
        title: None,
        cwd,
        messages,
    })
}

fn parse_grok(path: &Path, id: &str) -> Result<ChatDoc> {
    let mut messages = Vec::new();
    for value in json_lines(path)? {
        let kind = value
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if kind != "user" && kind != "assistant" {
            continue;
        }
        let text = value
            .get("content")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| text_blocks(value.get("content")));
        push_message(&mut messages, kind, text);
    }
    let title = path
        .parent()
        .and_then(|dir| fs::read_to_string(dir.join("summary.json")).ok())
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|value| value.get("title").and_then(|v| v.as_str()).map(str::to_string));
    Ok(ChatDoc {
        version: 1,
        harness: "grok".into(),
        id: id.into(),
        title,
        cwd: None,
        messages,
    })
}

fn claude_text(message: Option<&serde_json::Value>) -> String {
    let Some(message) = message else {
        return String::new();
    };
    match message.get("content") {
        Some(serde_json::Value::String(text)) => text.clone(),
        Some(serde_json::Value::Array(blocks)) => blocks
            .iter()
            .filter(|block| block.get("type").and_then(|v| v.as_str()) == Some("text"))
            .filter_map(|block| block.get("text").and_then(|v| v.as_str()))
            .collect::<Vec<_>>()
            .join("\n\n"),
        _ => String::new(),
    }
}

fn text_blocks(content: Option<&serde_json::Value>) -> String {
    let Some(content) = content else {
        return String::new();
    };
    if let Some(text) = content.as_str() {
        return text.to_string();
    }
    content
        .as_array()
        .map(|blocks| {
            blocks
                .iter()
                .filter_map(|block| {
                    block
                        .get("text")
                        .or_else(|| block.get("input_text"))
                        .or_else(|| block.get("output_text"))
                        .and_then(|v| v.as_str())
                })
                .collect::<Vec<_>>()
                .join("\n\n")
        })
        .unwrap_or_default()
}

fn push_message(messages: &mut Vec<ChatMessage>, role: &str, text: String) {
    let text = text.trim().to_string();
    if text.is_empty() {
        return;
    }
    if let Some(prev) = messages.last_mut() {
        if prev.role == role {
            prev.text.push_str("\n\n");
            prev.text.push_str(&text);
            return;
        }
    }
    messages.push(ChatMessage {
        role: role.to_string(),
        text,
    });
}

fn json_lines(path: &Path) -> Result<Vec<serde_json::Value>> {
    let text = fs::read_to_string(path).into_diagnostic()?;
    Ok(text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect())
}

fn walk(root: &Path, visit: &mut dyn FnMut(&Path)) -> Result<()> {
    let entries = fs::read_dir(root).into_diagnostic()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, visit)?;
        } else {
            visit(&path);
        }
    }
    Ok(())
}

fn find_file(root: &Path, id: &str, suffix: &str) -> Option<PathBuf> {
    let mut found = None;
    if !root.exists() {
        return None;
    }
    let _ = walk(root, &mut |path| {
        if found.is_some() {
            return;
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.ends_with(suffix) && (name.contains(id) || path.to_string_lossy().contains(id)) {
            found = Some(path.to_path_buf());
        }
    });
    found
}

fn find_grok(root: &Path, id: &str) -> Option<PathBuf> {
    let mut found = None;
    if !root.exists() {
        return None;
    }
    let _ = walk(root, &mut |path| {
        if found.is_some() {
            return;
        }
        if path.file_name().and_then(|n| n.to_str()) == Some("chat_history.jsonl")
            && path.to_string_lossy().contains(id)
        {
            found = Some(path.to_path_buf());
        }
    });
    found
}

fn title_from_file(path: &Path) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    for line in text.lines().take(40) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if let Some(title) = value.get("title").and_then(|v| v.as_str()) {
            return Some(title.to_string());
        }
    }
    None
}

fn session_id(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("session")
        .to_string()
}

fn opencode_db(home: &Path) -> PathBuf {
    if let Some(path) = std::env::var_os("KDO_OPENCODE_DB") {
        return PathBuf::from(path);
    }
    home.join(".local")
        .join("share")
        .join("opencode")
        .join("opencode.db")
}

fn sqlite(db: &Path, sql: &str) -> Result<String> {
    let output = Command::new("sqlite3")
        .arg(db)
        .arg("-separator")
        .arg("\t")
        .arg(sql)
        .output()
        .map_err(|err| miette!("sqlite3 is required to read OpenCode chats: {err}"))?;
    if !output.status.success() {
        return Err(miette!(
            "sqlite3: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn sql_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn refuse_existing(path: &Path) -> Result<()> {
    if path.exists() {
        return Err(miette!("{} already exists", path.display()));
    }
    Ok(())
}

fn sanitize(id: &str) -> String {
    id.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .take(80)
        .collect()
}

fn one_line(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        flat
    } else {
        format!("{}…", flat.chars().take(max).collect::<String>())
    }
}

fn chrono_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_round_trip_keeps_visible_text() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let src = home.join(".claude").join("projects").join("demo");
        fs::create_dir_all(&src).unwrap();
        fs::write(
            src.join("abc.jsonl"),
            "{\"type\":\"user\",\"cwd\":\"/repo\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"hello\"}]}}\n{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"hi\"}]}}\n",
        )
        .unwrap();
        let doc = export_one(home, Harness::Claude, "abc").unwrap();
        assert_eq!(doc.messages.len(), 2);
        assert_eq!(doc.messages[0].text, "hello");
        assert_eq!(doc.cwd.as_deref(), Some("/repo"));
        let imported = import_doc(home, Harness::Codex, &doc).unwrap();
        let back = parse_codex(&imported, "abc").unwrap();
        assert_eq!(back.messages[1].text, "hi");
    }

    #[test]
    fn grok_skips_reasoning_rows() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp
            .path()
            .join(".grok")
            .join("sessions")
            .join("cwd")
            .join("sess-1");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("summary.json"), "{\"title\":\"Rename button\"}\n").unwrap();
        fs::write(
            dir.join("chat_history.jsonl"),
            "{\"type\":\"user\",\"content\":\"change the label\"}\n{\"type\":\"reasoning\",\"content\":\"hidden\"}\n{\"type\":\"assistant\",\"content\":\"done\"}\n",
        )
        .unwrap();
        let doc = export_one(tmp.path(), Harness::Grok, "sess-1").unwrap();
        assert_eq!(doc.title.as_deref(), Some("Rename button"));
        assert_eq!(doc.messages.len(), 2);
        assert!(doc.messages.iter().all(|m| m.text != "hidden"));
    }
}
