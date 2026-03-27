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

# Create a merge commit as a child of workspace and main (simulates prior init)
merge_sha=$(git commit-tree "$(git rev-parse HEAD^{tree})" \
	-p "$workspace_sha" -p "$main_sha" \
	-m "Special workspace merge commit")

# Create a "working copy" commit on top of the merge (simulates jj wc)
wc_sha=$(git commit-tree "$(git rev-parse HEAD^{tree})" \
	-p "$merge_sha" \
	-m "working copy commit")

# Point HEAD at the wc commit so the merge is reachable via --all
git checkout --detach "$wc_sha"
