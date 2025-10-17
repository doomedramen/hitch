# Hitch Examples

This directory contains practical examples and walkthroughs for common Hitch workflows.

## Available Examples

### 1. [Basic Workflow](./01-basic-workflow.md)
Learn the fundamentals: initialize, promote, release, and cleanup.

**What you'll learn:**
- Setting up Hitch
- Creating and promoting features
- Releasing to production
- Cleaning up merged branches

**Time:** 10 minutes

---

### 2. [Multi-Environment Flow](./02-multi-environment.md)
Managing features across dev, qa, and production environments.

**What you'll learn:**
- Progressive promotion (dev → qa → main)
- Environment status checking
- Handling multiple features simultaneously
- Rolling back features from environments

**Time:** 15 minutes

---

### 3. [Team Collaboration](./03-team-collaboration.md)
Working with Hitch in a team setting.

**What you'll learn:**
- Multiple developers working simultaneously
- Lock handling and conflicts
- Resolving merge conflicts
- Communication workflows

**Time:** 20 minutes

---

### 4. [Handling Conflicts](./04-handling-conflicts.md)
Dealing with merge conflicts between features.

**What you'll learn:**
- Identifying conflicting features
- Resolving conflicts through rebasing
- Testing conflict resolution
- Preventing future conflicts

**Time:** 15 minutes

---

### 5. [CI/CD Integration](./05-cicd-integration.md)
Integrating Hitch with CI/CD pipelines.

**What you'll learn:**
- GitHub Actions integration
- GitLab CI integration
- Jenkins integration
- Automated deployment triggers

**Time:** 25 minutes

---

## Quick Reference

### Common Commands

```bash
# Initialize
hitch init

# Promote feature
hitch promote feature/new-login to dev
hitch promote feature/new-login to qa

# Check status
hitch status

# Release to main
hitch release feature/new-login

# Cleanup
hitch cleanup --dry-run
hitch cleanup
```

### Typical Flow

```
1. Developer creates feature branch
2. Developer promotes to dev
3. QA team promotes to qa
4. After testing, release to main
5. Cleanup merged branches
```

## Repository Structure

Each example contains:
- **README** - Step-by-step walkthrough
- **setup.sh** - Script to set up the example repo
- **expected-output/** - Expected command outputs
- **troubleshooting.md** - Common issues and solutions

## Running Examples

### Option 1: Follow the Guide

Read through each example and run commands in your own repository.

### Option 2: Use Example Scripts

Run the setup script to create a sandbox repository:

```bash
cd examples/01-basic-workflow
./setup.sh
cd /tmp/hitch-example-basic-workflow
# Follow the guide...
```

### Option 3: Docker Sandbox

Run examples in a completely isolated Docker environment:

```bash
docker run -it --rm \
  -v $(pwd)/examples:/examples \
  hitchexamples/sandbox:latest \
  /examples/01-basic-workflow/run.sh
```

## Need Help?

- Check [Troubleshooting](#troubleshooting) in each example
- Read the [FAQ](../docs/FAQ.md)
- Open an [Issue](https://github.com/DoomedRamen/hitch/issues)
- Join our [Discussions](https://github.com/DoomedRamen/hitch/discussions)

## Contributing Examples

Have a useful workflow? We'd love to add it!

1. Create a new directory: `examples/XX-your-example/`
2. Follow the structure of existing examples
3. Test thoroughly
4. Submit a PR

See [CONTRIBUTING.md](../CONTRIBUTING.md) for details.
