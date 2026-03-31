#!/bin/bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

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

commit1_hash="$(git rev-parse HEAD)"

echo "main commit 2" >> main.txt
git add main.txt
git commit -m "main commit 2"

echo "main commit 3" >> main.txt
git add main.txt
git commit -m "main commit 3"

git checkout -b branch1 "$commit1_hash"

echo "branch1 commit 1" > branch1.txt
git add branch1.txt
git commit -m "branch1 commit 1"

echo "branch1 commit 2" >> branch1.txt
git add branch1.txt
git commit -m "branch1 commit 2"

echo "branch1 commit 3" >> branch1.txt
git add branch1.txt
git commit -m "branch1 commit 3"

cat <<'EOF' > main.cpp
#include <iostream>

int main()
{
	std::cout << "Hello, world!\n";
	return 0;
}
EOF
git add main.cpp
git commit -m "add main.cpp with hello world"

cat <<'EOF' > main.cpp
#include <format>
#include <iostream>

int main()
{
	std::cout << std::format("{}\n", "Hello, world!");
	return 0;
}
EOF
git add main.cpp
git commit -m "use std::format in main.cpp"

git checkout -b branch2 "$commit1_hash"

echo "branch2 commit 1" > branch2.txt
git add branch2.txt
git commit -m "branch2 commit 1"

echo "branch2 commit 2" >> branch2.txt
git add branch2.txt
git commit -m "branch2 commit 2"

git checkout main

# Initialize jj colocated repo
jj git init --colocate

# Create workspace branch off main and add guiguitsu config
jj new main
cat <<'CONF' > .guiguitsu.json
{
  "workspace_branch": "workspace",
  "workspace_remote": "origin",
  "trunk": "main",
  "stacks": []
}
CONF
jj desc -m "Add guiguitsu configuration"
jj bookmark create workspace -r @

# Create the workspace merge commit (workspace + main)
workspace_sha="$(git rev-parse workspace)"
main_sha="$(git rev-parse main)"
jj new -m "Special workspace merge commit" "$workspace_sha" "$main_sha"

# Update config with stacks (no extra stack branches beyond workspace+trunk)
cat <<'CONF' > .guiguitsu.json
{
  "workspace_branch": "workspace",
  "workspace_remote": "origin",
  "trunk": "main",
  "stacks": []
}
CONF
jj desc -m "Special workspace merge commit"

# Move working copy past the merge commit
jj new
