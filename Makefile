APP_NAME := SequoiaCapture
BIN_NAME := sequoia_capture
APP_DIR := dist/$(APP_NAME).app
APP_EXE := $(APP_DIR)/Contents/MacOS/$(APP_NAME)
INFO_PLIST := packaging/Info.plist
ENTITLEMENTS := packaging/entitlements.plist
SIGN_IDENTITY ?= -
CAPTURE_SECS ?= 10
OUT ?= artifacts/hello-world.wav
SAMPLE_RATE ?= 48000

.PHONY: help build build-release probe capture bundle sign verify run-app reset-perms clean

help:
	@echo "Targets:"
	@echo "  build         - Build debug binaries"
	@echo "  build-release - Build release capture binary"
	@echo "  probe         - Run API probe (debug)"
	@echo "  capture       - Run WAV recorder (debug)"
	@echo "  bundle        - Create minimal .app bundle"
	@echo "  sign          - Codesign app bundle"
	@echo "  verify        - Verify signature and print entitlements"
	@echo "  run-app       - Run signed app bundle (launch via open)"
	@echo "  reset-perms   - Reset TCC permissions for this bundle id"
	@echo "  clean         - Remove build artifacts"

build:
	cargo build --bins

build-release:
	cargo build --release --bin $(BIN_NAME)

probe: build
	DYLD_LIBRARY_PATH=/usr/lib/swift cargo run -- $(CAPTURE_SECS)

capture: build
	DYLD_LIBRARY_PATH=/usr/lib/swift cargo run --bin $(BIN_NAME) -- $(CAPTURE_SECS) $(OUT) $(SAMPLE_RATE)

bundle: build-release
	rm -rf $(APP_DIR)
	mkdir -p $(APP_DIR)/Contents/MacOS
	cp target/release/$(BIN_NAME) $(APP_EXE)
	chmod +x $(APP_EXE)
	cp $(INFO_PLIST) $(APP_DIR)/Contents/Info.plist
	install_name_tool -add_rpath /usr/lib/swift $(APP_EXE) || true

sign: bundle
	codesign --force --deep --options runtime --entitlements $(ENTITLEMENTS) --sign "$(SIGN_IDENTITY)" $(APP_DIR)

verify:
	codesign --verify --deep --strict --verbose=2 $(APP_DIR)
	codesign -d --entitlements :- --verbose=2 $(APP_DIR)

run-app:
	open -W $(APP_DIR) --args $(CAPTURE_SECS) $(OUT) $(SAMPLE_RATE)

reset-perms:
	tccutil reset ScreenCapture com.recordit.sequoiacapture || true
	tccutil reset Microphone com.recordit.sequoiacapture || true

clean:
	rm -rf dist artifacts
	cargo clean
