# Dial Desktop. `make` lists the targets.
#
#   make run DIR=~/code/app        live engine on a project (hot reload)
#   make preview                   design preview, no engine needed
#   make app && make open DIR=…    debug build, then launch it
#
# DIR    project folder to open (default: the welcome screen)
# DIAL   engine binary (default: `dial` on PATH)
# ARGS   extra `dial rpc` flags, e.g. ARGS="--model local"

DIR  ?=
DIAL ?=
ARGS ?=

APP_ARGS = $(if $(DIAL),--dial $(DIAL)) $(DIR) $(if $(ARGS),-- $(ARGS))
DEBUG_BIN = target/debug/dial-desktop

.DEFAULT_GOAL := help
.PHONY: help install run preview preview-empty preview-question app open release test check clean

help: ## Show the targets
	@grep -E '^[a-z-]+:.*## ' $(MAKEFILE_LIST) | awk -F':.*## ' '{printf "  make %-17s %s\n", $$1, $$2}'

node_modules: package.json
	npm install
	@touch node_modules

install: node_modules ## Install the JavaScript dependencies

run: node_modules ## Run with hot reload: make run DIR=~/code/app
	npm run dev -- -- -- $(APP_ARGS)

preview: node_modules ## Design preview of a busy session (no engine)
	npm run dev -- -- -- --preview $(or $(DIR),$(CURDIR))

preview-empty: node_modules ## Design preview of an empty session
	npm run dev -- -- -- --preview=empty $(or $(DIR),$(CURDIR))

preview-question: node_modules ## Design preview waiting on a question
	npm run dev -- -- -- --preview=question $(or $(DIR),$(CURDIR))

app: node_modules ## Build the debug app (no bundle)
	npx tauri build --debug --no-bundle

open: ## Launch the built debug app: make open DIR=~/code/app
	@test -x $(DEBUG_BIN) || { echo "No build yet; run 'make app' first"; exit 1; }
	./$(DEBUG_BIN) $(APP_ARGS)

release: node_modules ## Release build and installable bundle
	npm run build

test: ## Rust tests and the TypeScript type check
	cargo test --manifest-path src-tauri/Cargo.toml
	npm run typecheck

check: test ## Tests plus clippy and formatting
	cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
	cargo fmt --manifest-path src-tauri/Cargo.toml --check

clean: ## Remove build output
	rm -rf dist target src-tauri/target
