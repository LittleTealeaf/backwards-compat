use std::collections::HashSet;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::dag::{DagPlan, ResolvedPath, combine_cfg_any, resolve_dag};
use crate::parse::{BackwardsCompatInput, VersionTag};

fn cfg_attr(pred: Option<&TokenStream>) -> TokenStream {
    pred.map_or_else(TokenStream::new, |p| quote!(#[cfg(#p)]))
}

fn generate_step_expr(
    step: &crate::dag::MigrationStep,
    error_ty_tokens: &TokenStream,
    has_custom_error: bool,
) -> TokenStream {
    let to_ty = &step.to_ty;
    match (step.with_fn.as_ref(), step.fallible, has_custom_error) {
        (Some(fn_path), true, true) => quote! {
            let cur: #to_ty = (#fn_path)(cur)?;
        },
        (Some(fn_path), true, false) => quote! {
            let cur: #to_ty = (#fn_path)(cur).map_err(::core::convert::Into::<#error_ty_tokens>::into)?;
        },
        (Some(fn_path), false, _) => quote! {
            let cur: #to_ty = (#fn_path)(cur);
        },
        (None, true, true) => quote! {
            let cur: #to_ty = ::core::convert::TryInto::<#to_ty>::try_into(cur)?;
        },
        (None, true, false) => quote! {
            let cur: #to_ty = ::core::convert::TryInto::<#to_ty>::try_into(cur).map_err(::core::convert::Into::<#error_ty_tokens>::into)?;
        },
        (None, false, _) => quote! {
            let cur: #to_ty = ::core::convert::Into::<#to_ty>::into(cur);
        },
    }
}

pub fn generate_conversions(input: &BackwardsCompatInput, dag_plan: &DagPlan) -> TokenStream {
    let target_ty = &input.target_ty;
    let (impl_generics, _, where_clause) = input.generics.split_for_impl();

    let mut seen_types = HashSet::new();
    let mut impls = Vec::new();

    let error_ty_tokens = input.error_ty.as_ref().map_or_else(
        || {
            quote!(
                ::std::boxed::Box<
                    dyn ::std::error::Error + ::core::marker::Send + ::core::marker::Sync + 'static,
                >
            )
        },
        |err_ty| quote!(#err_ty),
    );

    for (i, v) in input.versions.iter().enumerate() {
        let v_ty = &v.ty;

        if v_ty == target_ty {
            continue;
        }

        let Some(path) = dag_plan.paths.get(i) else {
            continue;
        };
        if path.steps.len() <= 1 {
            continue;
        }

        if !seen_types.insert(v_ty) {
            continue;
        }

        let matching_paths: Vec<_> = input
            .versions
            .iter()
            .enumerate()
            .filter(|(_, cand)| cand.ty == *v_ty)
            .filter_map(|(idx, _)| dag_plan.paths.get(idx))
            .filter(|p| p.steps.len() > 1)
            .collect();

        let path = matching_paths
            .iter()
            .copied()
            .find(|p| p.cfg.is_none())
            .unwrap_or(path);

        let impl_cfg = if matching_paths.iter().any(|p| p.cfg.is_none()) {
            None
        } else {
            combine_cfg_any(matching_paths.iter().filter_map(|p| p.cfg.as_ref()))
        };
        let cfg_tokens = cfg_attr(impl_cfg.as_ref());
        let has_custom_error = input.error_ty.is_some();
        let step_tokens: Vec<_> = path
            .steps
            .iter()
            .map(|step| generate_step_expr(step, &error_ty_tokens, has_custom_error))
            .collect();

        if path.is_fallible() {
            impls.push(quote! {
                #cfg_tokens
                impl #impl_generics ::core::convert::TryFrom<#v_ty> for #target_ty #where_clause {
                    type Error = #error_ty_tokens;

                    fn try_from(val: #v_ty) -> ::core::result::Result<Self, Self::Error> {
                        let cur = val;
                        #(#step_tokens)*
                        ::core::result::Result::Ok(cur)
                    }
                }
            });
        } else {
            impls.push(quote! {
                #cfg_tokens
                impl #impl_generics ::core::convert::From<#v_ty> for #target_ty #where_clause {
                    fn from(val: #v_ty) -> Self {
                        let cur = val;
                        #(#step_tokens)*
                        cur
                    }
                }
            });
        }
    }

    quote! {
        #(#impls)*
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code, reason = "fields used in macro code generation and testing")]
pub struct ShadowWireInfo<'a> {
    pub owned_ident: &'a syn::Ident,
    pub borrowed_ident: &'a syn::Ident,
    pub borrowed_ty_in_helper: TokenStream,
    pub construct_borrowed: TokenStream,
    pub is_unit_struct: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VersionMode {
    Int,
    Str,
    Mixed,
}

impl VersionMode {
    fn from_input(input: &BackwardsCompatInput) -> Self {
        let mut has_int = false;
        let mut has_str = false;
        let mut inspect = |tag: &VersionTag| match tag {
            VersionTag::Int(_) => has_int = true,
            VersionTag::String(_) => has_str = true,
            VersionTag::Untagged => {}
        };
        for v in &input.versions {
            inspect(&v.tag);
        }
        inspect(&input.current_version);
        inspect(&input.serialize_version);

        match (has_int, has_str) {
            (true, true) => Self::Mixed,
            (false, true) => Self::Str,
            _ => Self::Int,
        }
    }

    fn version_ty(self) -> TokenStream {
        match self {
            Self::Int => quote!(u64),
            Self::Str => quote!(&'static str),
            Self::Mixed => quote!(::backwards_compat::VersionKey),
        }
    }

    fn tag_expr(self, tag: &VersionTag) -> TokenStream {
        match (self, tag) {
            (Self::Int, VersionTag::Int(n)) => quote!(#n),
            (Self::Str, VersionTag::String(s)) => quote!(#s),
            (Self::Mixed, VersionTag::Int(n)) => quote!(::backwards_compat::VersionKey::Int(#n)),
            (Self::Mixed, VersionTag::String(s)) => quote!(::backwards_compat::VersionKey::Str(#s)),
            _ => unreachable!("tag_expr called with mismatched or untagged version"),
        }
    }
}

fn tag_display_str(tag: &VersionTag) -> String {
    match tag {
        VersionTag::Int(n) => n.to_string(),
        VersionTag::String(s) => format!("\"{s}\""),
        VersionTag::Untagged => "_".to_owned(),
    }
}

fn generate_arm_conversion(
    ty: &syn::Type,
    target_ty: &syn::Type,
    path: Option<&ResolvedPath>,
) -> TokenStream {
    if ty == target_ty {
        return quote!(::core::result::Result::Ok(::core::convert::Into::into(val)));
    }
    let Some(p) = path else {
        return quote!(::core::result::Result::Ok(val));
    };
    if p.steps.is_empty() {
        return quote!(::core::result::Result::Ok(val));
    }
    if p.steps.len() == 1
        && let Some(first_step) = p.steps.first()
        && let Some(ref fn_path) = first_step.with_fn
    {
        return if first_step.fallible {
            quote!((#fn_path)(val).map_err(::serde::de::Error::custom))
        } else {
            quote!(::core::result::Result::Ok((#fn_path)(val)))
        };
    }
    if p.is_fallible() {
        quote!(::core::convert::TryInto::<#target_ty>::try_into(val).map_err(::serde::de::Error::custom))
    } else {
        quote!(::core::result::Result::Ok(::core::convert::Into::into(val)))
    }
}

pub fn generate_backwards_compat(
    input: &BackwardsCompatInput,
    shadow_info: Option<&ShadowWireInfo<'_>>,
    extra_items: Option<TokenStream>,
) -> syn::Result<TokenStream> {
    let dag_plan = resolve_dag(input)?;

    let target_ty = &input.target_ty;
    let tag_field = &input.tag_field;
    let version_mode = VersionMode::from_input(input);

    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let mut helper_variants = Vec::new();
    let mut match_arms = Vec::new();
    let mut tag_match_arms = Vec::new();
    let mut supported_str_items = Vec::new();
    let mut supported_version_items = Vec::new();
    let mut disabled_checks = Vec::new();
    let mut has_any_gated_tagged = false;

    let has_lifetimes = input.generics.lifetimes().next().is_some();
    let borrow_attr = if has_lifetimes {
        quote!(#[serde(borrow)])
    } else {
        quote!()
    };

    for (i, v) in input.versions.iter().enumerate() {
        if v.tag.is_untagged() {
            continue;
        }
        let Some(path) = dag_plan.paths.get(i) else {
            continue;
        };
        let cfg_tokens = cfg_attr(path.cfg.as_ref());
        let var_ident = format_ident!("__V_{}", i);
        let var_name_str = var_ident.to_string();
        let ty = &v.ty;

        let variant_payload_ty = if ty == target_ty {
            if let Some(info) = shadow_info {
                let owned_ident = info.owned_ident;
                quote!(#owned_ident #ty_generics)
            } else {
                return Err(syn::Error::new_spanned(
                    &input.target_ty,
                    "using the domain struct as the active wire format is only supported when using the #[backwards_compat] attribute macro",
                ));
            }
        } else {
            quote!(#ty)
        };

        helper_variants.push(quote! {
            #cfg_tokens
            #var_ident(#borrow_attr #variant_payload_ty)
        });

        let conv = generate_arm_conversion(ty, target_ty, Some(path));
        match_arms.push(quote! {
            #cfg_tokens
            __VersionHelper::#var_ident(val) => #conv
        });

        let version_expr = version_mode.tag_expr(&v.tag);
        match &v.tag {
            VersionTag::Int(n) => {
                tag_match_arms.push(quote! {
                    #cfg_tokens
                    (::core::option::Option::Some(#n), _) => (#var_name_str, #version_expr)
                });
            }
            VersionTag::String(s) => {
                tag_match_arms.push(quote! {
                    #cfg_tokens
                    (_, ::core::option::Option::Some(#s)) => (#var_name_str, #version_expr)
                });
            }
            VersionTag::Untagged => {}
        }

        let disp = tag_display_str(&v.tag);
        supported_str_items.push(quote! { #cfg_tokens #disp });
        supported_version_items.push(quote! { #cfg_tokens #version_expr });

        if let Some(ref pred) = path.cfg {
            has_any_gated_tagged = true;
            disabled_checks.push(quote! { #[cfg(not(#pred))] () });
        }
    }

    let has_declared_current = input
        .versions
        .iter()
        .any(|v| v.tag.matches(&input.current_version));

    if dag_plan.target_is_wire && !has_declared_current {
        if let Some(info) = shadow_info {
            let current_var_ident = format_ident!("__V_{}", input.versions.len());
            let var_name_str = current_var_ident.to_string();
            let owned_ident = info.owned_ident;
            let version_expr = version_mode.tag_expr(&input.current_version);
            let disp = tag_display_str(&input.current_version);

            helper_variants.push(quote! {
                #current_var_ident(#borrow_attr #owned_ident #ty_generics)
            });

            match_arms.push(quote! {
                __VersionHelper::#current_var_ident(val) => ::core::result::Result::Ok(::core::convert::Into::into(val))
            });

            match &input.current_version {
                VersionTag::Int(n) => {
                    tag_match_arms.push(quote! {
                        (::core::option::Option::Some(#n), _) => (#var_name_str, #version_expr)
                    });
                }
                VersionTag::String(s) => {
                    tag_match_arms.push(quote! {
                        (_, ::core::option::Option::Some(#s)) => (#var_name_str, #version_expr)
                    });
                }
                VersionTag::Untagged => {}
            }

            supported_str_items.push(quote!(#disp));
            supported_version_items.push(quote!(#version_expr));
        } else {
            return Err(syn::Error::new_spanned(
                &input.target_ty,
                "using the domain struct as the active wire format is only supported when using the #[backwards_compat] attribute macro",
            ));
        }
    }

    let untagged_entries: Vec<_> = input
        .versions
        .iter()
        .enumerate()
        .filter(|(_, v)| v.tag.is_untagged())
        .collect();

    let mut untagged_variants = Vec::new();
    let mut untagged_match_arms = Vec::new();
    for &(idx, untagged_v) in &untagged_entries {
        let untagged_ty = &untagged_v.ty;
        let path = dag_plan.paths.get(idx);
        let cfg_tokens = cfg_attr(path.and_then(|p| p.cfg.as_ref()));
        let var_ident = format_ident!("__U_{}", idx);

        let de_ty_tokens = if untagged_ty == target_ty {
            if let Some(info) = shadow_info {
                let owned_ident = info.owned_ident;
                quote!(#owned_ident #ty_generics)
            } else {
                return Err(syn::Error::new_spanned(
                    &input.target_ty,
                    "using the domain struct as the unversioned wire format is only supported when using the #[backwards_compat] attribute macro",
                ));
            }
        } else {
            quote!(#untagged_ty)
        };

        untagged_variants.push(quote! {
            #cfg_tokens
            #var_ident(#borrow_attr #de_ty_tokens)
        });

        let conv = generate_arm_conversion(untagged_ty, target_ty, path);
        untagged_match_arms.push(quote! {
            #cfg_tokens
            __UntaggedHelper::#var_ident(val) => #conv
        });
    }

    let has_untagged = !untagged_entries.is_empty();
    let untagged_overall_cfg = if !has_untagged
        || untagged_entries
            .iter()
            .any(|(idx, _)| dag_plan.paths.get(*idx).and_then(|p| p.cfg.as_ref()).is_none())
    {
        None
    } else {
        combine_cfg_any(
            untagged_entries
                .iter()
                .filter_map(|(idx, _)| dag_plan.paths.get(*idx).and_then(|p| p.cfg.as_ref())),
        )
    };

    let (ser_tag_ty, ser_tag_expr) = match &input.serialize_version {
        VersionTag::Int(n) => (quote!(u64), quote!(#n)),
        VersionTag::String(s) => (quote!(&'static str), quote!(#s)),
        VersionTag::Untagged => unreachable!("serialize_version cannot be untagged"),
    };

    let is_internal_unit_shadow = input.content_field.is_none()
        && dag_plan.serialize_target_is_wire
        && dag_plan.serialize_with_fn.is_none()
        && shadow_info.is_some_and(|i| i.is_unit_struct);

    let (serialize_helper_def, serialize_stmt, ser_where_clause) = if is_internal_unit_shadow {
        let helper_def = quote! {
            #[doc(hidden)]
            #[allow(non_camel_case_types, dead_code)]
            #[derive(::serde::Serialize)]
            struct __SerializeHelper {
                #[serde(rename = #tag_field)]
                __tag: #ser_tag_ty,
            }
        };
        let stmt = quote! {
            let helper = __SerializeHelper { __tag: #ser_tag_expr };
            ::serde::Serialize::serialize(&helper, __serializer)
        };
        let ser_wc = where_clause.cloned();
        (helper_def, stmt, ser_wc)
    } else {
        let (stmt, latest_wire_ty_in_helper) = if let Some(ref ser_fn) = dag_plan.serialize_with_fn {
            let ser_ty = if dag_plan.serialize_target_is_wire {
                let Some(info) = shadow_info else {
                    return Err(syn::Error::new_spanned(
                        &input.target_ty,
                        "using the domain struct as the active wire format is only supported when using the #[backwards_compat] attribute macro",
                    ));
                };
                let owned_ident = info.owned_ident;
                quote!(#owned_ident #ty_generics)
            } else {
                let ty = &dag_plan.serialize_wire_ty;
                quote!(#ty)
            };
            let s = quote! {
                let latest: #ser_ty = (#ser_fn)(self);
                let helper = __SerializeHelper { __tag: #ser_tag_expr, __content: &latest };
                ::serde::Serialize::serialize(&helper, __serializer)
            };
            (s, ser_ty)
        } else if dag_plan.serialize_target_is_wire {
            let Some(info) = shadow_info else {
                return Err(syn::Error::new_spanned(
                    &input.target_ty,
                    "using the domain struct as the active wire format is only supported when using the #[backwards_compat] attribute macro",
                ));
            };
            let construct_borrowed = &info.construct_borrowed;
            let ty_in_helper = &info.borrowed_ty_in_helper;
            let s = quote! {
                let latest = #construct_borrowed;
                let helper = __SerializeHelper { __tag: #ser_tag_expr, __content: &latest };
                ::serde::Serialize::serialize(&helper, __serializer)
            };
            (s, quote!(#ty_in_helper))
        } else {
            let ty = &dag_plan.serialize_wire_ty;
            let s = quote! {
                use ::backwards_compat::__private::{
                    BorrowedWireConvert as _, FallbackWireConvert as _, WireConvert as __WireConvert,
                };

                let latest: #ty = (&__WireConvert(self)).convert_wire();
                let helper = __SerializeHelper { __tag: #ser_tag_expr, __content: &latest };
                ::serde::Serialize::serialize(&helper, __serializer)
            };
            (s, quote!(#ty))
        };

        let mut ser_helper_generics = input.generics.clone();
        let ser_helper_lifetime: syn::GenericParam = syn::parse_quote!('__a);
        ser_helper_generics.params.insert(0, ser_helper_lifetime);
        let (_, ser_helper_ty_generics, _) = ser_helper_generics.split_for_impl();

        let content_field_attr = input.content_field.as_ref().map_or_else(
            || quote!(#[serde(flatten)]),
            |content| quote!(#[serde(rename = #content)]),
        );

        let helper_def = quote! {
            #[doc(hidden)]
            #[allow(non_camel_case_types, dead_code)]
            #[derive(::serde::Serialize)]
            struct __SerializeHelper #ser_helper_generics #where_clause {
                #[serde(rename = #tag_field)]
                __tag: #ser_tag_ty,
                #content_field_attr
                __content: &'__a #latest_wire_ty_in_helper,
            }
        };

        let mut ser_wc = where_clause.cloned().unwrap_or_else(|| syn::WhereClause {
            where_token: syn::token::Where::default(),
            predicates: syn::punctuated::Punctuated::new(),
        });
        ser_wc.predicates.push(syn::parse_quote!(
            for<'__a> __SerializeHelper #ser_helper_ty_generics: ::serde::Serialize
        ));

        (helper_def, stmt, Some(ser_wc))
    };

    let conversions = generate_conversions(input, &dag_plan);
    let extra = extra_items.unwrap_or_default();

    let mut de_generics = input.generics.clone();
    let lifetime_idents: Vec<_> = input.generics.lifetimes().map(|lt| &lt.lifetime).collect();
    let de_lifetime: syn::GenericParam = if lifetime_idents.is_empty() {
        syn::parse_quote!('__de)
    } else {
        syn::parse_quote!('__de: #(#lifetime_idents)+*)
    };
    de_generics.params.insert(0, de_lifetime);
    let (de_impl_generics, _, de_orig_where_clause) = de_generics.split_for_impl();

    let mut de_where_clause_base = de_orig_where_clause.cloned().unwrap_or_else(|| syn::WhereClause {
        where_token: syn::token::Where::default(),
        predicates: syn::punctuated::Punctuated::new(),
    });
    de_where_clause_base.predicates.push(syn::parse_quote!(
        __VersionHelper #ty_generics: ::serde::Deserialize<'__de>
    ));

    let generics = &input.generics;

    let current_int_opt = match input.current_version {
        VersionTag::Int(n) => quote!(::core::option::Option::Some(#n)),
        VersionTag::String(_) | VersionTag::Untagged => quote!(::core::option::Option::None),
    };

    let has_disabled_init = if has_any_gated_tagged {
        quote! {
            let __disabled: &[()] = &[#(#disabled_checks,)*];
            let __has_disabled = !__disabled.is_empty();
        }
    } else {
        quote! {
            let __has_disabled = false;
        }
    };

    let extract_expr = input.content_field.as_ref().map_or_else(
        || quote!(__content.extract_internal_tag(#tag_field)),
        |content_field| quote!(__content.extract_adjacent_tag(#tag_field, #content_field)),
    );

    let missing_content_arm = input.content_field.as_ref().map_or_else(
        || {
            quote! {
                ::backwards_compat::__private::ExtractedPayload::MissingContent { .. } => {
                    ::core::result::Result::Err(::serde::de::Error::missing_field(#tag_field))
                }
            }
        },
        |content_field| {
            quote! {
                ::backwards_compat::__private::ExtractedPayload::MissingContent { .. } => {
                    ::core::result::Result::Err(::serde::de::Error::missing_field(#content_field))
                }
            }
        },
    );

    let tagged_arm = quote! {
        ::backwards_compat::__private::ExtractedPayload::Tagged { tag: __tag, payload: __payload } => {
            let (__variant_name, __source_version) = match (__tag.as_u64(), __tag.as_str()) {
                #(#tag_match_arms,)*
                _ => {
                    let __supported: &[&str] = &[#(#supported_str_items,)*];
                    #has_disabled_init
                    return ::core::result::Result::Err(
                        ::backwards_compat::__private::unknown_version_error(
                            &__tag,
                            #current_int_opt,
                            __supported,
                            __has_disabled,
                        ),
                    );
                }
            };
            let __helper = <__VersionHelper #ty_generics as ::serde::Deserialize>::deserialize(
                ::backwards_compat::__private::VariantContentDeserializer::<__D::Error>::new(
                    __variant_name,
                    __payload,
                ),
            )?;
            let __val = match __helper {
                #(#match_arms,)*
            }?;
            ::core::result::Result::Ok(::backwards_compat::Migrated {
                value: __val,
                source_version: ::core::option::Option::Some(__source_version),
            })
        }
    };

    let base_deserialize_body = quote! {
        let __content = <::backwards_compat::__private::Content<'__de> as ::serde::Deserialize>::deserialize(__deserializer)?;
        match #extract_expr {
            #tagged_arm
            ::backwards_compat::__private::ExtractedPayload::Untagged(_) => {
                ::core::result::Result::Err(::serde::de::Error::missing_field(#tag_field))
            }
            #missing_content_arm
            ::backwards_compat::__private::ExtractedPayload::NotAMap(_) => {
                ::core::result::Result::Err(::serde::de::Error::invalid_type(
                    ::serde::de::Unexpected::Other("non-map"),
                    &"a map",
                ))
            }
        }
    };

    let emit_de_pair = |cfg_prefix: TokenStream, where_cl: &syn::WhereClause, body: &TokenStream| {
        quote! {
            #cfg_prefix
            impl #de_impl_generics ::backwards_compat::BackwardsCompatDeserialize<'__de> for #target_ty #where_cl {
                fn deserialize_migrated<__D>(
                    __deserializer: __D,
                ) -> ::core::result::Result<::backwards_compat::Migrated<Self>, __D::Error>
                where
                    __D: ::serde::Deserializer<'__de>,
                {
                    #body
                }
            }

            #cfg_prefix
            impl #de_impl_generics ::serde::Deserialize<'__de> for #target_ty #where_cl {
                fn deserialize<__D>(__deserializer: __D) -> ::core::result::Result<Self, __D::Error>
                where
                    __D: ::serde::Deserializer<'__de>,
                {
                    <Self as ::backwards_compat::BackwardsCompatDeserialize<'__de>>::deserialize_migrated(__deserializer)
                        .map(|__m| __m.value)
                }
            }
        }
    };

    let (untagged_helper_def, deserialize_impls, accepts_untagged_const) = if has_untagged {
        let untagged_overall_cfg_tokens = cfg_attr(untagged_overall_cfg.as_ref());
        let helper_def = quote! {
            #untagged_overall_cfg_tokens
            #[doc(hidden)]
            #[allow(non_camel_case_types, dead_code)]
            #[derive(::serde::Deserialize)]
            #[serde(untagged)]
            enum __UntaggedHelper #generics #where_clause {
                #(#untagged_variants,)*
            }
        };

        let mut de_where_clause_untagged = de_where_clause_base.clone();
        de_where_clause_untagged.predicates.push(syn::parse_quote!(
            __UntaggedHelper #ty_generics: ::serde::Deserialize<'__de>
        ));

        let untagged_deserialize_body = quote! {
            let __content = <::backwards_compat::__private::Content<'__de> as ::serde::Deserialize>::deserialize(__deserializer)?;
            match #extract_expr {
                #tagged_arm
                #missing_content_arm
                ::backwards_compat::__private::ExtractedPayload::Untagged(__raw)
                | ::backwards_compat::__private::ExtractedPayload::NotAMap(__raw) => {
                    let __helper = <__UntaggedHelper #ty_generics as ::serde::Deserialize>::deserialize(
                        ::backwards_compat::__private::ContentDeserializer::<__D::Error>::new(__raw),
                    )?;
                    let __val = match __helper {
                        #(#untagged_match_arms,)*
                    }?;
                    ::core::result::Result::Ok(::backwards_compat::Migrated {
                        value: __val,
                        source_version: ::core::option::Option::None,
                    })
                }
            }
        };

        let (de_impls, accepts_const) = untagged_overall_cfg.as_ref().map_or_else(
            || {
                (
                    emit_de_pair(
                        TokenStream::new(),
                        &de_where_clause_untagged,
                        &untagged_deserialize_body,
                    ),
                    quote!(
                        const ACCEPTS_UNTAGGED: bool = true;
                    ),
                )
            },
            |pred| {
                let enabled = emit_de_pair(
                    quote!(#[cfg(#pred)]),
                    &de_where_clause_untagged,
                    &untagged_deserialize_body,
                );
                let disabled = emit_de_pair(
                    quote!(#[cfg(not(#pred))]),
                    &de_where_clause_base,
                    &base_deserialize_body,
                );
                (
                    quote! {
                        #enabled
                        #disabled
                    },
                    quote! {
                        #[cfg(#pred)]
                        const ACCEPTS_UNTAGGED: bool = true;
                        #[cfg(not(#pred))]
                        const ACCEPTS_UNTAGGED: bool = false;
                    },
                )
            },
        );

        (helper_def, de_impls, accepts_const)
    } else {
        (
            TokenStream::new(),
            emit_de_pair(TokenStream::new(), &de_where_clause_base, &base_deserialize_body),
            quote!(
                const ACCEPTS_UNTAGGED: bool = false;
            ),
        )
    };

    let version_ty = version_mode.version_ty();
    let current_version_expr = version_mode.tag_expr(&input.current_version);
    let serialize_version_expr = version_mode.tag_expr(&input.serialize_version);

    let code = quote! {
        const _: () = {
            #extra

            #[doc(hidden)]
            #[allow(non_camel_case_types, dead_code)]
            #[derive(::serde::Deserialize)]
            enum __VersionHelper #generics #where_clause {
                #(#helper_variants,)*
            }

            #untagged_helper_def

            #serialize_helper_def

            impl #impl_generics ::backwards_compat::BackwardsCompat for #target_ty #where_clause {
                type Version = #version_ty;
                const CURRENT_VERSION: Self::Version = #current_version_expr;
                const SERIALIZE_VERSION: Self::Version = #serialize_version_expr;
                const SUPPORTED_VERSIONS: &'static [Self::Version] = &[#(#supported_version_items,)*];
                #accepts_untagged_const
            }

            impl #impl_generics ::serde::Serialize for #target_ty #ser_where_clause {
                fn serialize<__S>(&self, __serializer: __S) -> ::core::result::Result<__S::Ok, __S::Error>
                where
                    __S: ::serde::Serializer,
                {
                    #serialize_stmt
                }
            }

            #deserialize_impls

            #conversions
        };
    };

    if input.dump {
        eprintln!(
            "=== backwards_compat output for {} ===\n{}\n======================================",
            quote!(#target_ty),
            code
        );
    }

    Ok(code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::BackwardsCompatArgs;

    fn parse_test_input(args_tokens: proc_macro2::TokenStream) -> BackwardsCompatInput {
        let args: BackwardsCompatArgs = syn::parse2(args_tokens).unwrap();
        let target_ty: syn::Type = syn::parse_quote!(TargetModel);
        let vis: syn::Visibility = syn::parse_quote!(pub);
        args.into_input(target_ty, vis, syn::Generics::default()).unwrap()
    }

    #[test]
    fn test_generate_conversions_infallible() {
        let input = parse_test_input(quote! {
            versions = [
                1: ModelV1 => 2,
                2: ModelV2,
            ]
        });

        let dag_plan = resolve_dag(&input).unwrap();
        let conversions = generate_conversions(&input, &dag_plan);
        let rendered = conversions.to_string();

        assert!(rendered.contains("impl :: core :: convert :: From < ModelV1 > for TargetModel"));
        // ModelV2 is a single step (2 => TargetModel), so it is not generated
        assert!(!rendered.contains("From < ModelV2 >"));
        assert!(!rendered.contains("TryFrom"));
    }

    #[test]
    fn test_generate_conversions_fallible_default_error() {
        let input = parse_test_input(quote! {
            versions = [
                #[fallible]
                1: ModelV1 => 2,
                2: ModelV2,
            ]
        });

        let dag_plan = resolve_dag(&input).unwrap();
        let conversions = generate_conversions(&input, &dag_plan);
        let rendered = conversions.to_string();
        assert!(rendered.contains("impl :: core :: convert :: TryFrom < ModelV1 > for TargetModel"));
        assert!(rendered.contains("type Error = :: std :: boxed :: Box < dyn :: std :: error :: Error + :: core :: marker :: Send + :: core :: marker :: Sync"));
        assert!(rendered.contains("'static"));
        assert!(!rendered.contains("From < ModelV2 >"));
    }

    #[test]
    fn test_generate_conversions_fallible_custom_error() {
        let input = parse_test_input(quote! {
            error = CustomError,
            versions = [
                #[fallible]
                1: ModelV1 => 2,
                2: ModelV2,
            ]
        });

        let dag_plan = resolve_dag(&input).unwrap();
        let conversions = generate_conversions(&input, &dag_plan);
        let rendered = conversions.to_string();

        assert!(rendered.contains("impl :: core :: convert :: TryFrom < ModelV1 > for TargetModel"));
        assert!(rendered.contains("type Error = CustomError"));
        assert!(rendered.contains(":: core :: convert :: TryInto :: < ModelV2 > :: try_into (cur) ?"));
    }

    #[test]
    fn test_generate_conversions_skips_target_and_duplicates() {
        let input = parse_test_input(quote! {
            versions = [
                1: ModelV1 => 2,
                2: ModelV1 => 3,
                3: ModelV2,
            ]
        });

        let dag_plan = resolve_dag(&input).unwrap();
        let conversions = generate_conversions(&input, &dag_plan);
        let rendered = conversions.to_string();

        // ModelV1 should only be implemented once
        let matches: Vec<_> = rendered.match_indices("From < ModelV1 >").collect();
        assert_eq!(matches.len(), 1);

        // ModelV2 (1 step) should not have a From implementation
        assert!(!rendered.contains("From < ModelV2 > for TargetModel"));
    }

    #[test]
    fn test_generate_backwards_compat_match_arms() {
        let input = parse_test_input(quote! {
            versions = [
                1: ModelV1 => 2,
                #[fallible]
                2: ModelV2 => 3,
                3: ModelV3,
            ]
        });

        let generated = generate_backwards_compat(&input, None, None).unwrap();
        let rendered = generated.to_string();

        assert!(rendered.contains("__VersionHelper :: __V_0 (val) => :: core :: convert :: TryInto :: < TargetModel > :: try_into (val) . map_err (:: serde :: de :: Error :: custom)"));
        assert!(rendered.contains("__VersionHelper :: __V_1 (val) => :: core :: convert :: TryInto :: < TargetModel > :: try_into (val) . map_err (:: serde :: de :: Error :: custom)"));
        assert!(rendered.contains("__VersionHelper :: __V_2 (val) => :: core :: result :: Result :: Ok (:: core :: convert :: Into :: into (val))"));
    }

    #[test]
    fn test_serialize_helper_and_version_helper_derives() {
        let input = parse_test_input(quote! {
            versions = [
                1: ModelV1 => 2,
                2: ModelV2 => 3,
                3: ModelV3,
            ]
        });

        let generated = generate_backwards_compat(&input, None, None).unwrap();
        let rendered = generated.to_string();

        // __VersionHelper only derives Deserialize
        assert!(rendered.contains("# [derive (:: serde :: Deserialize)]"));
        // __SerializeHelper derives Serialize as a struct with native tag + flattened ModelV3
        assert!(rendered.contains("# [derive (:: serde :: Serialize)]"));
        assert!(rendered.contains(
            "struct __SerializeHelper < '__a > { # [serde (rename = \"version\")] __tag : u64 , # [serde (flatten)] __content : & '__a ModelV3 , }"
        ));
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
        let generated = generate_backwards_compat(&input, Some(&shadow_info), None).unwrap();
        let rendered = generated.to_string();

        assert!(rendered.contains("__VersionHelper :: __V_2 (val) => :: core :: result :: Result :: Ok (:: core :: convert :: Into :: into (val))"));
        assert!(rendered.contains(
            "struct __SerializeHelper < '__a > { # [serde (rename = \"version\")] __tag : u64 , # [serde (flatten)] __content : & '__a TargetModelBorrowedWire < '__a > , }"
        ));
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

        let result = generate_backwards_compat(&input, None, None);
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

        let generated = generate_backwards_compat(&input, None, None).unwrap();
        let rendered = generated.to_string();

        assert!(
            rendered.contains("enum __VersionHelper < T : Clone + 'static > where T : std :: fmt :: Debug")
        );
        assert!(rendered.contains(
            "struct __SerializeHelper < '__a , T : Clone + 'static > where T : std :: fmt :: Debug"
        ));
        assert!(rendered.contains("impl < T : Clone + 'static > :: serde :: Serialize for GenericModel < T > where T : std :: fmt :: Debug"));
        assert!(rendered.contains("impl < '__de , T : Clone + 'static > :: serde :: Deserialize < '__de > for GenericModel < T > where T : std :: fmt :: Debug"));
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

        let generated = generate_backwards_compat(&input, None, None).unwrap();
        let rendered = generated.to_string();

        assert!(rendered.contains("extract_adjacent_tag (\"type\" , \"data\")"));
        assert!(rendered.contains(
            "struct __SerializeHelper < '__a > { # [serde (rename = \"type\")] __tag : u64 , # [serde (rename = \"data\")] __content : & '__a ModelV2 , }"
        ));
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

        let generated = generate_backwards_compat(&input, None, None).unwrap();
        let rendered = generated.to_string();

        assert!(rendered.contains("use :: backwards_compat :: __private :: { BorrowedWireConvert as _ , FallbackWireConvert as _ , WireConvert as __WireConvert , } ;"));
        assert!(rendered.contains("let latest : ModelV2 = (& __WireConvert (self)) . convert_wire () ;"));
    }

    #[test]
    fn test_generate_conversions_duplicate_types_sequential() {
        let input = parse_test_input(quote! {
            versions = [
                1: ModelV1 => 2,
                2: ModelV1 => 3,
                3: ModelV2,
            ]
        });

        let dag_plan = resolve_dag(&input).unwrap();
        let conversions = generate_conversions(&input, &dag_plan);
        let rendered = conversions.to_string();

        let matches: Vec<_> = rendered
            .match_indices("From < ModelV1 > for TargetModel")
            .collect();
        assert_eq!(matches.len(), 1);
        assert!(!rendered.contains("From < ModelV2 > for TargetModel"));

        let generated = generate_backwards_compat(&input, None, None).unwrap();
        let full_rendered = generated.to_string();

        assert!(full_rendered.contains("__V_0 (ModelV1)"));
        assert!(full_rendered.contains("__V_1 (ModelV1)"));
        assert!(full_rendered.contains("__V_2 (ModelV2)"));
        assert!(full_rendered.contains("__VersionHelper :: __V_0 (val) => :: core :: result :: Result :: Ok (:: core :: convert :: Into :: into (val))"));
        assert!(full_rendered.contains("__VersionHelper :: __V_1 (val) => :: core :: result :: Result :: Ok (:: core :: convert :: Into :: into (val))"));
        assert!(full_rendered.contains("__VersionHelper :: __V_2 (val) => :: core :: result :: Result :: Ok (:: core :: convert :: Into :: into (val))"));
    }

    #[test]
    fn test_generate_conversions_duplicate_types_alternating() {
        let input = parse_test_input(quote! {
            versions = [
                1: ModelV1,
                2: ModelV2,
                3: ModelV1,
            ]
        });

        let dag_plan = resolve_dag(&input).unwrap();
        let conversions = generate_conversions(&input, &dag_plan);
        let rendered = conversions.to_string();

        assert!(rendered.contains("impl :: core :: convert :: From < ModelV2 > for TargetModel"));
        assert!(!rendered.contains("From < ModelV1 > for TargetModel"));

        let generated = generate_backwards_compat(&input, None, None).unwrap();
        let full_rendered = generated.to_string();

        assert!(full_rendered.contains("__V_0 (ModelV1)"));
        assert!(full_rendered.contains("__V_1 (ModelV2)"));
        assert!(full_rendered.contains("__V_2 (ModelV1)"));
        assert!(full_rendered.contains("__VersionHelper :: __V_0 (val) => :: core :: result :: Result :: Ok (:: core :: convert :: Into :: into (val))"));
        assert!(full_rendered.contains("__VersionHelper :: __V_1 (val) => :: core :: result :: Result :: Ok (:: core :: convert :: Into :: into (val))"));
        assert!(full_rendered.contains("__VersionHelper :: __V_2 (val) => :: core :: result :: Result :: Ok (:: core :: convert :: Into :: into (val))"));
    }

    #[test]
    fn test_generate_conversions_single_gated_infallible_and_fallible() {
        let infallible_input = parse_test_input(quote! {
            versions = [
                #[cfg(feature = "v1")]
                1: ModelV1 => 2,
                2: ModelV2,
            ]
        });
        let dag_plan = resolve_dag(&infallible_input).unwrap();
        let rendered = generate_conversions(&infallible_input, &dag_plan).to_string();
        assert!(rendered.contains(
            "# [cfg (feature = \"v1\")] impl :: core :: convert :: From < ModelV1 > for TargetModel"
        ));
        assert!(!rendered.contains("From < ModelV2 > for TargetModel"));

        let fallible_input = parse_test_input(quote! {
            versions = [
                #[cfg(feature = "v1")]
                #[fallible]
                1: ModelV1 => 2,
                2: ModelV2,
            ]
        });
        let dag_plan_fallible = resolve_dag(&fallible_input).unwrap();
        let rendered_fallible = generate_conversions(&fallible_input, &dag_plan_fallible).to_string();
        assert!(rendered_fallible.contains(
            "# [cfg (feature = \"v1\")] impl :: core :: convert :: TryFrom < ModelV1 > for TargetModel"
        ));
    }

    #[test]
    fn test_generate_conversions_chained_gated_different_cfgs() {
        let input = parse_test_input(quote! {
            versions = [
                #[cfg(feature = "v1")]
                1: ModelV1 => 2,
                #[cfg(feature = "v2")]
                2: ModelV2 => 3,
                3: ModelV3,
            ]
        });
        let dag_plan = resolve_dag(&input).unwrap();
        let rendered = generate_conversions(&input, &dag_plan).to_string();

        assert!(rendered.contains(
            "# [cfg (all (feature = \"v1\" , feature = \"v2\"))] impl :: core :: convert :: From < ModelV1 > for TargetModel"
        ));
        assert!(rendered.contains(
            "# [cfg (feature = \"v2\")] impl :: core :: convert :: From < ModelV2 > for TargetModel"
        ));
    }

    #[test]
    fn test_generate_conversions_duplicate_type_two_different_cfgs_uses_any() {
        let input = parse_test_input(quote! {
            versions = [
                #[cfg(feature = "a")]
                1: ModelV1 => 3,
                #[cfg(feature = "b")]
                2: ModelV1 => 3,
                3: ModelV2,
            ]
        });
        let dag_plan = resolve_dag(&input).unwrap();
        let rendered = generate_conversions(&input, &dag_plan).to_string();

        let matches: Vec<_> = rendered
            .match_indices("From < ModelV1 > for TargetModel")
            .collect();
        assert_eq!(matches.len(), 1);
        assert!(rendered.contains(
            "# [cfg (any (feature = \"a\" , feature = \"b\"))] impl :: core :: convert :: From < ModelV1 > for TargetModel"
        ));
    }

    #[test]
    fn test_generate_conversions_duplicate_type_gated_and_unconditional_both_orders() {
        // Gated first, unconditional second
        let gated_first = parse_test_input(quote! {
            versions = [
                #[cfg(feature = "a")]
                #[fallible]
                1: ModelV1 => 2,
                #[cfg(feature = "a")]
                2: ModelV2 => 4,
                3: ModelV1 => 4,
                4: ModelV3,
            ]
        });
        let dag_plan1 = resolve_dag(&gated_first).unwrap();
        let rendered1 = generate_conversions(&gated_first, &dag_plan1).to_string();

        let matches1: Vec<_> = rendered1
            .match_indices("From < ModelV1 > for TargetModel")
            .collect();
        assert_eq!(matches1.len(), 1);
        assert!(!rendered1.contains("TryFrom < ModelV1 >"));
        assert!(!rendered1.contains("# [cfg (feature = \"a\")] impl :: core :: convert :: From < ModelV1 >"));
        assert!(rendered1.contains("impl :: core :: convert :: From < ModelV1 > for TargetModel"));
        // ModelV2 is still gated
        assert!(rendered1.contains(
            "# [cfg (feature = \"a\")] impl :: core :: convert :: From < ModelV2 > for TargetModel"
        ));

        // Unconditional first, gated second
        let unconditional_first = parse_test_input(quote! {
            versions = [
                1: ModelV1 => 3,
                #[cfg(feature = "legacy")]
                2: ModelV1 => 3,
                3: ModelV2,
            ]
        });
        let dag_plan2 = resolve_dag(&unconditional_first).unwrap();
        let rendered2 = generate_conversions(&unconditional_first, &dag_plan2).to_string();

        let matches2: Vec<_> = rendered2
            .match_indices("From < ModelV1 > for TargetModel")
            .collect();
        assert_eq!(matches2.len(), 1);
        assert!(!rendered2.contains("# [cfg"));
        assert!(rendered2.contains("impl :: core :: convert :: From < ModelV1 > for TargetModel"));
    }

    #[test]
    fn test_generate_backwards_compat_cfg_on_helper_variants_and_match_arms() {
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

        let generated = generate_backwards_compat(&input, None, None).unwrap();
        let rendered = generated.to_string();

        // __VersionHelper variants
        assert!(rendered.contains("# [cfg (all (feature = \"v1\" , feature = \"v2\"))] __V_0 (ModelV1)"));
        assert!(rendered.contains("# [cfg (feature = \"v2\")] __V_1 (ModelV2)"));
        assert!(rendered.contains(", __V_2 (ModelV3)"));

        // match helper arms
        assert!(rendered.contains(
            "# [cfg (all (feature = \"v1\" , feature = \"v2\"))] __VersionHelper :: __V_0 (val) => :: core :: convert :: TryInto :: < TargetModel > :: try_into (val) . map_err (:: serde :: de :: Error :: custom)"
        ));
        assert!(rendered.contains(
            "# [cfg (feature = \"v2\")] __VersionHelper :: __V_1 (val) => :: core :: convert :: TryInto :: < TargetModel > :: try_into (val) . map_err (:: serde :: de :: Error :: custom)"
        ));
        assert!(rendered.contains(
            ", __VersionHelper :: __V_2 (val) => :: core :: result :: Result :: Ok (:: core :: convert :: Into :: into (val))"
        ));

        // Generated code parses as valid Rust syntax
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

        let generated = generate_backwards_compat(&input, None, None).unwrap();
        let rendered = generated.to_string();

        assert!(rendered.contains("# [cfg (test)] # [doc (hidden)] # [allow (non_camel_case_types , dead_code)] # [derive (:: serde :: Deserialize)] # [serde (untagged)] enum __UntaggedHelper"));
        assert!(rendered.contains(
            "# [cfg (test)] impl < '__de > :: serde :: Deserialize < '__de > for TargetModel where __VersionHelper : :: serde :: Deserialize < '__de > , __UntaggedHelper : :: serde :: Deserialize < '__de > {"
        ));
        assert!(rendered.contains(
            "# [cfg (not (test))] impl < '__de > :: serde :: Deserialize < '__de > for TargetModel where __VersionHelper : :: serde :: Deserialize < '__de > {"
        ));
        assert!(rendered.contains("# [cfg (test)] const ACCEPTS_UNTAGGED : bool = true ;"));
        assert!(rendered.contains("# [cfg (not (test))] const ACCEPTS_UNTAGGED : bool = false ;"));

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

        let generated = generate_backwards_compat(&input, Some(&shadow_info), None).unwrap();
        let rendered = generated.to_string();

        assert!(rendered.contains("__U_0 (TargetModelWire)"));
        assert!(rendered.contains(
            "# [cfg (test)] impl < '__de > :: serde :: Deserialize < '__de > for TargetModel where __VersionHelper : :: serde :: Deserialize < '__de > , __UntaggedHelper : :: serde :: Deserialize < '__de > {"
        ));
        assert!(rendered.contains(
            "# [cfg (not (test))] impl < '__de > :: serde :: Deserialize < '__de > for TargetModel where __VersionHelper : :: serde :: Deserialize < '__de > {"
        ));

        syn::parse2::<syn::File>(generated).unwrap();
    }

    #[test]
    fn test_generate_conversions_with_fn_infallible() {
        let input = parse_test_input(quote! {
            versions = [
                #[with(my_mod::v1_to_v2)]
                1: ModelV1 => 2,
                2: ModelV2,
            ]
        });

        let dag_plan = resolve_dag(&input).unwrap();
        let conversions = generate_conversions(&input, &dag_plan);
        let rendered = conversions.to_string();

        assert!(rendered.contains("impl :: core :: convert :: From < ModelV1 > for TargetModel"));
        assert!(rendered.contains("let cur : ModelV2 = (my_mod :: v1_to_v2) (cur) ;"));
        assert!(rendered.contains(
            "let cur : TargetModel = :: core :: convert :: Into :: < TargetModel > :: into (cur) ;"
        ));
        syn::parse2::<syn::File>(conversions).unwrap();
    }

    #[test]
    fn test_generate_conversions_with_fn_fallible_default_error() {
        let input = parse_test_input(quote! {
            versions = [
                #[fallible]
                #[with(parse_v2)]
                1: ModelV1 => 2,
                2: ModelV2,
            ]
        });

        let dag_plan = resolve_dag(&input).unwrap();
        let conversions = generate_conversions(&input, &dag_plan);
        let rendered = conversions.to_string();

        assert!(rendered.contains("impl :: core :: convert :: TryFrom < ModelV1 > for TargetModel"));
        assert!(rendered.contains("let cur : ModelV2 = (parse_v2) (cur) . map_err (:: core :: convert :: Into :: < :: std :: boxed :: Box < dyn :: std :: error :: Error + :: core :: marker :: Send + :: core :: marker :: Sync + 'static , > > :: into) ? ;"));
        syn::parse2::<syn::File>(conversions).unwrap();
    }

    #[test]
    fn test_generate_conversions_with_fn_fallible_custom_error() {
        let input = parse_test_input(quote! {
            error = CustomError,
            versions = [
                #[fallible]
                #[with(parse_v2)]
                1: ModelV1 => 2,
                2: ModelV2,
            ]
        });

        let dag_plan = resolve_dag(&input).unwrap();
        let conversions = generate_conversions(&input, &dag_plan);
        let rendered = conversions.to_string();

        assert!(rendered.contains("impl :: core :: convert :: TryFrom < ModelV1 > for TargetModel"));
        assert!(rendered.contains("type Error = CustomError"));
        assert!(rendered.contains("let cur : ModelV2 = (parse_v2) (cur) ? ;"));
        syn::parse2::<syn::File>(conversions).unwrap();
    }

    #[test]
    fn test_generate_conversions_skips_one_step_with_fn() {
        let input = parse_test_input(quote! {
            versions = [
                #[with(convert_v2)]
                2: ModelV2,
            ]
        });

        let dag_plan = resolve_dag(&input).unwrap();
        let conversions = generate_conversions(&input, &dag_plan);
        assert!(conversions.is_empty());
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
        let int_gen = generate_backwards_compat(&int_input, None, None).unwrap();
        let int_rendered = int_gen.to_string();
        assert!(int_rendered.contains("type Version = u64 ;"));
        assert!(int_rendered.contains("const CURRENT_VERSION : Self :: Version = 2u64 ;"));
        assert!(int_rendered.contains("const SERIALIZE_VERSION : Self :: Version = 2u64 ;"));
        assert!(
            int_rendered
                .contains("const SUPPORTED_VERSIONS : & 'static [Self :: Version] = & [1u64 , 2u64 ,] ;")
        );
        syn::parse2::<syn::File>(int_gen).unwrap();

        // All-str -> &'static str
        let str_input = parse_test_input(quote! {
            versions = [
                "v1": ModelV1 => "v2",
                "v2": ModelV2,
            ]
        });
        let str_gen = generate_backwards_compat(&str_input, None, None).unwrap();
        let str_rendered = str_gen.to_string();
        assert!(str_rendered.contains("type Version = & 'static str ;"));
        assert!(str_rendered.contains("const CURRENT_VERSION : Self :: Version = \"v2\" ;"));
        assert!(str_rendered.contains("const SERIALIZE_VERSION : Self :: Version = \"v2\" ;"));
        syn::parse2::<syn::File>(str_gen).unwrap();

        // Mixed int + str -> VersionKey
        let mixed_input = parse_test_input(quote! {
            versions = [
                1: ModelV1 => "v2",
                "v2": ModelV2,
            ]
        });
        let mixed_gen = generate_backwards_compat(&mixed_input, None, None).unwrap();
        let mixed_rendered = mixed_gen.to_string();
        assert!(mixed_rendered.contains("type Version = :: backwards_compat :: VersionKey ;"));
        assert!(mixed_rendered.contains(
            "const CURRENT_VERSION : Self :: Version = :: backwards_compat :: VersionKey :: Str (\"v2\") ;"
        ));
        assert!(mixed_rendered.contains(":: backwards_compat :: VersionKey :: Int (1u64)"));
        syn::parse2::<syn::File>(mixed_gen).unwrap();
    }

    #[test]
    fn test_generate_backwards_compat_serialize_helper_variants() {
        // Internal unit struct (no __content, no '__a)
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
        let unit_gen = generate_backwards_compat(&unit_input, Some(&unit_shadow), None).unwrap();
        let unit_rendered = unit_gen.to_string();
        assert!(
            unit_rendered
                .contains("struct __SerializeHelper { # [serde (rename = \"version\")] __tag : u64 , }")
        );
        assert!(!unit_rendered.contains("__content :"));
        syn::parse2::<syn::File>(unit_gen).unwrap();

        // Adjacent tagging
        let adj_input = parse_test_input(quote! {
            tag = "ver",
            content = "data",
            versions = [
                1: ModelV1 => 2,
                2: ModelV2,
            ]
        });
        let adj_gen = generate_backwards_compat(&adj_input, None, None).unwrap();
        let adj_rendered = adj_gen.to_string();
        assert!(adj_rendered.contains("# [serde (rename = \"data\")] __content : & '__a ModelV2"));
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
        let ser_old_gen = generate_backwards_compat(&ser_old_input, None, None).unwrap();
        let ser_old_rendered = ser_old_gen.to_string();
        assert!(ser_old_rendered.contains("const CURRENT_VERSION : Self :: Version = 2u64 ;"));
        assert!(ser_old_rendered.contains("const SERIALIZE_VERSION : Self :: Version = 1u64 ;"));
        assert!(ser_old_rendered.contains("# [serde (flatten)] __content : & '__a ModelV1"));
        assert!(ser_old_rendered.contains("__tag : 1u64"));
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
        let ser_with_gen = generate_backwards_compat(&ser_with_input, None, None).unwrap();
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
        let gen1 = generate_backwards_compat(&infallible_input, Some(&shadow_info), None).unwrap();
        let rendered1 = gen1.to_string();
        assert!(rendered1.contains(
            "__VersionHelper :: __V_0 (val) => :: core :: result :: Result :: Ok ((convert_v1) (val))"
        ));
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
        let gen2 = generate_backwards_compat(&fallible_input, Some(&shadow_info), None).unwrap();
        let rendered2 = gen2.to_string();
        assert!(rendered2.contains(
            "__VersionHelper :: __V_0 (val) => (try_convert_v1) (val) . map_err (:: serde :: de :: Error :: custom)"
        ));
        syn::parse2::<syn::File>(gen2).unwrap();
    }

    #[test]
    fn test_generate_backwards_compat_multiple_untagged_entries() {
        // Mixed gated + unconditional
        let mixed_input = parse_test_input(quote! {
            versions = [
                #[cfg(test)]
                _: LegacyA => 1,
                _: LegacyB => 1,
                1: ModelV1,
            ]
        });
        let mixed_gen = generate_backwards_compat(&mixed_input, None, None).unwrap();
        let mixed_rendered = mixed_gen.to_string();
        assert!(mixed_rendered.contains("# [cfg (test)] __U_0 (LegacyA)"));
        assert!(mixed_rendered.contains(", __U_1 (LegacyB)"));
        assert!(mixed_rendered.contains("const ACCEPTS_UNTAGGED : bool = true ;"));
        syn::parse2::<syn::File>(mixed_gen).unwrap();

        // Multiple gated with different predicates -> combined with any(p1, p2)
        let gated_input = parse_test_input(quote! {
            versions = [
                #[cfg(feature = "a")]
                _: LegacyA => 1,
                #[cfg(feature = "b")]
                _: LegacyB => 1,
                1: ModelV1,
            ]
        });
        let gated_gen = generate_backwards_compat(&gated_input, None, None).unwrap();
        let gated_rendered = gated_gen.to_string();
        assert!(gated_rendered.contains(
            "# [cfg (any (feature = \"a\" , feature = \"b\"))] # [doc (hidden)] # [allow (non_camel_case_types , dead_code)] # [derive (:: serde :: Deserialize)] # [serde (untagged)] enum __UntaggedHelper"
        ));
        assert!(gated_rendered.contains(
            "# [cfg (not (any (feature = \"a\" , feature = \"b\")))] impl < '__de > :: serde :: Deserialize < '__de > for TargetModel"
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

        let generated = generate_backwards_compat(&input, Some(&shadow_info), None).unwrap();
        let rendered = generated.to_string();

        assert!(rendered.contains("__V_1 (TargetModelWire)"));
        assert!(!rendered.contains("__V_1 (TargetModel)"));
        assert!(!rendered.contains("__V_2"));
        syn::parse2::<syn::File>(generated).unwrap();
    }
}
