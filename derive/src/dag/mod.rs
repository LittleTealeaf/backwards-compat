use quote::ToTokens as _;
use std::collections::{HashMap, HashSet};
use syn::{Error, Result, Type};

use crate::parse::{BackwardsCompatInput, ExplicitNext, VersionEntry, VersionTag};

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
    #[allow(dead_code, reason = "used by DAG unit tests")]
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

/// Resolves an explicit `=> <Type>` transition target to either `TerminalTarget` or a concrete
/// version entry index.
///
/// Resolution order:
/// 1. Matches `input.target_ty` (`Self`) -> `NextTarget::TerminalTarget`.
/// 2. Matches the source entry's own type -> self-loop error.
/// 3. Matches the first concrete (non-untagged) entry per pipe-alias group with `cand.ty == *next_ty`:
///    - Rejects types that only appear on unversioned `_` entries or match no declared version.
///    - For unconditional sources, prefers unconditional candidates when present.
///    - Disambiguates multiple matches only when all candidates share the same `#[cfg(...)]`
///      signature and at most one defines an explicit `=>` transition (without conflicting with a
///      separate `current_version` candidate); otherwise returns an ambiguity error.
fn resolve_type_target(
    input: &BackwardsCompatInput,
    v: &VersionEntry,
    next_ty: &Type,
    next_span: proc_macro2::Span,
) -> Result<NextTarget> {
    if next_ty == &input.target_ty {
        return Ok(NextTarget::TerminalTarget);
    }
    if next_ty == &v.ty {
        return Err(Error::new(
            next_span,
            format!("self-loop transition in version `{}`", v.tag),
        ));
    }

    let mut all_tagged_primary: Vec<(usize, &VersionEntry)> = Vec::new();
    let mut group_has_concrete = false;
    for (idx, cand) in input.versions.iter().enumerate() {
        if !cand.is_alias {
            group_has_concrete = false;
        }
        if !group_has_concrete && !cand.tag.is_untagged() {
            group_has_concrete = true;
            if cand.ty == *next_ty {
                all_tagged_primary.push((idx, cand));
            }
        }
    }

    if all_tagged_primary.is_empty() {
        if input
            .versions
            .iter()
            .any(|cand| cand.tag.is_untagged() && cand.ty == *next_ty)
        {
            return Err(Error::new(
                next_span,
                format!(
                    "target type `{}` only appears on unversioned `_` entries and cannot be targeted by `=>`",
                    next_ty.to_token_stream()
                ),
            ));
        }
        return Err(Error::new(
            next_span,
            format!(
                "target type `{}` does not match any declared version or `Self`",
                next_ty.to_token_stream()
            ),
        ));
    }

    let candidates: Vec<(usize, &VersionEntry)> = if v.cfgs.is_empty() {
        let unconditional: Vec<(usize, &VersionEntry)> = all_tagged_primary
            .iter()
            .copied()
            .filter(|(_, cand)| cand.cfgs.is_empty())
            .collect();
        if unconditional.is_empty() {
            all_tagged_primary
        } else {
            unconditional
        }
    } else {
        all_tagged_primary
    };

    if let [(idx, _)] = candidates.as_slice() {
        return Ok(NextTarget::VersionIndex(*idx));
    }

    let format_ambiguity_err = |cands: &[(usize, &VersionEntry)]| {
        let tags = cands
            .iter()
            .map(|(_, cand)| format!("`{}`", cand.tag))
            .collect::<Vec<_>>()
            .join(", ");
        Error::new(
            next_span,
            format!(
                "target type `{}` is ambiguous (matched by versions {tags}); specify a version tag with `=>` instead",
                next_ty.to_token_stream()
            ),
        )
    };

    let cfg_sig =
        |cand: &VersionEntry| combine_cfg_all(cand.cfgs.iter().map(|c| &c.tokens)).map(|t| t.to_string());
    if let Some((_, first_cand)) = candidates.first() {
        let first_cfg = cfg_sig(first_cand);
        if candidates
            .iter()
            .skip(1)
            .any(|(_, cand)| cfg_sig(cand) != first_cfg)
        {
            return Err(format_ambiguity_err(&candidates));
        }
    }

    let explicit_candidates: Vec<(usize, &VersionEntry)> = candidates
        .iter()
        .copied()
        .filter(|(_, cand)| cand.explicit_next.is_some())
        .collect();

    match explicit_candidates.as_slice() {
        [] => candidates
            .last()
            .map(|&(last_idx, _)| NextTarget::VersionIndex(last_idx))
            .ok_or_else(|| format_ambiguity_err(&candidates)),
        [(explicit_idx, _)] => {
            if candidates
                .iter()
                .any(|(idx, cand)| *idx != *explicit_idx && cand.tag.matches(&input.current_version))
            {
                return Err(format_ambiguity_err(&candidates));
            }
            Ok(NextTarget::VersionIndex(*explicit_idx))
        }
        _ => Err(format_ambiguity_err(&candidates)),
    }
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

    // Pre-index first occurrence of each version tag in O(N) for O(1) tag lookups.
    let mut tag_to_first_idx: HashMap<&VersionTag, usize> = HashMap::with_capacity(n);
    for (idx, v) in input.versions.iter().enumerate() {
        tag_to_first_idx.entry(&v.tag).or_insert(idx);
    }

    let current_idx_opt = tag_to_first_idx.get(&input.current_version).copied();

    // Determine the next step for each version
    let mut next_targets: Vec<NextTarget> = Vec::with_capacity(n);

    for (i, v) in input.versions.iter().enumerate() {
        if let Some(ref explicit_next) = v.explicit_next {
            match explicit_next {
                ExplicitNext::SelfTarget(_) => {
                    next_targets.push(NextTarget::TerminalTarget);
                }
                ExplicitNext::Type(next_ty, next_span) => {
                    next_targets.push(resolve_type_target(input, v, next_ty, *next_span)?);
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
                    } else if let Some(&target_idx) = tag_to_first_idx.get(next_tag) {
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

    // Pre-index latest occurrences of each type (explicit/any, unconditional/any) in O(N).
    let mut last_explicit_any: HashMap<&Type, usize> = HashMap::with_capacity(n);
    let mut last_explicit_uncond: HashMap<&Type, usize> = HashMap::with_capacity(n);
    let mut last_any: HashMap<&Type, usize> = HashMap::with_capacity(n);
    let mut last_uncond: HashMap<&Type, usize> = HashMap::with_capacity(n);
    for (idx, cand) in input.versions.iter().enumerate() {
        last_any.insert(&cand.ty, idx);
        if cand.cfgs.is_empty() {
            last_uncond.insert(&cand.ty, idx);
        }
        if cand.explicit_next.is_some() {
            last_explicit_any.insert(&cand.ty, idx);
            if cand.cfgs.is_empty() {
                last_explicit_uncond.insert(&cand.ty, idx);
            }
        }
    }

    let mut transition_sources: Vec<usize> = (0..n).collect();
    for (i, v) in input.versions.iter().enumerate() {
        if v.explicit_next.is_none() && !v.tag.matches(&input.current_version) {
            let explicit_same_idx = if v.cfgs.is_empty() {
                last_explicit_uncond.get(&v.ty).copied()
            } else {
                last_explicit_any.get(&v.ty).copied()
            };
            if let Some(src_idx) = explicit_same_idx
                && let Some(target) = next_targets.get(src_idx).copied()
                && target != NextTarget::VersionIndex(i)
                && let Some(slot) = next_targets.get_mut(i)
            {
                *slot = target;
                if let Some(src_slot) = transition_sources.get_mut(i) {
                    *src_slot = src_idx;
                }
            } else {
                let last_same_idx = if v.cfgs.is_empty() {
                    last_uncond.get(&v.ty).copied().unwrap_or(i)
                } else {
                    last_any.get(&v.ty).copied().unwrap_or(i)
                };
                if last_same_idx > i
                    && let Some(target) = next_targets.get(last_same_idx).copied()
                    && target != NextTarget::VersionIndex(i)
                    && let Some(slot) = next_targets.get_mut(i)
                {
                    *slot = target;
                    if let Some(src_slot) = transition_sources.get_mut(i) {
                        *src_slot = last_same_idx;
                    }
                }
            }
        }
    }

    // Cycle detection via O(N) 3-color traversal (0 = unvisited, 1 = visiting, 2 = acyclic).
    let mut visit_state = vec![0u8; n];
    let mut path_nodes = Vec::new();
    for (i, v) in input.versions.iter().enumerate() {
        if visit_state.get(i).copied() == Some(2) {
            continue;
        }
        path_nodes.clear();
        let mut curr = i;
        while let Some(&state) = visit_state.get(curr) {
            if state == 2 {
                break;
            }
            if state == 1 {
                return Err(Error::new(
                    v.tag_span,
                    format!(
                        "cycle detected in backwards_compat migration graph starting at version `{}`",
                        v.tag
                    ),
                ));
            }
            if let Some(slot) = visit_state.get_mut(curr) {
                *slot = 1;
            }
            path_nodes.push(curr);
            match next_targets.get(curr).copied() {
                Some(NextTarget::VersionIndex(next_idx)) => curr = next_idx,
                Some(NextTarget::TerminalTarget) | None => break,
            }
        }
        for &node in &path_nodes {
            if let Some(slot) = visit_state.get_mut(node) {
                *slot = 2;
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
                        let transition_node = transition_sources.get(curr_node).copied().unwrap_or(curr_node);
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
                    if input.deserialize {
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
                    }
                    let next_ty = next_version.ty.clone();
                    if next_ty != curr_ty {
                        let transition_node = transition_sources.get(curr_node).copied().unwrap_or(curr_node);
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
mod tests;
