//! スクリプト／拡張のインストールと管理（Issue #82）。
//!
//! このモジュールは studio-scripts クレートにあり、video-editor と
//! studio-mcp が共有する（一覧の形が経路でずれないため。#82
//! レビュー round 1 指摘への対応としてアプリ内モジュールから移設）。
//!
//! スクリプトは所定フォルダ（アプリ側の `scripts_dir`）への配置で認識する。
//! 本体はプロジェクトに埋め込まず、クリップは名前（または相対パス）で
//! 参照するだけ（#81）。ここではフォルダの走査・一覧化、壊れた
//! スクリプトの隔離表示、有効／無効の管理、プロジェクト参照の
//! 欠落検出を担う。
//!
//! 有効／無効の保持先は**アプリ側設定**とした（判断の記録は試験
//! レポート issue-0082）: スクリプトフォルダ内の
//! `disabled-scripts.txt`（1行1名）に無効な名前を保持する。
//! プロジェクトファイルには入れないため、プロジェクトを共有しても
//! 「自分の環境で入れた拡張を止める」というローカルな判断は混ざらない。

use std::collections::BTreeSet;
use std::path::Path;

use studio_core::Project;

use crate::engine::ScriptEngine;

/// 無効化したスクリプト名を保持するファイル（スクリプトフォルダ内）。
pub const DISABLED_FILE_NAME: &str = "disabled-scripts.txt";

/// 一覧に載せるスクリプトの拡張子（小文字、ドットなし）。
/// AviUtl 形式（anm）は #83 でここへ足し、同じ一覧で扱う。
pub const SCRIPT_EXTENSIONS: [&str; 3] = ["lua", "anm", "obj"];

/// スクリプトの読み込み状態。壊れたスクリプトは他のスクリプトの
/// 読み込みを妨げず、エラーとして区別して一覧に出す。
#[derive(Debug, Clone, PartialEq)]
pub enum ScriptStatus {
    Ready,
    Error(String),
}

/// 一覧の1件。
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptEntry {
    pub name: String,
    pub enabled: bool,
    pub status: ScriptStatus,
}

/// プロジェクトが参照しているスクリプトの状態。
#[derive(Debug, Clone, PartialEq)]
pub enum ReferenceStatus {
    Ready,
    Disabled,
    Error(String),
    Missing,
}

/// プロジェクト参照1件（重複は畳む）。
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptReference {
    pub script: String,
    pub status: ReferenceStatus,
}

/// パネル表示行の種別。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScriptRowKind {
    Ready,
    Disabled,
    Error,
    Missing,
}

/// パネル表示行（エントリ＋欠落参照を1列に並べる）。
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptRow {
    pub name: String,
    pub kind: ScriptRowKind,
}

/// スクリプト参照を実ファイルに解決する。絶対パスはそのまま、
/// それ以外はスクリプトフォルダ内の名前として解決する。
/// 適用経路（アプリ側 media）と参照状態の判定で同じ規則を使う。
pub fn resolve_script_in(dir: &Path, reference: &str) -> std::path::PathBuf {
    let as_given = Path::new(reference);
    if as_given.is_absolute() {
        return as_given.to_path_buf();
    }
    dir.join(reference)
}

/// 既定のスクリプトフォルダ。アプリ（video-editor）と MCP サーバーは
/// 別プロセスのため、この規約は共有クレート側の単一の定義に寄せる
/// （#82 レビュー round 2 指摘: パス規約の複製で観測先がずれない
/// ようにする）。アプリのメディア作業フォルダ配下の `scripts`。
pub fn default_scripts_dir() -> std::path::PathBuf {
    std::env::temp_dir()
        .join("rust-studio-video-editor-media")
        .join("scripts")
}

/// スクリプトフォルダを走査して一覧を作る。フォルダが無ければ空。
/// 壊れたスクリプト（読めない／構文エラー）は Error として隔離し、
/// 他のスクリプトの読み込みは妨げない。
pub fn scan_scripts(dir: &Path) -> Vec<ScriptEntry> {
    let disabled = read_disabled(dir);
    let mut entries: Vec<ScriptEntry> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for entry in rd.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase());
            if !SCRIPT_EXTENSIONS.contains(&ext.as_deref().unwrap_or("")) {
                continue;
            }
            let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
                continue;
            };
            let status = match std::fs::read(&path) {
                Ok(bytes) => {
                    let engine = ScriptEngine::new();
                    match ext.as_deref() {
                        // AviUtl 形式（#83）: SJIS バイト列のまま、
                        // セクション単位で構文を検証する。
                        Some("anm") | Some("obj") => {
                            match crate::aviutl::check_anm_syntax(&engine, &bytes) {
                                Some(message) => ScriptStatus::Error(message),
                                None => ScriptStatus::Ready,
                            }
                        }
                        _ => match String::from_utf8(bytes) {
                            Ok(source) => match engine.check_syntax(&source) {
                                Ok(()) => ScriptStatus::Ready,
                                Err(e) => ScriptStatus::Error(e.message),
                            },
                            Err(_) => ScriptStatus::Error("UTF-8 として読めません".into()),
                        },
                    }
                }
                Err(e) => ScriptStatus::Error(format!("読み込めません: {e}")),
            };
            entries.push(ScriptEntry {
                enabled: !disabled.contains(&name),
                name,
                status,
            });
        }
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    entries
}

/// 無効化されているスクリプト名の集合を読む。ファイルが無ければ空。
pub fn read_disabled(dir: &Path) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    if let Ok(text) = std::fs::read_to_string(dir.join(DISABLED_FILE_NAME)) {
        for line in text.lines() {
            let name = line.trim();
            if !name.is_empty() {
                set.insert(name.to_string());
            }
        }
    }
    set
}

/// スクリプトが有効かどうか（無効リストに入っていなければ有効）。
pub fn is_enabled(dir: &Path, name: &str) -> bool {
    !read_disabled(dir).contains(name)
}

/// 有効／無効を切り替えて保持する。無効リストは名前順で書き出す。
pub fn set_enabled(dir: &Path, name: &str, enabled: bool) -> std::io::Result<()> {
    let mut disabled = read_disabled(dir);
    if enabled {
        disabled.remove(name);
    } else {
        disabled.insert(name.to_string());
    }
    std::fs::create_dir_all(dir)?;
    let mut text = String::new();
    for name in &disabled {
        text.push_str(name);
        text.push('\n');
    }
    std::fs::write(dir.join(DISABLED_FILE_NAME), text)
}

/// プロジェクトが参照するスクリプトの状態を、参照ごとに畳んで
/// 名前順に返す。見つからない参照は Missing として明示する。
pub fn reference_statuses(project: &Project, dir: &Path) -> Vec<ScriptReference> {
    let mut names: Vec<String> = Vec::new();
    for track in &project.tracks {
        for clip in &track.clips {
            if let Some(effect) = &clip.script_effect {
                if !names.contains(&effect.script) {
                    names.push(effect.script.clone());
                }
            }
        }
    }
    names.sort();
    let entries = scan_scripts(dir);
    names
        .into_iter()
        .map(|script| {
            let status = reference_status(&script, dir, &entries);
            ScriptReference { script, status }
        })
        .collect()
}

/// 参照1件の状態を決める。解決は `media::resolve_script_in` と同じ
/// 規則（絶対パスはそのまま、それ以外はフォルダ内の名前）。
/// AviUtl 形式のセクション参照（`ファイル.anm@名前`、#83）は
/// ファイル部で判定する。
fn reference_status(reference: &str, dir: &Path, entries: &[ScriptEntry]) -> ReferenceStatus {
    let (file_ref, _section) = crate::aviutl::split_reference(reference);
    let resolved = resolve_script_in(dir, file_ref);
    if !resolved.is_file() {
        return ReferenceStatus::Missing;
    }
    let file_name = resolved
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if let Some(entry) = entries.iter().find(|e| e.name == file_name) {
        // 壊れていることは無効化より優先して区別する
        // （display_rows と同じ優先順位。#82 レビュー round 2）。
        if let ScriptStatus::Error(msg) = &entry.status {
            return ReferenceStatus::Error(msg.clone());
        }
        if !entry.enabled {
            return ReferenceStatus::Disabled;
        }
        return ReferenceStatus::Ready;
    }
    // 一覧の対象外（フォルダ外の絶対パス参照など）は、その場で
    // 構文だけ確かめる。無効リストはファイル名で効く。
    if !is_enabled(dir, &file_name) {
        return ReferenceStatus::Disabled;
    }
    match std::fs::read_to_string(&resolved) {
        Ok(source) => match ScriptEngine::new().check_syntax(&source) {
            Ok(()) => ReferenceStatus::Ready,
            Err(e) => ReferenceStatus::Error(e.message),
        },
        Err(e) => ReferenceStatus::Error(format!("読み込めません: {e}")),
    }
}

/// パネル表示行を作る。一覧のエントリをそのまま並べ、一覧に無い
/// 欠落参照を末尾に Missing 行として足す。
pub fn display_rows(entries: &[ScriptEntry], references: &[ScriptReference]) -> Vec<ScriptRow> {
    let mut rows: Vec<ScriptRow> = entries
        .iter()
        .map(|e| ScriptRow {
            name: e.name.clone(),
            // 壊れていることは無効化より優先して区別する
            // （無効＋エラーの併存でエラーが見えなくならないため。
            // #82 レビュー round 1 Minor 指摘）。
            kind: if matches!(e.status, ScriptStatus::Error(_)) {
                ScriptRowKind::Error
            } else if !e.enabled {
                ScriptRowKind::Disabled
            } else {
                ScriptRowKind::Ready
            },
        })
        .collect();
    for reference in references {
        if reference.status == ReferenceStatus::Missing
            && !rows.iter().any(|r| r.name == reference.script)
        {
            rows.push(ScriptRow {
                name: reference.script.clone(),
                kind: ScriptRowKind::Missing,
            });
        }
    }
    rows
}

/// 観測用ダンプ（JSON）。GUI の標準出力と状態確認に使う。
pub fn dump_json(entries: &[ScriptEntry], references: &[ScriptReference]) -> String {
    let status_text = |s: &ScriptStatus| match s {
        ScriptStatus::Ready => serde_json::json!("ready"),
        ScriptStatus::Error(msg) => serde_json::json!({ "error": msg }),
    };
    let ref_status_text = |s: &ReferenceStatus| match s {
        ReferenceStatus::Ready => serde_json::json!("ready"),
        ReferenceStatus::Disabled => serde_json::json!("disabled"),
        ReferenceStatus::Missing => serde_json::json!("missing"),
        ReferenceStatus::Error(msg) => serde_json::json!({ "error": msg }),
    };
    serde_json::json!({
        "scripts": entries
            .iter()
            .map(|e| serde_json::json!({
                "name": e.name,
                "enabled": e.enabled,
                "status": status_text(&e.status),
            }))
            .collect::<Vec<_>>(),
        "references": references
            .iter()
            .map(|r| serde_json::json!({
                "script": r.script,
                "status": ref_status_text(&r.status),
            }))
            .collect::<Vec<_>>(),
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use studio_core::{apply, Command, ScriptEffect};

    fn scripts_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("studio-ext-{}-{}", std::process::id(), name));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(dir: &Path, name: &str, source: &str) {
        std::fs::write(dir.join(name), source).unwrap();
    }

    fn project_with_refs(refs: &[&str]) -> Project {
        let mut p = Project::new("ext-test");
        apply(
            &mut p,
            &Command::AddTrack {
                name: "V1".to_string(),
                kind: studio_core::TrackKind::Video,
            },
        )
        .unwrap();
        let track_id = p.tracks[0].id;
        for (i, r) in refs.iter().enumerate() {
            apply(
                &mut p,
                &Command::AddClip {
                    track_id,
                    name: format!("clip{i}"),
                    start_ms: (i as i64) * 1000,
                    duration_ms: 500,
                    source: "demo/opening.mp4".to_string(),
                },
            )
            .unwrap();
            let clip_id = p.tracks[0].clips[i].id;
            apply(
                &mut p,
                &Command::SetClipScriptEffect {
                    clip_id,
                    effect: Some(ScriptEffect {
                        script: r.to_string(),
                        params: Default::default(),
                    }),
                },
            )
            .unwrap();
        }
        p
    }

    #[test]
    fn default_scripts_dir_follows_the_shared_convention() {
        // アプリと MCP で同じフォルダを指すための単一の規約。
        let dir = default_scripts_dir();
        assert!(
            dir.ends_with("rust-studio-video-editor-media/scripts"),
            "{dir:?}"
        );
    }

    #[test]
    fn scan_lists_lua_files_sorted_and_ignores_others() {
        let dir = scripts_dir("scan");
        write(&dir, "b.lua", "return 1");
        write(&dir, "a.lua", "return 2");
        write(&dir, "note.txt", "not a script");
        write(&dir, DISABLED_FILE_NAME, "");
        let entries = scan_scripts(&dir);
        assert_eq!(
            entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
            vec!["a.lua", "b.lua"]
        );
        assert!(entries.iter().all(|e| e.enabled));
        assert!(entries.iter().all(|e| e.status == ScriptStatus::Ready));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn scan_recognizes_anm_and_flags_broken_body() {
        // AviUtl 形式（#83）: SJIS の .anm も一覧に出て、構文が
        // 壊れたものは Error として区別される。
        let dir = scripts_dir("scan-anm");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("ok.anm"),
            crate::aviutl::encode_sjis("@図形\nobj.draw()\n"),
        )
        .unwrap();
        std::fs::write(
            dir.join("broken.obj"),
            crate::aviutl::encode_sjis("@図形\nreturn 1 +\n"),
        )
        .unwrap();
        let entries = scan_scripts(&dir);
        assert_eq!(entries.len(), 2);
        let ok = entries.iter().find(|e| e.name == "ok.anm").unwrap();
        assert_eq!(ok.status, ScriptStatus::Ready);
        let broken = entries.iter().find(|e| e.name == "broken.obj").unwrap();
        assert!(
            matches!(broken.status, ScriptStatus::Error(_)),
            "{:?}",
            broken.status
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn scan_missing_dir_is_empty() {
        let dir = scripts_dir("scan-missing");
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(scan_scripts(&dir).is_empty());
    }

    #[test]
    fn scan_isolates_broken_script_as_error() {
        let dir = scripts_dir("broken");
        write(&dir, "good.lua", "return 1");
        write(&dir, "bad.lua", "return 1 +");
        let entries = scan_scripts(&dir);
        assert_eq!(entries.len(), 2);
        let good = entries.iter().find(|e| e.name == "good.lua").unwrap();
        assert_eq!(good.status, ScriptStatus::Ready);
        let bad = entries.iter().find(|e| e.name == "bad.lua").unwrap();
        match &bad.status {
            ScriptStatus::Error(msg) => assert!(!msg.is_empty()),
            other => panic!("bad.lua は Error のはず: {other:?}"),
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn set_enabled_round_trips_through_scan() {
        let dir = scripts_dir("toggle");
        write(&dir, "a.lua", "return 1");
        write(&dir, "b.lua", "return 2");
        set_enabled(&dir, "a.lua", false).unwrap();
        assert!(!is_enabled(&dir, "a.lua"));
        assert!(is_enabled(&dir, "b.lua"));
        let entries = scan_scripts(&dir);
        assert!(!entries.iter().find(|e| e.name == "a.lua").unwrap().enabled);
        assert!(entries.iter().find(|e| e.name == "b.lua").unwrap().enabled);
        // 再び有効化すると一覧から消える（ファイル自体は残る）
        set_enabled(&dir, "a.lua", true).unwrap();
        assert!(is_enabled(&dir, "a.lua"));
        assert!(scan_scripts(&dir).iter().all(|e| e.enabled));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reference_statuses_report_missing_ready_disabled_and_error() {
        let dir = scripts_dir("refs");
        write(&dir, "ok.lua", "return 1");
        write(&dir, "off.lua", "return 2");
        write(&dir, "bad.lua", "return 1 +");
        set_enabled(&dir, "off.lua", false).unwrap();
        let project = project_with_refs(&["ok.lua", "off.lua", "bad.lua", "ghost.lua", "ok.lua"]);
        let refs = reference_statuses(&project, &dir);
        // 重複は畳まれ、名前順
        assert_eq!(
            refs.iter().map(|r| r.script.as_str()).collect::<Vec<_>>(),
            vec!["bad.lua", "ghost.lua", "off.lua", "ok.lua"]
        );
        let by_name = |n: &str| refs.iter().find(|r| r.script == n).unwrap();
        assert_eq!(by_name("ok.lua").status, ReferenceStatus::Ready);
        assert_eq!(by_name("off.lua").status, ReferenceStatus::Disabled);
        assert_eq!(by_name("ghost.lua").status, ReferenceStatus::Missing);
        assert!(matches!(
            by_name("bad.lua").status,
            ReferenceStatus::Error(_)
        ));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reference_status_resolves_anm_section_reference_by_file() {
        // `ファイル.anm@セクション` 参照はファイル部で判定する
        // （#83 の実アプリ駆動で検出: セクション参照が missing に
        // なっていた）。
        let dir = scripts_dir("refs-section");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("fx.anm"),
            crate::aviutl::encode_sjis("@図形\nobj.draw()\n"),
        )
        .unwrap();
        let project = project_with_refs(&["fx.anm@図形"]);
        let refs = reference_statuses(&project, &dir);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].status, ReferenceStatus::Ready);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reference_status_prioritizes_error_over_disabled() {
        // 無効化されていても壊れていることは区別できること
        // （パネルの display_rows と同じ優先順位。#82 レビュー
        // round 2 Minor 指摘: ダンプ／MCP と画面で観測がずれない）。
        let dir = scripts_dir("refs-err-off");
        write(&dir, "bad.lua", "return 1 +");
        set_enabled(&dir, "bad.lua", false).unwrap();
        let project = project_with_refs(&["bad.lua"]);
        let refs = reference_statuses(&project, &dir);
        assert_eq!(refs.len(), 1);
        assert!(
            matches!(refs[0].status, ReferenceStatus::Error(_)),
            "壊れたスクリプトが disabled に隠れた: {:?}",
            refs[0].status
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn display_rows_merge_entries_and_missing_references() {
        let entries = vec![
            ScriptEntry {
                name: "a.lua".to_string(),
                enabled: true,
                status: ScriptStatus::Ready,
            },
            ScriptEntry {
                name: "b.lua".to_string(),
                enabled: false,
                status: ScriptStatus::Ready,
            },
            ScriptEntry {
                name: "c.lua".to_string(),
                enabled: true,
                status: ScriptStatus::Error("syntax".to_string()),
            },
        ];
        let references = vec![
            ScriptReference {
                script: "a.lua".to_string(),
                status: ReferenceStatus::Ready,
            },
            ScriptReference {
                script: "ghost.lua".to_string(),
                status: ReferenceStatus::Missing,
            },
        ];
        let rows = display_rows(&entries, &references);
        assert_eq!(
            rows,
            vec![
                ScriptRow {
                    name: "a.lua".to_string(),
                    kind: ScriptRowKind::Ready
                },
                ScriptRow {
                    name: "b.lua".to_string(),
                    kind: ScriptRowKind::Disabled
                },
                ScriptRow {
                    name: "c.lua".to_string(),
                    kind: ScriptRowKind::Error
                },
                ScriptRow {
                    name: "ghost.lua".to_string(),
                    kind: ScriptRowKind::Missing
                },
            ]
        );
    }

    #[test]
    fn display_rows_prioritize_error_over_disabled() {
        let entries = vec![ScriptEntry {
            name: "bad.lua".to_string(),
            enabled: false,
            status: ScriptStatus::Error("syntax".to_string()),
        }];
        let rows = display_rows(&entries, &[]);
        assert_eq!(
            rows,
            vec![ScriptRow {
                name: "bad.lua".to_string(),
                kind: ScriptRowKind::Error
            }]
        );
    }

    #[test]
    fn dump_json_contains_entries_and_references() {
        let entries = vec![ScriptEntry {
            name: "a.lua".to_string(),
            enabled: false,
            status: ScriptStatus::Ready,
        }];
        let references = vec![ScriptReference {
            script: "ghost.lua".to_string(),
            status: ReferenceStatus::Missing,
        }];
        let dump = dump_json(&entries, &references);
        let value: serde_json::Value = serde_json::from_str(&dump).unwrap();
        assert_eq!(value["scripts"][0]["name"], "a.lua");
        assert_eq!(value["scripts"][0]["enabled"], false);
        assert_eq!(value["scripts"][0]["status"], "ready");
        assert_eq!(value["references"][0]["script"], "ghost.lua");
        assert_eq!(value["references"][0]["status"], "missing");
    }
}
