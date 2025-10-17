# Changelog

All notable changes to Hitch will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Comprehensive test suite with isolated test environments
- Docker-based testing infrastructure
- CI/CD workflow for automated testing and linting
- Code quality tools (golangci-lint)
- CONTRIBUTING.md with development guidelines
- TESTING.md with testing documentation

## [0.1.4] - 2025-10-17

### Added
- Homebrew tap token support for automatic formula updates
- Token configuration in .goreleaser.yml for cross-repository access

### Changed
- Updated GitHub Actions workflow to use HOMEBREW_TAP_GITHUB_TOKEN
- Improved release automation for Homebrew distribution

### Fixed
- Homebrew formula automatic updates now work correctly
- Repository visibility issues resolved (made public)

## [0.1.3] - 2025-10-17

### Changed
- Attempted homebrew_casks configuration (reverted in 0.1.4)
- Token authentication improvements

### Notes
- This release was part of the Homebrew tap debugging process
- Not recommended for use; upgrade to 0.1.4+

## [0.1.2] - 2025-10-17

### Changed
- Migrated from deprecated `brews` to `homebrew_casks` in GoReleaser config
- Updated Homebrew tap directory structure

### Notes
- This release was part of the Homebrew tap debugging process
- Configuration was corrected in 0.1.4 to use `brews` (CLI tools)

## [0.1.1] - 2025-10-17

### Added
- Project logo (hitch.svg) added to README
- Enhanced Justfile with comprehensive release management recipes
  - `just release-check` - Pre-release validation
  - `just release-snapshot` - Local release testing
  - `just release VERSION` - Automated release creation
  - `just release-status` - Monitor GitHub Actions
  - `just release-delete TAG` - Tag cleanup (with confirmation)
  - `just release-bump TYPE` - Version calculation helper

### Changed
- Improved release workflow documentation
- Enhanced Justfile with better error handling and user feedback

## [0.1.0] - 2025-10-17

### Added

#### Core Commands
- `hitch init` - Initialize Hitch in a repository
  - Creates metadata branch
  - Configures environments (dev, qa)
  - Sets up default configuration
  - Optional `--no-push` flag to skip remote push
- `hitch status` - Show environment and branch status
  - Displays features in each environment
  - Shows branch lifecycle information
  - Identifies stale branches
- `hitch promote <branch> to <env>` - Promote branch to environment
  - Adds feature to environment
  - Tracks promotion history
  - Automatically rebuilds environment
- `hitch demote <branch> from <env>` - Remove branch from environment
  - Removes feature from environment
  - Tracks demotion in history
  - Rebuilds environment
- `hitch release <branch>` - Merge feature to main
  - Validates branch is promoted to at least one environment
  - Merges to main branch
  - Marks branch for cleanup
  - Tracks merge metadata
  - Supports `--squash` for squash merges
  - Supports `--message` for custom commit messages
  - Supports `--no-delete` to prevent cleanup marking
- `hitch rebuild <env>` - Rebuild environment from scratch
  - Creates temporary branch for safety
  - Merges all features in order
  - Only swaps if all merges succeed
  - Detects and reports merge conflicts
  - Force-pushes rebuilt environment
- `hitch cleanup` - Delete merged branches
  - Identifies branches eligible for cleanup
  - Respects retention period
  - Supports `--dry-run` flag
  - Deletes both local and remote branches
- `hitch lock <env>` - Manually lock environment
- `hitch unlock <env>` - Manually unlock environment
- `hitch hook pre-push` - Git hook integration for validation

#### Infrastructure
- Complete metadata management system
  - JSON-based metadata storage in orphan branch
  - Environment configuration and state tracking
  - Branch lifecycle tracking
  - Lock management for concurrent safety
  - Promotion/demotion history
- Git operations wrapper
  - Safe Git command execution
  - Branch management (create, delete, merge)
  - Push/pull operations with error handling
  - Merge conflict detection
- Automatic environment locking during rebuilds
- Stale lock detection and timeout handling

#### Release & Distribution
- GoReleaser configuration for automated releases
  - Multi-platform builds (macOS, Linux, Windows)
  - ARM64 and AMD64 support
  - GitHub Releases integration
  - Changelog generation from commits
- Homebrew tap for easy installation
  - Formula automatically updated on release
  - Supports macOS and Linux
- GitHub Actions workflows
  - Automated release on version tags
  - Platform-specific binary builds
  - Checksum generation
- Installation script for curl-based installation

#### Documentation
- Comprehensive README with installation and usage
- COMMANDS.md with detailed command reference
- ARCHITECTURE.md explaining system design
- WORKFLOWS.md with common usage patterns
- TERMINOLOGY.md for Hitch-specific terms
- SAFETY.md covering safety mechanisms
- RELEASING.md for maintainer release process
- HOOKS.md for Git hook integration

#### Developer Tools
- Justfile with development tasks
  - Build commands
  - Test runners
  - Code formatting
  - Linting
  - Dependency management
  - Release helpers
- MIT License
- Professional project structure

### Changed
- Standardized terminology to "hitched branches" throughout codebase and documentation

### Technical Details
- Built with Go 1.22+
- Uses Cobra for CLI framework
- Uses go-git for Git operations
- Stores metadata in `hitch-metadata` orphan branch
- Metadata format: JSON (hitch.json)

## [0.0.0] - Initial Development

- Project conception and initial planning
- Core architecture design
- Proof of concept implementations

---

## Release Categories

### Added
New features and capabilities.

### Changed
Changes to existing functionality.

### Deprecated
Features that will be removed in future releases.

### Removed
Features that have been removed.

### Fixed
Bug fixes.

### Security
Security-related changes.

---

## Upgrade Instructions

### From 0.1.x to 0.1.4+
No breaking changes. Safe to upgrade:

```bash
brew upgrade hitch
```

### From 0.0.x to 0.1.0
Major version with complete rewrite. Repositories using pre-0.1.0 versions should:

1. Export/backup any existing metadata
2. Uninstall old version
3. Install 0.1.0+
4. Reinitialize with `hitch init`

---

## Links

- [GitHub Releases](https://github.com/DoomedRamen/hitch/releases)
- [Installation Guide](https://github.com/DoomedRamen/hitch#installation)
- [Documentation](https://github.com/DoomedRamen/hitch/tree/main)
- [Report Issues](https://github.com/DoomedRamen/hitch/issues)

[Unreleased]: https://github.com/DoomedRamen/hitch/compare/v0.1.4...HEAD
[0.1.4]: https://github.com/DoomedRamen/hitch/releases/tag/v0.1.4
[0.1.3]: https://github.com/DoomedRamen/hitch/releases/tag/v0.1.3
[0.1.2]: https://github.com/DoomedRamen/hitch/releases/tag/v0.1.2
[0.1.1]: https://github.com/DoomedRamen/hitch/releases/tag/v0.1.1
[0.1.0]: https://github.com/DoomedRamen/hitch/releases/tag/v0.1.0
