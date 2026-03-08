use std::rc::Rc;
use slint::{ModelRc, VecModel, SharedString};

use crate::git_utils;
use crate::stacks::StackInfo;
use crate::{DiffLine, Stack, StackCommit};

pub fn build_stacks_model(stacks: &[StackInfo]) -> ModelRc<Stack> {
    let slint_stacks: Vec<Stack> = stacks
        .iter()
        .map(|stack| {
            let commits: Vec<StackCommit> = stack
                .commits
                .iter()
                .map(|c| StackCommit {
                    change_id: SharedString::from(c.change_id.as_str()),
                    commit_id: SharedString::from(c.commit_id.as_str()),
                    description: SharedString::from(c.description.as_str()),
                    author: SharedString::from(c.author.as_str()),
                    timestamp: SharedString::from(c.timestamp.as_str()),
                })
                .collect();

            Stack {
                name: SharedString::from(stack.name.as_str()),
                commits: ModelRc::from(Rc::new(VecModel::from(commits))),
            }
        })
        .collect();

    ModelRc::from(Rc::new(VecModel::from(slint_stacks)))
}

pub fn build_diff_model(lines: &[git_utils::DiffLine]) -> ModelRc<DiffLine> {
    let slint_lines: Vec<DiffLine> = lines
        .iter()
        .map(|l| DiffLine {
            content: SharedString::from(l.content.as_str()),
            kind: l.kind,
            hunk_id: l.hunk_id,
        })
        .collect();
    ModelRc::from(Rc::new(VecModel::from(slint_lines)))
}
