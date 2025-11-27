# Development Guide

Hitch is a CLI tool that brings environment branch management to Git. It helps teams organize and track deployment branches (like `dev`, `qa`, `main`) with proper promotion workflows, locking mechanisms, and rebuild automation.

This document covers the development workflow and release process for the Hitch CLI tool.

## 🛠️ Development Environment Setup

### Prerequisites

- Rust 1.83.0 or later
- Git
- Just command runner

### Quick Setup

```bash
# Clone the repository
git clone https://github.com/doomedramen/hitch.git
cd hitch

# Install development dependencies
just setup

# Install pre-commit hooks
lefthook install
```

### Manual Setup

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install just
cargo install just

# Install development tools
cargo install rustfmt
cargo install clippy
cargo install cargo-audit
cargo install cargo-tarpaulin  # for test coverage

# Install lefthook for pre-commit hooks
# macOS
brew install lefthook

# Linux
curl -L https://github.com/evilmartians/lefthook/releases/latest/download/lefthook_Linux_x86_64.tar.gz | tar xz
sudo mv lefthook /usr/local/bin/
```

## 🔧 Development Workflow

### Daily Development

```bash
# Check code formatting
just format

# Run linters
just lint

# Run tests
just test

# Watch for changes and rebuild
just watch

# Build in debug mode
just dev

# Run the CLI
just run -- --help
```

### Pre-commit & Pre-push Checks

The project uses Lefthook for Git hooks:

**Pre-commit:**
- **Code formatting** with `rustfmt --check`
- **Linting** with `clippy` (strict warnings)
- **Security audit** with `cargo audit`
- **Lock file consistency** with `cargo check --locked`

**Pre-push:**
- **Release build** verification
- **Test suite** execution with `cargo test --release`

## 🚀 Release Process

### Automated Releases

The main release command is all-in-one - it handles everything automatically:

```bash
# Complete release process (bump version → test → build → tag → push → trigger CI/CD)
just release
```

This command will:
1. Auto-increment the patch version (e.g., 1.0.0 → 1.0.1)
2. Run format, lint, and tests
3. Build the release binary
4. Update version in Cargo.toml
5. Commit the version bump
6. Create a git tag
7. **Automatically push the tag to trigger the release workflow**

### Alternative: Release Current Version

If you want to release the current version without bumping:

```bash
# Create and push release tag for current version
just release-tag
```

### Manual Release Process

1. **Update version** in `Cargo.toml`
2. **Commit changes**
3. **Create and push tag**:

```bash
git tag v1.0.1
git push origin v1.0.1
```

### What Happens During Release

When a version tag is pushed, the CI/CD pipeline:

1. **Builds binaries** for multiple platforms:
   - Linux (x86_64, ARM64)
   - macOS (x86_64 Intel, ARM64 Apple Silicon)

2. **Creates GitHub Release** with:
   - Compiled binaries
   - Auto-generated release notes
   - Checksums

3. **Updates Homebrew formula** in `doomedramen/homebrew-hitch`

## 📋 CI/CD Pipeline

The Hitch project uses a streamlined CI/CD pipeline with two main workflows:

### Continuous Integration (CI)

Every push and pull request triggers:

- **Multi-version Rust testing** (stable, beta, 1.83.0)
- **Code formatting** checks with `rustfmt`
- **Clippy linting** with strict warnings
- **Test suite** execution
- **Cross-platform build** verification on Linux and macOS
- **Documentation** generation and deployment

### Continuous Deployment (CD)

Release tags trigger:

- **Cross-platform binary builds** using cargo-dist
- **GitHub release** creation with artifacts
- **Homebrew formula** automatic updates

## 📊 Testing

### Running Tests

```bash
# Run all tests
just test

# Run tests with verbose output
just test-verbose

# Run specific test file
just test-file my_test_file

# Run tests with coverage
just test-coverage

# Generate detailed coverage reports
just test-coverage-detailed
```

### Test Coverage

The project includes comprehensive testing with local-first coverage checking:

- **Unit tests**: Core functionality and utilities
- **Integration tests**: CLI commands and workflows
- **Scenario tests**: Edge cases and error handling

### Coverage Commands

```bash
# Current coverage percentage (quick check)
just test-coverage-check

# Full coverage report with file breakdown
just test-coverage

# Quick coverage summary (last 5 lines)
just test-coverage-summary

# Full coverage analysis (detailed text report)
just test-coverage-full

# Detailed coverage for tooling (JSON/XML exports)
just test-coverage-detailed
```

### Current Coverage Status

**Overall Coverage: 73.42% (1,210/1,648 lines covered)**

🟢 **Excellent coverage on core functionality:**
- `add.rs`, `lock.rs`, `rebuild.rs`: 100% coverage
- `types.rs`, `main.rs`: 100% coverage
- Most command files: 80-90% coverage

🟡 **Areas for improvement:**
- `release.rs`: 61.6% (newer command, needs more test scenarios)
- `prelude.rs`: 59.0% (utility functions, some edge cases untested)

### Single-Threaded Execution

All coverage commands run with `--test-threads=1` to ensure:
- Consistent test execution order
- Avoid race conditions in Git operations
- Reliable coverage results
- Deterministic behavior across environments

### Coverage Configuration

Coverage settings are configured in `coverage.toml`:
- Minimum threshold: 80%
- Critical files: Higher coverage requirements
- Local-first approach: No external services required
- Manual execution: Use `just test-coverage-quick` when needed

## 🔍 Code Quality

### Linting

```bash
just lint                  # Run clippy with strict rules
cargo clippy --fix         # Auto-fix issues
```

### Formatting

```bash
just format                # Format all code
cargo fmt --all -- --check # Check formatting
```

### Security

```bash
cargo audit                # Security audit
cargo audit --fix          # Auto-fix where possible
```

## 📝 Development Commands

### Just Commands

```bash
just                    # Show available commands
just build              # Build in release mode
just dev                # Build in debug mode
just run -- [args]      # Run binary with args
just test               # Run tests (single-threaded)
just test-coverage      # Generate coverage reports (tarpaulin)
just test-coverage-check # Current coverage percentage
just test-coverage-full # Full coverage analysis (llvm-cov)
just test-coverage-summary # Quick coverage overview
just test-coverage-detailed # Coverage reports for tooling
just lint               # Run linters
just format             # Format code
just release            # Create new release
just setup              # Setup development environment
```

## 🏗️ Project Structure

```
hitch/
├── src/
│   ├── main.rs          # CLI entry point
│   ├── commands/        # CLI command implementations
│   ├── utils/           # Utility modules
│   └── types.rs         # Type definitions
├── tests/               # Integration and unit tests
├── .github/workflows/   # CI/CD configurations
├── lefthook.yml        # Git hooks configuration
├── justfile            # Development commands
├── coverage.toml       # Coverage configuration
├── dist-workspace.toml # Distribution config
└── Cargo.toml          # Project configuration
```

## 🤝 Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Run tests and checks
5. Commit your changes
6. Push to your fork
7. Create a pull request

### Commit Messages

Use conventional commit messages (`feat:`, `fix:`, `docs:`, etc.)

## 📦 Distribution

### Supported Platforms

- **Linux**: x86_64, ARM64
- **macOS**: x86_64 (Intel), ARM64 (Apple Silicon)

### Package Managers

```bash
# Homebrew
brew install doomedramen/homebrew-hitch/hitch

# Cargo
cargo install hitch
```

### Manual Download

Download from [GitHub Releases](https://github.com/doomedramen/hitch/releases)

## 🔧 Configuration

- **CI/CD**: `.github/workflows/`
- **Git Hooks**: `lefthook.yml`
- **Distribution**: `dist-workspace.toml`

## 🚨 Troubleshooting

### Common Issues

1. **Build fails**: Check Rust version (requires 1.83.0+)
2. **Tests fail**: Ensure Git is properly initialized
3. **Release fails**: Verify tag format and GitHub permissions

### Getting Help

- Check GitHub Issues
- Run `just info` for system information
- Run `just setup` to verify environment
- For coverage issues, check `coverage.toml` configuration
- Coverage reports generated in `coverage/` directory