use std::collections::HashSet;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use super::{SelfWireMode, cfg_attr, non_empty_where_clause, resolve_de_ty, target_type_name};
use crate::dag::DagPlan;
use crate::parse::{BackwardsCompatInput, VersionTag};

#[allow(
    clippy::too_many_arguments,
    reason = "shared JSON Schema generation across container and field-level helpers"
)]
pub fn generate_json_schema_body(
    input: &BackwardsCompatInput,
    dag_plan: &DagPlan,
    wire_mode: SelfWireMode<'_>,
    impl_self_ty: &syn::Type,
    impl_generics: &syn::ImplGenerics<'_>,
    orig_where_clause: Option<&syn::WhereClause>,
    schema_wc: &mut syn::WhereClause,
    helper_defs_list: &mut Vec<TokenStream>,
) -> syn::Result<TokenStream> {
    let target_ty = &input.target_ty;
    let tag_field = &input.tag_field;
    let content_field_opt = input.content_field.as_ref();
    let is_generic = input.generics.type_params().next().is_some();
    let (_, ty_generics, _) = input.generics.split_for_impl();

    let mut seen_schema_tys: HashSet<String> = HashSet::new();
    let mut push_stmts = Vec::new();

    let mut push_arm =
        |idx: usize, de_ty: TokenStream, cfg: Option<&TokenStream>, schema_expr: TokenStream| {
            let cfg_tokens = cfg_attr(cfg);
            if is_generic && let Some(pred) = cfg {
                let trait_ident = format_ident!("__BcSchemaArm_{}", idx);
                let mut arm_wc = orig_where_clause.cloned().unwrap_or_else(|| syn::WhereClause {
                    where_token: syn::token::Where::default(),
                    predicates: syn::punctuated::Punctuated::new(),
                });
                arm_wc.predicates.push(syn::parse_quote!(
                    #de_ty: ::backwards_compat::__private::schemars::JsonSchema
                ));
                helper_defs_list.push(quote! {
                    #[doc(hidden)]
                    #[allow(non_camel_case_types, dead_code, reason = "internal cfg-gated schemars helper")]
                    trait #trait_ident {
                        #[cfg(#pred)]
                        fn __bc_schema(
                            __gen: &mut ::backwards_compat::__private::schemars::SchemaGenerator,
                        ) -> ::backwards_compat::__private::schemars::Schema;
                    }

                    #[cfg(#pred)]
                    impl #impl_generics #trait_ident for #impl_self_ty #arm_wc {
                        fn __bc_schema(
                            __gen: &mut ::backwards_compat::__private::schemars::SchemaGenerator,
                        ) -> ::backwards_compat::__private::schemars::Schema {
                            #schema_expr
                        }
                    }

                    #[cfg(not(#pred))]
                    impl #impl_generics #trait_ident for #impl_self_ty #orig_where_clause {}
                });
                schema_wc
                    .predicates
                    .push(syn::parse_quote!(#impl_self_ty: #trait_ident));
                push_stmts.push(quote! {
                    #cfg_tokens
                    __schemas.push(<Self as #trait_ident>::__bc_schema(__gen));
                });
            } else {
                if is_generic && cfg.is_none() {
                    let key = de_ty.to_string();
                    if seen_schema_tys.insert(key) {
                        schema_wc.predicates.push(syn::parse_quote!(
                            #de_ty: ::backwards_compat::__private::schemars::JsonSchema
                        ));
                    }
                }
                push_stmts.push(quote! {
                    #cfg_tokens
                    __schemas.push(#schema_expr);
                });
            }
        };

    let build_tagged_expr = |de_ty: &TokenStream, tag: &VersionTag| -> TokenStream {
        if input.external {
            let variant_name = tag.to_tag_string();
            return quote! {
                ::backwards_compat::__private::schema_externally_tagged(
                    __gen.subschema_for::<#de_ty>(),
                    #variant_name,
                )
            };
        }
        let tag_schema_val = match tag {
            VersionTag::Int(n) => quote!(::backwards_compat::__private::TagSchemaValue::Int(#n)),
            VersionTag::String(s) => quote!(::backwards_compat::__private::TagSchemaValue::Str(#s)),
            VersionTag::Untagged => unreachable!("tagged arm cannot be untagged"),
        };
        content_field_opt.map_or_else(
            || {
                quote! {
                    ::backwards_compat::__private::schema_internally_tagged(
                        <#de_ty as ::backwards_compat::__private::schemars::JsonSchema>::json_schema(__gen),
                        #tag_field,
                        #tag_schema_val,
                    )
                }
            },
            |content_field| {
                quote! {
                    ::backwards_compat::__private::schema_adjacently_tagged(
                        __gen.subschema_for::<#de_ty>(),
                        #tag_field,
                        #content_field,
                        #tag_schema_val,
                    )
                }
            },
        )
    };

    for (i, v) in input.versions.iter().enumerate() {
        if v.tag.is_untagged() {
            continue;
        }
        let Some(path) = dag_plan.paths.get(i) else {
            continue;
        };
        let de_ty = resolve_de_ty(
            &v.ty,
            target_ty,
            &ty_generics,
            wire_mode,
            "using the domain struct as the active wire format is only supported when using the #[backwards_compat] attribute macro",
        )?;
        let expr = build_tagged_expr(&de_ty, &v.tag);
        push_arm(i, de_ty, path.cfg.as_ref(), expr);
    }

    let has_declared_current = input
        .versions
        .iter()
        .any(|v| v.tag.matches(&input.current_version));
    if dag_plan.target_is_wire && !has_declared_current {
        let de_ty = resolve_de_ty(
            target_ty,
            target_ty,
            &ty_generics,
            wire_mode,
            "using the domain struct as the active wire format is only supported when using the #[backwards_compat] attribute macro",
        )?;
        let expr = build_tagged_expr(&de_ty, &input.current_version);
        push_arm(input.versions.len(), de_ty, None, expr);
    }

    for (idx, untagged_v) in input.versions.iter().enumerate() {
        if !untagged_v.tag.is_untagged() {
            continue;
        }
        let path = dag_plan.paths.get(idx);
        let cfg = path.and_then(|p| p.cfg.as_ref());
        let de_ty = resolve_de_ty(
            &untagged_v.ty,
            target_ty,
            &ty_generics,
            wire_mode,
            "using the domain struct as the unversioned wire format is only supported when using the #[backwards_compat] attribute macro",
        )?;
        let expr = quote!(__gen.subschema_for::<#de_ty>());
        push_arm(idx, de_ty, cfg, expr);
    }

    Ok(quote! {
        let mut __schemas = ::backwards_compat::__private::Vec::new();
        #(#push_stmts)*
        ::backwards_compat::__private::schema_one_of(__schemas)
    })
}

pub fn build_schema_name_and_id(
    base_name: &str,
    generics: &syn::Generics,
    schema_wc: &mut syn::WhereClause,
) -> (TokenStream, TokenStream) {
    let mut name_fmt_parts = Vec::new();
    let mut name_args = Vec::new();
    let mut id_fmt_parts = Vec::new();
    let mut id_args = Vec::new();

    for param in &generics.params {
        match param {
            syn::GenericParam::Type(tp) => {
                let ident = &tp.ident;
                schema_wc.predicates.push(syn::parse_quote!(
                    #ident: ::backwards_compat::__private::schemars::JsonSchema
                ));
                name_fmt_parts.push("{}");
                name_args.push(quote!(
                    <#ident as ::backwards_compat::__private::schemars::JsonSchema>::schema_name()
                ));
                id_fmt_parts.push("{}");
                id_args.push(quote!(
                    <#ident as ::backwards_compat::__private::schemars::JsonSchema>::schema_id()
                ));
            }
            syn::GenericParam::Const(cp) => {
                let ident = &cp.ident;
                name_fmt_parts.push("{}");
                name_args.push(quote!(#ident));
                id_fmt_parts.push("{}");
                id_args.push(quote!(#ident));
            }
            syn::GenericParam::Lifetime(_) => {}
        }
    }

    if name_args.is_empty() {
        (
            quote! {
                ::backwards_compat::__private::schemars::_private::alloc::borrow::Cow::Borrowed(#base_name)
            },
            quote! {
                ::backwards_compat::__private::schemars::_private::alloc::borrow::Cow::Borrowed(
                    ::core::concat!(::core::module_path!(), "::", #base_name),
                )
            },
        )
    } else {
        let name_fmt = format!("{}_{}", base_name, name_fmt_parts.join("_"));
        let id_fmt = format!("{{}}::{}<{}>", base_name, id_fmt_parts.join(", "));
        (
            quote! {
                ::backwards_compat::__private::schemars::_private::alloc::borrow::Cow::Owned(
                    ::backwards_compat::__private::format!(#name_fmt, #(#name_args),*),
                )
            },
            quote! {
                ::backwards_compat::__private::schemars::_private::alloc::borrow::Cow::Owned(
                    ::backwards_compat::__private::format!(
                        #id_fmt,
                        ::core::module_path!(),
                        #(#id_args),*
                    ),
                )
            },
        )
    }
}

pub(super) fn generate_json_schema_impl(
    input: &BackwardsCompatInput,
    dag_plan: &DagPlan,
    wire_mode: SelfWireMode<'_>,
    helper_defs_list: &mut Vec<TokenStream>,
) -> syn::Result<TokenStream> {
    let target_ty = &input.target_ty;
    let target_name = target_type_name(target_ty);
    let (impl_generics, _, where_clause) = input.generics.split_for_impl();

    let mut schema_wc = where_clause.cloned().unwrap_or_else(|| syn::WhereClause {
        where_token: syn::token::Where::default(),
        predicates: syn::punctuated::Punctuated::new(),
    });

    let (schema_name_body, schema_id_body) =
        build_schema_name_and_id(&target_name, &input.generics, &mut schema_wc);
    let json_schema_body = generate_json_schema_body(
        input,
        dag_plan,
        wire_mode,
        target_ty,
        &impl_generics,
        where_clause,
        &mut schema_wc,
        helper_defs_list,
    )?;

    let schema_where_clause = non_empty_where_clause(schema_wc);

    Ok(quote! {
        impl #impl_generics ::backwards_compat::__private::schemars::JsonSchema
            for #target_ty #schema_where_clause
        {
            fn schema_name() -> ::backwards_compat::__private::schemars::_private::alloc::borrow::Cow<'static, str> {
                #schema_name_body
            }

            fn schema_id() -> ::backwards_compat::__private::schemars::_private::alloc::borrow::Cow<'static, str> {
                #schema_id_body
            }

            fn json_schema(
                __gen: &mut ::backwards_compat::__private::schemars::SchemaGenerator,
            ) -> ::backwards_compat::__private::schemars::Schema {
                #json_schema_body
            }
        }
    })
}
