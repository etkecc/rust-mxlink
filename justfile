mise_data_dir := env("MISE_DATA_DIR", justfile_directory() / "var/mise")
mise_trusted_config_paths := justfile_directory() / "mise.toml"

# Show help by default
default:
	@just --list --justfile {{ justfile() }}

# Runs tests
test *extra_args:
	RUST_BACKTRACE=1 cargo test {{ extra_args }}

# Formats the code
fmt:
	RUST_BACKTRACE=1 cargo fmt --all

# Runs the clippy linter
clippy *extra_args:
	cargo clippy {{ extra_args }}

# Checks that the code compiles without building
check:
	cargo check

# Invokes mise with the project-local data directory
mise *args: _ensure_mise_data_directory
	#!/bin/sh
	export MISE_DATA_DIR="{{ mise_data_dir }}"
	export MISE_TRUSTED_CONFIG_PATHS="{{ mise_trusted_config_paths }}"
	mise {{ args }}

# Runs prek (pre-commit hooks manager) with the given arguments
prek *args: _ensure_mise_tools_installed
	@just --justfile {{ justfile() }} mise exec -- prek {{ args }}

# Runs pre-commit hooks on staged files
prek-run-on-staged *args: _ensure_mise_tools_installed
	@just --justfile {{ justfile() }} mise exec -- prek run {{ args }}

# Runs pre-commit hooks on all files
prek-run-on-all *args: _ensure_mise_tools_installed
	@just --justfile {{ justfile() }} mise exec -- prek run --all-files {{ args }}

# Installs the git pre-commit hook (runs prek automatically before each commit)
prek-install-git-pre-commit-hook: _ensure_mise_tools_installed
	@just --justfile {{ justfile() }} mise exec -- prek install

# Internal - ensures var/mise directory exists
_ensure_mise_data_directory:
	#!/bin/sh
	if [ ! -d "{{ mise_data_dir }}" ]; then
		mkdir -p "{{ mise_data_dir }}"
	fi

# Internal - ensures mise tools are installed
_ensure_mise_tools_installed: _ensure_mise_data_directory
	@just --justfile {{ justfile() }} mise install --quiet
