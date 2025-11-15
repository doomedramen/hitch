# Development Guide

Hitch is a CLI tool that brings environment branch management to Git. It helps teams organize and track deployment branches (like `dev`, `qa`, `main`) with proper promotion workflows, locking mechanisms, and rebuild automation.

This document covers the development workflow, CI/CD setup, and release process for the Hitch CLI tool.

## 🛠️ Development Environment Setup

### Prerequisites

- Rust 1.70.0 or later
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

### Pre-commit Checks

The project uses Lefthook for pre-commit hooks:

- **Code formatting** with `rustfmt`
- **Linting** with `clippy`
- **Security audit** with `cargo audit`
- **Lock file consistency** check

### Pre-push Checks

Before pushing changes, the following checks run:

- **Full test suite** execution
- **Release build** verification

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
   - Linux (x86_64)
   - macOS (x86_64, ARM64)

2. **Creates GitHub Release** with:
   - Compiled binaries
   - Auto-generated release notes
   - Checksums

3. **Updates Homebrew formula** in `doomedramen/homebrew-hitch`

4. **Publishes to Cargo crates.io** (if configured)

## 📋 CI/CD Pipeline

### Continuous Integration (CI)

Every push and pull request triggers:

- **Code formatting** checks
- **Clippy linting**
- **Security audit**
- **Test suite** execution
- **Build verification** on multiple platforms

### Continuous Deployment (CD)

Release tags trigger:

- **Cross-platform builds**
- **GitHub release creation**
- **Homebrew formula update**
- **Binary distribution**

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

The project aims for high test coverage. Current status:

- **Unit tests**: Core functionality
- **Integration tests**: CLI commands
- **Edge case tests**: Error handling

### Coverage Reports

Coverage reports are generated in the `coverage/` directory:

- **HTML report**: Open `coverage/tarpaulin-report.html`
- **JSON report**: For CI integration
- **XML report**: For tooling integration

## 🔍 Code Quality

### Linting

```bash
# Run clippy with strict rules
just lint

# Run clippy with auto-fix
cargo clippy --fix --allow-dirty --allow-staged
```

### Formatting

```bash
# Format all code
just format

# Check formatting without modifying files
cargo fmt --all -- --check
```

### Security

```bash
# Run security audit
cargo audit

# Fix security issues automatically
cargo audit --fix
```

## 📝 Development Commands

### Just Commands

```bash
just                    # Show available commands
just build             # Build in release mode
just dev               # Build in debug mode
just run               # Run the binary
just test              # Run tests
just lint              # Run linters
just format            # Format code
just clean             # Clean artifacts
just release           # Create new release (bump version, build, tag)
just release-tag       # Create and push release tag for current version
just setup             # Setup development environment
just info              # Show project information
```

### Cargo Commands

```bash
# Build
cargo build              # Debug build
cargo build --release    # Release build

# Test
cargo test               # Run tests
cargo test --release     # Run tests on release build

# Check
cargo check              # Quick compile check
cargo clippy             # Run linter
cargo fmt                # Format code

# Other
cargo clean              # Clean artifacts
cargo update             # Update dependencies
cargo audit              # Security audit
```

## 🏗️ Project Structure

```
hitch/
├── src/
│   ├── main.rs          # CLI entry point
│   ├── commands/        # CLI command implementations
│   ├── utils/           # Utility modules
│   └── types.rs         # Type definitions
├── tests/               # Integration tests
├── .github/workflows/   # CI/CD configurations
├── lefthook.yml        # Pre-commit hook configuration
├── justfile            # Just commands
├── Cargo.toml          # Project configuration
└── README.md           # Project documentation
```

## 🤝 Contributing

1. **Fork** the repository
2. **Create** a feature branch
3. **Make** your changes
4. **Run** tests and checks
5. **Commit** your changes
6. **Push** to your fork
7. **Create** a pull request

### Commit Messages

Use conventional commit messages:

- `feat:` for new features
- `fix:` for bug fixes
- `docs:` for documentation
- `style:` for formatting
- `refactor:` for refactoring
- `test:` for test changes
- `chore:` for maintenance

## 📦 Distribution

### Binary Downloads

Binaries are available for:

- **Linux**: x86_64
- **macOS**: x86_64, ARM64

### Package Managers

#### Homebrew

```bash
brew install doomedramen/homebrew-hitch/hitch
```

#### Cargo

```bash
cargo install hitch
```

#### Manual Download

Download from [GitHub Releases](https://github.com/doomedramen/hitch/releases).

## 🔧 Configuration

### GitHub Actions

- **CI**: `.github/workflows/ci.yml`
- **Release**: `.github/workflows/release-dist.yml`
- **Setup**: `.github/workflows/setup.yml`

### Lefthook

Configuration in `lefthook.yml`:

- **Pre-commit**: Format, lint, audit
- **Pre-push**: Test, build

### Cargo-dist

Configuration in `Cargo.toml` and `dist-workspace.toml`:

- **Target platforms**
- **Archive formats**
- **Homebrew integration**

## 🚨 Troubleshooting

### Common Issues

1. **Lefthook not found**: Install lefthook following the setup instructions
2. **Tests fail with git errors**: Make sure git is initialized in test directories
3. **Build fails**: Check Rust version and run `cargo update`
4. **Release fails**: Verify tag format (vX.Y.Z) and GitHub permissions

### Getting Help

- Check GitHub Issues
- Review CI/CD logs
- Run `just info` for system information
- Run `just setup` to verify environment