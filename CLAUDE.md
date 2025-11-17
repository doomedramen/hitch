# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Development Commands

The project uses `just` as the primary command runner. Install with `cargo install just`.

### Building and Testing
- `just build` - Build project in release mode
- `just dev` - Build in debug mode for faster iteration
- `just test` - Run all tests
- `just test-core` - Run core tests only (excludes problematic integration tests)
- `just test-verbose` - Run tests with verbose output
- `just test-file <name>` - Run specific test file

### Code Quality
- `just format` - Format code with rustfmt
- `just lint` - Run clippy linter
- `cargo audit` - Run security audit
- `cargo check` - Quick compilation check

### Release and Distribution
- `just release` - Complete release process (auto-bumps version, tests, builds, tags, pushes)
- `just release-tag` - Creates and pushes release tag for current version
- `just install` - Install binary locally from source

### Utilities
- `just run -- <args>` - Run hitch binary with arguments in debug mode
- `just clean` - Clean build artifacts
- `just info` - Show project and system information
- `just setup` - Set up development environment

## Project Architecture

### Core Components

**Hitch** is a CLI tool for Git-based environment branch management that provides structured deployment workflows.

#### Main Modules
- `src/main.rs` - CLI entry point using clap for argument parsing
- `src/commands/` - Individual command implementations (init, add, promote, rebuild, status, lock, unlock, guard)
- `src/types.rs` - Core data structures (`Environment`, `HitchConfig`)
- `src/utils/` - Git operations, validation, and helper utilities

#### Key Data Structures
- `HitchConfig` - Main configuration stored in `hitch.json`, contains environment map
- `Environment` - Represents deployment environment with:
  - `base` - Source branch for rebuilding
  - `branches` - Promoted branches list
  - `locked`/`locked_by`/`locked_at` - Locking mechanism
  - `rebuilt_at` - Rebuild timestamp

### Command Structure
All commands follow the pattern in `src/commands/`:
- Each command has its own module with argument struct
- Commands use `GlobalContext` for verbose/no-push flags
- Git operations are abstracted through `utils::git_operations`
- Configuration is managed through JSON serialization with serde

### Git Integration
- Uses `git2` crate for Git operations
- Stores metadata in `.hitch/` directory
- Supports branch promotion/demotion workflows
- Provides environment locking mechanism
- Rebuild functionality merges promoted branches to base branch

## Development Workflow

### Pre-commit Hooks (Lefthook)
The project uses lefthook for pre-commit automation (`lefthook.yml`):
- Code formatting check (`cargo fmt --all -- --check`)
- Clippy linting (`cargo clippy --all-targets --all-features -- -D warnings`)
- Security audit (`cargo audit`)
- Cargo.lock consistency check

### Pre-push Checks
- Full build verification (`cargo build --release`)
- Complete test suite (`cargo test --all-features --release`)

### Testing Strategy
- Unit tests for core logic in `src/`
- Integration tests in `tests/` directory
- Tests cover git operations, CLI commands, and edge cases
- Use `tempfile` for isolated test environments

## Configuration Files

- `Cargo.toml` - Project dependencies and metadata
- `lefthook.yml` - Pre-commit/pre-commit hook configuration
- `justfile` - Development commands and build automation
- `hitch.json` - Runtime configuration (created by `hitch init`)

## Release Process

The project has automated releases triggered by git tags:
1. Use `just release` to bump version and create tag
2. GitHub Actions builds binaries for multiple platforms
3. Release automatically creates GitHub release with binaries
4. Supports Homebrew and Cargo distribution