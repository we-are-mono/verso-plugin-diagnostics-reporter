#!/bin/sh
# SPDX-License-Identifier: GPL-2.0-only
# SPDX-FileCopyrightText: 2026 Mono Technologies Inc.
#
# apk post-deinstall hook, once the files are gone: remove the account and the
# run directory, then tell the shell to read the manifests again so the page
# leaves the menu.
sed -i '/^verso-plugin-diag:/d' /etc/passwd
rm -rf /var/run/verso-plugin-diag
ubus call service signal '{"name":"verso","signal":1}' 2>/dev/null
exit 0
