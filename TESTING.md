# Testing Guide

This document explains how to run tests for Hitch and how the testing infrastructure works.

## Quick Start

```bash
# Run tests directly (fast)
just test

# Run tests in isolated Docker environment (recommended for local development)
just test-docker

# Run tests with coverage
just test-coverage

# Run all quality checks (tests + lint)
just test-all
```

## Testing Philosophy

Hitch manipulates Git repositories directly, which means tests need isolated environments to avoid "poisoning" the working repository. We achieve this through:

1. **Temporary test repositories** - Each test creates isolated Git repos in `/tmp`
2. **Docker isolation (optional)** - For extra safety during local development
3. **CI isolation** - GitHub Actions runners are already isolated VMs

## Running Tests

### Direct (Fastest)

Tests create temporary Git repositories automatically, so they're safe to run directly:

```bash
go test ./...
```

or

```bash
just test
```

**When to use:** Quick feedback during development, CI/CD

### Docker (Safest for Local Development)

Run tests in complete isolation with Docker:

```bash
# Basic tests
just test-docker

# Verbose output
just test-docker-verbose

# Integration tests
just test-integration

# Coverage report
just test-docker-coverage
```

**When to use:** Before committing, when you want guaranteed isolation

### CI/CD

Tests run automatically on:
- Every push to `main`
- Every pull request
- Before releases

The CI workflow (`test.yml`) runs:
- Unit tests with race detection
- Linting (golangci-lint)
- Multi-platform builds
- Coverage reporting

## Test Structure

### Unit Tests

Located alongside source code with `_test.go` suffix:

```
internal/
  metadata/
    metadata.go
    metadata_test.go  ← Unit tests
  git/
    repo.go
    repo_test.go      ← Unit tests
```

### Test Utilities

The `internal/testutil` package provides helpers for creating isolated test environments:

```go
package mypackage_test

import (
    "testing"
    "github.com/DoomedRamen/hitch/internal/testutil"
)

func TestSomething(t *testing.T) {
    // Create isolated Git repository
    repo := testutil.NewTestRepo(t)
    // repo is automatically cleaned up after test

    // Create branches with commits
    repo.CreateBranch("feature/test", true)

    // Commit files
    repo.CommitFile("test.txt", "content", "Add test file")

    // Use the repo
    currentBranch, _ := repo.GetCurrentBranch()
}
```

### Integration Tests

For end-to-end workflow tests:

```bash
# Build integration tests with special tag
go test -tags=integration ./...

# Or use Docker
just test-integration
```

## Writing Tests

### Best Practices

1. **Use testutil helpers** - Don't create Git repos manually
2. **Test one thing** - Keep tests focused and simple
3. **Use t.Helper()** - Mark helper functions properly
4. **Clean up** - testutil handles this, but be aware
5. **Race detection** - Tests run with `-race` flag in CI

### Example Test

```go
func TestPromoteBranch(t *testing.T) {
    // Setup: Create test repo with Hitch initialized
    repo := testutil.NewTestRepo(t)

    // Initialize Hitch
    meta := metadata.NewMetadata()
    meta.AddEnvironment("dev", "main", "test@example.com")

    // Create a feature branch
    repo.CreateBranch("feature/test", true)

    // Test: Promote branch to dev
    err := meta.AddBranchToEnvironment("dev", "feature/test", "test@example.com")

    // Assert
    if err != nil {
        t.Fatalf("Failed to promote branch: %v", err)
    }

    env := meta.Environments["dev"]
    if len(env.Features) != 1 {
        t.Errorf("Expected 1 feature, got %d", len(env.Features))
    }
}
```

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

Configuration: `.golangci.yml`

### Coverage

Aim for >70% test coverage:

```bash
# Generate coverage report
just test-coverage

# Opens coverage.html in browser
```

## Continuous Integration

### GitHub Actions

Two workflows:

1. **`test.yml`** - Runs on every push/PR
   - Unit tests with race detection
   - Linting
   - Multi-platform builds
   - Coverage reporting

2. **`release.yml`** - Runs on version tags
   - All tests must pass
   - Builds for all platforms
   - Creates GitHub release
   - Updates Homebrew tap

### Status Badges

Add to README:

```markdown
[![Test](https://github.com/DoomedRamen/hitch/actions/workflows/test.yml/badge.svg)](https://github.com/DoomedRamen/hitch/actions/workflows/test.yml)
[![Coverage](https://codecov.io/gh/DoomedRamen/hitch/branch/main/graph/badge.svg)](https://codecov.io/gh/DoomedRamen/hitch)
```

## Debugging Tests

### Verbose Output

```bash
go test -v ./...
```

### Specific Test

```bash
go test -v -run TestPromoteBranch ./internal/metadata
```

### With Debugging

```bash
# Print all output
go test -v ./... 2>&1 | tee test.log

# Keep temp directories for inspection
HITCH_TEST_KEEP_TEMP=1 go test -v ./...
```

### Docker Debugging

```bash
# Enter test container
docker compose run --rm test bash

# Run tests manually inside container
go test -v ./internal/metadata
```

## Common Issues

### "Repository is dirty" errors

Tests create isolated repos, so this shouldn't happen. If it does:

1. Make sure you're using `testutil.NewTestRepo()`
2. Check that cleanup is running (use `t.Cleanup()`)
3. Run tests in Docker for guaranteed isolation

### Race detector warnings

These are real bugs! Fix them by:

1. Using proper locking (mutexes)
2. Avoiding shared state
3. Using channels for communication

### Slow tests

If tests are slow:

1. Run specific packages: `go test ./internal/metadata`
2. Use test caching: `go test -count=1` to disable cache
3. Parallelize: Tests run in parallel by default

## Docker Commands Reference

```bash
# Build test image
docker compose build test

# Run tests
docker compose run --rm test

# Run with custom command
docker compose run --rm test go test -v -run TestSpecific ./...

# Clean up volumes
docker compose down -v

# Rebuild without cache
docker compose build --no-cache test
```

## Adding New Tests

1. Create `*_test.go` file next to source
2. Use `testutil.NewTestRepo()` for isolation
3. Follow naming convention: `TestFunctionName`
4. Run tests: `just test`
5. Run lint: `just lint`
6. Commit and push (CI will validate)

## Resources

- [Go Testing](https://golang.org/pkg/testing/)
- [golangci-lint](https://golangci-lint.run/)
- [Table-driven tests](https://dave.cheney.net/2019/05/07/prefer-table-driven-tests)
- [Testing best practices](https://go.dev/doc/effective_go#testing)
