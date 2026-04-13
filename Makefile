.PHONY: all build release install uninstall test lint fmt fmt-fix check doc clean dev ci fixture completions docker help

# Variables
CARGO       := cargo
BINARY      := kdo
RELEASE_DIR := target/release
FIXTURE     := fixtures/sample-monorepo

# Install prefix: KDO_PREFIX env var, or ~/.local by default.
# Override: KDO_PREFIX=/usr/local make install  (requires sudo on macOS)
PREFIX  ?= $(HOME)/.local
BINDIR  := $(PREFIX)/bin

all: build ## Build in debug mode (default)

build: ## Build in debug mode
	$(CARGO) build --all

release: ## Build optimized release binary
	$(CARGO) build --release

install: release ## Install kdo to BINDIR (auto-detects ~/.local/bin or /usr/local/bin)
	@mkdir -p $(BINDIR)
	@install -m 755 $(RELEASE_DIR)/$(BINARY) $(BINDIR)/$(BINARY)
	@echo "Installed $(BINARY) to $(BINDIR)/$(BINARY)"
	@echo ""
	@echo "Make sure $(BINDIR) is on your PATH:"
	@echo "  export PATH=\"$(BINDIR):\$$PATH\""

uninstall: ## Remove kdo from BINDIR
	@rm -f $(BINDIR)/$(BINARY)
	@echo "Removed $(BINARY) from $(BINDIR)/$(BINARY)"

test: ## Run all tests
	$(CARGO) test --all

lint: ## Run clippy with warnings as errors
	$(CARGO) clippy --all-targets -- -D warnings

fmt: ## Check code formatting
	$(CARGO) fmt --all --check

fmt-fix: ## Auto-fix code formatting
	$(CARGO) fmt --all

check: ## Run cargo check
	$(CARGO) check --all-targets --all-features

doc: ## Build documentation
	$(CARGO) doc --no-deps --all-features

clean: ## Clean build artifacts (preserves fixture source files)
	$(CARGO) clean
	@# Only remove generated/cached files from the fixture — not committed source files.
	@rm -rf $(FIXTURE)/.kdo $(FIXTURE)/.kdoignore

dev: build ## Build debug binary and run kdo init on fixture
	@rm -rf $(FIXTURE)/.kdo $(FIXTURE)/.kdoignore
	cd $(FIXTURE) && $(CURDIR)/$(RELEASE_DIR)/$(BINARY) init

ci: fmt lint test doc ## Run full CI pipeline locally
	@echo ""
	@echo "All checks passed."

fixture: release ## Run full demo on sample-monorepo
	@# Reset only cache/generated files — keep committed kdo.toml
	@rm -rf $(FIXTURE)/.kdo $(FIXTURE)/.kdoignore
	@echo "=== init ==="
	cd $(FIXTURE) && $(CURDIR)/$(RELEASE_DIR)/$(BINARY) init
	@echo ""
	@echo "=== list ==="
	cd $(FIXTURE) && $(CURDIR)/$(RELEASE_DIR)/$(BINARY) list
	@echo ""
	@echo "=== graph ==="
	cd $(FIXTURE) && $(CURDIR)/$(RELEASE_DIR)/$(BINARY) graph
	@echo ""
	@echo "=== context vault-program ==="
	cd $(FIXTURE) && $(CURDIR)/$(RELEASE_DIR)/$(BINARY) context vault-program --budget 2048
	@echo ""
	@echo "=== run build ==="
	cd $(FIXTURE) && $(CURDIR)/$(RELEASE_DIR)/$(BINARY) run build
	@echo ""
	@echo "=== doctor ==="
	cd $(FIXTURE) && $(CURDIR)/$(RELEASE_DIR)/$(BINARY) doctor

completions: release ## Generate shell completions to ./completions/
	@mkdir -p completions
	$(RELEASE_DIR)/$(BINARY) completions bash > completions/kdo.bash
	$(RELEASE_DIR)/$(BINARY) completions zsh  > completions/_kdo
	$(RELEASE_DIR)/$(BINARY) completions fish > completions/kdo.fish
	@echo "Generated completions in ./completions/"

docker: ## Build Docker image
	docker build -t kdo .

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | \
		awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-14s\033[0m %s\n", $$1, $$2}'
