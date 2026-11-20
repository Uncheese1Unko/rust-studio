//! daw: Reaper 風の DAW（nannou）。
//!
//! GUI とオーディオ再生は M1 の PBI で実装する。先に調査 PBI
//! 「nannou のビルドと最小ウィンドウ表示の検証」で前提を確認してから
//! 着手すること（`.claude/rules/issue-authoring.md` §1）。
//! 状態の変更は studio-core のコマンド層経由でのみ行う（ADR-0004）。

fn main() {
    let project = studio_core::Project::new("untitled session");
    println!("daw (skeleton)");
    println!("{}", project.dump_json());
}
