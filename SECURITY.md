# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 1.2.x   | :white_check_mark: |
| < 1.2   | :x:                |

## Reporting a Vulnerability

If you discover a security vulnerability in Hitch, please report it responsibly:

1. **Do not** open a public GitHub issue for security vulnerabilities
2. Email the maintainers directly or use GitHub's private vulnerability reporting feature
3. Include as much detail as possible:
   - Description of the vulnerability
   - Steps to reproduce
   - Potential impact
   - Suggested fix (if any)

## Security Measures

### Pre-commit Hooks

This project uses [lefthook](https://github.com/evilmartians/lefthook) for pre-commit security checks:

- `cargo audit` - Checks for known vulnerabilities in dependencies
- `cargo clippy` - Static analysis for common bugs and security issues

### CI/CD Security

- Security audit runs on every push via GitHub Actions
- Dependencies are regularly updated
- Release binaries are built in isolated CI environments

### Code Security Practices

- Input validation for all user-provided data
- Safe handling of git operations
- No execution of arbitrary shell commands from user input
- Proper error handling without information leakage

## Dependency Security

We monitor dependencies for vulnerabilities using:

- `cargo audit` in pre-commit hooks
- GitHub Dependabot alerts
- Regular dependency updates

To check for vulnerabilities locally:

```bash
cargo audit
```

## Response Timeline

- **Initial Response**: Within 48 hours
- **Assessment**: Within 7 days
- **Fix Timeline**: Depends on severity
  - Critical: 24-48 hours
  - High: 7 days
  - Medium: 30 days
  - Low: Next release cycle
