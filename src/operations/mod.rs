//! The operation model: plan → apply → receipt.
//!
//! Read this header before adding a planner, because two of its claims are
//! load-bearing and both are easy to get backwards.
//!
//! **A plan is a decision that has already been made, not a recipe.** By the
//! time a plan exists, every merge has been run: the trees and the composed
//! commit already exist in the object database. A planner is therefore a
//! *summariser* of work already done, not a description of work yet to be
//! attempted. It does not matter much for correctness, but it matters a great
//! deal for what a plan may honestly claim — the plan can name the exact
//! commit it will land, because it built it.
//!
//! **Which is why re-planning can differ from the plan a human read.** The
//! composition path uses `git commit-tree`, which stamps the ambient
//! wall-clock time into the commit, so composing the same inputs twice yields
//! two different SHAs with identical trees. A second planner invocation is
//! therefore not a way to "refresh" a plan; it produces a *different object
//! with the same content*. That is precisely the gap
//! [`model::PlanFingerprint`] closes, and it is why [`model::OperationPlan`]
//! carries the composed commit forward rather than a description of how to
//! recompose it. A plan that had to be re-evaluated to be applied could not
//! be shown to a human at all.
//!
//! **A receipt is a record, never a prediction.** Every effect in
//! [`model::ExecutionReceipt`] is read back from the repository after the
//! transaction that performed it, not copied out of the plan. A predicted SHA
//! and an observed SHA are different claims, and conflating them is how a tool
//! ends up reporting "fully synced" about a push that failed.
//!
//! One further boundary, enforced by convention rather than by the type
//! system: **this is the only place a plan may be built.** A command module
//! composes arguments and calls a planner; it does not assemble an
//! [`model::OperationPlan`], and it does not hand-construct a
//! [`utils::git_operations::RefEdit`] for a plan's own bookkeeping. Everything
//! hitch plans is planned here, so there is exactly one answer to "what is
//! hitch about to do" for any given command.
//!
//! Scope note: a fingerprint gates staleness; it does not authorise. Nothing
//! in this module accepts a plan from the caller, and no plan is ever read
//! back from disk or from a flag. It is always core-generated in this process,
//! and the repo-wide lock is held across the whole plan → apply window for
//! mutating commands, so the fingerprint is defence in depth against the
//! things the lock cannot cover (remote refs, and another user's push), not
//! the primary mechanism.

pub mod model;
pub mod rebuild;
