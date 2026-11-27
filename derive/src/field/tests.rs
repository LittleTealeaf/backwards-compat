#![allow(clippy::indexing_slicing, reason = "index accesses in unit test assertions")]

use super::*;

#[test]
fn test_extract_named_and_tuple_struct_fields_default_content() {
    let named: syn::ItemStruct = syn::parse_quote! {
        pub struct Config {
            #[backwards_compat(versions(1: u32, 2: Self))]
            pub port: u16,
            #[backwards_compat]
            pub name: String,
            #[backwards_compat(content = "payload", versions(1: Self))]
            pub custom: bool,
            pub plain: i32,
        }
    };
    let specs = extract_struct_field_compat(&named).unwrap();
    assert_eq!(specs.len(), 3);
    assert_eq!(specs[0].helper_ident, "__BcField_Config_port");
    assert_eq!(specs[0].field_idx, 0);
    assert_eq!(specs[0].args.content_field.as_deref(), Some("data"));

    assert_eq!(specs[1].helper_ident, "__BcField_Config_name");
    assert_eq!(specs[1].field_idx, 1);
    assert_eq!(specs[1].args.content_field.as_deref(), Some("data"));
    assert!(specs[1].args.versions.is_empty());

    assert_eq!(specs[2].helper_ident, "__BcField_Config_custom");
    assert_eq!(specs[2].field_idx, 2);
    assert_eq!(specs[2].args.content_field.as_deref(), Some("payload"));

    let tuple: syn::ItemStruct = syn::parse_quote! {
        pub struct Pair(
            u32,
            #[backwards_compat(versions(1: String, 2: Self))]
            i64,
        );
    };
    let t_specs = extract_struct_field_compat(&tuple).unwrap();
    assert_eq!(t_specs.len(), 1);
    assert_eq!(t_specs[0].helper_ident, "__BcField_Pair_1");
    assert_eq!(t_specs[0].field_idx, 1);
    assert_eq!(t_specs[0].args.content_field.as_deref(), Some("data"));
}

#[test]
fn test_extract_enum_fields_and_cfg_preservation() {
    let item_enum: syn::ItemEnum = syn::parse_quote! {
        pub enum Event {
            Ping,
            #[cfg(feature = "extra")]
            Data(
                #[cfg(unix)]
                #[backwards_compat(versions(1: u32, 2: Self))]
                u64,
            ),
            Named {
                #[backwards_compat(versions(1: Self))]
                r#type: String,
            },
        }
    };
    let specs = extract_enum_field_compat(&item_enum).unwrap();
    assert_eq!(specs.len(), 2);

    assert_eq!(specs[0].variant_idx, Some(1));
    assert_eq!(specs[0].field_idx, 0);
    assert_eq!(specs[0].helper_ident, "__BcField_Event_Data_0");
    assert_eq!(specs[0].cfgs.len(), 2);

    assert_eq!(specs[1].variant_idx, Some(2));
    assert_eq!(specs[1].field_idx, 0);
    assert_eq!(specs[1].helper_ident, "__BcField_Event_Named_type");
}

#[test]
fn test_raw_ident_stripping_on_struct_and_field() {
    let item: syn::ItemStruct = syn::parse_quote! {
        pub struct r#match {
            #[backwards_compat(versions(1: Self))]
            pub r#type: u32,
        }
    };
    let specs = extract_struct_field_compat(&item).unwrap();
    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].helper_ident, "__BcField_match_type");
}

#[test]
fn test_duplicate_backwards_compat_attr_error() {
    let item: syn::ItemStruct = syn::parse_quote! {
        pub struct S {
            #[backwards_compat(versions(1: Self))]
            #[backwards_compat(versions(1: Self))]
            pub a: u32,
        }
    };
    let err = extract_struct_field_compat(&item).unwrap_err();
    assert!(err.to_string().contains("duplicate `#[backwards_compat]`"));
}

#[test]
fn test_serde_with_conflicts() {
    for serde_attr in [
        quote!(#[serde(with = "my_mod")]),
        quote!(#[serde(serialize_with = "my_ser")]),
        quote!(#[serde(deserialize_with = "my_de")]),
        quote!(#[serde(rename = "x", deserialize_with = "my_de")]),
        quote!(#[serde(bound(serialize = "T: Serialize"), with = "my_mod")]),
    ] {
        let item: syn::ItemStruct = syn::parse_quote! {
            pub struct S {
                #serde_attr
                #[backwards_compat(versions(1: Self))]
                pub a: u32,
            }
        };
        let err = extract_struct_field_compat(&item).unwrap_err();
        assert!(err.to_string().contains("conflicts with"));
    }

    // Non-conflicting serde attributes must succeed
    let ok_item: syn::ItemStruct = syn::parse_quote! {
        pub struct S {
            #[serde(rename = "x", default, skip_serializing_if = "Option::is_none")]
            #[backwards_compat(versions(1: Self))]
            pub a: Option<u32>,
        }
    };
    assert_eq!(extract_struct_field_compat(&ok_item).unwrap().len(), 1);
}

#[test]
fn test_generic_filtering_lifetimes_types_consts_and_where_clause() {
    let item: syn::ItemStruct = syn::parse_quote! {
        pub struct Envelope<'a, 'b: 'a, T: Clone + 'b = u8, U = String, const N: usize = 4, const M: usize = 8>
        where
            T: Default,
            U: core::fmt::Debug,
            'a: 'static,
        {
            #[backwards_compat(versions(1: OldBuf<'a, T, N>, 2: Self))]
            pub buf: ([T; N], &'a str),
            pub other: &'b U,
            pub unused_arr: [u8; M],
        }
    };
    let specs = extract_struct_field_compat(&item).unwrap();
    assert_eq!(specs.len(), 1);

    let fg = &specs[0].filtered_generics;
    let fg_str = quote!(#fg).to_string();
    assert!(fg_str.contains("'a"));
    assert!(!fg_str.contains("'b"));
    assert!(fg_str.contains("T : Clone"));
    assert!(!fg_str.contains('U'));
    assert!(fg_str.contains("const N : usize"));
    assert!(!fg_str.contains("const M"));
    // Defaults must be stripped from function generics
    assert!(!fg_str.contains("= u8"));
    assert!(!fg_str.contains("= 4"));

    let wc_str = fg
        .where_clause
        .as_ref()
        .map_or_else(String::new, |w| quote!(#w).to_string());
    assert!(wc_str.contains("T : Default"));
    assert!(wc_str.contains("'a : 'static"));
    assert!(!wc_str.contains("U :"));
}

#[test]
fn test_unreferenced_outer_generic_in_historical_version_errors() {
    // Unreferenced type parameter U
    let item_ty: syn::ItemStruct = syn::parse_quote! {
        pub struct S<T, U> {
            #[backwards_compat(versions(1: Old<T, U>, 2: Self))]
            pub a: T,
            pub b: U,
        }
    };
    let err_ty = extract_struct_field_compat(&item_ty).unwrap_err();
    assert!(err_ty.to_string().contains("outer generic type parameter `U`"));

    // Unreferenced lifetime 'b
    let item_lt: syn::ItemStruct = syn::parse_quote! {
        pub struct S<'a, 'b> {
            #[backwards_compat(versions(1: Old<'b>, 2: Self))]
            pub a: &'a str,
            pub b: &'b str,
        }
    };
    let err_lt = extract_struct_field_compat(&item_lt).unwrap_err();
    assert!(err_lt.to_string().contains("outer lifetime `'b`"));

    // Unreferenced const parameter M
    let item_const: syn::ItemStruct = syn::parse_quote! {
        pub struct S<const N: usize, const M: usize> {
            #[backwards_compat(versions(1: [u8; M], 2: Self))]
            pub a: [u8; N],
            pub b: [u8; M],
        }
    };
    let err_const = extract_struct_field_compat(&item_const).unwrap_err();
    assert!(err_const.to_string().contains("outer const parameter `M`"));
}

#[test]
fn test_generate_field_helper_valid_syntax() {
    let item: syn::ItemStruct = syn::parse_quote! {
        pub struct Outer<'a, T, U, const N: usize>
        where
            T: Clone,
            U: Clone,
        {
            #[backwards_compat(versions(1: OldField<'a, T, N>, 2: Self))]
            pub field: NewField<'a, T, N>,
            pub other: U,
        }
    };
    let specs = extract_struct_field_compat(&item).unwrap();
    let tokens = generate_field_helper(&specs[0], false, true, true).unwrap();
    let rendered = tokens.to_string();

    assert!(rendered.contains("struct __BcField_Outer_field ;"));
    assert!(rendered.contains("impl __BcField_Outer_field"));
    assert!(rendered.contains("fn serialize < 'a , T , const N : usize , __S >"));
    assert!(rendered.contains("fn serialize_ref < 'a , T , const N : usize , __S >"));
    assert!(rendered.contains("fn deserialize < '__de : 'a , 'a , T , const N : usize , __D >"));
    assert!(!rendered.contains('U'));
    syn::parse2::<syn::File>(tokens).unwrap();
}

#[cfg(feature = "schemars")]
#[test]
fn test_generate_field_helper_with_schemars() {
    let item: syn::ItemStruct = syn::parse_quote! {
        pub struct Outer<T> {
            #[backwards_compat(versions(1: OldField<T>, 2: Self))]
            pub field: NewField<T>,
        }
    };
    let specs = extract_struct_field_compat(&item).unwrap();
    let tokens = generate_field_helper(&specs[0], true, true, true).unwrap();
    let rendered = tokens.to_string();

    assert!(rendered.contains("struct __BcField_Outer_field_Schema < T >"));
    assert!(rendered.contains("impl < T > :: backwards_compat :: __private :: schemars :: JsonSchema for __BcField_Outer_field_Schema < T >"));
    assert!(rendered.contains("schema_adjacently_tagged"));
    syn::parse2::<syn::File>(tokens).unwrap();
}

#[test]
fn test_field_level_external_does_not_default_content() {
    let item: syn::ItemStruct = syn::parse_quote! {
        pub struct Config {
            #[backwards_compat(external, versions(1: u32, 2: Self))]
            pub port: u64,
        }
    };
    let specs = extract_struct_field_compat(&item).unwrap();
    assert_eq!(specs.len(), 1);
    assert!(specs[0].args.external);
    assert!(specs[0].args.content_field.is_none());

    let tokens = generate_field_helper(&specs[0], false, true, true).unwrap();
    let rendered = tokens.to_string();
    assert!(rendered.contains("serialize_newtype_variant"));
    assert!(rendered.contains("deserialize_enum"));
    syn::parse2::<syn::File>(tokens).unwrap();
}

#[test]
fn test_field_level_rejects_serialize_and_deserialize_options() {
    let expected = "`serialize` and `deserialize` options are only supported on struct/enum-level `#[backwards_compat(...)]`, not on field-level attributes";

    for attr_args in [
        quote!(serialize = false, versions(1: u32, 2: Self)),
        quote!(deserialize = false, versions(1: u32, 2: Self)),
        quote!(serialize = true, versions(1: u32, 2: Self)),
        quote!(deserialize = true, versions(1: u32, 2: Self)),
        quote!(serialize(false), versions(1: u32, 2: Self)),
        quote!(deserialize(true), versions(1: u32, 2: Self)),
    ] {
        let item: syn::ItemStruct = syn::parse_quote! {
            pub struct Config {
                #[backwards_compat(#attr_args)]
                pub port: u16,
            }
        };
        let err = extract_struct_field_compat(&item).unwrap_err();
        assert_eq!(err.to_string(), expected);
    }

    let item_enum: syn::ItemEnum = syn::parse_quote! {
        pub enum Event {
            Data(#[backwards_compat(serialize = false, versions(1: u32, 2: Self))] u64),
        }
    };
    let err_enum = extract_enum_field_compat(&item_enum).unwrap_err();
    assert_eq!(err_enum.to_string(), expected);
}

#[test]
fn test_generate_field_helper_conditional_serialize_and_deserialize() {
    let item: syn::ItemStruct = syn::parse_quote! {
        pub struct Outer<T> {
            #[backwards_compat(versions(1: OldField<T>, 2: Self))]
            pub field: NewField<T>,
        }
    };
    let specs = extract_struct_field_compat(&item).unwrap();

    // enable_serialize = false, enable_deserialize = true
    let de_only = generate_field_helper(&specs[0], false, false, true).unwrap();
    let de_rendered = de_only.to_string();
    assert!(!de_rendered.contains("fn serialize "));
    assert!(!de_rendered.contains("fn serialize <"));
    assert!(!de_rendered.contains("fn serialize_ref"));
    assert!(de_rendered.contains("fn deserialize <"));
    syn::parse2::<syn::File>(de_only).unwrap();

    // enable_serialize = true, enable_deserialize = false
    let ser_only = generate_field_helper(&specs[0], false, true, false).unwrap();
    let ser_rendered = ser_only.to_string();
    assert!(!ser_rendered.contains("fn deserialize"));
    assert!(ser_rendered.contains("fn serialize <"));
    assert!(ser_rendered.contains("fn serialize_ref <"));
    syn::parse2::<syn::File>(ser_only).unwrap();
}

#[cfg(feature = "schemars")]
#[test]
fn test_generate_field_helper_with_schemars_and_disabled_serde() {
    let item: syn::ItemStruct = syn::parse_quote! {
        pub struct Outer<T> {
            #[backwards_compat(versions(1: OldField<T>, 2: Self))]
            pub field: NewField<T>,
        }
    };
    let specs = extract_struct_field_compat(&item).unwrap();

    // derive_json_schema = true, enable_serialize = false, enable_deserialize = true
    let de_schema = generate_field_helper(&specs[0], true, false, true).unwrap();
    let de_rendered = de_schema.to_string();
    assert!(de_rendered.contains("struct __BcField_Outer_field_Schema < T >"));
    assert!(de_rendered.contains(
        "impl < T > :: backwards_compat :: __private :: schemars :: JsonSchema for __BcField_Outer_field_Schema < T >"
    ));
    assert!(!de_rendered.contains("fn serialize <"));
    assert!(!de_rendered.contains("fn serialize_ref"));
    assert!(de_rendered.contains("fn deserialize <"));
    syn::parse2::<syn::File>(de_schema).unwrap();

    // derive_json_schema = true, enable_serialize = true, enable_deserialize = false
    let ser_schema = generate_field_helper(&specs[0], true, true, false).unwrap();
    let ser_rendered = ser_schema.to_string();
    assert!(ser_rendered.contains("struct __BcField_Outer_field_Schema < T >"));
    assert!(ser_rendered.contains(
        "impl < T > :: backwards_compat :: __private :: schemars :: JsonSchema for __BcField_Outer_field_Schema < T >"
    ));
    assert!(ser_rendered.contains("fn serialize <"));
    assert!(ser_rendered.contains("fn serialize_ref <"));
    assert!(!ser_rendered.contains("fn deserialize"));
    syn::parse2::<syn::File>(ser_schema).unwrap();
}

#[test]
fn test_generate_field_helper_legacy_deprecation_warnings_and_allow_forwarding() {
    let legacy_item: syn::ItemStruct = syn::parse_quote! {
        pub struct Outer {
            #[allow(deprecated)]
            #[backwards_compat(versions(#[fallible] 1: OldField, 2: Self))]
            pub field: NewField,
        }
    };
    let legacy_specs = extract_struct_field_compat(&legacy_item).unwrap();
    let legacy_tokens = generate_field_helper(&legacy_specs[0], false, true, true).unwrap();
    let legacy_rendered = legacy_tokens.to_string();
    assert!(legacy_rendered.contains(
        "# [deprecated (note = \"deprecated: use `#[migrate(try_from)]` instead of `#[fallible]`\")]"
    ));
    assert!(legacy_rendered.contains("const fn __bc_deprecated_legacy_attr_0 ()"));
    assert!(
        legacy_rendered.contains("# [allow (deprecated)] const _ : () = __bc_deprecated_legacy_attr_0 () ;")
    );
    syn::parse2::<syn::File>(legacy_tokens).unwrap();

    let canonical_item: syn::ItemStruct = syn::parse_quote! {
        pub struct Outer {
            #[backwards_compat(versions(#[migrate(try_from)] 1: OldField, 2: Self))]
            pub field: NewField,
        }
    };
    let canonical_specs = extract_struct_field_compat(&canonical_item).unwrap();
    let canonical_tokens = generate_field_helper(&canonical_specs[0], false, true, true).unwrap();
    let canonical_rendered = canonical_tokens.to_string();
    assert!(!canonical_rendered.contains("__bc_deprecated_legacy_attr_"));
    assert!(!canonical_rendered.contains("deprecated"));
    syn::parse2::<syn::File>(canonical_tokens).unwrap();
}
