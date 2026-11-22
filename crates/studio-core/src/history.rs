//! コマンド履歴（Issue #29）: Undo/Redo の基盤。
//!
//! 適用前スナップショット方式。コマンドの適用は単一の変更経路である
//! [`crate::command::apply`] を通し、成功したときだけ適用前の状態を
//! undo スタックに積む。失敗したコマンドは状態も履歴も変えない。
//! 履歴は永続化しない（プロジェクト JSON の形式は変わらない）。

use crate::command::{apply, Command, CommandError};
use crate::model::Project;

/// 保持するスナップショットの上限。超過したら古いものから捨てる。
pub const HISTORY_LIMIT: usize = 100;

/// プロジェクトとその undo/redo 履歴。
#[derive(Debug, Clone)]
pub struct History {
    project: Project,
    undo_stack: Vec<Project>,
    redo_stack: Vec<Project>,
}

impl History {
    pub fn new(project: Project) -> Self {
        History {
            project,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    /// 現在のプロジェクト（読み取り専用）。
    pub fn project(&self) -> &Project {
        &self.project
    }

    /// コマンドを適用する。成功したら適用前の状態を履歴に積み、
    /// redo スタックを消す。失敗したら状態も履歴も変えない。
    pub fn apply(&mut self, command: &Command) -> Result<Option<u64>, CommandError> {
        let snapshot = self.project.clone();
        match apply(&mut self.project, command) {
            Ok(created_id) => {
                self.push_snapshot(snapshot);
                Ok(created_id)
            }
            Err(e) => {
                // apply は失敗時に状態を変えない契約だが、履歴側でも
                // スナップショットへ戻して契約を二重に守る。
                self.project = snapshot;
                Err(e)
            }
        }
    }

    /// 履歴に積まずにコマンドを適用する（Issue #31: GUI のドラッグの
    /// ように、1つのジェスチャで何度も適用し直す操作の逐次適用用）。
    /// ジェスチャの開始時に控えたスナップショットを、最初の成功時に
    /// [`History::record_undo_snapshot`] で積むこと。失敗時は状態が
    /// 変わらない（コマンド層の契約どおり）。
    pub fn apply_transient(&mut self, command: &Command) -> Result<Option<u64>, CommandError> {
        apply(&mut self.project, command)
    }

    /// 外部で控えたスナップショットを undo スタックへ積む
    /// （Issue #31: ジェスチャ単位の取り消し）。上限と redo の消去は
    /// 通常の適用と同じ規則に従う。ただしスナップショットが現在の
    /// 状態と同じなら何もしない（動かさないクリック等が「幽霊 undo」
    /// を積んだり redo を消したりしないため、#31 レビュー対応）。
    pub fn record_undo_snapshot(&mut self, snapshot: Project) {
        if snapshot.dump_json() == self.project.dump_json() {
            return;
        }
        self.push_snapshot(snapshot);
    }

    /// スナップショットを undo スタックへ積み、上限を超えたら古い
    /// ものから捨て、redo スタックを消す。
    fn push_snapshot(&mut self, snapshot: Project) {
        self.undo_stack.push(snapshot);
        if self.undo_stack.len() > HISTORY_LIMIT {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }

    /// 1つ前の状態に戻す。戻せたら true、履歴が空なら false
    /// （状態は変わらない）。
    pub fn undo(&mut self) -> bool {
        match self.undo_stack.pop() {
            Some(previous) => {
                let current = std::mem::replace(&mut self.project, previous);
                self.redo_stack.push(current);
                true
            }
            None => false,
        }
    }

    /// undo で取り消した状態をやり直す。やり直せたら true、
    /// redo スタックが空なら false（状態は変わらない）。
    pub fn redo(&mut self) -> bool {
        match self.redo_stack.pop() {
            Some(next) => {
                let current = std::mem::replace(&mut self.project, next);
                self.undo_stack.push(current);
                true
            }
            None => false,
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }
}
