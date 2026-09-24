#!/usr/bin/env bash
# Tag a pushed commit as a fork build. The tag push runs .github/workflows/fork-release.yml.
# Usage: scripts/fork/publish.sh [ref]   (default: HEAD; FORK_REMOTE defaults to origin)
set -euo pipefail

ref="${1:-HEAD}"
remote="${FORK_REMOTE:-origin}"

cd "$(git rev-parse --show-toplevel)"
git fetch --quiet "$remote"

commit="$(git rev-parse --verify "$ref^{commit}")"
if ! git branch --remotes --contains "$commit" | sed 's/^ *//' | grep -q "^$remote/"; then
    echo "error: $commit is not on any $remote branch; push it before publishing" >&2
    exit 1
fi

# Must match the tag computed by the plan job in fork-release.yml.
tag="fork-$(git show -s --format=%cs "$commit")-${commit:0:12}"
if existing="$(git rev-parse -q --verify "refs/tags/$tag^{commit}")"; then
    if [ "$existing" != "$commit" ]; then
        echo "error: local tag $tag points at $existing, not $commit" >&2
        exit 1
    fi
else
    git tag -a "$tag" "$commit" -m "$tag"
fi
git push "$remote" "refs/tags/$tag"

repo="$(git remote get-url "$remote" | sed -E 's#\.git$##; s#^.*[:/]([^/]+/[^/]+)$#\1#')"
echo "pushed $tag"
echo "release run: https://github.com/$repo/actions/workflows/fork-release.yml"
