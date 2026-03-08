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

        let mut sha = head_sha;
        let parents = loop {
            let parents = parent_shas(&self.repo_path, &sha)?;
            if parents.len() >= 2 {
                break parents;
            }
            match parents.into_iter().next() {
                Some(p) => sha = p,
                None => return Ok(vec![]),
            }
        };

        let main_parent = parents.last().unwrap().clone();
        let mut stacks = Vec::new();

        for branch_parent in &parents[..parents.len() - 1] {
            let name = branch_name_for(&self.repo_path, branch_parent)
                .unwrap_or_else(|_| branch_parent[..branch_parent.len().min(8)].to_string());
            let commits = commits_in_range(&self.repo_path, &main_parent, branch_parent)?;
            stacks.push(StackInfo { name, commits });
        }

        Ok(stacks)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    use anyhow::{Context, Result, anyhow};

    use super::{GitStackProvider, StackProvider};

    struct TempRepo {
        path: PathBuf,
    }

    impl TempRepo {
        fn create() -> Result<Self> {
            let mut path = std::env::temp_dir();
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .context("time went backwards")?
                .as_nanos();
            path.push(format!("guiguitsu-stacks-test-{now}-{}", std::process::id()));

            let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/repos/repo1.sh");
            let output = Command::new("bash")
                .arg(script)
                .arg(&path)
                .output()
                .context("failed to execute repo1.sh")?;

            if !output.status.success() {
                return Err(anyhow!(
                    "repo1.sh failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ));
            }

            Ok(Self { path })
        }
    }

    impl Drop for TempRepo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn get_stacks_returns_two_stacks_from_repo1() -> Result<()> {
        let repo = TempRepo::create()?;
        let provider = GitStackProvider::new(repo.path.clone());

        let stacks = provider.get_stacks()?;

        assert_eq!(stacks.len(), 1);
        Ok(())
    }

    #[test]
    fn get_stacks_returns_correct_stack_names_from_repo1() -> Result<()> {
        let repo = TempRepo::create()?;
        let provider = GitStackProvider::new(repo.path.clone());

        let stacks = provider.get_stacks()?;

        let names: Vec<&str> = stacks.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["workspace"]);
        Ok(())
    }
}
