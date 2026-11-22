use std::collections::HashSet;
use syn::{Error, Result, Type};

use crate::parse::{BackwardsCompatInput, ExplicitNext, VersionTag};

#[derive(Debug, Clone)]
#[allow(dead_code, reason = "debug and DAG introspection fields")]
pub struct MigrationStep {
    pub from_ty: Type,
    pub to_ty: Type,
    pub fallible: bool,
    pub with_fn: Option<syn::ExprPath>,
}

#[derive(Debug, Clone)]
#[allow(dead_code, reason = "debug and DAG introspection fields")]
pub struct ResolvedPath {
    pub tag: VersionTag,
    pub steps: Vec<MigrationStep>,
    pub cfg: Option<proc_macro2::TokenStream>,
}

fn dedup_predicates<'a, I>(predicates: I) -> Vec<&'a proc_macro2::TokenStream>
where
    I: IntoIterator<Item = &'a proc_macro2::TokenStream>,
{
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    for p in predicates {
        if seen.insert(p.to_string()) {
            deduped.push(p);
        }
    }
    deduped
}

/// Combines zero or more `#[cfg(...)]` predicates with `all(...)`, deduplicating in insertion order.
pub fn combine_cfg_all<'a, I>(predicates: I) -> Option<proc_macro2::TokenStream>
where
    I: IntoIterator<Item = &'a proc_macro2::TokenStream>,
{
    let deduped = dedup_predicates(predicates);
    match deduped.as_slice() {
        [] => None,
        [single] => Some((*single).clone()),
        many => Some(quote::quote!(all(#(#many),*))),
    }
}

/// Combines zero or more `#[cfg(...)]` predicates with `any(...)`, deduplicating in insertion order.
pub fn combine_cfg_any<'a, I>(predicates: I) -> Option<proc_macro2::TokenStream>
where
    I: IntoIterator<Item = &'a proc_macro2::TokenStream>,
{
    let deduped = dedup_predicates(predicates);
    match deduped.as_slice() {
        [] => None,
        [single] => Some((*single).clone()),
        many => Some(quote::quote!(any(#(#many),*))),
    }
}

impl ResolvedPath {
    pub fn is_fallible(&self) -> bool {
        self.steps.iter().any(|s| s.fallible)
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code, reason = "fields used in macro code generation")]
pub struct DagPlan {
    pub paths: Vec<ResolvedPath>,
    pub target_is_wire: bool,
    pub serialize_wire_ty: Type,
    pub serialize_target_is_wire: bool,
    pub serialize_with_fn: Option<syn::ExprPath>,
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
            target_is_wire: true,
            serialize_wire_ty: input.target_ty.clone(),
            serialize_target_is_wire: true,
            serialize_with_fn: None,
        });
    }

    let current_idx_opt = input
        .versions
        .iter()
        .position(|v| v.tag.matches(&input.current_version));

    // Determine the next step for each version
    let mut next_targets: Vec<NextTarget> = Vec::with_capacity(n);

    for (i, v) in input.versions.iter().enumerate() {
        if let Some(ref explicit_next) = v.explicit_next {
            match explicit_next {
                ExplicitNext::SelfTarget(_) => {
                    next_targets.push(NextTarget::TerminalTarget);
                }
                ExplicitNext::Tag(next_tag, next_span) => {
                    let next_span = *next_span;
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
                    } else if let Some(target_idx) =
                        input.versions.iter().position(|cand| cand.tag.matches(next_tag))
                    {
                        next_targets.push(NextTarget::VersionIndex(target_idx));
                    } else {
                        return Err(Error::new(
                            next_span,
                            format!("target version `{next_tag}` does not exist"),
                        ));
                    }
                }
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
            let last_same_idx = input
                .versions
                .iter()
                .rposition(|cand| cand.ty == v.ty && (!v.cfgs.is_empty() || cand.cfgs.is_empty()))
                .unwrap_or(i);
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
        let mut path_cfgs: Vec<&proc_macro2::TokenStream> = v.cfgs.iter().map(|c| &c.tokens).collect();
        let mut curr_node = i;
        let mut curr_ty = v.ty.clone();

        while let Some(&target) = next_targets.get(curr_node) {
            match target {
                NextTarget::TerminalTarget => {
                    if curr_ty != input.target_ty {
                        let transition_node = if input
                            .versions
                            .get(curr_node)
                            .is_some_and(|node| node.explicit_next.is_some())
                        {
                            curr_node
                        } else {
                            input
                                .versions
                                .iter()
                                .rposition(|cand| {
                                    cand.ty == curr_ty && (!v.cfgs.is_empty() || cand.cfgs.is_empty())
                                })
                                .unwrap_or(curr_node)
                        };
                        let fallible = input
                            .versions
                            .get(transition_node)
                            .is_some_and(|node| node.fallible);
                        let with_fn = input
                            .versions
                            .get(transition_node)
                            .and_then(|node| node.with_fn.clone());
                        steps.push(MigrationStep {
                            from_ty: curr_ty,
                            to_ty: input.target_ty.clone(),
                            fallible,
                            with_fn,
                        });
                    }
                    break;
                }
                NextTarget::VersionIndex(next_idx) => {
                    let Some(next_version) = input.versions.get(next_idx) else {
                        break;
                    };
                    if v.cfgs.is_empty() && !next_version.cfgs.is_empty() {
                        return Err(Error::new(
                            v.tag_span,
                            format!(
                                "unconditional version `{}` cannot migrate through conditionally compiled version `{}`; add `#[cfg(...)]` or use an explicit `=>` transition to an unconditional target",
                                v.tag, next_version.tag
                            ),
                        ));
                    }
                    path_cfgs.extend(next_version.cfgs.iter().map(|c| &c.tokens));
                    let next_ty = next_version.ty.clone();
                    if next_ty != curr_ty {
                        let transition_node = if input
                            .versions
                            .get(curr_node)
                            .is_some_and(|node| node.explicit_next.is_some())
                        {
                            curr_node
                        } else {
                            input
                                .versions
                                .iter()
                                .rposition(|cand| {
                                    cand.ty == curr_ty && (!v.cfgs.is_empty() || cand.cfgs.is_empty())
                                })
                                .unwrap_or(curr_node)
                        };
                        let fallible = input
                            .versions
                            .get(transition_node)
                            .is_some_and(|node| node.fallible);
                        let with_fn = input
                            .versions
                            .get(transition_node)
                            .and_then(|node| node.with_fn.clone());
                        steps.push(MigrationStep {
                            from_ty: curr_ty,
                            to_ty: next_ty.clone(),
                            fallible,
                            with_fn,
                        });
                    }
                    curr_node = next_idx;
                    curr_ty = next_ty;
                }
            }
        }

        paths.push(ResolvedPath {
            tag: v.tag.clone(),
            steps,
            cfg: combine_cfg_all(path_cfgs),
        });
    }

    let target_is_wire = current_idx_opt
        .and_then(|idx| input.versions.get(idx))
        .is_none_or(|v| v.ty == input.target_ty);

    let serialize_idx_opt = input
        .versions
        .iter()
        .position(|v| v.tag.matches(&input.serialize_version));
    let (serialize_wire_ty, serialize_target_is_wire) = serialize_idx_opt
        .and_then(|idx| input.versions.get(idx))
        .map_or_else(
            || (input.target_ty.clone(), true),
            |v| (v.ty.clone(), v.ty == input.target_ty),
        );

    Ok(DagPlan {
        paths,
        target_is_wire,
        serialize_wire_ty,
        serialize_target_is_wire,
        serialize_with_fn: input.serialize_with_fn.clone(),
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
            cfg: None,
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
                    with_fn: None,
                },
                MigrationStep {
                    from_ty: t2,
                    to_ty: t3,
                    fallible: false,
                    with_fn: None,
                },
            ],
            cfg: None,
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
                    with_fn: None,
                },
                MigrationStep {
                    from_ty: t2,
                    to_ty: t3,
                    fallible: false,
                    with_fn: None,
                },
            ],
            cfg: None,
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

    #[test]
    fn test_combine_cfg_all_and_any() {
        let p1 = quote::quote!(feature = "a");
        let p2 = quote::quote!(feature = "b");
        let p3 = quote::quote!(test);

        // 0 predicates
        assert!(combine_cfg_all(core::iter::empty::<&proc_macro2::TokenStream>()).is_none());
        assert!(combine_cfg_any(core::iter::empty::<&proc_macro2::TokenStream>()).is_none());

        // 1 predicate
        assert_eq!(combine_cfg_all([&p1]).unwrap().to_string(), p1.to_string());
        assert_eq!(combine_cfg_any([&p1]).unwrap().to_string(), p1.to_string());

        // Duplicate single predicate -> collapsed to single (no all/any wrapper)
        assert_eq!(combine_cfg_all([&p1, &p1]).unwrap().to_string(), p1.to_string());
        assert_eq!(combine_cfg_any([&p1, &p1]).unwrap().to_string(), p1.to_string());

        // 2+ predicates with duplicates -> deduplicated in insertion order
        assert_eq!(
            combine_cfg_all([&p1, &p2, &p1, &p3, &p2]).unwrap().to_string(),
            quote::quote!(all(feature = "a", feature = "b", test)).to_string()
        );
        assert_eq!(
            combine_cfg_any([&p1, &p2, &p1, &p3, &p2]).unwrap().to_string(),
            quote::quote!(any(feature = "a", feature = "b", test)).to_string()
        );
    }

    #[test]
    fn test_dag_gated_entry_jumping_to_unconditional_target() {
        let input_tokens = quote::quote! {
            versions = [
                #[cfg(feature = "legacy")]
                1: V1 => 2,
                2: V2,
            ]
        };
        let args: BackwardsCompatArgs = syn::parse2(input_tokens).unwrap();
        let input = args
            .into_input(parse_quote!(Target), parse_quote!(pub), syn::Generics::default())
            .unwrap();
        let plan = resolve_dag(&input).unwrap();

        assert_eq!(plan.paths.len(), 2);
        assert_eq!(
            plan.paths[0].cfg.as_ref().unwrap().to_string(),
            quote::quote!(feature = "legacy").to_string()
        );
        assert!(plan.paths[1].cfg.is_none());
    }

    #[test]
    fn test_dag_chained_gated_entries_same_cfg_deduplicated() {
        let input_tokens = quote::quote! {
            versions = [
                #[cfg(feature = "legacy")]
                1: V1 => 2,
                #[cfg(feature = "legacy")]
                2: V2 => 3,
                3: V3,
            ]
        };
        let args: BackwardsCompatArgs = syn::parse2(input_tokens).unwrap();
        let input = args
            .into_input(parse_quote!(Target), parse_quote!(pub), syn::Generics::default())
            .unwrap();
        let plan = resolve_dag(&input).unwrap();

        assert_eq!(
            plan.paths[0].cfg.as_ref().unwrap().to_string(),
            quote::quote!(feature = "legacy").to_string()
        );
        assert_eq!(
            plan.paths[1].cfg.as_ref().unwrap().to_string(),
            quote::quote!(feature = "legacy").to_string()
        );
        assert!(plan.paths[2].cfg.is_none());
    }

    #[test]
    fn test_dag_chained_gated_entries_different_cfgs_combined_with_all() {
        let input_tokens = quote::quote! {
            versions = [
                #[cfg(feature = "v1")]
                1: V1 => 2,
                #[cfg(feature = "v2")]
                2: V2 => 3,
                3: V3,
            ]
        };
        let args: BackwardsCompatArgs = syn::parse2(input_tokens).unwrap();
        let input = args
            .into_input(parse_quote!(Target), parse_quote!(pub), syn::Generics::default())
            .unwrap();
        let plan = resolve_dag(&input).unwrap();

        assert_eq!(
            plan.paths[0].cfg.as_ref().unwrap().to_string(),
            quote::quote!(all(feature = "v1", feature = "v2")).to_string()
        );
        assert_eq!(
            plan.paths[1].cfg.as_ref().unwrap().to_string(),
            quote::quote!(feature = "v2").to_string()
        );
        assert!(plan.paths[2].cfg.is_none());
    }

    #[test]
    fn test_dag_multiple_cfgs_on_single_entry_combined_with_all() {
        let input_tokens = quote::quote! {
            versions = [
                #[cfg(feature = "v1")]
                #[cfg(not(target_arch = "wasm32"))]
                1: V1 => 2,
                2: V2,
            ]
        };
        let args: BackwardsCompatArgs = syn::parse2(input_tokens).unwrap();
        let input = args
            .into_input(parse_quote!(Target), parse_quote!(pub), syn::Generics::default())
            .unwrap();
        let plan = resolve_dag(&input).unwrap();

        assert_eq!(
            plan.paths[0].cfg.as_ref().unwrap().to_string(),
            quote::quote!(all(feature = "v1", not(target_arch = "wasm32"))).to_string()
        );
        assert!(plan.paths[1].cfg.is_none());
    }

    #[test]
    fn test_dag_unconditional_chaining_through_gated_entry_errors() {
        // Explicit `=>` transition from unconditional to gated
        let explicit_input = quote::quote! {
            versions = [
                1: V1 => 2,
                #[cfg(feature = "v2")]
                2: V2 => 3,
                3: V3,
            ]
        };
        let args1: BackwardsCompatArgs = syn::parse2(explicit_input).unwrap();
        let input1 = args1
            .into_input(parse_quote!(Target), parse_quote!(pub), syn::Generics::default())
            .unwrap();
        let err1 = resolve_dag(&input1).unwrap_err();
        assert_eq!(
            err1.to_string(),
            "unconditional version `1` cannot migrate through conditionally compiled version `2`; add `#[cfg(...)]` or use an explicit `=>` transition to an unconditional target"
        );

        // Implicit sequential transition from unconditional to gated
        let implicit_input = quote::quote! {
            versions = [
                1: V1,
                #[cfg(feature = "v2")]
                2: V2,
                3: V3,
            ]
        };
        let args2: BackwardsCompatArgs = syn::parse2(implicit_input).unwrap();
        let input2 = args2
            .into_input(parse_quote!(Target), parse_quote!(pub), syn::Generics::default())
            .unwrap();
        let err2 = resolve_dag(&input2).unwrap_err();
        assert_eq!(
            err2.to_string(),
            "unconditional version `1` cannot migrate through conditionally compiled version `2`; add `#[cfg(...)]` or use an explicit `=>` transition to an unconditional target"
        );
    }

    #[test]
    fn test_dag_duplicate_types_unconditional_not_hijacked_by_later_gated_entry() {
        let input_tokens = quote::quote! {
            versions = [
                1: V1,
                2: V2 => 5,
                #[cfg(feature = "v3")]
                3: V1 => 4,
                #[cfg(feature = "v4")]
                4: V4 => 5,
                5: V5,
            ]
        };
        let args: BackwardsCompatArgs = syn::parse2(input_tokens).unwrap();
        let input = args
            .into_input(parse_quote!(Target), parse_quote!(pub), syn::Generics::default())
            .unwrap();
        let plan = resolve_dag(&input).unwrap();

        let v1: Type = parse_quote!(V1);
        let v2: Type = parse_quote!(V2);
        let v4: Type = parse_quote!(V4);
        let v5: Type = parse_quote!(V5);
        let target: Type = parse_quote!(Target);

        // Tag 1 (unconditional V1) must follow V1 -> V2 -> V5 -> Target, not be hijacked by gated tag 3
        assert_eq!(plan.paths[0].tag, VersionTag::Int(1));
        assert!(plan.paths[0].cfg.is_none());
        assert_eq!(plan.paths[0].steps.len(), 3);
        assert_eq!(plan.paths[0].steps[0].from_ty, v1);
        assert_eq!(plan.paths[0].steps[0].to_ty, v2);
        assert_eq!(plan.paths[0].steps[1].from_ty, v2);
        assert_eq!(plan.paths[0].steps[1].to_ty, v5);
        assert_eq!(plan.paths[0].steps[2].from_ty, v5);
        assert_eq!(plan.paths[0].steps[2].to_ty, target);

        // Tag 3 (gated V1) follows V1 -> V4 -> V5 -> Target with combined cfg
        assert_eq!(plan.paths[2].tag, VersionTag::Int(3));
        assert_eq!(
            plan.paths[2].cfg.as_ref().unwrap().to_string(),
            quote::quote!(all(feature = "v3", feature = "v4")).to_string()
        );
        assert_eq!(plan.paths[2].steps.len(), 3);
        assert_eq!(plan.paths[2].steps[0].from_ty, v1);
        assert_eq!(plan.paths[2].steps[0].to_ty, v4);
        assert_eq!(plan.paths[2].steps[1].from_ty, v4);
        assert_eq!(plan.paths[2].steps[1].to_ty, v5);
        assert_eq!(plan.paths[2].steps[2].from_ty, v5);
        assert_eq!(plan.paths[2].steps[2].to_ty, target);
    }

    #[test]
    fn test_dag_fallible_on_later_gated_duplicate_type_does_not_leak_to_unconditional() {
        // Intermediate transition (`NextTarget::VersionIndex`)
        let intermediate_tokens = quote::quote! {
            versions = [
                1: V1,
                2: V2 => 5,
                #[cfg(feature = "v3")]
                #[fallible]
                3: V1 => 4,
                #[cfg(feature = "v4")]
                4: V4 => 5,
                5: V5,
            ]
        };
        let args1: BackwardsCompatArgs = syn::parse2(intermediate_tokens).unwrap();
        let input1 = args1
            .into_input(parse_quote!(Target), parse_quote!(pub), syn::Generics::default())
            .unwrap();
        let plan1 = resolve_dag(&input1).unwrap();

        assert!(!plan1.paths[0].is_fallible());
        assert!(!plan1.paths[0].steps[0].fallible);
        assert!(plan1.paths[2].is_fallible());
        assert!(plan1.paths[2].steps[0].fallible);

        // Terminal transition (`NextTarget::TerminalTarget`)
        let terminal_tokens = quote::quote! {
            versions = [
                1: V1 => Self,
                #[cfg(feature = "v2")]
                #[fallible]
                2: V1 => Self,
                3: Self,
            ]
        };
        let args2: BackwardsCompatArgs = syn::parse2(terminal_tokens).unwrap();
        let input2 = args2
            .into_input(parse_quote!(Target), parse_quote!(pub), syn::Generics::default())
            .unwrap();
        let plan2 = resolve_dag(&input2).unwrap();

        assert!(!plan2.paths[0].is_fallible());
        assert!(!plan2.paths[0].steps[0].fallible);
        assert!(plan2.paths[1].is_fallible());
        assert!(plan2.paths[1].steps[0].fallible);
    }

    #[test]
    fn test_dag_serialize_on_older_version_and_with_fn() {
        use quote::ToTokens as _;

        // Bare #[serialize] on older version
        let args1: BackwardsCompatArgs = syn::parse2(quote::quote! {
            versions = [
                #[serialize] 2: V2 => 3,
                #[current] 3: V3,
            ]
        })
        .unwrap();
        let input1 = args1
            .into_input(parse_quote!(Target), parse_quote!(pub), syn::Generics::default())
            .unwrap();
        let plan1 = resolve_dag(&input1).unwrap();

        let v2: Type = parse_quote!(V2);
        assert_eq!(plan1.serialize_wire_ty, v2);
        assert!(!plan1.serialize_target_is_wire);
        assert!(!plan1.target_is_wire);
        assert!(plan1.serialize_with_fn.is_none());

        // #[serialize(with = to_v2)] on older version
        let args2: BackwardsCompatArgs = syn::parse2(quote::quote! {
            versions = [
                #[serialize(with = to_v2)] 2: V2 => 3,
                #[current] 3: V3,
            ]
        })
        .unwrap();
        let input2 = args2
            .into_input(parse_quote!(Target), parse_quote!(pub), syn::Generics::default())
            .unwrap();
        let plan2 = resolve_dag(&input2).unwrap();

        assert_eq!(plan2.serialize_wire_ty, v2);
        assert!(!plan2.serialize_target_is_wire);
        assert_eq!(
            plan2
                .serialize_with_fn
                .as_ref()
                .unwrap()
                .to_token_stream()
                .to_string(),
            "to_v2"
        );
    }

    #[test]
    fn test_dag_self_wire_sets_target_is_wire_and_serialize_target_is_wire() {
        let args: BackwardsCompatArgs = syn::parse2(quote::quote! {
            versions = [
                1: V1,
                2: Self,
            ]
        })
        .unwrap();
        let target: Type = parse_quote!(Target);
        let input = args
            .into_input(target.clone(), parse_quote!(pub), syn::Generics::default())
            .unwrap();
        let plan = resolve_dag(&input).unwrap();

        assert!(plan.target_is_wire);
        assert!(plan.serialize_target_is_wire);
        assert_eq!(plan.serialize_wire_ty, target);
        assert_eq!(plan.paths[0].steps.len(), 1);
        assert!(plan.paths[1].steps.is_empty());
    }

    #[test]
    fn test_dag_explicit_next_self_target_bypasses_current_wire() {
        let args: BackwardsCompatArgs = syn::parse2(quote::quote! {
            versions = [
                1: V1 => Self,
                #[current] 2: V2,
            ]
        })
        .unwrap();
        let input = args
            .into_input(parse_quote!(Target), parse_quote!(pub), syn::Generics::default())
            .unwrap();
        let plan = resolve_dag(&input).unwrap();

        let v1: Type = parse_quote!(V1);
        let target: Type = parse_quote!(Target);
        assert_eq!(plan.paths[0].steps.len(), 1);
        assert_eq!(plan.paths[0].steps[0].from_ty, v1);
        assert_eq!(plan.paths[0].steps[0].to_ty, target);
    }

    #[test]
    fn test_dag_with_fn_propagation_and_explicit_next_preservation() {
        use quote::ToTokens as _;

        // Multi-step chain propagation
        let args1: BackwardsCompatArgs = syn::parse2(quote::quote! {
            versions = [
                #[with(my_mod::up1)] 1: V1 => 2,
                #[with(my_mod::up2)] 2: V2 => 3,
                3: Self,
            ]
        })
        .unwrap();
        let input1 = args1
            .into_input(parse_quote!(Target), parse_quote!(pub), syn::Generics::default())
            .unwrap();
        let plan1 = resolve_dag(&input1).unwrap();

        assert_eq!(plan1.paths[0].steps.len(), 2);
        assert_eq!(
            plan1.paths[0].steps[0]
                .with_fn
                .as_ref()
                .unwrap()
                .to_token_stream()
                .to_string(),
            quote::quote!(my_mod::up1).to_string()
        );
        assert_eq!(
            plan1.paths[0].steps[1]
                .with_fn
                .as_ref()
                .unwrap()
                .to_token_stream()
                .to_string(),
            quote::quote!(my_mod::up2).to_string()
        );

        // Explicit `=>` entry with #[with(fn_a)] is NOT overwritten by later duplicate-type entry without #[with]
        let args2: BackwardsCompatArgs = syn::parse2(quote::quote! {
            versions = [
                #[with(fn_a)] 1: V1 => 3,
                2: V1 => 3,
                3: Self,
            ]
        })
        .unwrap();
        let input2 = args2
            .into_input(parse_quote!(Target), parse_quote!(pub), syn::Generics::default())
            .unwrap();
        let plan2 = resolve_dag(&input2).unwrap();

        assert_eq!(
            plan2.paths[0].steps[0]
                .with_fn
                .as_ref()
                .unwrap()
                .to_token_stream()
                .to_string(),
            "fn_a"
        );
        assert!(plan2.paths[1].steps[0].with_fn.is_none());
    }

    #[test]
    fn test_dag_inline_pipe_aliases_resolve_identical_paths() {
        let args: BackwardsCompatArgs = syn::parse2(quote::quote! {
            versions = [
                1 | "1": V1 => 2,
                2: V2,
            ]
        })
        .unwrap();
        let input = args
            .into_input(parse_quote!(Target), parse_quote!(pub), syn::Generics::default())
            .unwrap();
        let plan = resolve_dag(&input).unwrap();

        let v1: Type = parse_quote!(V1);
        let v2: Type = parse_quote!(V2);
        let target: Type = parse_quote!(Target);

        assert_eq!(plan.paths.len(), 3);
        assert_eq!(plan.paths[0].tag, VersionTag::Int(1));
        assert_eq!(plan.paths[1].tag, VersionTag::String("1".to_owned()));
        for path in [&plan.paths[0], &plan.paths[1]] {
            assert_eq!(path.steps.len(), 2);
            assert_eq!(path.steps[0].from_ty, v1);
            assert_eq!(path.steps[0].to_ty, v2);
            assert_eq!(path.steps[1].from_ty, v2);
            assert_eq!(path.steps[1].to_ty, target);
        }
    }

    #[test]
    fn test_dag_aliased_current_version_no_redundant_self_step() {
        let v2: Type = parse_quote!(V2);
        let target: Type = parse_quote!(Target);

        // Explicit wire type aliased on current version: `2 | "2": V2` -> 1 step `[V2 -> Target]` each
        let args1: BackwardsCompatArgs = syn::parse2(quote::quote! {
            versions = [
                1: V1,
                2 | "2": V2,
            ]
        })
        .unwrap();
        let input1 = args1
            .into_input(target.clone(), parse_quote!(pub), syn::Generics::default())
            .unwrap();
        let plan1 = resolve_dag(&input1).unwrap();

        assert_eq!(plan1.paths.len(), 3);
        for path in [&plan1.paths[1], &plan1.paths[2]] {
            assert_eq!(path.steps.len(), 1);
            assert_eq!(path.steps[0].from_ty, v2);
            assert_eq!(path.steps[0].to_ty, target);
        }

        // Shadow wire `Self` aliased on current version: `2 | "2": Self` -> 0 steps each
        let args2: BackwardsCompatArgs = syn::parse2(quote::quote! {
            versions = [
                1: V1,
                2 | "2": Self,
            ]
        })
        .unwrap();
        let input2 = args2
            .into_input(target, parse_quote!(pub), syn::Generics::default())
            .unwrap();
        let plan2 = resolve_dag(&input2).unwrap();

        assert_eq!(plan2.paths.len(), 3);
        assert!(plan2.paths[1].steps.is_empty());
        assert!(plan2.paths[2].steps.is_empty());
    }
}
