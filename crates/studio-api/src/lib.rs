//! studio-api: HTTP/JSON API（Issue #4、ADR-0004 / ADR-0005 / ADR-0008）。
//!
//! プロセスの外から studio-core のコマンドを実行し、プロジェクト状態の
//! 全量ダンプを観測するための経路。意味論はすべてコマンド層が持ち、
//! ここは HTTP と JSON の変換だけを行う薄いラッパーである。
//!
//! エンドポイント:
//! - `GET /project` — 状態の全量ダンプ（JSON）を返す
//! - `POST /commands` — コマンド JSON を適用し、成否を返す。
//!   成功: `200 {"ok":true,"created_id":<id|null>}`
//!   失敗: `400 {"ok":false,"error":"..."}`（状態は変わらない）

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Mutex;

use studio_core::{apply, Command, Project};

/// API が保持するプロジェクト状態。変更はコマンド経由でのみ行う。
pub struct ApiState {
    project: Mutex<Project>,
}

impl ApiState {
    pub fn new(project: Project) -> Self {
        ApiState {
            project: Mutex::new(project),
        }
    }
}

/// HTTP から切り離したリクエスト表現（テスト可能性のため）。
pub struct Request {
    pub method: String,
    pub path: String,
    pub body: Vec<u8>,
}

/// HTTP から切り離したレスポンス表現。
pub struct Response {
    pub status: u16,
    pub body: String,
}

fn error_response(status: u16, message: &str) -> Response {
    Response {
        status,
        body: serde_json::json!({ "ok": false, "error": message }).to_string(),
    }
}

/// リクエストを処理する。状態の変更は `studio_core::apply` 経由でのみ行う。
pub fn handle_request(state: &ApiState, request: &Request) -> Response {
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/project") => {
            let project = state.project.lock().expect("project lock poisoned");
            Response {
                status: 200,
                body: project.dump_json(),
            }
        }
        ("POST", "/commands") => {
            let command: Command = match serde_json::from_slice(&request.body) {
                Ok(c) => c,
                Err(e) => {
                    return error_response(400, &format!("invalid command JSON: {e}"));
                }
            };
            let mut project = state.project.lock().expect("project lock poisoned");
            match apply(&mut project, &command) {
                Ok(created_id) => Response {
                    status: 200,
                    body: serde_json::json!({ "ok": true, "created_id": created_id }).to_string(),
                },
                Err(e) => error_response(400, &e.to_string()),
            }
        }
        (_, "/project") | (_, "/commands") => error_response(405, "method not allowed"),
        _ => error_response(404, "not found"),
    }
}

fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Internal Server Error",
    }
}

/// 1リクエスト全体の読み取り期限。これを超える接続は諦めて次へ進む
/// （何も送らない接続が逐次処理のサーバーを固まらせないため）。
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// 期限までの残り時間を読み取りタイムアウトに設定して1回読む。
fn read_with_deadline(
    stream: &mut TcpStream,
    chunk: &mut [u8],
    started: std::time::Instant,
) -> std::io::Result<usize> {
    let remaining = REQUEST_TIMEOUT.checked_sub(started.elapsed());
    match remaining {
        Some(remaining) if !remaining.is_zero() => {
            stream.set_read_timeout(Some(remaining))?;
            stream.read(chunk)
        }
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "request deadline exceeded",
        )),
    }
}

/// 1接続分の生 HTTP を読み、[`Request`] に変換する。
/// 対応するのは `Content-Length` を持つ単純なリクエストのみ。
/// 読み取り全体は [`REQUEST_TIMEOUT`] で打ち切る。
fn read_request(stream: &mut TcpStream) -> std::io::Result<Option<Request>> {
    let started = std::time::Instant::now();
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        if let Some(pos) = find_subsequence(&buf, b"\r\n\r\n") {
            break pos + 4;
        }
        let n = read_with_deadline(stream, &mut chunk, started)?;
        if n == 0 {
            return Ok(None);
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.len() > 64 * 1024 {
            return Ok(None);
        }
    };

    let header_text = String::from_utf8_lossy(&buf[..header_end]).into_owned();
    let mut lines = header_text.lines();
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    // クエリ文字列はルーティングに使わないので落とす
    let path = parts
        .next()
        .unwrap_or("")
        .split('?')
        .next()
        .unwrap_or("")
        .to_string();
    let mut content_length = 0usize;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse().unwrap_or(0);
            }
        }
    }

    let mut body = buf[header_end..].to_vec();
    while body.len() < content_length {
        let n = read_with_deadline(stream, &mut chunk, started)?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(content_length);

    Ok(Some(Request { method, path, body }))
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// 1接続分の処理。接続単位の I/O エラーはここで閉じ、サーバー全体は
/// 止めない（乱暴に切断するクライアントが観測経路を道連れにしないため）。
fn handle_connection(stream: &mut TcpStream, state: &ApiState) -> std::io::Result<()> {
    match read_request(stream)? {
        Some(request) => {
            let response = handle_request(state, &request);
            let text = format!(
                "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response.status,
                reason_phrase(response.status),
                response.body.len(),
                response.body
            );
            stream.write_all(text.as_bytes())
        }
        None => Ok(()),
    }
}

/// TCP リスナーで HTTP リクエストを受け続ける（接続は逐次処理）。
/// 受け付けや接続単位の失敗は記録して次に進み、ループは止めない。
pub fn serve(listener: TcpListener, state: ApiState) -> std::io::Result<()> {
    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                if let Err(e) = handle_connection(&mut stream, &state) {
                    eprintln!("studio-api: connection error: {e}");
                }
            }
            Err(e) => {
                eprintln!("studio-api: accept error: {e}");
            }
        }
    }
    Ok(())
}
