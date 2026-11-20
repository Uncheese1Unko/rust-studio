//! video-editor: AviUtl 風の動画編集ソフト（nannou）。
//!
//! GUI は M1 の PBI で実装する。先に調査 PBI「nannou のビルドと最小
//! ウィンドウ表示の検証」で前提を確認してから着手すること
//! （`.claude/rules/issue-authoring.md` §1）。
//! 状態の変更は studio-core のコマンド層経由でのみ行う（ADR-0004）。

fn main() {
    let project = studio_core::Project::new("untitled video");
    println!("video-editor (skeleton)");
    println!("{}", project.dump_json());
}
