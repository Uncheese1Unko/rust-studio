//! studio-mcp バイナリ: stdio で MCP セッションを処理する薄い入口。

use std::io::{BufReader, BufWriter};

fn main() -> std::io::Result<()> {
    let server = studio_mcp::McpServer::new();
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    server.serve_stdio(BufReader::new(stdin.lock()), BufWriter::new(stdout.lock()))
}
