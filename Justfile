# Hitch development tasks

# Default recipe (list all recipes)
default:
    @just --list

# Build the hitch binary
build:
    go build -o hitch ./cmd/hitch

# Build for multiple platforms
build-all:
    GOOS=linux GOARCH=amd64 go build -o dist/hitch-linux-amd64 ./cmd/hitch
    GOOS=darwin GOARCH=amd64 go build -o dist/hitch-darwin-amd64 ./cmd/hitch
    GOOS=darwin GOARCH=arm64 go build -o dist/hitch-darwin-arm64 ./cmd/hitch
    GOOS=windows GOARCH=amd64 go build -o dist/hitch-windows-amd64.exe ./cmd/hitch

# Run tests
test:
    go test ./...

# Run tests with coverage
test-coverage:
    go test -coverprofile=coverage.out ./...
    go tool cover -html=coverage.out

# Format code
fmt:
    go fmt ./...

# Lint code
lint:
    golangci-lint run

# Clean build artifacts
clean:
    rm -f hitch
    rm -rf dist/
    rm -f coverage.out

# Install locally
install: build
    cp hitch /usr/local/bin/hitch

# Uninstall
uninstall:
    rm -f /usr/local/bin/hitch

# Run hitch with arguments
run *ARGS:
    go run ./cmd/hitch {{ARGS}}

# Show hitch version
version: build
    ./hitch version

# Initialize go mod
mod-init:
    go mod tidy
    go mod download

# Update dependencies
mod-update:
    go get -u ./...
    go mod tidy

# Check for outdated dependencies
mod-check:
    go list -u -m all

# Development mode - build and run
dev *ARGS: build
    ./hitch {{ARGS}}

# Release management

# Check if ready for release
release-check:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "🔍 Checking release readiness..."

    # Check we're on main
    BRANCH=$(git branch --show-current)
    if [ "$BRANCH" != "main" ]; then
        echo "❌ Must be on main branch (currently on: $BRANCH)"
        exit 1
    fi

    # Check for uncommitted changes
    if ! git diff-index --quiet HEAD --; then
        echo "❌ Uncommitted changes detected"
        git status --short
        exit 1
    fi

    # Check we're up to date with remote
    git fetch origin main
    LOCAL=$(git rev-parse main)
    REMOTE=$(git rev-parse origin/main)
    if [ "$LOCAL" != "$REMOTE" ]; then
        echo "❌ Local main is not in sync with remote"
        exit 1
    fi

    echo "✅ Branch: main"
    echo "✅ No uncommitted changes"
    echo "✅ In sync with remote"
    echo ""
    echo "Running tests..."
    go test ./...
    echo ""
    echo "✅ Ready to release!"

# Test release locally with GoReleaser snapshot
release-snapshot:
    @echo "📦 Building release snapshot..."
    @rm -rf dist/
    goreleaser release --snapshot --clean
    @echo ""
    @echo "✅ Snapshot built successfully!"
    @echo ""
    @echo "Binaries in dist/:"
    @ls -lh dist/*.tar.gz dist/*.zip 2>/dev/null || true

# Create and push release tag
release VERSION:
    #!/usr/bin/env bash
    set -euo pipefail

    # Validate version format
    if [[ ! "{{VERSION}}" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
        echo "❌ Version must be in format: vX.Y.Z (e.g., v0.1.0)"
        exit 1
    fi

    # Check if tag already exists
    if git rev-parse "{{VERSION}}" >/dev/null 2>&1; then
        echo "❌ Tag {{VERSION}} already exists"
        echo "To delete: git tag -d {{VERSION}} && git push origin :refs/tags/{{VERSION}}"
        exit 1
    fi

    # Run pre-release checks
    just release-check

    echo ""
    echo "🏷️  Creating release tag: {{VERSION}}"

    # Create tag with message
    git tag -a "{{VERSION}}" -m "Release {{VERSION}}"

    echo "✅ Tag created locally"
    echo ""
    echo "📤 Pushing tag to GitHub..."

    # Push tag
    git push origin "{{VERSION}}"

    echo ""
    echo "✅ Release tag pushed!"
    echo ""
    echo "🚀 GitHub Actions will now:"
    echo "   1. Run tests"
    echo "   2. Build binaries for all platforms"
    echo "   3. Create GitHub Release"
    echo "   4. Update Homebrew formula"
    echo ""
    echo "Monitor progress:"
    echo "   just release-status"
    echo ""
    echo "View release when complete:"
    echo "   https://github.com/DoomedRamen/hitch/releases/tag/{{VERSION}}"

# Check GitHub Actions release status
release-status:
    @echo "📊 Recent workflow runs:"
    @gh run list --limit 5
    @echo ""
    @echo "To watch live: gh run watch <run-id>"

# Delete a release tag (use with caution!)
release-delete TAG:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "⚠️  Deleting release tag: {{TAG}}"
    echo "This will delete the tag locally and remotely."
    read -p "Are you sure? (y/N): " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        git tag -d "{{TAG}}" || true
        git push origin ":refs/tags/{{TAG}}" || true
        echo "✅ Tag deleted"
    else
        echo "❌ Cancelled"
    fi

# Bump version in files (dry run)
release-bump TYPE:
    #!/usr/bin/env bash
    set -euo pipefail

    # Get current version from latest tag
    CURRENT=$(git describe --tags --abbrev=0 2>/dev/null || echo "v0.0.0")
    CURRENT=${CURRENT#v}

    IFS='.' read -r MAJOR MINOR PATCH <<< "$CURRENT"

    case "{{TYPE}}" in
        major)
            MAJOR=$((MAJOR + 1))
            MINOR=0
            PATCH=0
            ;;
        minor)
            MINOR=$((MINOR + 1))
            PATCH=0
            ;;
        patch)
            PATCH=$((PATCH + 1))
            ;;
        *)
            echo "❌ TYPE must be: major, minor, or patch"
            exit 1
            ;;
    esac

    NEW="v${MAJOR}.${MINOR}.${PATCH}"

    echo "Current version: v$CURRENT"
    echo "New version:     $NEW"
    echo ""
    echo "To release:"
    echo "  just release $NEW"
