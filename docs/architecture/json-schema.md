# `--json` document schema

`--json` makes a command print **one JSON document on stdout and nothing else**.
Progress, warnings and errors go to stderr. A command that does not support the
flag leaves stdout empty rather than printing prose into a stream you are about
to parse.

Every example below was produced by running the built binary against a throwaway
repository. Long documents are trimmed with `…`; nothing else is edited. Commit
ids are real but will differ in your repository.

## Commands that honour `--json`

The list is checked by `tests/integration/json_support_tests.rs` against the
`--json` doc comment in `src/cli.rs` and against the commands that actually emit
a document. Keep one backticked name per bullet; subcommands are written with a
space.

- `rebuild`
- `promote`
- `demote`
- `release`
- `lock`
- `unlock`
- `set`
- `add`
- `remove`
- `cleanup`
- `approvals approve`
- `status`
- `why`
- `log`

That is every mutating command plus the three read-only views (`status`, `why`,
`log`). `hitch diff`, `tree`, `conflicts`, `resolve` and the rest do not emit a
document.

## Conventions

* **`schema_version`** is the first key of every document and is currently `1`.
  It changes only for a change that can break a consumer that was reading the old
  shape: a removed or renamed key, or a changed meaning. Adding a key does not
  bump it, so ignore keys you do not know. A consumer should check the version
  and refuse a number it has not seen.
* **Timestamps** (`when`, `started_at`, `completed_at`, `built_at`, `captured_at`,
  `rebuilt_at`, ...) are RFC 3339 in UTC, e.g. `2026-10-01T13:35:46Z`. Some
  carry fractional seconds, some do not; parse them, do not pattern-match them.
* **Enum casing.** In the read-only documents (`status`, `why`, `log`) every enum
  value is `snake_case` (`"with_holds"`, `"not_desired"`), and externally tagged
  variants are objects keyed by a `snake_case` name (`{"held": {...}}`). In the
  `plan` and `receipt` halves of a mutating document, the operation and outcome
  enums (`kind`, `intent`, `effects`, `outcome`) are currently spelled with their
  Rust variant names (`"Lock"`, `"Applied"`, `"MetadataChange"`). Match on both
  spellings if you must handle either document family in one code path.
* **Commit ids** are full 40-character ids in JSON, abbreviated in human output.
* **Plan ids** (`plan.id`, repeated as `receipt.plan_id`) are opaque strings; use
  them to pair a plan with its receipt, not to parse.

## Exit codes

Exit codes are the same with and without `--json`.

| Code | Meaning |
| --- | --- |
| 0 | Success, or a declined confirmation, or a read-only view |
| 1 | Failure: a refusal, a stale plan, a failed operation |
| 2 | **Applied with held branches** (`rebuild`, including a `--dry-run` that would hold). The document is still printed in full. A command-line usage error from the parser also exits 2, with nothing on stdout |

`--json` **without `--yes`** on a mutating command that needs confirmation exits
**1** with the reason on stderr and nothing on stdout, and nothing is changed.
It never prompts: a program that blocks on a terminal read is the failure this
flag exists to remove. Add `--yes` (or set `HITCH_YES=1`), or use `--dry-run` to
read a plan without applying it.

```
$ hitch lock qa --json
Error: Refusing to prompt under --json: this operation needs confirmation.
Re-run with --yes (or set HITCH_YES=1) to confirm without a terminal.
```

## Mutating commands: `{schema_version, plan, receipt}`

`plan` says what was about to happen; `receipt` says what did. `receipt` is
`null` for a `--dry-run`, because nothing happened.

### Dry run (`receipt: null`)

`hitch add staging --base main --dry-run --json`

```json
{
  "schema_version": 1,
  "plan": {
    "id": "add:staging:staging:e0fe1667933664683c86781bff25703e95aeee92",
    "kind": "AddEnvironment",
    "intent": {
      "AddEnvironment": {
        "environment": "staging",
        "base": "main"
      }
    },
    "fingerprint": {
      "metadata_sha": "d83836b1c49c79ac604df9306514aa2029abc4ac",
      "refs": {},
      "remote_refs": {},
      "resolution_keys": []
    },
    "current": null,
    "proposed": {
      "environment": "staging",
      "base": "main",
      "branches": [],
      "branch_sha": null
    },
    "compositions": [],
    "effects": [
      {
        "MetadataChange": {
          "refname": "refs/heads/hitch-metadata",
          "description": "declare environment 'staging' on base main"
        }
      }
    ],
    "unaffected": [],
    "warnings": [],
    "confirmation": {
      "required": false,
      "reason": null
    },
    "detail": {
      "environment": "staging",
      "argument": "staging",
      "edit": "Create",
      "changes": [],
      "branch_absorbed_by_base": null,
      "locked_by": null,
      "base": "main"
    }
  },
  "receipt": null
}
```

The plan's parts:

* `current` / `proposed` — the environment's composition before and after
  (`branch_sha` is the environment branch's tip; `null` when it does not exist).
  `current` is `null` when there is nothing yet, and for a release: the target is a
  shared branch the release merges into, not a composition, so its before-state
  would only restate the target's name; read `proposed` and `effects` instead.
* `fingerprint` — what the plan depended on; see `explainable-operations.md`.
* `compositions` — for builds, one entry per branch: included, held (with the
  partner and files) or replayed.
* `effects` — what applying would change. `unaffected` — what it will not touch.
* `warnings` — each has a `kind` (an advisory, or a blocking refusal or approval
  gate), a `message`, and an optional `remedy`.
* `confirmation` — whether a person (or `--yes`) must agree first, and why.
* `detail` — operation-specific; its shape follows `kind`.

### Applied

`hitch lock qa --yes --json`, with `receipt.resulting_state` trimmed (it is the
full environment snapshot, and is the same shape as `status.environments`):

```json
{
  "schema_version": 1,
  "plan": { "id": "lock:qa:qa:f504ab7d…", "kind": "Lock", "…": "…" },
  "receipt": {
    "plan_id": "lock:qa:qa:f504ab7dc586fecdc5768779ba1e49bd325e2c82",
    "operation": "Lock",
    "started_at": "2026-10-01T13:37:39.151400Z",
    "completed_at": "2026-10-01T13:37:39.377871Z",
    "outcome": "Applied",
    "effects": [
      {
        "MetadataChange": {
          "refname": "refs/heads/hitch-metadata",
          "description": "lock 'qa' held by dev@example.com"
        }
      }
    ],
    "warnings": [],
    "resulting_state": { "metadata_sha": "…", "environments": ["…"], "features": ["…"], "captured_at": "…" }
  }
}
```

`outcome` is one of `Applied`, `AppliedWithHolds` (exit 2), `ApprovalRequested`
(an approval request was filed instead of editing the declaration) or `NoChange`.
A declined confirmation or a refusal produces no receipt at all. `receipt.warnings` holds only what the apply learned and the plan
could not have known; an entry with `owes_effect: true` is work still owed
(the human output marks it `⧗`). A rebuild's held branches are in
`resulting_state`, in the environment's `health`:

```json
"health": { "partially_realised": { "held": ["clash"] } }
```

### Cleanup: `failures`

`hitch cleanup --apply --json` adds a top-level `failures` key **only when a
delete failed** after the plan was accepted. The receipt describes what applied,
the command then exits 1, and the key is absent when nothing failed. The receipt
has no `resulting_state` (`null`).

```json
{
  "schema_version": 1,
  "plan": { "kind": "Cleanup", "…": "…" },
  "receipt": {
    "operation": "Cleanup",
    "outcome": "Applied",
    "effects": [],
    "warnings": [],
    "resulting_state": null,
    "…": "…"
  },
  "failures": [
    {
      "refname": "refs/heads/stale3",
      "cause": "Failed to delete branch 'stale3': error: could not delete reference refs/heads/stale3: cannot lock ref 'refs/heads/stale3': Unable to create '/…/.git/refs/heads/stale3.lock': File exists."
    }
  ]
}
```

A cleanup with nothing to delete still emits one document.

### Approve below its threshold: `plan: null, receipt: null`

`hitch approvals approve <id> --json --yes` for a vote that does not yet meet the
threshold changes nothing but the vote, so there is no plan to carry. The envelope
keeps the same keys, and adds `approval`:

```json
{
  "approval": {
    "approvals": 1,
    "environment": "qa",
    "remaining_approvers": [
      "third@example.com"
    ],
    "request_id": "3e9853e4-27a9-4516-92b6-e531241c8e95",
    "required": 2,
    "threshold_met": false
  },
  "plan": null,
  "receipt": null,
  "schema_version": 1
}
```

(Key order differs from the other documents; it is not significant.) A vote that
*does* meet the threshold applies the promotion and emits an ordinary
`{schema_version, plan, receipt}`.

## Read-only commands: `{schema_version, <view>}`

A read-only view has no "after", and a `null` receipt would say "nothing
happened", which is true and useless. So the envelope has one half, keyed by the
view's name. None of the three takes the repository lock.

### `status`

`hitch status --json`, trimmed to its shape:

```json
{
  "schema_version": 1,
  "status": {
    "current_branch": "main",
    "captured_at": "2026-10-01T13:37:41.019382Z",
    "environments": [ { "name": "dev", "…": "…" } ],
    "matrix": {
      "columns": ["dev", "qa"],
      "rows": [
        { "feature": "clash",    "cells": ["held", "held"] },
        { "feature": "payments", "cells": ["included", "not_desired"] },
        { "feature": "search",   "cells": ["included", "not_desired"] }
      ],
      "summaries": [
        {
          "environment": "dev",
          "base": "main",
          "desired": 3,
          "realised": 2,
          "held": 1,
          "needs_rebuild": 0,
          "missing": 0,
          "actual_unknown": 0,
          "locked": false,
          "health": { "partially_realised": { "held": ["clash"] } },
          "equation": { "environment": "dev", "base": "main", "terms": [ { "branch": "clash", "state": "plain" }, "…" ], "excluded": [] },
          "rebuilt_at": "2026-10-02T12:13:00Z"
        },
        "…"
      ]
    }
  }
}
```

Cells are one of `not_desired`, `included`, `held`, `in_base`, `needs_rebuild`,
`actual_unknown`, `missing`. `equation` is the environment's declaration and `rebuilt_at` its last rebuild (`null` if never). `health` is one of `realised`,
`{"partially_realised": {"held": [...]}}`, `{"needs_rebuild": {...}}`,
`never_built`, `legacy_unknown`, `missing_branch`. Each environment's `desired`
and `actual` are both present, so a consumer can diff them itself.

### `why`

`hitch why clash qa --json` — the `form` key selects the shape
(`feature`, `feature_in_environment`, `environment`):

```json
{
  "schema_version": 1,
  "why": {
    "form": "feature_in_environment",
    "branch": "clash",
    "environment": "qa",
    "desired_equation": {
      "environment": "qa",
      "base": "main",
      "terms": [ { "branch": "clash", "state": "plain" } ],
      "excluded": []
    },
    "actual_equation": {
      "environment": "qa",
      "base": "main",
      "terms": [],
      "excluded": [
        {
          "branch": "clash",
          "reason": { "held": { "conflicts_with": "main", "files": ["shared.txt"] } }
        }
      ]
    },
    "membership": "held",
    "reason": { "held_against": { "conflicts_with": "main", "files": ["shared.txt"] } },
    "health": { "partially_realised": { "held": ["clash"] } },
    "locked": false,
    "what_hitch_did": [ { "held": { "branch": "clash", "conflicts_with": "main" } } ],
    "…": "…"
  }
}
```

When Hitch cannot know (an environment with no build record), `membership` is
`actual_unknown` and no reason is given; it never invents one.

### `log`

`hitch log --json -n 1`:

```json
{
  "schema_version": 1,
  "log": {
    "entries": [
      {
        "commit": "c61c2f0233566aa1044c50d3b8f85605cd8cd30a",
        "when": "2026-10-01T13:35:46Z",
        "actor": "Dev",
        "events": [
          {
            "kind": "rebuilt",
            "environment": "qa",
            "outcome": {
              "status": "with_holds",
              "included": [],
              "held": [ { "branch": "clash", "conflicts_with": "main" } ]
            }
          }
        ]
      }
    ],
    "skipped": [],
    "truncated": true
  }
}
```

`kind` is one of `environment_created`, `environment_removed`, `base_changed`,
`promoted`, `demoted`, `locked`, `unlocked`, `rebuilt`, `released`, and the
`approval_*` family. `outcome.status` for a rebuild is `clean`, `with_holds` or
`unrecorded` (no build record could be proved to belong to that rebuild).
`truncated` is `true` when `--limit` cut the history short; `skipped` lists
history commits that could not be read, so a partial log is never silent.
