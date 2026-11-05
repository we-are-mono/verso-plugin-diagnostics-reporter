#!/bin/sh
# SPDX-License-Identifier: GPL-2.0-only
# SPDX-FileCopyrightText: 2026 Mono Technologies Inc.
#
# apk post-install / post-upgrade hook: start the service, then tell the shell
# to read the manifests again so System › Mono feedback appears at once.
/etc/init.d/verso-plugin-diag enable 2>/dev/null
/etc/init.d/verso-plugin-diag restart 2>/dev/null
ubus call service signal '{"name":"verso","signal":1}' 2>/dev/null
exit 0
