use std::collections::HashMap;
use std::io::{self, BufRead, Read, Write};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::syntax::{Lexer, Parser, Token};

pub const TOKEN_TYPES: &[&str] = &[
    "keyword",    // 0
    "type",       // 1
    "function",   // 2
    "variable",   // 3
    "string",     // 4
    "number",     // 5
    "operator",   // 6
    "comment",    // 7
    "property",   // 8
    "enumMember", // 9
];

#[derive(Debug, Serialize, Deserialize)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Diagnostic {
    pub range: Range,
    pub severity: u32,
    pub source: String,
    pub message: String,
}

struct RawToken {
    line: u32,
    col: u32,
    len: u32,
    token_type: u32,
}

pub fn run_lsp_server() -> Result<(), Box<dyn std::error::Error>> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut stdin_reader = io::BufReader::new(stdin.lock());
    let mut stdout_lock = stdout.lock();

    let mut documents: HashMap<String, String> = HashMap::new();

    loop {
        let mut line = String::new();
        let bytes_read = stdin_reader.read_line(&mut line)?;
        if bytes_read == 0 {
            break;
        }

        let trimmed = line.trim();
        if trimmed.starts_with("Content-Length:") {
            let len_str = trimmed["Content-Length:".len()..].trim();
            let content_len: usize = match len_str.parse() {
                Ok(n) => n,
                Err(_) => continue,
            };

            // Read until empty line (\r\n\r\n)
            loop {
                line.clear();
                stdin_reader.read_line(&mut line)?;
                if line.trim().is_empty() {
                    break;
                }
            }

            // Read message body
            let mut body = vec![0u8; content_len];
            stdin_reader.read_exact(&mut body)?;

            let msg: Value = match serde_json::from_slice(&body) {
                Ok(v) => v,
                Err(_) => continue,
            };

            handle_message(&msg, &mut stdout_lock, &mut documents)?;
        }
    }

    Ok(())
}

fn send_response(
    stdout: &mut dyn Write,
    id: &Value,
    result: Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let resp = json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    });
    let payload = serde_json::to_string(&resp)?;
    write!(
        stdout,
        "Content-Length: {}\r\n\r\n{}",
        payload.len(),
        payload
    )?;
    stdout.flush()?;
    Ok(())
}

fn send_notification(
    stdout: &mut dyn Write,
    method: &str,
    params: Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let notif = json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params
    });
    let payload = serde_json::to_string(&notif)?;
    write!(
        stdout,
        "Content-Length: {}\r\n\r\n{}",
        payload.len(),
        payload
    )?;
    stdout.flush()?;
    Ok(())
}

fn handle_message(
    msg: &Value,
    stdout: &mut dyn Write,
    documents: &mut HashMap<String, String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let method = match msg.get("method").and_then(|m| m.as_str()) {
        Some(m) => m,
        None => return Ok(()),
    };

    let id = msg.get("id");

    match method {
        "initialize" => {
            if let Some(req_id) = id {
                let result = json!({
                    "capabilities": {
                        "textDocumentSync": 1, // Full document sync
                        "semanticTokensProvider": {
                            "legend": {
                                "tokenTypes": TOKEN_TYPES,
                                "tokenModifiers": []
                            },
                            "full": true
                        }
                    },
                    "serverInfo": {
                        "name": "hart-lsp",
                        "version": "0.1.0"
                    }
                });
                send_response(stdout, req_id, result)?;
            }
        }
        "initialized" => {
            // No response needed
        }
        "shutdown" => {
            if let Some(req_id) = id {
                send_response(stdout, req_id, Value::Null)?;
            }
        }
        "exit" => {
            std::process::exit(0);
        }
        "textDocument/didOpen" => {
            if let Some(params) = msg.get("params") {
                if let (Some(uri), Some(text)) = (
                    params.get("textDocument").and_then(|d| d.get("uri")).and_then(|u| u.as_str()),
                    params.get("textDocument").and_then(|d| d.get("text")).and_then(|t| t.as_str()),
                ) {
                    documents.insert(uri.to_string(), text.to_string());
                    publish_diagnostics(stdout, uri, text)?;
                }
            }
        }
        "textDocument/didChange" => {
            if let Some(params) = msg.get("params") {
                if let (Some(uri), Some(changes)) = (
                    params.get("textDocument").and_then(|d| d.get("uri")).and_then(|u| u.as_str()),
                    params.get("contentChanges").and_then(|c| c.as_array()),
                ) {
                    if let Some(last_change) = changes.last() {
                        if let Some(text) = last_change.get("text").and_then(|t| t.as_str()) {
                            documents.insert(uri.to_string(), text.to_string());
                            publish_diagnostics(stdout, uri, text)?;
                        }
                    }
                }
            }
        }
        "textDocument/didClose" => {
            if let Some(params) = msg.get("params") {
                if let Some(uri) = params.get("textDocument").and_then(|d| d.get("uri")).and_then(|u| u.as_str()) {
                    documents.remove(uri);
                    // Clear diagnostics on close
                    send_notification(stdout, "textDocument/publishDiagnostics", json!({
                        "uri": uri,
                        "diagnostics": []
                    }))?;
                }
            }
        }
        "textDocument/semanticTokens/full" => {
            if let Some(req_id) = id {
                let uri = msg.get("params")
                    .and_then(|p| p.get("textDocument"))
                    .and_then(|d| d.get("uri"))
                    .and_then(|u| u.as_str());

                let tokens_data = if let Some(uri_str) = uri {
                    if let Some(source) = documents.get(uri_str) {
                        compute_semantic_tokens(source)
                    } else {
                        Vec::new()
                    }
                } else {
                    Vec::new()
                };

                send_response(stdout, req_id, json!({
                    "data": tokens_data
                }))?;
            }
        }
        _ => {
            // Unhandled request: respond with method not found if it was a request
            if let Some(req_id) = id {
                let err_resp = json!({
                    "jsonrpc": "2.0",
                    "id": req_id,
                    "error": {
                        "code": -32601,
                        "message": format!("Method not found: {}", method)
                    }
                });
                let payload = serde_json::to_string(&err_resp)?;
                write!(
                    stdout,
                    "Content-Length: {}\r\n\r\n{}",
                    payload.len(),
                    payload
                )?;
                stdout.flush()?;
            }
        }
    }

    Ok(())
}

fn publish_diagnostics(
    stdout: &mut dyn Write,
    uri: &str,
    source: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut diagnostics = Vec::new();

    let file_path = if uri.starts_with("file://") {
        &uri["file://".len()..]
    } else {
        uri
    };

    match Lexer::new_with_file(source.to_string(), file_path).tokenize_with_spans() {
        Err(err_msg) => {
            diagnostics.push(parse_error_to_diagnostic(&err_msg));
        }
        Ok((tokens, spans)) => {
            let mut parser = Parser::new_with_spans(tokens, spans);
            if let Err(err_msg) = parser.parse_file_declarations(file_path) {
                let span = parser.current_span();
                diagnostics.push(Diagnostic {
                    range: Range {
                        start: Position {
                            line: span.line.saturating_sub(1) as u32,
                            character: span.col.saturating_sub(1) as u32,
                        },
                        end: Position {
                            line: span.line.saturating_sub(1) as u32,
                            character: span.col as u32,
                        },
                    },
                    severity: 1,
                    source: "hart".to_string(),
                    message: err_msg,
                });
            }
        }
    }

    send_notification(stdout, "textDocument/publishDiagnostics", json!({
        "uri": uri,
        "diagnostics": diagnostics
    }))?;

    Ok(())
}

fn parse_error_to_diagnostic(error_str: &str) -> Diagnostic {
    let mut line = 0u32;
    let mut col = 0u32;
    let mut message = error_str.to_string();

    for part in error_str.split('\n') {
        let segments: Vec<&str> = part.split(':').collect();
        for i in 0..segments.len().saturating_sub(2) {
            if let (Ok(l), Ok(c)) = (
                segments[i].trim().parse::<u32>(),
                segments[i + 1].trim().parse::<u32>(),
            ) {
                line = l.saturating_sub(1);
                col = c.saturating_sub(1);
                if let Some(pos) = part.find("error:") {
                    message = part[pos + 6..].trim().to_string();
                } else {
                    message = part.to_string();
                }
                break;
            }
        }
        if line != 0 || col != 0 {
            break;
        }
    }

    Diagnostic {
        range: Range {
            start: Position { line, character: col },
            end: Position {
                line,
                character: col.saturating_add(1),
            },
        },
        severity: 1, // Error
        source: "hart".to_string(),
        message,
    }
}

pub fn compute_semantic_tokens(source: &str) -> Vec<u32> {
    let mut raw_tokens: Vec<RawToken> = Vec::new();

    // 1. Lex Hart tokens
    if let Ok((tokens, spans)) = Lexer::new(source.to_string()).tokenize_with_spans() {
        for (i, token) in tokens.iter().enumerate() {
            let next_token = tokens.get(i + 1);
            if let Some((type_idx, len)) = classify_token(token, next_token) {
                let span = &spans[i];
                raw_tokens.push(RawToken {
                    line: span.line.saturating_sub(1) as u32,
                    col: span.col.saturating_sub(1) as u32,
                    len: len as u32,
                    token_type: type_idx,
                });
            }
        }
    }

    // 2. Scan comments (# ...)
    for (line_idx, line_str) in source.lines().enumerate() {
        if let Some(hash_pos) = line_str.find('#') {
            raw_tokens.push(RawToken {
                line: line_idx as u32,
                col: hash_pos as u32,
                len: (line_str.len() - hash_pos) as u32,
                token_type: 7, // comment
            });
        }
    }

    // 3. Sort by (line, col)
    raw_tokens.sort_by(|a, b| {
        a.line.cmp(&b.line).then_with(|| a.col.cmp(&b.col))
    });

    // 4. Compute LSP delta encoding
    let mut data = Vec::with_capacity(raw_tokens.len() * 5);
    let mut prev_line = 0u32;
    let mut prev_col = 0u32;

    for tok in raw_tokens {
        let delta_line = tok.line - prev_line;
        let delta_start = if delta_line == 0 {
            tok.col.saturating_sub(prev_col)
        } else {
            tok.col
        };

        data.push(delta_line);
        data.push(delta_start);
        data.push(tok.len);
        data.push(tok.token_type);
        data.push(0); // modifiers bitflag

        prev_line = tok.line;
        prev_col = tok.col;
    }

    data
}

fn classify_token(token: &Token, next_token: Option<&Token>) -> Option<(u32, usize)> {
    match token {
        Token::FnKeyword => Some((0, 4)),
        Token::Def => Some((0, 3)),
        Token::Return => Some((0, 6)),
        Token::Yield => Some((0, 5)),
        Token::Break => Some((0, 5)),
        Token::Continue => Some((0, 8)),
        Token::Repeat => Some((0, 6)),
        Token::While => Some((0, 5)),
        Token::Until => Some((0, 5)),
        Token::For => Some((0, 3)),
        Token::If => Some((0, 2)),
        Token::Else => Some((0, 4)),
        Token::Match => Some((0, 5)),
        Token::Switch => Some((0, 6)),
        Token::Using => Some((0, 5)),
        Token::Mut => Some((0, 3)),
        Token::Decl => Some((0, 4)),
        Token::Mod => Some((0, 3)),
        Token::Export => Some((0, 6)),
        Token::Import => Some((0, 6)),
        Token::To => Some((0, 2)),
        Token::Const => Some((0, 5)),
        Token::TypeKeyword => Some((0, 4)),
        Token::Extern => Some((0, 6)),
        Token::As => Some((0, 2)),
        Token::At => Some((0, 1)),

        Token::TypeOpaque => Some((1, 6)),
        Token::Never => Some((1, 5)),
        Token::TypeS16 => Some((1, 3)),
        Token::TypeS32 => Some((1, 3)),
        Token::TypeS64 => Some((1, 3)),
        Token::TypeU8 => Some((1, 2)),
        Token::TypeU16 => Some((1, 3)),
        Token::TypeU32 => Some((1, 3)),
        Token::TypeU64 => Some((1, 3)),
        Token::TypeUSize => Some((1, 5)),
        Token::TypeSSize => Some((1, 5)),

        Token::Symbol(s) => Some((9, s.len() + 1)), // atom :foo -> enumMember
        Token::String(bytes) => Some((4, bytes.len() + 2)), // string
        Token::Integer(n) => Some((5, n.to_string().len())),
        Token::Float(fl) => Some((5, fl.to_string().len())),

        Token::Identifier(s) => {
            if s.chars().next().map_or(false, |c| c.is_uppercase()) {
                Some((1, s.len())) // type
            } else if matches!(next_token, Some(Token::LParen)) {
                Some((2, s.len())) // function
            } else {
                Some((3, s.len())) // variable
            }
        }

        Token::ColonEqual
        | Token::PlusEqual
        | Token::MinusEqual
        | Token::StarEqual
        | Token::SlashEqual => Some((6, 2)),
        Token::Equal => Some((6, 1)),
        Token::NotEqual => Some((6, 2)),
        Token::Less => Some((6, 1)),
        Token::LessEqual => Some((6, 2)),
        Token::Greater => Some((6, 1)),
        Token::GreaterEqual => Some((6, 2)),
        Token::Plus => Some((6, 1)),
        Token::Minus => Some((6, 1)),
        Token::Star => Some((6, 1)),
        Token::Slash => Some((6, 1)),
        Token::Percent => Some((6, 1)),
        Token::Ampersand => Some((6, 1)),
        Token::Caret => Some((6, 1)),
        Token::Pipe => Some((6, 1)),
        Token::FatArrow => Some((6, 2)),
        Token::Arrow => Some((6, 2)),
        Token::DotDot => Some((6, 2)),
        Token::DotDotDot => Some((6, 3)),

        _ => None,
    }
}
