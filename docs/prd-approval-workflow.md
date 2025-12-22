# PRD: Approval Workflow for Hitch

**Document Status:** Draft
**Author:** Martin
**Created:** 2024-12-17
**Last Updated:** 2024-12-17
**Version:** 1.0

---

## Executive Summary

This PRD defines a new approval workflow system for hitch that enables teams to require approval before promoting or demoting branches to specific environments. The feature allows per-environment configuration of approval requirements, supports multi-approver workflows, and integrates seamlessly with existing hitch operations while maintaining backward compatibility.

**Key Benefits:**
- Prevent unauthorized deployments to critical environments (production, staging)
- Enable code review and approval workflows without external dependencies
- Maintain audit trail of who approved what and when
- Support flexible approval policies (single approver, multi-approver, threshold voting)
- Work with any git provider without requiring API integration

---

## Problem Statement

### Current State

Today, any user with git repository access can promote or demote branches to any environment using hitch. While environment locking prevents concurrent modifications, it does not prevent unauthorized changes. There is no approval or review process before changes are deployed.

**Example scenario:**
```bash
# Any developer can immediately push to production
hitch promote untested-feature production
→ Feature is immediately deployed to production
```

### Problems

1. **Lack of Oversight:** No approval process for critical environment changes
2. **No Review Workflow:** Cannot require code review before deployment
3. **Compliance Issues:** Many organizations require approval for production changes
4. **Accidental Deployments:** Easy to accidentally deploy to wrong environment
5. **No Audit Trail:** Difficult to track who authorized deployments

### User Impact

**Teams affected:**
- Engineering teams needing approval workflows for production deploys
- Organizations with compliance requirements (SOC 2, HIPAA, etc.)
- Teams wanting to enforce review processes without heavy tooling

**Pain points:**
- Manual approval processes (Slack messages, email chains)
- Using external tools for approval (Jira tickets, custom dashboards)
- Risk of unauthorized deployments
- Lack of deployment authorization audit trail

---

## Goals and Objectives

### Primary Goals

1. **Enable approval workflows** for environment promotions/demotions
2. **Maintain simplicity** - no external dependencies or complex setup
3. **Universal compatibility** - work with any git provider
4. **Backward compatibility** - existing workflows unchanged
5. **Clear audit trail** - track all approvals with who/when/what

### Non-Goals (Out of Scope for v1)

1. ❌ Integration with external approval systems (Jira, ServiceNow)
2. ❌ Automated email/Slack notifications (can add later)
3. ❌ Native PR/MR creation via git provider APIs
4. ❌ CI/CD integration and checks
5. ❌ Web UI for viewing approvals
6. ❌ Real-time notifications or webhooks
7. ❌ Role-based access control (RBAC) beyond approver lists

### Success Metrics

**Adoption Metrics:**
- 50%+ of hitch users enable approval workflow within 6 months
- Average setup time < 10 minutes per repository

**Quality Metrics:**
- Zero unauthorized deployments to approval-required environments
- < 5% of approval requests rejected due to conflicts
- 95%+ user satisfaction with approval workflow UX

**Operational Metrics:**
- Average time from approval request to execution < 2 hours
- < 1% approval requests stuck in pending state > 24 hours

---

## User Personas

### Persona 1: Release Manager (Sarah)

**Role:** Senior Engineer, manages production deployments
**Needs:**
- Approve production deployments before they happen
- See what's being deployed (which branches)
- Ensure only tested code reaches production
- Maintain audit trail of approvals

**Pain Points:**
- Currently uses ad-hoc Slack approvals
- No enforcement mechanism
- Hard to track who approved what

**Goals:**
- Formalize approval process
- Reduce deployment anxiety
- Clear audit trail for compliance

### Persona 2: Software Developer (Alex)

**Role:** Mid-level Engineer, deploys features to staging/production
**Needs:**
- Easy way to request deployment approval
- Clear feedback on approval status
- Fast deployment once approved
- Not blocked by complex processes

**Pain Points:**
- Manual approval requests via Slack/email
- Unclear when approval is granted
- Afraid of accidentally deploying to production

**Goals:**
- Simple deployment workflow
- Quick feedback on approvals
- Confidence in deployment process

### Persona 3: DevOps Engineer (Jordan)

**Role:** Platform Engineer, maintains deployment infrastructure
**Needs:**
- Configure approval policies per environment
- Enforce approval requirements technically (not just policy)
- Audit who can approve what
- Troubleshoot stuck approvals

**Pain Points:**
- No technical enforcement of approval policies
- Relies on git provider branch protection alone
- Hard to audit approval compliance

**Goals:**
- Technical enforcement of policies
- Easy configuration management
- Clear audit and troubleshooting tools

---

## User Stories

### Epic 1: Basic Approval Workflow

**US-1.1: Request Approval as Developer**
```
As a developer,
I want to request approval when promoting to production,
So that the release manager can review before deployment.

Acceptance Criteria:
- Running `hitch promote <branch> production` creates approval request
- System displays approval request ID
- System shows who can approve
- No immediate deployment occurs
- User can check approval status
```

**US-1.2: Approve Request as Release Manager**
```
As a release manager,
I want to approve pending deployment requests,
So that approved code can be deployed.

Acceptance Criteria:
- Can list all pending approval requests
- Can view details of specific request (branch, requester, environment)
- Can approve request with optional comment
- System executes deployment after approval
- User receives confirmation of approval and deployment
```

**US-1.3: View Approval Status as Developer**
```
As a developer,
I want to see the status of my approval request,
So that I know when deployment will happen.

Acceptance Criteria:
- Can check status by request ID
- Shows who has approved so far
- Shows who still needs to approve
- Shows if request is approved, pending, or rejected
- Displays any comments from approvers
```

### Epic 2: Configuration and Setup

**US-2.1: Configure Environment Approval Requirements**
```
As a DevOps engineer,
I want to configure which environments require approval,
So that I can enforce approval policies technically.

Acceptance Criteria:
- Can mark environment as requires_approval: true in config
- Can specify list of approver emails
- Can set minimum number of approvals required
- Configuration stored in hitch.json
- Changes take effect immediately
```

**US-2.2: Backward Compatibility for Existing Environments**
```
As a DevOps engineer,
I want existing environments to work without changes,
So that upgrading hitch doesn't break workflows.

Acceptance Criteria:
- Environments without requires_approval field work as before
- No approval required by default
- Existing promote/demote commands unchanged
- Can opt-in per environment gradually
```

### Epic 3: Multi-Approval Support

**US-3.1: Require Multiple Approvals**
```
As a release manager,
I want to require multiple approvals for production,
So that critical deployments have consensus.

Acceptance Criteria:
- Can configure min_approvals: N in environment config
- System tracks each approval separately
- Deployment only happens after N approvals
- Shows progress (e.g., "2 of 3 approvals")
- Prevents duplicate approvals from same user
```

**US-3.2: Prevent Self-Approval**
```
As a compliance officer,
I want to prevent users from approving their own requests,
So that we maintain separation of duties.

Acceptance Criteria:
- System checks if approver == requester
- Blocks approval if same user
- Shows clear error message
- Works across multiple approvals
```

### Epic 4: Security and Authorization

**US-4.1: Authorize Approvers**
```
As a DevOps engineer,
I want to restrict who can approve deployments,
So that only authorized people can deploy.

Acceptance Criteria:
- System checks approver against approvers list
- Rejects approval if user not in list
- Uses git config user.email for identity
- Shows clear authorization error
- Can have different approvers per environment
```

**US-4.2: Prevent Bypassing Approvals**
```
As a security engineer,
I want to ensure approvals cannot be bypassed,
So that unauthorized deployments are impossible.

Acceptance Criteria:
- Promote/demote don't execute when approval required
- Only approval commands can execute deployment
- Git provider branch protection enforces at server level
- hitch guard warns about direct commits
- Documentation explains security model
```

### Epic 5: Approval Management

**US-5.1: Reject Approval Request**
```
As a release manager,
I want to reject deployment requests,
So that problematic changes aren't deployed.

Acceptance Criteria:
- Can reject request with required reason comment
- Request moves to Rejected status
- Deployment does not occur
- Requester can see rejection reason
- Can create new request after addressing issues
```

**US-5.2: Cancel Approval Request**
```
As a developer,
I want to cancel my pending approval request,
So that I can fix issues before resubmitting.

Acceptance Criteria:
- Only requester can cancel their own request
- Request moves to Cancelled status
- Shows in approval history
- Can create new request immediately
- Approvers notified (via status change)
```

**US-5.3: List and Filter Approvals**
```
As a release manager,
I want to see all approval requests,
So that I can manage pending approvals.

Acceptance Criteria:
- Lists all pending approvals by default
- Can filter by environment
- Can filter by status (pending, approved, rejected)
- Shows key metadata (requester, date, branch)
- Sorted by request date (newest first)
```

### Epic 6: Snapshot Protection

**US-6.1: Capture State at Request Time**
```
As a release manager,
I want to approve the exact code state,
So that I know what I'm approving hasn't changed.

Acceptance Criteria:
- System captures branch commit SHAs when request created
- Stores snapshot in approval request
- Validates snapshot before applying approval
- Shows what branches and SHAs are included
```

**US-6.2: Detect Stale Approvals**
```
As a release manager,
I want to be notified if approved code has changed,
So that I can review changes again.

Acceptance Criteria:
- System validates SHAs before executing deployment
- Fails if any branch SHA changed since request
- Shows clear error with what changed
- Requires new approval request
- Original approval preserved for audit
```

### Epic 7: User Experience

**US-7.1: Clear Command Feedback**
```
As a developer,
I want clear feedback from hitch commands,
So that I understand what happened and what to do next.

Acceptance Criteria:
- Success messages include next steps
- Error messages include resolution steps
- Shows helpful context (who can approve, how many approvals needed)
- Consistent formatting across all approval commands
```

**US-7.2: Helpful Guard Messages**
```
As a developer,
I want hitch guard to explain approval workflow,
So that I understand why I can't commit and what to do instead.

Acceptance Criteria:
- Guard detects approval-required environments
- Shows custom message explaining approval workflow
- Provides example commands to use
- Links to documentation
- Friendly, educational tone (not just blocking)
```

---

## Functional Requirements

### FR-1: Configuration Schema

**FR-1.1: Environment Configuration**

Extend `Environment` struct in `hitch.json`:

```json
{
  "base": "main",
  "branches": ["feature-a"],
  "locked": false,
  "locked_by": null,
  "locked_at": null,
  "rebuilt_at": "2024-12-17T10:30:00Z",
  "released_at": null,

  // NEW FIELDS
  "requires_approval": true,
  "approvers": [
    "alice@company.com",
    "bob@company.com",
    "charlie@company.com"
  ],
  "min_approvals": 2
}
```

**Field Specifications:**

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `requires_approval` | boolean | No | `false` | Whether environment requires approval |
| `approvers` | string[] | Conditional* | `[]` | List of email addresses who can approve |
| `min_approvals` | integer | No | `1` | Minimum number of approvals required |

*Required if `requires_approval` is `true`

**Validation Rules:**
- If `requires_approval` is `true`, `approvers` must not be empty
- `min_approvals` must be ≥ 1 and ≤ length of `approvers`
- Approver emails must be valid email format
- Approver list must not contain duplicates

**FR-1.2: Approval Request Schema**

Add `approval_requests` array to `hitch.json`:

```json
{
  "version": "1.0",
  "environments": { ... },
  "approval_requests": [
    {
      "id": "a1b2c3d4-5678-90ab-cdef-1234567890ab",
      "environment": "production",
      "branch": "feature-x",
      "operation": "Promote",
      "requested_by": "developer@company.com",
      "requested_at": "2024-12-17T10:30:00Z",
      "status": "Pending",
      "approvals": [
        {
          "approved_by": "alice@company.com",
          "approved_at": "2024-12-17T11:00:00Z",
          "comment": "LGTM, tests passed"
        }
      ],
      "rejection": null,
      "rebuild_snapshot": {
        "base_sha": "abc123def456789",
        "branch_shas": {
          "feature-a": "def456ghi789012",
          "feature-x": "ghi789jkl012345"
        },
        "merge_conflicts": false
      }
    }
  ]
}
```

**Field Specifications:**

| Field | Type | Description |
|-------|------|-------------|
| `id` | string (UUID) | Unique identifier for approval request |
| `environment` | string | Target environment name |
| `branch` | string | Branch being promoted/demoted |
| `operation` | enum | "Promote" or "Demote" |
| `requested_by` | string (email) | Who requested the operation |
| `requested_at` | ISO 8601 datetime | When request was created |
| `status` | enum | "Pending", "Approved", "Applied", "Rejected", "Cancelled" |
| `approvals` | array | List of approval records |
| `rejection` | object | Rejection details (if rejected) |
| `rebuild_snapshot` | object | State snapshot at request time |

**Approval Record Schema:**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `approved_by` | string (email) | Yes | Who gave approval |
| `approved_at` | ISO 8601 datetime | Yes | When approval was given |
| `comment` | string | No | Optional approval comment |

**Rejection Record Schema:**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `rejected_by` | string (email) | Yes | Who rejected |
| `rejected_at` | ISO 8601 datetime | Yes | When rejection occurred |
| `reason` | string | Yes | Required rejection reason |

**Rebuild Snapshot Schema:**

| Field | Type | Description |
|-------|------|-------------|
| `base_sha` | string | Commit SHA of base branch |
| `branch_shas` | map<string, string> | Map of branch name → commit SHA |
| `merge_conflicts` | boolean | Whether conflicts detected at request time |

### FR-2: Command Modifications

**FR-2.1: Modified `hitch promote` Behavior**

**Current behavior:**
```bash
hitch promote <branch> <environment>
→ Immediately promotes branch
```

**New behavior (when approval required):**
```bash
hitch promote <branch> <environment>
→ Creates approval request
→ Does NOT execute promotion
→ Returns request ID and approval info
```

**Implementation Logic:**
```rust
if environment.requires_approval {
    // Create approval request
    let request_id = create_approval_request(/* ... */)?;

    // Display info
    println!("✓ Approval request created: {}", request_id);
    println!("  Environment: {}", environment_name);
    println!("  Branch: {}", branch_name);
    println!("  Operation: Promote");
    println!();
    println!("Waiting for {} approval(s) from:", min_approvals);
    for approver in &approvers {
        println!("  - {}", approver);
    }
    println!();
    println!("Check status: hitch approvals status {}", request_id);

    return Ok(()); // Exit without executing promotion
}

// Otherwise, execute immediately as before
execute_promotion(/* ... */)?;
```

**Exit codes:**
- `0` - Success (request created or immediate promotion completed)
- `1` - Error (validation failed, conflicts detected, etc.)

**FR-2.2: Modified `hitch demote` Behavior**

Same logic as promote, but for demotion operations.

### FR-3: New Commands

**FR-3.1: `hitch approvals list`**

**Purpose:** List approval requests

**Usage:**
```bash
hitch approvals list [OPTIONS]

Options:
  --all              Show all requests (default: pending only)
  --environment <env>  Filter by environment
  --status <status>    Filter by status (pending|approved|applied|rejected|cancelled)
  --format <format>    Output format (table|json) [default: table]
```

**Output Format (table):**
```
ID       Environment  Branch      Requester              Status    Created              Approvals
a1b2c3d4 production   feature-x   dev@company.com        Pending   2024-12-17 10:30:00  1/2
e5f6g7h8 staging      feature-y   another@company.com    Applied   2024-12-17 09:15:00  1/1
```

**Output Format (json):**
```json
[
  {
    "id": "a1b2c3d4",
    "environment": "production",
    "branch": "feature-x",
    "operation": "Promote",
    "requested_by": "dev@company.com",
    "status": "Pending",
    "requested_at": "2024-12-17T10:30:00Z",
    "approvals_count": 1,
    "approvals_required": 2
  }
]
```

**Exit codes:**
- `0` - Success
- `1` - Error (invalid filter, cannot read metadata, etc.)

**FR-3.2: `hitch approvals status <request-id>`**

**Purpose:** Show detailed status of specific approval request

**Usage:**
```bash
hitch approvals status <request-id>
```

**Output:**
```
Approval Request: a1b2c3d4-5678-90ab-cdef-1234567890ab

Environment: production
Branch: feature-x
Operation: Promote
Status: Pending

Requested by: developer@company.com
Requested at: 2024-12-17 10:30:00 UTC

Approvals (1/2):
  ✓ alice@company.com - 2024-12-17 11:00:00 UTC
    "LGTM, tests passed"

  ⏳ Waiting for 1 more approval from:
    - bob@company.com
    - charlie@company.com

Snapshot:
  Base branch: main (abc123d)
  Branches to merge:
    - feature-a (def456g)
    - feature-x (ghi789j)
  Merge conflicts: No
```

**Exit codes:**
- `0` - Success
- `1` - Request ID not found
- `1` - Cannot read metadata

**FR-3.3: `hitch approvals approve <request-id>`**

**Purpose:** Approve a pending request

**Usage:**
```bash
hitch approvals approve <request-id> [OPTIONS]

Options:
  --comment <text>  Optional approval comment
```

**Behavior:**
1. Fetch approval request from metadata
2. Validate request status is "Pending"
3. Get current user from `git config user.email`
4. Validate user is in approvers list
5. Validate user != requester (no self-approval)
6. Validate user hasn't already approved
7. Add approval record
8. Check if min_approvals threshold met
9. If threshold met:
   - Update status to "Approved"
   - Validate snapshot (branch SHAs unchanged)
   - Execute promotion/demotion
   - Update status to "Applied"
10. Commit and push metadata

**Output (threshold not yet met):**
```
✓ Approval recorded for request a1b2c3d4

Approvals: 1/2
Waiting for 1 more approval from:
  - bob@company.com
  - charlie@company.com
```

**Output (threshold met, promotion executed):**
```
✓ Approval recorded for request a1b2c3d4

Approvals: 2/2 - Threshold met!

Executing promotion...
  ✓ Locked environment 'production'
  ✓ Updated environment branches
  ✓ Rebuilt environment branch
  ✓ Pushed to origin/production
  ✓ Unlocked environment

✓ Promotion complete!
```

**Error Cases:**
- Request not found
- Request not in Pending status
- User not in approvers list
- User is the requester (self-approval)
- User already approved
- Snapshot validation failed (branches changed)
- Promotion execution failed

**Exit codes:**
- `0` - Success (approval recorded or promotion completed)
- `1` - Error (authorization failed, validation failed, etc.)

**FR-3.4: `hitch approvals reject <request-id>`**

**Purpose:** Reject a pending request

**Usage:**
```bash
hitch approvals reject <request-id> --reason <text>

Options:
  --reason <text>  Required rejection reason
```

**Behavior:**
1. Fetch approval request
2. Validate request status is "Pending"
3. Get current user from `git config user.email`
4. Validate user is in approvers list
5. Require --reason flag
6. Update status to "Rejected"
7. Add rejection record with user, timestamp, reason
8. Commit and push metadata

**Output:**
```
✓ Request a1b2c3d4 rejected

Reason: "Needs more testing in staging first"
Rejected by: alice@company.com
```

**Exit codes:**
- `0` - Success
- `1` - Error (authorization failed, missing reason, etc.)

**FR-3.5: `hitch approvals cancel <request-id>`**

**Purpose:** Cancel your own approval request

**Usage:**
```bash
hitch approvals cancel <request-id>
```

**Behavior:**
1. Fetch approval request
2. Validate request status is "Pending"
3. Get current user from `git config user.email`
4. Validate user is the requester
5. Update status to "Cancelled"
6. Commit and push metadata

**Output:**
```
✓ Request a1b2c3d4 cancelled

You can create a new approval request when ready:
  hitch promote <branch> <environment>
```

**Exit codes:**
- `0` - Success
- `1` - Error (not your request, invalid status, etc.)

### FR-4: Enhanced `hitch guard`

**FR-4.1: Approval-Aware Guard Messages**

**Current behavior:**
```bash
hitch guard
→ Blocks commits to environment branches
→ Generic error message
```

**New behavior:**
```bash
hitch guard
→ Detects if environment requires approval
→ Shows approval-specific guidance
```

**Output (approval required):**
```
✗ Error: Cannot commit directly to environment branch 'production'

This environment requires approval for changes.

To promote a branch to production:
  1. Commit your changes on a feature branch
  2. Run: hitch promote <your-branch> production
  3. Wait for approval from:
       - alice@company.com
       - bob@company.com
     (2 approvals required)

Learn more: hitch help promote
```

**Output (no approval required):**
```
✗ Error: Cannot commit directly to environment branch 'staging'

Environment branches are managed by hitch.
To promote a branch to staging:
  hitch promote <your-branch> staging

Learn more: hitch help promote
```

### FR-5: Snapshot Protection

**FR-5.1: Capture Snapshot at Request Time**

When creating approval request:
1. Get current SHA of base branch
2. Get current SHA of each promoted branch
3. Get current SHA of branch being promoted
4. Perform test merge to detect conflicts
5. Store all SHAs in `rebuild_snapshot`

**FR-5.2: Validate Snapshot Before Applying**

When approval threshold met, before executing promotion:
1. Re-fetch all branches
2. Get current SHAs
3. Compare with stored snapshot SHAs
4. If any SHA changed:
   - Abort operation
   - Update status to "Stale"
   - Show clear error with details
   - Require new approval request

**Error Output:**
```
✗ Cannot apply approval: snapshot is stale

The following branches have changed since approval was requested:

  Branch      Expected SHA  Current SHA
  feature-x   ghi789j       xyz123a
  main        abc123d       bcd234e

A new approval request is required.
Old request ID: a1b2c3d4 (preserved for audit)

Create new request: hitch promote feature-x production
```

### FR-6: Authorization

**FR-6.1: Approver Validation**

On `hitch approvals approve`:
1. Get current user from `git config user.email`
2. Load environment configuration
3. Check if user email in `approvers` list
4. If not, reject with error

**Error Output:**
```
✗ Authorization failed

You (user@company.com) are not authorized to approve changes to 'production'.

Authorized approvers:
  - alice@company.com
  - bob@company.com
  - charlie@company.com

Contact your release manager if you should have approval access.
```

**FR-6.2: Self-Approval Prevention**

On `hitch approvals approve`:
1. Compare current user with `requested_by` field
2. If same, reject with error

**Error Output:**
```
✗ Self-approval not allowed

You cannot approve your own deployment request.
An authorized approver must review and approve this change.
```

**FR-6.3: Duplicate Approval Prevention**

On `hitch approvals approve`:
1. Check if user already in `approvals` array
2. If yes, reject with error

**Error Output:**
```
✗ Already approved

You (alice@company.com) have already approved this request.

Approvals: 1/2
Waiting for 1 more approval from:
  - bob@company.com
  - charlie@company.com
```

---

## Non-Functional Requirements

### NFR-1: Performance

**NFR-1.1: Command Response Time**
- `hitch approvals list` completes in < 2 seconds for up to 1000 approval requests
- `hitch approvals approve` completes in < 5 seconds (excluding promotion execution time)
- Approval request creation adds < 1 second to promote/demote commands

**NFR-1.2: Metadata Size**
- Single approval request < 5 KB
- System warns if approval_requests array exceeds 10 MB
- Automatic cleanup removes requests older than 90 days

### NFR-2: Reliability

**NFR-2.1: Data Integrity**
- All approval data stored in git commits (ACID guarantees)
- No data loss on system crash or network interruption
- Approval state always recoverable from git history

**NFR-2.2: Concurrent Operations**
- Multiple users can create approval requests simultaneously (git handles conflicts)
- Concurrent approvals handled gracefully (one succeeds, others retry)
- Lock mechanism during promotion execution prevents race conditions

**NFR-2.3: Failure Handling**
- If approval threshold met but promotion fails, status stays "Approved" (can retry)
- Network failures during push don't lose approval data (committed locally)
- Clear error messages for all failure scenarios

### NFR-3: Usability

**NFR-3.1: Command Discoverability**
- `hitch --help` shows approvals subcommand
- `hitch approvals --help` shows all approval commands
- Error messages include next-step suggestions

**NFR-3.2: Output Clarity**
- Table output aligned and readable
- Status indicators clear (✓ ✗ ⏳)
- Timestamps in local timezone
- Colors used for emphasis (if terminal supports)

**NFR-3.3: Documentation**
- All commands documented in README
- Examples for common workflows
- Troubleshooting guide for common errors
- Migration guide for existing users

### NFR-4: Security

**NFR-4.1: Authorization**
- Only listed approvers can approve
- Git committer email used for identity
- Self-approval prevented
- No privilege escalation possible

**NFR-4.2: Audit Trail**
- All approvals recorded with timestamp and approver
- Approval history preserved in git log
- Cannot delete or modify past approvals
- Can reconstruct full approval history from git

**NFR-4.3: Data Privacy**
- No sensitive data in approval records
- Email addresses visible (required for authorization)
- Comments visible to all users
- No encryption needed (public repository data)

### NFR-5: Compatibility

**NFR-5.1: Backward Compatibility**
- Existing hitch configurations work without changes
- No approval required if `requires_approval` not set
- Can mix approval-required and non-approval environments
- Upgrade path preserves all existing data

**NFR-5.2: Git Provider Compatibility**
- Works with any git hosting (GitHub, GitLab, Bitbucket, self-hosted)
- No provider-specific APIs required
- Standard git operations only
- Works with local-only repositories (no remote)

**NFR-5.3: Platform Compatibility**
- Works on Linux, macOS, Windows
- Same behavior across all platforms
- No platform-specific dependencies

### NFR-6: Maintainability

**NFR-6.1: Code Quality**
- Approval code follows existing hitch patterns
- Comprehensive unit tests (> 80% coverage)
- Integration tests for full workflows
- Clear error messages with context

**NFR-6.2: Extensibility**
- Easy to add new approval statuses
- Easy to add new authorization mechanisms
- Easy to add notification integrations later
- Modular command structure

---

## User Experience

### UX-1: Command-Line Interface

**Design Principles:**
1. **Consistent:** Follow existing hitch command patterns
2. **Progressive:** Simple default behavior, advanced options available
3. **Informative:** Always explain what's happening and why
4. **Recoverable:** Clear path forward on errors
5. **Efficient:** Minimal keystrokes for common tasks

**Color Scheme:**
- ✓ Green for success
- ✗ Red for errors
- ⏳ Yellow for pending/waiting
- Blue for informational messages
- Gray for timestamps and metadata

### UX-2: Error Messages

**Error Message Structure:**
```
✗ [Clear problem statement]

[Context: what was being attempted]

[Resolution: what to do next]
```

**Examples:**

```
✗ Authorization failed

You (user@company.com) are not authorized to approve changes to 'production'.

Resolution:
  - Contact release manager for approval access
  - Authorized approvers: alice@company.com, bob@company.com
```

```
✗ Snapshot validation failed

The following branches changed since approval was requested:
  - feature-x: ghi789j → xyz123a (force-pushed)
  - main: abc123d → bcd234e (new commits)

Resolution:
  - Create new approval request: hitch promote feature-x production
  - Original request preserved for audit: a1b2c3d4
```

### UX-3: Success Messages

**Success Message Structure:**
```
✓ [What succeeded]

[Next steps or additional info]
```

**Examples:**

```
✓ Approval request created: a1b2c3d4

Environment: production
Branch: feature-x
Operation: Promote

Waiting for 2 approval(s) from:
  - alice@company.com
  - bob@company.com

Check status: hitch approvals status a1b2c3d4
```

```
✓ Approval recorded (2/2 - Threshold met!)

Executing promotion...
  ✓ Locked environment 'production'
  ✓ Rebuilt environment branch
  ✓ Pushed to origin/production
  ✓ Unlocked environment

✓ Promotion complete!
```

### UX-4: Interactive Flows

**FR-4.1: Approval Request Creation**

```
$ hitch promote feature-x production

Checking environment configuration...
  ✓ Environment 'production' exists
  ✓ Branch 'feature-x' is valid
  ✓ No merge conflicts detected

This environment requires approval.

Creating approval request...
  ✓ Captured branch snapshots
  ✓ Created request: a1b2c3d4
  ✓ Pushed to hitch-metadata

Approval request created: a1b2c3d4

Environment: production
Branch: feature-x
Operation: Promote

Waiting for 2 approval(s) from:
  - alice@company.com
  - bob@company.com

Next steps:
  - Notify approvers of pending request
  - Check status: hitch approvals status a1b2c3d4
  - View all requests: hitch approvals list
```

**FR-4.2: Approval Execution**

```
$ hitch approvals approve a1b2c3d4 --comment "LGTM"

Fetching approval request...
  ✓ Request found: a1b2c3d4

Validating authorization...
  ✓ You (bob@company.com) are authorized
  ✓ Not a self-approval
  ✓ No duplicate approval

Recording approval...
  ✓ Approval recorded

Approvals: 2/2 - Threshold met!

Validating snapshot...
  ✓ Base branch unchanged (main: abc123d)
  ✓ All branches unchanged
  ✓ No new conflicts

Executing promotion...
  ⏳ Locking environment 'production'...
  ✓ Environment locked

  ⏳ Updating environment metadata...
  ✓ Branch added to environment

  ⏳ Rebuilding environment branch...
  ✓ Created temp branch
  ✓ Merged feature-x (squash)
  ✓ Replaced production branch

  ⏳ Pushing to origin...
  Do you want to force push to origin/production? [y/N]: y
  ✓ Pushed to origin/production

  ⏳ Unlocking environment...
  ✓ Environment unlocked

✓ Promotion complete!

Branch 'feature-x' is now promoted to 'production'.
```

---

## Technical Architecture

### Architecture Overview

**Core Principle:** Extend existing hitch-metadata pattern to store approval state

**Components:**
1. **Approval Request Manager** - Create, read, update approval requests
2. **Authorization Service** - Validate approver permissions
3. **Snapshot Manager** - Capture and validate branch states
4. **Approval Commands** - CLI interface for approval operations
5. **Modified Promote/Demote** - Integration with approval workflow

**Data Flow:**

```
Request Creation:
User → promote/demote → Check requires_approval → Create approval request →
Push metadata → Return request ID

Approval Flow:
Approver → approve command → Validate authorization → Add approval →
Check threshold → Validate snapshot → Execute promotion → Update status →
Push metadata

Status Check:
User → status/list command → Read metadata → Display formatted output
```

### File Structure

```
src/
├── types.rs
│   ├── Environment (add approval fields)
│   ├── ApprovalRequest (new)
│   ├── Approval (new)
│   ├── ApprovalStatus (new)
│   ├── RebuildSnapshot (new)
│   └── Operation (new)
│
├── commands/
│   ├── promote.rs (modify for approval check)
│   ├── demote.rs (modify for approval check)
│   ├── guard.rs (enhance with approval messages)
│   │
│   └── approvals/
│       ├── mod.rs (approvals subcommand router)
│       ├── list.rs (list command)
│       ├── status.rs (status command)
│       ├── approve.rs (approve command)
│       ├── reject.rs (reject command)
│       └── cancel.rs (cancel command)
│
├── utils/
│   ├── prelude.rs (add approval helpers)
│   ├── approvals.rs (new - approval business logic)
│   ├── authorization.rs (new - permission checks)
│   └── snapshot.rs (new - snapshot capture/validation)
│
└── main.rs (register approvals subcommand)
```

### Key Algorithms

**Algorithm 1: Create Approval Request**

```rust
fn create_approval_request(
    context: &GlobalContext,
    env_name: &str,
    branch: &str,
    operation: Operation
) -> Result<String> {
    // 1. Generate UUID
    let request_id = Uuid::new_v4().to_string();

    // 2. Get requester email
    let requested_by = context.git().get_user_email()?;

    // 3. Capture snapshot
    let snapshot = capture_rebuild_snapshot(context, env_name, branch)?;

    // 4. Create request object
    let request = ApprovalRequest {
        id: request_id.clone(),
        environment: env_name.to_string(),
        branch: branch.to_string(),
        operation,
        requested_by,
        requested_at: Utc::now(),
        status: ApprovalStatus::Pending,
        approvals: vec![],
        rejection: None,
        rebuild_snapshot: snapshot,
    };

    // 5. Add to metadata
    modify_metadata(context, |config| {
        config.approval_requests.push(request);
        Ok(())
    })?;

    Ok(request_id)
}
```

**Algorithm 2: Approve Request**

```rust
fn approve_request(
    context: &GlobalContext,
    request_id: &str,
    comment: Option<String>
) -> Result<bool> {
    let current_user = context.git().get_user_email()?;
    let threshold_met = false;

    modify_metadata(context, |config| {
        // Find request
        let request = config.approval_requests.iter_mut()
            .find(|r| r.id == request_id)
            .ok_or_else(|| anyhow!("Request not found"))?;

        // Validate status
        if request.status != ApprovalStatus::Pending {
            bail!("Request is not pending");
        }

        // Get environment config
        let env = config.environments.get(&request.environment)
            .ok_or_else(|| anyhow!("Environment not found"))?;

        // Authorize
        authorize_approver(&current_user, env, &request.requested_by, request)?;

        // Add approval
        request.approvals.push(Approval {
            approved_by: current_user,
            approved_at: Utc::now(),
            comment,
        });

        // Check threshold
        let min_approvals = env.min_approvals.unwrap_or(1);
        threshold_met = request.approvals.len() >= min_approvals;

        if threshold_met {
            request.status = ApprovalStatus::Approved;
        }

        Ok(())
    })?;

    Ok(threshold_met)
}
```

**Algorithm 3: Validate Snapshot**

```rust
fn validate_snapshot(
    context: &GlobalContext,
    snapshot: &RebuildSnapshot
) -> Result<()> {
    // Validate base branch
    let current_base_sha = context.git()
        .get_branch_commit_sha(&snapshot.base_branch)?;

    if current_base_sha != snapshot.base_sha {
        bail!("Base branch has changed: {} → {}",
            &snapshot.base_sha[..7],
            &current_base_sha[..7]
        );
    }

    // Validate all promoted branches
    for (branch, expected_sha) in &snapshot.branch_shas {
        let current_sha = context.git()
            .get_branch_commit_sha(branch)?;

        if current_sha != *expected_sha {
            bail!("Branch '{}' has changed: {} → {}",
                branch,
                &expected_sha[..7],
                &current_sha[..7]
            );
        }
    }

    Ok(())
}
```

### Data Structures

**Rust Types:**

```rust
// In src/types.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Environment {
    pub base: String,
    pub branches: Vec<String>,
    pub locked: bool,
    pub locked_by: Option<String>,
    pub locked_at: Option<DateTime<Utc>>,
    pub rebuilt_at: Option<DateTime<Utc>>,
    pub released_at: Option<DateTime<Utc>>,

    // New fields
    #[serde(default)]
    pub requires_approval: bool,

    #[serde(default)]
    pub approvers: Vec<String>,

    #[serde(default = "default_min_approvals")]
    pub min_approvals: usize,
}

fn default_min_approvals() -> usize { 1 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HitchConfig {
    pub version: String,
    pub environments: HashMap<String, Environment>,

    // New field
    #[serde(default)]
    pub approval_requests: Vec<ApprovalRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub id: String,
    pub environment: String,
    pub branch: String,
    pub operation: Operation,
    pub requested_by: String,
    pub requested_at: DateTime<Utc>,
    pub status: ApprovalStatus,
    pub approvals: Vec<Approval>,
    pub rejection: Option<Rejection>,
    pub rebuild_snapshot: RebuildSnapshot,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Operation {
    Promote,
    Demote,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ApprovalStatus {
    Pending,
    Approved,
    Applied,
    Rejected,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Approval {
    pub approved_by: String,
    pub approved_at: DateTime<Utc>,
    pub comment: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rejection {
    pub rejected_by: String,
    pub rejected_at: DateTime<Utc>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RebuildSnapshot {
    pub base_sha: String,
    pub branch_shas: HashMap<String, String>,
    pub merge_conflicts: bool,
}
```

### Security Model

**Identity:**
- Uses `git config user.email` for user identity
- Same as existing locking mechanism
- Email can be spoofed locally, but:
  - Git commits show real committer (audit trail)
  - Git provider enforces authentication
  - Branch protection is primary security layer

**Authorization:**
- Per-environment approver list
- Checked on every approval operation
- Self-approval prevented
- Duplicate approval prevented

**Enforcement:**
1. **Primary:** Git provider branch protection (server-enforced)
2. **Secondary:** Hitch approval workflow (requires approval before creating deployable state)
3. **Tertiary:** hitch guard (user guidance, pre-commit hook)

**Trust Model:**
- Trust git provider to authenticate users
- Trust git provider to enforce branch protection
- Trust hitch to manage approval workflow
- Trust approvers to make good decisions
- Do NOT trust local git config for security (only for identity)

**Attack Vectors & Mitigations:**

| Attack | Mitigation |
|--------|-----------|
| Bypass hitch, push directly | Git provider branch protection blocks |
| Spoof git config email | Git commit shows real author, audit trail preserved |
| Modify hitch.json manually | Git provider branch protection prevents push |
| Use --no-push and push manually | Approval check prevents local state creation, branch protection blocks push |
| Force push to environment branch | Branch protection blocks force push |
| Delete approval requests | Git history preserves all approvals |
| Self-approve | Hitch checks requester != approver |
| Approve twice | Hitch checks for duplicate approvals |

### Edge Cases

**EC-1: Concurrent Approval Requests**

**Scenario:** Two users create approval requests for same environment simultaneously

**Resolution:**
- Both requests created with unique IDs
- Both stored in metadata (may cause merge conflict)
- Git handles conflict resolution
- Both requests can proceed independently

**EC-2: Concurrent Approvals**

**Scenario:** Two approvers approve same request simultaneously

**Resolution:**
- Both fetch metadata
- Both add their approval
- Both commit to hitch-metadata
- One push succeeds, one fails (git conflict)
- Failed approver sees "push rejected"
- Failed approver re-runs command (idempotent)
- Second attempt sees approval already recorded

**EC-3: Branch Changed During Approval**

**Scenario:** Branch force-pushed while approval pending

**Resolution:**
- Snapshot validation detects SHA mismatch
- Approval execution fails with clear error
- Requires new approval request
- Old approval preserved for audit

**EC-4: Environment Configuration Changed**

**Scenario:** Approvers list changed while approval pending

**Resolution:**
- Use approver list from current config (not snapshot)
- If approver removed from list, their existing approval still counts
- New approvers can approve pending requests
- Min_approvals threshold uses current config value

**EC-5: Approval Request for Non-Existent Branch**

**Scenario:** Branch deleted after approval request created

**Resolution:**
- Snapshot validation detects missing branch
- Approval execution fails
- Request can be cancelled or rejected
- No harmful state created

**EC-6: Environment Deleted**

**Scenario:** Environment removed from config while approval pending

**Resolution:**
- Approval commands fail with "environment not found"
- Request remains in metadata (historical record)
- Can be cleaned up manually or via cleanup command

**EC-7: Very Large Approval Request List**

**Scenario:** Thousands of approval requests in metadata

**Resolution:**
- List command supports pagination/filtering
- Cleanup command removes old requests (> 90 days)
- Performance warning if array > 10 MB
- JSON parsing still efficient for reasonable sizes (< 10K requests)

---

## Implementation Plan

### Phase 1: Foundation (Week 1-2)

**Goals:**
- Core data structures
- Basic approval workflow
- Single approver support

**Tasks:**

**Week 1:**
1. Add data structures to `src/types.rs` ✓
   - Environment approval fields
   - ApprovalRequest, Approval, ApprovalStatus, Operation, RebuildSnapshot
   - Serialization/deserialization
   - Unit tests for serialization

2. Create approval utility modules ✓
   - `src/utils/approvals.rs` - CRUD operations
   - `src/utils/authorization.rs` - Permission checks
   - `src/utils/snapshot.rs` - Snapshot capture/validation
   - Unit tests for each module

3. Modify promote/demote commands ✓
   - Check requires_approval
   - Create approval request instead of immediate execution
   - Update output messages
   - Integration tests

**Week 2:**
4. Create approvals subcommand structure ✓
   - `src/commands/approvals/mod.rs`
   - Register in main.rs
   - Help text and documentation

5. Implement list command ✓
   - `src/commands/approvals/list.rs`
   - Table formatting
   - Filtering options
   - Tests

6. Implement approve command (single approver) ✓
   - `src/commands/approvals/approve.rs`
   - Authorization check
   - Execute promotion when approved
   - Tests

7. Implement status command ✓
   - `src/commands/approvals/status.rs`
   - Detailed output formatting
   - Tests

**Deliverables:**
- Working approval workflow for single approver
- All commands functional
- Unit and integration tests passing
- Basic documentation

### Phase 2: Advanced Features (Week 3)

**Goals:**
- Multi-approval support
- Snapshot validation
- Reject and cancel commands

**Tasks:**

1. Multi-approval support ✓
   - Threshold checking
   - Progress display (X/Y approvals)
   - Self-approval prevention
   - Duplicate approval prevention
   - Tests

2. Snapshot validation ✓
   - Validate before execution
   - Clear error messages
   - Handle stale approvals
   - Tests

3. Implement reject command ✓
   - Required reason
   - Authorization check
   - Status update
   - Tests

4. Implement cancel command ✓
   - Requester-only authorization
   - Status update
   - Tests

5. Enhanced error handling ✓
   - Better error messages
   - Next-step suggestions
   - Edge case handling

**Deliverables:**
- Multi-approval support working
- Snapshot protection active
- All approval commands complete
- Comprehensive test coverage

### Phase 3: User Experience (Week 4)

**Goals:**
- Enhanced hitch guard
- Better output formatting
- Improved error messages

**Tasks:**

1. Enhance hitch guard ✓
   - Detect approval-required environments
   - Approval-specific guidance
   - Clear, helpful messages
   - Tests

2. Improve output formatting ✓
   - Consistent table formatting
   - Status indicators (✓ ✗ ⏳)
   - Color support (optional)
   - Aligned columns

3. Polish error messages ✓
   - Consistent format
   - Clear problem statement
   - Resolution steps
   - Context information

4. Add progress indicators ✓
   - Show steps during promotion
   - Clear success/failure states
   - Time estimates where appropriate

**Deliverables:**
- Polished user experience
- Consistent, helpful messages
- Good terminal formatting

### Phase 4: Documentation & Testing (Week 5)

**Goals:**
- Comprehensive documentation
- Full test coverage
- Migration guide

**Tasks:**

1. Update README.md ✓
   - Approval workflow section
   - Configuration examples
   - Command reference
   - Quick start guide

2. Create approval workflow guide ✓
   - Detailed usage examples
   - Common workflows
   - Troubleshooting
   - Best practices

3. Write migration guide ✓
   - For existing users
   - Configuration steps
   - Git provider setup
   - Rollout strategy

4. Update CLAUDE.md ✓
   - New command structure
   - Testing instructions
   - Architecture notes

5. Add comprehensive tests ✓
   - Edge case coverage
   - Authorization scenarios
   - Concurrent operations
   - Error conditions
   - Integration test suite

6. Security documentation ✓
   - Security model explanation
   - Branch protection setup
   - Attack vectors and mitigations
   - Best practices

**Deliverables:**
- Complete documentation
- > 80% test coverage
- Ready for release

### Phase 5: Beta Testing & Polish (Week 6)

**Goals:**
- Real-world testing
- Bug fixes
- Performance optimization

**Tasks:**

1. Beta testing ✓
   - Deploy to test environments
   - Gather feedback
   - Identify issues

2. Bug fixes ✓
   - Address beta feedback
   - Fix edge cases
   - Improve error handling

3. Performance optimization ✓
   - Optimize list command for large datasets
   - Reduce metadata size if needed
   - Improve approval execution time

4. Final polish ✓
   - Code review
   - Documentation review
   - Final testing

**Deliverables:**
- Production-ready release
- Beta feedback addressed
- All tests passing

### Total Timeline: 6 weeks

**Milestones:**
- End of Week 2: Basic approval workflow working
- End of Week 3: All features complete
- End of Week 4: Polished UX
- End of Week 5: Documented and tested
- End of Week 6: Production-ready

---

## Testing Strategy

### Unit Tests

**Coverage targets:**
- Data structure serialization/deserialization: 100%
- Authorization logic: 100%
- Snapshot validation: 100%
- Approval workflow logic: > 90%
- Command parsing: > 80%

**Key test scenarios:**

```rust
#[cfg(test)]
mod tests {
    // Authorization tests
    #[test] fn test_authorized_approver() { }
    #[test] fn test_unauthorized_approver() { }
    #[test] fn test_self_approval_rejected() { }
    #[test] fn test_duplicate_approval_rejected() { }

    // Threshold tests
    #[test] fn test_single_approval_threshold() { }
    #[test] fn test_multi_approval_threshold() { }
    #[test] fn test_partial_approvals() { }

    // Snapshot tests
    #[test] fn test_snapshot_capture() { }
    #[test] fn test_snapshot_validation_success() { }
    #[test] fn test_snapshot_validation_failure() { }
    #[test] fn test_stale_snapshot_detected() { }

    // Status transitions
    #[test] fn test_pending_to_approved() { }
    #[test] fn test_pending_to_rejected() { }
    #[test] fn test_pending_to_cancelled() { }
    #[test] fn test_approved_to_applied() { }
    #[test] fn test_invalid_status_transitions() { }
}
```

### Integration Tests

**Test fixtures:**
- Temporary git repositories
- Mock hitch configurations
- Sample approval requests

**Key test scenarios:**

```rust
#[test]
fn test_full_approval_workflow() {
    // Setup: Create repo with approval-required environment
    // Act: promote → approve → verify execution
    // Assert: Branch promoted, status Applied
}

#[test]
fn test_multi_approver_workflow() {
    // Setup: Environment requiring 2 approvals
    // Act: promote → approve (user1) → approve (user2)
    // Assert: Executes after 2nd approval
}

#[test]
fn test_rejection_workflow() {
    // Setup: Pending approval
    // Act: reject with reason
    // Assert: Status Rejected, not executed
}

#[test]
fn test_snapshot_staleness() {
    // Setup: Pending approval
    // Act: Force push branch → approve
    // Assert: Fails with stale snapshot error
}

#[test]
fn test_concurrent_approvals() {
    // Setup: Pending approval
    // Act: Two approvers approve simultaneously
    // Assert: Both approvals recorded correctly
}

#[test]
fn test_backward_compatibility() {
    // Setup: Environment without requires_approval
    // Act: promote
    // Assert: Executes immediately as before
}
```

### Manual Testing Checklist

**Setup:**
- [ ] Install hitch with approval feature
- [ ] Configure test repository
- [ ] Set up git provider branch protection
- [ ] Configure test environment with approval

**Basic Workflow:**
- [ ] Create approval request via promote
- [ ] List pending approvals
- [ ] View approval status
- [ ] Approve request
- [ ] Verify promotion executed
- [ ] Check hitch status shows promoted branch

**Authorization:**
- [ ] Attempt approval by unauthorized user → fails
- [ ] Attempt self-approval → fails
- [ ] Attempt duplicate approval → fails
- [ ] Approve by authorized user → succeeds

**Multi-Approval:**
- [ ] Configure environment requiring 2 approvals
- [ ] Create approval request
- [ ] First approval → still pending
- [ ] Second approval → executes

**Rejection:**
- [ ] Create approval request
- [ ] Reject with reason
- [ ] Verify not executed
- [ ] Verify rejection recorded

**Snapshot Validation:**
- [ ] Create approval request
- [ ] Force push branch
- [ ] Attempt approval → fails
- [ ] Create new request → succeeds

**Edge Cases:**
- [ ] Delete branch after approval requested
- [ ] Change environment config during approval
- [ ] Very long branch names
- [ ] Special characters in comments
- [ ] Network failure during push

**User Experience:**
- [ ] hitch guard on approval-required environment
- [ ] Clear error messages
- [ ] Helpful success messages
- [ ] Table formatting readable
- [ ] Help text accurate

---

## Success Metrics

### Launch Metrics (First 3 Months)

**Adoption:**
- Target: 30% of active hitch repositories enable approval workflow
- Measure: Track requires_approval usage in telemetry (opt-in)

**Engagement:**
- Target: Average 10+ approval requests per repository per month
- Measure: Count approval requests created

**Time to Value:**
- Target: < 15 minutes from install to first approval workflow
- Measure: User feedback surveys

### Quality Metrics (Ongoing)

**Reliability:**
- Target: < 1% approval requests fail due to bugs
- Measure: Error rate in telemetry

**Security:**
- Target: Zero unauthorized deployments reported
- Measure: Security incident reports

**Performance:**
- Target: 95th percentile approval execution < 10 seconds
- Measure: Command duration telemetry

### User Satisfaction (Quarterly)

**Net Promoter Score:**
- Target: NPS > 40 for approval workflow feature
- Measure: User surveys

**Support Volume:**
- Target: < 5 support requests per 100 approval workflows
- Measure: GitHub issues, support tickets

**Documentation Quality:**
- Target: 80% of users find documentation helpful
- Measure: Documentation feedback

---

## Dependencies and Risks

### Dependencies

**External:**
- Git provider branch protection (critical for security)
- Rust ecosystem (uuid, chrono, serde)
- Git command-line tools

**Internal:**
- Existing hitch metadata system
- Git operations utilities
- Command infrastructure

### Risks

**Risk 1: Git Provider Branch Protection Misconfiguration**

**Impact:** High - Users could bypass approvals
**Likelihood:** Medium
**Mitigation:**
- Clear documentation on setup
- Validation command to check configuration
- Warning in output if branch protection not detected
- Setup checklist in docs

**Risk 2: Concurrent Approval Conflicts**

**Impact:** Medium - Approvals could be lost
**Likelihood:** Low
**Mitigation:**
- Idempotent approval operations
- Retry logic with backoff
- Clear error messages
- Guidance on retry

**Risk 3: Metadata Growth**

**Impact:** Medium - Performance degradation
**Likelihood:** Medium
**Mitigation:**
- Cleanup command for old approvals
- Archive mechanism
- Performance monitoring
- Size warnings

**Risk 4: Approval Request Staleness**

**Impact:** Medium - Approving outdated code
**Likelihood:** Medium
**Mitigation:**
- Snapshot validation (primary)
- Clear error messages
- Easy to create new requests
- Stale request detection

**Risk 5: User Confusion on Security Model**

**Impact:** High - Misunderstanding security guarantees
**Likelihood:** Medium
**Mitigation:**
- Clear security documentation
- Setup guide with checklist
- Warning messages where appropriate
- Examples and best practices

**Risk 6: Breaking Changes in Upgrade**

**Impact:** High - Existing workflows broken
**Likelihood:** Low
**Mitigation:**
- Strict backward compatibility
- Comprehensive upgrade testing
- Migration guide
- Opt-in feature (default off)

---

## Open Questions

### Q1: Approval Request Retention

**Question:** How long should completed approval requests be kept?

**Options:**
1. Keep forever (audit trail)
2. Archive after 90 days
3. Configurable retention period
4. Manual cleanup only

**Decision needed by:** End of Phase 3
**Stakeholders:** DevOps teams, Compliance
**Current thinking:** Archive after 90 days, manual override

### Q2: Notification Mechanism

**Question:** Should v1 include any notification mechanism?

**Options:**
1. No notifications (v1)
2. CLI output only
3. Optional webhook support
4. Email/Slack integration

**Decision needed by:** End of Phase 1
**Stakeholders:** Product, Engineering
**Current thinking:** CLI only for v1, webhooks in v2

### Q3: Approval Expiration

**Question:** Should approval requests expire after a time period?

**Options:**
1. No expiration
2. Configurable expiration (e.g., 7 days)
3. Stale detection warning only
4. Automatic expiration with notification

**Decision needed by:** End of Phase 2
**Stakeholders:** Release managers
**Current thinking:** No expiration, rely on snapshot validation

### Q4: Emergency Override

**Question:** Should there be an emergency bypass mechanism?

**Options:**
1. No bypass (require approval always)
2. --force flag (logged in audit trail)
3. Emergency approver list
4. Disabled via environment config

**Decision needed by:** End of Phase 3
**Stakeholders:** DevOps, Security
**Current thinking:** No bypass, rely on git provider access for emergencies

### Q5: Multi-Repository Approval

**Question:** Should approvals work across repositories?

**Options:**
1. Single repository only (v1)
2. Cross-repository approval references
3. Shared approval service
4. Monorepo support

**Decision needed by:** End of Phase 1
**Stakeholders:** Product
**Current thinking:** Single repository for v1

---

## Appendix A: Alternative Architectures Considered

### Option 1: Git Provider Native (GitHub/GitLab API)

**Rejected because:**
- High implementation complexity (2000-3000 LOC)
- External API dependencies
- Provider-specific code paths
- API token management burden
- Rate limiting concerns

**Could revisit if:**
- Many users request native PR/MR integration
- Resources available for complex implementation
- Clear demand for CI integration

### Option 2: Git-Based Approval Branches

**Rejected because:**
- Branch pollution (many temp branches)
- Poor discoverability
- Awkward branch name encoding
- No clear UI for viewing approvals

**Could revisit if:**
- Metadata approach proves problematic
- Users want something even simpler

### Option 4: Hybrid Approach

**Rejected because:**
- Very high complexity (2500-3500 LOC)
- Two parallel approval tracks
- State synchronization challenges
- Complex user mental model

**Could revisit if:**
- Option 3 successful and users want more
- Phase 2 enhancement after v1 stable

### Option 5: Hosted Approval Service

**Rejected because:**
- Massive implementation effort (full-stack)
- Infrastructure costs
- External dependency
- Not aligned with hitch philosophy

**Could revisit as:**
- Separate commercial SaaS offering
- Optional companion service (not required)

### Option 6: Git Notes-Based

**Rejected because:**
- Poor discoverability
- Limited UI support
- Unfamiliar to developers
- Search performance issues

**Could revisit if:**
- Metadata approach has performance issues
- Need for commit-attached approvals

---

## Appendix B: Configuration Examples

### Example 1: Production Requires 2 Approvals

```json
{
  "version": "1.0",
  "environments": {
    "dev": {
      "base": "main",
      "branches": [],
      "requires_approval": false
    },
    "staging": {
      "base": "main",
      "branches": [],
      "requires_approval": true,
      "approvers": ["tech-lead@company.com"],
      "min_approvals": 1
    },
    "production": {
      "base": "main",
      "branches": [],
      "requires_approval": true,
      "approvers": [
        "tech-lead@company.com",
        "senior-eng-1@company.com",
        "senior-eng-2@company.com"
      ],
      "min_approvals": 2
    }
  }
}
```

### Example 2: Gradual Rollout

**Step 1:** Enable for staging (test workflow)
```json
{
  "environments": {
    "staging": {
      "requires_approval": true,
      "approvers": ["you@company.com"]
    }
  }
}
```

**Step 2:** Add production with single approver
```json
{
  "environments": {
    "production": {
      "requires_approval": true,
      "approvers": ["tech-lead@company.com"],
      "min_approvals": 1
    }
  }
}
```

**Step 3:** Increase to multiple approvers
```json
{
  "environments": {
    "production": {
      "requires_approval": true,
      "approvers": [
        "tech-lead@company.com",
        "senior-eng@company.com"
      ],
      "min_approvals": 2
    }
  }
}
```

### Example 3: Different Teams for Different Environments

```json
{
  "environments": {
    "qa": {
      "requires_approval": true,
      "approvers": ["qa-lead@company.com"],
      "min_approvals": 1
    },
    "staging": {
      "requires_approval": true,
      "approvers": [
        "backend-lead@company.com",
        "frontend-lead@company.com"
      ],
      "min_approvals": 1
    },
    "production": {
      "requires_approval": true,
      "approvers": [
        "cto@company.com",
        "tech-lead@company.com",
        "devops-lead@company.com"
      ],
      "min_approvals": 2
    }
  }
}
```

---

## Appendix C: Git Provider Setup Guides

### GitHub Branch Protection

1. Navigate to repository **Settings → Branches**
2. Click **Add branch protection rule**
3. Branch name pattern: `production` (or your environment name)
4. Configure rules:
   - ☑️ Require a pull request before merging
   - ☑️ Require approvals: 1
   - ☑️ Restrict who can push to matching branches
     - Add: CI/CD service account or release managers
   - ☑️ Do not allow bypassing the above settings
   - ☐ Include administrators (optional, for stricter security)
5. Click **Create** or **Save changes**
6. Repeat for each environment branch

**Verification:**
```bash
# Attempt to push directly (should fail)
git push origin production
→ remote: error: GH006: Protected branch update failed
```

### GitLab Protected Branches

1. Navigate to **Settings → Repository → Protected Branches**
2. Select branch: `production`
3. Configure protection:
   - **Allowed to merge:** Maintainers + Developers
   - **Allowed to push:** Select specific users or "No one"
   - **Allowed to force push:** ☐ (unchecked)
4. Click **Protect**
5. Repeat for each environment branch

**Verification:**
```bash
# Attempt to push directly (should fail)
git push origin production
→ remote: GitLab: You are not allowed to push code to protected branches
```

### Bitbucket Branch Permissions

1. Navigate to **Repository settings → Branch permissions**
2. Click **Add a branch permission**
3. Branch: `production`
4. Type: **Prevent all changes**
5. Exceptions:
   - Add specific users who can push (CI/CD account)
6. Click **Create**
7. Repeat for each environment branch

**Verification:**
```bash
# Attempt to push directly (should fail)
git push origin production
→ remote: Permission denied
```

---

## Appendix D: Migration Checklist

### For Repository Administrators

- [ ] **Update hitch** to version with approval support
- [ ] **Review environments** that need approval
- [ ] **Identify approvers** for each environment
- [ ] **Update hitch.json** with approval configuration
- [ ] **Set up git provider branch protection**
- [ ] **Test approval workflow** in non-production environment
- [ ] **Document process** for your team
- [ ] **Train approvers** on new commands
- [ ] **Enable for production** environment
- [ ] **Monitor first few approvals** for issues
- [ ] **Gather feedback** from team

### For Developers

- [ ] **Update hitch** to latest version
- [ ] **Read approval workflow docs**
- [ ] **Understand which environments require approval**
- [ ] **Know who the approvers are**
- [ ] **Practice creating approval requests** in dev/staging
- [ ] **Bookmark approval commands** for reference

### For Approvers

- [ ] **Verify you're in approvers list** (`hitch status`)
- [ ] **Learn approval commands** (`hitch approvals --help`)
- [ ] **Set up git config email** (identity verification)
- [ ] **Practice approving** in staging environment
- [ ] **Understand approval responsibilities**
- [ ] **Know how to reject** requests appropriately

---

## Document History

| Version | Date | Author | Changes |
|---------|------|--------|---------|
| 1.0 | 2024-12-17 | Martin | Initial PRD created |

---

**Status:** Draft - Ready for Review
**Next Steps:** Technical review → Stakeholder approval → Begin implementation
