use super::shadow::*;
use super::*;

#[test]
fn test_respan_to_call_site_preserves_syntax() {
    let item_struct: syn::ItemStruct = syn::parse_quote! {
        #[serde(rename_all = "camelCase")]
        pub struct Record<'a, T: Clone>
        where
            T: core::fmt::Debug,
        {
            pub label: &'a str,
            pub value: T,
        }
    };
    let respanned = respan_to_call_site(&item_struct);
    assert_eq!(
        quote::quote!(#respanned).to_string(),
        quote::quote!(#item_struct).to_string()
    );

    let item_enum: syn::ItemEnum = syn::parse_quote! {
        pub enum Event<T> {
            Ping,
            Tuple(u32, T),
            Named { text: String },
        }
    };
    let respanned_enum = respan_to_call_site(&item_enum);
    assert_eq!(
        quote::quote!(#respanned_enum).to_string(),
        quote::quote!(#item_enum).to_string()
    );
}

#[test]
fn test_is_serde_derive_variants() {
    let p1: syn::Path = syn::parse_quote!(Serialize);
    let p2: syn::Path = syn::parse_quote!(serde::Serialize);
    let p3: syn::Path = syn::parse_quote!(::serde::Serialize);
    let p4: syn::Path = syn::parse_quote!(Deserialize);
    let p5: syn::Path = syn::parse_quote!(serde::Deserialize);
    let p6: syn::Path = syn::parse_quote!(::serde::Deserialize);
    let p_other: syn::Path = syn::parse_quote!(other::Serialize);
    let p_debug: syn::Path = syn::parse_quote!(Debug);

    assert!(is_serde_derive(&p1, "Serialize"));
    assert!(is_serde_derive(&p2, "Serialize"));
    assert!(is_serde_derive(&p3, "Serialize"));
    assert!(!is_serde_derive(&p1, "Deserialize"));

    assert!(is_serde_derive(&p4, "Deserialize"));
    assert!(is_serde_derive(&p5, "Deserialize"));
    assert!(is_serde_derive(&p6, "Deserialize"));

    assert!(!is_serde_derive(&p_other, "Serialize"));
    assert!(!is_serde_derive(&p_debug, "Serialize"));
}

#[test]
fn test_is_forwarded_attr() {
    let doc_attr: syn::Attribute = syn::parse_quote!(#[doc = "documentation"]);
    let serde_attr: syn::Attribute = syn::parse_quote!(#[serde(rename = "foo")]);
    let allow_attr: syn::Attribute = syn::parse_quote!(#[allow(dead_code)]);
    let warn_attr: syn::Attribute = syn::parse_quote!(#[warn(unused)]);
    let deny_attr: syn::Attribute = syn::parse_quote!(#[deny(clippy::all)]);
    let forbid_attr: syn::Attribute = syn::parse_quote!(#[forbid(unsafe_code)]);
    let cfg_attr: syn::Attribute = syn::parse_quote!(#[cfg(feature = "extra")]);
    let cfg_attr_attr: syn::Attribute = syn::parse_quote!(#[cfg_attr(test, derive(Debug))]);
    let derive_attr: syn::Attribute = syn::parse_quote!(#[derive(Debug)]);
    let custom_attr: syn::Attribute = syn::parse_quote!(#[custom_attribute]);

    assert!(!is_forwarded_attr(&doc_attr));
    assert!(is_forwarded_attr(&serde_attr));
    assert!(is_forwarded_attr(&allow_attr));
    assert!(is_forwarded_attr(&warn_attr));
    assert!(is_forwarded_attr(&deny_attr));
    assert!(is_forwarded_attr(&forbid_attr));
    assert!(is_forwarded_attr(&cfg_attr));
    assert!(is_forwarded_attr(&cfg_attr_attr));

    assert!(!is_forwarded_attr(&derive_attr));
    assert!(!is_forwarded_attr(&custom_attr));
}

#[test]
fn test_clean_derive_attrs() {
    let mut attrs = vec![
        syn::parse_quote!(#[derive(Debug, Serialize, Clone, ::serde::Deserialize)]),
        syn::parse_quote!(#[allow(unused)]),
    ];
    clean_derive_attrs(&mut attrs);
    assert_eq!(attrs.len(), 2);
    let formatted = quote::quote!(#(#attrs)*).to_string();
    assert!(formatted.contains("Debug"));
    assert!(formatted.contains("Clone"));
    assert!(!formatted.contains("Serialize"));
    assert!(!formatted.contains("Deserialize"));

    let mut all_serde_attrs = vec![syn::parse_quote!(#[derive(Serialize, Deserialize, serde::Serialize)])];
    clean_derive_attrs(&mut all_serde_attrs);
    assert_eq!(all_serde_attrs.len(), 0);
}

#[test]
fn test_build_enum_shadow_wire_mixed_variants_generics_and_cfg() {
    let item_enum: syn::ItemEnum = syn::parse_quote! {
        #[serde(rename_all = "snake_case")]
        pub enum MyEnum<'a, T: Clone> {
            Empty,
            Tuple(&'a str, T),
            Struct {
                id: u32,
                payload: T,
            },
            #[cfg(feature = "extra")]
            Extra(u64),
        }
    };

    let built = build_enum_shadow_wire(&item_enum, &[], false);
    assert!(!built.is_unit_struct);

    let shadow_str = built.shadow_tokens.to_string();
    let construct_str = built.construct_borrowed.to_string();
    let helper_ty_str = built.borrowed_ty_in_helper.to_string();

    assert!(shadow_str.contains("enum __MyEnumShadowWire < 'a , T : Clone >"));
    assert!(
        shadow_str.contains("enum __MyEnumBorrowedWire < '__wire , 'a : '__wire , T : Clone + '__wire >")
    );
    assert!(helper_ty_str.contains("__MyEnumBorrowedWire < '__a , 'a , T >"));
    assert!(shadow_str.contains("& '__wire & 'a str"));
    assert!(shadow_str.contains("& '__wire T"));
    assert!(shadow_str.contains("id : & '__wire u32"));
    assert!(shadow_str.contains("# [cfg (feature = \"extra\")] Extra (& '__wire u64 ,)"));
    assert!(!shadow_str.contains("From < MyEnum < 'a , T > > for __MyEnumShadowWire"));
    assert!(shadow_str.contains(
        "# [cfg (feature = \"extra\")] __MyEnumShadowWire :: Extra (__f0) => Self :: Extra (__f0)"
    ));
    assert!(construct_str.contains(
        "# [cfg (feature = \"extra\")] Self :: Extra (__f0) => __MyEnumBorrowedWire :: Extra (__f0)"
    ));

    syn::parse2::<syn::File>(built.shadow_tokens).unwrap();
    syn::parse2::<syn::Expr>(built.construct_borrowed).unwrap();
}

#[test]
fn test_build_enum_shadow_wire_all_unit_no_wire_lifetime() {
    let item_enum: syn::ItemEnum = syn::parse_quote! {
        pub enum Status {
            Active,
            Inactive,
        }
    };

    let built = build_enum_shadow_wire(&item_enum, &[], false);
    assert!(!built.is_unit_struct);

    let shadow_str = built.shadow_tokens.to_string();
    let helper_ty_str = built.borrowed_ty_in_helper.to_string();

    assert!(!shadow_str.contains("'__wire"));
    assert!(!helper_ty_str.contains("'__a"));
    assert_eq!(helper_ty_str, "__StatusBorrowedWire");

    syn::parse2::<syn::File>(built.shadow_tokens).unwrap();
    syn::parse2::<syn::Expr>(built.construct_borrowed).unwrap();
}

#[test]
fn test_build_enum_shadow_wire_empty_enum() {
    let item_enum: syn::ItemEnum = syn::parse_quote! {
        pub enum Never {}
    };

    let built = build_enum_shadow_wire(&item_enum, &[], false);
    assert!(!built.is_unit_struct);

    let shadow_str = built.shadow_tokens.to_string();
    let construct_str = built.construct_borrowed.to_string();

    assert!(!shadow_str.contains("'__wire"));
    assert_eq!(construct_str, "match * self { }");
    assert!(shadow_str.contains("match val { }"));

    syn::parse2::<syn::File>(built.shadow_tokens).unwrap();
    syn::parse2::<syn::Expr>(built.construct_borrowed).unwrap();
}

#[test]
fn test_build_borrowed_generics_with_const_param() {
    let item: syn::ItemStruct = syn::parse_quote! {
        pub struct Buf<const N: usize>([u8; N]);
    };
    let borrowed_ident = quote::format_ident!("__BufBorrowedWire");
    let (borrowed_generics, ty_in_helper) = build_borrowed_generics(&item.generics, &borrowed_ident);

    assert_eq!(
        quote::quote!(#borrowed_generics).to_string(),
        "< '__wire , const N : usize >"
    );
    assert_eq!(ty_in_helper.to_string(), "__BufBorrowedWire < '__a , N >");
}

#[test]
fn test_should_mark_transparent_edge_cases() {
    let plain: syn::ItemStruct = syn::parse_quote! { struct Plain(u32); };
    assert!(should_mark_transparent(&plain));

    let multi: syn::ItemStruct = syn::parse_quote! { struct Multi(u32, u32); };
    assert!(!should_mark_transparent(&multi));

    let named: syn::ItemStruct = syn::parse_quote! { struct Named { a: u32 } };
    assert!(!should_mark_transparent(&named));

    let container_serde: syn::ItemStruct = syn::parse_quote! {
        #[serde(rename_all = "camelCase")]
        struct ContainerSerde(u32);
    };
    assert!(!should_mark_transparent(&container_serde));

    let field_skip: syn::ItemStruct = syn::parse_quote! {
        struct FieldSkip(#[serde(skip)] u32);
    };
    assert!(!should_mark_transparent(&field_skip));

    let field_skip_ser: syn::ItemStruct = syn::parse_quote! {
        struct FieldSkipSer(#[serde(skip_serializing)] u32);
    };
    assert!(!should_mark_transparent(&field_skip_ser));

    let field_skip_de: syn::ItemStruct = syn::parse_quote! {
        struct FieldSkipDe(#[serde(skip_deserializing)] u32);
    };
    assert!(!should_mark_transparent(&field_skip_de));

    let field_non_skip: syn::ItemStruct = syn::parse_quote! {
        struct FieldDefault(#[serde(default)] u32);
    };
    assert!(should_mark_transparent(&field_non_skip));
}

#[test]
fn test_backwards_compat_impl_error_paths() {
    // Non-struct/non-enum item
    let non_item = backwards_compat_impl(
        quote::quote!(versions(1: Self)),
        quote::quote!(
            fn foo() {}
        ),
    )
    .to_string();
    assert!(non_item.contains("compile_error !"));
    assert!(non_item.contains("can only be applied to structs or enums"));

    // Invalid attr syntax
    let bad_attr = backwards_compat_impl(
        quote::quote!(unknown_key = 123),
        quote::quote!(
            struct S {
                a: u32,
            }
        ),
    )
    .to_string();
    assert!(bad_attr.contains("compile_error !"));

    // Invalid into_input (duplicate version tags)
    let dup_tags = backwards_compat_impl(
        quote::quote!(versions(1: V1, 1: V2)),
        quote::quote!(
            struct S {
                a: u32,
            }
        ),
    )
    .to_string();
    assert!(dup_tags.contains("compile_error !"));

    // Invalid resolve_dag (cycle)
    let cycle = backwards_compat_impl(
        quote::quote!(versions(1: V1 => 2, 2: V2 => 1, 3: Self)),
        quote::quote!(
            struct S {
                a: u32,
            }
        ),
    )
    .to_string();
    assert!(cycle.contains("compile_error !"));
    assert!(cycle.contains("cycle detected"));
}

#[test]
fn test_backwards_compat_via_impl_paths() {
    // Valid struct with conflicting derives stripped
    let struct_ts = backwards_compat_impl(
        quote::quote!(via = Proxy, try_from),
        quote::quote! {
            #[derive(Debug, Clone, Serialize, Deserialize)]
            pub struct Domain { pub a: u32 }
        },
    );
    let struct_str = struct_ts.to_string();
    assert!(!struct_str.contains("compile_error !"));
    assert!(struct_str.contains("impl :: backwards_compat :: BackwardsCompat for Domain"));
    assert!(!struct_str.contains("derive (Debug , Clone , Serialize"));
    syn::parse2::<syn::File>(struct_ts).unwrap();

    // Valid enum with `via(...)` syntax and arbitrary option order
    let enum_ts = backwards_compat_impl(
        quote::quote!(fallible, via(ProxyEnum)),
        quote::quote! {
            #[derive(Debug, serde::Serialize, serde::Deserialize)]
            pub enum DomainEnum { A, B(u32) }
        },
    );
    let enum_str = enum_ts.to_string();
    assert!(!enum_str.contains("compile_error !"));
    assert!(enum_str.contains("impl :: backwards_compat :: BackwardsCompat for DomainEnum"));
    syn::parse2::<syn::File>(enum_ts).unwrap();

    // Non-struct/non-enum item
    let fn_err = backwards_compat_impl(
        quote::quote!(via = Proxy),
        quote::quote!(
            fn foo() {}
        ),
    )
    .to_string();
    assert!(fn_err.contains("compile_error !"));
    assert!(fn_err.contains("can only be applied to structs or enums"));

    // Invalid attr syntax (`via = Self`, bare `via`, mixing with `tag`)
    let self_err = backwards_compat_impl(
        quote::quote!(via = Self),
        quote::quote!(
            struct Domain;
        ),
    )
    .to_string();
    assert!(self_err.contains("compile_error !"));
    assert!(self_err.contains("cannot be `Self`"));

    let bare_via_err = backwards_compat_impl(
        quote::quote!(via),
        quote::quote!(
            struct Domain;
        ),
    )
    .to_string();
    assert!(bare_via_err.contains("compile_error !"));
    assert!(bare_via_err.contains("expected `via = <Type>` or `via(<Type>)`"));

    let mixed_err = backwards_compat_impl(
        quote::quote!(tag = "v", via = Proxy),
        quote::quote!(
            struct Domain;
        ),
    )
    .to_string();
    assert!(mixed_err.contains("compile_error !"));
    assert!(mixed_err.contains("`tag` cannot be combined with `via`"));

    // Field-level #[backwards_compat(...)] is rejected on struct and enum with `via`
    let struct_field_err = backwards_compat_impl(
        quote::quote!(via = Proxy),
        quote::quote! {
            pub struct Domain {
                #[backwards_compat(versions(1: u32, 2: Self))]
                pub a: u64,
            }
        },
    )
    .to_string();
    assert!(struct_field_err.contains("compile_error !"));
    assert!(struct_field_err.contains("field-level `#[backwards_compat(...)]` cannot be used"));

    let enum_field_err = backwards_compat_impl(
        quote::quote!(via = Proxy),
        quote::quote! {
            pub enum DomainEnum {
                A(#[backwards_compat(versions(1: u32, 2: Self))] u64),
            }
        },
    )
    .to_string();
    assert!(enum_field_err.contains("compile_error !"));
    assert!(enum_field_err.contains("field-level `#[backwards_compat(...)]` cannot be used"));

    // Nested `#[with(via)]` inside `versions(...)` must NOT trigger `via` mode
    let nested_with_via = backwards_compat_impl(
        quote::quote!(tag = "v", versions(#[with(via)] 1: V1, 2: Self)),
        quote::quote! {
            pub struct Domain { pub a: u32 }
        },
    );
    let nested_str = nested_with_via.to_string();
    assert!(!nested_str.contains("compile_error !"));
    assert!(nested_str.contains("impl :: backwards_compat :: BackwardsCompat for Domain"));
    syn::parse2::<syn::File>(nested_with_via).unwrap();

    // Invalid item syntax
    let bad_item = backwards_compat_impl(quote::quote!(via = Proxy), quote::quote!(struct !!!)).to_string();
    assert!(bad_item.contains("compile_error !"));
}

#[test]
fn test_field_level_mode1_bare_with_versioned_fields_struct_and_enum() {
    let struct_ts = backwards_compat_impl(
        quote::quote!(),
        quote::quote! {
            #[derive(Debug, Serialize, Deserialize)]
            #[serde(rename_all = "camelCase")]
            pub struct Config {
                #[serde(rename = "p")]
                #[backwards_compat(versions(1: u32, 2: Self))]
                pub port: u16,
                pub host: String,
            }
        },
    );
    let s = struct_ts.to_string();
    assert!(!s.contains("compile_error !"));
    assert!(!s.contains("impl :: backwards_compat :: BackwardsCompat for Config"));
    assert!(s.contains("derive (Debug , Serialize , Deserialize)"));
    assert!(s.contains("rename_all = \"camelCase\""));
    assert!(s.contains("rename = \"p\""));
    assert!(s.contains("with = \"__BcField_Config_port\""));
    assert!(!s.contains("backwards_compat (versions"));
    assert!(s.contains("struct __BcField_Config_port ;"));
    syn::parse2::<syn::File>(struct_ts).unwrap();

    let enum_ts = backwards_compat_impl(
        quote::quote!(),
        quote::quote! {
            #[derive(Serialize, Deserialize)]
            pub enum Msg {
                Ping,
                Data(#[backwards_compat(versions(1: u32, 2: Self))] u64),
                Named {
                    #[backwards_compat(versions(1: Self))]
                    r#type: String,
                },
            }
        },
    );
    let e = enum_ts.to_string();
    assert!(!e.contains("compile_error !"));
    assert!(!e.contains("impl :: backwards_compat :: BackwardsCompat for Msg"));
    assert!(e.contains("with = \"__BcField_Msg_Data_0\""));
    assert!(e.contains("with = \"__BcField_Msg_Named_type\""));
    assert!(e.contains("struct __BcField_Msg_Data_0 ;"));
    assert!(e.contains("struct __BcField_Msg_Named_type ;"));
    syn::parse2::<syn::File>(enum_ts).unwrap();
}

#[test]
fn test_field_level_mode2_bare_without_versioned_fields() {
    let ts = backwards_compat_impl(
        quote::quote!(),
        quote::quote! {
            #[derive(Debug, Serialize, Deserialize)]
            pub struct Plain {
                pub a: u32,
            }
        },
    );
    let s = ts.to_string();
    assert!(!s.contains("compile_error !"));
    assert!(s.contains("impl :: backwards_compat :: BackwardsCompat for Plain"));
    assert!(!s.contains("__BcField_"));
    syn::parse2::<syn::File>(ts).unwrap();
}

#[test]
fn test_field_level_mode3_combined_outer_and_field_versioning() {
    let struct_ts = backwards_compat_impl(
        quote::quote!(tag = "v", versions(1: Self)),
        quote::quote! {
            #[derive(Debug, Serialize, Deserialize)]
            pub struct Combined {
                #[backwards_compat(versions(1: u32, 2: Self))]
                pub port: u16,
                pub host: String,
            }
        },
    );
    let s = struct_ts.to_string();
    assert!(!s.contains("compile_error !"));
    assert!(s.contains("impl :: backwards_compat :: BackwardsCompat for Combined"));
    assert!(s.contains("deserialize_with = \"__BcField_Combined_port::deserialize\""));
    assert!(s.contains("serialize_with = \"__BcField_Combined_port::serialize_ref\""));
    assert!(s.contains("struct __BcField_Combined_port ;"));
    assert!(!s.contains("backwards_compat (versions"));
    syn::parse2::<syn::File>(struct_ts).unwrap();

    let enum_ts = backwards_compat_impl(
        quote::quote!(tag = "v", content = "d", versions(1: Self)),
        quote::quote! {
            pub enum CombinedEnum {
                A(#[backwards_compat(versions(1: u32, 2: Self))] u64),
                B {
                    #[backwards_compat(versions(1: Self))]
                    x: String,
                },
            }
        },
    );
    let e = enum_ts.to_string();
    assert!(!e.contains("compile_error !"));
    assert!(e.contains("impl :: backwards_compat :: BackwardsCompat for CombinedEnum"));
    assert!(e.contains("deserialize_with = \"__BcField_CombinedEnum_A_0::deserialize\""));
    assert!(e.contains("serialize_with = \"__BcField_CombinedEnum_A_0::serialize_ref\""));
    assert!(e.contains("deserialize_with = \"__BcField_CombinedEnum_B_x::deserialize\""));
    assert!(e.contains("serialize_with = \"__BcField_CombinedEnum_B_x::serialize_ref\""));
    syn::parse2::<syn::File>(enum_ts).unwrap();
}

#[test]
fn test_field_level_error_propagation() {
    let dup = backwards_compat_impl(
        quote::quote!(),
        quote::quote! {
            pub struct S {
                #[backwards_compat]
                #[backwards_compat]
                pub a: u32,
            }
        },
    )
    .to_string();
    assert!(dup.contains("compile_error !"));
    assert!(dup.contains("duplicate"));

    let conflict = backwards_compat_impl(
        quote::quote!(),
        quote::quote! {
            pub struct S {
                #[serde(with = "other")]
                #[backwards_compat]
                pub a: u32,
            }
        },
    )
    .to_string();
    assert!(conflict.contains("compile_error !"));
    assert!(conflict.contains("conflicts with"));

    let unref_gen = backwards_compat_impl(
        quote::quote!(),
        quote::quote! {
            pub struct S<T, U> {
                #[backwards_compat(versions(1: Old<U>, 2: Self))]
                pub a: T,
                pub b: U,
            }
        },
    )
    .to_string();
    assert!(unref_gen.contains("compile_error !"));
    assert!(unref_gen.contains("outer generic type parameter `U`"));
}

#[test]
fn test_tag_field_collision_validation() {
    // Direct field name collision on internally tagged struct
    let direct_err = backwards_compat_impl(
        quote::quote!(versions(1: Self)),
        quote::quote! {
            pub struct Bad {
                pub version: u64,
                pub name: String,
            }
        },
    )
    .to_string();
    assert!(direct_err.contains("compile_error !"));
    assert!(direct_err.contains("collides with the `#[backwards_compat]` version tag field `version`"));

    // #[serde(rename = "...")] collision even when struct has rename_all
    let rename_err = backwards_compat_impl(
        quote::quote!(tag = "v", versions(1: Self)),
        quote::quote! {
            #[serde(rename_all = "camelCase")]
            pub struct BadRename {
                #[serde(rename = "v")]
                pub ver_num: u64,
            }
        },
    )
    .to_string();
    assert!(rename_err.contains("compile_error !"));
    assert!(rename_err.contains("collides with the `#[backwards_compat]` version tag field `v`"));

    // Raw ident r#type collides with tag = "type"
    let raw_err = backwards_compat_impl(
        quote::quote!(tag = "type", versions(1: Self)),
        quote::quote! {
            pub struct BadRaw {
                pub r#type: String,
            }
        },
    )
    .to_string();
    assert!(raw_err.contains("compile_error !"));

    // Safe cases: #[serde(skip)], #[serde(flatten)], #[serde(rename = "other")], rename_all without explicit rename, adjacent content = "data"
    for ok_case in [
        backwards_compat_impl(
            quote::quote!(versions(1: Self)),
            quote::quote! {
                pub struct Skipped {
                    #[serde(skip)]
                    pub version: u64,
                }
            },
        ),
        backwards_compat_impl(
            quote::quote!(versions(1: Self)),
            quote::quote! {
                pub struct Flattened {
                    #[serde(flatten)]
                    pub version: Extra,
                }
            },
        ),
        backwards_compat_impl(
            quote::quote!(versions(1: Self)),
            quote::quote! {
                pub struct RenamedAway {
                    #[serde(rename = "schema_ver")]
                    pub version: u64,
                }
            },
        ),
        backwards_compat_impl(
            quote::quote!(tag = "myField", versions(1: Self)),
            quote::quote! {
                #[serde(rename_all = "camelCase")]
                pub struct RenameAll {
                    pub myField: u64,
                }
            },
        ),
        backwards_compat_impl(
            quote::quote!(tag = "version", content = "data", versions(1: Self)),
            quote::quote! {
                pub struct Adjacent {
                    pub version: u64,
                }
            },
        ),
        backwards_compat_impl(
            quote::quote!(versions(1: V1, 2: V2)),
            quote::quote! {
                pub struct NonShadow {
                    pub version: u64,
                }
            },
        ),
    ] {
        assert!(!ok_case.to_string().contains("compile_error !"));
    }
}

#[cfg(feature = "schemars")]
#[test]
fn test_schemars_integration_shadow_mode1_and_via() {
    // has_json_schema_derive & clean_derive_attrs
    let mut attrs: Vec<syn::Attribute> = vec![
        syn::parse_quote!(#[derive(Debug, JsonSchema, ::schemars::JsonSchema, Serialize)]),
        syn::parse_quote!(#[schemars(title = "T")]),
    ];
    assert!(has_json_schema_derive(&attrs));
    clean_derive_attrs(&mut attrs);
    let cleaned_str = quote::quote!(#(#attrs)*).to_string();
    assert!(cleaned_str.contains("Debug"));
    assert!(!cleaned_str.contains("JsonSchema"));
    assert!(!cleaned_str.contains("Serialize"));

    // Type-level #[backwards_compat] with #[derive(JsonSchema)]
    let main_ts = backwards_compat_impl(
        quote::quote!(versions(1: V1, 2: Self)),
        quote::quote! {
            #[derive(Serialize, Deserialize, JsonSchema)]
            #[schemars(title = "MyModel")]
            pub struct MyModel {
                #[schemars(range(min = 1))]
                pub count: u32,
            }
        },
    );
    let main_str = main_ts.to_string();
    assert!(!main_str.contains("compile_error !"));
    assert!(main_str.contains(
        "derive (:: serde :: Deserialize , :: backwards_compat :: __private :: schemars :: JsonSchema)"
    ));
    assert!(main_str.contains("impl :: backwards_compat :: __private :: schemars :: JsonSchema for MyModel"));
    syn::parse2::<syn::File>(main_ts).unwrap();

    // Mode 1 bare #[backwards_compat] with field-level + #[derive(JsonSchema)]
    let mode1_ts = backwards_compat_impl(
        quote::quote!(),
        quote::quote! {
            #[derive(Serialize, Deserialize, JsonSchema)]
            pub struct Mode1Model {
                #[backwards_compat(versions(1: u32, 2: Self))]
                pub port: u16,
            }
        },
    );
    let mode1_str = mode1_ts.to_string();
    assert!(!mode1_str.contains("compile_error !"));
    assert!(mode1_str.contains("with = \"__BcField_Mode1Model_port_Schema\""));
    assert!(mode1_str.contains("struct __BcField_Mode1Model_port_Schema"));
    syn::parse2::<syn::File>(mode1_ts).unwrap();

    // Via mode with #[derive(JsonSchema)]
    let via_ts = backwards_compat_impl(
        quote::quote!(via = Proxy),
        quote::quote! {
            #[derive(Serialize, Deserialize, schemars::JsonSchema)]
            #[schemars(deny_unknown_fields)]
            pub struct ViaModel {
                pub a: u32,
            }
        },
    );
    let via_str = via_ts.to_string();
    assert!(!via_str.contains("compile_error !"));
    assert!(via_str.contains("impl :: backwards_compat :: __private :: schemars :: JsonSchema for ViaModel"));
    syn::parse2::<syn::File>(via_ts).unwrap();
}
