//! The typed event model for `hitch log`: what changed between two configs.
//!
//! Pure: two `HitchConfig` values in, events out. No git, no clock, no words —
//! wording belongs to `core::render`.

use crate::operations::model::HoldPair;
use crate::types::{ApprovalRequest, ApprovalStatus, HitchConfig, Operation};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HitchEvent {
    EnvironmentCreated {
        environment: String,
        base: String,
    },
    EnvironmentRemoved {
        environment: String,
    },
    BaseChanged {
        environment: String,
        from: String,
        to: String,
    },
    Promoted {
        environment: String,
        branch: String,
    },
    Demoted {
        environment: String,
        branch: String,
    },
    Locked {
        environment: String,
        by: Option<String>,
    },
    Unlocked {
        environment: String,
    },
    Rebuilt {
        environment: String,
        outcome: RebuildOutcome,
    },
    Released {
        environment: String,
    },
    ApprovalRequested {
        request_id: String,
        environment: String,
        branch: String,
        direction: ApprovalDirection,
    },
    ApprovalVoted {
        request_id: String,
        environment: String,
        branch: String,
        approvals: usize,
        required: usize,
    },
    ApprovalGranted {
        request_id: String,
        environment: String,
        branch: String,
    },
    ApprovalRejected {
        request_id: String,
        environment: String,
        branch: String,
    },
    ApprovalApplied {
        request_id: String,
        environment: String,
        branch: String,
    },
    ApprovalCancelled {
        request_id: String,
        environment: String,
        branch: String,
    },
}

/// The event's own vocabulary for `Operation`, which serializes PascalCase
/// because it is persisted in `hitch.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDirection {
    Promote,
    Demote,
}

impl From<Operation> for ApprovalDirection {
    fn from(op: Operation) -> Self {
        match op {
            Operation::Promote => ApprovalDirection::Promote,
            Operation::Demote => ApprovalDirection::Demote,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RebuildOutcome {
    /// No build record can be proven to describe this rebuild. Never read as "clean".
    Unrecorded,
    Clean {
        included: Vec<String>,
    },
    WithHolds {
        included: Vec<String>,
        held: Vec<HoldPair>,
    },
}

impl HitchEvent {
    pub fn environment(&self) -> &str {
        match self {
            HitchEvent::EnvironmentCreated { environment, .. }
            | HitchEvent::EnvironmentRemoved { environment }
            | HitchEvent::BaseChanged { environment, .. }
            | HitchEvent::Promoted { environment, .. }
            | HitchEvent::Demoted { environment, .. }
            | HitchEvent::Locked { environment, .. }
            | HitchEvent::Unlocked { environment }
            | HitchEvent::Rebuilt { environment, .. }
            | HitchEvent::Released { environment }
            | HitchEvent::ApprovalRequested { environment, .. }
            | HitchEvent::ApprovalVoted { environment, .. }
            | HitchEvent::ApprovalGranted { environment, .. }
            | HitchEvent::ApprovalRejected { environment, .. }
            | HitchEvent::ApprovalApplied { environment, .. }
            | HitchEvent::ApprovalCancelled { environment, .. } => environment,
        }
    }

    pub fn branches(&self) -> Vec<&str> {
        match self {
            HitchEvent::Promoted { branch, .. }
            | HitchEvent::Demoted { branch, .. }
            | HitchEvent::ApprovalRequested { branch, .. }
            | HitchEvent::ApprovalVoted { branch, .. }
            | HitchEvent::ApprovalGranted { branch, .. }
            | HitchEvent::ApprovalRejected { branch, .. }
            | HitchEvent::ApprovalApplied { branch, .. }
            | HitchEvent::ApprovalCancelled { branch, .. } => vec![branch],
            HitchEvent::Rebuilt { outcome, .. } => match outcome {
                RebuildOutcome::Unrecorded => Vec::new(),
                RebuildOutcome::Clean { included } => included.iter().map(String::as_str).collect(),
                RebuildOutcome::WithHolds { included, held } => included
                    .iter()
                    .map(String::as_str)
                    .chain(held.iter().map(|h| h.branch.as_str()))
                    .collect(),
            },
            HitchEvent::EnvironmentCreated { .. }
            | HitchEvent::EnvironmentRemoved { .. }
            | HitchEvent::BaseChanged { .. }
            | HitchEvent::Locked { .. }
            | HitchEvent::Unlocked { .. }
            | HitchEvent::Released { .. } => Vec::new(),
        }
    }
}

/// Every event that turns `old` into `new`, in a fixed order: environments by
/// name, then removed environments by name, then approval requests by id.
/// `HitchConfig::environments` is a `HashMap`, so map order would make the same
/// commit list its events differently between runs.
///
/// Lock changes are emitted faithfully; deciding whether a lock belongs to an
/// operation is the reader's job, since it can see neighbouring commits.
pub fn derive_events(old: &HitchConfig, new: &HitchConfig) -> Vec<HitchEvent> {
    let mut out = Vec::new();

    let mut names: Vec<&String> = new.environments.keys().collect();
    names.sort();
    for name in names {
        let new_env = &new.environments[name];
        let Some(old_env) = old.environments.get(name) else {
            out.push(HitchEvent::EnvironmentCreated {
                environment: name.clone(),
                base: new_env.base.clone(),
            });
            continue;
        };

        if old_env.base != new_env.base {
            out.push(HitchEvent::BaseChanged {
                environment: name.clone(),
                from: old_env.base.clone(),
                to: new_env.base.clone(),
            });
        }
        for added in new_env
            .branches
            .iter()
            .filter(|b| !old_env.branches.contains(b))
        {
            out.push(HitchEvent::Promoted {
                environment: name.clone(),
                branch: added.clone(),
            });
        }
        for removed in old_env
            .branches
            .iter()
            .filter(|b| !new_env.branches.contains(b))
        {
            out.push(HitchEvent::Demoted {
                environment: name.clone(),
                branch: removed.clone(),
            });
        }
        if old_env.locked != new_env.locked {
            out.push(if new_env.locked {
                HitchEvent::Locked {
                    environment: name.clone(),
                    by: new_env.locked_by.clone(),
                }
            } else {
                HitchEvent::Unlocked {
                    environment: name.clone(),
                }
            });
        }
        if old_env.rebuilt_at != new_env.rebuilt_at {
            out.push(HitchEvent::Rebuilt {
                environment: name.clone(),
                outcome: RebuildOutcome::Unrecorded,
            });
        }
        if old_env.released_at != new_env.released_at {
            out.push(HitchEvent::Released {
                environment: name.clone(),
            });
        }
    }

    let mut removed: Vec<&String> = old
        .environments
        .keys()
        .filter(|n| !new.environments.contains_key(*n))
        .collect();
    removed.sort();
    for name in removed {
        out.push(HitchEvent::EnvironmentRemoved {
            environment: name.clone(),
        });
    }

    let mut requests: Vec<&ApprovalRequest> = new.approval_requests.iter().collect();
    requests.sort_by(|a, b| a.id.cmp(&b.id));
    for req in requests {
        match old.approval_requests.iter().find(|r| r.id == req.id) {
            None => out.push(HitchEvent::ApprovalRequested {
                request_id: req.id.clone(),
                environment: req.environment.clone(),
                branch: req.branch.clone(),
                direction: req.operation.into(),
            }),
            Some(old_req) => approval_transition(old_req, req, new, &mut out),
        }
    }

    out
}

fn approval_transition(
    old_req: &ApprovalRequest,
    req: &ApprovalRequest,
    new: &HitchConfig,
    out: &mut Vec<HitchEvent>,
) {
    let id = || req.id.clone();
    let environment = || req.environment.clone();
    let branch = || req.branch.clone();

    if req.approvals.len() > old_req.approvals.len() {
        // Falls back to the vote count when the environment has been removed,
        // so a vote is never shown as "n of 0".
        let required = new
            .environments
            .get(&req.environment)
            .map_or(req.approvals.len(), |e| e.min_approvals);
        out.push(HitchEvent::ApprovalVoted {
            request_id: id(),
            environment: environment(),
            branch: branch(),
            approvals: req.approvals.len(),
            required,
        });
    }

    // A rejection sets both `status` and `rejection`; one rejection is one event.
    let rejected_now = (old_req.status != req.status && req.status == ApprovalStatus::Rejected)
        || (old_req.rejection.is_none() && req.rejection.is_some());
    if rejected_now {
        out.push(HitchEvent::ApprovalRejected {
            request_id: id(),
            environment: environment(),
            branch: branch(),
        });
    }

    if old_req.status != req.status {
        match req.status {
            ApprovalStatus::Approved => out.push(HitchEvent::ApprovalGranted {
                request_id: id(),
                environment: environment(),
                branch: branch(),
            }),
            ApprovalStatus::Applied => out.push(HitchEvent::ApprovalApplied {
                request_id: id(),
                environment: environment(),
                branch: branch(),
            }),
            ApprovalStatus::Cancelled => out.push(HitchEvent::ApprovalCancelled {
                request_id: id(),
                environment: environment(),
                branch: branch(),
            }),
            ApprovalStatus::Rejected | ApprovalStatus::Pending => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Approval, Environment, RebuildSnapshot, Rejection};
    use chrono::Utc;

    fn config(envs: &[(&str, &str, &[&str])]) -> HitchConfig {
        let mut c = HitchConfig::new();
        for (name, base, branches) in envs {
            let mut e = Environment::new(base.to_string());
            e.branches = branches.iter().map(|b| b.to_string()).collect();
            c.environments.insert(name.to_string(), e);
        }
        c
    }

    fn request(id: &str, status: ApprovalStatus) -> ApprovalRequest {
        let mut r = ApprovalRequest::new(
            "prod".into(),
            "feat".into(),
            Operation::Promote,
            "a@x.com".into(),
            RebuildSnapshot {
                base_branch: "main".into(),
                base_sha: "0".into(),
                branch_shas: Default::default(),
                merge_conflicts: false,
            },
        );
        r.id = id.into();
        r.status = status;
        r
    }

    fn vote() -> Approval {
        Approval {
            approved_by: "b@x.com".into(),
            approved_at: Utc::now(),
            comment: None,
        }
    }

    fn with_request(mut c: HitchConfig, r: ApprovalRequest) -> HitchConfig {
        c.approval_requests.push(r);
        c
    }

    fn ev(kind: &str, id: &str) -> HitchEvent {
        let (request_id, environment, branch) =
            (id.to_string(), "prod".to_string(), "feat".to_string());
        match kind {
            "granted" => HitchEvent::ApprovalGranted {
                request_id,
                environment,
                branch,
            },
            "rejected" => HitchEvent::ApprovalRejected {
                request_id,
                environment,
                branch,
            },
            "applied" => HitchEvent::ApprovalApplied {
                request_id,
                environment,
                branch,
            },
            "cancelled" => HitchEvent::ApprovalCancelled {
                request_id,
                environment,
                branch,
            },
            _ => unreachable!(),
        }
    }

    #[test]
    fn a_new_environment_is_created_with_its_base() {
        let old = config(&[]);
        let new = config(&[("dev", "main", &[])]);
        assert_eq!(
            derive_events(&old, &new),
            vec![HitchEvent::EnvironmentCreated {
                environment: "dev".into(),
                base: "main".into()
            }]
        );
    }

    #[test]
    fn a_removed_environment_is_reported() {
        let old = config(&[("dev", "main", &[])]);
        let new = config(&[]);
        assert_eq!(
            derive_events(&old, &new),
            vec![HitchEvent::EnvironmentRemoved {
                environment: "dev".into()
            }]
        );
    }

    #[test]
    fn a_base_change_names_both_bases() {
        let old = config(&[("dev", "main", &[])]);
        let new = config(&[("dev", "develop", &[])]);
        assert_eq!(
            derive_events(&old, &new),
            vec![HitchEvent::BaseChanged {
                environment: "dev".into(),
                from: "main".into(),
                to: "develop".into()
            }]
        );
    }

    #[test]
    fn promotions_come_out_in_declaration_order_and_demotions_in_old_order() {
        let old = config(&[("dev", "main", &["a", "b", "c"])]);
        let new = config(&[("dev", "main", &["c", "z", "y"])]);
        let p = |b: &str| HitchEvent::Promoted {
            environment: "dev".into(),
            branch: b.into(),
        };
        let d = |b: &str| HitchEvent::Demoted {
            environment: "dev".into(),
            branch: b.into(),
        };
        assert_eq!(
            derive_events(&old, &new),
            vec![p("z"), p("y"), d("a"), d("b")]
        );
    }

    #[test]
    fn environments_are_diffed_in_name_order_not_map_order() {
        let names: Vec<String> = (0..20).map(|i| format!("env{:02}", i)).collect();
        let build = |branch: Option<&str>| {
            let mut c = HitchConfig::new();
            for n in &names {
                let mut e = Environment::new("main".into());
                if let Some(b) = branch {
                    e.branches.push(b.into());
                }
                c.environments.insert(n.clone(), e);
            }
            c
        };
        let (old, new) = (build(None), build(Some("f")));
        let first = derive_events(&old, &new);
        let got: Vec<&str> = first.iter().map(|e| e.environment()).collect();
        let want: Vec<&str> = names.iter().map(String::as_str).collect();
        assert_eq!(got, want);
        for _ in 0..50 {
            assert_eq!(derive_events(&old, &new), first);
        }
    }

    #[test]
    fn locking_carries_the_holder_when_there_is_one() {
        let old = config(&[("dev", "main", &[])]);
        let mut new = old.clone();
        let e = new.environments.get_mut("dev").unwrap();
        e.locked = true;
        e.locked_by = Some("a@x.com".into());
        assert_eq!(
            derive_events(&old, &new),
            vec![HitchEvent::Locked {
                environment: "dev".into(),
                by: Some("a@x.com".into())
            }]
        );
        new.environments.get_mut("dev").unwrap().locked_by = None;
        assert_eq!(
            derive_events(&old, &new),
            vec![HitchEvent::Locked {
                environment: "dev".into(),
                by: None
            }]
        );
    }

    #[test]
    fn unlocking_is_reported() {
        let mut old = config(&[("dev", "main", &[])]);
        old.environments.get_mut("dev").unwrap().locked = true;
        let new = config(&[("dev", "main", &[])]);
        assert_eq!(
            derive_events(&old, &new),
            vec![HitchEvent::Unlocked {
                environment: "dev".into()
            }]
        );
    }

    #[test]
    fn a_rebuild_timestamp_is_an_unrecorded_rebuild_and_a_release_timestamp_a_release() {
        let old = config(&[("dev", "main", &[])]);
        let mut new = old.clone();
        let e = new.environments.get_mut("dev").unwrap();
        e.rebuilt_at = Some(Utc::now());
        e.released_at = Some(Utc::now());
        assert_eq!(
            derive_events(&old, &new),
            vec![
                HitchEvent::Rebuilt {
                    environment: "dev".into(),
                    outcome: RebuildOutcome::Unrecorded
                },
                HitchEvent::Released {
                    environment: "dev".into()
                },
            ]
        );
    }

    #[test]
    fn a_filter_on_dev_does_not_match_a_branch_called_feature_devtools() {
        let e = HitchEvent::Promoted {
            environment: "qa".into(),
            branch: "feature/devtools".into(),
        };
        assert_ne!(e.environment(), "dev");
        assert!(!e.branches().contains(&"dev"));
    }

    #[test]
    fn a_new_request_is_requested_with_its_direction() {
        let old = config(&[("prod", "main", &[])]);
        let new = with_request(old.clone(), request("r1", ApprovalStatus::Pending));
        assert_eq!(
            derive_events(&old, &new),
            vec![HitchEvent::ApprovalRequested {
                request_id: "r1".into(),
                environment: "prod".into(),
                branch: "feat".into(),
                direction: ApprovalDirection::Promote,
            }]
        );
    }

    #[test]
    fn a_new_vote_carries_the_count_and_the_new_configs_threshold() {
        let mut old = config(&[("prod", "main", &[])]);
        old.environments.get_mut("prod").unwrap().min_approvals = 5;
        let old = with_request(old, request("r1", ApprovalStatus::Pending));
        let mut new = old.clone();
        new.environments.get_mut("prod").unwrap().min_approvals = 3;
        new.approval_requests[0].approvals.push(vote());
        assert_eq!(
            derive_events(&old, &new),
            vec![HitchEvent::ApprovalVoted {
                request_id: "r1".into(),
                environment: "prod".into(),
                branch: "feat".into(),
                approvals: 1,
                required: 3,
            }]
        );
    }

    #[test]
    fn a_vote_for_a_removed_environment_falls_back_to_the_vote_count() {
        let old = with_request(
            config(&[("prod", "main", &[])]),
            request("r1", ApprovalStatus::Pending),
        );
        let mut new = with_request(config(&[]), request("r1", ApprovalStatus::Pending));
        new.approval_requests[0].approvals.push(vote());
        let events = derive_events(&old, &new);
        assert!(events.contains(&HitchEvent::ApprovalVoted {
            request_id: "r1".into(),
            environment: "prod".into(),
            branch: "feat".into(),
            approvals: 1,
            required: 1,
        }));
    }

    #[test]
    fn each_status_transition_is_its_own_event() {
        for (status, kind) in [
            (ApprovalStatus::Approved, "granted"),
            (ApprovalStatus::Applied, "applied"),
            (ApprovalStatus::Cancelled, "cancelled"),
        ] {
            let old = with_request(
                config(&[("prod", "main", &[])]),
                request("r1", ApprovalStatus::Pending),
            );
            let mut new = old.clone();
            new.approval_requests[0].status = status;
            assert_eq!(derive_events(&old, &new), vec![ev(kind, "r1")]);
        }
    }

    #[test]
    fn one_rejection_is_one_event_however_it_is_recorded() {
        let old = with_request(
            config(&[("prod", "main", &[])]),
            request("r1", ApprovalStatus::Pending),
        );
        let rejection = Rejection {
            rejected_by: "b@x.com".into(),
            rejected_at: Utc::now(),
            reason: "no".into(),
        };

        let mut both = old.clone();
        both.approval_requests[0].status = ApprovalStatus::Rejected;
        both.approval_requests[0].rejection = Some(rejection.clone());
        assert_eq!(derive_events(&old, &both), vec![ev("rejected", "r1")]);

        let mut status_only = old.clone();
        status_only.approval_requests[0].status = ApprovalStatus::Rejected;
        assert_eq!(
            derive_events(&old, &status_only),
            vec![ev("rejected", "r1")]
        );

        let mut field_only = old.clone();
        field_only.approval_requests[0].rejection = Some(rejection);
        assert_eq!(derive_events(&old, &field_only), vec![ev("rejected", "r1")]);
    }

    #[test]
    fn a_vote_that_reaches_the_threshold_is_voted_then_granted() {
        let old = with_request(
            config(&[("prod", "main", &[])]),
            request("r1", ApprovalStatus::Pending),
        );
        let mut new = old.clone();
        new.approval_requests[0].approvals.push(vote());
        new.approval_requests[0].status = ApprovalStatus::Approved;
        let events = derive_events(&old, &new);
        assert!(matches!(events[0], HitchEvent::ApprovalVoted { .. }));
        assert_eq!(events[1], ev("granted", "r1"));
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn approval_requests_are_diffed_in_id_order() {
        let base = config(&[("prod", "main", &[])]);
        let mut new = base.clone();
        new.approval_requests
            .push(request("b", ApprovalStatus::Pending));
        new.approval_requests
            .push(request("a", ApprovalStatus::Pending));
        let ids: Vec<String> = derive_events(&base, &new)
            .into_iter()
            .map(|e| match e {
                HitchEvent::ApprovalRequested { request_id, .. } => request_id,
                other => panic!("unexpected {other:?}"),
            })
            .collect();
        assert_eq!(ids, vec!["a", "b"]);
    }

    #[test]
    fn events_serialize_with_snake_case_tags_and_direction() {
        let e = HitchEvent::ApprovalRequested {
            request_id: "r".into(),
            environment: "prod".into(),
            branch: "f".into(),
            direction: Operation::Demote.into(),
        };
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["kind"], "approval_requested");
        assert_eq!(v["direction"], "demote");
        let r = serde_json::to_value(RebuildOutcome::Unrecorded).unwrap();
        assert_eq!(r["status"], "unrecorded");
    }
}
