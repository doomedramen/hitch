# Hitch Documentation Index

Complete reference for the Hitch project.

## Quick Links

- **[README.md](./README.md)** - Start here! Project overview, quick start, installation
- **[COMMANDS.md](./COMMANDS.md)** - Complete CLI reference for all commands
- **[WORKFLOWS.md](./WORKFLOWS.md)** - Real-world usage examples and team workflows
- **[HOOKS.md](./HOOKS.md)** - Git hook integration guide (Husky, Lefthook, etc.)

## Deep Dive

- **[ARCHITECTURE.md](./ARCHITECTURE.md)** - Technical design, algorithms, safety mechanisms
- **[METADATA.md](./METADATA.md)** - Complete JSON schema specification for `hitch.json`
- **[SAFETY.md](./SAFETY.md)** - Comprehensive safety guarantees and error handling

## Documentation by Use Case

### Getting Started
1. Read [README.md](./README.md) for overview
2. Install Hitch
3. Run `hitch init` in your repository
4. Follow quick start examples in README

### Daily Usage
- **Common commands**: [COMMANDS.md](./COMMANDS.md)
- **Team workflows**: [WORKFLOWS.md](./WORKFLOWS.md)
- **Check status**: `hitch status`

### Hook Integration
- **Setup guide**: [HOOKS.md](./HOOKS.md)
- **Quick setup**: Add `hitch hook pre-push` to `.git/hooks/pre-push`
- **Framework integration**: See HOOKS.md for Husky, Lefthook, etc.

### Understanding Internals
- **How it works**: [ARCHITECTURE.md](./ARCHITECTURE.md)
- **Metadata format**: [METADATA.md](./METADATA.md)
- **Safety mechanisms**: [SAFETY.md](./SAFETY.md)

### Troubleshooting
- **Common issues**: [WORKFLOWS.md#Troubleshooting](./WORKFLOWS.md#troubleshooting)
- **Safety guarantees**: [SAFETY.md](./SAFETY.md)
- **Command reference**: [COMMANDS.md](./COMMANDS.md)

## Documentation Files

### README.md
**Purpose**: First stop for new users
**Contents**:
- Problem statement and solution
- Installation instructions
- Quick start guide
- Core commands list
- Basic examples

**Read if**: You're new to Hitch

---

### COMMANDS.md
**Purpose**: Complete CLI reference
**Contents**:
- All commands with flags and options
- Input/output examples
- Error handling examples
- Exit codes
- Environment variables

**Read if**: You need to know exact command syntax

---

### WORKFLOWS.md
**Purpose**: Real-world usage patterns
**Contents**:
- Standard feature development flow
- Multi-developer coordination
- Emergency hotfixes
- Long-running features
- Release procedures
- Troubleshooting guide
- CI/CD integration examples

**Read if**: You want to see how teams actually use Hitch

---

### HOOKS.md
**Purpose**: Git hook integration guide
**Contents**:
- `hitch hook` command reference
- Manual Git hooks setup
- Husky integration
- Lefthook integration
- pre-commit framework integration
- Server-side protection (GitHub Actions, GitLab CI)
- Troubleshooting hook issues

**Read if**: You want to prevent pushes to locked branches

---

### ARCHITECTURE.md
**Purpose**: Technical design documentation
**Contents**:
- Core concepts (metadata branch, ephemeral integration branches, locking)
- Safety guarantees (always return to original branch, temp branches, clean state validation)
- Data model and Git operations
- Key algorithms (rebuild, promote, stale detection)
- Error handling strategies
- Dependencies and testing approach
- Future enhancements

**Read if**: You're contributing code or need deep understanding

---

### METADATA.md
**Purpose**: Specification for hitch.json format
**Contents**:
- Complete JSON schema
- Field definitions with types
- Validation rules
- Concurrency control (optimistic locking)
- Schema evolution and versioning
- Backup and recovery procedures
- Complete examples

**Read if**: You're debugging metadata issues or building integrations

---

### SAFETY.md
**Purpose**: Comprehensive safety documentation
**Contents**:
- 12 core safety guarantees
- Metadata branch isolation
- Always return to original branch
- Safe rebuilding with temp branches
- Optimistic concurrency control
- Environment locking
- Force-push safety
- Dry run mode
- Atomic operations
- Idempotent operations
- Error recovery
- Git hook safety
- Backup and recovery
- Emergency procedures

**Read if**: You want to understand how Hitch prevents data loss

---

## Documentation Standards

### Code Examples

All code examples use:
- Bash syntax highlighting for shell commands
- Go syntax highlighting for implementation pseudocode
- JSON syntax highlighting for metadata examples
- YAML syntax highlighting for CI/CD configs

### Conventions

- **Commands**: `hitch command` in code blocks
- **Branches**: `feature/branch-name` or `dev`, `qa`, `main`
- **Files**: `hitch.json`, `.git/hooks/pre-push`
- **Flags**: `--flag-name`
- **Variables**: `<placeholder>` or `$VARIABLE`

### Output Format

Command outputs show:
- ✓ Success indicators
- ✗ Error indicators
- ⚠️ Warning indicators
- Exit codes where relevant

### Examples Use Realistic Data

- Email: `dev-m@example.com`, `dev-s@example.com`
- Dates: `2025-10-16T10:30:00Z`
- Commits: `a1b2c3d`, `b2c3d4e`
- URLs: `https://dev.example.org`

---

## Contributing to Documentation

### Adding New Documentation

1. Add file to repository root
2. Update this index (DOCUMENTATION.md)
3. Link from README.md if user-facing
4. Use consistent formatting

### Updating Existing Documentation

1. Keep examples realistic and tested
2. Update version numbers if needed
3. Check cross-references are still valid
4. Update table of contents if structure changes

### Documentation Checklist

- [ ] Clear purpose statement
- [ ] Realistic examples
- [ ] Error cases shown
- [ ] Links to related docs
- [ ] Code syntax highlighting
- [ ] Consistent formatting
- [ ] Tested commands (where applicable)

---

## Quick Reference

### Essential Commands

```bash
hitch init                          # Initialize Hitch
hitch status                        # Check environment state
hitch promote feature/xyz to dev    # Add feature to dev
hitch promote feature/xyz to qa     # Promote to qa
hitch release feature/xyz           # Merge to main
hitch rebuild dev                   # Rebuild environment
hitch cleanup                       # Delete stale branches
```

### Essential Concepts

- **Environments**: Ephemeral integration branches (dev, qa)
- **Features**: Your actual work branches
- **Metadata**: State stored in `hitch-metadata` branch
- **Locking**: Prevents concurrent modifications
- **Temp branches**: Used during rebuilds for safety

### Essential Files

- `hitch.json` - Metadata (in `hitch-metadata` branch)
- `.git/hooks/pre-push` - Optional lock checking
- `hitch` binary - The CLI tool

---

## Version

This documentation is for **Hitch v1.0.0** (planned).

Last updated: 2025-10-16
