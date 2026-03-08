use std::path::PathBuf;

use anyhow::Result;

use crate::git_utils::{CommitInfo, branch_name_for, commits_in_range, current_head_sha, parent_shas};

pub struct StackInfo {
    pub name: String,
    pub commits: Vec<CommitInfo>,
}

pub trait StackProvider {
    fn get_stacks(&self) -> Result<Vec<StackInfo>>;
}

pub struct GitStackProvider {
    repo_path: PathBuf,
}

impl GitStackProvider {
    pub fn new(repo_path: PathBuf) -> Self {
        Self { repo_path }
    }
}

impl StackProvider for GitStackProvider {
    fn get_stacks(&self) -> Result<Vec<StackInfo>> {
        let head_sha = current_head_sha(&self.repo_path)?;
        let parents = parent_shas(&self.repo_path, &head_sha)?;
        if parents.len() < 2 {
            return Ok(vec![]);
        }

        let main_parent = &parents[0];
        let mut stacks = Vec::new();

        for branch_parent in &parents[1..] {
            let name = branch_name_for(&self.repo_path, branch_parent)
                .unwrap_or_else(|_| branch_parent[..branch_parent.len().min(8)].to_string());
            let commits = commits_in_range(&self.repo_path, main_parent, branch_parent)?;
            stacks.push(StackInfo { name, commits });
        }

        Ok(stacks)
    }
}
