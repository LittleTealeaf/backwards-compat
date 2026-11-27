use std::collections::HashSet;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use super::{
    SelfWireMode, VersionMode, cfg_attr, collect_external_variants, emit_ext_variant_indices,
    ext_variant_lookup, non_empty_where_clause, target_type_name,
};
use crate::dag::{DagPlan, combine_cfg_all};
use crate::parse::{BackwardsCompatInput, SerializeConvertMode, VersionTag};

pub struct SerializeCore {
    pub helper_def: TokenStream,
    pub body: TokenStream,
    pub where_clause: Option<syn::WhereClause>,
}

pub fn generate_serialize_core(
    input: &BackwardsCompatInput,
    dag_plan: &DagPlan,
    wire_mode: SelfWireMode<'_>,
    value_expr: &TokenStream,
) -> syn::Result<SerializeCore> {
    let target_ty = &input.target_ty;
    let target_name = target_type_name(target_ty);
    let tag_field = &input.tag_field;
    let (_, ty_generics, where_clause) = input.generics.split_for_impl();

    let ser_tag_expr = match &input.serialize_version {
        VersionTag::Int(n) => quote!(#n),
        VersionTag::String(s) => quote!(#s),
        VersionTag::Untagged => unreachable!("serialize_version cannot be untagged"),
    };

    let is_internal_unit_shadow = !input.external
        && input.content_field.is_none()
        && dag_plan.serialize_target_is_wire
        && matches!(dag_plan.serialize_mode, SerializeConvertMode::Into)
        && matches!(wire_mode, SelfWireMode::Shadow(info) if info.is_unit_struct);

    if is_internal_unit_shadow {
        let body = quote! {
            ::backwards_compat::__private::serialize_unit_tagged(
                __serializer,
                #target_name,
                #tag_field,
                &#ser_tag_expr,
            )
        };
        return Ok(SerializeCore {
            helper_def: TokenStream::new(),
            body,
            where_clause: where_clause.cloned(),
        });
    }

    let content_field_opt = input.content_field.as_ref();
    let ext_variants = if input.external {
        collect_external_variants(input, dag_plan, "__BC_EXT_SER")
    } else {
        Vec::new()
    };
    let ext_idx_defs = if input.external {
        emit_ext_variant_indices(&ext_variants, "__BC_EXT_SER", &quote!(u32))
    } else {
        TokenStream::new()
    };
    let (ext_idx_expr, ext_variant_name) = if input.external {
        ext_variant_lookup(&ext_variants, &input.serialize_version)
    } else {
        (TokenStream::new(), String::new())
    };

    let emit_ser_expr = |wire_ref: TokenStream| -> TokenStream {
        if input.external {
            quote! {
                #ext_idx_defs
                ::serde::Serializer::serialize_newtype_variant(
                    __serializer,
                    #target_name,
                    #ext_idx_expr,
                    #ext_variant_name,
                    #wire_ref,
                )
            }
        } else if let Some(content_field) = content_field_opt {
            quote! {
                ::backwards_compat::__private::serialize_adjacently_tagged(
                    __serializer,
                    #target_name,
                    #tag_field,
                    &#ser_tag_expr,
                    #content_field,
                    #wire_ref,
                )
            }
        } else {
            quote! {
                ::backwards_compat::__private::serialize_internally_tagged(
                    __serializer,
                    #target_name,
                    #tag_field,
                    &#ser_tag_expr,
                    #wire_ref,
                )
            }
        }
    };

    let resolve_owned_ser_ty = || -> syn::Result<TokenStream> {
        if dag_plan.serialize_target_is_wire {
            match wire_mode {
                SelfWireMode::Shadow(info) => {
                    let owned_ident = info.owned_ident;
                    Ok(quote!(#owned_ident #ty_generics))
                }
                SelfWireMode::Direct => Ok(quote!(#target_ty)),
                SelfWireMode::Disallowed => Err(syn::Error::new_spanned(
                    &input.target_ty,
                    "using the domain struct as the active wire format is only supported when using the #[backwards_compat] attribute macro",
                )),
            }
        } else {
            let ty = &dag_plan.serialize_wire_ty;
            Ok(quote!(#ty))
        }
    };

    let (body, latest_wire_ty_in_helper) = match &dag_plan.serialize_mode {
        SerializeConvertMode::With(ser_fn) => {
            let ser_ty = resolve_owned_ser_ty()?;
            let ser_call = emit_ser_expr(quote!(&latest));
            let s = quote! {
                let latest: #ser_ty = (#ser_fn)(#value_expr);
                #ser_call
            };
            (s, ser_ty)
        }
        SerializeConvertMode::TryWith(ser_fn) => {
            let ser_ty = resolve_owned_ser_ty()?;
            let ser_call = emit_ser_expr(quote!(&latest));
            let s = quote! {
                let latest: #ser_ty = (#ser_fn)(#value_expr).map_err(::serde::ser::Error::custom)?;
                #ser_call
            };
            (s, ser_ty)
        }
        SerializeConvertMode::TryInto => {
            let ser_ty = resolve_owned_ser_ty()?;
            let ser_call = emit_ser_expr(quote!(&latest));
            let s = quote! {
                #[allow(unused_imports)]
                use ::backwards_compat::__private::{
                    BorrowedTryWireConvert as _,
                    FallbackTryWireConvert as _,
                    TryWireConvert as __TryWireConvert,
                };
                let latest: #ser_ty = __TryWireConvert::<_, #ser_ty>::new(#value_expr)
                    .try_convert_wire()
                    .map_err(::serde::ser::Error::custom)?;
                #ser_call
            };
            (s, ser_ty)
        }
        SerializeConvertMode::Into => {
            if dag_plan.serialize_target_is_wire {
                match wire_mode {
                    SelfWireMode::Shadow(info) => {
                        let construct_borrowed = &info.construct_borrowed;
                        let ty_in_helper = &info.borrowed_ty_in_helper;
                        let ser_call = emit_ser_expr(quote!(&latest));
                        let s = quote! {
                            let latest = #construct_borrowed;
                            #ser_call
                        };
                        (s, quote!(#ty_in_helper))
                    }
                    SelfWireMode::Direct => {
                        let s = emit_ser_expr(quote!(#value_expr));
                        (s, quote!(#target_ty))
                    }
                    SelfWireMode::Disallowed => {
                        return Err(syn::Error::new_spanned(
                            &input.target_ty,
                            "using the domain struct as the active wire format is only supported when using the #[backwards_compat] attribute macro",
                        ));
                    }
                }
            } else {
                let ty = &dag_plan.serialize_wire_ty;
                let ser_call = emit_ser_expr(quote!(&latest));
                let s = quote! {
                    use ::backwards_compat::__private::{
                        BorrowedWireConvert as _, FallbackWireConvert as _, WireConvert as __WireConvert,
                    };

                    let latest: #ty = __WireConvert::<_, #ty>::new(#value_expr).convert_wire();
                    #ser_call
                };
                (s, quote!(#ty))
            }
        }
    };

    let mut ser_wc = where_clause.cloned().unwrap_or_else(|| syn::WhereClause {
        where_token: syn::token::Where::default(),
        predicates: syn::punctuated::Punctuated::new(),
    });
    ser_wc.predicates.push(syn::parse_quote!(
        for<'__a> #latest_wire_ty_in_helper: ::serde::Serialize
    ));

    Ok(SerializeCore {
        helper_def: TokenStream::new(),
        body,
        where_clause: Some(ser_wc),
    })
}

struct SerializableArm {
    tag: VersionTag,
    cfg: Option<TokenStream>,
    body: TokenStream,
    bound_ty: Option<TokenStream>,
}

pub(super) fn generate_backwards_compat_serialize(
    input: &BackwardsCompatInput,
    dag_plan: &DagPlan,
    wire_mode: SelfWireMode<'_>,
    version_mode: VersionMode,
    helper_defs_list: &mut Vec<TokenStream>,
) -> syn::Result<TokenStream> {
    let target_ty = &input.target_ty;
    let target_name = target_type_name(target_ty);
    let tag_field = &input.tag_field;
    let content_field_opt = input.content_field.as_ref();
    let is_adjacent = !input.external && content_field_opt.is_some();
    let is_generic = input.generics.type_params().next().is_some();
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let ext_variants = if input.external {
        collect_external_variants(input, dag_plan, "__BC_EXT_AS")
    } else {
        Vec::new()
    };
    let ext_idx_defs = if input.external {
        emit_ext_variant_indices(&ext_variants, "__BC_EXT_AS", &quote!(u32))
    } else {
        TokenStream::new()
    };

    let mut ser_as_wc = where_clause.cloned().unwrap_or_else(|| syn::WhereClause {
        where_token: syn::token::Where::default(),
        predicates: syn::punctuated::Punctuated::new(),
    });
    let mut seen_bound_tys: HashSet<String> = HashSet::new();
    let mut arms: Vec<(usize, SerializableArm)> = Vec::new();

    let build_arm = |tag: &VersionTag,
                     ty: &syn::Type,
                     cfg: Option<TokenStream>,
                     is_ser_ver: bool,
                     serializable: Option<&SerializeConvertMode>|
     -> syn::Result<SerializableArm> {
        let tag_lit = match tag {
            VersionTag::Int(n) => quote!(#n),
            VersionTag::String(s) => quote!(#s),
            VersionTag::Untagged => unreachable!("serializable arm cannot be untagged"),
        };

        let emit_ser_call = |wire_ref: TokenStream| -> TokenStream {
            if input.external {
                let (ext_idx_expr, ext_variant_name) = ext_variant_lookup(&ext_variants, tag);
                quote! {
                    ::serde::Serializer::serialize_newtype_variant(
                        __serializer,
                        #target_name,
                        #ext_idx_expr,
                        #ext_variant_name,
                        #wire_ref,
                    )
                }
            } else {
                content_field_opt.map_or_else(
                    || {
                        quote! {
                            ::backwards_compat::__private::serialize_internally_tagged(
                                __serializer,
                                #target_name,
                                #tag_field,
                                &#tag_lit,
                                #wire_ref,
                            )
                        }
                    },
                    |content_field| {
                        quote! {
                            ::backwards_compat::__private::serialize_adjacently_tagged(
                                __serializer,
                                #target_name,
                                #tag_field,
                                &#tag_lit,
                                #content_field,
                                #wire_ref,
                            )
                        }
                    },
                )
            }
        };

        let resolve_owned_wire_ty = |v_ty: &syn::Type| -> syn::Result<TokenStream> {
            if v_ty == target_ty {
                match wire_mode {
                    SelfWireMode::Shadow(info) => {
                        let owned_ident = info.owned_ident;
                        Ok(quote!(#owned_ident #ty_generics))
                    }
                    SelfWireMode::Direct => Ok(quote!(#target_ty)),
                    SelfWireMode::Disallowed => Err(syn::Error::new_spanned(
                        &input.target_ty,
                        "using the domain struct as the active wire format is only supported when using the #[backwards_compat] attribute macro",
                    )),
                }
            } else {
                Ok(quote!(#v_ty))
            }
        };

        let (body, bound_ty) = match serializable {
            Some(SerializeConvertMode::With(fn_path)) => {
                let wire_ty = resolve_owned_wire_ty(ty)?;
                let ser_call = emit_ser_call(quote!(&__wire_val));
                (
                    quote! {
                        let __wire_val: #wire_ty = (#fn_path)(self);
                        #ser_call
                    },
                    Some(wire_ty),
                )
            }
            Some(SerializeConvertMode::TryWith(fn_path)) => {
                let wire_ty = resolve_owned_wire_ty(ty)?;
                let ser_call = emit_ser_call(quote!(&__wire_val));
                (
                    quote! {
                        let __wire_val: #wire_ty = (#fn_path)(self).map_err(::serde::ser::Error::custom)?;
                        #ser_call
                    },
                    Some(wire_ty),
                )
            }
            Some(SerializeConvertMode::TryInto) => {
                let wire_ty = resolve_owned_wire_ty(ty)?;
                let ser_call = emit_ser_call(quote!(&__wire_val));
                (
                    quote! {
                        #[allow(unused_imports)]
                        use ::backwards_compat::__private::{
                            BorrowedTryWireConvert as _,
                            FallbackTryWireConvert as _,
                            TryWireConvert as __TryWireConvert,
                        };
                        let __wire_val: #wire_ty = __TryWireConvert::<_, #wire_ty>::new(self)
                            .try_convert_wire()
                            .map_err(::serde::ser::Error::custom)?;
                        #ser_call
                    },
                    Some(wire_ty),
                )
            }
            Some(SerializeConvertMode::Into) | None => {
                let bound_ty = if is_ser_ver && !matches!(input.serialize_mode, SerializeConvertMode::Into) {
                    Some(resolve_owned_wire_ty(ty)?)
                } else if ty == target_ty {
                    match wire_mode {
                        SelfWireMode::Shadow(info) => {
                            if !is_adjacent && !input.external && info.is_unit_struct {
                                None
                            } else {
                                let ty_in_helper = &info.borrowed_ty_in_helper;
                                Some(quote!(#ty_in_helper))
                            }
                        }
                        SelfWireMode::Direct => Some(quote!(#target_ty)),
                        SelfWireMode::Disallowed => {
                            return Err(syn::Error::new_spanned(
                                &input.target_ty,
                                "using the domain struct as the active wire format is only supported when using the #[backwards_compat] attribute macro",
                            ));
                        }
                    }
                } else {
                    Some(quote!(#ty))
                };

                if is_ser_ver {
                    (
                        quote!(<Self as ::serde::Serialize>::serialize(self, __serializer)),
                        bound_ty,
                    )
                } else if ty == target_ty {
                    match wire_mode {
                        SelfWireMode::Shadow(info) => {
                            if !is_adjacent && !input.external && info.is_unit_struct {
                                (emit_ser_call(quote!(&())), bound_ty)
                            } else {
                                let construct_borrowed = &info.construct_borrowed;
                                let ser_call = emit_ser_call(quote!(&__wire_val));
                                (
                                    quote! {
                                        let __wire_val = #construct_borrowed;
                                        #ser_call
                                    },
                                    bound_ty,
                                )
                            }
                        }
                        SelfWireMode::Direct => (emit_ser_call(quote!(self)), bound_ty),
                        SelfWireMode::Disallowed => unreachable!("handled above"),
                    }
                } else {
                    let wire_ty = quote!(#ty);
                    let ser_call = emit_ser_call(quote!(&__wire_val));
                    (
                        quote! {
                            #[allow(unused_imports)]
                            use ::backwards_compat::__private::{
                                BorrowedWireConvert as _,
                                FallbackWireConvert as _,
                                WireConvert as __WireConvert,
                            };
                            let __wire_val: #wire_ty = __WireConvert::<_, #wire_ty>::new(self).convert_wire();
                            #ser_call
                        },
                        bound_ty,
                    )
                }
            }
        };

        Ok(SerializableArm {
            tag: tag.clone(),
            cfg,
            body,
            bound_ty,
        })
    };

    for (i, v) in input.versions.iter().enumerate() {
        if v.tag.is_untagged() {
            continue;
        }
        let is_ser_ver = v.tag.matches(&input.serialize_version);
        let is_cur_wire = dag_plan.target_is_wire && v.tag.matches(&input.current_version);
        if !(is_ser_ver || is_cur_wire || v.serializable.is_some()) {
            continue;
        }
        let cfg = combine_cfg_all(v.cfgs.iter().map(|c| &c.tokens));
        arms.push((
            i,
            build_arm(&v.tag, &v.ty, cfg, is_ser_ver, v.serializable.as_ref())?,
        ));
    }

    let has_declared_current = input
        .versions
        .iter()
        .any(|v| v.tag.matches(&input.current_version));
    if dag_plan.target_is_wire && !has_declared_current {
        let is_ser_ver = input.current_version.matches(&input.serialize_version);
        arms.push((
            input.versions.len(),
            build_arm(&input.current_version, target_ty, None, is_ser_ver, None)?,
        ));
    }

    let mut serializable_version_items = Vec::new();
    let mut match_arms = Vec::new();

    for (idx, arm) in arms {
        let cfg_tokens = cfg_attr(arm.cfg.as_ref());
        let ver_expr = version_mode.tag_expr(&arm.tag);
        serializable_version_items.push(quote!(#cfg_tokens #ver_expr));

        if is_generic && let Some(ref pred) = arm.cfg {
            let trait_ident = format_ident!("__BcSerArm_{}", idx);
            let mut arm_wc = where_clause.cloned().unwrap_or_else(|| syn::WhereClause {
                where_token: syn::token::Where::default(),
                predicates: syn::punctuated::Punctuated::new(),
            });
            if let Some(ref bty) = arm.bound_ty {
                arm_wc
                    .predicates
                    .push(syn::parse_quote!(for<'__a> #bty: ::serde::Serialize));
            }
            let arm_body = &arm.body;
            helper_defs_list.push(quote! {
                #[doc(hidden)]
                #[allow(non_camel_case_types, dead_code, reason = "internal cfg-gated serialization helper")]
                trait #trait_ident {
                    #[cfg(#pred)]
                    fn __bc_ser<__S>(
                        &self,
                        __serializer: __S,
                    ) -> ::core::result::Result<__S::Ok, __S::Error>
                    where
                        __S: ::serde::Serializer;
                }

                #[cfg(#pred)]
                impl #impl_generics #trait_ident for #target_ty #arm_wc {
                    fn __bc_ser<__S>(
                        &self,
                        __serializer: __S,
                    ) -> ::core::result::Result<__S::Ok, __S::Error>
                    where
                        __S: ::serde::Serializer,
                    {
                        #arm_body
                    }
                }

                #[cfg(not(#pred))]
                impl #impl_generics #trait_ident for #target_ty #where_clause {}
            });
            ser_as_wc
                .predicates
                .push(syn::parse_quote!(#target_ty: #trait_ident));
            match_arms.push(quote! {
                #cfg_tokens
                #ver_expr => <#target_ty as #trait_ident>::__bc_ser(self, __serializer),
            });
        } else {
            if arm.cfg.is_none()
                && let Some(ref bty) = arm.bound_ty
            {
                let key = bty.to_string();
                if seen_bound_tys.insert(key) {
                    ser_as_wc
                        .predicates
                        .push(syn::parse_quote!(for<'__a> #bty: ::serde::Serialize));
                }
            }
            let arm_body = &arm.body;
            match_arms.push(quote! {
                #cfg_tokens
                #ver_expr => {
                    #arm_body
                }
            });
        }
    }

    let ser_as_where_clause = non_empty_where_clause(ser_as_wc);

    Ok(quote! {
        #ext_idx_defs

        impl #impl_generics ::backwards_compat::BackwardsCompatSerialize for #target_ty #ser_as_where_clause {
            const SERIALIZABLE_VERSIONS: &'static [Self::Version] = &[#(#serializable_version_items,)*];

            fn serialize_as_version<__S>(
                &self,
                __version: Self::Version,
                __serializer: __S,
            ) -> ::core::result::Result<__S::Ok, __S::Error>
            where
                __S: ::serde::Serializer,
            {
                match __version {
                    #(#match_arms)*
                    _ => ::core::result::Result::Err(
                        ::backwards_compat::__private::unsupported_serialize_version_error(__version),
                    ),
                }
            }
        }
    })
}
