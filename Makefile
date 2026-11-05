# SPDX-License-Identifier: GPL-2.0-only
# SPDX-FileCopyrightText: 2026 Mono Technologies Inc.

# verso-plugin-diag: build, test and package.
#
#   make test
#   make apk COLLECTOR_URL=https://diag.example.com/d DIAG_TOKEN=secret
#   make apk ... COLLECTOR_CA=path/to/ca.pem   # trust this CA, not ca-bundle
#
# The package carries the token, so treat every copy of it as holding the
# token. Only arm64: the sensors exist only on the Mono Gateway.

-include local.mk

CARGO       ?= $(if $(wildcard $(HOME)/.cargo/bin/cargo),$(HOME)/.cargo/bin/cargo,cargo)
TARGET      := aarch64-unknown-linux-musl
BIN         := target/$(TARGET)/release/verso-plugin-diag
OPENWRT_DIR ?= $(abspath ../openwrt)
APK         ?= $(OPENWRT_DIR)/staging_dir/host/bin/apk
KEY         ?= $(OPENWRT_DIR)/private-key.pem
VERSION     ?= 0.1.0
RELEASE     ?= 0
VER         := $(VERSION)-r$(RELEASE)

COLLECTOR_URL ?=
DIAG_TOKEN    ?=
COLLECTOR_CA  ?=
CA_PATH       := /etc/verso-plugin-diag/ca.pem

PAYLOAD := build/payload
OUT     := build/verso-plugin-diag-$(VER).apk

.PHONY: test build apk clean

test:
	$(CARGO) test --locked
	sh -n rootfs/etc/init.d/verso-plugin-diag packaging/*.sh
	sh packaging/make-config.test.sh

build:
	$(CARGO) build --locked --release --target $(TARGET)

apk: build
	@test -x "$(APK)" || { echo "apk tool not found at $(APK); set OPENWRT_DIR or APK"; exit 1; }
	@test -f "$(KEY)" || { echo "signing key not found at $(KEY); set OPENWRT_DIR or KEY"; exit 1; }
	@command -v fakeroot >/dev/null || { echo "fakeroot is needed to record root ownership"; exit 1; }
	@test -z "$(COLLECTOR_CA)" || test -f "$(COLLECTOR_CA)" || { echo "COLLECTOR_CA $(COLLECTOR_CA) is not a file"; exit 1; }
	rm -rf $(PAYLOAD)
	sh packaging/make-config.sh "$(COLLECTOR_URL)" "$(DIAG_TOKEN)" "$(if $(COLLECTOR_CA),$(CA_PATH))" $(PAYLOAD)/etc/config/verso_diag
	chmod 0600 $(PAYLOAD)/etc/config/verso_diag
	install -Dm755 $(BIN) $(PAYLOAD)/usr/bin/verso-plugin-diag
	install -Dm755 rootfs/etc/init.d/verso-plugin-diag $(PAYLOAD)/etc/init.d/verso-plugin-diag
	install -Dm644 manifest.json $(PAYLOAD)/usr/share/verso/plugins/diag/manifest.json
	install -Dm644 i18n/sl.json $(PAYLOAD)/usr/share/verso/plugins/diag/i18n/sl.json
	$(if $(COLLECTOR_CA),install -Dm644 $(COLLECTOR_CA) $(PAYLOAD)$(CA_PATH))
	# Directories as OpenWrt has them (0755), whatever this machine's umask:
	# apk may set an existing directory's mode to the package's.
	find $(PAYLOAD) -type d -exec chmod 0755 {} +
	fakeroot -- sh -c 'chown -R 0:0 "$(PAYLOAD)" && "$(APK)" mkpkg \
	  --info name:verso-plugin-diag --info version:$(VER) --info arch:aarch64_generic \
	  --info "description:Sends Mono Gateway sensor readings to mono-collector" \
	  --info license:GPL-2.0-only --info origin:verso-plugin-diag \
	  --info "depends:verso curl ca-bundle" \
	  --files "$(PAYLOAD)" \
	  --script post-install:packaging/post-install.sh \
	  --script post-upgrade:packaging/post-install.sh \
	  --script pre-deinstall:packaging/pre-deinstall.sh \
	  --script post-deinstall:packaging/post-deinstall.sh \
	  --sign-key "$(KEY)" \
	  --output "$(OUT)"'
	@echo "built and signed: $(OUT)"

clean:
	rm -rf build target
