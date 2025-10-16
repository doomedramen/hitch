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
