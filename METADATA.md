# Hitch Metadata Format

This document describes the structure of the `hitch.json` file stored in the `hitch-metadata` branch.

## Overview

Hitch stores all state in a single JSON file: `hitch.json`

This file lives in an orphan branch called `hitch-metadata` and is the single source of truth for:
- Which features are in which environments
- Branch lifecycle information
- Lock status
- Configuration

## Schema

### Top Level

```json
{
  "version": "1.0.0",
  "environments": { ... },
  "branches": { ... },
  "config": { ... },
  "metadata": { ... }
}
```

| Field | Type | Description |
|-------|------|-------------|
| `version` | string | Metadata schema version (for future migrations) |
| `environments` | object | Environment definitions and current state |
| `branches` | object | Branch lifecycle tracking |
| `config` | object | Configuration settings |
| `metadata` | object | Metadata about the metadata (last update, etc.) |

---

## `environments`

Maps environment names to their configuration and current state.

```json
{
  "environments": {
    "dev": {
      "base": "main",
      "features": ["feature/user-auth", "feature/dashboard"],
      "locked": false,
      "locked_by": null,
      "locked_at": null,
      "last_rebuild": "2025-10-16T10:30:00Z",
      "last_rebuild_commit": "a1b2c3d4e5f6"
    },
    "qa": {
      "base": "main",
      "features": ["feature/user-auth"],
      "locked": true,
      "locked_by": "dev-m@example.com",
      "locked_at": "2025-10-16T11:00:00Z",
      "locked_reason": "Testing critical fix",
      "last_rebuild": "2025-10-16T09:15:00Z",
      "last_rebuild_commit": "b2c3d4e5f6g7"
    }
  }
}
```

### Environment Object

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `base` | string | Yes | Base branch name (usually "main") |
| `features` | array[string] | Yes | Ordered list of feature branches in this environment |
| `locked` | boolean | Yes | Whether environment is currently locked |
| `locked_by` | string | No | Email/username of who locked the environment |
| `locked_at` | string (ISO 8601) | No | When the lock was acquired |
| `locked_reason` | string | No | Optional reason for lock |
| `last_rebuild` | string (ISO 8601) | No | When environment was last rebuilt |
| `last_rebuild_commit` | string | No | Git commit SHA of base branch at last rebuild |

**Notes:**
- `features` array order matters - features are merged in this order
- When `locked=true`, only the locking user can modify (unless `--force`)
- Locks older than 15 minutes are considered stale

---

## `branches`

Maps branch names to their lifecycle information.

```json
{
  "branches": {
    "feature/user-auth": {
      "created_at": "2025-10-01T09:00:00Z",
      "created_by": "dev-m@example.com",
      "promoted_to": ["dev", "qa"],
      "promoted_history": [
        {
          "environment": "dev",
          "promoted_at": "2025-10-01T10:00:00Z",
          "promoted_by": "dev-m@example.com"
        },
        {
          "environment": "qa",
          "promoted_at": "2025-10-02T14:30:00Z",
          "promoted_by": "dev-m@example.com"
        }
      ],
      "merged_to_main_at": null,
      "merged_to_main_by": null,
      "last_commit_at": "2025-10-03T08:15:00Z",
      "last_commit_sha": "c3d4e5f6g7h8"
    },
    "bug/fix-login": {
      "created_at": "2025-09-28T11:00:00Z",
      "created_by": "dev-s@example.com",
      "promoted_to": [],
      "promoted_history": [
        {
          "environment": "dev",
          "promoted_at": "2025-09-28T12:00:00Z",
          "promoted_by": "dev-s@example.com"
        },
        {
          "environment": "qa",
          "promoted_at": "2025-09-29T09:00:00Z",
          "promoted_by": "dev-s@example.com"
        }
      ],
      "merged_to_main_at": "2025-10-08T16:45:00Z",
      "merged_to_main_by": "dev-s@example.com",
      "last_commit_at": "2025-10-08T16:30:00Z",
      "last_commit_sha": "d4e5f6g7h8i9",
      "eligible_for_cleanup_at": "2025-10-15T16:45:00Z"
    }
  }
}
```

### Branch Object

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `created_at` | string (ISO 8601) | Yes | When branch was first tracked by Hitch |
| `created_by` | string | No | Who created/first promoted the branch |
| `promoted_to` | array[string] | Yes | Currently deployed environments (empty if merged) |
| `promoted_history` | array[PromotionEvent] | No | Full history of promotions |
| `merged_to_main_at` | string (ISO 8601) | No | When branch was merged to main (null if not merged) |
| `merged_to_main_by` | string | No | Who merged the branch |
| `last_commit_at` | string (ISO 8601) | No | Last commit timestamp on this branch |
| `last_commit_sha` | string | No | Last commit SHA |
| `eligible_for_cleanup_at` | string (ISO 8601) | No | When branch can be safely deleted |

### PromotionEvent Object

```json
{
  "environment": "dev",
  "promoted_at": "2025-10-01T10:00:00Z",
  "promoted_by": "dev-m@example.com",
  "demoted_at": "2025-10-02T11:00:00Z",
  "demoted_by": "dev-m@example.com"
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `environment` | string | Yes | Environment name |
| `promoted_at` | string (ISO 8601) | Yes | When promoted |
| `promoted_by` | string | No | Who promoted |
| `demoted_at` | string (ISO 8601) | No | When demoted (if removed before merge) |
| `demoted_by` | string | No | Who demoted |

**Notes:**
- When a branch is merged to main, it's removed from all `promoted_to` arrays
- `eligible_for_cleanup_at` = `merged_to_main_at` + `config.retention_days_after_merge`
- Branches not in any environment and past cleanup date can be deleted

---

## `config`

Global configuration settings.

```json
{
  "config": {
    "retention_days_after_merge": 7,
    "stale_days_no_activity": 30,
    "base_branch": "main",
    "lock_timeout_minutes": 15,
    "auto_rebuild_on_promote": true,
    "conflict_strategy": "abort",
    "notification_webhooks": [
      {
        "url": "https://hooks.slack.com/services/...",
        "events": ["promote", "release", "conflict"]
      }
    ]
  }
}
```

### Config Fields

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `retention_days_after_merge` | integer | 7 | Days to keep branches after merging to main |
| `stale_days_no_activity` | integer | 30 | Days of inactivity before considering branch stale |
| `base_branch` | string | "main" | Base branch name |
| `lock_timeout_minutes` | integer | 15 | Minutes before lock is considered stale |
| `auto_rebuild_on_promote` | boolean | true | Automatically rebuild environment after promotion |
| `conflict_strategy` | enum | "abort" | How to handle merge conflicts: "abort" or "manual" |
| `notification_webhooks` | array[Webhook] | [] | Webhook URLs to notify on events |

### Webhook Object

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `url` | string | Yes | Webhook URL |
| `events` | array[string] | Yes | Events to trigger webhook: "promote", "demote", "release", "conflict", "lock", "unlock" |
| `headers` | object | No | Custom headers to send |

---

## `metadata`

Metadata about the metadata itself.

```json
{
  "metadata": {
    "initialized_at": "2025-10-01T08:00:00Z",
    "initialized_by": "dev-m@example.com",
    "last_modified_at": "2025-10-16T11:00:00Z",
    "last_modified_by": "dev-s@example.com",
    "last_command": "hitch promote feature/dashboard to dev",
    "hitch_version": "1.0.0"
  }
}
```

| Field | Type | Description |
|-------|------|-------------|
| `initialized_at` | string (ISO 8601) | When `hitch init` was run |
| `initialized_by` | string | Who ran `hitch init` |
| `last_modified_at` | string (ISO 8601) | Last metadata update |
| `last_modified_by` | string | Who made last update |
| `last_command` | string | Last Hitch command executed |
| `hitch_version` | string | Version of Hitch that last modified metadata |

---

## Complete Example

```json
{
  "version": "1.0.0",
  "environments": {
    "dev": {
      "base": "main",
      "features": [
        "feature/user-auth",
        "feature/dashboard",
        "bug/fix-login"
      ],
      "locked": false,
      "locked_by": null,
      "locked_at": null,
      "last_rebuild": "2025-10-16T10:30:00Z",
      "last_rebuild_commit": "a1b2c3d4e5f6"
    },
    "qa": {
      "base": "main",
      "features": [
        "feature/user-auth",
        "feature/dashboard"
      ],
      "locked": true,
      "locked_by": "dev-m@example.com",
      "locked_at": "2025-10-16T11:00:00Z",
      "locked_reason": "Testing before production release",
      "last_rebuild": "2025-10-16T09:15:00Z",
      "last_rebuild_commit": "b2c3d4e5f6g7"
    }
  },
  "branches": {
    "feature/user-auth": {
      "created_at": "2025-10-01T09:00:00Z",
      "created_by": "dev-m@example.com",
      "promoted_to": ["dev", "qa"],
      "promoted_history": [
        {
          "environment": "dev",
          "promoted_at": "2025-10-01T10:00:00Z",
          "promoted_by": "dev-m@example.com"
        },
        {
          "environment": "qa",
          "promoted_at": "2025-10-02T14:30:00Z",
          "promoted_by": "dev-m@example.com"
        }
      ],
      "merged_to_main_at": null,
      "merged_to_main_by": null,
      "last_commit_at": "2025-10-03T08:15:00Z",
      "last_commit_sha": "c3d4e5f6g7h8"
    },
    "feature/dashboard": {
      "created_at": "2025-10-05T11:00:00Z",
      "created_by": "dev-s@example.com",
      "promoted_to": ["dev", "qa"],
      "promoted_history": [
        {
          "environment": "dev",
          "promoted_at": "2025-10-05T12:00:00Z",
          "promoted_by": "dev-s@example.com"
        },
        {
          "environment": "qa",
          "promoted_at": "2025-10-06T09:00:00Z",
          "promoted_by": "dev-s@example.com"
        }
      ],
      "merged_to_main_at": null,
      "merged_to_main_by": null,
      "last_commit_at": "2025-10-15T16:30:00Z",
      "last_commit_sha": "e5f6g7h8i9j0"
    },
    "bug/fix-login": {
      "created_at": "2025-09-28T11:00:00Z",
      "created_by": "dev-r@example.com",
      "promoted_to": ["dev"],
      "promoted_history": [
        {
          "environment": "dev",
          "promoted_at": "2025-09-28T12:00:00Z",
          "promoted_by": "dev-r@example.com"
        },
        {
          "environment": "qa",
          "promoted_at": "2025-09-29T09:00:00Z",
          "promoted_by": "dev-r@example.com",
          "demoted_at": "2025-09-30T10:00:00Z",
          "demoted_by": "dev-r@example.com"
        },
        {
          "environment": "qa",
          "promoted_at": "2025-10-01T11:00:00Z",
          "promoted_by": "dev-r@example.com"
        }
      ],
      "merged_to_main_at": "2025-10-08T16:45:00Z",
      "merged_to_main_by": "dev-r@example.com",
      "last_commit_at": "2025-10-08T16:30:00Z",
      "last_commit_sha": "f6g7h8i9j0k1",
      "eligible_for_cleanup_at": "2025-10-15T16:45:00Z"
    }
  },
  "config": {
    "retention_days_after_merge": 7,
    "stale_days_no_activity": 30,
    "base_branch": "main",
    "lock_timeout_minutes": 15,
    "auto_rebuild_on_promote": true,
    "conflict_strategy": "abort",
    "notification_webhooks": []
  },
  "metadata": {
    "initialized_at": "2025-10-01T08:00:00Z",
    "initialized_by": "dev-m@example.com",
    "last_modified_at": "2025-10-16T11:00:00Z",
    "last_modified_by": "dev-s@example.com",
    "last_command": "hitch promote bug/fix-login to dev",
    "hitch_version": "1.0.0"
  }
}
```

---

## Validation Rules

1. **Version**: Must be semantic version string (e.g., "1.0.0")
2. **Environment names**: Must match `[a-z0-9-]+` (lowercase, numbers, hyphens)
3. **Branch names**: Must be valid Git branch names
4. **Timestamps**: Must be ISO 8601 format with timezone
5. **Lock consistency**: If `locked=true`, `locked_by` and `locked_at` must be set
6. **Feature array**: Features in environment must exist in `branches` object
7. **Promoted consistency**: If branch in `environment.features`, environment must be in `branch.promoted_to`

---

## Schema Evolution

### Versioning

The `version` field allows for future schema migrations:

```json
{
  "version": "2.0.0",
  "environments": { ... }
}
```

When Hitch reads metadata:
1. Check `version` field
2. If version < current, run migration
3. Update `version` to current
4. Save migrated metadata

### Migration Example

```go
func MigrateMetadata(data map[string]interface{}) error {
    version := data["version"].(string)

    if version == "1.0.0" {
        // Add new field introduced in 2.0.0
        for _, env := range data["environments"] {
            if env["conflict_count"] == nil {
                env["conflict_count"] = 0
            }
        }
        data["version"] = "2.0.0"
    }

    return nil
}
```

---

## Concurrency

### Optimistic Locking

To prevent lost updates when multiple developers run Hitch simultaneously:

1. **Read** metadata (note commit SHA)
2. **Modify** metadata in memory
3. **Write** metadata with `git push --force-with-lease=hitch-metadata:<SHA>`

If another update occurred between read and write, push will fail.

### Lock Protocol

```
1. Read metadata
2. Check if environment.locked == true
3. If locked and lock age < timeout:
     - If locked_by == current_user: proceed
     - Else: fail with "environment locked" error
4. If unlocked or stale lock:
     - Set locked = true
     - Set locked_by = current_user
     - Set locked_at = now
     - Write metadata
5. Perform operation
6. Set locked = false
7. Write metadata
```

---

## Backup and Recovery

### Manual Backup

```bash
# Checkout metadata branch
git checkout hitch-metadata

# Create backup
git tag hitch-backup-$(date +%Y%m%d-%H%M%S)
git push origin --tags
```

### Restore from Backup

```bash
# List backups
git tag -l "hitch-backup-*"

# Restore specific backup
git checkout hitch-metadata
git reset --hard hitch-backup-20251016-103000
git push --force-with-lease origin hitch-metadata
```

### Automatic Backups

Hitch could automatically create daily tags:
- `hitch-auto-backup-20251016`
- Retention: 30 days
