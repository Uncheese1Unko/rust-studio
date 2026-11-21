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
                self.undo_stack.push(snapshot);
                if self.undo_stack.len() > HISTORY_LIMIT {
                    self.undo_stack.remove(0);
                }
                self.redo_stack.clear();
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
