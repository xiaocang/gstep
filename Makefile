# gstep — build & install

BIN        := gstep
PREFIX     ?= $(HOME)/.local
BINDIR     := $(PREFIX)/bin
RELEASE    := target/release/$(BIN)
INSTALL    := install

# Skill install locations
CODEX_SKILL_DIR := $(HOME)/.codex/skills/gstep

.DEFAULT_GOAL := build

.PHONY: build
build: ## Build the release binary
	cargo build --release

.PHONY: install
install: build ## Build, then install the binary into $(BINDIR)
	$(INSTALL) -d "$(BINDIR)"
	$(INSTALL) -m 755 "$(RELEASE)" "$(BINDIR)/$(BIN)"
	@echo "installed $(BIN) -> $(BINDIR)/$(BIN)"

.PHONY: install-skills
install-skills: ## Sync the Codex skill into ~/.codex/skills/gstep
	$(INSTALL) -d "$(CODEX_SKILL_DIR)/agents"
	$(INSTALL) -m 644 codex-skills/gstep/SKILL.md "$(CODEX_SKILL_DIR)/SKILL.md"
	$(INSTALL) -m 644 codex-skills/gstep/agents/openai.yaml "$(CODEX_SKILL_DIR)/agents/openai.yaml"
	@echo "synced codex skill -> $(CODEX_SKILL_DIR)"

.PHONY: install-all
install-all: install install-skills ## Install the binary and the skills

.PHONY: uninstall
uninstall: ## Remove the installed binary
	rm -f "$(BINDIR)/$(BIN)"
	@echo "removed $(BINDIR)/$(BIN)"

.PHONY: clean
clean: ## Remove build artifacts
	cargo clean

.PHONY: help
help: ## List available targets
	@grep -E '^[a-zA-Z_-]+:.*?## ' $(MAKEFILE_LIST) \
		| awk 'BEGIN{FS=":.*?## "}{printf "  \033[36m%-14s\033[0m %s\n", $$1, $$2}'
