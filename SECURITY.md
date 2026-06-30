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

### Approval Workflow Is Advisory, Not an Enforcement Boundary

The approval workflow (`requires_approval`, `approvers`, `min_approvals`) is a
**coordination and audit aid, not a security control**. Be aware that:

- Approver identity is taken from local `git config user.email`, which any user
  can set to any value.
- Approval state is stored in the `hitch-metadata` branch. Anyone with write
  access to the repository can edit and push that branch directly, bypassing the
  workflow entirely.

For real enforcement of "no deploy without N approvals", rely on server-side
controls such as branch protection rules, required reviews, and CI checks on the
remote. Treat Hitch's approval workflow as a way to make intent explicit and
auditable among cooperating teammates, not as a barrier against a malicious or
careless actor with repository write access.

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
