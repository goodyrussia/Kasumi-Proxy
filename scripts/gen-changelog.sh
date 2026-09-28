#!/usr/bin/env bash
# shellcheck shell=bash
# Generate the CHANGELOG.md entry for a release: the version heading plus the
# commits landed since the last tag (housekeeping commits filtered out).
#
# Idempotent: if the version already has an entry (hand-written ahead of a
# skip_bump release, or a re-run), it is left untouched.
#
# Usage: scripts/gen-changelog.sh <version>   # e.g. v0.5.0
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/CHANGELOG.md"

VERSION="${1:?version required}"

if [ -f "$OUT" ]; then
	ver_re=$(printf '%s' "$VERSION" | sed 's/[][\\.*^$/]/\\&/g')
	if grep -qE "^## ${ver_re}([[:space:]]|\$)" "$OUT"; then
		echo "→ $OUT already has a $VERSION entry — leaving it untouched"
		exit 0
	fi
fi

last_tag=$(git -C "$ROOT" describe --tags --abbrev=0 2>/dev/null || echo "")
tmp="$(mktemp)"

{
	echo "## $VERSION — $(date -u '+%Y-%m-%d')"
	echo ""
	if [ -n "$last_tag" ]; then
		repo_url=$(git -C "$ROOT" remote get-url origin 2>/dev/null | sed 's|git@github.com:|https://github.com/|;s|\.git$||' || echo "")
		commits=$(git -C "$ROOT" log "$last_tag"..HEAD --oneline \
			--no-merges \
			-- . ':(exclude)module/module.prop' ':(exclude)update.json' ':(exclude)CHANGELOG.md' \
			2>/dev/null | grep -vE '^[0-9a-f]+ (ci|chore|refactor)[:(]' || true)
		if [ -n "$commits" ]; then
			echo "### Changes"
			echo ""
			printf '%s\n' "$commits" | while IFS= read -r line; do
				sha="${line%% *}"
				msg="${line#* }"
				if [ -n "$repo_url" ]; then
					echo "- [\`${sha}\`](${repo_url}/commit/${sha}) ${msg}"
				else
					echo "- ${line}"
				fi
			done
			echo ""
		fi
	fi
} >"$tmp"

if [ -f "$OUT" ]; then
	cat "$tmp" "$OUT" >"$tmp.merged"
	mv "$tmp.merged" "$OUT"
else
	mv "$tmp" "$OUT"
fi
rm -f "$tmp"

echo "→ updated $OUT"
