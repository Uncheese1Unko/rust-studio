//! studio-api バイナリ: HTTP/JSON API サーバー（Issue #4）。
//!
//! 使い方: `studio-api [addr]`（既定は `127.0.0.1:17890`）。
//! ローカルでの検証と computer use テストのための独立プロセス。
//! ロジックはすべてライブラリ（`studio_api`）と studio-core 側にある。

use studio_api::ApiState;
use studio_core::Project;

fn main() -> std::io::Result<()> {
    let addr = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:17890".to_string());
    let listener = std::net::TcpListener::bind(&addr)?;
    println!("studio-api listening on http://{addr}");
    studio_api::serve(listener, ApiState::new(Project::new("untitled")))
}
