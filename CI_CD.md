# CI/CD Pipeline Documentation

Hitch is a Git branch management tool for environment-based deployments. This document explains the complete CI/CD pipeline for the Hitch CLI tool.

## 🔄 Workflow Overview

The Hitch project uses a streamlined CI/CD pipeline with two main workflows:

### **1. CI Pipeline** (`.github/workflows/ci.yml`)
**Triggers**: Push to main/develop, Pull Requests

**Jobs**:
- **Test Suite**: Multi-version Rust testing (stable, beta, 1.83.0)
  - Code formatting checks (`rustfmt`)
  - Linting (`clippy`)
  - Basic unit and integration tests
- **Build Matrix**: Cross-platform build verification
  - Linux (x86_64, ARM64)
  - macOS (x86_64, ARM64)
- **Documentation**: Generates and deploys API docs to GitHub Pages

### **2. Release Pipeline** (`.github/workflows/release.yml`)
**Triggers**: Version tags (v1.0.0, etc.)

**Release Process**:
- Uses cargo-dist for cross-platform builds
- Creates GitHub releases with binary artifacts
- Updates Homebrew formula automatically
- Supports multiple target platforms

## 🛡️ Security & Quality

### **Pre-commit Hooks** (`lefthook.yml`)
- **Code formatting**: `rustfmt` with check mode
- **Linting**: `clippy` with strict warnings
- **Security audit**: `cargo audit`
- **Lock file consistency**: `cargo check --locked`

### **Pre-push Hooks**
- **Release build verification**: `cargo build --release`
- **Test suite execution**: `cargo test --release`

### **CI Requirements**
- All tests must pass on stable, beta, and minimum Rust versions (1.83.0+)
- Code must be properly formatted
- No clippy warnings allowed
- Build must succeed on all target platforms

## 🚀 Release Process

### **Automated Release**
```bash
just release
```

**Steps**:
1. Quality checks (format, lint, test)
2. Version auto-increment
3. Cross-platform build
4. Git tag creation and push
5. GitHub Actions triggered

### **What Gets Released**
- **Linux**: x86_64 and ARM64 binaries
- **macOS**: x86_64 (Intel) and ARM64 (Apple Silicon) binaries
- **GitHub Release**: With all binaries and checksums
- **Homebrew**: Formula automatically updated

## 🔧 Configuration Files

| File | Purpose |
|------|---------|
| `.github/workflows/ci.yml` | Main CI pipeline |
| `.github/workflows/release.yml` | Release automation |
| `lefthook.yml` | Pre-commit/pre-push hooks |
| `dist-workspace.toml` | Distribution configuration |

## 🚨 Required Secrets

To enable full CI/CD functionality, configure these repository secrets:

| Secret | Purpose | Required For |
|--------|---------|--------------|
| `HOMEBREW_TAP_TOKEN` | Homebrew formula updates | Release pipeline |
| `GITHUB_TOKEN` | GitHub API access | All workflows (auto-provided) |

## 🔄 Workflow Dependencies

```
Push/PR → CI → Tests → Build → Docs
    ↓
Release Tag → Release Pipeline → GitHub Release → Homebrew
```

## 📝 Best Practices

1. **Always run `just release` locally** before pushing major changes
2. **Follow conventional commits** for automated changelog generation
3. **Keep dependencies updated** via Dependabot PRs

## 🔍 Troubleshooting

### **Common Issues**
- **Release fails**: Verify tag format and permissions
- **Build fails**: Check Rust version compatibility (requires 1.83.0+)
- **Test failures**: Ensure Git is properly initialized in test environments

### **Debugging Steps**
1. Check workflow logs in GitHub Actions
2. Run `just setup` to verify local environment
3. Check for platform-specific build issues