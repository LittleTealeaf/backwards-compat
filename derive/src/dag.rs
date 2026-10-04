use std::collections::HashSet;
use syn::{Error, Result, Type};

use crate::parse::{BackwardsCompatInput, VersionTag};

#[derive(Debug, Clone)]
#[allow(dead_code, reason = "debug and DAG introspection fields")]
pub struct MigrationStep {
    pub from_ty: Type,
    pub to_ty: Type,
    pub fallible: bool,
}

#[derive(Debug, Clone)]
#[allow(dead_code, reason = "debug and DAG introspection fields")]
pub struct ResolvedPath {
    pub tag: VersionTag,
    pub steps: Vec<MigrationStep>,
}

impl ResolvedPath {
    pub fn is_fallible(&self) -> bool {
        self.steps.iter().any(|s| s.fallible)
    }
}

#[derive(Debug, Clone)]
pub struct DagPlan {
    pub paths: Vec<ResolvedPath>,
    pub latest_wire_ty: Type,
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
        return Ok(DagPlan {
            paths: Vec::new(),
            latest_wire_ty: input.target_ty.clone(),
            target_is_wire: true,
        });
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
                    format!("target version `{next_tag}` does not exist"),
                ));
            }
        } else {
            // explicit_next is None
            if v.tag.matches(&input.current_version) {
                next_targets.push(NextTarget::TerminalTarget);
            } else if i < n - 1 {
                next_targets.push(NextTarget::VersionIndex(i + 1));
            } else if let Some(target_idx) = current_idx_opt {
                if target_idx == i {
                    next_targets.push(NextTarget::TerminalTarget);
                } else {
                    next_targets.push(NextTarget::VersionIndex(target_idx));
                }
            } else {
                next_targets.push(NextTarget::TerminalTarget);
            }
        }
    }

    for (i, v) in input.versions.iter().enumerate() {
        if v.explicit_next.is_none() && !v.tag.matches(&input.current_version) {
            let last_same_idx = input.versions.iter().rposition(|cand| cand.ty == v.ty).unwrap();
            if last_same_idx > i
                && let Some(target) = next_targets.get(last_same_idx).copied()
                && let Some(slot) = next_targets.get_mut(i)
            {
                *slot = target;
            }
        }
    }

    // Cycle detection & dead-end detection
    for (i, v) in input.versions.iter().enumerate() {
        let mut visited_in_path = HashSet::new();
        let mut curr = i;
        visited_in_path.insert(curr);

        while let Some(&target) = next_targets.get(curr) {
            match target {
                NextTarget::TerminalTarget => break,
                NextTarget::VersionIndex(next_idx) => {
                    if !visited_in_path.insert(next_idx) {
                        return Err(Error::new(
                            v.tag_span,
                            format!(
                                "cycle detected in backwards_compat migration graph starting at version `{}`",
                                v.tag
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
    for (i, v) in input.versions.iter().enumerate() {
        let mut steps = Vec::new();
        let mut curr_node = i;
        let mut curr_ty = v.ty.clone();

        while let Some(&target) = next_targets.get(curr_node) {
            match target {
                NextTarget::TerminalTarget => {
                    if curr_ty != input.target_ty {
                        let transition_node = input
                            .versions
                            .iter()
                            .rposition(|cand| cand.ty == curr_ty)
                            .unwrap_or(curr_node);
                        let fallible = input
                            .versions
                            .get(transition_node)
                            .is_some_and(|node| node.fallible);
                        steps.push(MigrationStep {
                            from_ty: curr_ty,
                            to_ty: input.target_ty.clone(),
                            fallible,
                        });
                    }
                    break;
                }
                NextTarget::VersionIndex(next_idx) => {
                    let Some(next_version) = input.versions.get(next_idx) else {
                        break;
                    };
                    let next_ty = next_version.ty.clone();
                    let transition_node = input
                        .versions
                        .iter()
                        .rposition(|cand| cand.ty == curr_ty)
                        .unwrap_or(curr_node);
                    let fallible = input
                        .versions
                        .get(transition_node)
                        .is_some_and(|node| node.fallible);
                    steps.push(MigrationStep {
                        from_ty: curr_ty,
                        to_ty: next_ty.clone(),
                        fallible,
                    });
                    curr_node = next_idx;
                    curr_ty = next_ty;
                }
            }
        }

        paths.push(ResolvedPath {
            tag: v.tag.clone(),
            steps,
        });
    }

    let (latest_wire_ty, target_is_wire) = current_idx_opt
        .and_then(|idx| input.versions.get(idx))
        .map_or_else(|| (input.target_ty.clone(), true), |v| (v.ty.clone(), false));

    Ok(DagPlan {
        paths,
        latest_wire_ty,
        target_is_wire,
    })
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, reason = "index accesses in unit test assertions")]
mod tests {
    use super::*;
    use crate::parse::BackwardsCompatArgs;
    use syn::parse_quote;

    #[test]
    fn test_is_fallible_empty_steps() {
        let path = ResolvedPath {
            tag: VersionTag::Int(1),
            steps: vec![],
        };
        assert!(!path.is_fallible());
    }

    #[test]
    fn test_is_fallible_all_infallible_steps() {
        let t1: Type = parse_quote!(V1);
        let t2: Type = parse_quote!(V2);
        let t3: Type = parse_quote!(V3);
        let path = ResolvedPath {
            tag: VersionTag::Int(1),
            steps: vec![
                MigrationStep {
                    from_ty: t1,
                    to_ty: t2.clone(),
                    fallible: false,
                },
                MigrationStep {
                    from_ty: t2,
                    to_ty: t3,
                    fallible: false,
                },
            ],
        };
        assert!(!path.is_fallible());
    }

    #[test]
    fn test_is_fallible_with_intermediate_fallible_step() {
        let t1: Type = parse_quote!(V1);
        let t2: Type = parse_quote!(V2);
        let t3: Type = parse_quote!(V3);
        let path = ResolvedPath {
            tag: VersionTag::Int(1),
            steps: vec![
                MigrationStep {
                    from_ty: t1,
                    to_ty: t2.clone(),
                    fallible: true,
                },
                MigrationStep {
                    from_ty: t2,
                    to_ty: t3,
                    fallible: false,
                },
            ],
        };
        assert!(path.is_fallible());
    }

    #[test]
    fn test_dag_duplicate_types_sequential() {
        let input_tokens = quote::quote! {
            versions = [
                1: V1,
                2: V1,
                3: V2,
            ]
        };
        let args: BackwardsCompatArgs = syn::parse2(input_tokens).unwrap();
        let input = args
            .into_input(parse_quote!(Target), parse_quote!(pub), syn::Generics::default())
            .unwrap();
        let plan = resolve_dag(&input).unwrap();
        assert_eq!(plan.paths.len(), 3);

        let v1: Type = parse_quote!(V1);
        let v2: Type = parse_quote!(V2);
        let target: Type = parse_quote!(Target);

        // Tag 1: V1 -> V2 -> Target (2 steps)
        assert_eq!(plan.paths[0].tag, VersionTag::Int(1));
        assert_eq!(plan.paths[0].steps.len(), 2);
        assert_eq!(plan.paths[0].steps[0].from_ty, v1);
        assert_eq!(plan.paths[0].steps[0].to_ty, v2);
        assert_eq!(plan.paths[0].steps[1].from_ty, v2);
        assert_eq!(plan.paths[0].steps[1].to_ty, target);

        // Tag 2: V1 -> V2 -> Target (2 steps)
        assert_eq!(plan.paths[1].tag, VersionTag::Int(2));
        assert_eq!(plan.paths[1].steps.len(), 2);
        assert_eq!(plan.paths[1].steps[0].from_ty, v1);
        assert_eq!(plan.paths[1].steps[0].to_ty, v2);
        assert_eq!(plan.paths[1].steps[1].from_ty, v2);
        assert_eq!(plan.paths[1].steps[1].to_ty, target);

        // Tag 3: V2 -> Target (1 step)
        assert_eq!(plan.paths[2].tag, VersionTag::Int(3));
        assert_eq!(plan.paths[2].steps.len(), 1);
        assert_eq!(plan.paths[2].steps[0].from_ty, v2);
        assert_eq!(plan.paths[2].steps[0].to_ty, target);
    }

    #[test]
    fn test_dag_duplicate_types_alternating() {
        let input_tokens = quote::quote! {
            versions = [
                1: V1,
                2: V2,
                3: V1,
            ]
        };
        let args: BackwardsCompatArgs = syn::parse2(input_tokens).unwrap();
        let input = args
            .into_input(parse_quote!(Target), parse_quote!(pub), syn::Generics::default())
            .unwrap();
        let plan = resolve_dag(&input).unwrap();
        assert_eq!(plan.paths.len(), 3);

        let v1: Type = parse_quote!(V1);
        let v2: Type = parse_quote!(V2);
        let target: Type = parse_quote!(Target);

        // Tag 1: V1 -> Target (1 step)
        assert_eq!(plan.paths[0].tag, VersionTag::Int(1));
        assert_eq!(plan.paths[0].steps.len(), 1);
        assert_eq!(plan.paths[0].steps[0].from_ty, v1);
        assert_eq!(plan.paths[0].steps[0].to_ty, target);

        // Tag 2: V2 -> V1 -> Target (2 steps)
        assert_eq!(plan.paths[1].tag, VersionTag::Int(2));
        assert_eq!(plan.paths[1].steps.len(), 2);
        assert_eq!(plan.paths[1].steps[0].from_ty, v2);
        assert_eq!(plan.paths[1].steps[0].to_ty, v1);
        assert_eq!(plan.paths[1].steps[1].from_ty, v1);
        assert_eq!(plan.paths[1].steps[1].to_ty, target);

        // Tag 3: V1 -> Target (1 step)
        assert_eq!(plan.paths[2].tag, VersionTag::Int(3));
        assert_eq!(plan.paths[2].steps.len(), 1);
        assert_eq!(plan.paths[2].steps[0].from_ty, v1);
        assert_eq!(plan.paths[2].steps[0].to_ty, target);
    }

    #[test]
    fn test_dag_duplicate_types_with_fallible() {
        // Case 1: Latest occurrence of V1 (tag 2) is fallible
        let input_tokens1 = quote::quote! {
            versions = [
                1: V1,
                #[fallible] 2: V1,
                3: V2,
            ]
        };
        let args1: BackwardsCompatArgs = syn::parse2(input_tokens1).unwrap();
        let input1 = args1
            .into_input(parse_quote!(Target), parse_quote!(pub), syn::Generics::default())
            .unwrap();
        let plan1 = resolve_dag(&input1).unwrap();

        // Tag 1 path: V1 -> V2 (fallible), V2 -> Target (infallible)
        assert_eq!(plan1.paths[0].steps.len(), 2);
        assert!(plan1.paths[0].steps[0].fallible);
        assert!(!plan1.paths[0].steps[1].fallible);
        assert!(plan1.paths[0].is_fallible());

        // Tag 2 path: V1 -> V2 (fallible), V2 -> Target (infallible)
        assert_eq!(plan1.paths[1].steps.len(), 2);
        assert!(plan1.paths[1].steps[0].fallible);
        assert!(!plan1.paths[1].steps[1].fallible);
        assert!(plan1.paths[1].is_fallible());

        // Tag 3 path: V2 -> Target (infallible)
        assert_eq!(plan1.paths[2].steps.len(), 1);
        assert!(!plan1.paths[2].steps[0].fallible);
        assert!(!plan1.paths[2].is_fallible());

        // Case 2: Earlier occurrence of V1 (tag 1) is fallible, but latest (tag 2) is infallible
        let input_tokens2 = quote::quote! {
            versions = [
                #[fallible] 1: V1,
                2: V1,
                3: V2,
            ]
        };
        let args2: BackwardsCompatArgs = syn::parse2(input_tokens2).unwrap();
        let input2 = args2
            .into_input(parse_quote!(Target), parse_quote!(pub), syn::Generics::default())
            .unwrap();
        let plan2 = resolve_dag(&input2).unwrap();

        // Transition V1 -> V2 routes from latest V1 (tag 2), which is infallible
        assert!(!plan2.paths[0].steps[0].fallible);
        assert!(!plan2.paths[1].steps[0].fallible);
        assert!(!plan2.paths[0].is_fallible());
        assert!(!plan2.paths[1].is_fallible());

        // Case 3: Terminal version is fallible
        let input_tokens3 = quote::quote! {
            versions = [
                1: V1,
                2: V1,
                #[fallible] 3: V2,
            ]
        };
        let args3: BackwardsCompatArgs = syn::parse2(input_tokens3).unwrap();
        let input3 = args3
            .into_input(parse_quote!(Target), parse_quote!(pub), syn::Generics::default())
            .unwrap();
        let plan3 = resolve_dag(&input3).unwrap();

        // V2 -> Target step is fallible for all paths
        assert!(!plan3.paths[0].steps[0].fallible);
        assert!(plan3.paths[0].steps[1].fallible);
        assert!(plan3.paths[0].is_fallible());

        assert!(!plan3.paths[1].steps[0].fallible);
        assert!(plan3.paths[1].steps[1].fallible);
        assert!(plan3.paths[1].is_fallible());

        assert!(plan3.paths[2].steps[0].fallible);
        assert!(plan3.paths[2].is_fallible());
    }
}
