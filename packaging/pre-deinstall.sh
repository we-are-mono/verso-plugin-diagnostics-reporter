#!/bin/sh
# SPDX-License-Identifier: GPL-2.0-only
# SPDX-FileCopyrightText: 2026 Mono Technologies Inc.
#
# apk pre-deinstall hook, while the files are still on disk: stop sending.
/etc/init.d/verso-plugin-diag stop 2>/dev/null
/etc/init.d/verso-plugin-diag disable 2>/dev/null
exit 0
