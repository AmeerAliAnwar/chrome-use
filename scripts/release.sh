#!/bin/sh
# Release chrome-use: bump package.json (sync-version.js carries it into cli/Cargo.toml + Cargo.lock),
# run the fast CI preflight checks, commit "chore(release): prepare vX", push main + the vX tag, wait
# for release-binaries.yml to build and publish the GitHub Release, then sync the plugin marketplace
# so Claude Code plugin installs pick the new version up right away (no token: uses your `gh` login).
# Write the CHANGELOG.md entry (release markers) and both docs/*changelog.html entries first (AGENTS.md).
#   scripts/release.sh [--dry-run] 1.5.145
set -eu
DRY=
[ "${1:-}" = --dry-run ] && { DRY=1; shift; }
V=${1:?usage: scripts/release.sh [--dry-run] <version>}
REPO=leeguooooo/chrome-use
MARKETPLACE=leeguooooo/plugins
cd "$(dirname "$0")/.."
die() { echo "error: $*" >&2; exit 1; }
# Release-content checks warn in a dry run (the entry may not be written yet) and stop a real release.
need() { "$@" || { [ -n "$DRY" ] || exit 1; echo "warn: '$*' failed; a real release stops here" >&2; }; }

[ "$(git rev-parse --abbrev-ref HEAD)" = main ] || die "not on main"
[ -z "$(git status --porcelain)" ] || die "working tree not clean"
git fetch -q origin main
[ "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)" ] || die "main is not in sync with origin/main"
[ -z "$(git ls-remote --tags origin "refs/tags/v$V")" ] || die "v$V already exists"
[ -n "$DRY" ] && trap 'git checkout -q -- .' EXIT

sed -i.bak "s/^  \"version\": \".*\"/  \"version\": \"$V\"/" package.json && rm package.json.bak
node scripts/sync-version.js >/dev/null
node scripts/check-version-sync.js
need node scripts/release-notes.js "v$V" >/dev/null
for f in docs/changelog.html docs/en/changelog.html; do need grep -q "<strong>v$V</strong>" "$f"; done
node --test scripts/release-notes.test.js >/dev/null
node --test extensions/ab-connect/*.test.js >/dev/null
sh scripts/test-install.sh >/dev/null
cargo fmt --manifest-path cli/Cargo.toml -- --check
if [ -n "$DRY" ]; then git --no-pager diff; echo "dry run: checks done, version bump reverted"; exit 0; fi

if ! git diff --quiet; then git commit -qam "chore(release): prepare v$V"; fi
git tag "v$V"
git push -q origin main "v$V"

# The tag push starts release-binaries.yml, which builds seven binaries and creates the Release.
# Wait for it: the plugin must not update before its binaries exist.
RUN=
for _ in 1 2 3 4 5 6 7 8 9 10 11 12; do
  sleep 5
  RUN=$(gh run list -R "$REPO" -w release-binaries.yml -b "v$V" -e push -L 1 --json databaseId -q '.[0].databaseId')
  [ -n "$RUN" ] && break
done
[ -n "$RUN" ] || die "no release-binaries run for v$V; see https://github.com/$REPO/actions"
echo "waiting for release build $RUN"
gh run watch "$RUN" -R "$REPO" --exit-status >/dev/null || die "release build $RUN failed; marketplace not synced"
echo "released https://github.com/$REPO/releases/tag/v$V"

# A release prepared as "(Unreleased)" gets its publication date now, as the repo does by hand.
if grep -q "v$V</strong>（未发布）" docs/changelog.html || grep -q "v$V</strong> (Unreleased)" docs/en/changelog.html; then
  sed -i.bak "s|v$V</strong>（未发布）|v$V</strong>（$(date +%F)）|" docs/changelog.html && rm docs/changelog.html.bak
  sed -i.bak "s|v$V</strong> (Unreleased)|v$V</strong> ($(LC_ALL=C date '+%B %-d, %Y'))|" docs/en/changelog.html && rm docs/en/changelog.html.bak
  git commit -qam "docs: record v$V publication date" && git push -q origin main || echo "warn: could not push the publication date" >&2
fi

# The marketplace reads the version from package.json on main; run its sync now instead of waiting for the hourly cron.
PREV=$(gh run list -R "$MARKETPLACE" -w auto-sync-versions.yml -e workflow_dispatch -L 1 --json databaseId -q '.[0].databaseId')
gh workflow run auto-sync-versions.yml -R "$MARKETPLACE"
RUN=$PREV
for _ in 1 2 3 4 5 6 7 8 9 10 11 12; do
  sleep 5
  RUN=$(gh run list -R "$MARKETPLACE" -w auto-sync-versions.yml -e workflow_dispatch -L 1 --json databaseId -q '.[0].databaseId')
  [ "$RUN" != "$PREV" ] && break
done
gh run watch "$RUN" -R "$MARKETPLACE" --exit-status >/dev/null && echo "marketplace synced" || echo "warn: marketplace sync run $RUN failed; the hourly run will retry"
gh api "repos/$MARKETPLACE/contents/.claude-plugin/marketplace.json" -q .content | base64 -d \
  | python3 -c "import json,sys; print('marketplace chrome-use:', next(p['version'] for p in json.load(sys.stdin)['plugins'] if p['name']=='chrome-use'))"
