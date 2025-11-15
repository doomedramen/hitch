# CI/CD Pipeline Documentation

Hitch is a Git branch management tool for environment-based deployments. This document explains the complete CI/CD pipeline for the Hitch CLI tool.

## 🔄 Workflow Overview

The Hitch project uses a comprehensive CI/CD pipeline with the following workflows:

### **1. CI Pipeline** (`.github/workflows/ci.yml`)
**Triggers**: Push to main/develop, Pull Requests

**Jobs**:
- **Test Suite**: Multi-version Rust testing (stable, beta, 1.70.0)
  - Code formatting checks (`rustfmt`)
  - Linting (`clippy`)
  - Full test suite execution
- **Build Matrix**: Cross-platform build verification
  - Linux (x86_64)
  - macOS (x86_64, ARM64)

### **2. Code Coverage** (`.github/workflows/codecov.yml`)
**Triggers**: Push to main/develop, Pull Requests

**Features**:
- Uses `cargo-llvm-cov` for precise coverage reporting
- Uploads results to [Codecov](https://codecov.io)
- Fails CI if coverage drops significantly
- Generates LCOV format reports

### **3. Security Scanning** (`.github/workflows/security.yml`)
**Triggers**: Push to main, Pull Requests, Daily schedule

**Security Tools**:
- **`cargo audit`**: Security vulnerability scanning
- **`cargo-deny`**: License compliance and security checks
- **Trivy**: Container and dependency vulnerability scanning
- Results uploaded to GitHub Security tab

### **4. Documentation** (`.github/workflows/docs.yml`)
**Triggers**: Push to main/develop, Pull Requests

**Features**:
- Generates Rust documentation
- Auto-deploys to GitHub Pages on main branch
- Includes private items in documentation

### **5. Performance Benchmarks** (`.github/workflows/benchmark.yml`)
**Triggers**: Push to main, Pull Requests

**Features**:
- Uses `cargo-criterion` for performance testing
- Compares against baseline performance
- Creates performance regression alerts
- Stores historical benchmark data

### **6. Release Pipeline** (`.github/workflows/release-dist.yml`)
**Triggers**: Version tags (v1.0.0, etc.)

**Release Process**:
- **Plan Phase**: Analyzes release requirements
- **Build Phase**: Cross-platform binary compilation
- **Create Release**: GitHub release with assets
- **Homebrew**: Automatic formula update

## 🛡️ Security & Compliance

### **Dependabot** (`.github/dependabot.yml`)
- **Rust dependencies**: Weekly automated updates
- **GitHub Actions**: Weekly CI/CD updates
- Auto-assigns to maintainer
- Creates PRs with dependency updates

### **License Compliance** (`deny.toml`)
- **Allowed licenses**: MIT, Apache-2.0, BSD, ISC
- **Denied licenses**: GPL, AGPL
- **Security advisories**: Automated checking
- **Duplicate dependency detection**

## 📊 Quality Gates

### **Pre-commit Hooks** (`lefthook.yml`)
- **Code formatting**: `rustfmt`
- **Linting**: `clippy`
- **Security audit**: `cargo audit`
- **Lock file consistency**: `cargo check --locked`

### **Pre-push Hooks**
- **Full test suite**: `cargo test`
- **Release build**: `cargo build --release`

### **CI Requirements**
- All tests must pass on stable, beta, and minimum Rust versions
- Code must be properly formatted
- No clippy warnings allowed
- Security scans must pass
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
- **Linux**: x86_64 binary
- **macOS**: x86_64 and ARM64 binaries
- **GitHub Release**: With all binaries and checksums
- **Homebrew**: Formula automatically updated
- **Documentation**: Updated and deployed

## 📈 Monitoring & Reporting

### **Coverage Reports**
- **Codecov**: [codecov.io](https://codecov.io)
- **Trend tracking**: Coverage over time
- **PR coverage**: Impact of changes on test coverage

### **Security Monitoring**
- **GitHub Security tab**: Vulnerability reports
- **Dependabot alerts**: Dependency vulnerabilities
- **Daily security scans**: Automated vulnerability detection

### **Performance Monitoring**
- **Criterion benchmarks**: Performance regression detection
- **Historical data**: Performance trends over time
- **Alert thresholds**: 200% performance regression triggers alerts

## 🔧 Configuration Files

| File | Purpose |
|------|---------|
| `.github/workflows/ci.yml` | Main CI pipeline |
| `.github/workflows/codecov.yml` | Code coverage reporting |
| `.github/workflows/security.yml` | Security scanning |
| `.github/workflows/docs.yml` | Documentation generation |
| `.github/workflows/benchmark.yml` | Performance testing |
| `.github/workflows/release-dist.yml` | Release automation |
| `.github/dependabot.yml` | Dependency updates |
| `deny.toml` | License and security compliance |
| `lefthook.yml` | Pre-commit/pre-push hooks |

## 🚨 Required Secrets

To enable full CI/CD functionality, configure these repository secrets:

| Secret | Purpose | Required For |
|--------|---------|--------------|
| `CODECOV_TOKEN` | Codecov upload token | Coverage reporting |
| `HOMEBREW_TAP_GITHUB_TOKEN` | Homebrew formula updates | Release pipeline |
| `GITHUB_TOKEN` | GitHub API access | All workflows (auto-provided) |

## 🔄 Workflow Dependencies

```
Push/PR → CI → Security → Coverage → Docs
    ↓
Release Tag → Release Pipeline → GitHub Release → Homebrew
    ↓
Daily → Security Scan → Dependabot PRs
```

## 📝 Best Practices

1. **Always run `just release` locally** before pushing major changes
2. **Monitor coverage reports** to maintain high test coverage
3. **Review security alerts** promptly
4. **Check benchmark results** for performance regressions
5. **Keep dependencies updated** via Dependabot PRs
6. **Follow conventional commits** for automated changelog generation

## 🔍 Troubleshooting

### **Common Issues**
- **Coverage upload fails**: Check `CODECOV_TOKEN` secret
- **Release fails**: Verify tag format and permissions
- **Security scan fails**: Review deny.toml configuration
- **Build fails**: Check Rust version compatibility

### **Debugging Steps**
1. Check workflow logs in GitHub Actions
2. Run `just setup` to verify local environment
3. Review recent dependency updates
4. Check for platform-specific build issues