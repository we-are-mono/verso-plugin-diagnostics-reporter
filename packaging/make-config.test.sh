#!/bin/sh
# SPDX-License-Identifier: GPL-2.0-only
# SPDX-FileCopyrightText: 2026 Mono Technologies Inc.
#
# Tests for make-config.sh. Run from the repository root.
set -u
dir=$(mktemp -d)
trap 'rm -rf "$dir"' EXIT
fails=0

refuses() {
	if sh packaging/make-config.sh "$1" "$2" "" "$dir/out" 2>/dev/null; then
		echo "FAIL: accepted url='$1' token='$2'"
		fails=$((fails + 1))
	fi
}

refuses "http://x/d" "abc"
refuses "https://" "abc"
refuses "https://x/d" ""
refuses "https://x/d" 'a"b'
refuses "https://x/d" "a b"
refuses "https://x/'d" "abc"
refuses 'https://x/$d' "abc"

if sh packaging/make-config.sh "https://diag.example.com/d" "Super.Secret-Key" "" "$dir/out"; then
	grep -qx "	option url 'https://diag.example.com/d'" "$dir/out" || { echo "FAIL: url line"; fails=$((fails + 1)); }
	grep -qx "	option token 'Super.Secret-Key'" "$dir/out" || { echo "FAIL: token line"; fails=$((fails + 1)); }
	grep -qx "	option ca ''" "$dir/out" || { echo "FAIL: ca line"; fails=$((fails + 1)); }
else
	echo "FAIL: refused good settings"
	fails=$((fails + 1))
fi

[ "$fails" -eq 0 ] && echo "make-config: ok"
exit "$fails"
