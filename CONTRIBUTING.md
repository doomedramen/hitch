# Contributing to Hitch

Thank you for your interest in contributing to Hitch! This document provides guidelines and instructions for contributing.

## Table of Contents

- [Code of Conduct](#code-of-conduct)
- [Getting Started](#getting-started)
- [Development Setup](#development-setup)
- [Making Changes](#making-changes)
- [Testing](#testing)
- [Code Quality](#code-quality)
- [Submitting Changes](#submitting-changes)
- [Project Structure](#project-structure)
- [Release Process](#release-process)

## Code of Conduct

This project adheres to a code of conduct that we expect all contributors to follow. Please be respectful, inclusive, and constructive in all interactions.

## Getting Started

### Prerequisites

- **Go 1.22+** - [Install Go](https://golang.org/doc/install)
- **Git** - Obviously :)
- **Just** (optional) - [Install just](https://github.com/casey/just#installation)
- **Docker** (optional) - For isolated testing
- **golangci-lint** (optional) - For linting locally

### Finding Issues to Work On

- Check the [Issues](https://github.com/DoomedRamen/hitch/issues) page
- Look for issues labeled `good first issue` or `help wanted`
- Ask questions in issue comments if you need clarification

## Development Setup

### 1. Fork and Clone

```bash
# Fork the repository on GitHub, then:
git clone https://github.com/YOUR_USERNAME/hitch.git
cd hitch

# Add upstream remote
git remote add upstream https://github.com/DoomedRamen/hitch.git
```

### 2. Install Dependencies

```bash
# Download Go dependencies
go mod download

# Install golangci-lint (optional, for local linting)
# macOS
brew install golangci-lint

# Linux
curl -sSfL https://raw.githubusercontent.com/golangci/golangci-lint/master/install.sh | sh -s -- -b $(go env GOPATH)/bin

# Or use Docker (no local installation needed)
docker compose build
```

### 3. Build Hitch

```bash
# Using just
just build

# Or directly
go build -o hitch ./cmd/hitch

# Verify it works
./hitch --version
```

## Making Changes

### Branching Strategy

1. Create a feature branch from `main`:

```bash
git checkout main
git pull upstream main
git checkout -b feature/your-feature-name
```

2. Use descriptive branch names:
   - `feature/add-new-command`
   - `fix/resolve-metadata-bug`
   - `docs/update-readme`

### Coding Standards

#### Go Style

- Follow [Effective Go](https://golang.org/doc/effective_go)
- Use `gofmt` for formatting (run `just fmt`)
- Use meaningful variable and function names
- Write comments for exported functions
- Keep functions focused and small

#### Example

```go
// Good: Clear, documented, focused
// CreateBranch creates a new Git branch from the specified base.
// Returns an error if the branch already exists or base branch is invalid.
func (r *Repo) CreateBranch(name string, base string) error {
    if name == "" {
        return fmt.Errorf("branch name cannot be empty")
    }
    // ... implementation
}

// Bad: Unclear, no docs, too complex
func cb(n, b string) error {
    // Does many things...
}
```

#### Error Handling

```go
// Good: Descriptive errors with context
if err := doSomething(); err != nil {
    return fmt.Errorf("failed to do something: %w", err)
}

// Bad: Silent failures or generic errors
if err := doSomething(); err != nil {
    return err  // What failed? Why?
}
```

#### Git Commit Messages

Follow [Conventional Commits](https://www.conventionalcommits.org/):

```
feat: add support for custom lock timeouts
fix: resolve race condition in metadata writer
docs: update COMMANDS.md with examples
test: add tests for branch cleanup logic
chore: update dependencies
```

## Testing

**All code changes must include tests!**

### Running Tests

```bash
# Quick tests (uses temp directories, safe)
just test

# Docker isolated tests (recommended before PR)
just test-docker

# Tests with coverage
just test-coverage

# All quality checks
just test-all
```

### Writing Tests

1. **Create test files** alongside source code:
   ```
   internal/git/
     repo.go
     repo_test.go  ← Your tests here
   ```

2. **Use testutil helpers** for Git operations:
   ```go
   func TestSomething(t *testing.T) {
       repo := testutil.NewTestRepo(t)
       // repo is automatically cleaned up

       // Create branches with commits
       repo.CreateBranch("feature/test", true)

       // Make assertions
       if !repo.BranchExists("feature/test") {
           t.Error("Branch should exist")
       }
   }
   ```

3. **Test one thing per test**:
   ```go
   // Good
   func TestCreateBranch(t *testing.T) { /* ... */ }
   func TestCreateBranchFailsWithInvalidName(t *testing.T) { /* ... */ }

   // Bad
   func TestBranches(t *testing.T) {
       // Tests 10 different things...
   }
   ```

4. **Use table-driven tests** for multiple cases:
   ```go
   func TestValidateName(t *testing.T) {
       tests := []struct {
           name    string
           input   string
           wantErr bool
       }{
           {"valid name", "feature/test", false},
           {"empty name", "", true},
           {"invalid chars", "feat/test$", true},
       }

       for _, tt := range tests {
           t.Run(tt.name, func(t *testing.T) {
               err := ValidateName(tt.input)
               if (err != nil) != tt.wantErr {
                   t.Errorf("ValidateName() error = %v, wantErr %v", err, tt.wantErr)
               }
           })
       }
   }
   ```

See [TESTING.md](./TESTING.md) for more details.

## Code Quality

### Linting

We use `golangci-lint` with strict configuration:

```bash
# Run locally
just lint

# Run in Docker
docker compose run --rm lint

# Auto-fix some issues
golangci-lint run --fix
```

**All PRs must pass linting!** Fix any issues before submitting.

### Pre-commit Checks

Before committing, run:

```bash
# Format code
just fmt

# Run tests
just test

# Run linter
just lint
```

Or all at once:

```bash
just test-all
```

## Submitting Changes

### Pull Request Process

1. **Update your branch** with latest main:
   ```bash
   git fetch upstream
   git rebase upstream/main
   ```

2. **Run quality checks**:
   ```bash
   just test-all
   ```

3. **Push to your fork**:
   ```bash
   git push origin feature/your-feature-name
   ```

4. **Create Pull Request** on GitHub:
   - Use a clear, descriptive title
   - Reference related issues (e.g., "Fixes #123")
   - Describe what changed and why
   - Include test results if relevant

### PR Title Format

```
feat: add cleanup --dry-run flag
fix: resolve metadata race condition
docs: update installation instructions
test: add integration tests for promote command
```

### PR Description Template

```markdown
## Description
Brief description of what this PR does.

## Motivation
Why is this change needed? What problem does it solve?

## Changes
- Added X
- Modified Y
- Removed Z

## Testing
How was this tested?
- [ ] Unit tests added
- [ ] Integration tests added
- [ ] Manual testing performed

## Related Issues
Fixes #123
Relates to #456

## Checklist
- [ ] Tests pass locally
- [ ] Linting passes
- [ ] Documentation updated
- [ ] Commit messages follow convention
```

### Review Process

1. **Automated checks** run (tests, linting)
2. **Maintainer review** - may request changes
3. **Address feedback** - push updates to your branch
4. **Approval** - maintainer approves PR
5. **Merge** - maintainer merges to main

## Project Structure

```
hitch/
├── cmd/hitch/              # CLI entry point
│   └── main.go
├── internal/
│   ├── cmd/                # Command implementations
│   │   ├── init.go
│   │   ├── promote.go
│   │   ├── release.go
│   │   └── ...
│   ├── git/                # Git operations
│   │   ├── repo.go
│   │   └── repo_test.go
│   ├── metadata/           # Metadata management
│   │   ├── types.go
│   │   ├── reader.go
│   │   ├── writer.go
│   │   └── metadata_test.go
│   └── testutil/           # Test helpers
│       └── git.go
├── .github/
│   └── workflows/          # CI/CD
│       ├── test.yml        # PR/push tests
│       └── release.yml     # Release automation
├── docs/                   # Documentation
├── examples/               # Usage examples
├── .goreleaser.yml         # Release config
├── Justfile                # Development tasks
├── Dockerfile.test         # Test container
├── docker-compose.yml      # Test orchestration
└── README.md
```

### Key Packages

- **cmd/hitch** - CLI entry point, uses Cobra
- **internal/cmd** - Command implementations
- **internal/git** - Git repository operations wrapper
- **internal/metadata** - Hitch metadata (hitch.json) management
- **internal/testutil** - Test helpers for isolated Git repos

## Release Process

Releases are automated via GitHub Actions:

1. Maintainer runs: `just release v0.X.Y`
2. Tag is pushed to GitHub
3. GitHub Actions:
   - Runs all tests
   - Builds binaries for all platforms
   - Creates GitHub Release
   - Updates Homebrew tap
4. Users can install via `brew upgrade hitch`

See [RELEASING.md](./RELEASING.md) for maintainer details.

## Getting Help

- **Questions?** Open a [Discussion](https://github.com/DoomedRamen/hitch/discussions)
- **Bug report?** Open an [Issue](https://github.com/DoomedRamen/hitch/issues)
- **Chat?** Join our [Discord/Slack] (if applicable)

## License

By contributing, you agree that your contributions will be licensed under the MIT License.

## Recognition

Contributors are recognized in:
- [CHANGELOG.md](./CHANGELOG.md) for each release
- GitHub's Contributors page
- Release notes

Thank you for contributing to Hitch! 🚀
