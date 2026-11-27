#![allow(clippy::indexing_slicing, reason = "index accesses in unit test assertions")]

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
    assert_eq!(plan1.serialize_mode, crate::parse::SerializeConvertMode::Into);

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
        plan2.serialize_mode,
        crate::parse::SerializeConvertMode::With(parse_quote!(to_v2))
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
            1 | 10: V1 => 2,
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
    assert_eq!(plan.paths[1].tag, VersionTag::Int(10));
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

    // Explicit wire type aliased on current version: `2 | 20: V2` -> 1 step `[V2 -> Target]` each
    let args1: BackwardsCompatArgs = syn::parse2(quote::quote! {
        versions = [
            1: V1,
            2 | 20: V2,
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

    // Shadow wire `Self` aliased on current version: `2 | 20: Self` -> 0 steps each
    let args2: BackwardsCompatArgs = syn::parse2(quote::quote! {
        versions = [
            1: V1,
            2 | 20: Self,
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

#[test]
fn test_dag_error_cases_self_loop_missing_target_and_cycle() {
    // Self-loop
    let self_loop_args: BackwardsCompatArgs = syn::parse2(quote::quote! {
        versions = [
            1: V1 => 1,
            2: Self,
        ]
    })
    .unwrap();
    let self_loop_input = self_loop_args
        .into_input(parse_quote!(Target), parse_quote!(pub), syn::Generics::default())
        .unwrap();
    let err = resolve_dag(&self_loop_input).unwrap_err();
    assert!(err.to_string().contains("self-loop transition in version `1`"));

    // Missing target
    let missing_args: BackwardsCompatArgs = syn::parse2(quote::quote! {
        versions = [
            1: V1 => 99,
            2: Self,
        ]
    })
    .unwrap();
    let missing_input = missing_args
        .into_input(parse_quote!(Target), parse_quote!(pub), syn::Generics::default())
        .unwrap();
    let err = resolve_dag(&missing_input).unwrap_err();
    assert!(err.to_string().contains("target version `99` does not exist"));

    // Cycle detection
    let cycle_args: BackwardsCompatArgs = syn::parse2(quote::quote! {
        versions = [
            1: V1 => 2,
            2: V2 => 1,
            3: Self,
        ]
    })
    .unwrap();
    let cycle_input = cycle_args
        .into_input(parse_quote!(Target), parse_quote!(pub), syn::Generics::default())
        .unwrap();
    let err = resolve_dag(&cycle_input).unwrap_err();
    assert!(err.to_string().contains("cycle detected"));
}

#[test]
fn test_dag_current_version_absent_from_versions_fallback() {
    let args: BackwardsCompatArgs = syn::parse2(quote::quote! {
        versions = [
            1: V1 => 99,
            2: V2,
        ]
    })
    .unwrap();
    let mut input = args
        .into_input(parse_quote!(Target), parse_quote!(pub), syn::Generics::default())
        .unwrap();
    // Force current_version to a tag not present in `versions` to exercise `current_idx_opt == None`.
    input.current_version = VersionTag::Int(99);
    input.serialize_version = VersionTag::Int(99);

    let plan = resolve_dag(&input).unwrap();
    assert!(plan.target_is_wire);
    assert!(plan.serialize_target_is_wire);
    assert_eq!(plan.paths.len(), 2);
    assert_eq!(plan.paths[0].steps.len(), 1);
    assert_eq!(plan.paths[0].steps[0].to_ty, parse_quote!(Target));
    assert_eq!(plan.paths[1].steps.len(), 1);
    assert_eq!(plan.paths[1].steps[0].to_ty, parse_quote!(Target));
}

fn resolve_tokens(tokens: proc_macro2::TokenStream, target_ty: Type) -> Result<DagPlan> {
    let args: BackwardsCompatArgs = syn::parse2(tokens)?;
    let input = args.into_input(target_ty, parse_quote!(pub), syn::Generics::default())?;
    resolve_dag(&input)
}

fn step_pairs(path: &ResolvedPath) -> Vec<(String, String)> {
    path.steps
        .iter()
        .map(|s| {
            (
                s.from_ty.to_token_stream().to_string(),
                s.to_ty.to_token_stream().to_string(),
            )
        })
        .collect()
}

#[test]
fn test_dag_explicit_type_chained_mixed_and_jump() {
    let plan = resolve_tokens(
        quote::quote! {
            versions = [
                1: V1 => 2,
                2: V2 => V3,
                3: V3 => V4,
                4: V4 => Self,
            ]
        },
        parse_quote!(Target),
    )
    .unwrap();

    assert_eq!(
        step_pairs(&plan.paths[0]),
        vec![
            ("V1".into(), "V2".into()),
            ("V2".into(), "V3".into()),
            ("V3".into(), "V4".into()),
            ("V4".into(), "Target".into()),
        ]
    );
    assert_eq!(
        step_pairs(&plan.paths[1]),
        vec![
            ("V2".into(), "V3".into()),
            ("V3".into(), "V4".into()),
            ("V4".into(), "Target".into()),
        ]
    );
    assert_eq!(
        step_pairs(&plan.paths[2]),
        vec![("V3".into(), "V4".into()), ("V4".into(), "Target".into())]
    );
    assert_eq!(step_pairs(&plan.paths[3]), vec![("V4".into(), "Target".into())]);

    // Jump over V2 directly to V3
    let jump_plan = resolve_tokens(
        quote::quote! {
            versions = [
                1: V1 => V3,
                2: V2,
                3: V3,
                4: Self,
            ]
        },
        parse_quote!(Target),
    )
    .unwrap();

    assert_eq!(
        step_pairs(&jump_plan.paths[0]),
        vec![("V1".into(), "V3".into()), ("V3".into(), "Target".into())]
    );
    assert_eq!(
        step_pairs(&jump_plan.paths[1]),
        vec![("V2".into(), "V3".into()), ("V3".into(), "Target".into())]
    );
    assert_eq!(
        step_pairs(&jump_plan.paths[2]),
        vec![("V3".into(), "Target".into())]
    );
    assert!(jump_plan.paths[3].steps.is_empty());
}

#[test]
fn test_dag_explicit_type_target_bypasses_current_wire() {
    let plan = resolve_tokens(
        quote::quote! {
            versions = [
                1: V1 => Target,
                #[current] 2: V2,
            ]
        },
        parse_quote!(Target),
    )
    .unwrap();

    assert!(!plan.target_is_wire);
    assert_eq!(step_pairs(&plan.paths[0]), vec![("V1".into(), "Target".into())]);
    assert_eq!(step_pairs(&plan.paths[1]), vec![("V2".into(), "Target".into())]);

    // Generic domain type target
    let generic_plan = resolve_tokens(
        quote::quote! {
            versions = [
                1: V1<'a, T> => Target<'a, T>,
                #[current] 2: V2<'a, T>,
            ]
        },
        parse_quote!(Target<'a, T>),
    )
    .unwrap();

    assert!(!generic_plan.target_is_wire);
    assert_eq!(
        step_pairs(&generic_plan.paths[0]),
        vec![(
            quote::quote!(V1<'a, T>).to_string(),
            quote::quote!(Target<'a, T>).to_string(),
        )]
    );
    assert_eq!(
        step_pairs(&generic_plan.paths[1]),
        vec![(
            quote::quote!(V2<'a, T>).to_string(),
            quote::quote!(Target<'a, T>).to_string(),
        )]
    );
}

#[test]
fn test_dag_bidirectional_duplicate_type_inheritance() {
    // Forward order: untagged before explicit
    let forward = resolve_tokens(
        quote::quote! {
            versions = [
                _: ConfigV1,
                _: ConfigV2,
                1: ConfigV1 => Self,
                2: ConfigV2 => Self,
                3: Self,
            ]
        },
        parse_quote!(Target),
    )
    .unwrap();

    assert_eq!(
        step_pairs(&forward.paths[0]),
        vec![("ConfigV1".into(), "Target".into())]
    );
    assert_eq!(
        step_pairs(&forward.paths[1]),
        vec![("ConfigV2".into(), "Target".into())]
    );
    assert_eq!(
        step_pairs(&forward.paths[2]),
        vec![("ConfigV1".into(), "Target".into())]
    );
    assert_eq!(
        step_pairs(&forward.paths[3]),
        vec![("ConfigV2".into(), "Target".into())]
    );
    assert!(forward.paths[4].steps.is_empty());

    // Reverse order: explicit before untagged
    let reverse = resolve_tokens(
        quote::quote! {
            versions = [
                1: ConfigV1 => Self,
                2: ConfigV2 => Self,
                _: ConfigV1,
                _: ConfigV2,
                3: Self,
            ]
        },
        parse_quote!(Target),
    )
    .unwrap();

    assert_eq!(
        step_pairs(&reverse.paths[0]),
        vec![("ConfigV1".into(), "Target".into())]
    );
    assert_eq!(
        step_pairs(&reverse.paths[1]),
        vec![("ConfigV2".into(), "Target".into())]
    );
    assert_eq!(
        step_pairs(&reverse.paths[2]),
        vec![("ConfigV1".into(), "Target".into())]
    );
    assert_eq!(
        step_pairs(&reverse.paths[3]),
        vec![("ConfigV2".into(), "Target".into())]
    );
    assert!(reverse.paths[4].steps.is_empty());

    // Mixed with => Type, #[fallible], and #[with(...)]
    let mixed = resolve_tokens(
        quote::quote! {
            versions = [
                #[fallible]
                #[with(my_fn)]
                1: V1 => V3,
                2: V1,
                3: V2,
                4: V3,
                5: Self,
            ]
        },
        parse_quote!(Target),
    )
    .unwrap();

    for idx in [0, 1] {
        assert_eq!(
            step_pairs(&mixed.paths[idx]),
            vec![("V1".into(), "V3".into()), ("V3".into(), "Target".into())]
        );
        assert!(mixed.paths[idx].steps[0].fallible);
        assert_eq!(
            mixed.paths[idx].steps[0]
                .with_fn
                .as_ref()
                .unwrap()
                .to_token_stream()
                .to_string(),
            "my_fn"
        );
        assert!(!mixed.paths[idx].steps[1].fallible);
        assert!(mixed.paths[idx].steps[1].with_fn.is_none());
    }
}

#[test]
fn test_dag_explicit_type_multiple_rows_without_conflict() {
    // Multiple implicit rows of same type
    let plan1 = resolve_tokens(
        quote::quote! {
            versions = [
                1: V1 => V2,
                2: V2,
                3: V2,
                4: V3,
                5: Self,
            ]
        },
        parse_quote!(Target),
    )
    .unwrap();
    assert_eq!(
        step_pairs(&plan1.paths[0]),
        vec![
            ("V1".into(), "V2".into()),
            ("V2".into(), "V3".into()),
            ("V3".into(), "Target".into()),
        ]
    );

    // One explicit row + one implicit row of same type
    let plan2 = resolve_tokens(
        quote::quote! {
            versions = [
                1: V1 => V2,
                2: V2 => V4,
                3: V2,
                4: V4,
                5: Self,
            ]
        },
        parse_quote!(Target),
    )
    .unwrap();
    assert_eq!(
        step_pairs(&plan2.paths[0]),
        vec![
            ("V1".into(), "V2".into()),
            ("V2".into(), "V4".into()),
            ("V4".into(), "Target".into()),
        ]
    );
    assert_eq!(
        step_pairs(&plan2.paths[1]),
        vec![("V2".into(), "V4".into()), ("V4".into(), "Target".into())]
    );
    assert_eq!(
        step_pairs(&plan2.paths[2]),
        vec![("V2".into(), "V4".into()), ("V4".into(), "Target".into())]
    );

    // Pipe alias row
    let plan3 = resolve_tokens(
        quote::quote! {
            versions = [
                1: V1 => V2,
                2 | 20: V2,
                3: Self,
            ]
        },
        parse_quote!(Target),
    )
    .unwrap();
    assert_eq!(
        step_pairs(&plan3.paths[0]),
        vec![("V1".into(), "V2".into()), ("V2".into(), "Target".into())]
    );

    // Untagged row shares type with tagged row
    let plan4 = resolve_tokens(
        quote::quote! {
            versions = [
                _: V2,
                _: V0 => V2,
                1: V2,
                2: Self,
            ]
        },
        parse_quote!(Target),
    )
    .unwrap();
    assert_eq!(
        step_pairs(&plan4.paths[1]),
        vec![("V0".into(), "V2".into()), ("V2".into(), "Target".into())]
    );
}

#[test]
fn test_dag_explicit_type_cfg_interactions() {
    // Gated -> gated combines with all(...)
    let gated_to_gated = resolve_tokens(
        quote::quote! {
            versions = [
                #[cfg(feature = "a")]
                1: V1 => V2,
                #[cfg(feature = "b")]
                2: V2,
                3: Self,
            ]
        },
        parse_quote!(Target),
    )
    .unwrap();
    assert_eq!(
        step_pairs(&gated_to_gated.paths[0]),
        vec![("V1".into(), "V2".into()), ("V2".into(), "Target".into())]
    );
    assert_eq!(
        gated_to_gated.paths[0].cfg.as_ref().unwrap().to_string(),
        quote::quote!(all(feature = "a", feature = "b")).to_string()
    );

    // Unconditional -> gated errors
    let uncond_to_gated = resolve_tokens(
        quote::quote! {
            versions = [
                1: V1 => V2,
                #[cfg(feature = "b")]
                2: V2,
                3: Self,
            ]
        },
        parse_quote!(Target),
    )
    .unwrap_err();
    assert!(
        uncond_to_gated
            .to_string()
            .contains("unconditional version `1` cannot migrate through conditionally compiled version `2`")
    );

    // Unconditional -> type with both gated and unconditional resolves to unconditional
    let uncond_narrowed = resolve_tokens(
        quote::quote! {
            versions = [
                1: V1 => V2,
                #[cfg(feature = "b")]
                2: V2,
                3: V2,
                4: Self,
            ]
        },
        parse_quote!(Target),
    )
    .unwrap();
    assert!(uncond_narrowed.paths[0].cfg.is_none());
    assert_eq!(
        step_pairs(&uncond_narrowed.paths[0]),
        vec![("V1".into(), "V2".into()), ("V2".into(), "Target".into())]
    );

    // Gated -> type with two differently-gated entries errors as ambiguous
    let gated_ambig = resolve_tokens(
        quote::quote! {
            versions = [
                #[cfg(feature = "a")]
                1: V1 => V2,
                #[cfg(feature = "b")]
                2: V2,
                #[cfg(feature = "c")]
                3: V2,
                4: Self,
            ]
        },
        parse_quote!(Target),
    )
    .unwrap_err();
    assert!(gated_ambig.to_string().contains("target type `V2` is ambiguous"));
}

#[test]
fn test_dag_explicit_type_error_cases() {
    // Missing type
    let err_missing = resolve_tokens(
        quote::quote! {
            versions = [
                1: V1 => NonExistent,
                2: Self,
            ]
        },
        parse_quote!(Target),
    )
    .unwrap_err();
    assert!(
        err_missing
            .to_string()
            .contains("target type `NonExistent` does not match any declared version or `Self`")
    );

    // Self-loop
    let err_self_loop = resolve_tokens(
        quote::quote! {
            versions = [
                1: V1 => V1,
                2: Self,
            ]
        },
        parse_quote!(Target),
    )
    .unwrap_err();
    assert!(
        err_self_loop
            .to_string()
            .contains("self-loop transition in version `1`")
    );

    // Cycle
    let err_cycle = resolve_tokens(
        quote::quote! {
            versions = [
                1: V1 => V2,
                2: V2 => V1,
                3: Self,
            ]
        },
        parse_quote!(Target),
    )
    .unwrap_err();
    assert!(err_cycle.to_string().contains("cycle detected"));

    // Conflicting multiple explicit transitions
    let err_conflict = resolve_tokens(
        quote::quote! {
            versions = [
                1: V1 => V2,
                2: V2 => V4,
                3: V2 => V5,
                4: V4,
                5: V5,
                6: Self,
            ]
        },
        parse_quote!(Target),
    )
    .unwrap_err();
    assert!(err_conflict.to_string().contains("target type `V2` is ambiguous"));

    // Single explicit alongside #[current] of same type
    let err_current_conflict = resolve_tokens(
        quote::quote! {
            versions = [
                1: V1 => V2,
                2: V2 => Self,
                #[current] 3: V2,
            ]
        },
        parse_quote!(Target),
    )
    .unwrap_err();
    assert!(
        err_current_conflict
            .to_string()
            .contains("target type `V2` is ambiguous")
    );

    // Type only on untagged _
    let err_untagged_only = resolve_tokens(
        quote::quote! {
            versions = [
                _: V0,
                1: V1 => V0,
                2: Self,
            ]
        },
        parse_quote!(Target),
    )
    .unwrap_err();
    assert!(
        err_untagged_only.to_string().contains(
            "target type `V0` only appears on unversioned `_` entries and cannot be targeted by `=>`"
        )
    );
}

#[test]
fn test_dag_non_inheriting_duplicate_type_metadata_isolation() {
    // #[current] entry of same type must not inherit #[with] or #[fallible] from an explicit entry
    let plan1 = resolve_tokens(
        quote::quote! {
            versions = [
                #[fallible]
                #[with(v1_to_v2)]
                1: V1 => 3,
                #[current]
                2: V1,
                3: V2,
            ]
        },
        parse_quote!(Target),
    )
    .unwrap();

    assert_eq!(plan1.paths[0].steps[0].from_ty, parse_quote!(V1));
    assert_eq!(plan1.paths[0].steps[0].to_ty, parse_quote!(V2));
    assert!(plan1.paths[0].steps[0].fallible);
    assert_eq!(
        plan1.paths[0].steps[0]
            .with_fn
            .as_ref()
            .unwrap()
            .to_token_stream()
            .to_string(),
        "v1_to_v2"
    );

    assert_eq!(step_pairs(&plan1.paths[1]), vec![("V1".into(), "Target".into())]);
    assert!(!plan1.paths[1].steps[0].fallible);
    assert!(plan1.paths[1].steps[0].with_fn.is_none());

    // Entry 2 guarded against self-loop retains its own #[fallible] on V1 -> V3
    let plan2 = resolve_tokens(
        quote::quote! {
            versions = [
                1: V1 => 2,
                #[fallible]
                2: V1,
                3: V3,
            ]
        },
        parse_quote!(Target),
    )
    .unwrap();

    assert_eq!(
        step_pairs(&plan2.paths[1]),
        vec![("V1".into(), "V3".into()), ("V3".into(), "Target".into())]
    );
    assert!(plan2.paths[1].steps[0].fallible);
    assert!(!plan2.paths[1].steps[1].fallible);
}

#[test]
fn test_dag_explicit_type_untagged_leading_pipe_alias_group() {
    // `_ | 2: V2` should be targetable via `=> V2`
    let plan1 = resolve_tokens(
        quote::quote! {
            versions = [
                1: V1 => V2,
                _ | 2: V2,
                3: Self,
            ]
        },
        parse_quote!(Target),
    )
    .unwrap();

    assert_eq!(
        step_pairs(&plan1.paths[0]),
        vec![("V1".into(), "V2".into()), ("V2".into(), "Target".into())]
    );

    // `_ | 2 | 20: V2 => V3` should resolve cleanly to `2: V2` without ambiguity
    let plan2 = resolve_tokens(
        quote::quote! {
            versions = [
                1: V1 => V2,
                _ | 2 | 20: V2 => V3,
                3: V3,
                4: Self,
            ]
        },
        parse_quote!(Target),
    )
    .unwrap();

    assert_eq!(
        step_pairs(&plan2.paths[0]),
        vec![
            ("V1".into(), "V2".into()),
            ("V2".into(), "V3".into()),
            ("V3".into(), "Target".into()),
        ]
    );
}

#[test]
fn test_dag_deserialize_false_allows_unconditional_before_cfg_gated_version() {
    // When `deserialize = false`, migration paths are not used, so an unconditional
    // historical entry followed by a `#[cfg(...)]`-gated entry must succeed.
    let plan = resolve_tokens(
        quote::quote! {
            deserialize = false,
            versions = [
                #[serializable] 1: V1,
                #[cfg(feature = "v2")]
                #[serializable] 2: V2,
                3: Self,
            ]
        },
        parse_quote!(Target),
    )
    .unwrap();

    assert!(plan.target_is_wire);
    assert!(plan.serialize_target_is_wire);
    assert_eq!(plan.serialize_wire_ty, parse_quote!(Target));
    assert_eq!(plan.serialize_mode, crate::parse::SerializeConvertMode::Into);
    assert_eq!(plan.paths.len(), 3);
    assert!(plan.paths[0].cfg.is_none());
    assert_eq!(
        plan.paths[1].cfg.as_ref().unwrap().to_string(),
        quote::quote!(feature = "v2").to_string()
    );
    assert!(plan.paths[2].cfg.is_none());

    // Also verify `#[serialize(with = ...)]` on an unconditional entry before a gated entry.
    let plan_ser = resolve_tokens(
        quote::quote! {
            deserialize = false,
            versions = [
                #[serialize(with = to_v1)] 1: V1,
                #[cfg(feature = "v2")]
                #[serializable] 2: V2,
                #[current] 3: Self,
            ]
        },
        parse_quote!(Target),
    )
    .unwrap();

    assert!(plan_ser.target_is_wire);
    assert!(!plan_ser.serialize_target_is_wire);
    assert_eq!(plan_ser.serialize_wire_ty, parse_quote!(V1));
    assert_eq!(
        plan_ser.serialize_mode,
        crate::parse::SerializeConvertMode::With(parse_quote!(to_v1))
    );

    // With `deserialize = true` (explicitly or by default), the same layout is rejected.
    for tokens in [
        quote::quote! {
            deserialize = true,
            versions = [
                #[serializable] 1: V1,
                #[cfg(feature = "v2")]
                #[serializable] 2: V2,
                3: Self,
            ]
        },
        quote::quote! {
            versions = [
                #[serializable] 1: V1,
                #[cfg(feature = "v2")]
                #[serializable] 2: V2,
                3: Self,
            ]
        },
    ] {
        let err = resolve_tokens(tokens, parse_quote!(Target)).unwrap_err();
        assert_eq!(
            err.to_string(),
            "unconditional version `1` cannot migrate through conditionally compiled version `2`; add `#[cfg(...)]` or use an explicit `=>` transition to an unconditional target"
        );
    }
}
