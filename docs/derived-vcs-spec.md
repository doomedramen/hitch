# Strata — a derived-branch VCS (design spec)

**Status:** exploratory design, v0.1 — 2026-08-28.
**Working name:** "Strata" (placeholder, unbranded — rename freely).
**Origin:** ADHD brainstorm session over the hitch codebase; this document
captures the converged design before further evolution. Not a hitch
implementation plan — a from-scratch git-replacement thought experiment
that takes hitch's ideas as native primitives instead of a layer.

**Thesis:** hitch is evidence that git's data model has the derivation
arrow backwards. Git stores the *output* (branch tips, merge commits) and
throws away the *program* (which branches, in what order, on what base).
Strata stores the program and treats every branch, commit, and checkout as
regenerable compiled output.

---

## 1. The derivation core

### 1.1 What is stored vs. derived

Two stored kinds:

1. **Content-addressed objects** — blobs, trees, commits. Unchanged from
   git; the object database is the one part of git worth keeping intact.
2. **Declarations** — small structured documents, a tiny build language:
   a base ref plus an ordered list of operations (`promote <layer>`,
   `rebase-onto`, `patch-apply`), plus a resolution set (§2.2). The
   generalization of `hitch.json` from config sidecar to first-class
   source of truth.

Everything else is a **memoized derivation**: a cache entry keyed on
`(declaration hash, resolved input OIDs)` mapping to a tree/commit OID,
stored under its own ref namespace (`refs/derivation/*`, generalizing
hitch's `refs/hitch/build/*` anchor refs) so it survives GC and is never
hand-edited.

### 1.2 History bifurcates

- The **declaration log** is the real audit trail: small, causal,
  human-authored diffs ("promoted B before C", "changed base to main").
  Blame, review, and history questions target this log.
- The **materialized commit graph** is compiled output. Regenerating it
  wholesale is not "rewriting history" — the history is the declaration
  log, and it never rewrites.

### 1.3 Publish = pointer alias onto a ghost

A build is materialized and tested on a shadow ref first. CI's role
shifts from "test a commit" to "test a derivation and cache the verdict
against its key." A shadow ref that has gone green *is* the artifact;
promoting it to the real branch is a zero-compute CAS pointer alias — the
same atomic ref transaction hitch's `publish_branch` performs today, with
the same crash-journal obligations model. There is never a build step
between "approved" and "live".

CI verdicts can be folded into the derivation key itself ("this tree,
green as of test-suite-hash X"), making promotion a *structural* refusal
when no verdict exists — there is nothing to alias to. This replaces
branch-protection rules with a property of the data model.

### 1.4 Checkouts are disposable renders

A working directory is a named, disposable render of a declaration —
opened and closed like a browser tab, optionally FUSE-lazy (files resolve
on first read; a monorepo checkout costs nothing until touched). No
checkout is ever authoritative. The hitch rule "the user's working tree
is sacred / nothing builds in the checkout" generalizes to: every
checkout, human included, is a cache the system may re-render but never
needs to repair.

### 1.5 CLI shape

Imperative history surgery (rebase, cherry-pick, reset) is replaced by
declarative edits (`promote`, `demote`, `set-base`, `pin`) plus one
universal repair verb — `rebuild` — meaning "recompute the cache from the
declaration," full stop. "Unstuck" always means regenerate, never repair
by hand.

Declarations behave like lockfiles: a promoted layer can be a pinned
exact OID or a resolvable range ("latest passing"); an `update` command
re-resolves ranges and shows the diff before writing anything.

### 1.6 In-progress operations are objects

Every in-progress operation is a first-class object with a liveness
heartbeat — no hidden dotfiles (`.git/rebase-merge` folklore) and no
wedgeable state machine. One query distinguishes "stuck" from "slow."
Destructive operations preview their blast radius in human terms
("this orphans Sarah's Tuesday WIP"), never only in hashes. Concurrent
publishes are serialized by the CAS transaction; a lost race produces a
queryable race report, not an incident.

Remote push is a detached background obligation (journal-tracked, exactly
as hitch's publish journal records an owed push); the local ref
transaction is the entire perceived latency.

---

## 2. The merge model: conflict is a value, resolution is an input

Git's conflict UX exists because merge (a computation) and resolution
(human input) collapse into one mutable-workspace event, and the
resolution's identity is then destroyed inside a merge commit. Strata
separates them.

### 2.1 Conflict objects

Layer composition is a pure fold: `materialize(base, [L1, L2, ...])`,
each step an ORT-style tree merge in the object database (hitch's
`merge_tree_compose` path — no worktree, no index). A merge step never
"fails": its output is either a clean tree or a **conflict object** — a
typed, content-addressed record of `{base, ours, theirs}` stage triples
per path, with a per-hunk decomposition alongside the whole-file triple.
Nothing wedges; a conflict lives in the object store with an identity,
like any blob.

### 2.2 Resolutions are declared inputs

The declaration's third component:

```
env = base + ordered layers + resolution set
materialize(base, layers, resolutions) -> tree     # pure, deterministic, cacheable
```

A resolution object is keyed by the exact conflict identity (the
stage-OID triple). During a rebuild, hitting a conflict looks up the key,
splices the resolution blob, and continues. If either layer moves at all,
the triple changes, the key misses, and the conflict resurfaces fresh —
a stale fix is structurally impossible. This is hitch's exact-match
`--replay-resolutions` guarantee promoted from opt-in flag to core
mechanic.

Consequence: hitch's "one-time inclusion" wart disappears. A resolution
can only be in a build *by being an input*, so it is recorded by
construction. Silent one-off fixes cannot exist.

### 2.3 Two conflict classes, two routes

- **Layer × base** (base moved on after the layer diverged): the durable
  fix belongs *in the layer* — the layer owes a rebase. The system routes
  the obligation to the layer's owner; the layer is held meanwhile and a
  signed refusal record (§3.3) is emitted. A composition-level resolution
  would be the wrong home — it would haunt every environment forever.
- **Layer × layer** (peers; neither side can own the fix alone): this is
  the right home for a declared resolution input. A human resolves once
  in a detached render, the resolution object lands, and every
  environment whose composition produces the same triple reuses it.

Held-not-blocking is the default policy: an unresolved conflict excludes
that layer while the rest still materializes; `halt` remains available
per environment. Held state is a queryable refusal object, not silence.

### 2.4 Ordering

Order is part of the declaration, so "who conflicts with whom" is
well-defined: layer N conflicts with `composed(base..N-1)`, and pairwise
attribution names the actual colliding layer. Reordering is an explicit
declaration edit; changed intermediate trees change the triples, so
resolutions validated under the old order miss and their conflicts
resurface. Reordering cannot silently reuse a resolution validated under
a different stacking — safe by construction.

**Rejected:** auto-searching layer orderings until one composes clean.
Order is semantic priority; a clean compose under a silently different
order is not a correct compose.

### 2.5 Cross-environment behavior

The same peer conflict appearing in `dev` and `qa` produces the same
triple, so one resolution object serves both. Per-environment divergence
is allowed but explicit: declarations reference resolutions by id, so two
environments *may* pin different resolutions of the same conflict —
auditable weirdness, never silent drift. (This is the bounded, salvaged
kernel of "multi-valued conflicts"; the unbounded version — a standing
menu every consumer chooses from — is rejected as combinatorial
divergence.)

### 2.6 Conflict lifecycle: memory, speculation, debt

- **Pre-seeding (speculative provocation).** The promotion order is
  declared, so the next rebuild is computable before anyone runs it. CI
  pre-composes upcoming and plausible compositions, deposits conflict
  objects plus drafted resolutions (replayed / heuristic / LLM-proposed)
  awaiting sign-off. A human arriving at a conflict finds a proposal
  already waiting. Drafts never auto-fire; exact-match auto-fire is
  reserved for human-validated entries. Provocation is triaged by
  exposure likelihood (front of the promotion queue first), not run
  combinatorially.
- **Affinity maturation.** When a human tweaks a replayed fix before
  accepting, the edit is recorded as a refinement linked to the original
  entry — resolutions improve over repeated near-identical exposures
  instead of the store only growing disjoint entries.
- **Hunk-granular recycling.** Abandoned resolution attempts are
  decomposed into per-hunk resolved fragments offered as feedstock to the
  next attempt. Auto-assembly is permitted only where hunks are
  mechanically independent; anywhere hunks interact (renames, moved
  boundaries), assembly is propose-and-confirm, never propose-and-fire.
- **A regulatory layer retires bad memories.** Track how often a human
  overrides an auto-fired replay; past a threshold, demote that entry to
  suggest-only.
- **Debt semantics.** Every resolution object is visible debt — a standing
  statement that two layers still disagree. Debt has an age; an SLA gate
  (hitch's `doctor --max-resolution-age-days`, made enforcing) blocks
  further promotion past a threshold. Retirement means carrying the fix
  into a layer, after which the conflict stops occurring, the resolution
  object goes unreferenced, and GC collects it. Debt disappears by
  becoming unnecessary, not by deletion.

---

## 3. The attestation layer

The 2026 forcing function is AI authorship volume. Provenance is native
object model, not commit-trailer convention.

### 3.1 Attestation objects

A new content-addressed object type, sibling to commit/tree/blob:

```
{ tree_hash, principal_id, class: human|agent,
  semantics: accountable-for|wrote,
  decision: approve|refuse, reason, signature }
```

stored under `refs/attest/<tree-oid>/*`, so the only lookup is "does this
tree have attestations" — never a separate mutable status field.

### 3.2 Tree-hash binding makes invalidation free

Binding to the *tree* rather than the commit is what makes
self-invalidation structural: a no-op rebuild stamps a new commit but the
tree is pure content, so unchanged content keeps its attestations, and
any real change yields a tree with zero matching attestations.
Invalidation is non-referential-integrity, not a revocation broadcast.
(Same reason hitch's crash-recovery tests compare `^{tree}`, not commit
SHAs.)

Environment trees compose bottom-up: an environment attestation is either
synthesized (every current input tree carries a valid attestation and the
composition function is trusted) or a direct review of the composed tree.
The former scales with layer count and reuses per-branch review; the
latter is simpler but re-triggers full review on any input change.

### 3.3 Refusals are first-class

A held/excluded layer produces the same object shape with
`decision: refuse` and a reason — the decision *not* to ship becomes a
durable, timestamped, queryable fact that survives the branch's own
deletion, instead of the silent gap a failed check leaves today. The
refusal corpus is rare, high-signal, currently discarded by every VCS —
and is exactly the dataset needed to evaluate the AI reviewers the
principal-class rules govern.

### 3.4 Principal classes

Signer identity is established the way git already does it serverlessly —
an allowed-signers file — extended with a class tag per entry and a
semantics tag per attestation. Verification is pure local computation
over signed objects. Structural rules:

- an AI agent is a distinct principal class;
- an agent cannot self-approve;
- an agent cannot serve as the second signer for another agent in a chain
  requiring two accountable-for signatures;
- `accountable-for` and `wrote` are distinct claims — the resolver of a
  conflict authored neither side but owns the outcome (liability
  signature ≠ authorship signature);
- agent classes should be capability-scoped sub-types (diff-suggester vs
  tool-executing autonomous agent), since risk tracks capability, not
  non-human origin.

### 3.5 Enforcement boundary — stated honestly

Content-addressed objects are permissionless to construct, so enforcement
can only live at the ref-update boundary: a protected ref refuses to move
unless the CAS transaction carries a matching attestation set. In a
genuinely distributed deployment this is advisory-with-teeth-for-whoever-
enforces-it, not a protocol-level guarantee — the same honest bound as
hitch's `require_signed_resolutions` (trust rooted in control of the
checkout/ref-server, not in push/clone security).

Extensions: attestation time-decay (tree binding is blind to context
drift — CVEs, departed reviewers); policy-as-attested-object (the rule
for "what counts as reviewed" is itself a tree governed by the same
mechanism, so weakening policy is a signed, invalidatable event);
cross-repo provenance chaining (a vendored dependency's upstream
attestations are native inputs to the consumer's trust computation —
same object model where SBOM/SLSA/in-toto are side channels).

---

## 4. The semantic cost, attacked in five layers

Two layers that genuinely disagree still need a judgment. The cost
decomposes; most of it is removable; the residue is not a VCS problem.

| # | Attack | Mechanism | Cost fate |
|---|--------|-----------|-----------|
| 1 | False conflicts | AST-aware merge units (tree-sitter grammars); patch commutation as the formal conflict boundary (Darcs/Pijul) | eliminated |
| 2 | Under-specified conflicts | oracle-filtered resolution search: LLM proposes candidates, oracle filters (compiles, typechecks, **union of both layers' test suites passes**); survivor = draft awaiting sign-off; auto-land gated on oracle *coverage*, never model confidence | automated |
| 3 | Late discovery | edit-time contact inhibition (continuous speculative composition of open layers pings both owners at first overlap); intent declarations (layers declare the invariant they change; incompatible intents collide before code is written) | moved earlier, ~free |
| 4 | Genuine but deferrable | variation points: a conflict materializes as a first-class runtime choice (choice-calculus / product-line grounding); per-env selection; product decides with data; losing branch retired. Bounded use only — unbounded variation is combinatorial state | deferred to runtime data |
| 5 | Genuine product disagreement | two incompatible intended futures, both defensible: choosing is *new information entering the system*, not derivable from repo contents. System's whole job: name it, route it to the owner pair, hold everything else unblocked, never lose the decision (refusal record or signed resolution input) | irreducible — named, owned, never lost |

Layer 1 drains an estimated majority of today's conflict volume before
any judgment question arises. Layer 2's honest limit is oracle strength:
weak test suites let a machine confidently ship wrong semantics that
pass. The residue at layer 5 *should* hurt — it is the system telling two
humans they want different products.

Merge reducers are pluggable per type (a registry; "conflict" = a type
with no registered reducer); line-based text merge is the worst reducer,
a default, not destiny. The custom-merge-driver trust gap hitch documents
carries over: a configured reducer is a program the composition executes.

---

## 5. What is genuinely different from git

| git | Strata |
|-----|--------|
| branch = mutable pointer you repair | branch = cached materialization of a declaration |
| history = the commit graph | history = the declaration log; commit graph is compiled output |
| conflict = wedged working-tree state | conflict = content-addressed object |
| resolution = anonymous edits inside a merge commit | resolution = named input with provenance, signer, lifetime, debt semantics |
| re-merge = re-resolve from scratch (rerere = fuzzy local cache) | rebuild = pure function; resolutions replay exact-match or resurface |
| abort = state surgery (`rebase --abort`) | abort = drop the render; declaration untouched |
| review status = forge-side mutable metadata | attestation = content-addressed object bound to the tree it reviewed, self-invalidating |
| "did we ever resolve this?" = archaeology | resolution set = a readable list with age and owner |
| deploy gate = branch protection bolted on | no CI verdict in the derivation key = nothing to alias = structurally unpublishable |

---

## 6. Load-bearing risks

1. **Determinism of derivation.** The entire model rests on rebuilds
   being cheap and reproducible. Hitch already hit the edge (`commit-tree`
   stamps wall-clock time; crash tests must compare trees, not SHAs).
   Scaled to expensive builds and non-deterministic reducers, every
   headline promise collapses back into hand-repair behind an unfamiliar
   cache layer. **This is the first thing to falsify (§8).**
2. **Trust-root relabeling.** The principal-class tag lives in the same
   locally-mutable trust root as signer identity. Whoever can edit the
   allowed-signers file can relabel an agent as human and grant it
   self-approval. Only auditable after the fact via that file's own
   (attested) history.
3. **Autoimmunity in fine-grained resolution reuse.** Whole-file
   bit-identical triples encode a human's full judgment; hunk-level
   hashes discard surrounding context, so an exact-*looking* match can
   fire against a situation it was never validated for as a whole. Hence
   propose-and-confirm below file granularity, and the regulatory
   demotion layer.
4. **Oracle weakness.** §4 layer 2's auto-land is only as safe as test
   coverage. Gate on measured oracle strength.

## 7. Rejected traps (kept for the record)

- **Global hunk approval by content hash** — same bytes, different
  context, different meaning; an approval-laundering vector.
- **Auto-reordering promotions until clean** — order is semantic
  priority; silent reorder changes meaning (§2.4).
- **Unbounded multi-valued conflicts** — combinatorial downstream
  divergence; only the per-env bounded form survives (§2.5, §4 layer 4).
- **Paging the operation's author at 3am** — attribution and async
  notification yes; rerouting the page to someone asleep, no.

## 8. Prototype path (all inside hitch, cheapest falsification first)

1. **Derivation cache** (falsifies risk 1): hash
   `(declaration, base OID, ordered layer OIDs)` into a key; look up
   `refs/derivation/<key>`; on miss, compute via the existing
   `merge_tree_compose` + `commit_tree` path; publish = CAS from the
   cache ref. Then measure one number: do repeated builds of identical
   inputs actually hit the cache?
2. **Attestations**: `attestations.rs` beside `resolutions.rs`; records
   under `refs/hitch/attestations/<tree-oid>/*`; reuse
   `verify_signature_ssh` + a class column in the allowed-signers file;
   gate `hitch rebuild`'s post-compose step on a valid attestation before
   `publish_branch` runs.
3. **Hunk-recurrence miner** (falsifies the fragment-recycling premise):
   mine a real repo's merge history, hash conflicting hunks'
   base/ours/theirs triples independently of their files, measure
   hunk-level recurrence across conflicts that differ at whole-file
   level. Rare recurrence ⇒ don't build the recycler.

## 9. Open directions (not yet designed)

- Cross-repo declarations: an environment as a bundle
  (`service A @ X, service B @ Y, config @ Z`) materialized and cached
  identically — native where GitOps tooling bolts it on.
- Time-travel as pure query: evaluate a declaration at historical input
  OIDs with no ref created; environment bisection becomes a function
  call.
- Sync model: repos exchanging declarations and continuously reconciling,
  rather than push/pull of materializations.
- The provocation: if a branch is derived output, why is a release still
  a merge? Production as a permanent environment whose declaration only
  grows; `main` as its materialization; "shipped" = attested membership.
  The product's history becomes the history of one declaration file.

## 10. Evolution log

- **v0.1 (2026-08-28):** initial capture from brainstorm — derivation
  core, conflict/resolution model, attestation layer, five-layer semantic
  cost analysis, risks, traps, prototype path.
