#!/bin/sh
# SPDX-License-Identifier: GPL-2.0-only
# SPDX-FileCopyrightText: 2026 Mono Technologies Inc.
#
# make-config.sh URL TOKEN CA OUT writes the package's /etc/config/verso_diag.
# It refuses what the plugin would refuse (src/settings.rs), so a package never
# carries settings its push loop cannot start with. CA is the path the
# package installs a CA file at, or empty for the router's ca-bundle.
set -eu
url=$1 token=$2 ca=$3 out=$4

case "$url" in
https://?*) ;;
*) echo "make-config: COLLECTOR_URL must start with https:// (got '$url')" >&2; exit 1 ;;
esac
if printf '%s' "${url#https://}" | grep -q '[^][A-Za-z0-9._~:/?#@!&()*+,;=%-]'; then
	echo "make-config: COLLECTOR_URL has a character a config line cannot carry" >&2
	exit 1
fi
if [ -z "$token" ]; then
	echo "make-config: DIAG_TOKEN is empty" >&2
	exit 1
fi
if printf '%s' "$token" | grep -q '[^A-Za-z0-9._~+/=-]'; then
	echo "make-config: DIAG_TOKEN may hold only A-Z a-z 0-9 . _ ~ + / = -" >&2
	exit 1
fi

mkdir -p "$(dirname "$out")"
{
	echo "config collector 'collector'"
	echo "	option url '$url'"
	echo "	option token '$token'"
	echo "	option ca '$ca'"
	echo "	# Empty: the eth0 MAC. Set a MAC here to send as another device ID."
	echo "	option device_id ''"
} > "$out"
