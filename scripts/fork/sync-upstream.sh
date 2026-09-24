#!/usr/bin/env bash
# Merge upstream master into the current branch while keeping this fork's CI.
#
# Fork-owned paths always keep the fork's version: upstream workflows and
# composite actions (including ones upstream adds) are removed, and fork
# workflows, fork actions, fork scripts, FORK.md, and
# scripts/release-workflows.test.ts are restored from HEAD. Upstream's changes
# to the replaced paths are saved as a patch for review. Other conflicts are
# left for manual resolution.
#
# Env: UPSTREAM_REMOTE (upstream), UPSTREAM_URL (https://github.com/herdrdev/herdr.git),
#      UPSTREAM_BRANCH (master), SYNC_COMMIT_SUBJECT (merge: sync upstream <branch>)
set -euo pipefail

upstream_remote="${UPSTREAM_REMOTE:-upstream}"
upstream_url="${UPSTREAM_URL:-https://github.com/herdrdev/herdr.git}"
upstream_branch="${UPSTREAM_BRANCH:-master}"
subject="${SYNC_COMMIT_SUBJECT:-merge: sync upstream $upstream_branch}"

die() {
    echo "error: $*" >&2
    exit 1
}

cd "$(git rev-parse --show-toplevel)"

if git rev-parse -q --verify MERGE_HEAD >/dev/null; then
    die "a merge is already in progress; finish or abort it first"
fi
if ! git diff --quiet || ! git diff --cached --quiet; then
    die "commit or stash tracked changes before syncing"
fi

if ! git remote get-url "$upstream_remote" >/dev/null 2>&1; then
    echo "adding remote $upstream_remote -> $upstream_url"
    git remote add "$upstream_remote" "$upstream_url"
fi
upstream_ref="refs/remotes/$upstream_remote/$upstream_branch"
git fetch --no-tags "$upstream_remote" "+refs/heads/$upstream_branch:$upstream_ref"

if git merge-base --is-ancestor "$upstream_ref" HEAD; then
    echo "already up to date with $upstream_remote/$upstream_branch"
    exit 0
fi
previous_base="$(git merge-base HEAD "$upstream_ref")"

# Conflicts are expected here; they are handled below.
git merge --no-ff --no-commit "$upstream_ref" || true
git rev-parse -q --verify MERGE_HEAD >/dev/null || die "git merge did not start; see the output above"

git ls-files -- .github/workflows .github/actions .github/dependabot.yml | sort -u | while IFS= read -r path; do
    case "$path" in
        .github/workflows/fork-* | .github/actions/fork-*) ;;
        *) git rm -q -f --ignore-unmatch -- "$path" ;;
    esac
done

git ls-tree -r --name-only HEAD -- .github/workflows .github/actions scripts/fork FORK.md scripts/release-workflows.test.ts |
    while IFS= read -r path; do
        case "$path" in
            .github/workflows/fork-* | .github/actions/fork-* | scripts/fork/* | FORK.md | scripts/release-workflows.test.ts)
                git checkout HEAD -- "$path"
                ;;
        esac
    done

echo
# Nothing upstream changes in the paths this fork replaces is merged; keep it reviewable instead.
replaced_paths=(.github/workflows .github/actions .github/dependabot.yml scripts/release-workflows.test.ts)
review_dir="$(git rev-parse --git-path fork-sync)"
mkdir -p "$review_dir"
review_patch="$review_dir/upstream-ci-${previous_base:0:12}..$(git rev-parse --short=12 "$upstream_ref").patch"
git diff "$previous_base" "$upstream_ref" -- "${replaced_paths[@]}" > "$review_patch"
if [ -s "$review_patch" ]; then
    echo "upstream changed CI paths that this fork replaces; review them for fixes worth porting:"
    git --no-pager diff --stat "$previous_base" "$upstream_ref" -- "${replaced_paths[@]}"
    echo "full patch: $review_patch"
else
    rm -f "$review_patch"
    echo "upstream did not change the CI paths this fork replaces"
fi

# Report where upstream pins actions or tools differently from the fork's copies.
fork_ci="$(git ls-files -z -- '.github/workflows/fork-*' '.github/actions/fork-*' | xargs -0 cat)"
upstream_ci="$(git grep -h -e '' "$upstream_ref" -- .github/workflows 2>/dev/null || true)"
drift=0
report_drift() {
    echo "drift: $1 fork=[$2] upstream=[$3]"
    drift=1
}
for pin in $(printf '%s\n' "$fork_ci" | grep -o -E 'uses: [A-Za-z0-9_.-]+/[A-Za-z0-9_./-]+@[0-9a-f]{40}' | sed 's/^uses: //' | sort -u); do
    action="${pin%@*}"
    upstream_pins="$(printf '%s\n' "$upstream_ci" | grep -o -E "uses: $action@[0-9a-f]{40}" | sed 's/^.*@//' | sort -u | tr '\n' ' ' || true)"
    if [ -n "$upstream_pins" ] && ! grep -q -w -- "${pin#*@}" <<<"$upstream_pins"; then
        report_drift "$action" "${pin#*@}" "${upstream_pins% }"
    fi
done
tool_versions() {
    printf '%s\n' "$1" | grep -A4 -E "$2" | grep -o -E "$3: [0-9][0-9.]*" | sed "s/^$3: //" | sort -u | tr '\n' ' ' || true
}
for tool in "Zig|setup-zig@|version" "Bun|setup-bun@|bun-version"; do
    IFS='|' read -r label anchor key <<<"$tool"
    fork_version="$(tool_versions "$fork_ci" "$anchor" "$key")"
    upstream_version="$(tool_versions "$upstream_ci" "$anchor" "$key")"
    if [ -n "$upstream_version" ] && [ "$fork_version" != "$upstream_version" ]; then
        report_drift "$label" "${fork_version% }" "${upstream_version% }"
    fi
done
if [ "$drift" = 0 ]; then
    echo "no action pin or Zig/Bun version drift from upstream workflows"
fi

unresolved="$(git diff --name-only --diff-filter=U)"
if [ -n "$unresolved" ]; then
    echo
    echo "resolve these conflicts, stage them, then run: git commit --no-verify -m \"$subject\""
    printf '%s\n' "$unresolved" | sed 's/^/  /'
    exit 1
fi

# Merge commits are exempt from subject linting in fork CI; the pre-commit lint is redundant here.
git commit --no-verify -q -m "$subject"
echo
echo "merged $upstream_remote/$upstream_branch as $(git rev-parse --short HEAD); run just check, then push"
