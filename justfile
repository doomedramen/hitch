# Hitch CLI Tool - Just Commands
# This file defines common development tasks for the Hitch project

# Default recipe
default:
    @echo "Available commands:"
    @echo "  build     - Build the project in release mode"
    @echo "  format    - Format the code"
    @echo "  lint      - Run linters"
    @echo "  install   - Install the binary locally"
    @echo "  test      - Run all tests"
    @echo "  clean     - Clean build artifacts"
    @echo "  dev       - Build in debug mode"
    @echo "  run       - Run the hitch binary in debug mode"

# Build the project in release mode
build:
    @echo "🔨 Building Hitch in release mode..."
    cargo build --release
    @echo "✅ Build complete. Binary available at: ./target/release/hitch"

# Format the code using rustfmt
format:
    @echo "🎨 Formatting code..."
    cargo fmt
    @echo "✅ Code formatted"

# Run linters (clippy)
lint:
    @echo "🔍 Running linters..."
    cargo clippy --all-targets --all-features -- -W clippy::all
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
    cargo test
    @echo "✅ All tests passed"

# Run tests with verbose output
test-verbose:
    @echo "🧪 Running all tests with verbose output..."
    cargo test -- --nocapture
    @echo "✅ All tests passed"

# Run specific test file
test-file file:
    @echo "🧪 Running tests in {{file}}..."
    cargo test --test {{file}}
    @echo "✅ Tests in {{file}} passed"

# Run tests with coverage (requires cargo-tarpaulin)
test-coverage:
    @echo "🧪 Running tests with coverage..."
    @command -v cargo-tarpaulin >/dev/null 2>&1 || \
        (echo "❌ cargo-tarpaulin not found. Install with: cargo install cargo-tarpaulin" && exit 1)
    cargo tarpaulin --out Html
    @echo "✅ Coverage report generated in tarpaulin-report.html"

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
    @echo "🚀 Running Hitch with args: {{args}}"
    ./target/debug/hitch {{args}}

# Check if the code compiles without building (faster than build)
check:
    @echo "🔍 Checking if code compiles..."
    cargo check
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
    cargo doc --no-deps --document-private-items
    @echo "✅ Documentation generated. Open with: cargo doc --open"

# Serve documentation locally
docs-serve: docs
    @echo "🌐 Serving documentation locally..."
    @command -v python3 >/dev/null 2>&1 && \
        python3 -m http.server 8000 --directory target/doc || \
        (echo "❌ python3 not found for serving docs" && exit 1)

# Create a release (build, test, lint, format)
release: format lint test build
    @echo "🎉 Hitch release ready!"
    @echo "Binary available at: ./target/release/hitch"

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