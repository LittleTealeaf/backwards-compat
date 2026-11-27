#![allow(clippy::indexing_slicing, reason = "index accesses in unit test assertions")]

use super::*;

#[test]
fn test_parse_args_basic() {
    let input = quote::quote! {
        tag = "schema_version",
        dump,
        versions = [
            1: ModelV1 => 2,
            2: ModelV2 => 3,
            #[current] 3: ModelV3,
        ]
    };
    let args: BackwardsCompatArgs = syn::parse2(input).unwrap();
    assert_eq!(args.tag_field, "schema_version");
    assert!(args.dump);
    assert_eq!(args.versions.len(), 3);
    assert_eq!(args.versions[0].tag, VersionTag::Int(1));
    assert!(args.versions[2].is_current);
}

#[test]
fn test_parse_args_paren() {
    let input = quote::quote! {
        tag("schema_version"),
        versions(
            1: ModelV1 => 2,
            #[fallible] 2: ModelV2 => 3,
            3: Self,
        )
    };
    let args: BackwardsCompatArgs = syn::parse2(input).unwrap();
    assert_eq!(args.tag_field, "schema_version");
    assert_eq!(args.versions.len(), 3);
    assert!(args.versions[1].fallible);
}

#[test]
fn test_parse_args_with_strings() {
    let input = quote::quote! {
        tag = "schema",
        dump,
        versions(
            "v1": ModelV1 => "v2",
            #[fallible] "v2": ModelV2 => "v3",
            "v3": Self,
        )
    };
    let args: BackwardsCompatArgs = syn::parse2(input).unwrap();
    assert_eq!(args.tag_field, "schema");
    assert_eq!(args.versions.len(), 3);
    assert_eq!(args.versions[0].tag, VersionTag::String("v1".to_owned()));
    assert!(matches!(
        args.versions[0].explicit_next,
        Some(ExplicitNext::Tag(VersionTag::String(ref s), _)) if s == "v2"
    ));
    assert!(!args.versions[0].fallible);
    assert_eq!(args.versions[1].tag, VersionTag::String("v2".to_owned()));
    assert!(args.versions[1].fallible);
    assert!(args.dump);
}

#[test]
fn test_parse_args_error_ty_removed() {
    let input1 = quote::quote! {
        error = MyCustomError,
        versions = [ 1: ModelV1 ]
    };
    let err1 = syn::parse2::<BackwardsCompatArgs>(input1).unwrap_err();
    assert!(err1.to_string().contains("`error = ...` was removed"));

    let input2 = quote::quote! {
        error(MyCustomError),
        versions = [ 1: ModelV1 ]
    };
    let err2 = syn::parse2::<BackwardsCompatArgs>(input2).unwrap_err();
    assert!(err2.to_string().contains("`error = ...` was removed"));
}

#[test]
fn test_parse_args_braced_versions() {
    let input = quote::quote! {
        versions = {
            1: ModelV1 => 2,
            2: ModelV2,
        }
    };
    let args: BackwardsCompatArgs = syn::parse2(input).unwrap();
    assert_eq!(args.versions.len(), 2);
}

#[test]
fn test_parse_args_into_input_success() {
    let input = quote::quote! {
        tag = "custom_tag",
        dump,
        versions = [
            1: ModelV1 => 2,
            #[current] 2: ModelV2,
        ]
    };
    let args: BackwardsCompatArgs = syn::parse2(input).unwrap();
    let target_ty: syn::Type = syn::parse_quote!(TargetModel);
    let vis: syn::Visibility = syn::parse_quote!(pub);
    let compat_input = args.into_input(target_ty, vis, syn::Generics::default()).unwrap();

    assert_eq!(compat_input.tag_field, "custom_tag");
    assert_eq!(compat_input.current_version, VersionTag::Int(2));
    assert!(compat_input.dump);
    assert_eq!(compat_input.versions.len(), 2);
}

#[test]
fn test_parse_args_into_input_default_version() {
    let input = quote::quote! {
        versions = [
            1: ModelV1 => 2,
            2: ModelV2,
        ]
    };
    let args: BackwardsCompatArgs = syn::parse2(input).unwrap();
    let target_ty: syn::Type = syn::parse_quote!(TargetModel);
    let vis: syn::Visibility = syn::Visibility::Inherited;
    let compat_input = args.into_input(target_ty, vis, syn::Generics::default()).unwrap();

    assert_eq!(compat_input.current_version, VersionTag::Int(2));
}

#[test]
fn test_parse_args_into_input_empty_versions_allowed() {
    let input = quote::quote! {
        versions = []
    };
    let args: BackwardsCompatArgs = syn::parse2(input).unwrap();
    let target_ty: syn::Type = syn::parse_quote!(TargetModel);
    let vis: syn::Visibility = syn::Visibility::Inherited;
    let compat_input = args.into_input(target_ty, vis, syn::Generics::default()).unwrap();
    assert_eq!(compat_input.current_version, VersionTag::Int(1));
    assert!(compat_input.versions.is_empty());
}

#[test]
fn test_parse_args_into_input_duplicate_version_tag_error() {
    let input = quote::quote! {
        versions = [
            1: ModelV1,
            1: ModelV1Dup,
        ]
    };
    let args: BackwardsCompatArgs = syn::parse2(input).unwrap();
    let target_ty: syn::Type = syn::parse_quote!(TargetModel);
    let vis: syn::Visibility = syn::Visibility::Inherited;
    let err = args
        .into_input(target_ty, vis, syn::Generics::default())
        .unwrap_err();
    assert_eq!(err.to_string(), "duplicate version tag `1`");
}

#[test]
fn test_parse_args_top_level_version_removed_error() {
    let input = quote::quote! {
        version = 99,
        versions = [
            1: ModelV1 => 2,
            2: ModelV2,
        ]
    };
    let err = syn::parse2::<BackwardsCompatArgs>(input).unwrap_err();
    assert!(err.to_string().contains("top-level `version` was removed in 2.0"));
}

#[test]
fn test_parse_args_invalid_keys() {
    let input = quote::quote! {
        unknown_arg = 123
    };
    let err = syn::parse2::<BackwardsCompatArgs>(input).unwrap_err();
    assert_eq!(err.to_string(), "unknown attribute argument `unknown_arg`");
}

#[test]
fn test_parse_args_invalid_syntax() {
    let input_tag = quote::quote! {
        tag 123
    };
    let err_tag = syn::parse2::<BackwardsCompatArgs>(input_tag).unwrap_err();
    assert_eq!(err_tag.to_string(), "expected `=` or `()` for tag");

    let input_error = quote::quote! {
        error 123
    };
    let err_error = syn::parse2::<BackwardsCompatArgs>(input_error).unwrap_err();
    assert!(err_error.to_string().contains("`error = ...` was removed"));

    let input_content = quote::quote! {
        content 123
    };
    let err_content = syn::parse2::<BackwardsCompatArgs>(input_content).unwrap_err();
    assert_eq!(err_content.to_string(), "expected `=` or `()` for content");

    let input_versions = quote::quote! {
        versions: [1: ModelV1]
    };
    let err_versions = syn::parse2::<BackwardsCompatArgs>(input_versions).unwrap_err();
    assert_eq!(
        err_versions.to_string(),
        "expected `(...)`, `= [...]`, or `= { ... }` for versions"
    );
}

#[test]
fn test_parse_args_content_eq() {
    let input = quote::quote! {
        content = "data",
        versions = [1: ModelV1]
    };
    let args: BackwardsCompatArgs = syn::parse2(input).unwrap();
    assert_eq!(args.content_field.as_deref(), Some("data"));

    let target_ty: syn::Type = syn::parse_quote!(TargetModel);
    let vis: syn::Visibility = syn::Visibility::Inherited;
    let compat_input = args.into_input(target_ty, vis, syn::Generics::default()).unwrap();
    assert_eq!(compat_input.content_field.as_deref(), Some("data"));
}

#[test]
fn test_parse_args_content_paren() {
    let input = quote::quote! {
        content("payload"),
        versions = [1: ModelV1]
    };
    let args: BackwardsCompatArgs = syn::parse2(input).unwrap();
    assert_eq!(args.content_field.as_deref(), Some("payload"));

    let target_ty: syn::Type = syn::parse_quote!(TargetModel);
    let vis: syn::Visibility = syn::Visibility::Inherited;
    let compat_input = args.into_input(target_ty, vis, syn::Generics::default()).unwrap();
    assert_eq!(compat_input.content_field.as_deref(), Some("payload"));
}

#[test]
fn test_parse_args_content_omitted_is_none() {
    let input = quote::quote! {
        tag = "schema",
        versions = [1: ModelV1]
    };
    let args: BackwardsCompatArgs = syn::parse2(input).unwrap();
    assert_eq!(args.content_field, None);

    let target_ty: syn::Type = syn::parse_quote!(TargetModel);
    let vis: syn::Visibility = syn::Visibility::Inherited;
    let compat_input = args.into_input(target_ty, vis, syn::Generics::default()).unwrap();
    assert_eq!(compat_input.content_field, None);
}

#[test]
fn test_parse_args_content_with_tag_and_version() {
    let input = quote::quote! {
        tag = "type",
        content = "data",
        dump,
        versions = [
            1: ModelV1 => 2,
            2: ModelV2,
        ]
    };
    let args: BackwardsCompatArgs = syn::parse2(input).unwrap();
    assert_eq!(args.tag_field, "type");
    assert_eq!(args.content_field.as_deref(), Some("data"));
    assert!(args.dump);

    let target_ty: syn::Type = syn::parse_quote!(TargetModel);
    let vis: syn::Visibility = syn::parse_quote!(pub);
    let compat_input = args.into_input(target_ty, vis, syn::Generics::default()).unwrap();
    assert_eq!(compat_input.tag_field, "type");
    assert_eq!(compat_input.content_field.as_deref(), Some("data"));
    assert_eq!(compat_input.current_version, VersionTag::Int(2));
    assert!(compat_input.dump);
    assert_eq!(compat_input.versions.len(), 2);
}

#[test]
fn test_parse_args_ident_version_key_error() {
    let input = quote::quote! {
        versions = [
            v1: ModelV1,
        ]
    };
    let err = syn::parse2::<BackwardsCompatArgs>(input).unwrap_err();
    assert_eq!(
        err.to_string(),
        "version keys must be string or integer literals (e.g. 1 or \"1\"), or `_` for unversioned, not identifiers like v1"
    );
}

#[test]
fn test_parse_args_untagged_wildcard() {
    let input = quote::quote! {
        versions = [
            _: ModelV0,
            1: ModelV1,
        ]
    };
    let args: BackwardsCompatArgs = syn::parse2(input).unwrap();
    assert_eq!(args.versions.len(), 2);
    assert_eq!(args.versions[0].tag, VersionTag::Untagged);
    assert_eq!(args.versions[1].tag, VersionTag::Int(1));
}

#[test]
fn test_parse_args_untagged_self_normalized_and_default_version() {
    let input = quote::quote! {
        versions(_: Self)
    };
    let args: BackwardsCompatArgs = syn::parse2(input).unwrap();
    let target_ty: syn::Type = syn::parse_quote!(TargetModel<T>);
    let vis: syn::Visibility = syn::parse_quote!(pub);
    let compat_input = args
        .into_input(target_ty.clone(), vis, syn::Generics::default())
        .unwrap();
    assert_eq!(compat_input.current_version, VersionTag::Int(1));
    assert_eq!(compat_input.versions.len(), 1);
    assert_eq!(compat_input.versions[0].tag, VersionTag::Untagged);
    assert_eq!(compat_input.versions[0].ty, target_ty);
}

#[test]
fn test_parse_args_untagged_invalid_contexts() {
    let err2 = syn::parse2::<BackwardsCompatArgs>(quote::quote! {
        versions(1: ModelV1 => _)
    })
    .unwrap_err();
    assert_eq!(
        err2.to_string(),
        "unversioned `_` cannot be used as target of `=>`"
    );
}

#[test]
fn test_parse_args_fallible_attribute() {
    let input = quote::quote! {
        versions = [
            #[fallible] 1: V1 => 2,
            2: V2,
        ]
    };
    let args: BackwardsCompatArgs = syn::parse2(input).unwrap();
    assert_eq!(args.versions.len(), 2);
    assert!(args.versions[0].fallible);
    assert!(!args.versions[1].fallible);
}

#[test]
fn test_parse_args_cfg_single() {
    let input = quote::quote! {
        versions = [
            #[cfg(test)]
            1: ModelV1 => 2,
            #[cfg(feature = "compat")]
            2: ModelV2 => 3,
            3: ModelV3,
        ]
    };
    let args: BackwardsCompatArgs = syn::parse2(input).unwrap();
    assert_eq!(args.versions.len(), 3);

    assert_eq!(args.versions[0].cfgs.len(), 1);
    assert_eq!(args.versions[0].cfgs[0].tokens.to_string(), "test");
    assert!(!args.versions[0].fallible);

    assert_eq!(args.versions[1].cfgs.len(), 1);
    assert_eq!(
        args.versions[1].cfgs[0].tokens.to_string(),
        quote::quote!(feature = "compat").to_string()
    );
    assert!(!args.versions[1].fallible);

    assert!(args.versions[2].cfgs.is_empty());
}

#[test]
fn test_parse_args_cfg_multiple_on_single_entry() {
    let input = quote::quote! {
        versions = [
            #[cfg(feature = "v1")]
            #[cfg(not(target_arch = "wasm32"))]
            #[cfg(all(unix, debug_assertions))]
            1: ModelV1 => 2,
            2: ModelV2,
        ]
    };
    let args: BackwardsCompatArgs = syn::parse2(input).unwrap();
    assert_eq!(args.versions.len(), 2);
    assert_eq!(args.versions[0].cfgs.len(), 3);
    assert_eq!(
        args.versions[0].cfgs[0].tokens.to_string(),
        quote::quote!(feature = "v1").to_string()
    );
    assert_eq!(
        args.versions[0].cfgs[1].tokens.to_string(),
        quote::quote!(not(target_arch = "wasm32")).to_string()
    );
    assert_eq!(
        args.versions[0].cfgs[2].tokens.to_string(),
        quote::quote!(all(unix, debug_assertions)).to_string()
    );
    assert!(args.versions[1].cfgs.is_empty());
}

#[test]
fn test_parse_args_cfg_combined_with_fallible_both_orders() {
    let input = quote::quote! {
        versions = [
            #[cfg(feature = "compat")]
            #[fallible]
            1: ModelV1 => 2,
            #[fallible]
            #[cfg(test)]
            2: ModelV2 => 3,
            3: ModelV3,
        ]
    };
    let args: BackwardsCompatArgs = syn::parse2(input).unwrap();
    assert_eq!(args.versions.len(), 3);

    assert!(args.versions[0].fallible);
    assert_eq!(args.versions[0].cfgs.len(), 1);
    assert_eq!(
        args.versions[0].cfgs[0].tokens.to_string(),
        quote::quote!(feature = "compat").to_string()
    );

    assert!(args.versions[1].fallible);
    assert_eq!(args.versions[1].cfgs.len(), 1);
    assert_eq!(args.versions[1].cfgs[0].tokens.to_string(), "test");

    assert!(!args.versions[2].fallible);
    assert!(args.versions[2].cfgs.is_empty());
}

#[test]
fn test_parse_args_doc_comments_silently_ignored() {
    let input = quote::quote! {
        versions = [
            /// Legacy version 1.
            #[cfg(feature = "v1")]
            1: ModelV1 => 2,
            #[doc = "Explicit doc attribute"]
            #[fallible]
            2: ModelV2 => 3,
            /// Current version.
            3: ModelV3,
        ]
    };
    let args: BackwardsCompatArgs = syn::parse2(input).unwrap();
    assert_eq!(args.versions.len(), 3);

    assert_eq!(args.versions[0].cfgs.len(), 1);
    assert_eq!(
        args.versions[0].cfgs[0].tokens.to_string(),
        quote::quote!(feature = "v1").to_string()
    );
    assert!(!args.versions[0].fallible);

    assert!(args.versions[1].cfgs.is_empty());
    assert!(args.versions[1].fallible);

    assert!(args.versions[2].cfgs.is_empty());
    assert!(!args.versions[2].fallible);
}

#[test]
fn test_parse_args_invalid_cfg_forms_error() {
    let invalid_cases = [
        quote::quote! {
            versions = [
                #[cfg]
                1: ModelV1,
            ]
        },
        quote::quote! {
            versions = [
                #[cfg = "foo"]
                1: ModelV1,
            ]
        },
        quote::quote! {
            versions = [
                #[cfg()]
                1: ModelV1,
            ]
        },
    ];

    for input in invalid_cases {
        let err = syn::parse2::<BackwardsCompatArgs>(input).unwrap_err();
        assert_eq!(
            err.to_string(),
            "expected `#[cfg(...)]` with a non-empty predicate"
        );
    }
}

#[test]
fn test_parse_args_unsupported_attributes_error() {
    let err_typo = syn::parse2::<BackwardsCompatArgs>(quote::quote! {
        versions = [
            #[cgf(test)]
            1: ModelV1,
        ]
    })
    .unwrap_err();
    assert_eq!(
        err_typo.to_string(),
        "unsupported attribute `cgf` on version entry; only `#[cfg(...)]`, `#[current]`, `#[serialize]`, `#[serializable]`, `#[fallible]`, and `#[with(...)]` are supported"
    );

    let err_serde = syn::parse2::<BackwardsCompatArgs>(quote::quote! {
        versions = [
            #[serde(rename = "x")]
            1: ModelV1,
        ]
    })
    .unwrap_err();
    assert_eq!(
        err_serde.to_string(),
        "unsupported attribute `serde` on version entry; only `#[cfg(...)]`, `#[current]`, `#[serialize]`, `#[serializable]`, `#[fallible]`, and `#[with(...)]` are supported"
    );

    let err_path = syn::parse2::<BackwardsCompatArgs>(quote::quote! {
        versions = [
            #[foo::bar]
            1: ModelV1,
        ]
    })
    .unwrap_err();
    assert_eq!(
        err_path.to_string(),
        format!(
            "unsupported attribute `{}` on version entry; only `#[cfg(...)]`, `#[current]`, `#[serialize]`, `#[serializable]`, `#[fallible]`, and `#[with(...)]` are supported",
            quote::quote!(foo::bar)
        )
    );
}

#[test]
fn test_into_input_rejects_cfg_on_explicit_current_version() {
    let input = quote::quote! {
        versions = [
            1: V1 => 2,
            #[current]
            #[cfg(test)]
            2: V2,
        ]
    };
    let args: BackwardsCompatArgs = syn::parse2(input).unwrap();
    let target_ty: syn::Type = syn::parse_quote!(TargetModel);
    let vis: syn::Visibility = syn::Visibility::Inherited;
    let err = args
        .into_input(target_ty, vis, syn::Generics::default())
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "active current version `2` cannot be conditionally compiled with `#[cfg(...)]`"
    );
}

#[test]
fn test_into_input_rejects_cfg_on_implicit_current_version() {
    let input = quote::quote! {
        versions = [
            1: V1 => 2,
            #[cfg(feature = "v2")]
            2: V2,
        ]
    };
    let args: BackwardsCompatArgs = syn::parse2(input).unwrap();
    let target_ty: syn::Type = syn::parse_quote!(TargetModel);
    let vis: syn::Visibility = syn::Visibility::Inherited;
    let err = args
        .into_input(target_ty, vis, syn::Generics::default())
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "active current version `2` cannot be conditionally compiled with `#[cfg(...)]`"
    );
}

#[test]
fn test_into_input_allows_all_historical_versions_gated_with_self_current() {
    let input = quote::quote! {
        versions = [
            #[cfg(feature = "v1")]
            1: V1 => 2,
            #[cfg(feature = "v2")]
            2: V2 => 3,
            3: Self,
        ]
    };
    let args: BackwardsCompatArgs = syn::parse2(input).unwrap();
    let target_ty: syn::Type = syn::parse_quote!(TargetModel);
    let vis: syn::Visibility = syn::Visibility::Inherited;
    let compat_input = args.into_input(target_ty, vis, syn::Generics::default()).unwrap();
    assert_eq!(compat_input.current_version, VersionTag::Int(3));
    assert_eq!(compat_input.versions.len(), 3);
    assert_eq!(compat_input.versions[0].cfgs.len(), 1);
    assert_eq!(compat_input.versions[1].cfgs.len(), 1);
    assert!(compat_input.versions[2].cfgs.is_empty());
}

#[test]
fn test_into_input_allows_cfg_on_historical_versions_with_ungated_current() {
    let input = quote::quote! {
        versions = [
            #[cfg(feature = "legacy")]
            _: V0 => 1,
            #[cfg(feature = "v1")]
            1: V1 => 2,
            2: V2,
        ]
    };
    let args: BackwardsCompatArgs = syn::parse2(input).unwrap();
    let target_ty: syn::Type = syn::parse_quote!(TargetModel);
    let vis: syn::Visibility = syn::Visibility::Inherited;
    let compat_input = args.into_input(target_ty, vis, syn::Generics::default()).unwrap();
    assert_eq!(compat_input.current_version, VersionTag::Int(2));
    assert_eq!(compat_input.versions.len(), 3);
    assert_eq!(compat_input.versions[0].cfgs.len(), 1);
    assert_eq!(compat_input.versions[1].cfgs.len(), 1);
    assert!(compat_input.versions[2].cfgs.is_empty());
}

#[test]
fn test_strict_version_tag_matches_and_duplicates() {
    assert!(!VersionTag::Int(1).matches(&VersionTag::String("1".to_owned())));
    assert!(VersionTag::Int(1).matches(&VersionTag::Int(1)));
    assert!(VersionTag::String("1".to_owned()).matches(&VersionTag::String("1".to_owned())));

    // Homogeneous int pipe alias `1 | 10` succeeds
    let args: BackwardsCompatArgs = syn::parse2(quote::quote! {
        versions = [
            1 | 10: V1,
        ]
    })
    .unwrap();
    let compat_input = args
        .into_input(
            syn::parse_quote!(TargetModel),
            syn::Visibility::Inherited,
            syn::Generics::default(),
        )
        .unwrap();
    assert_eq!(compat_input.versions.len(), 2);
    assert_eq!(compat_input.versions[0].tag, VersionTag::Int(1));
    assert_eq!(compat_input.versions[1].tag, VersionTag::Int(10));

    // Mixing int and str tags (either inline `1 | "1"` or across entries) is rejected
    for mixed in [
        quote::quote! { versions = [1 | "1": V1] },
        quote::quote! { versions = [1: V1 => "2", "2": V2] },
        quote::quote! { versions = ["v1": V1, 2: V2] },
    ] {
        let mixed_args: BackwardsCompatArgs = syn::parse2(mixed).unwrap();
        let err_mixed = mixed_args
            .into_input(
                syn::parse_quote!(TargetModel),
                syn::Visibility::Inherited,
                syn::Generics::default(),
            )
            .unwrap_err();
        assert!(
            err_mixed
                .to_string()
                .contains("cannot mix integer and string version tags"),
            "unexpected mixed error: {err_mixed}"
        );
    }

    // Duplicate int tag 1 | 1 fails
    let dup_int: BackwardsCompatArgs = syn::parse2(quote::quote! {
        versions = [
            1 | 1: V1,
        ]
    })
    .unwrap();
    let err_int = dup_int
        .into_input(
            syn::parse_quote!(TargetModel),
            syn::Visibility::Inherited,
            syn::Generics::default(),
        )
        .unwrap_err();
    assert_eq!(err_int.to_string(), "duplicate version tag `1`");

    // Duplicate string tag "1" | "1" fails
    let dup_str: BackwardsCompatArgs = syn::parse2(quote::quote! {
        versions = [
            "1" | "1": V1,
        ]
    })
    .unwrap();
    let err_str = dup_str
        .into_input(
            syn::parse_quote!(TargetModel),
            syn::Visibility::Inherited,
            syn::Generics::default(),
        )
        .unwrap_err();
    assert_eq!(err_str.to_string(), "duplicate version tag `1`");
}

#[test]
fn test_removed_legacy_aliases_and_top_level_version_error() {
    for key in [quote::quote!(none), quote::quote!(None), quote::quote!(untagged)] {
        let err = syn::parse2::<BackwardsCompatArgs>(quote::quote! {
            versions = [
                #key: V0,
                1: V1,
            ]
        })
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            "version keys must be string or integer literals (e.g. 1 or \"1\"), or `_` for unversioned, not identifiers like v1"
        );
    }

    for attr in [
        quote::quote!(#[try_from]),
        quote::quote!(#[falliable]),
        quote::quote!(#[r#try]),
    ] {
        let err = syn::parse2::<BackwardsCompatArgs>(quote::quote! {
            versions = [
                #attr 1: V1,
            ]
        })
        .unwrap_err();
        assert!(err.to_string().contains("unsupported attribute"));
    }

    let err_version = syn::parse2::<BackwardsCompatArgs>(quote::quote! {
        version = 2,
        versions = [1: V1]
    })
    .unwrap_err();
    assert_eq!(
        err_version.to_string(),
        "top-level `version` was removed in 2.0; declare it inside `versions(...)` using `#[current]` or `N: Self`"
    );
}

#[test]
fn test_parse_explicit_next_self_target() {
    let args: BackwardsCompatArgs = syn::parse2(quote::quote! {
        versions = [
            1: V1 => Self,
            2: V2,
        ]
    })
    .unwrap();
    assert!(matches!(
        args.versions[0].explicit_next,
        Some(ExplicitNext::SelfTarget(_))
    ));
    assert!(args.versions[1].explicit_next.is_none());
}

#[test]
fn test_parse_and_validate_current_attr() {
    // Non-last entry marked #[current]
    let args: BackwardsCompatArgs = syn::parse2(quote::quote! {
        versions = [
            #[current] 1: V1,
            2: V2 => 1,
        ]
    })
    .unwrap();
    let compat_input = args
        .into_input(
            syn::parse_quote!(TargetModel),
            syn::Visibility::Inherited,
            syn::Generics::default(),
        )
        .unwrap();
    assert_eq!(compat_input.current_version, VersionTag::Int(1));
    assert_eq!(compat_input.serialize_version, VersionTag::Int(1));

    // Duplicate #[current] on same entry
    let err_same = syn::parse2::<BackwardsCompatArgs>(quote::quote! {
        versions = [#[current] #[current] 1: V1]
    })
    .unwrap_err();
    assert_eq!(
        err_same.to_string(),
        "duplicate `#[current]` attribute on version entry"
    );

    // Non-bare #[current(...)]
    let err_args = syn::parse2::<BackwardsCompatArgs>(quote::quote! {
        versions = [#[current(foo)] 1: V1]
    })
    .unwrap_err();
    assert_eq!(err_args.to_string(), "expected bare `#[current]` attribute");

    // Duplicate #[current] across entries
    let dup_across: BackwardsCompatArgs = syn::parse2(quote::quote! {
        versions = [
            #[current] 1: V1,
            #[current] 2: V2,
        ]
    })
    .unwrap();
    let err_across = dup_across
        .into_input(
            syn::parse_quote!(TargetModel),
            syn::Visibility::Inherited,
            syn::Generics::default(),
        )
        .unwrap_err();
    assert_eq!(
        err_across.to_string(),
        "duplicate `#[current]` attribute; at most one version may be marked `#[current]`"
    );

    // #[current] on _
    let err_untagged = syn::parse2::<BackwardsCompatArgs>(quote::quote! {
        versions = [#[current] _: V0]
    })
    .unwrap_err();
    assert_eq!(
        err_untagged.to_string(),
        "unversioned `_` cannot be marked `#[current]`"
    );

    // #[current] with #[cfg(...)]
    let cfg_current: BackwardsCompatArgs = syn::parse2(quote::quote! {
        versions = [
            #[current]
            #[cfg(test)]
            1: V1,
            2: V2,
        ]
    })
    .unwrap();
    let err_cfg = cfg_current
        .into_input(
            syn::parse_quote!(TargetModel),
            syn::Visibility::Inherited,
            syn::Generics::default(),
        )
        .unwrap_err();
    assert_eq!(
        err_cfg.to_string(),
        "active current version `1` cannot be conditionally compiled with `#[cfg(...)]`"
    );
}

#[test]
fn test_parse_and_validate_serialize_attr() {
    // Bare #[serialize] on non-current entry
    let bare: BackwardsCompatArgs = syn::parse2(quote::quote! {
        versions = [
            #[serialize] 1: V1 => 2,
            #[current] 2: V2,
        ]
    })
    .unwrap();
    let bare_input = bare
        .into_input(
            syn::parse_quote!(TargetModel),
            syn::Visibility::Inherited,
            syn::Generics::default(),
        )
        .unwrap();
    assert_eq!(bare_input.current_version, VersionTag::Int(2));
    assert_eq!(bare_input.serialize_version, VersionTag::Int(1));
    assert!(bare_input.serialize_with_fn.is_none());

    // #[serialize(with = my_mod::to_v6)]
    let with_fn: BackwardsCompatArgs = syn::parse2(quote::quote! {
        versions = [
            #[serialize(with = my_mod::to_v6)] 6: V6 => 7,
            #[current] 7: V7,
        ]
    })
    .unwrap();
    let with_input = with_fn
        .into_input(
            syn::parse_quote!(TargetModel),
            syn::Visibility::Inherited,
            syn::Generics::default(),
        )
        .unwrap();
    assert_eq!(with_input.serialize_version, VersionTag::Int(6));
    assert_eq!(
        with_input
            .serialize_with_fn
            .as_ref()
            .unwrap()
            .to_token_stream()
            .to_string(),
        quote::quote!(my_mod::to_v6).to_string()
    );

    // Duplicate #[serialize] on same entry
    let err_same = syn::parse2::<BackwardsCompatArgs>(quote::quote! {
        versions = [#[serialize] #[serialize] 1: V1]
    })
    .unwrap_err();
    assert_eq!(
        err_same.to_string(),
        "duplicate `#[serialize]` attribute on version entry"
    );

    // Duplicate #[serialize] across entries
    let dup_across: BackwardsCompatArgs = syn::parse2(quote::quote! {
        versions = [
            #[serialize] 1: V1 => 2,
            #[serialize] 2: V2,
        ]
    })
    .unwrap();
    let err_across = dup_across
        .into_input(
            syn::parse_quote!(TargetModel),
            syn::Visibility::Inherited,
            syn::Generics::default(),
        )
        .unwrap_err();
    assert_eq!(
        err_across.to_string(),
        "duplicate `#[serialize]` attribute; at most one version may be marked `#[serialize]`"
    );

    // Invalid args
    let err_bad_kw = syn::parse2::<BackwardsCompatArgs>(quote::quote! {
        versions = [#[serialize(foo = bar)] 1: V1]
    })
    .unwrap_err();
    assert_eq!(
        err_bad_kw.to_string(),
        "expected `with = <fn_path>` inside `#[serialize(...)]`"
    );

    let err_trailing = syn::parse2::<BackwardsCompatArgs>(quote::quote! {
        versions = [#[serialize(with = my_fn extra)] 1: V1]
    })
    .unwrap_err();
    assert_eq!(
        err_trailing.to_string(),
        "unexpected trailing tokens in `#[serialize(with = ...)]`"
    );

    let err_nv = syn::parse2::<BackwardsCompatArgs>(quote::quote! {
        versions = [#[serialize = "v1"] 1: V1]
    })
    .unwrap_err();
    assert_eq!(
        err_nv.to_string(),
        "expected `#[serialize]` or `#[serialize(with = fn_path)]`"
    );

    // #[serialize] on _
    let err_untagged = syn::parse2::<BackwardsCompatArgs>(quote::quote! {
        versions = [#[serialize] _: V0, 1: V1]
    })
    .unwrap_err();
    assert_eq!(
        err_untagged.to_string(),
        "unversioned `_` cannot be marked `#[serialize]`"
    );

    // #[serialize] with #[cfg(...)]
    let cfg_ser: BackwardsCompatArgs = syn::parse2(quote::quote! {
        versions = [
            #[serialize]
            #[cfg(test)]
            1: V1 => 2,
            #[current]
            2: V2,
        ]
    })
    .unwrap();
    let err_cfg = cfg_ser
        .into_input(
            syn::parse_quote!(TargetModel),
            syn::Visibility::Inherited,
            syn::Generics::default(),
        )
        .unwrap_err();
    assert_eq!(
        err_cfg.to_string(),
        "serialize version `1` cannot be conditionally compiled with `#[cfg(...)]`"
    );
}

#[test]
fn test_parse_and_validate_with_attr() {
    let args: BackwardsCompatArgs = syn::parse2(quote::quote! {
        versions = [
            #[with(my_mod::upgrade)] 1: V1 => 2,
            2: V2,
        ]
    })
    .unwrap();
    assert_eq!(
        args.versions[0]
            .with_fn
            .as_ref()
            .unwrap()
            .to_token_stream()
            .to_string(),
        quote::quote!(my_mod::upgrade).to_string()
    );
    assert!(args.versions[1].with_fn.is_none());

    // Duplicate #[with(...)]
    let err_dup = syn::parse2::<BackwardsCompatArgs>(quote::quote! {
        versions = [#[with(a)] #[with(b)] 1: V1]
    })
    .unwrap_err();
    assert_eq!(
        err_dup.to_string(),
        "duplicate `#[with(...)]` attribute on version entry"
    );

    // Invalid syntax
    for invalid in [
        quote::quote! { versions = [#[with] 1: V1] },
        quote::quote! { versions = [#[with = "a"] 1: V1] },
        quote::quote! { versions = [#[with()] 1: V1] },
    ] {
        let _ = syn::parse2::<BackwardsCompatArgs>(invalid).unwrap_err();
    }
}

#[test]
fn test_parse_inline_pipe_aliases_and_multiple_untagged() {
    let args: BackwardsCompatArgs = syn::parse2(quote::quote! {
        versions = [
            _: LegacyA => 6,
            _: LegacyB => 6,
            #[cfg(test)]
            #[with(my_mod::up)]
            3 | 30: V3 => Self,
            #[serialize(with = my_mod::ser)]
            5 | 50: V5 => 6,
            #[current]
            6 | 60: V6,
        ]
    })
    .unwrap();

    let compat_input = args
        .into_input(
            syn::parse_quote!(TargetModel),
            syn::Visibility::Inherited,
            syn::Generics::default(),
        )
        .unwrap();

    assert_eq!(compat_input.versions.len(), 8);
    assert_eq!(compat_input.versions[0].tag, VersionTag::Untagged);
    assert_eq!(compat_input.versions[1].tag, VersionTag::Untagged);

    // 3 | 30 both inherit cfg, with_fn, and => Self
    assert_eq!(compat_input.versions[2].tag, VersionTag::Int(3));
    assert_eq!(compat_input.versions[3].tag, VersionTag::Int(30));
    assert_eq!(compat_input.versions[2].cfgs.len(), 1);
    assert_eq!(compat_input.versions[3].cfgs.len(), 1);
    assert!(compat_input.versions[2].with_fn.is_some());
    assert!(compat_input.versions[3].with_fn.is_some());
    assert!(matches!(
        compat_input.versions[2].explicit_next,
        Some(ExplicitNext::SelfTarget(_))
    ));
    assert!(matches!(
        compat_input.versions[3].explicit_next,
        Some(ExplicitNext::SelfTarget(_))
    ));

    // #[serialize(with = ...)] applies only to first tag (5), not alias (50)
    assert_eq!(compat_input.serialize_version, VersionTag::Int(5));
    assert!(compat_input.versions[4].is_serialize);
    assert!(compat_input.versions[4].serialize_with_fn.is_some());
    assert!(!compat_input.versions[5].is_serialize);
    assert!(compat_input.versions[5].serialize_with_fn.is_none());

    // #[current] applies only to first tag (6), not alias (60)
    assert_eq!(compat_input.current_version, VersionTag::Int(6));
    assert!(compat_input.versions[6].is_current);
    assert!(!compat_input.versions[7].is_current);
}

#[test]
fn test_default_current_version_prefers_primary_tag_over_pipe_alias() {
    // When #[current] is omitted and last entry is `2 | 20: Self`, default to primary `Int(2)`
    let args1: BackwardsCompatArgs = syn::parse2(quote::quote! {
        versions(
            1: V1,
            2 | 20: Self,
        )
    })
    .unwrap();
    let input1 = args1
        .into_input(
            syn::parse_quote!(TargetModel),
            syn::Visibility::Inherited,
            syn::Generics::default(),
        )
        .unwrap();
    assert_eq!(input1.current_version, VersionTag::Int(2));
    assert_eq!(input1.serialize_version, VersionTag::Int(2));
    assert!(!input1.versions[1].is_alias);
    assert!(input1.versions[2].is_alias);

    // When primary tag is `_` (`_ | 2: Self`), falls back to concrete alias `Int(2)`
    let args2: BackwardsCompatArgs = syn::parse2(quote::quote! {
        versions(
            _ | 2: Self,
        )
    })
    .unwrap();
    let input2 = args2
        .into_input(
            syn::parse_quote!(TargetModel),
            syn::Visibility::Inherited,
            syn::Generics::default(),
        )
        .unwrap();
    assert_eq!(input2.current_version, VersionTag::Int(2));
    assert_eq!(input2.serialize_version, VersionTag::Int(2));
}

#[test]
fn test_version_tag_helpers_to_tokens_display_and_invalid_literal() {
    let untagged = VersionTag::Untagged;
    let int_tag = VersionTag::Int(42);
    let str_tag = VersionTag::String("v1.0".to_owned());

    assert_eq!(untagged.to_tag_string(), "_");
    assert_eq!(int_tag.to_tag_string(), "42");
    assert_eq!(str_tag.to_tag_string(), "v1.0");

    assert_eq!(untagged.to_string(), "_");
    assert_eq!(int_tag.to_string(), "42");
    assert_eq!(str_tag.to_string(), "v1.0");

    assert_eq!(untagged.to_token_stream().to_string(), "_");
    assert_eq!(int_tag.to_token_stream().to_string(), "42");
    assert_eq!(str_tag.to_token_stream().to_string(), "\"v1.0\"");

    let err = VersionTag::error(proc_macro2::Span::call_site(), "boom");
    assert_eq!(err.to_string(), "boom");

    let bad_lit = syn::parse2::<BackwardsCompatArgs>(quote::quote! {
        versions(3.14: V1)
    })
    .unwrap_err();
    assert!(bad_lit.to_string().contains("expected integer or string literal"));
}

#[test]
fn test_parse_explicit_next_type_targets() {
    let args: BackwardsCompatArgs = syn::parse2(quote::quote! {
        versions(
            1: V1 => V2,
            2: V2 => path::to::V3,
            3: V3 => V4<'a, T>,
            4: V4 => TargetModel,
            5 | 50: V5 => TargetModel,
        )
    })
    .unwrap();

    assert_eq!(args.versions.len(), 6);

    let expected = [
        quote::quote!(V2).to_string(),
        quote::quote!(path::to::V3).to_string(),
        quote::quote!(V4<'a, T>).to_string(),
        quote::quote!(TargetModel).to_string(),
        quote::quote!(TargetModel).to_string(),
        quote::quote!(TargetModel).to_string(),
    ];

    for (entry, expected_ty) in args.versions.iter().zip(expected) {
        match &entry.explicit_next {
            Some(ExplicitNext::Type(ty, _)) => {
                assert_eq!(ty.to_token_stream().to_string(), expected_ty);
            }
            other => panic!("expected ExplicitNext::Type({expected_ty}), got {other:?}"),
        }
    }
}

#[test]
fn test_parse_explicit_next_self_associated_type() {
    let args: BackwardsCompatArgs = syn::parse2(quote::quote! {
        versions(
            1: V1 => Self::Item,
            2: V2 => Self,
        )
    })
    .unwrap();

    assert_eq!(args.versions.len(), 2);

    match &args.versions[0].explicit_next {
        Some(ExplicitNext::Type(ty, _)) => {
            assert_eq!(
                ty.to_token_stream().to_string(),
                quote::quote!(Self::Item).to_string()
            );
        }
        other => panic!("expected ExplicitNext::Type(Self::Item), got {other:?}"),
    }

    assert!(matches!(
        args.versions[1].explicit_next,
        Some(ExplicitNext::SelfTarget(_))
    ));
}

#[test]
fn test_parse_explicit_next_existing_forms() {
    let args: BackwardsCompatArgs = syn::parse2(quote::quote! {
        versions(
            1: V1 => 2,
            3: V3 => Self,
            4: V4,
        )
    })
    .unwrap();

    assert_eq!(args.versions.len(), 3);
    assert!(matches!(
        args.versions[0].explicit_next,
        Some(ExplicitNext::Tag(VersionTag::Int(2), _))
    ));
    assert!(matches!(
        args.versions[1].explicit_next,
        Some(ExplicitNext::SelfTarget(_))
    ));
    assert!(args.versions[2].explicit_next.is_none());

    let str_args: BackwardsCompatArgs = syn::parse2(quote::quote! {
        versions(
            "v1": V1Str => "v2",
            "v2": V2Str,
        )
    })
    .unwrap();
    assert!(matches!(
        str_args.versions[0].explicit_next,
        Some(ExplicitNext::Tag(VersionTag::String(ref s), _)) if s == "v2"
    ));
}

#[test]
fn test_parse_explicit_next_errors() {
    let err_untagged = syn::parse2::<BackwardsCompatArgs>(quote::quote! {
        versions(1: V1 => _)
    })
    .unwrap_err();
    assert_eq!(
        err_untagged.to_string(),
        "unversioned `_` cannot be used as target of `=>`"
    );

    let _ = syn::parse2::<BackwardsCompatArgs>(quote::quote! {
        versions(1: V1 => ,)
    })
    .unwrap_err();

    let _ = syn::parse2::<BackwardsCompatArgs>(quote::quote! {
        versions(1: V1 =>)
    })
    .unwrap_err();
}

#[test]
fn test_parse_empty_and_colliding_tag_content_rejected() {
    for input in [
        quote::quote!(tag = "", versions(1: V1)),
        quote::quote!(tag(""), versions(1: V1)),
    ] {
        let err = syn::parse2::<BackwardsCompatArgs>(input).unwrap_err();
        assert_eq!(err.to_string(), "tag field name cannot be empty");
    }

    for input in [
        quote::quote!(content = "", versions(1: V1)),
        quote::quote!(content(""), versions(1: V1)),
    ] {
        let err = syn::parse2::<BackwardsCompatArgs>(input).unwrap_err();
        assert_eq!(err.to_string(), "content field name cannot be empty");
    }

    for input in [
        quote::quote!(tag = "v", content = "v", versions(1: V1)),
        quote::quote!(content = "version", versions(1: V1)),
    ] {
        let err = syn::parse2::<BackwardsCompatArgs>(input).unwrap_err();
        assert!(
            err.to_string()
                .contains("tag and content field names cannot be the same")
        );
    }

    // into_input also catches tag == content (e.g. field-level defaulted content = "data")
    let mut field_like: BackwardsCompatArgs = syn::parse2(quote::quote! {
        tag = "data",
        versions(1: V1)
    })
    .unwrap();
    field_like.content_field = Some("data".to_owned());
    let err = field_like
        .into_input(
            syn::parse_quote!(u32),
            syn::Visibility::Inherited,
            syn::Generics::default(),
        )
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "tag and content field names cannot be the same (`data`)"
    );
}

#[test]
fn test_parse_and_validate_serializable_attr() {
    let args: BackwardsCompatArgs = syn::parse2(quote::quote! {
        versions = [
            #[serializable] 1 | 10: V1 => 2,
            #[serializable(with = my_mod::to_v2)] 2: V2 => 3,
            #[serializable(try_with = my_mod::try_to_v3)] #[with(my_mod::up_v3)] 3: V3 => 4,
            #[current] #[serializable] 4: Self,
        ]
    })
    .unwrap();
    assert_eq!(args.versions[0].serializable, Some(SerializableMode::Infallible));
    assert_eq!(args.versions[1].serializable, None); // pipe alias does not inherit
    assert_eq!(
        args.versions[2].serializable,
        Some(SerializableMode::With(syn::parse_quote!(my_mod::to_v2)))
    );
    assert_eq!(
        args.versions[3].serializable,
        Some(SerializableMode::TryWith(syn::parse_quote!(my_mod::try_to_v3)))
    );
    assert_eq!(args.versions[4].serializable, Some(SerializableMode::Infallible));

    // Duplicate #[serializable]
    let err_dup = syn::parse2::<BackwardsCompatArgs>(quote::quote! {
        versions = [#[serializable] #[serializable] 1: V1]
    })
    .unwrap_err();
    assert_eq!(
        err_dup.to_string(),
        "duplicate `#[serializable]` attribute on version entry"
    );

    // #[serializable] on _
    let err_untagged = syn::parse2::<BackwardsCompatArgs>(quote::quote! {
        versions = [#[serializable] _: V0, 1: V1]
    })
    .unwrap_err();
    assert_eq!(
        err_untagged.to_string(),
        "unversioned `_` cannot be marked `#[serializable]`"
    );

    // Invalid #[serializable(...)] forms
    let err_kw = syn::parse2::<BackwardsCompatArgs>(quote::quote! {
        versions = [#[serializable(foo = bar)] 1: V1]
    })
    .unwrap_err();
    assert_eq!(
        err_kw.to_string(),
        "expected `with = <fn_path>` or `try_with = <fn_path>` inside `#[serializable(...)]`"
    );

    let err_trailing = syn::parse2::<BackwardsCompatArgs>(quote::quote! {
        versions = [#[serializable(with = my_fn extra)] 1: V1]
    })
    .unwrap_err();
    assert_eq!(
        err_trailing.to_string(),
        "unexpected trailing tokens in `#[serializable(with = ...)]`"
    );

    let err_try_trailing = syn::parse2::<BackwardsCompatArgs>(quote::quote! {
        versions = [#[serializable(try_with = my_fn extra)] 1: V1]
    })
    .unwrap_err();
    assert_eq!(
        err_try_trailing.to_string(),
        "unexpected trailing tokens in `#[serializable(try_with = ...)]`"
    );

    let err_nv = syn::parse2::<BackwardsCompatArgs>(quote::quote! {
        versions = [#[serializable = "v1"] 1: V1]
    })
    .unwrap_err();
    assert_eq!(
        err_nv.to_string(),
        "expected `#[serializable]`, `#[serializable(with = fn_path)]`, or `#[serializable(try_with = fn_path)]`"
    );
}

#[test]
fn test_parse_and_validate_external_flag() {
    let args: BackwardsCompatArgs = syn::parse2(quote::quote! {
        external,
        versions(1: V1, 2: Self)
    })
    .unwrap();
    assert!(args.external);
    let input = args
        .into_input(
            syn::parse_quote!(TargetModel),
            syn::Visibility::Inherited,
            syn::Generics::default(),
        )
        .unwrap();
    assert!(input.external);

    for (tokens, expected_msg) in [
        (
            quote::quote!(external = true, versions(1: V1)),
            "`external` does not take a value",
        ),
        (
            quote::quote!(external(true), versions(1: V1)),
            "`external` does not take a value",
        ),
        (
            quote::quote!(external, external, versions(1: V1)),
            "duplicate `external` attribute argument",
        ),
        (
            quote::quote!(external, tag = "v", versions(1: V1)),
            "cannot combine `external` with `tag = ...`",
        ),
        (
            quote::quote!(tag = "v", external, versions(1: V1)),
            "cannot combine `external` with `tag = ...`",
        ),
        (
            quote::quote!(external, content = "d", versions(1: V1)),
            "cannot combine `external` with `content = ...`",
        ),
        (
            quote::quote!(content = "d", external, versions(1: V1)),
            "cannot combine `external` with `content = ...`",
        ),
    ] {
        let err = syn::parse2::<BackwardsCompatArgs>(tokens).unwrap_err();
        assert!(
            err.to_string().contains(expected_msg),
            "expected `{expected_msg}`, got `{err}`"
        );
    }

    let untagged_args: BackwardsCompatArgs = syn::parse2(quote::quote! {
        external,
        versions(_: V0 => 1, 1: V1)
    })
    .unwrap();
    let err_untagged = untagged_args
        .into_input(
            syn::parse_quote!(TargetModel),
            syn::Visibility::Inherited,
            syn::Generics::default(),
        )
        .unwrap_err();
    assert_eq!(
        err_untagged.to_string(),
        "unversioned `_` cannot be used with `external` tagging"
    );
}

#[test]
fn test_parse_serialize_deserialize_defaults() {
    let args: BackwardsCompatArgs = syn::parse2(quote::quote! {
        versions(1: V1, 2: Self)
    })
    .unwrap();
    assert!(args.serialize);
    assert!(args.deserialize);
    assert!(!args.serialize_specified);
    assert!(!args.deserialize_specified);

    let input = args
        .into_input(
            syn::parse_quote!(TargetModel),
            syn::Visibility::Inherited,
            syn::Generics::default(),
        )
        .unwrap();
    assert!(input.serialize);
    assert!(input.deserialize);
}

#[test]
fn test_parse_serialize_option_forms() {
    for (tokens, expected_serialize) in [
        (quote::quote!(serialize = true, versions(1: V1, 2: Self)), true),
        (quote::quote!(serialize = false, versions(1: V1, 2: Self)), false),
        (quote::quote!(serialize(true), versions(1: V1, 2: Self)), true),
        (quote::quote!(serialize(false), versions(1: V1, 2: Self)), false),
    ] {
        let args: BackwardsCompatArgs = syn::parse2(tokens).unwrap();
        assert_eq!(args.serialize, expected_serialize);
        assert!(args.deserialize);
        assert!(args.serialize_specified);
        assert!(!args.deserialize_specified);

        let input = args
            .into_input(
                syn::parse_quote!(TargetModel),
                syn::Visibility::Inherited,
                syn::Generics::default(),
            )
            .unwrap();
        assert_eq!(input.serialize, expected_serialize);
        assert!(input.deserialize);
    }
}

#[test]
fn test_parse_deserialize_option_forms() {
    for (tokens, expected_deserialize) in [
        (quote::quote!(deserialize = true, versions(1: V1, 2: Self)), true),
        (
            quote::quote!(deserialize = false, versions(1: V1, 2: Self)),
            false,
        ),
        (quote::quote!(deserialize(true), versions(1: V1, 2: Self)), true),
        (quote::quote!(deserialize(false), versions(1: V1, 2: Self)), false),
    ] {
        let args: BackwardsCompatArgs = syn::parse2(tokens).unwrap();
        assert!(args.serialize);
        assert_eq!(args.deserialize, expected_deserialize);
        assert!(!args.serialize_specified);
        assert!(args.deserialize_specified);

        let input = args
            .into_input(
                syn::parse_quote!(TargetModel),
                syn::Visibility::Inherited,
                syn::Generics::default(),
            )
            .unwrap();
        assert!(input.serialize);
        assert_eq!(input.deserialize, expected_deserialize);
    }
}

#[test]
fn test_parse_serialize_deserialize_combined_valid() {
    for (tokens, expected_serialize, expected_deserialize) in [
        (
            quote::quote!(serialize = true, deserialize = true, versions(1: V1, 2: Self)),
            true,
            true,
        ),
        (
            quote::quote!(serialize = false, deserialize = true, versions(1: V1, 2: Self)),
            false,
            true,
        ),
        (
            quote::quote!(serialize(true), deserialize(false), versions(1: V1, 2: Self)),
            true,
            false,
        ),
    ] {
        let args: BackwardsCompatArgs = syn::parse2(tokens).unwrap();
        assert_eq!(args.serialize, expected_serialize);
        assert_eq!(args.deserialize, expected_deserialize);
        assert!(args.serialize_specified);
        assert!(args.deserialize_specified);

        let input = args
            .into_input(
                syn::parse_quote!(TargetModel),
                syn::Visibility::Inherited,
                syn::Generics::default(),
            )
            .unwrap();
        assert_eq!(input.serialize, expected_serialize);
        assert_eq!(input.deserialize, expected_deserialize);
    }
}

#[test]
fn test_parse_serialize_deserialize_duplicate_errors() {
    for (tokens, expected_msg) in [
        (
            quote::quote!(serialize = true, serialize = false, versions(1: Self)),
            "duplicate `serialize` attribute argument",
        ),
        (
            quote::quote!(serialize(true), serialize(true), versions(1: Self)),
            "duplicate `serialize` attribute argument",
        ),
        (
            quote::quote!(deserialize = true, deserialize = false, versions(1: Self)),
            "duplicate `deserialize` attribute argument",
        ),
        (
            quote::quote!(deserialize(false), deserialize(true), versions(1: Self)),
            "duplicate `deserialize` attribute argument",
        ),
    ] {
        let err = syn::parse2::<BackwardsCompatArgs>(tokens).unwrap_err();
        assert_eq!(err.to_string(), expected_msg);
    }
}

#[test]
fn test_parse_serialize_deserialize_invalid_syntax_and_values() {
    for (tokens, expected_msg) in [
        (
            quote::quote!(serialize, versions(1: Self)),
            "expected `= true|false` or `(true|false)` for `serialize`",
        ),
        (
            quote::quote!(deserialize, versions(1: Self)),
            "expected `= true|false` or `(true|false)` for `deserialize`",
        ),
        (
            quote::quote!(serialize = "false", versions(1: Self)),
            "expected boolean literal (`true` or `false`) for `serialize`",
        ),
        (
            quote::quote!(serialize = 0, versions(1: Self)),
            "expected boolean literal (`true` or `false`) for `serialize`",
        ),
        (
            quote::quote!(deserialize = "true", versions(1: Self)),
            "expected boolean literal (`true` or `false`) for `deserialize`",
        ),
        (
            quote::quote!(serialize("false"), versions(1: Self)),
            "expected boolean literal (`true` or `false`) for `serialize`",
        ),
        (
            quote::quote!(deserialize(1), versions(1: Self)),
            "expected boolean literal (`true` or `false`) for `deserialize`",
        ),
        (
            quote::quote!(serialize(), versions(1: Self)),
            "expected boolean literal (`true` or `false`) for `serialize`",
        ),
        (
            quote::quote!(deserialize(), versions(1: Self)),
            "expected boolean literal (`true` or `false`) for `deserialize`",
        ),
        (
            quote::quote!(serialize(true, false), versions(1: Self)),
            "unexpected trailing tokens in `serialize(...)`",
        ),
        (
            quote::quote!(deserialize(false, true), versions(1: Self)),
            "unexpected trailing tokens in `deserialize(...)`",
        ),
    ] {
        let err = syn::parse2::<BackwardsCompatArgs>(tokens).unwrap_err();
        assert_eq!(err.to_string(), expected_msg);
    }
}

#[test]
fn test_reject_both_serialize_and_deserialize_false() {
    let expected = "cannot disable both `serialize` and `deserialize` in `#[backwards_compat(...)]`; at least one must be enabled";

    // Rejected during Parse
    for tokens in [
        quote::quote!(serialize = false, deserialize = false, versions(1: Self)),
        quote::quote!(deserialize(false), serialize(false), versions(1: Self)),
    ] {
        let err = syn::parse2::<BackwardsCompatArgs>(tokens).unwrap_err();
        assert_eq!(err.to_string(), expected);
    }

    // Rejected defensively in into_input
    let manual_args = BackwardsCompatArgs {
        tag_field: "version".to_owned(),
        content_field: None,
        external: false,
        dump: false,
        serialize: false,
        deserialize: false,
        serialize_specified: true,
        deserialize_specified: true,
        versions: Vec::new(),
    };
    let err = manual_args
        .into_input(
            syn::parse_quote!(TargetModel),
            syn::Visibility::Inherited,
            syn::Generics::default(),
        )
        .unwrap_err();
    assert_eq!(err.to_string(), expected);
}

#[test]
fn test_serialize_false_rejects_serialize_and_serializable_attrs() {
    for (tokens, expected_msg) in [
        (
            quote::quote!(
                serialize = false,
                versions(#[serialize] 1: V1, 2: Self)
            ),
            "cannot use `#[serialize]` when `serialize = false`",
        ),
        (
            quote::quote!(
                serialize = false,
                versions(#[serialize(with = to_v1)] 1: V1, 2: Self)
            ),
            "cannot use `#[serialize]` when `serialize = false`",
        ),
        (
            quote::quote!(
                serialize = false,
                versions(#[serializable] 1: V1, 2: Self)
            ),
            "cannot use `#[serializable]` when `serialize = false`",
        ),
        (
            quote::quote!(
                serialize = false,
                versions(#[serializable(with = to_v1)] 1: V1, 2: Self)
            ),
            "cannot use `#[serializable]` when `serialize = false`",
        ),
        (
            quote::quote!(
                serialize = false,
                versions(#[serializable(try_with = try_to_v1)] 1: V1, 2: Self)
            ),
            "cannot use `#[serializable]` when `serialize = false`",
        ),
    ] {
        let args: BackwardsCompatArgs = syn::parse2(tokens).unwrap();
        let err = args
            .into_input(
                syn::parse_quote!(TargetModel),
                syn::Visibility::Inherited,
                syn::Generics::default(),
            )
            .unwrap_err();
        assert_eq!(err.to_string(), expected_msg);
    }

    // Allows deserialization features when serialize = false
    let args: BackwardsCompatArgs = syn::parse2(quote::quote! {
        serialize = false,
        versions(
            _: V0 => 1,
            #[fallible] 1: V1 => 2,
            #[with(migrate_v2)] 2: V2 => Self,
            #[current] 3: Self,
        )
    })
    .unwrap();
    let input = args
        .into_input(
            syn::parse_quote!(TargetModel),
            syn::Visibility::Inherited,
            syn::Generics::default(),
        )
        .unwrap();
    assert!(!input.serialize);
    assert!(input.deserialize);
    assert_eq!(input.versions.len(), 4);
}

#[test]
fn test_deserialize_false_rejects_deserialization_only_features() {
    for (tokens, expected_msg) in [
        (
            quote::quote!(
                deserialize = false,
                versions(_: V0, 1: Self)
            ),
            "cannot use unversioned `_` when `deserialize = false`",
        ),
        (
            quote::quote!(
                deserialize = false,
                versions(#[fallible] 1: V1, 2: Self)
            ),
            "cannot use `#[fallible]` when `deserialize = false`",
        ),
        (
            quote::quote!(
                deserialize = false,
                versions(#[with(migrate_v1)] 1: V1, 2: Self)
            ),
            "cannot use `#[with(...)]` when `deserialize = false`",
        ),
        (
            quote::quote!(
                deserialize = false,
                versions(1: V1 => 2, 2: Self)
            ),
            "cannot use explicit migration target `=> ...` when `deserialize = false`",
        ),
        (
            quote::quote!(
                deserialize = false,
                versions(1: V1 => Self, 2: Self)
            ),
            "cannot use explicit migration target `=> ...` when `deserialize = false`",
        ),
        (
            quote::quote!(
                deserialize = false,
                versions(1: V1 => V2, 2: Self)
            ),
            "cannot use explicit migration target `=> ...` when `deserialize = false`",
        ),
    ] {
        let args: BackwardsCompatArgs = syn::parse2(tokens).unwrap();
        let err = args
            .into_input(
                syn::parse_quote!(TargetModel),
                syn::Visibility::Inherited,
                syn::Generics::default(),
            )
            .unwrap_err();
        assert_eq!(err.to_string(), expected_msg);
    }

    // Allows historical entries and serialization attributes when deserialize = false
    let args: BackwardsCompatArgs = syn::parse2(quote::quote! {
        deserialize = false,
        versions(
            #[serializable] 1: ModelV1,
            #[serialize(with = to_v2)] 2: ModelV2,
            #[current] 3: Self,
        )
    })
    .unwrap();
    let input = args
        .into_input(
            syn::parse_quote!(TargetModel),
            syn::Visibility::Inherited,
            syn::Generics::default(),
        )
        .unwrap();
    assert!(input.serialize);
    assert!(!input.deserialize);
    assert_eq!(input.versions.len(), 3);
    assert_eq!(input.current_version, VersionTag::Int(3));
    assert_eq!(input.serialize_version, VersionTag::Int(2));
}
