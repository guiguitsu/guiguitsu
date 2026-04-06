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

# Initialize jj colocated repo
jj git init --colocate

# Create workspace branch with a commit
jj new main
echo "workspace file" > workspace.txt
jj desc -m "workspace commit 1"
jj bookmark create workspace -r @

workspace_sha=$(git rev-parse workspace)
main_sha=$(git rev-parse main)

# Create the merge commit (workspace + main)
jj new -m "Special workspace merge commit" "$workspace_sha" "$main_sha"
merge_change_id=$(jj log -r @ --no-graph -T 'change_id')

# Create a child on top of the merge
jj new -m "unstacked child commit"
child_change_id=$(jj log -r @ --no-graph -T 'change_id')

# Create a grandchild on top of the child
jj new -m "unstacked grandchild commit"
grandchild_change_id=$(jj log -r @ --no-graph -T 'change_id')

# Write guiguitsu config with merge_commit set (using change-id).
# This must happen before resolving final SHAs because writing a file
# triggers a jj snapshot that rewrites the working-copy commit.
cat <<CONF > .guiguitsu.json
{
  "workspace_branch": "workspace",
  "workspace_remote": "origin",
  "trunk": "main",
  "merge_commit": "$merge_change_id",
  "stacks": [
    { "name": "workspace", "local_branch": "workspace", "remote_branch": "workspace@origin" },
    { "name": "main", "remote_branch": "main@origin" }
  ]
}
CONF

# Force a snapshot so jj incorporates the config file change,
# then resolve the final commit SHAs via their stable change-ids.
merge_sha=$(jj log -r "$merge_change_id" --no-graph -T 'commit_id')
child_sha=$(jj log -r "$child_change_id" --no-graph -T 'commit_id')
grandchild_sha=$(jj log -r "$grandchild_change_id" --no-graph -T 'commit_id')

echo "$merge_sha" > .merge_sha
echo "$merge_change_id" > .merge_change_id
echo "$child_sha" > .child_sha
echo "$grandchild_sha" > .grandchild_sha
