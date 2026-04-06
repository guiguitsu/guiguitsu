#!/bin/bash

set -euo pipefail

repo_path="${1:-}"

if [ -z "$repo_path" ]; then
	echo "Usage: $0 <new-repo-path>"
	exit 1
fi

if [ -e "$repo_path" ]; then
	echo "Path already exists: $repo_path"
	exit 1
fi

mkdir -p "$repo_path"
cd "$repo_path"

git init
git remote add origin https://dummy.com/repo1.git
git symbolic-ref HEAD refs/heads/main
git config user.name "Test User"
git config user.email "test@example.com"

echo "main commit 1" > main.txt
git add main.txt
git commit -m "main commit 1"

echo "main commit 2" >> main.txt
git add main.txt
git commit -m "main commit 2"

# Create workspace branch with a commit
git checkout -b workspace
echo "workspace file" > workspace.txt
git add workspace.txt
git commit -m "workspace commit 1"

workspace_sha=$(git rev-parse HEAD)
main_sha=$(git rev-parse main)

# Create a merge commit with workspace and main as parents
merge_sha=$(git commit-tree "$(git rev-parse HEAD^{tree})" \
	-p "$workspace_sha" -p "$main_sha" \
	-m "Special workspace merge commit")

# Create a child commit on top of the merge
child_sha=$(git commit-tree "$(git rev-parse HEAD^{tree})" \
	-p "$merge_sha" \
	-m "unstacked child commit")

# Create a grandchild commit on top of the child
grandchild_sha=$(git commit-tree "$(git rev-parse HEAD^{tree})" \
	-p "$child_sha" \
	-m "unstacked grandchild commit")

# Point a branch at the grandchild so all commits are reachable
git checkout --detach "$grandchild_sha"

# Write guiguitsu config with merge_commit set
cat <<CONF > .guiguitsu.json
{
  "workspace_branch": "workspace",
  "workspace_remote": "origin",
  "trunk": "main",
  "merge_commit": "$merge_sha",
  "stacks": [
    { "name": "workspace", "local_branch": "workspace", "remote_branch": "workspace@origin" },
    { "name": "main", "remote_branch": "main@origin" }
  ]
}
CONF

# Export SHAs for tests to verify against
echo "$merge_sha" > .merge_sha
echo "$child_sha" > .child_sha
echo "$grandchild_sha" > .grandchild_sha
