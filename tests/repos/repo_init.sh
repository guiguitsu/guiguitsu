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

# Create an existing workspace branch with a commit
git checkout -b workspace
echo "workspace file" > workspace.txt
git add workspace.txt
git commit -m "workspace commit 1"

# Go back to main so tests start from main
git checkout main
