# Hitch CLI Tool - Just Commands
# This file defines common development tasks for the Hitch project

# Default recipe
default:
    @just --list

# Build the project in release mode
build:
    @echo "🔨 Building Hitch in release mode..."
    cargo build --release -p hitch
    @echo "✅ Build complete. Binary available at: ./target/release/hitch"

# Format the code using rustfmt
format:
    @echo "🎨 Formatting code..."
    cargo fmt
    @echo "✅ Code formatted"

# Check code formatting without modifying files
format-check:
    @echo "🎨 Checking code formatting..."
    cargo fmt --all -- --check
    @echo "✅ Code formatting is correct"

# Run linters (clippy)
lint:
    @echo "🔍 Running linters..."
    cargo clippy -p hitch --all-targets -- -D warnings
    @echo "✅ Linting completed (check output for warnings)"

# Install the binary locally (release build)
install: build
    @echo "📦 Installing Hitch locally..."
    cargo install --path .
    @echo "✅ Hitch installed successfully"

# Install the binary from the release build directly
install-release: build
    @echo "📦 Installing Hitch from release build..."
    cp ./target/release/hitch ~/.cargo/bin/hitch
    @echo "✅ Hitch installed to ~/.cargo/bin/hitch"

# Run all tests
test:
    @echo "🧪 Running all tests..."
    cargo test -p hitch
    @echo "✅ All tests passed"

# Run core tests only (for release when some integration tests are failing)
test-core:
    @echo "🧪 Running core tests..."
    cargo test -p hitch --lib --bins
    @echo "✅ Core tests passed"

# Run tests with verbose output
test-verbose:
    @echo "🧪 Running all tests with verbose output..."
    cargo test -- --nocapture
    @echo "✅ All tests passed"

# Run specific test file
test-file file:
    @echo "🧪 Running tests in {{ file }}..."
    cargo test --test {{ file }}
    @echo "✅ Tests in {{ file }} passed"

# Run security audit
audit:
    @echo "🔒 Running security audit..."
    cargo audit
    @echo "✅ Security audit completed"

# Run tests with coverage (requires cargo-tarpaulin)
test-coverage:
    @echo "🧪 Running tests with coverage..."
    @command -v cargo-tarpaulin >/dev/null 2>&1 || \
        (echo "❌ cargo-tarpaulin not found. Install with: cargo install cargo-tarpaulin" && exit 1)
    cargo tarpaulin --out Stdout -- --test-threads=1
    @echo ""
    @echo "✅ Coverage report completed"

# Generate coverage report for analysis (using llvm-cov)
test-coverage-full:
    @echo "🧪 Running full coverage analysis..."
    @command -v cargo-llvm-cov >/dev/null 2>&1 || \
        (echo "❌ cargo-llvm-cov not found. Install with: cargo install cargo-llvm-cov" && exit 1)
    @cargo llvm-cov --workspace --text -- --test-threads=1 2>&1 | tee coverage-report.txt
    @echo "📋 Coverage report saved: coverage-report.txt"
    @echo "💡 Check coverage-report.txt for detailed coverage analysis"

# Generate detailed coverage report in text files
test-coverage-detailed:
    @echo "🧪 Running tests with detailed coverage..."
    @command -v cargo-tarpaulin >/dev/null 2>&1 || \
        (echo "❌ cargo-tarpaulin not found. Install with: cargo install cargo-tarpaulin" && exit 1)
    cargo tarpaulin --out Json --output-dir coverage/ -- --test-threads=1
    cargo tarpaulin --out Xml --output-dir coverage/ -- --test-threads=1
    @echo "✅ Detailed coverage reports generated in coverage/ directory"

# Run coverage using llvm-cov (matches CI command)
coverage:
    @echo "🧪 Running tests with coverage (llvm-cov)..."
    @command -v cargo-llvm-cov >/dev/null 2>&1 || \
        (echo "❌ cargo-llvm-cov not found. Install with: cargo install cargo-llvm-cov" && exit 1)
    cargo llvm-cov --lib --bins --lcov --output-path lcov.info -- --test-threads=1
    @echo "✅ Coverage report generated: lcov.info"

# Show just the coverage summary without checking thresholds
test-coverage-summary:
    @echo "📊 Coverage Summary:"
    @command -v cargo-tarpaulin >/dev/null 2>&1 || \
        (echo "❌ cargo-tarpaulin not found. Install with: cargo install cargo-tarpaulin" && exit 1)
    @echo ""
    @cargo tarpaulin --workspace --out Stdout -- --test-threads=1 2>/dev/null | tail -5

# Generate coverage check (shows current coverage percentage)
test-coverage-check:
    @echo "🧪 Current Coverage Analysis..."
    @command -v cargo-tarpaulin >/dev/null 2>&1 || \
        (echo "❌ cargo-tarpaulin not found. Install with: cargo install cargo-tarpaulin" && exit 1)
    @echo ""
    @cargo tarpaulin --workspace --out Stdout -- --test-threads=1 | tail -2

# Clean build artifacts
clean:
    @echo "🧹 Cleaning build artifacts..."
    cargo clean
    @echo "✅ Build artifacts cleaned"

# Build in debug mode
dev:
    @echo "🔨 Building Hitch in debug mode..."
    cargo build
    @echo "✅ Debug build complete. Binary available at: ./target/debug/hitch"

# Run the hitch binary in debug mode
run: dev
    @echo "🚀 Running Hitch in debug mode..."
    ./target/debug/hitch

# Run the hitch binary with arguments
run-with args: dev
    @echo "🚀 Running Hitch with args: {{ args }}"
    ./target/debug/hitch {{ args }}

# Check if the code compiles without building (faster than build)
check:
    @echo "🔍 Checking if code compiles..."
    cargo check -p hitch
    @echo "✅ Code compiles successfully"

# Update dependencies
update:
    @echo "⬆️ Updating dependencies..."
    cargo update
    @echo "✅ Dependencies updated"

# Check for outdated dependencies
outdated:
    @echo "📋 Checking for outdated dependencies..."
    @command -v cargo-outdated >/dev/null 2>&1 || \
        (echo "❌ cargo-outdated not found. Install with: cargo install cargo-outdated" && exit 1)
    cargo outdated

# Generate documentation
docs:
    @echo "📚 Generating documentation..."
    cargo doc -p hitch --no-deps --document-private-items
    @echo "✅ Documentation generated. Open with: cargo doc --open"

# Generate documentation for CI (no-deps, all-features, private items)
docs-ci:
    @echo "📚 Generating documentation for CI..."
    cargo doc -p hitch --no-deps --document-private-items
    @echo "✅ Documentation generated for CI"

# -----------------------
# Desktop GUI (Tauri)
# -----------------------

desktop-dev:
    @echo "🖥️ Starting Hitch Desktop (dev)..."
    cd crates/hitch-desktop && CI=true pnpm install --no-frozen-lockfile
    cd crates/hitch-desktop && pnpm tauri:dev

desktop-build:
    @echo "🖥️ Building Hitch Desktop..."
    cd crates/hitch-desktop && CI=true pnpm install --no-frozen-lockfile
    cd crates/hitch-desktop && pnpm tauri:build

desktop-build-dmg:
    @echo "🖥️ Building Hitch Desktop (macOS DMG)..."
    cd crates/hitch-desktop && CI=true pnpm install --no-frozen-lockfile
    cd crates/hitch-desktop && pnpm tauri build --bundles dmg

desktop-check:
    @echo "🖥️ Checking Hitch Desktop (frontend build + Rust compile)..."
    cd crates/hitch-desktop && CI=true pnpm install --frozen-lockfile
    cd crates/hitch-desktop && pnpm build
    cargo check -p hitch-desktop

desktop-icons:
    @echo "🎨 Generating Hitch Desktop icons from hitch.svg..."
    mkdir -p crates/hitch-desktop/src-tauri/icons
    cd crates/hitch-desktop && CI=true pnpm install --frozen-lockfile --prefer-offline
    cd crates/hitch-desktop && pnpm tauri icon --output src-tauri/icons ../../hitch.svg

# Create a new desktop release (auto-bump version like CLI)
release-desktop:
    #!/usr/bin/env bash
    set -e

    echo "🚀 Creating new desktop release with automatic version bump..."

    # Safety checks
    current_branch=$(git branch --show-current)
    if [ "$current_branch" != "main" ]; then
        echo "❌ Error: Releases must be created from the 'main' branch"
        echo "   Current branch: $current_branch"
        echo "   Please checkout main and try again: git checkout main"
        exit 1
    fi

    if ! git diff --quiet || ! git diff --cached --quiet; then
        echo "❌ Error: Working directory has uncommitted changes"
        echo "   Please commit or stash your changes before releasing"
        git status --short
        exit 1
    fi

    current_version=$(grep '"version"' crates/hitch-desktop/package.json | sed 's/.*"\([^"]*\)".*/\1/')
    echo "Current version: v${current_version}"

    major=$(echo ${current_version} | cut -d. -f1)
    minor=$(echo ${current_version} | cut -d. -f2)
    patch=$(echo ${current_version} | cut -d. -f3)
    new_patch=$((patch + 1))
    new_version="${major}.${minor}.${new_patch}"
    echo "New version: v${new_version}"

    echo "🔍 Running pre-flight checks..."
    cd crates/hitch-desktop
    pnpm build
    cargo check -p hitch-desktop
    cd ../..

    echo "📝 Updating version..."
    sed -i '' "s/\"version\": \"${current_version}\"/\"version\": \"${new_version}\"/" crates/hitch-desktop/package.json
    sed -i '' "s/version = \"${current_version}\"/version = \"${new_version}\"/" crates/hitch-desktop/src-tauri/Cargo.toml
    jq ".version = \"${new_version}\"" crates/hitch-desktop/src-tauri/tauri.conf.json > tmp.json && mv tmp.json crates/hitch-desktop/src-tauri/tauri.conf.json

    echo "📦 Committing version bump..."
    git add crates/hitch-desktop/package.json crates/hitch-desktop/src-tauri/Cargo.toml crates/hitch-desktop/src-tauri/tauri.conf.json

    if [ -n "$(git status --porcelain Cargo.lock)" ]; then
        echo "📦 Cargo.lock updated, including in commit..."
        git add Cargo.lock
    fi

    git commit -m "chore: bump desktop version to v${new_version}"

    echo "🏷️ Creating tag..."
    git tag "desktop-v${new_version}"

    echo "🚀 Pushing to trigger release workflow..."
    git push origin main
    git push origin "desktop-v${new_version}"

    echo "✅ Release v${new_version} triggered! Check GitHub Actions for progress."

# Serve documentation locally
docs-serve: docs
    @echo "🌐 Serving documentation locally..."
    @command -v python3 >/dev/null 2>&1 && \
        python3 -m http.server 8000 --directory target/doc || \
        (echo "❌ python3 not found for serving docs" && exit 1)

# Create a new release (bump version, test, build, and push tag)
release:
    #!/usr/bin/env bash
    set -e

    # Function to cleanup on failure
    cleanup() {
        if [ $? -ne 0 ]; then
            echo "❌ Release failed, cleaning up..."
            # Restore original version if it was changed
            if [ -n "${current_version}" ] && [ -n "${new_version}" ]; then
                echo "🔄 Restoring version from v${new_version} to v${current_version}"
                # Use portable sed -i with backup, then remove backup (works on macOS and Linux)
                sed -i.bak "s/^version = \"${new_version}\"/version = \"${current_version}\"/" Cargo.toml && rm -f Cargo.toml.bak

                # Discard any Cargo.lock changes
                git restore Cargo.lock 2>/dev/null || true

                echo "✅ Cleanup completed"
            fi
        fi
    }

    # Set trap for cleanup on error
    trap cleanup ERR

    echo "🚀 Creating new release with automatic version bump..."

    # Safety checks
    current_branch=$(git branch --show-current)
    if [ "$current_branch" != "main" ]; then
        echo "❌ Error: Releases must be created from the 'main' branch"
        echo "   Current branch: $current_branch"
        echo "   Please checkout main and try again: git checkout main"
        exit 1
    fi

    if ! git diff --quiet || ! git diff --cached --quiet; then
        echo "❌ Error: Working directory has uncommitted changes"
        echo "   Please commit or stash your changes before releasing"
        git status --short
        exit 1
    fi

    # Get current version from Cargo.toml
    current_version=$(grep '^version = ' Cargo.toml | head -1 | sed 's/version = "//' | sed 's/"//')
    echo "Current version: v${current_version}"

    # Extract version components
    major=$(echo ${current_version} | cut -d. -f1)
    minor=$(echo ${current_version} | cut -d. -f2)
    patch=$(echo ${current_version} | cut -d. -f3)
    # Increment patch version
    new_patch=$((patch + 1))
    new_version="${major}.${minor}.${new_patch}"
    echo "New version: v${new_version}"

    # Pre-flight checks first (before modifying anything)
    echo "🔍 Running pre-flight checks..."
    cargo fmt --all -- --check
    cargo clippy --all-targets --all-features -- -D warnings
    cargo test --all-targets --all-features --release -- --test-threads=1

    # Now that everything passes, update version
    echo "📝 Updating version..."
    # Use portable sed -i with backup, then remove backup (works on macOS and Linux)
    sed -i.bak "s/^version = \"${current_version}\"/version = \"${new_version}\"/" Cargo.toml && rm -f Cargo.toml.bak

    # Build release with new version
    echo "🔨 Building release v${new_version}..."
    cargo build --release

    # Check if any files were modified during build (like Cargo.lock)
    if [ -n "$(git status --porcelain Cargo.lock)" ]; then
        echo "📦 Cargo.lock was updated during build, including in commit..."
        git add Cargo.lock
    fi

    # Commit the version bump (and any Cargo.lock changes)
    git add Cargo.toml
    git commit -m "chore: bump version to v${new_version}"

    # Create the tag
    git tag "v${new_version}"

    echo "🚀 Pushing commit and tag to trigger release workflow..."
    # Push both the commit and the tag
    current_branch=$(git branch --show-current)
    git push origin "${current_branch}"
    git push origin "v${new_version}"

    # Clear the trap since we succeeded
    trap - ERR

    echo "✅ Release v${new_version} triggered! Check GitHub Actions for progress."
    echo "📦 Binary available at: ./target/release/hitch"

# Create and push a new release tag (for current version without bumping)
release-tag:
    @echo "🏷️ Creating and pushing release tag..."
    # Get current version from Cargo.toml
    @current_version=$(grep '^version = ' Cargo.toml | head -1 | sed 's/version = "//' | sed 's/"//')
    @echo "Current version: v${current_version}"
    # Check if tag already exists
    @if git rev-parse "v${current_version}" >/dev/null 2>&1; then \
        echo "❌ Tag v${current_version} already exists!"; \
        echo "💡 Run 'just release' to create a new version"; \
        exit 1; \
    fi
    # Commit any changes first
    @if [ -n "$(git status --porcelain)" ]; then \
        echo "📝 Committing changes..."; \
        git add .; \
        git commit -m "chore: prepare for v${current_version} release"; \
        echo "🚀 Pushing changes before tag..."; \
        current_branch=$(git branch --show-current); \
        git push origin "${current_branch}"; \
    fi
    # Create and push the tag
    @git tag "v${current_version}"
    @echo "✅ Created tag v${current_version}"
    @git push origin "v${current_version}"
    @echo "🚀 Release triggered! Check GitHub Actions for progress."

# Show project information
info:
    @echo "📊 Hitch CLI Tool Information:"
    @echo "  Version: $(cargo metadata --format-version=1 --no-deps | jq -r '.packages[0].version')"
    @echo "  Rust version: $(rustc --version)"
    @echo "  Cargo version: $(cargo --version)"
    @echo "  Target directory: $(pwd)/target"

# Setup development environment
setup:
    @echo "🛠️ Setting up development environment..."
    @echo "Installing required tools..."
    @command -v cargo >/dev/null 2>&1 || (echo "❌ Cargo not found" && exit 1)
    @command -v rustfmt >/dev/null 2>&1 || cargo install rustfmt
    @command -v cargo-tarpaulin >/dev/null 2>&1 || echo "💡 Install cargo-tarpaulin for test coverage: cargo install cargo-tarpaulin"
    @command -v cargo-outdated >/dev/null 2>&1 || echo "💡 Install cargo-outdated for dependency checking: cargo install cargo-outdated"
    @echo "✅ Development environment ready"
    @echo ""
    @echo "💡 Now install git hooks: lefthook install"

# Watch for changes and rebuild (requires cargo-watch)
watch:
    @echo "👀 Watching for changes..."
    @command -v cargo-watch >/dev/null 2>&1 || \
        (echo "❌ cargo-watch not found. Install with: cargo install cargo-watch" && exit 1)
    cargo watch -x "check" -x "test"

# Benchmark the binary (requires cargo-criterion)
benchmark:
    @echo "📊 Running benchmarks..."
    @command -v cargo-criterion >/dev/null 2>&1 || \
        (echo "❌ cargo-criterion not found. Install with: cargo install cargo-criterion" && exit 1)
    cargo criterion
    @echo "✅ Benchmarks complete"
