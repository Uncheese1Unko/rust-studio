//! studio-mcp バイナリ: stdio で MCP セッションを処理する薄い入口。
//!
//! `--project-file <path>` を付けると、そのファイルをプロジェクトの
//! backing にする（Issue #11、ADR-0010）。video-editor も同じファイルを
//! 読むため、MCP の編集結果が GUI に現れる。

use std::io::{BufReader, BufWriter};

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let server = match args.iter().position(|a| a == "--project-file") {
        Some(i) => {
            let path = args.get(i + 1).map(std::path::PathBuf::from);
            match path {
                Some(path) => studio_mcp::McpServer::with_project_file(path),
                None => {
                    eprintln!("error: --project-file requires a path argument");
                    std::process::exit(2);
                }
            }
        }
        None => studio_mcp::McpServer::new(),
    };
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    server.serve_stdio(BufReader::new(stdin.lock()), BufWriter::new(stdout.lock()))
}
