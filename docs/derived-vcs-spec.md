# Strata — a derived-branch VCS (design spec)

**Status:** SHELVED, v0.2 — 2026-08-28. Explored to a deliberate "don't
build." Verdict: Phase 1 (the daily-driver core) is jj's already-shipped
product — working-copy-as-change, change IDs, op log/undo, first-class
conflicts, revsets all exist there, and jj's conflict algebra beats §5.1.
The genuine differentiators (Phases 2–3: composition engine, trust layer)
require team-level buy-in and are better built as a layer on jj — i.e.
hitch's own pattern on a better substrate — if ever. Kept as a record of
the design space and the reasoning; §5 (resolution-as-input), §6
(attestations), and the risk/trap registers remain quarry for future
hitch work.
**Working name:** "Strata" (placeholder, unbranded — rename freely).
**Origin:** ADHD brainstorm session over the hitch codebase, then a
free-reign "what would a truly modern replacement be" pass. Not a hitch
implementation plan — a from-scratch git-replacement thought experiment
that takes hitch's ideas as native primitives instead of a layer.

**Thesis (sharpened in v0.2):** git stores outputs and throws away the
program. Strata stores the program — a *declaration* — and treats every
branch, commit, and checkout as regenerable compiled output. One
recursive primitive at every scale: a declaration composes ordered
layers. An environment is a stack of branches; a branch is a stack of
changes; a change is a stack of edits. Layers all the way down.

---

## 0. What v0.2 keeps from v0.1, and why

v0.1 fixed git's data model. v0.2 adds what it undersold: daily
ergonomics, agents as coworkers, forge ownership, and the repo boundary.
Explicit disposition of v0.1 material:

**Carried forward whole (the load-bearing inventions):**

- The derivation core — declarations as source of truth, branches as
  memoized materializations, publish as pointer alias (§2.1, §2.6).
  Without this nothing else works.
- Conflict-as-value / resolution-as-declared-input (§5). The single best
  idea in v0.1: it deletes git's wedged-workspace conflict UX *and*
  hitch's one-time-inclusion wart in one move, and makes rebuilds pure.
- Two conflict classes with different routing (layer×base owes a rebase;
  layer×layer owns a composition-level resolution) (§5.3).
- Held-not-blocking as default policy, with signed refusal records
  (§5.3, §6.3). "What didn't ship and why" as durable data.
- Resolution debt semantics — visible, aged, SLA-gated, retired by
  becoming unnecessary (§5.5).
- Ordering safety: resolutions keyed by content triples miss on reorder,
  so a resolution validated under one stacking can never silently apply
  under another (§5.4).
- The attestation layer's core mechanics — content-addressed attestation
  objects, structural self-invalidation, principal classes, agent
  signing rules, the honest enforcement boundary (§6).
- The five-layer semantic-cost analysis (§10). Unchanged as analysis;
  one layer demoted as a shipping feature (below).
- All four load-bearing risks and all four rejected traps. Retained
  verbatim so we don't re-walk into them (§12, §13).
- The hitch-based falsification prototypes (§14). Still the cheapest
  experiments; still worth running before any greenfield line is
  written.

**Revised by v0.2:**

- Attestation binding: v0.1 bound only to tree hash. Now dual-keyed —
  tree hash for *validity*, change identity for *lineage* (§2.3, §6.2).
- Checkouts-as-renders: subsumed into the change model — a render's
  edits *are* an anonymous change being derived in reverse (§2.4).
- Ops-as-objects-with-heartbeats: subsumed by the universal operation
  log, which is strictly stronger (§2.7).
- Cross-repo declarations and time-travel queries (v0.1 "open
  directions"): no longer separate features — corollaries of repo-as-view
  (§8) and the query language (§9.4) respectively.

**Demoted (kept in the record, presumption against shipping):**

- Choice-calculus variation points → appendix-grade experiment. The
  theory is sound; teams with unbounded variation state are not (§13).
- Hunk-granular resolution recycling → stays gated behind the
  recurrence miner (§14.3), and the presumption is now *against*
  building it even on a positive result unless the margin is large.

**Nothing from v0.1 is cut outright.** Its skeleton was right; v0.2 is
flesh, not surgery.

---

## 1. Design targets: the modern developer, 2026

Named explicitly because they drive every v0.2 addition:

1. Works alongside AI agents daily — often several at once, in the same
   repository, in parallel with their own edits.
2. Learned git as incantations, not a model; fears rebase, force-push,
   detached HEAD. Fear is git's real UX tax.
3. Lives in stacked-diff / small-change workflows that git's branch
   model actively fights.
4. Has their entire review and discussion history held hostage by a
   forge vendor.
5. Works in repos where clone size, media/ML assets, and mono-vs-poly
   architecture debates are daily friction.
6. Will never configure GPG. Security defaults nobody configures are
   the only security defaults that exist.

---

## 2. The core model

### 2.1 Stored vs. derived

Two stored kinds:

1. **Content-addressed objects** — blobs, trees, commits (plus the new
   object types below). Git's object database is the one part worth
   keeping intact.
2. **Declarations** — small structured documents, a tiny build
   language: a base plus an ordered list of layers, plus a resolution
   set (§5.2). The generalization of `hitch.json` from config sidecar
   to first-class source of truth.

Everything else is a **memoized derivation**: cache entries keyed on
`(declaration hash, resolved input OIDs)` mapping to tree/commit OIDs,
stored under `refs/derivation/*` (generalizing hitch's
`refs/hitch/build/*` anchors) — GC-safe, never hand-edited.

History bifurcates: the **declaration log** is the real audit trail
(small, causal, human-authored: "promoted B before C"); the materialized
commit graph is compiled output, regenerable wholesale without that
being "history rewriting."

### 2.2 No staging area. The working copy is a change.

Every save auto-journals into an anonymous **change** object. There is
no index, no stash, no "forgot to add," no dirty-tree error — commands
never demand a clean tree because the tree is already a change. Git's
`commit` decomposes into two honest verbs:

- **describe** — name the change when it means something (message,
  intent declaration §10.3, ownership).
- **publish** — CAS the change into a declaration (a branch's stack, an
  environment's list).

jj proved working-copy-as-commit converts git users in a day. This is
table stakes for a 2026 tool, not a research bet.

### 2.3 Dual identity: change ID and tree hash

Every change carries two identities with different jobs:

- **Change ID** — stable logical identity (jj/Gerrit-style) that
  survives rebase, amend, split, and reordering. Reviews, CI verdicts,
  attestations, and discussion attach here. This is *lineage*: "this is
  v7 of the thing Sarah reviewed at v3."
- **Tree hash** — content identity. Attestation *validity* binds here
  and self-invalidates structurally on any content change (§6.2).

The split kills the force-push-orphans-my-review class entirely: rewrite
the content freely, the conversation and the accountability chain follow
the change ID, while approval validity correctly resets with the tree.

### 2.4 The recursive layer stack

One primitive, three scales:

```
environment = declaration over ordered branches   (hitch's model)
branch      = declaration over ordered changes    (a stack, natively)
change      = journal of edits in a render        (§2.2)
```

Stacked-diff workflow is not a feature — it is what a branch *is*.
Splitting, reordering, and rebasing a stack are declaration edits, and
every guarantee defined for environment layers (conflict objects,
resolution inputs, ordering safety, held-not-blocking) applies uniformly
one level down.

A **render** (working directory) is a disposable materialization of any
declaration — opened and closed like a browser tab, optionally
FUSE-lazy so a monorepo checkout costs nothing until a file is read. No
render is ever authoritative; hitch's "the user's working tree is
sacred / nothing builds in the checkout" generalizes to: every checkout
is a cache the system may re-render but never needs to repair.

### 2.5 Anonymous by default

Changes need no branch. Branches are optional labels over stacks, not
workspace prerequisites — no detached-HEAD concept, no checkout dance
before starting work. Start typing; describe and publish when it means
something.

### 2.6 Publish = pointer alias onto a ghost

Builds materialize and test on shadow refs first. CI tests a
*derivation* and caches the verdict against its key; a green shadow ref
already is the artifact, and promotion to the real branch is a
zero-compute CAS pointer alias — the same atomic ref transaction plus
crash-journal obligation model as hitch's `publish_branch`. There is
never a build step between "approved" and "live."

CI verdicts fold into the derivation key ("this tree, green as of
suite-hash X"): a promotion with no verdict has nothing to alias to.
Branch protection stops being a bolted-on rule and becomes a structural
property — an unverified tree is *unpublishable*, not forbidden.

### 2.7 The operation log, and universal undo

Every mutation of refs, declarations, and metadata appends to an
**operation log**. `undo` works on *anything* — a publish, a
declaration edit, a resolution recording, an undo. This is not history
rewriting; it is meta-level time travel over the declaration log, which
is itself versioned.

Consequences: the entire 3am incident class (v0.1 §1.6) collapses into
one command; in-progress operations are ordinary log entries with
liveness heartbeats (no `.git/rebase-merge` folklore, "stuck vs. slow"
is one query); concurrent publishes serialize via CAS and a lost race is
a readable log entry, not an incident. Destructive operations preview
blast radius in human terms ("this orphans Sarah's Tuesday WIP"), never
only hashes.

Remote push remains a detached, journal-tracked background obligation;
the local ref transaction is the entire perceived latency.

---

## 3. Concurrency: agents as coworkers

The 2026 daily reality: N agents plus a human in the same repository
simultaneously. Designed for, not retrofitted:

- **Workspace = session.** Each agent (and each human terminal) gets its
  own render and its own anonymous change lineage. There is no shared
  mutable branch state to fight over — publishing is CAS onto a
  declaration; losers retry cleanly against the new state.
- **Territory leases.** A session declares intent ("editing `auth/`") as
  a visible, expiring object. Contact inhibition (§10.3) applies
  agent-to-agent and agent-to-human: two sessions converging on the same
  module are pinged at first overlap, when negotiation costs one
  sentence — not at merge time, after both entrenched.
- **Attribution is native shape.** Every change carries principal and
  class (§6.4): `wrote: agent-x, accountable-for: martin` is the normal
  form of AI-assisted work, not a trailer convention.
- **The merge queue is native.** Ghost-ref materialization (§2.6) means
  agents never race the real branch; they race for a place in a
  declaration, and the queue is just the declaration's pending edits.

---

## 4. Identity: zero-config, device-rooted

Signing is default-on via device keys — keychain/enclave-backed SSH-style
keys enrolled per device, chained to a person or agent identity.
`user.email` as identity was always forgeable string theater. Nobody
configures GPG; therefore the design assumes nobody will, and makes the
signed path the zero-config path. Key rotation and device revocation are
declaration edits on an identity object, logged and attestable like
everything else. (Transparency-log anchoring is an open direction, §15.)

---

## 5. The merge model: conflict is a value, resolution is an input

Unchanged from v0.1 in substance — restated here compactly because
everything else leans on it.

### 5.1 Conflict objects

Composition is a pure fold: `materialize(base, [L1, L2, ...])`, each
step an ORT-style tree merge in the object database (hitch's
`merge_tree_compose` path — no worktree, no index). A merge step never
fails: output is a clean tree or a **conflict object** — typed,
content-addressed `{base, ours, theirs}` stage triples per path, with a
per-hunk decomposition alongside. Nothing wedges; a conflict has an
identity, like any blob.

### 5.2 Resolutions are declared inputs

```
env = base + ordered layers + resolution set
materialize(base, layers, resolutions) -> tree    # pure, deterministic, cacheable
```

A resolution is keyed by its exact conflict triple. Rebuilds splice it
on exact match; if either side moves, the key misses and the conflict
resurfaces fresh — a stale fix is structurally impossible (hitch's
exact-match replay guarantee, promoted to core mechanic). A resolution
can only be in a build *by being an input*, so it is recorded by
construction — silent one-off fixes cannot exist.

### 5.3 Two classes, two routes

- **Layer × base**: the layer owes a rebase; the durable fix belongs in
  the layer. Routed to the owner; held meanwhile; signed refusal record
  emitted.
- **Layer × layer**: neither side can own the fix — the resolution input
  lives at composition level, resolved once in a detached render, reused
  by every environment producing the same triple.

Held-not-blocking is default (one unresolved layer never blocks the
rest); `halt` per environment. Held state is queryable refusal data,
never silence.

### 5.4 Ordering safety

Order is declaration. Layer N conflicts with `composed(base..N-1)`;
attribution names the actual colliding layer. Reorder changes
intermediate trees, changes triples, misses keys — conflicts resurface
rather than silently reusing resolutions validated under a different
stacking.

### 5.5 Lifecycle: memory, speculation, debt

- **Pre-seeding**: promotion order is declared, so upcoming compositions
  are computable early. CI provokes them, deposits conflict objects and
  drafted resolutions (replay/LLM) awaiting sign-off — triaged by
  exposure likelihood, never combinatorially. Drafts never auto-fire;
  auto-fire is reserved for human-validated exact matches.
- **Affinity maturation**: a human tweak to a replayed fix records a
  linked refinement, so entries improve over near-identical exposures.
- **Regulatory retirement**: entries whose auto-fires get overridden
  past a threshold are demoted to suggest-only.
- **Debt**: every resolution object is visible, aged debt ("these two
  layers still disagree"), SLA-gated (hitch's
  `doctor --max-resolution-age-days`, made enforcing). Retirement =
  carry the fix into a layer; the conflict stops occurring, the object
  goes unreferenced, GC collects it. Debt disappears by becoming
  unnecessary.

### 5.6 Merge reducers per type

A registry of typed merge reducers; "conflict" = a type with no reducer
(or a reducer that gated shut). Line-based text merge is the worst
reducer — a default, not destiny. AST-aware reducers (tree-sitter
grammars) eliminate the false-conflict majority (§10.1); semantic diff
is the default review presentation, line view the fallback. The
custom-merge-driver trust gap hitch documents carries over: a configured
reducer is a program composition executes — which is why reducers are
declared and sandboxed automation (§8), never ambient repo-local config.

---

## 6. The attestation layer

### 6.1 Attestation objects

Content-addressed, sibling to commit/tree/blob:

```
{ change_id, tree_hash, principal_id, class: human|agent,
  semantics: accountable-for|wrote,
  decision: approve|refuse, reason, signature }
```

stored under `refs/attest/<tree-oid>/*` and indexed by change ID. The
only status lookup is "does this tree have attestations" — never a
mutable status field.

### 6.2 Validity vs. lineage

Tree-hash binding makes invalidation free and structural: a no-op
rebuild keeps its attestations (tree unchanged); any real change yields
a tree with zero matching attestations. No revocation broadcast.
Change-ID indexing preserves continuity across rewrites: "v7 is
unapproved, and here is the approved v3 it descends from, with the
conversation attached." Environments compose attestations bottom-up:
synthesized from per-layer attestations plus a trusted composition
function, or a direct review of the composed tree.

### 6.3 Refusals are first-class

A held layer, a rejected review, a declined promotion — same object
shape, `decision: refuse`, with a reason. The decision *not* to ship is
durable, timestamped, queryable, and survives branch deletion. The
refusal corpus is rare, high-signal, discarded by every current VCS —
and is exactly the evaluation data for the AI reviewers these rules
govern.

### 6.4 Principal classes

Identity from the device-key chain (§4), class-tagged. Structural rules,
verified as pure local computation over signed objects:

- an AI agent is a distinct principal class, capability-scoped into
  sub-types (diff-suggester ≠ tool-executing autonomous agent — risk
  tracks capability, not non-human origin);
- an agent cannot self-approve;
- an agent cannot be the second signer in a chain requiring two
  `accountable-for` signatures;
- `accountable-for` ≠ `wrote`: a conflict's resolver authored neither
  side but owns the outcome.

### 6.5 Enforcement boundary — stated honestly

Content-addressed objects are permissionless to construct; enforcement
lives at the ref-update boundary. A protected ref refuses to move unless
the CAS carries a matching attestation set. Distributed, this is
advisory-with-teeth-for-whoever-enforces-it, not a protocol guarantee —
the same honest bound as hitch's `require_signed_resolutions` (trust
rooted in control of the ref server / checkout, not in transport
security).

Extensions: attestation time-decay (tree binding is blind to context
drift — CVEs, departed reviewers); policy-as-attested-object (the rule
for "what counts as reviewed" is itself governed by the mechanism it
defines, so weakening policy is a signed, invalidatable event);
cross-repo provenance chaining (a dependency's upstream attestations are
native inputs to the consumer's trust computation — one object model
where SBOM/SLSA/in-toto are side channels).

---

## 7. The forge, absorbed

Reviews, comments, threads, and issue links are content-addressed
objects in the repository, attached to change IDs, synced like
everything else. Attestations already live there; discussion is the same
shape with softer semantics.

The forge shrinks to three honest jobs:

1. **Dumb replication host** — availability and bandwidth.
2. **Policy enforcer at the ref boundary** — the one place enforcement
   is real (§6.5).
3. **Compute vendor** — CI runners, speculative provocation sweeps,
   ghost materializations.

Everything social is portable: leave a vendor, lose nothing. Review
history stops being hostage data. This is the ownership problem modern
developers actually have and never name.

---

## 8. Storage, scale, and the repo boundary

- **Repo = view, not storage unit.** One content-addressed pool; a
  "repo" is a declaration over a subtree with ACLs. Mono-vs-polyrepo
  dissolves: same pool, different views. Cross-repo environments
  (`service A @ X, service B @ Y, config @ Z`) stop being a feature and
  become a corollary — one declaration composing across views,
  materialized and cached like any other.
- **Big files native.** Content-defined chunking (FastCDC-style) makes
  media and ML weights ordinary lazy objects — streamed on read, deduped
  across versions. LFS was an apology; delete the apology.
- **Lazy everywhere.** Renders resolve files on first read (§2.4);
  clones fetch declarations plus the object frontier, not history.
  History depth is a query concern (§9), not a clone concern.
- **Declarations as lockfiles.** A layer can be pinned (exact OID) or a
  resolvable range ("latest passing"); `update` re-resolves and shows
  the diff before writing. Lockfile-diff review is a trust pattern
  developers already have.

---

## 9. The query language

History is a database; treat it like one. Revsets (jj/Mercurial)
generalized over changes, declarations, attestations, and operations:

```
changes touching auth/ since v3.2 where attested-by:agent and not attested-by:human
resolutions older-than 30d in env:qa
operations by:agent-x last 24h
layers in env:dev held
```

- One-liner queries are the difference between the attestation layer
  being governance and being theater.
- Time travel is a query, not a feature: evaluate any declaration at
  historical input OIDs with no ref created. Environment bisection
  becomes a function call.
- The op log (§2.7), the debt ledger (§5.5), and the refusal corpus
  (§6.3) are all just tables.

---

## 10. The semantic cost, attacked in five layers

Two layers that genuinely disagree still need a judgment. The cost
decomposes; most is removable; the residue is not a VCS problem.

| # | Attack | Mechanism | Cost fate |
|---|--------|-----------|-----------|
| 1 | False conflicts | AST-aware merge reducers; patch commutation as the formal conflict boundary (Darcs/Pijul) | eliminated |
| 2 | Under-specified conflicts | oracle-filtered resolution search: LLM proposes, oracle filters (compiles, typechecks, **union of both layers' test suites passes**); survivor = draft awaiting sign-off; auto-land gated on oracle *coverage*, never model confidence | automated |
| 3 | Late discovery | edit-time contact inhibition (§3's leases + continuous speculative composition ping both owners at first overlap); intent declarations (a described change names the invariant it alters; incompatible intents collide before code exists) | moved earlier, ~free |
| 4 | Genuine but deferrable | variation points: conflict materializes as a runtime choice, per-env selection, product decides with data. **Demoted to appendix experiment** — theory sound (choice calculus / product lines), unbounded variation state is a trap | deferred — experimental only |
| 5 | Genuine product disagreement | two incompatible intended futures, both defensible: choosing is *new information entering the system*, not derivable from repo contents. The system's whole job: name it, route it to the owner pair, hold everything else unblocked, never lose the decision (refusal record or signed resolution input) | irreducible — named, owned, never lost |

Layer 1 drains the majority of today's conflict volume before any
judgment question arises. Layer 2's honest limit is oracle strength —
weak suites let a machine confidently ship wrong semantics that pass.
Layer 5's residue *should* hurt: it is the system telling two humans
they want different products.

---

## 11. What is genuinely different from git

| git | Strata |
|-----|--------|
| branch = mutable pointer you repair | branch = cached materialization of a declaration |
| history = the commit graph | history = the declaration log; the graph is compiled output |
| staging area, stash, dirty-tree errors | working copy is a change; no index, nothing to forget |
| commit SHA is the only identity | change ID (lineage) + tree hash (validity) |
| rebase/force-push orphans reviews | conversation and accountability follow the change ID |
| conflict = wedged working-tree state | conflict = content-addressed object |
| resolution = anonymous edits inside a merge commit | resolution = named input with provenance, signer, lifetime, debt |
| re-merge = re-resolve (rerere = fuzzy local cache) | rebuild = pure function; resolutions replay exact-match or resurface |
| abort = state surgery (`rebase --abort`) | abort = drop the render; `undo` = one command, works on everything |
| reflog per-ref, folklore recovery | universal operation log, meta-level time travel |
| review status = forge-side mutable metadata | attestation = object bound to the tree it reviewed, self-invalidating |
| forge owns reviews, comments, issues | social graph is repo objects; forge = host + policy + compute |
| repo = storage unit; LFS bolted on | repo = ACL'd view over one pool; chunked blobs native |
| hooks = ambient-authority code execution | automation declared, attested, sandboxed (§8 of v0.1 hardening class deleted structurally) |
| identity = `user.email` string | identity = device-key chain, signing default-on |
| deploy gate = branch protection bolted on | no CI verdict in the derivation key = nothing to alias = unpublishable |

*(Automation note: there is no repo-local hook mechanism at all.
Automation — including merge reducers — is declared in attested
declarations, runs sandboxed with no ambient credentials. Kills the
malicious-hook / hostile-config class hitch defends against flag-by-flag
with `HARDENING_ARGS`, structurally instead.)*

---

## 12. Load-bearing risks

Carried from v0.1 (1–4), extended (5–7):

1. **Determinism of derivation.** Everything rests on rebuilds being
   cheap and reproducible. Hitch already hit the edge (`commit-tree`
   stamps wall-clock; crash tests must compare trees, not SHAs). Scaled
   to expensive builds and nondeterministic reducers, every promise
   collapses into hand-repair behind an unfamiliar cache. **First thing
   to falsify (§14.1).**
2. **Trust-root relabeling.** Principal class lives in the same
   locally-mutable trust root as identity; whoever edits it can relabel
   an agent as human. Device-key chains (§4) raise the bar but the
   class *assignment* still needs an attested policy object, and
   auditability remains after-the-fact.
3. **Autoimmunity in fine-grained resolution reuse.** Hunk-level hashes
   discard the context that made whole-file exact-match safe. Hence
   propose-and-confirm below file granularity, regulatory demotion, and
   the presumption against building the recycler at all.
4. **Oracle weakness.** §10 layer 2's auto-land is only as safe as
   measured test coverage.
5. **Op-log scale and privacy.** Every save journals (§2.2) and every
   mutation logs (§2.7). Needs aggressive compaction/expiry tiers, and
   an answer for "my 4pm flailing is now queryable by my manager."
   Anonymous-change journals should default to device-local until
   described.
6. **View-ACL enforcement.** Repo-as-view (§8) puts access control on
   views over a shared pool; content-addressing makes object *existence*
   leak across views unless the pool is partitioned per ACL domain.
   Needs real design, not hand-waving.
7. **Forge-absorption sync.** Review/discussion objects written
   concurrently by many parties need merge semantics of their own
   (append-only sets commute; edits don't). Declarations face the same:
   concurrent promotes by two people should merge as operations
   (disjoint promote/demote commute), not conflict spuriously.

## 13. Rejected and demoted (kept for the record)

Rejected (v0.1, still rejected):

- **Global hunk approval by content hash** — same bytes, different
  context, different meaning; approval laundering.
- **Auto-reordering layers until clean** — order is semantic priority;
  a clean compose under a silently different order is not correct.
- **Unbounded multi-valued conflicts** — combinatorial divergence; only
  the bounded per-env pin survives (§5.2's resolution references).
- **Paging the operation's author at 3am** — attribution and async
  notification yes; rerouting the page to someone asleep, no.

Demoted (v0.2):

- **Choice-calculus variation points** — appendix experiment only.
- **Hunk-granular resolution recycling** — gated behind §14.3's miner,
  presumption against.

## 14. Build order

Greenfield priority (why jj converts people in a day — parity first,
decade bets second):

- **Phase 1 — the daily-driver core:** working-copy-as-change + no
  staging (§2.2), dual identity (§2.3), op log + universal undo (§2.7),
  recursive stacks (§2.4). A developer must feel the fear-tax vanish in
  the first hour.
- **Phase 2 — the composition engine:** declarations, derivation cache,
  ghost refs, conflict/resolution model (§2.1, §2.6, §5). This is the
  hitch DNA and the actual differentiator.
- **Phase 3 — the decade bets:** attestation layer + agent concurrency
  (§3, §6), forge absorption (§7), repo-as-view + chunked storage (§8),
  query language (§9).

Falsification prototypes, all inside hitch, all cheaper than any
greenfield work — run these first:

1. **Derivation cache** (falsifies risk 1): key
   `(declaration, base OID, ordered layer OIDs)` →
   `refs/derivation/<key>`; miss computes via the existing
   `merge_tree_compose` + `commit_tree` path; publish = CAS from the
   cache ref. Measure one number: do identical inputs actually hit?
2. **Attestations**: `attestations.rs` beside `resolutions.rs`; records
   under `refs/hitch/attestations/<tree-oid>/*`; reuse
   `verify_signature_ssh` plus a class column in allowed-signers; gate
   `hitch rebuild`'s post-compose step on a valid attestation before
   `publish_branch`.
3. **Hunk-recurrence miner** (falsifies the recycling premise): mine a
   real repo's merge history, hash conflicting hunks' triples
   independently of their files, measure hunk-level recurrence across
   conflicts that differ at whole-file level.

## 15. Open questions

- Sync model between pools: declarations exchanged and continuously
  reconciled rather than push/pull of materializations — how much CRDT
  machinery do declaration edits actually need beyond commuting
  promote/demote?
- Transparency-log anchoring for the identity chain (§4): worth the
  operational weight, or org-optional?
- Object-existence privacy across views (risk 6).
- Op-log retention/privacy tiers (risk 5).
- The release provocation, still open: if a branch is derived output,
  why is a release still a merge? Production as a permanent environment
  whose declaration only grows; `main` as its materialization;
  "shipped" = attested membership. The product's history becomes the
  history of one declaration file. What breaks first — blame, or the
  org chart?

## 16. Evolution log

- **v0.1 (2026-08-28):** initial capture from brainstorm — derivation
  core, conflict/resolution model, attestation layer, five-layer
  semantic-cost analysis, risks, traps, prototype path.
- **v0.2-shelved (2026-08-28):** decision not to build, after the jj
  comparison. Phase 1 = jj parity (already shipped there); Phases 2–3
  need team buy-in and fit better as a jj-substrate layer, evidence
  first. Doc retained as design-space record and idea quarry.
- **v0.2 (2026-08-28):** free-reign modernization pass. Added: no
  staging / working-copy-as-change, dual change-ID+tree identity,
  universal op log + undo, recursive layer stacks (env→branch→change),
  agent-native concurrency (sessions, leases, native attribution),
  zero-config device-key identity, forge absorption, repo-as-view +
  chunked storage, query language, structural no-hooks automation.
  Revised: attestation binding to dual-key; checkouts subsumed into
  change model; ops-objects subsumed into op log; cross-repo and
  time-travel recast as corollaries. Demoted: variation points,
  hunk recycling. Added risks 5–7. Explicit v0.1 disposition in §0.
