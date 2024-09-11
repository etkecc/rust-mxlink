# Show help by default
default:
	@just --list --justfile {{ justfile() }}

# Runs tests
test *extra_args:
	RUST_BACKTRACE=1 cargo test {{ extra_args }}

# Runs the clippy linter
clippy *extra_args:
	cargo clippy {{ extra_args }}
