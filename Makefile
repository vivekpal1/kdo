.PHONY: all build release install uninstall test lint fmt check doc clean dev ci help

# Variables
CARGO       := cargo
BINARY      := kdo
PREFIX      := /usr/local
BINDIR      := $(PREFIX)/bin
RELEASE_DIR := target/release
FIXTURE     := fixtures/sample-monorepo

all: build ## Build in debug mode (default)

build: ## Build in debug mode
	$(CARGO) build --all

release: ## Build optimized release binary
	$(CARGO) build --release

install: release ## Install kdo to $(PREFIX)/bin
	install -d $(BINDIR)
	install -m 755 $(RELEASE_DIR)/$(BINARY) $(BINDIR)/$(BINARY)
	@echo "Installed $(BINARY) to $(BINDIR)/$(BINARY)"

uninstall: ## Remove kdo from $(PREFIX)/bin
	rm -f $(BINDIR)/$(BINARY)
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

clean: ## Clean build artifacts
	$(CARGO) clean
	rm -rf $(FIXTURE)/.kdo $(FIXTURE)/kdo.toml $(FIXTURE)/.kdoignore $(FIXTURE)/.gitignore

dev: build ## Build and run kdo init on fixture
	cd $(FIXTURE) && ../../$(RELEASE_DIR)/$(BINARY) init

ci: fmt lint test doc ## Run full CI pipeline locally
	@echo ""
	@echo "All checks passed."

fixture: release ## Run full demo on sample-monorepo
	@rm -rf $(FIXTURE)/.kdo $(FIXTURE)/kdo.toml $(FIXTURE)/.kdoignore $(FIXTURE)/.gitignore
	@echo "=== init ==="
	cd $(FIXTURE) && ../../$(RELEASE_DIR)/$(BINARY) init
	@echo ""
	@echo "=== list ==="
	cd $(FIXTURE) && ../../$(RELEASE_DIR)/$(BINARY) list
	@echo ""
	@echo "=== graph ==="
	cd $(FIXTURE) && ../../$(RELEASE_DIR)/$(BINARY) graph
	@echo ""
	@echo "=== context vault-program ==="
	cd $(FIXTURE) && ../../$(RELEASE_DIR)/$(BINARY) context vault-program --budget 2048
	@echo ""
	@echo "=== run build ==="
	cd $(FIXTURE) && ../../$(RELEASE_DIR)/$(BINARY) run build
	@echo ""
	@echo "=== doctor ==="
	cd $(FIXTURE) && ../../$(RELEASE_DIR)/$(BINARY) doctor

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
