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
    clean_derive_attrs(&mut attrs, true, true);
    assert_eq!(attrs.len(), 2);
    let formatted = quote::quote!(#(#attrs)*).to_string();
    assert!(formatted.contains("Debug"));
    assert!(formatted.contains("Clone"));
    assert!(!formatted.contains("Serialize"));
    assert!(!formatted.contains("Deserialize"));

    let mut all_serde_attrs = vec![syn::parse_quote!(#[derive(Serialize, Deserialize, serde::Serialize)])];
    clean_derive_attrs(&mut all_serde_attrs, true, true);
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

    let built = build_enum_shadow_wire(
        &item_enum,
        &[],
        ShadowWireOptions {
            serialize: true,
            deserialize: true,
            derive_json_schema: false,
        },
    );
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

    let built = build_enum_shadow_wire(
        &item_enum,
        &[],
        ShadowWireOptions {
            serialize: true,
            deserialize: true,
            derive_json_schema: false,
        },
    );
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

    let built = build_enum_shadow_wire(
        &item_enum,
        &[],
        ShadowWireOptions {
            serialize: true,
            deserialize: true,
            derive_json_schema: false,
        },
    );
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
    clean_derive_attrs(&mut attrs, true, true);
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

#[test]
fn test_build_struct_shadow_wire_conditional_emission() {
    let named: syn::ItemStruct = syn::parse_quote! {
        pub struct NamedModel {
            #[backwards_compat(versions(1: u32, 2: Self))]
            pub port: u16,
            pub host: String,
        }
    };
    let named_specs = field::extract_struct_field_compat(&named).unwrap();

    let tuple: syn::ItemStruct = syn::parse_quote! {
        pub struct TupleModel(
            #[backwards_compat(versions(1: u32, 2: Self))] u16
        );
    };
    let tuple_specs = field::extract_struct_field_compat(&tuple).unwrap();

    let unit: syn::ItemStruct = syn::parse_quote! {
        pub struct UnitModel;
    };

    // Case A: serialize = false, deserialize = true, derive_json_schema = false
    let de_only = ShadowWireOptions {
        serialize: false,
        deserialize: true,
        derive_json_schema: false,
    };
    for (item, specs, expected_de_with) in [
        (
            &named,
            named_specs.as_slice(),
            Some("__BcField_NamedModel_port::deserialize"),
        ),
        (
            &tuple,
            tuple_specs.as_slice(),
            Some("__BcField_TupleModel_0::deserialize"),
        ),
        (&unit, &[][..], None),
    ] {
        let built = build_struct_shadow_wire(item, specs, de_only);
        let shadow_str = built.shadow_tokens.to_string();
        let name = item.ident.to_string();

        assert!(built.borrowed_ty_in_helper.is_empty());
        assert!(built.construct_borrowed.is_empty());
        assert!(!shadow_str.contains(&format!("__{name}BorrowedWire")));
        assert!(!shadow_str.contains("Serialize"));
        assert!(!shadow_str.contains("serde (serialize_with"));
        assert!(!shadow_str.contains("serialize_ref"));

        assert!(shadow_str.contains(&format!("struct __{name}ShadowWire")));
        assert!(shadow_str.contains("derive (:: serde :: Deserialize)"));
        assert!(shadow_str.contains(&format!("From < __{name}ShadowWire > for {name}")));
        if let Some(de_with) = expected_de_with {
            assert!(shadow_str.contains(&format!("deserialize_with = \"{de_with}\"")));
        }

        syn::parse2::<syn::File>(built.shadow_tokens).unwrap();
    }

    // Case B: serialize = true, deserialize = false, derive_json_schema = false
    let ser_only = ShadowWireOptions {
        serialize: true,
        deserialize: false,
        derive_json_schema: false,
    };
    for (item, specs, expected_ser_with) in [
        (
            &named,
            named_specs.as_slice(),
            Some("__BcField_NamedModel_port::serialize_ref"),
        ),
        (
            &tuple,
            tuple_specs.as_slice(),
            Some("__BcField_TupleModel_0::serialize_ref"),
        ),
        (&unit, &[][..], None),
    ] {
        let built = build_struct_shadow_wire(item, specs, ser_only);
        let shadow_str = built.shadow_tokens.to_string();
        let name = item.ident.to_string();

        assert!(!built.borrowed_ty_in_helper.is_empty());
        assert!(!built.construct_borrowed.is_empty());
        assert!(!shadow_str.contains(&format!("__{name}ShadowWire")));
        assert!(!shadow_str.contains("Deserialize"));
        assert!(!shadow_str.contains("deserialize_with"));
        assert!(!shadow_str.contains(":: core :: convert :: From"));

        assert!(shadow_str.contains(&format!("struct __{name}BorrowedWire")));
        assert!(shadow_str.contains("derive (:: serde :: Serialize)"));
        if let Some(ser_with) = expected_ser_with {
            assert!(shadow_str.contains(&format!("serialize_with = \"{ser_with}\"")));
        }

        syn::parse2::<syn::File>(built.shadow_tokens).unwrap();
        syn::parse2::<syn::Expr>(built.construct_borrowed).unwrap();
    }
}

#[test]
fn test_build_enum_shadow_wire_conditional_emission() {
    let mixed: syn::ItemEnum = syn::parse_quote! {
        pub enum MixedEnum {
            Unit,
            Tuple(#[backwards_compat(versions(1: u32, 2: Self))] u64),
            Named {
                #[backwards_compat(versions(1: u32, 2: Self))]
                id: u64,
                label: String,
            },
        }
    };
    let mixed_specs = field::extract_enum_field_compat(&mixed).unwrap();

    let all_unit: syn::ItemEnum = syn::parse_quote! {
        pub enum UnitEnum {
            A,
            B,
        }
    };

    let empty: syn::ItemEnum = syn::parse_quote! {
        pub enum EmptyEnum {}
    };

    // Case A: serialize = false, deserialize = true, derive_json_schema = false
    let de_only = ShadowWireOptions {
        serialize: false,
        deserialize: true,
        derive_json_schema: false,
    };
    for (item, specs, has_field_compat) in [
        (&mixed, mixed_specs.as_slice(), true),
        (&all_unit, &[][..], false),
        (&empty, &[][..], false),
    ] {
        let built = build_enum_shadow_wire(item, specs, de_only);
        let shadow_str = built.shadow_tokens.to_string();
        let name = item.ident.to_string();

        assert!(built.borrowed_ty_in_helper.is_empty());
        assert!(built.construct_borrowed.is_empty());
        assert!(!shadow_str.contains(&format!("__{name}BorrowedWire")));
        assert!(!shadow_str.contains("Serialize"));
        assert!(!shadow_str.contains("serde (serialize_with"));
        assert!(!shadow_str.contains("serialize_ref"));

        assert!(shadow_str.contains(&format!("enum __{name}ShadowWire")));
        assert!(shadow_str.contains("derive (:: serde :: Deserialize)"));
        assert!(shadow_str.contains(&format!("From < __{name}ShadowWire > for {name}")));
        if has_field_compat {
            assert!(shadow_str.contains("deserialize_with = \"__BcField_MixedEnum_Tuple_0::deserialize\""));
            assert!(shadow_str.contains("deserialize_with = \"__BcField_MixedEnum_Named_id::deserialize\""));
        }

        syn::parse2::<syn::File>(built.shadow_tokens).unwrap();
    }

    // Case B: serialize = true, deserialize = false, derive_json_schema = false
    let ser_only = ShadowWireOptions {
        serialize: true,
        deserialize: false,
        derive_json_schema: false,
    };
    for (item, specs, has_field_compat) in [
        (&mixed, mixed_specs.as_slice(), true),
        (&all_unit, &[][..], false),
        (&empty, &[][..], false),
    ] {
        let built = build_enum_shadow_wire(item, specs, ser_only);
        let shadow_str = built.shadow_tokens.to_string();
        let name = item.ident.to_string();

        assert!(!built.borrowed_ty_in_helper.is_empty());
        assert!(!built.construct_borrowed.is_empty());
        assert!(!shadow_str.contains(&format!("__{name}ShadowWire")));
        assert!(!shadow_str.contains("Deserialize"));
        assert!(!shadow_str.contains("deserialize_with"));
        assert!(!shadow_str.contains(":: core :: convert :: From"));

        assert!(shadow_str.contains(&format!("enum __{name}BorrowedWire")));
        assert!(shadow_str.contains("derive (:: serde :: Serialize)"));
        if has_field_compat {
            assert!(shadow_str.contains("serialize_with = \"__BcField_MixedEnum_Tuple_0::serialize_ref\""));
            assert!(shadow_str.contains("serialize_with = \"__BcField_MixedEnum_Named_id::serialize_ref\""));
        }

        syn::parse2::<syn::File>(built.shadow_tokens).unwrap();
        syn::parse2::<syn::Expr>(built.construct_borrowed).unwrap();
    }
}

#[cfg(feature = "schemars")]
#[test]
fn test_shadow_wire_deserialize_false_with_json_schema() {
    let opts = ShadowWireOptions {
        serialize: true,
        deserialize: false,
        derive_json_schema: true,
    };

    // Struct with field-compat field
    let item_struct: syn::ItemStruct = syn::parse_quote! {
        pub struct SchemaStruct {
            #[backwards_compat(versions(1: u32, 2: Self))]
            pub port: u16,
            pub name: String,
        }
    };
    let struct_specs = field::extract_struct_field_compat(&item_struct).unwrap();
    let built_struct = build_struct_shadow_wire(&item_struct, &struct_specs, opts);
    let s = built_struct.shadow_tokens.to_string();

    assert!(s.contains("struct __SchemaStructShadowWire"));
    assert!(s.contains("derive (:: backwards_compat :: __private :: schemars :: JsonSchema)"));
    assert!(!s.contains("Deserialize"));
    assert!(!s.contains("deserialize_with"));
    assert!(!s.contains(":: core :: convert :: From"));
    assert!(s.contains("schemars (with = \"__BcField_SchemaStruct_port_Schema\")"));
    assert!(s.contains("struct __SchemaStructBorrowedWire"));
    assert!(s.contains("derive (:: serde :: Serialize)"));
    assert!(s.contains("serialize_with = \"__BcField_SchemaStruct_port::serialize_ref\""));
    syn::parse2::<syn::File>(built_struct.shadow_tokens).unwrap();

    // Enum with field-compat field
    let item_enum: syn::ItemEnum = syn::parse_quote! {
        pub enum SchemaEnum {
            Unit,
            Data(#[backwards_compat(versions(1: u32, 2: Self))] u64),
        }
    };
    let enum_specs = field::extract_enum_field_compat(&item_enum).unwrap();
    let built_enum = build_enum_shadow_wire(&item_enum, &enum_specs, opts);
    let e = built_enum.shadow_tokens.to_string();

    assert!(e.contains("enum __SchemaEnumShadowWire"));
    assert!(e.contains("derive (:: backwards_compat :: __private :: schemars :: JsonSchema)"));
    assert!(!e.contains("Deserialize"));
    assert!(!e.contains("deserialize_with"));
    assert!(!e.contains(":: core :: convert :: From"));
    assert!(e.contains("schemars (with = \"__BcField_SchemaEnum_Data_0_Schema\")"));
    assert!(e.contains("enum __SchemaEnumBorrowedWire"));
    assert!(e.contains("derive (:: serde :: Serialize)"));
    assert!(e.contains("serialize_with = \"__BcField_SchemaEnum_Data_0::serialize_ref\""));
    syn::parse2::<syn::File>(built_enum.shadow_tokens).unwrap();
}

#[test]
fn test_via_parse_serialize_and_deserialize_options() {
    let default_args: via::ViaArgs = syn::parse_str("via = Proxy").unwrap();
    assert!(default_args.serialize);
    assert!(default_args.deserialize);

    for (input, expected_ser, expected_de) in [
        ("via = Proxy, serialize = true", true, true),
        ("via = Proxy, serialize = false", false, true),
        ("via = Proxy, serialize(true)", true, true),
        ("via = Proxy, serialize(false)", false, true),
        ("via = Proxy, deserialize = true", true, true),
        ("via = Proxy, deserialize = false", true, false),
        ("via = Proxy, deserialize(true)", true, true),
        ("via = Proxy, deserialize(false)", true, false),
        ("via = Proxy, serialize = false, deserialize = true", false, true),
        ("via = Proxy, serialize(true), deserialize(false)", true, false),
    ] {
        let parsed: via::ViaArgs = syn::parse_str(input).unwrap();
        assert_eq!(parsed.serialize, expected_ser, "input: {input}");
        assert_eq!(parsed.deserialize, expected_de, "input: {input}");
    }

    // serialize = false allows `from` and `try_from`
    let de_only: via::ViaArgs =
        syn::parse_str("via = Proxy, serialize = false, from = decode_fn, try_from").unwrap();
    assert!(!de_only.serialize);
    assert!(de_only.deserialize);
    assert!(de_only.from_fn.is_some());
    assert!(de_only.try_from);

    // deserialize = false allows `to` and `try_into`
    let ser_only: via::ViaArgs =
        syn::parse_str("via = Proxy, deserialize = false, to = encode_fn, try_into").unwrap();
    assert!(ser_only.serialize);
    assert!(!ser_only.deserialize);
    assert!(ser_only.to_fn.is_some());
    assert!(ser_only.try_into);
}

#[test]
fn test_via_parse_serialize_and_deserialize_errors() {
    for (input, expected_msg) in [
        (
            "via = Proxy, serialize = true, serialize = false",
            "duplicate `serialize` option in #[backwards_compat(...)]",
        ),
        (
            "via = Proxy, deserialize(true), deserialize(false)",
            "duplicate `deserialize` option in #[backwards_compat(...)]",
        ),
        (
            "via = Proxy, serialize",
            "expected `= true|false` or `(true|false)` for `serialize`",
        ),
        (
            "via = Proxy, deserialize",
            "expected `= true|false` or `(true|false)` for `deserialize`",
        ),
        (
            "via = Proxy, serialize = \"false\"",
            "expected boolean literal (`true` or `false`) for `serialize`",
        ),
        (
            "via = Proxy, deserialize(1)",
            "expected boolean literal (`true` or `false`) for `deserialize`",
        ),
        (
            "via = Proxy, serialize()",
            "expected boolean literal (`true` or `false`) for `serialize`",
        ),
        (
            "via = Proxy, serialize(true, false)",
            "unexpected trailing tokens in `serialize(...)`",
        ),
        (
            "via = Proxy, serialize = false, deserialize = false",
            "cannot disable both `serialize` and `deserialize` in `#[backwards_compat(...)]`; at least one must be enabled",
        ),
        (
            "via = Proxy, deserialize(false), serialize(false)",
            "cannot disable both `serialize` and `deserialize` in `#[backwards_compat(...)]`; at least one must be enabled",
        ),
        (
            "via = Proxy, serialize = false, to = encode_fn",
            "cannot use `to` when `serialize = false`",
        ),
        (
            "via = Proxy, serialize = false, try_into",
            "cannot use `try_into` when `serialize = false`",
        ),
        (
            "via = Proxy, serialize = false, fallible",
            "cannot use `fallible` when `serialize = false`; use `try_from` instead",
        ),
        (
            "via = Proxy, deserialize = false, from = decode_fn",
            "cannot use `from` when `deserialize = false`",
        ),
        (
            "via = Proxy, deserialize = false, try_from",
            "cannot use `try_from` when `deserialize = false`",
        ),
        (
            "via = Proxy, deserialize = false, fallible",
            "cannot use `fallible` when `deserialize = false`; use `try_into` instead",
        ),
    ] {
        let err = syn::parse_str::<via::ViaArgs>(input).unwrap_err();
        assert_eq!(err.to_string(), expected_msg, "input: {input}");
    }
}

#[test]
fn test_via_generate_conditional_serialize_and_deserialize() {
    let item: syn::DeriveInput = syn::parse_quote!(
        struct Domain {
            val: u32,
        }
    );

    // serialize = false
    let de_args: via::ViaArgs = syn::parse_str("via = Proxy, serialize = false, try_from").unwrap();
    let de_ts = via::generate_via(&de_args, &item.ident, &item.generics, false);
    let de_rendered = de_ts.to_string();
    assert!(!de_rendered.contains("impl :: serde :: Serialize"));
    assert!(!de_rendered.contains("BackwardsCompatSerialize"));
    assert!(de_rendered.contains("impl :: backwards_compat :: BackwardsCompat for Domain"));
    assert!(de_rendered.contains("impl < '__de > :: serde :: Deserialize < '__de > for Domain"));
    assert!(
        de_rendered.contains(
            "impl < '__de > :: backwards_compat :: BackwardsCompatDeserialize < '__de > for Domain"
        )
    );
    syn::parse2::<syn::File>(de_ts).unwrap();

    // deserialize = false
    let ser_args: via::ViaArgs = syn::parse_str("via = Proxy, deserialize = false, try_into").unwrap();
    let ser_ts = via::generate_via(&ser_args, &item.ident, &item.generics, false);
    let ser_rendered = ser_ts.to_string();
    assert!(!ser_rendered.contains("impl < '__de > :: serde :: Deserialize"));
    assert!(!ser_rendered.contains("BackwardsCompatDeserialize"));
    assert!(ser_rendered.contains("impl :: backwards_compat :: BackwardsCompat for Domain"));
    assert!(ser_rendered.contains("impl :: serde :: Serialize for Domain"));
    assert!(ser_rendered.contains("impl :: backwards_compat :: BackwardsCompatSerialize for Domain"));
    syn::parse2::<syn::File>(ser_ts).unwrap();
}

#[cfg(feature = "schemars")]
#[test]
fn test_via_generate_schemars_with_serialize_or_deserialize_false() {
    let item: syn::DeriveInput = syn::parse_quote!(
        struct Domain {
            val: u32,
        }
    );

    // serialize = false + JsonSchema
    let de_args: via::ViaArgs = syn::parse_str("via = Proxy, serialize = false").unwrap();
    let de_ts = via::generate_via(&de_args, &item.ident, &item.generics, true);
    let de_rendered = de_ts.to_string();
    assert!(
        de_rendered.contains("impl :: backwards_compat :: __private :: schemars :: JsonSchema for Domain")
    );
    assert!(!de_rendered.contains("impl :: serde :: Serialize"));
    assert!(de_rendered.contains("impl < '__de > :: serde :: Deserialize < '__de > for Domain"));
    syn::parse2::<syn::File>(de_ts).unwrap();

    // deserialize = false + JsonSchema
    let ser_args: via::ViaArgs = syn::parse_str("via = Proxy, deserialize = false").unwrap();
    let ser_ts = via::generate_via(&ser_args, &item.ident, &item.generics, true);
    let ser_rendered = ser_ts.to_string();
    assert!(
        ser_rendered.contains("impl :: backwards_compat :: __private :: schemars :: JsonSchema for Domain")
    );
    assert!(ser_rendered.contains("impl :: serde :: Serialize for Domain"));
    assert!(!ser_rendered.contains("impl < '__de > :: serde :: Deserialize"));
    syn::parse2::<syn::File>(ser_ts).unwrap();
}

#[test]
fn test_clean_derive_attrs_selective_and_has_serde_derive() {
    // (false, true): keeps Serialize, strips Deserialize (and JsonSchema under schemars)
    #[cfg(feature = "schemars")]
    let mut keep_ser: Vec<syn::Attribute> =
        vec![syn::parse_quote!(#[derive(Debug, Serialize, Deserialize, JsonSchema)])];
    #[cfg(not(feature = "schemars"))]
    let mut keep_ser: Vec<syn::Attribute> = vec![syn::parse_quote!(#[derive(Debug, Serialize, Deserialize)])];

    clean_derive_attrs(&mut keep_ser, false, true);
    let keep_ser_str = quote::quote!(#(#keep_ser)*).to_string();
    assert!(keep_ser_str.contains("Debug"));
    assert!(keep_ser_str.contains("Serialize"));
    assert!(!keep_ser_str.contains("Deserialize"));
    #[cfg(feature = "schemars")]
    assert!(!keep_ser_str.contains("JsonSchema"));
    assert!(has_serde_derive(&keep_ser));

    // (true, false): keeps Deserialize, strips Serialize
    let mut keep_de: Vec<syn::Attribute> =
        vec![syn::parse_quote!(#[derive(Debug, serde::Serialize, ::serde::Deserialize)])];
    clean_derive_attrs(&mut keep_de, true, false);
    let keep_de_str = quote::quote!(#(#keep_de)*).to_string();
    assert!(keep_de_str.contains("Debug"));
    assert!(keep_de_str.contains(":: serde :: Deserialize"));
    assert!(!keep_de_str.contains("Serialize"));
    assert!(has_serde_derive(&keep_de));

    // has_serde_derive variants
    assert!(has_serde_derive(&[syn::parse_quote!(#[derive(Serialize)])]));
    assert!(has_serde_derive(&[
        syn::parse_quote!(#[derive(serde::Serialize)])
    ]));
    assert!(has_serde_derive(&[syn::parse_quote!(#[derive(Deserialize)])]));
    assert!(has_serde_derive(&[
        syn::parse_quote!(#[derive(::serde::Deserialize)])
    ]));
    assert!(!has_serde_derive(&[syn::parse_quote!(#[derive(Debug, Clone)])]));
    assert!(!has_serde_derive(&[]));
}

#[test]
fn test_backwards_compat_impl_selective_serialize_deserialize_struct_and_enum() {
    // 1. serialize = false WITH #[derive(Serialize)] keeps Serialize & #[serde(...)] on cleaned struct
    let ser_false_keep = backwards_compat_impl(
        quote::quote!(serialize = false, versions(1: V1, 2: Self)),
        quote::quote! {
            #[derive(Debug, Serialize, Deserialize)]
            #[serde(rename_all = "camelCase")]
            pub struct KeepSerModel {
                #[serde(rename = "n")]
                pub name: String,
            }
        },
    );
    let rendered = ser_false_keep.to_string();
    assert!(!rendered.contains("compile_error !"));
    assert!(!rendered.contains("__KeepSerModelBorrowedWire"));
    assert!(!rendered.contains("impl :: serde :: Serialize"));
    assert!(rendered.contains("struct __KeepSerModelShadowWire"));
    assert!(rendered.contains("impl < '__de > :: serde :: Deserialize < '__de > for KeepSerModel"));

    let parsed_file: syn::File = syn::parse2(ser_false_keep).unwrap();
    let Some(syn::Item::Struct(cleaned_struct)) = parsed_file.items.first() else {
        panic!("expected struct as first item");
    };
    let cleaned_struct_str = quote::quote!(#cleaned_struct).to_string();
    assert!(cleaned_struct_str.contains("derive (Debug , Serialize)"));
    assert!(cleaned_struct_str.contains("rename_all = \"camelCase\""));
    assert!(cleaned_struct_str.contains("rename = \"n\""));

    // 2. serialize = false WITHOUT #[derive(Serialize)] strips #[serde(...)] from cleaned struct
    let ser_false_strip = backwards_compat_impl(
        quote::quote!(serialize = false, versions(1: V1, 2: Self)),
        quote::quote! {
            #[derive(Debug, Deserialize)]
            #[serde(rename_all = "camelCase")]
            pub struct StripSerdeModel {
                #[serde(rename = "n")]
                pub name: String,
            }
        },
    );
    let parsed_strip: syn::File = syn::parse2(ser_false_strip).unwrap();
    let Some(syn::Item::Struct(stripped_struct)) = parsed_strip.items.first() else {
        panic!("expected struct as first item");
    };
    let stripped_struct_str = quote::quote!(#stripped_struct).to_string();
    assert!(stripped_struct_str.contains("derive (Debug)"));
    assert!(!stripped_struct_str.contains("serde"));

    // 3. deserialize = false WITH #[derive(Deserialize)] keeps Deserialize & #[serde(...)] on cleaned enum
    let de_false_keep = backwards_compat_impl(
        quote::quote!(deserialize = false, versions(1: V1, 2: Self)),
        quote::quote! {
            #[derive(Debug, Serialize, Deserialize)]
            #[serde(rename_all = "camelCase")]
            pub enum KeepDeEnum {
                #[serde(rename = "a")]
                Alpha(u32),
            }
        },
    );
    let de_rendered = de_false_keep.to_string();
    assert!(!de_rendered.contains("compile_error !"));
    assert!(!de_rendered.contains("__KeepDeEnumShadowWire"));
    assert!(!de_rendered.contains("impl < '__de > :: serde :: Deserialize"));
    assert!(de_rendered.contains("enum __KeepDeEnumBorrowedWire"));
    assert!(de_rendered.contains("impl :: serde :: Serialize for KeepDeEnum"));

    let parsed_enum_file: syn::File = syn::parse2(de_false_keep).unwrap();
    let Some(syn::Item::Enum(cleaned_enum)) = parsed_enum_file.items.first() else {
        panic!("expected enum as first item");
    };
    let cleaned_enum_str = quote::quote!(#cleaned_enum).to_string();
    assert!(cleaned_enum_str.contains("derive (Debug , Deserialize)"));
    assert!(cleaned_enum_str.contains("rename_all = \"camelCase\""));
    assert!(cleaned_enum_str.contains("rename = \"a\""));
}

#[test]
fn test_backwards_compat_via_impl_selective_serialize_deserialize() {
    let via_ser_false = backwards_compat_via_impl(
        quote::quote!(via = Proxy, serialize = false),
        quote::quote! {
            #[derive(Debug, Serialize, Deserialize)]
            pub struct ViaDeOnly {
                pub x: u32,
            }
        },
    );
    let s1 = via_ser_false.to_string();
    assert!(!s1.contains("compile_error !"));
    assert!(!s1.contains("impl :: serde :: Serialize"));
    assert!(s1.contains("impl < '__de > :: serde :: Deserialize < '__de > for ViaDeOnly"));
    let f1: syn::File = syn::parse2(via_ser_false).unwrap();
    let Some(syn::Item::Struct(st1)) = f1.items.first() else {
        panic!("expected struct");
    };
    assert!(
        quote::quote!(#st1)
            .to_string()
            .contains("derive (Debug , Serialize)")
    );

    let via_de_false = backwards_compat_via_impl(
        quote::quote!(via = ProxyEnum, deserialize = false),
        quote::quote! {
            #[derive(Debug, Serialize, Deserialize)]
            pub enum ViaSerOnly {
                A,
                B(u32),
            }
        },
    );
    let s2 = via_de_false.to_string();
    assert!(!s2.contains("compile_error !"));
    assert!(!s2.contains("impl < '__de > :: serde :: Deserialize"));
    assert!(s2.contains("impl :: serde :: Serialize for ViaSerOnly"));
    let f2: syn::File = syn::parse2(via_de_false).unwrap();
    let Some(syn::Item::Enum(en2)) = f2.items.first() else {
        panic!("expected enum");
    };
    assert!(
        quote::quote!(#en2)
            .to_string()
            .contains("derive (Debug , Deserialize)")
    );
}
