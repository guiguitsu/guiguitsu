use serde::Deserialize;
use std::fs;

#[derive(Default, Deserialize, Clone)]
pub struct AuthorInfo {
    pub name: String,
    pub email: String,
    #[serde(default)]
    pub timestamp: String,
}

#[derive(Default, Deserialize, Clone)]
pub struct CommitInfo {
    #[serde(rename = "commit_id")]
    pub id: String,
    #[serde(default)]
    pub parents: Vec<String>,
    #[serde(default)]
    pub change_id: String,
    #[serde(rename = "description", default)]
    pub message: String,
    pub author: AuthorInfo,
    #[serde(default)]
    pub committer: AuthorInfo,
}

pub fn load_log(path: &str) -> anyhow::Result<Vec<CommitInfo>> {
    let content = fs::read_to_string(path)?;
    let mut commits = Vec::new();
    for line in content.lines() {
        if !line.trim().is_empty() {
            let commit: CommitInfo = serde_json::from_str(line)?;
            commits.push(commit);
        }
    }
    Ok(commits)
}

#[cfg(test)]
mod tests {
    #[test]
    fn parse_sample1() {
        let commits = super::load_log("tests/samples/fork_with_orphan.json").unwrap();
        assert_eq!(commits.len(), 4);
        assert_eq!(commits[0].parents[0], commits[2].parents[0]);
    }
}
