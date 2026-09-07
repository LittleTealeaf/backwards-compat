use std::collections::HashSet;
use syn::{Error, Result, Type};

use crate::parse::{BackwardsCompatInput, VersionTag};

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct MigrationStep {
    pub from_ty: Type,
    pub to_ty: Type,
    pub fallible: bool,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct ResolvedPath {
    pub tag: VersionTag,
    pub steps: Vec<MigrationStep>,
}

#[derive(Debug, Clone)]
pub struct DagPlan {
    pub paths: Vec<ResolvedPath>,
    pub latest_wire_ty: Type,
    pub latest_variant_idx: usize,
    pub target_is_wire: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NextTarget {
    TerminalTarget,
    VersionIndex(usize),
}

pub fn resolve_dag(input: &BackwardsCompatInput) -> Result<DagPlan> {
    let n = input.versions.len();
    if n == 0 {
        return Err(Error::new_spanned(
            &input.target_ty,
            "at least one version entry must be declared",
        ));
    }

    let current_idx_opt = input
        .versions
        .iter()
        .position(|v| v.tag.matches(&input.current_version));

    // Determine the next step for each version
    let mut next_targets: Vec<NextTarget> = Vec::with_capacity(n);

    for (i, v) in input.versions.iter().enumerate() {
        if let Some((ref next_tag, next_span)) = v.explicit_next {
            if next_tag.matches(&v.tag) {
                return Err(Error::new(
                    next_span,
                    format!("self-loop transition in version `{}`", v.tag),
                ));
            }

            if next_tag.matches(&input.current_version) {
                if let Some(target_idx) = current_idx_opt {
                    if target_idx == i {
                        return Err(Error::new(
                            next_span,
                            format!("self-loop transition in version `{}`", v.tag),
                        ));
                    }
                    next_targets.push(NextTarget::VersionIndex(target_idx));
                } else {
                    next_targets.push(NextTarget::TerminalTarget);
                }
            } else if let Some(target_idx) = input.versions.iter().position(|cand| cand.tag.matches(next_tag))
            {
                next_targets.push(NextTarget::VersionIndex(target_idx));
            } else {
                return Err(Error::new(
                    next_span,
                    format!("target version `{}` does not exist", next_tag),
                ));
            }
        } else {
            // explicit_next is None
            if v.tag.matches(&input.current_version) {
                next_targets.push(NextTarget::TerminalTarget);
            } else if i < n - 1 {
                next_targets.push(NextTarget::VersionIndex(i + 1));
            } else if let Some(target_idx) = current_idx_opt {
                if target_idx != i {
                    next_targets.push(NextTarget::VersionIndex(target_idx));
                } else {
                    next_targets.push(NextTarget::TerminalTarget);
                }
            } else {
                next_targets.push(NextTarget::TerminalTarget);
            }
        }
    }

    // Cycle detection & dead-end detection
    for i in 0..n {
        let mut visited_in_path = HashSet::new();
        let mut curr = i;
        visited_in_path.insert(curr);

        loop {
            match next_targets[curr] {
                NextTarget::TerminalTarget => break,
                NextTarget::VersionIndex(next_idx) => {
                    if !visited_in_path.insert(next_idx) {
                        return Err(Error::new(
                            input.versions[i].tag_span,
                            format!(
                                "cycle detected in backwards_compat migration graph starting at version `{}`",
                                input.versions[i].tag
                            ),
                        ));
                    }
                    curr = next_idx;
                }
            }
        }
    }

    // Build resolved paths
    let mut paths = Vec::with_capacity(n);
    for i in 0..n {
        let mut steps = Vec::new();
        let mut curr_node = i;
        let mut curr_ty = input.versions[i].ty.clone();

        loop {
            match next_targets[curr_node] {
                NextTarget::TerminalTarget => {
                    steps.push(MigrationStep {
                        from_ty: curr_ty,
                        to_ty: input.target_ty.clone(),
                        fallible: input.versions[curr_node].fallible,
                    });
                    break;
                }
                NextTarget::VersionIndex(next_idx) => {
                    let next_ty = input.versions[next_idx].ty.clone();
                    steps.push(MigrationStep {
                        from_ty: curr_ty,
                        to_ty: next_ty.clone(),
                        fallible: input.versions[curr_node].fallible,
                    });
                    curr_node = next_idx;
                    curr_ty = next_ty;
                }
            }
        }

        paths.push(ResolvedPath {
            tag: input.versions[i].tag.clone(),
            steps,
        });
    }

    let (latest_wire_ty, latest_variant_idx, target_is_wire) = match current_idx_opt {
        Some(idx) => (input.versions[idx].ty.clone(), idx, false),
        None => (input.target_ty.clone(), n, true),
    };

    Ok(DagPlan {
        paths,
        latest_wire_ty,
        latest_variant_idx,
        target_is_wire,
    })
}
