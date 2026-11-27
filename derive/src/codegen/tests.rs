use super::*;
use crate::dag::resolve_dag;
use crate::parse::BackwardsCompatArgs;

fn parse_test_input(args_tokens: proc_macro2::TokenStream) -> BackwardsCompatInput {
    let args: BackwardsCompatArgs = syn::parse2(args_tokens).unwrap();
    let target_ty: syn::Type = syn::parse_quote!(TargetModel);
    let vis: syn::Visibility = syn::parse_quote!(pub);
    args.into_input(target_ty, vis, syn::Generics::default()).unwrap()
}

fn generate_for_test(
    input: &BackwardsCompatInput,
    wire_mode: SelfWireMode<'_>,
    extra_items: Option<TokenStream>,
) -> syn::Result<TokenStream> {
    let dag_plan = resolve_dag(input)?;
    generate_backwards_compat(input, &dag_plan, wire_mode, extra_items, false)
}

#[test]
fn test_generate_backwards_compat_streaming_match_arms_no_helpers() {
    let input = parse_test_input(quote! {
        versions = [
            1: ModelV1 => 2,
            #[fallible]
            2: ModelV2 => 3,
            3: ModelV3,
        ]
    });

    let generated = generate_for_test(&input, SelfWireMode::Disallowed, None).unwrap();
    let rendered = generated.to_string();

    // No __VersionHelper, __UntaggedHelper, or VariantContentDeserializer
    assert!(!rendered.contains("__VersionHelper"));
    assert!(!rendered.contains("__UntaggedHelper"));
    assert!(!rendered.contains("VariantContentDeserializer"));

    // Uses streaming Visitor + generalized visit_internal_map
    assert!(rendered.contains("__deserializer . deserialize_any (__visitor)"));
    assert!(rendered.contains(
        "visit_internal_map :: < :: backwards_compat :: __private :: IntTagMode , Self , __A > (__map , \"version\" ,)"
    ));
    assert!(!rendered.contains("RemainingMapDeserializer :: new (__map)"));

    // Direct deserialize + non-generic tail-chained migration helpers
    assert!(rendered.contains("< ModelV1 as :: serde :: Deserialize < '__de >> :: deserialize (__d) ?"));
    assert!(rendered.contains("fn __bc_migrate_0 (__val : ModelV3) -> TargetModel"));
    assert!(rendered.contains(
        "fn __bc_migrate_1 (__val : ModelV2) -> :: core :: result :: Result < TargetModel , :: backwards_compat :: __private :: String >"
    ));
    assert!(rendered.contains(
        "fn __bc_migrate_2 (__val : ModelV1) -> :: core :: result :: Result < TargetModel , :: backwards_compat :: __private :: String >"
    ));
    assert!(rendered.contains(
        "let __val : TargetModel = __bc_migrate_2 (__val) . map_err (:: serde :: de :: Error :: custom) ? ;"
    ));
    assert!(rendered.contains(
        "let __val : TargetModel = __bc_migrate_1 (__val) . map_err (:: serde :: de :: Error :: custom) ? ;"
    ));
    assert!(rendered.contains("let __val : TargetModel = __bc_migrate_0 (__val) ;"));
    assert!(rendered.contains("< ModelV2 as :: serde :: Deserialize < '__de >> :: deserialize (__d) ?"));
    assert!(rendered.contains("< ModelV3 as :: serde :: Deserialize < '__de >> :: deserialize (__d) ?"));
    assert!(!rendered.contains("impl :: core :: convert :: From <"));
    assert!(!rendered.contains("impl :: core :: convert :: TryFrom <"));

    syn::parse2::<syn::File>(generated).unwrap();
}

#[test]
fn test_serialize_helper_derives_and_no_version_helper() {
    let input = parse_test_input(quote! {
        versions = [
            1: ModelV1 => 2,
            2: ModelV2 => 3,
            3: ModelV3,
        ]
    });

    let generated = generate_for_test(&input, SelfWireMode::Disallowed, None).unwrap();
    let rendered = generated.to_string();

    assert!(!rendered.contains("# [derive (:: serde :: Deserialize)]"));
    assert!(!rendered.contains("# [derive (:: serde :: Serialize)]"));
    assert!(rendered.contains("serialize_internally_tagged"));
    assert!(!rendered.contains(":: std ::"));
}

#[test]
fn test_generate_backwards_compat_target_is_wire_with_shadow() {
    let input = parse_test_input(quote! {
        versions = [
            1: ModelV1 => 2,
            2: ModelV2 => 3,
            3: Self,
        ]
    });

    let owned_shadow = format_ident!("TargetModelWire");
    let borrowed_shadow = format_ident!("TargetModelBorrowedWire");
    let shadow_info = ShadowWireInfo {
        owned_ident: &owned_shadow,
        borrowed_ident: &borrowed_shadow,
        borrowed_ty_in_helper: quote!(TargetModelBorrowedWire<'__a>),
        construct_borrowed: quote!(TargetModelBorrowedWire {}),
        is_unit_struct: false,
    };
    let generated = generate_for_test(&input, SelfWireMode::Shadow(&shadow_info), None).unwrap();
    let rendered = generated.to_string();

    assert!(
        rendered.contains("< TargetModelWire as :: serde :: Deserialize < '__de >> :: deserialize (__d) ?")
    );
    assert!(rendered.contains("let __val : TargetModel = :: core :: convert :: Into :: into (__val) ;"));
    assert!(rendered.contains("serialize_internally_tagged"));
    assert!(rendered.contains("for < '__a > TargetModelBorrowedWire < '__a > : :: serde :: Serialize"));
    syn::parse2::<syn::File>(generated).unwrap();
}

#[test]
fn test_generate_backwards_compat_target_is_wire_direct_mode() {
    let input = parse_test_input(quote! {
        versions = [
            1: ModelV1 => 2,
            2: Self,
        ]
    });
    let dag_plan = resolve_dag(&input).unwrap();

    let ser = generate_serialize_core(&input, &dag_plan, SelfWireMode::Direct, &quote!(__value)).unwrap();
    let h = &ser.helper_def;
    let b = &ser.body;
    let ser_str = quote!(#h fn f() { #b }).to_string();
    assert!(ser_str.contains("serialize_internally_tagged"));
    assert!(ser_str.contains("__value"));

    let de = generate_deserialize_core(&input, &dag_plan, SelfWireMode::Direct, false).unwrap();
    let de_str = de.base_body.to_string();
    assert!(de_str.contains("< TargetModel as :: serde :: Deserialize < '__de >> :: deserialize (__d) ?"));
    assert!(!de_str.contains("Migrated"));
}

#[test]
fn test_generate_backwards_compat_target_is_wire_without_shadow_errors() {
    let input = parse_test_input(quote! {
        versions = [
            1: ModelV1 => 2,
            2: ModelV2 => 3,
            3: Self,
        ]
    });

    let result = generate_for_test(&input, SelfWireMode::Disallowed, None);
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().to_string(),
        "using the domain struct as the active wire format is only supported when using the #[backwards_compat] attribute macro"
    );
}

#[test]
fn test_generate_backwards_compat_generics_and_where_clause() {
    let args: BackwardsCompatArgs = syn::parse2(quote! {
        versions = [
            1: GenericModelV1<T> => 2,
            2: GenericModelV2<T>,
        ]
    })
    .unwrap();
    let target_ty: syn::Type = syn::parse_quote!(GenericModel<T>);
    let vis: syn::Visibility = syn::parse_quote!(pub);
    let mut generics: syn::Generics = syn::parse_quote!(<T: Clone + 'static>);
    generics.where_clause = syn::parse_quote!(where T: std::fmt::Debug);
    let input = args.into_input(target_ty, vis, generics).unwrap();

    let generated = generate_for_test(&input, SelfWireMode::Disallowed, None).unwrap();
    let rendered = generated.to_string();

    assert!(!rendered.contains("__VersionHelper"));
    assert!(rendered.contains(
        "impl < T : Clone + 'static > :: serde :: Serialize for GenericModel < T > where T : std :: fmt :: Debug , for < '__a > GenericModelV2 < T > : :: serde :: Serialize"
    ));
    assert!(rendered.contains(
        "impl < T : Clone + 'static > :: serde :: Serialize for GenericModel < T > where T : std :: fmt :: Debug"
    ));
    assert!(rendered.contains(
        "impl < '__de , T : Clone + 'static > :: serde :: Deserialize < '__de > for GenericModel < T > where T : std :: fmt :: Debug , GenericModelV1 < T > : :: serde :: Deserialize < '__de > , GenericModelV2 < T > : :: serde :: Deserialize < '__de >"
    ));
    syn::parse2::<syn::File>(generated).unwrap();
}

#[test]
fn test_generate_backwards_compat_generic_cfg_gated_arm_trait() {
    let args: BackwardsCompatArgs = syn::parse2(quote! {
        versions = [
            #[cfg(feature = "v1")]
            1: GenericModelV1<T> => 2,
            2: GenericModelV2<T>,
        ]
    })
    .unwrap();
    let target_ty: syn::Type = syn::parse_quote!(GenericModel<T>);
    let vis: syn::Visibility = syn::parse_quote!(pub);
    let generics: syn::Generics = syn::parse_quote!(<T>);
    let input = args.into_input(target_ty, vis, generics).unwrap();

    let generated = generate_for_test(&input, SelfWireMode::Disallowed, None).unwrap();
    let rendered = generated.to_string();

    assert!(rendered.contains("trait __BcArm_0 < '__de >"));
    assert!(rendered.contains(
        "# [cfg (feature = \"v1\")] impl < '__de , T > __BcArm_0 < '__de > for GenericModel < T > where GenericModelV1 < T > : :: serde :: Deserialize < '__de >"
    ));
    assert!(rendered.contains(
        "# [cfg (not (feature = \"v1\"))] impl < '__de , T > __BcArm_0 < '__de > for GenericModel < T > { }"
    ));
    assert!(rendered.contains("GenericModel < T > : __BcArm_0 < '__de >"));
    syn::parse2::<syn::File>(generated).unwrap();
}

#[test]
fn test_generate_backwards_compat_adjacent_tagging() {
    let args: BackwardsCompatArgs = syn::parse2(quote! {
        tag = "type",
        content = "data",
        versions = [
            1: ModelV1 => 2,
            2: ModelV2,
        ]
    })
    .unwrap();
    let target_ty: syn::Type = syn::parse_quote!(TargetModel);
    let vis: syn::Visibility = syn::parse_quote!(pub);
    let input = args.into_input(target_ty, vis, syn::Generics::default()).unwrap();

    let generated = generate_for_test(&input, SelfWireMode::Disallowed, None).unwrap();
    let rendered = generated.to_string();

    assert!(
        rendered.contains("__deserializer . deserialize_struct (\"\" , & [\"type\" , \"data\"] , __visitor)")
    );
    assert!(rendered.contains(
        "visit_adjacent_map :: < :: backwards_compat :: __private :: IntTagMode , Self , __A > (__map , \"type\" , \"data\" ,)"
    ));
    assert!(rendered.contains("read_adjacent_content :: < __M , ModelV1 >"));
    assert!(rendered.contains("fn visit_seq < __A >"));
    assert!(rendered.contains("serialize_adjacently_tagged"));
    assert!(!rendered.contains("struct __SerializeHelper"));
    assert!(!rendered.contains("struct __BcAdjSerHelper"));
    syn::parse2::<syn::File>(generated).unwrap();
}

#[test]
fn test_generate_backwards_compat_explicit_wire_autoref() {
    let args: BackwardsCompatArgs = syn::parse2(quote! {
        versions = [
            1: ModelV1 => 2,
            2: ModelV2,
        ]
    })
    .unwrap();
    let target_ty: syn::Type = syn::parse_quote!(TargetModel);
    let vis: syn::Visibility = syn::parse_quote!(pub);
    let input = args.into_input(target_ty, vis, syn::Generics::default()).unwrap();

    let generated = generate_for_test(&input, SelfWireMode::Disallowed, None).unwrap();
    let rendered = generated.to_string();

    assert!(rendered.contains("use :: backwards_compat :: __private :: { BorrowedWireConvert as _ , FallbackWireConvert as _ , WireConvert as __WireConvert , } ;"));
    assert!(rendered.contains(
        "let latest : ModelV2 = __WireConvert :: < _ , ModelV2 > :: new (self) . convert_wire () ;"
    ));
}

#[test]
fn test_generate_backwards_compat_cfg_on_match_arms() {
    let input = parse_test_input(quote! {
        versions = [
            #[cfg(feature = "v1")]
            1: ModelV1 => 2,
            #[cfg(feature = "v2")]
            #[fallible]
            2: ModelV2 => 3,
            3: ModelV3,
        ]
    });

    let generated = generate_for_test(&input, SelfWireMode::Disallowed, None).unwrap();
    let rendered = generated.to_string();

    assert!(!rendered.contains("__VersionHelper"));
    assert!(rendered.contains("# [cfg (all (feature = \"v1\" , feature = \"v2\"))] 1u64 =>"));
    assert!(rendered.contains("# [cfg (feature = \"v2\")] 2u64 =>"));
    assert!(rendered.contains("3u64 =>"));

    syn::parse2::<syn::File>(generated).unwrap();
}

#[test]
fn test_generate_backwards_compat_gated_untagged_explicit_type() {
    let input = parse_test_input(quote! {
        versions = [
            #[cfg(test)]
            _: ModelV0 => 1,
            1: ModelV1,
        ]
    });

    let generated = generate_for_test(&input, SelfWireMode::Disallowed, None).unwrap();
    let rendered = generated.to_string();

    assert!(!rendered.contains("__UntaggedHelper"));
    assert!(!rendered.contains("__VersionHelper"));
    assert!(
        rendered
            .contains("# [cfg (test)] impl < '__de > :: serde :: Deserialize < '__de > for TargetModel {")
    );
    assert!(
        rendered.contains(
            "# [cfg (not (test))] impl < '__de > :: serde :: Deserialize < '__de > for TargetModel {"
        )
    );
    assert!(rendered.contains("# [cfg (test)] const ACCEPTS_UNTAGGED : bool = true ;"));
    assert!(rendered.contains("# [cfg (not (test))] const ACCEPTS_UNTAGGED : bool = false ;"));
    // Single cfg-gated untagged arm uses ContentRefDeserializer (never moves __content)
    assert!(rendered.contains("ContentRefDeserializer :: < __E > :: new (& __content)"));

    syn::parse2::<syn::File>(generated).unwrap();
}

#[test]
fn test_generate_backwards_compat_gated_untagged_self_with_shadow() {
    let input = parse_test_input(quote! {
        versions = [
            #[cfg(test)]
            _: Self,
            1: ModelV1 => 2,
            2: Self,
        ]
    });

    let owned_shadow = format_ident!("TargetModelWire");
    let borrowed_shadow = format_ident!("TargetModelBorrowedWire");
    let shadow_info = ShadowWireInfo {
        owned_ident: &owned_shadow,
        borrowed_ident: &borrowed_shadow,
        borrowed_ty_in_helper: quote!(TargetModelBorrowedWire<'__a>),
        construct_borrowed: quote!(TargetModelBorrowedWire {}),
        is_unit_struct: false,
    };

    let generated = generate_for_test(&input, SelfWireMode::Shadow(&shadow_info), None).unwrap();
    let rendered = generated.to_string();

    assert!(rendered.contains(
        "< TargetModelWire as :: serde :: Deserialize < '__de >> :: deserialize (:: backwards_compat :: __private :: ContentRefDeserializer :: < __E > :: new (& __content))"
    ));
    assert!(
        rendered
            .contains("# [cfg (test)] impl < '__de > :: serde :: Deserialize < '__de > for TargetModel {")
    );
    assert!(
        rendered.contains(
            "# [cfg (not (test))] impl < '__de > :: serde :: Deserialize < '__de > for TargetModel {"
        )
    );

    syn::parse2::<syn::File>(generated).unwrap();
}

#[test]
fn test_generate_backwards_compat_single_unconditional_untagged_zero_buffer() {
    let input = parse_test_input(quote! {
        versions = [
            _: LegacyV0 => 1,
            1: ModelV1,
        ]
    });

    let generated = generate_for_test(&input, SelfWireMode::Disallowed, None).unwrap();
    let rendered = generated.to_string();

    assert!(
        rendered.contains(
            "__deserializer . deserialize_any (:: backwards_compat :: __private :: UntaggedSingleVisitor (__visitor) ,)"
        )
    );
    assert!(rendered.contains("UntaggedSingleDispatch < '__de > for __BcVisitor"));
    assert!(rendered.contains("fn dispatch_de"));
    assert!(!rendered.contains("UntaggedMultiDispatch"));
    assert!(!rendered.contains("fn dispatch_content"));
    assert!(rendered.contains("visit_internal_map_untagged"));
    assert!(rendered.contains("IntTagMode"));
    assert!(!rendered.contains(":: std ::"));
    syn::parse2::<syn::File>(generated).unwrap();

    let str_untagged = parse_test_input(quote! {
        versions = [
            _: LegacyV0 => "v1",
            "v1": ModelV1,
        ]
    });
    let str_rendered = generate_for_test(&str_untagged, SelfWireMode::Disallowed, None)
        .unwrap()
        .to_string();
    assert!(str_rendered.contains("visit_internal_map_untagged"));
    assert!(str_rendered.contains("StrTagMode"));
}

#[test]
fn test_generate_backwards_compat_version_modes() {
    // All-int -> u64
    let int_input = parse_test_input(quote! {
        versions = [
            1: ModelV1 => 2,
            2: ModelV2,
        ]
    });
    let int_gen = generate_for_test(&int_input, SelfWireMode::Disallowed, None).unwrap();
    let int_rendered = int_gen.to_string();
    assert!(int_rendered.contains("type Version = u64 ;"));
    assert!(int_rendered.contains("const CURRENT_VERSION : Self :: Version = 2u64 ;"));
    assert!(int_rendered.contains("const SERIALIZE_VERSION : Self :: Version = 2u64 ;"));
    assert!(
        int_rendered.contains("const SUPPORTED_VERSIONS : & 'static [Self :: Version] = & [1u64 , 2u64 ,] ;")
    );
    syn::parse2::<syn::File>(int_gen).unwrap();

    // All-str -> &'static str
    let str_input = parse_test_input(quote! {
        versions = [
            "v1": ModelV1 => "v2",
            "v2": ModelV2,
        ]
    });
    let str_gen = generate_for_test(&str_input, SelfWireMode::Disallowed, None).unwrap();
    let str_rendered = str_gen.to_string();
    assert!(str_rendered.contains("type Version = & 'static str ;"));
    assert!(str_rendered.contains("const CURRENT_VERSION : Self :: Version = \"v2\" ;"));
    assert!(str_rendered.contains("const SERIALIZE_VERSION : Self :: Version = \"v2\" ;"));
    assert!(str_rendered.contains("StrTagMode"));
    syn::parse2::<syn::File>(str_gen).unwrap();
}

#[test]
fn test_generate_backwards_compat_serialize_helper_variants() {
    // Internal unit struct uses serialize_unit_tagged without generating __SerializeHelper
    let unit_input = parse_test_input(quote! {
        versions = [
            1: Self,
        ]
    });
    let owned_shadow = format_ident!("TargetModelWire");
    let borrowed_shadow = format_ident!("TargetModelBorrowedWire");
    let unit_shadow = ShadowWireInfo {
        owned_ident: &owned_shadow,
        borrowed_ident: &borrowed_shadow,
        borrowed_ty_in_helper: quote!(TargetModelBorrowedWire),
        construct_borrowed: quote!(TargetModelBorrowedWire),
        is_unit_struct: true,
    };
    let unit_gen = generate_for_test(&unit_input, SelfWireMode::Shadow(&unit_shadow), None).unwrap();
    let unit_rendered = unit_gen.to_string();
    assert!(unit_rendered.contains("serialize_unit_tagged"));
    assert!(!unit_rendered.contains("struct __SerializeHelper"));
    assert!(!unit_rendered.contains("__content :"));
    syn::parse2::<syn::File>(unit_gen).unwrap();

    // Adjacent tagging uses serialize_adjacently_tagged without generating __SerializeHelper
    let adj_input = parse_test_input(quote! {
        tag = "ver",
        content = "data",
        versions = [
            1: ModelV1 => 2,
            2: ModelV2,
        ]
    });
    let adj_gen = generate_for_test(&adj_input, SelfWireMode::Disallowed, None).unwrap();
    let adj_rendered = adj_gen.to_string();
    assert!(adj_rendered.contains("serialize_adjacently_tagged"));
    assert!(adj_rendered.contains("\"data\""));
    assert!(!adj_rendered.contains("struct __SerializeHelper"));
    assert!(!adj_rendered.contains("struct __BcAdjSerHelper"));
    syn::parse2::<syn::File>(adj_gen).unwrap();

    // #[serialize] on older version
    let ser_old_input = parse_test_input(quote! {
        versions = [
            #[serialize]
            1: ModelV1 => 2,
            #[current]
            2: ModelV2,
        ]
    });
    let ser_old_gen = generate_for_test(&ser_old_input, SelfWireMode::Disallowed, None).unwrap();
    let ser_old_rendered = ser_old_gen.to_string();
    assert!(ser_old_rendered.contains("const CURRENT_VERSION : Self :: Version = 2u64 ;"));
    assert!(ser_old_rendered.contains("const SERIALIZE_VERSION : Self :: Version = 1u64 ;"));
    assert!(ser_old_rendered.contains("serialize_internally_tagged"));
    assert!(ser_old_rendered.contains("& 1u64"));
    syn::parse2::<syn::File>(ser_old_gen).unwrap();

    // #[serialize(with = to_v1)] on older version
    let ser_with_input = parse_test_input(quote! {
        versions = [
            #[serialize(with = to_v1)]
            1: ModelV1 => 2,
            #[current]
            2: ModelV2,
        ]
    });
    let ser_with_gen = generate_for_test(&ser_with_input, SelfWireMode::Disallowed, None).unwrap();
    let ser_with_rendered = ser_with_gen.to_string();
    assert!(ser_with_rendered.contains("let latest : ModelV1 = (to_v1) (self) ;"));
    syn::parse2::<syn::File>(ser_with_gen).unwrap();
}

#[test]
fn test_generate_backwards_compat_one_step_with_fn_in_match_arm() {
    let owned_shadow = format_ident!("TargetModelWire");
    let borrowed_shadow = format_ident!("TargetModelBorrowedWire");
    let shadow_info = ShadowWireInfo {
        owned_ident: &owned_shadow,
        borrowed_ident: &borrowed_shadow,
        borrowed_ty_in_helper: quote!(TargetModelBorrowedWire<'__a>),
        construct_borrowed: quote!(TargetModelBorrowedWire {}),
        is_unit_struct: false,
    };

    // Infallible 1-step #[with(convert_v1)]
    let infallible_input = parse_test_input(quote! {
        versions = [
            #[with(convert_v1)]
            1: ModelV1 => 2,
            2: Self,
        ]
    });
    let gen1 = generate_for_test(&infallible_input, SelfWireMode::Shadow(&shadow_info), None).unwrap();
    let rendered1 = gen1.to_string();
    assert!(
        rendered1.contains("fn __bc_migrate_0 (__val : ModelV1) -> TargetModel { (convert_v1) (__val) }")
    );
    assert!(rendered1.contains("let __val : TargetModel = __bc_migrate_0 (__val) ;"));
    syn::parse2::<syn::File>(gen1).unwrap();

    // Fallible 1-step #[fallible] #[with(try_convert_v1)]
    let fallible_input = parse_test_input(quote! {
        versions = [
            #[fallible]
            #[with(try_convert_v1)]
            1: ModelV1 => 2,
            2: Self,
        ]
    });
    let gen2 = generate_for_test(&fallible_input, SelfWireMode::Shadow(&shadow_info), None).unwrap();
    let rendered2 = gen2.to_string();
    assert!(rendered2.contains(
        "fn __bc_migrate_0 (__val : ModelV1) -> :: core :: result :: Result < TargetModel , :: backwards_compat :: __private :: String >"
    ));
    assert!(rendered2.contains("(try_convert_v1) (__val) . map_err"));
    assert!(rendered2.contains(
        "let __val : TargetModel = __bc_migrate_0 (__val) . map_err (:: serde :: de :: Error :: custom) ? ;"
    ));
    syn::parse2::<syn::File>(gen2).unwrap();
}

#[test]
fn test_migration_helpers_tail_share_and_skip_unused_base_body() {
    let input = parse_test_input(quote! {
        versions = [
            _: ModelV1 => 1,
            1: ModelV1 => 3,
            2: ModelV2 => 3,
            3: ModelV3,
        ]
    });
    let dag_plan = resolve_dag(&input).unwrap();
    let de_core = generate_deserialize_core(&input, &dag_plan, SelfWireMode::Disallowed, true).unwrap();

    // Unconditional untagged skips building unused base_body.
    assert!(de_core.base_body.is_empty());
    assert!(de_core.untagged.is_some());

    // Tail-sharing: V1->V3->Target, V2->V3->Target, V3->Target, and untagged V1->V3->Target
    // emit exactly 3 helpers (__bc_migrate_0 for V3, __bc_migrate_1 for V1, __bc_migrate_2 for V2).
    let helpers_str = de_core.helper_defs.to_string();
    assert!(helpers_str.contains("fn __bc_migrate_0 (__val : ModelV3) -> TargetModel"));
    assert!(helpers_str.contains("fn __bc_migrate_1 (__val : ModelV1) -> TargetModel"));
    assert!(helpers_str.contains("fn __bc_migrate_2 (__val : ModelV2) -> TargetModel"));
    assert!(!helpers_str.contains("__bc_migrate_3"));
}

#[test]
fn test_generate_backwards_compat_multiple_untagged_entries() {
    // Mixed gated + unconditional -> first uses ContentRefDeserializer, last uses owned ContentDeserializer
    let mixed_input = parse_test_input(quote! {
        versions = [
            #[cfg(test)]
            _: LegacyA => 1,
            _: LegacyB => 1,
            1: ModelV1,
        ]
    });
    let mixed_gen = generate_for_test(&mixed_input, SelfWireMode::Disallowed, None).unwrap();
    let mixed_rendered = mixed_gen.to_string();
    assert!(!mixed_rendered.contains("__UntaggedHelper"));
    assert!(mixed_rendered.contains(
        "# [cfg (test)] if let :: core :: result :: Result :: Ok (__val) = < LegacyA as :: serde :: Deserialize < '__de >> :: deserialize (:: backwards_compat :: __private :: ContentRefDeserializer :: < __E > :: new (& __content))"
    ));
    assert!(mixed_rendered.contains(
        "if let :: core :: result :: Result :: Ok (__val) = < LegacyB as :: serde :: Deserialize < '__de >> :: deserialize (:: backwards_compat :: __private :: ContentDeserializer :: < __E > :: new (__content))"
    ));
    assert!(mixed_rendered.contains("const ACCEPTS_UNTAGGED : bool = true ;"));
    syn::parse2::<syn::File>(mixed_gen).unwrap();

    // Multiple gated with different predicates -> combined with any(p1, p2) and both use ContentRefDeserializer
    let gated_input = parse_test_input(quote! {
        versions = [
            #[cfg(feature = "a")]
            _: LegacyA => 1,
            #[cfg(feature = "b")]
            _: LegacyB => 1,
            1: ModelV1,
        ]
    });
    let gated_gen = generate_for_test(&gated_input, SelfWireMode::Disallowed, None).unwrap();
    let gated_rendered = gated_gen.to_string();
    assert!(gated_rendered.contains(
        "# [cfg (any (feature = \"a\" , feature = \"b\"))] impl < '__de > :: serde :: Deserialize < '__de > for TargetModel"
    ));
    assert!(gated_rendered.contains(
        "# [cfg (not (any (feature = \"a\" , feature = \"b\")))] impl < '__de > :: serde :: Deserialize < '__de > for TargetModel"
    ));
    assert!(gated_rendered.contains(
        "# [cfg (feature = \"b\")] if let :: core :: result :: Result :: Ok (__val) = < LegacyB as :: serde :: Deserialize < '__de >> :: deserialize (:: backwards_compat :: __private :: ContentRefDeserializer :: < __E > :: new (& __content))"
    ));
    syn::parse2::<syn::File>(gated_gen).unwrap();
}

#[test]
fn test_generate_backwards_compat_explicit_self_uses_shadow_without_duplicate() {
    let input = parse_test_input(quote! {
        versions = [
            1: ModelV1,
            2: Self,
        ]
    });

    let owned_shadow = format_ident!("TargetModelWire");
    let borrowed_shadow = format_ident!("TargetModelBorrowedWire");
    let shadow_info = ShadowWireInfo {
        owned_ident: &owned_shadow,
        borrowed_ident: &borrowed_shadow,
        borrowed_ty_in_helper: quote!(TargetModelBorrowedWire<'__a>),
        construct_borrowed: quote!(TargetModelBorrowedWire {}),
        is_unit_struct: false,
    };

    let generated = generate_for_test(&input, SelfWireMode::Shadow(&shadow_info), None).unwrap();
    let rendered = generated.to_string();

    assert!(
        rendered.contains("< TargetModelWire as :: serde :: Deserialize < '__de >> :: deserialize (__d) ?")
    );
    assert!(!rendered.contains("< TargetModel as :: serde :: Deserialize"));
    syn::parse2::<syn::File>(generated).unwrap();
}

#[test]
fn test_generate_backwards_compat_dump_string_shadow_and_missing_shadow_guards() {
    // dump: true
    let dump_input = parse_test_input(quote! {
        dump,
        versions = [
            1: ModelV1,
            2: ModelV2,
        ]
    });
    let dump_gen = generate_for_test(&dump_input, SelfWireMode::Disallowed, None).unwrap();
    syn::parse2::<syn::File>(dump_gen).unwrap();

    // String current_version with shadow wire
    let str_shadow_input = parse_test_input(quote! {
        versions = [
            "1.0": ModelV1,
            "2.0": Self,
        ]
    });
    let owned_shadow = format_ident!("TargetModelWire");
    let borrowed_shadow = format_ident!("TargetModelBorrowedWire");
    let shadow_info = ShadowWireInfo {
        owned_ident: &owned_shadow,
        borrowed_ident: &borrowed_shadow,
        borrowed_ty_in_helper: quote!(TargetModelBorrowedWire<'__a>),
        construct_borrowed: quote!(TargetModelBorrowedWire {}),
        is_unit_struct: false,
    };
    let str_gen = generate_for_test(&str_shadow_input, SelfWireMode::Shadow(&shadow_info), None).unwrap();
    let str_rendered = str_gen.to_string();
    assert!(str_rendered.contains("const CURRENT_VERSION : Self :: Version = \"2.0\" ;"));
    assert!(
        str_rendered
            .contains("< TargetModelWire as :: serde :: Deserialize < '__de >> :: deserialize (__d) ?")
    );
    syn::parse2::<syn::File>(str_gen).unwrap();

    // Untagged Self without shadow_info -> error
    let untagged_self_input = parse_test_input(quote! {
        versions = [
            _: Self,
            1: ModelV1,
        ]
    });
    let err_untagged = generate_for_test(&untagged_self_input, SelfWireMode::Disallowed, None).unwrap_err();
    assert!(
        err_untagged
            .to_string()
            .contains("using the domain struct as the unversioned wire format")
    );

    // serialize_with_fn + serialize_target_is_wire without shadow_info -> error
    let ser_with_self_input = parse_test_input(quote! {
        versions = [
            #[serialize(with = my_ser)] 1: Self,
            #[current] 2: ModelV2,
        ]
    });
    let err_ser_with = generate_for_test(&ser_with_self_input, SelfWireMode::Disallowed, None).unwrap_err();
    assert!(
        err_ser_with
            .to_string()
            .contains("using the domain struct as the active wire format")
    );

    // Direct serialize_with_fn + serialize_target_is_wire guard (when version arm is non-Self)
    let mut ser_with_guard_input = parse_test_input(quote! {
        versions = [
            1: ModelV1,
            2: ModelV2,
        ]
    });
    ser_with_guard_input.serialize_version = crate::parse::VersionTag::Int(99);
    ser_with_guard_input.serialize_with_fn = Some(syn::parse_quote!(my_ser));
    let err_ser_with_guard =
        generate_for_test(&ser_with_guard_input, SelfWireMode::Disallowed, None).unwrap_err();
    assert!(
        err_ser_with_guard
            .to_string()
            .contains("using the domain struct as the active wire format")
    );

    // Direct serialize_target_is_wire (without serialize_with_fn) guard
    let mut ser_bare_guard_input = parse_test_input(quote! {
        versions = [
            1: ModelV1,
            2: ModelV2,
        ]
    });
    ser_bare_guard_input.serialize_version = crate::parse::VersionTag::Int(99);
    let err_ser_bare_guard =
        generate_for_test(&ser_bare_guard_input, SelfWireMode::Disallowed, None).unwrap_err();
    assert!(
        err_ser_bare_guard
            .to_string()
            .contains("using the domain struct as the active wire format")
    );
}

#[test]
fn test_generate_backwards_compat_tagging_consts_and_version_matching() {
    // Int internal mode
    let int_input = parse_test_input(quote! {
        tag = "v",
        versions = [1: V1, 2: V2]
    });
    let int_gen = generate_for_test(&int_input, SelfWireMode::Disallowed, None).unwrap();
    let int_str = int_gen.to_string();
    assert!(int_str.contains(
        "const TAGGING : :: backwards_compat :: Tagging = :: backwards_compat :: Tagging :: Internal { tag : \"v\" , } ;"
    ));
    assert!(int_str.contains(
        "visit_internal_map :: < :: backwards_compat :: __private :: IntTagMode , Self , __A > (__map , \"v\" ,)"
    ));
    assert!(int_str.contains("match __tag { 1u64 =>"));

    // Str mode with adjacent content
    let str_input = parse_test_input(quote! {
        tag = "ver",
        content = "payload",
        versions = ["a": V1, "b": V2]
    });
    let str_gen = generate_for_test(&str_input, SelfWireMode::Disallowed, None).unwrap();
    let str_str = str_gen.to_string();
    assert!(str_str.contains(
        "const TAGGING : :: backwards_compat :: Tagging = :: backwards_compat :: Tagging :: Adjacent { tag : \"ver\" , content : \"payload\" , } ;"
    ));
    assert!(str_str.contains(
        "visit_adjacent_map :: < :: backwards_compat :: __private :: StrTagMode , Self , __A > (__map , \"ver\" , \"payload\" ,)"
    ));
    assert!(
        str_str.contains(
            "__seq . next_element :: < :: backwards_compat :: __private :: StrTag < '__de > > () ?"
        )
    );
    assert!(str_str.contains("match __tag { \"a\" =>"));

    // External mode
    let ext_input = parse_test_input(quote! {
        external,
        versions = [
            #[serializable] 1 | 10: V1 => 3,
            #[cfg(feature = "v2")] #[serializable] 2: V2 => 3,
            #[current] 3: V3,
        ]
    });
    let ext_gen = generate_for_test(&ext_input, SelfWireMode::Disallowed, None).unwrap();
    let ext_str = ext_gen.to_string();
    assert!(ext_str.contains(
        "const TAGGING : :: backwards_compat :: Tagging = :: backwards_compat :: Tagging :: External ;"
    ));
    assert!(ext_str.contains("serialize_newtype_variant"));
    assert!(ext_str.contains("deserialize_enum"));
    assert!(ext_str.contains("deserialize_identifier"));
    assert!(!ext_str.contains("deserialize_any"));
    assert!(ext_str.contains("const __VARIANTS : & 'static [& 'static str] = & [\"1\" , # [cfg (feature = \"v2\")] \"2\" , \"3\" ,] ;"));
    syn::parse2::<syn::File>(ext_gen).unwrap();
}

#[test]
fn test_generate_backwards_compat_serialize_and_serializable_arms() {
    let owned_shadow = format_ident!("TargetModelWire");
    let borrowed_shadow = format_ident!("TargetModelBorrowedWire");
    let shadow_info = ShadowWireInfo {
        owned_ident: &owned_shadow,
        borrowed_ident: &borrowed_shadow,
        borrowed_ty_in_helper: quote!(TargetModelBorrowedWire<'__a>),
        construct_borrowed: quote!(TargetModelBorrowedWire {}),
        is_unit_struct: false,
    };

    let input = parse_test_input(quote! {
        versions = [
            #[serializable] 1: ModelV1 => 2,
            #[serializable(with = to_v2)] 2: ModelV2 => 3,
            #[serializable(try_with = try_to_v3)] 3: ModelV3 => 4,
            #[current] 4: Self,
        ]
    });
    let generated = generate_for_test(&input, SelfWireMode::Shadow(&shadow_info), None).unwrap();
    let rendered = generated.to_string();

    assert!(rendered.contains("impl :: backwards_compat :: BackwardsCompatSerialize for TargetModel"));
    assert!(rendered.contains(
        "const SERIALIZABLE_VERSIONS : & 'static [Self :: Version] = & [1u64 , 2u64 , 3u64 , 4u64 ,] ;"
    ));
    assert!(rendered.contains(
        "let __wire_val : ModelV1 = __WireConvert :: < _ , ModelV1 > :: new (self) . convert_wire () ;"
    ));
    assert!(rendered.contains("let __wire_val : ModelV2 = (to_v2) (self) ;"));
    assert!(rendered.contains(
        "let __wire_val : ModelV3 = (try_to_v3) (self) . map_err (:: serde :: ser :: Error :: custom) ? ;"
    ));
    assert!(rendered.contains("is not supported for serialization"));
    syn::parse2::<syn::File>(generated).unwrap();
}

#[test]
fn test_generate_backwards_compat_serialize_generic_cfg_arm_trait() {
    let args: BackwardsCompatArgs = syn::parse2(quote! {
        versions = [
            #[cfg(feature = "v1")]
            #[serializable]
            1: GenericV1<T> => 2,
            #[current]
            2: GenericV2<T>,
        ]
    })
    .unwrap();
    let target_ty: syn::Type = syn::parse_quote!(GenericModel<T>);
    let vis: syn::Visibility = syn::parse_quote!(pub);
    let generics: syn::Generics = syn::parse_quote!(<T>);
    let input = args.into_input(target_ty, vis, generics).unwrap();

    let generated = generate_for_test(&input, SelfWireMode::Disallowed, None).unwrap();
    let rendered = generated.to_string();

    assert!(rendered.contains("trait __BcSerArm_0"));
    assert!(rendered.contains("# [cfg (feature = \"v1\")] impl < T > __BcSerArm_0 for GenericModel < T >"));
    assert!(
        rendered
            .contains("# [cfg (not (feature = \"v1\"))] impl < T > __BcSerArm_0 for GenericModel < T > { }")
    );
    assert!(rendered.contains("GenericModel < T > : __BcSerArm_0"));
    syn::parse2::<syn::File>(generated).unwrap();
}

#[cfg(feature = "schemars")]
#[test]
fn test_generate_backwards_compat_schemars_internal_and_adjacent() {
    let internal = parse_test_input(quote! {
        versions = [
            _: LegacyV0 => 1,
            1: ModelV1 => 2,
            #[current] 2: Self,
        ]
    });
    let owned_shadow = format_ident!("TargetModelWire");
    let borrowed_shadow = format_ident!("TargetModelBorrowedWire");
    let shadow_info = ShadowWireInfo {
        owned_ident: &owned_shadow,
        borrowed_ident: &borrowed_shadow,
        borrowed_ty_in_helper: quote!(TargetModelBorrowedWire<'__a>),
        construct_borrowed: quote!(TargetModelBorrowedWire {}),
        is_unit_struct: false,
    };
    let dag_plan = resolve_dag(&internal).unwrap();
    let generated = generate_backwards_compat(
        &internal,
        &dag_plan,
        SelfWireMode::Shadow(&shadow_info),
        None,
        true,
    )
    .unwrap();
    let rendered = generated.to_string();

    assert!(
        rendered.contains("impl :: backwards_compat :: __private :: schemars :: JsonSchema for TargetModel")
    );
    assert!(rendered.contains("schema_internally_tagged"));
    assert!(rendered.contains("TagSchemaValue :: Int (1u64)"));
    assert!(rendered.contains("TagSchemaValue :: Int (2u64)"));
    assert!(rendered.contains("schema_one_of"));
    syn::parse2::<syn::File>(generated).unwrap();

    let adj = parse_test_input(quote! {
        tag = "ver",
        content = "data",
        versions = [
            "v1": ModelV1,
            "v2": ModelV2,
        ]
    });
    let adj_plan = resolve_dag(&adj).unwrap();
    let adj_gen = generate_backwards_compat(&adj, &adj_plan, SelfWireMode::Disallowed, None, true).unwrap();
    let adj_rendered = adj_gen.to_string();
    assert!(adj_rendered.contains("schema_adjacently_tagged"));
    assert!(adj_rendered.contains("TagSchemaValue :: Str (\"v1\")"));
    syn::parse2::<syn::File>(adj_gen).unwrap();

    let ext = parse_test_input(quote! {
        external,
        versions = [
            1: ModelV1,
            2: ModelV2,
        ]
    });
    let ext_plan = resolve_dag(&ext).unwrap();
    let ext_gen = generate_backwards_compat(&ext, &ext_plan, SelfWireMode::Disallowed, None, true).unwrap();
    let ext_rendered = ext_gen.to_string();
    assert!(ext_rendered.contains("schema_externally_tagged"));
    assert!(ext_rendered.contains("\"1\""));
    assert!(ext_rendered.contains("\"2\""));
    syn::parse2::<syn::File>(ext_gen).unwrap();
}

#[test]
fn test_generate_backwards_compat_external_generic_cfg_arm_trait() {
    let args: BackwardsCompatArgs = syn::parse2(quote! {
        external,
        versions = [
            #[cfg(feature = "v1")]
            #[serializable]
            "v1": GenericV1<T> => "v2",
            #[current]
            "v2": GenericV2<T>,
        ]
    })
    .unwrap();
    let target_ty: syn::Type = syn::parse_quote!(GenericModel<T>);
    let vis: syn::Visibility = syn::parse_quote!(pub);
    let generics: syn::Generics = syn::parse_quote!(<T>);
    let input = args.into_input(target_ty, vis, generics).unwrap();

    let generated = generate_for_test(&input, SelfWireMode::Disallowed, None).unwrap();
    let rendered = generated.to_string();

    assert!(rendered.contains("fn __bc_de_variant"));
    assert!(rendered.contains("trait __BcSerArm_0"));
    assert!(!rendered.contains("deserialize_any"));
    syn::parse2::<syn::File>(generated).unwrap();
}
