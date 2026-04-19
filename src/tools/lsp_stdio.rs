//! 最小 LSP stdio 客户端：单次会话（initialize → initialized → didOpen → 请求 → shutdown）。
//! 设计目标是对齐 OpenCode「lsp 工具 + 行列定位」的交互方式，而非实现完整 LSP 生态。

use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};

use anyhow::Context;
use serde_json::{json, Value};
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LspOperation {
    GoToDefinition,
    FindReferences,
}

impl LspOperation {
    pub fn parse(s: &str) -> anyhow::Result<Self> {
        match s.trim() {
            "go_to_definition" | "goToDefinition" => Ok(Self::GoToDefinition),
            "find_references" | "findReferences" => Ok(Self::FindReferences),
            other => anyhow::bail!("unsupported lsp operation: {other}"),
        }
    }

    fn method(self) -> &'static str {
        match self {
            Self::GoToDefinition => "textDocument/definition",
            Self::FindReferences => "textDocument/references",
        }
    }
}

pub fn file_uri(path: &Path) -> anyhow::Result<String> {
    let url = Url::from_file_path(path).map_err(|()| anyhow::anyhow!("invalid file path for URL"))?;
    Ok(String::from(url))
}

pub fn language_id(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("rs") => "rust",
        Some("ts") => "typescript",
        Some("tsx") => "typescript",
        Some("js") | Some("jsx") | Some("mjs") | Some("cjs") => "javascript",
        Some("json") => "json",
        Some("md") => "markdown",
        Some("toml") => "toml",
        Some("go") => "go",
        Some("py") => "python",
        Some("c" | "h") => "c",
        Some("cpp" | "cc" | "cxx" | "hpp") => "cpp",
        _ => "plaintext",
    }
}

fn write_message(writer: &mut impl Write, msg: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(msg)?;
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    writer.write_all(&body)?;
    writer.flush()
}

fn read_message(reader: &mut impl BufRead) -> io::Result<Value> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "unexpected EOF while reading LSP headers",
            ));
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        let line_trim = line.trim_end_matches(['\r', '\n']);
        if let Some(rest) = line_trim.strip_prefix("Content-Length:") {
            content_length = Some(rest.trim().parse().map_err(|e| {
                io::Error::new(io::ErrorKind::InvalidData, format!("Content-Length: {e}"))
            })?);
        }
    }
    let len = content_length.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "missing Content-Length in LSP message",
        )
    })?;
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf)?;
    serde_json::from_slice(&buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

fn read_until_id(reader: &mut impl BufRead, want: i64) -> io::Result<Value> {
    const MAX_MESSAGES: usize = 4096;
    for _ in 0..MAX_MESSAGES {
        let msg = read_message(reader)?;
        // 只消费 JSON-RPC Response（含 result/error）；跳过 notification 与 server->client request。
        if msg.get("result").is_some() || msg.get("error").is_some() {
            if msg.get("id").and_then(|v| v.as_i64()) == Some(want) {
                return Ok(msg);
            }
        }
    }
    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        format!("timed out waiting for LSP response id={want}"),
    ))
}

/// 启动 `command`（默认 `rust-analyzer`），对 `file` 在 1-based `line`/`character` 执行一次 LSP 查询后关闭。
pub fn lsp_query_one_shot(
    workspace_root: &Path,
    command: &str,
    op: LspOperation,
    file: &Path,
    line_1_based: u32,
    character_1_based: u32,
) -> anyhow::Result<Value> {
    let ws = workspace_root.canonicalize().context("workspace_root")?;
    let file = file.canonicalize().context("lsp file path")?;

    let root_uri = file_uri(&ws)?;
    let doc_uri = file_uri(&file)?;

    let text = std::fs::read_to_string(&file).with_context(|| format!("read {}", file.display()))?;
    if text.len() > 1_000_000 {
        anyhow::bail!("file too large for LSP didOpen (max 1_000_000 bytes)");
    }

    let mut child: Child = Command::new(command)
        .current_dir(&ws)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to spawn LSP command `{command}` (is it on PATH?)"))?;

    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let stderr = child.stderr.take().expect("stderr");

    std::thread::spawn(move || {
        let mut stderr = stderr;
        let _ = std::io::copy(&mut stderr, &mut std::io::sink());
    });

    let mut reader = BufReader::new(stdout);

    let init_id: i64 = 1;
    write_message(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": init_id,
            "method": "initialize",
            "params": {
                "processId": serde_json::Value::Null,
                "rootUri": root_uri,
                "capabilities": {},
                "workspaceFolders": [{
                    "uri": root_uri,
                    "name": "workspace"
                }]
            }
        }),
    )?;

    let init_resp = read_until_id(&mut reader, init_id).map_err(io_err_to_anyhow)?;
    if init_resp.get("error").is_some() {
        anyhow::bail!("LSP initialize error: {}", init_resp);
    }

    write_message(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "method": "initialized",
            "params": {}
        }),
    )?;

    write_message(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": doc_uri,
                    "languageId": language_id(&file),
                    "version": 1,
                    "text": text
                }
            }
        }),
    )?;

    let line0 = line_1_based.saturating_sub(1);
    let char0 = character_1_based.saturating_sub(1);

    let req_id: i64 = 2;
    let params = match op {
        LspOperation::GoToDefinition => json!({
            "textDocument": { "uri": doc_uri },
            "position": { "line": line0, "character": char0 }
        }),
        LspOperation::FindReferences => json!({
            "textDocument": { "uri": doc_uri },
            "position": { "line": line0, "character": char0 },
            "context": { "includeDeclaration": true }
        }),
    };

    write_message(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": req_id,
            "method": op.method(),
            "params": params
        }),
    )?;

    let resp = read_until_id(&mut reader, req_id).map_err(io_err_to_anyhow)?;

    let _ = write_message(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 3_i64,
            "method": "shutdown",
            "params": serde_json::Value::Null
        }),
    );
    let _ = write_message(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "method": "exit",
            "params": serde_json::Value::Null
        }),
    );
    let _ = child.wait();

    if let Some(err) = resp.get("error") {
        anyhow::bail!("LSP {} error: {}", op.method(), err);
    }

    let op_label = match op {
        LspOperation::GoToDefinition => "go_to_definition",
        LspOperation::FindReferences => "find_references",
    };

    Ok(json!({
        "operation": op_label,
        "method": op.method(),
        "result": resp.get("result").cloned().unwrap_or(Value::Null),
    }))
}

fn io_err_to_anyhow(e: io::Error) -> anyhow::Error {
    anyhow::Error::new(e)
}
