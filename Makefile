# gstep — build & install

BIN        := gstep
PREFIX     ?= $(HOME)/.local
BINDIR     := $(PREFIX)/bin
RELEASE    := target/release/$(BIN)
INSTALL    := install

# Skill install locations
CLAUDE_SKILL_DIR := $(HOME)/.claude/skills/gstep
CODEX_SKILL_DIR  := $(HOME)/.codex/skills/gstep

.DEFAULT_GOAL := build

.PHONY: build
build: ## Build the release binary
	cargo build --release

.PHONY: install
install: build ## Build, then install the binary into $(BINDIR)
	$(INSTALL) -d "$(BINDIR)"
	$(INSTALL) -m 755 "$(RELEASE)" "$(BINDIR)/$(BIN)"
	@echo "installed $(BIN) -> $(BINDIR)/$(BIN)"

.PHONY: install-claude-skill
install-claude-skill: ## Sync the Claude Code skill into ~/.claude/skills/gstep
	$(INSTALL) -d "$(CLAUDE_SKILL_DIR)"
	$(INSTALL) -m 644 .claude/skills/gstep/SKILL.md "$(CLAUDE_SKILL_DIR)/SKILL.md"
	@echo "synced claude skill -> $(CLAUDE_SKILL_DIR)"

.PHONY: install-codex-skill
install-codex-skill: ## Sync the Codex skill into ~/.codex/skills/gstep
	$(INSTALL) -d "$(CODEX_SKILL_DIR)/agents"
	$(INSTALL) -m 644 codex-skills/gstep/SKILL.md "$(CODEX_SKILL_DIR)/SKILL.md"
	$(INSTALL) -m 644 codex-skills/gstep/agents/openai.yaml "$(CODEX_SKILL_DIR)/agents/openai.yaml"
	@echo "synced codex skill -> $(CODEX_SKILL_DIR)"

.PHONY: install-skills
install-skills: install-claude-skill install-codex-skill ## Sync skills to Claude Code + Codex

.PHONY: install-all
install-all: install install-skills ## Install the binary and the skills

# Both Claude Code and Codex run the installed binary as their gstep MCP server
# (~/.claude.json and ~/.codex/config.toml point at $(BINDIR)/$(BIN)), so a fresh
# install is all it takes to ship binary changes; skills are synced alongside.
.PHONY: deploy
deploy: install-all ## Build, install the binary, and sync skills to Claude Code + Codex
	@echo "deployed gstep -> Claude Code + Codex"
	@echo "restart your Claude Code / Codex sessions to load the new binary"

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
