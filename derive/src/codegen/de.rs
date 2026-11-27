use std::collections::HashSet;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use super::migrate::{MigrationHelperPool, generate_arm_conversion_stmts};
use super::{
    SelfWireMode, VersionMode, build_de_generics, cfg_attr, non_empty_where_clause, resolve_de_ty,
    tag_display_str, target_type_name,
};
use crate::dag::{DagPlan, combine_cfg_any};
use crate::parse::{BackwardsCompatInput, VersionTag};

struct TaggedArm<'a> {
    tag: &'a VersionTag,
    is_alias: bool,
    de_ty: TokenStream,
    conv_stmts: TokenStream,
    cfg: Option<&'a TokenStream>,
    version_expr: Option<TokenStream>,
    arm_trait: Option<syn::Ident>,
}

struct UntaggedArm<'a> {
    de_ty: TokenStream,
    conv_stmts: TokenStream,
    cfg: Option<&'a TokenStream>,
    arm_trait: Option<syn::Ident>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArmTaggingKind {
    Internal,
    Adjacent,
    External,
}

pub struct UntaggedDeserializeCore {
    pub overall_cfg: Option<TokenStream>,
    pub where_clause: Option<syn::WhereClause>,
    pub body: TokenStream,
}

pub struct DeserializeCore {
    pub helper_defs: TokenStream,
    pub base_where_clause: Option<syn::WhereClause>,
    pub base_body: TokenStream,
    pub untagged: Option<UntaggedDeserializeCore>,
}

fn wrap_tagged_ok(version_expr: Option<&TokenStream>, track_migrated: bool) -> TokenStream {
    if track_migrated {
        let ver = version_expr.expect("version_expr required when track_migrated is true");
        quote! {
            ::core::result::Result::Ok(::backwards_compat::Migrated {
                value: __val,
                source_version: ::core::option::Option::Some(#ver),
            })
        }
    } else {
        quote!(::core::result::Result::Ok(__val))
    }
}

fn wrap_untagged_ok(track_migrated: bool) -> TokenStream {
    if track_migrated {
        quote! {
            ::core::result::Result::Ok(::backwards_compat::Migrated {
                value: __val,
                source_version: ::core::option::Option::None,
            })
        }
    } else {
        quote!(::core::result::Result::Ok(__val))
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "internal helper for cfg-gated generic arm traits"
)]
fn emit_tagged_arm_trait(
    arm_trait: &syn::Ident,
    pred: &TokenStream,
    de_ty: &TokenStream,
    conv_stmts: &TokenStream,
    target_ty: &syn::Type,
    de_impl_generics: &syn::ImplGenerics<'_>,
    de_orig_where_clause: Option<&syn::WhereClause>,
    tagging_kind: ArmTaggingKind,
) -> TokenStream {
    let mut arm_wc = de_orig_where_clause.cloned().unwrap_or_else(|| syn::WhereClause {
        where_token: syn::token::Where::default(),
        predicates: syn::punctuated::Punctuated::new(),
    });
    arm_wc
        .predicates
        .push(syn::parse_quote!(#de_ty: ::serde::Deserialize<'__de>));

    let (trait_methods, impl_methods) = match tagging_kind {
        ArmTaggingKind::Internal => (
            quote! {
                #[cfg(#pred)]
                fn __bc_de<__D>(__d: __D) -> ::core::result::Result<Self, __D::Error>
                where
                    __D: ::serde::Deserializer<'__de>;
            },
            quote! {
                fn __bc_de<__D>(__d: __D) -> ::core::result::Result<Self, __D::Error>
                where
                    __D: ::serde::Deserializer<'__de>,
                {
                    let __val = <#de_ty as ::serde::Deserialize<'__de>>::deserialize(__d)?;
                    #conv_stmts
                    ::core::result::Result::Ok(__val)
                }
            },
        ),
        ArmTaggingKind::Adjacent => (
            quote! {
                #[cfg(#pred)]
                fn __bc_de<__D>(__d: __D) -> ::core::result::Result<Self, __D::Error>
                where
                    __D: ::serde::Deserializer<'__de>;

                #[cfg(#pred)]
                fn __bc_de_adjacent<__M>(
                    __map: &mut __M,
                    __content_field: &'static str,
                ) -> ::core::result::Result<Self, __M::Error>
                where
                    __M: ::serde::de::MapAccess<'__de>;

                #[cfg(#pred)]
                fn __bc_de_seq<__A>(
                    __seq: &mut __A,
                    __exp: &dyn ::serde::de::Expected,
                ) -> ::core::result::Result<Self, __A::Error>
                where
                    __A: ::serde::de::SeqAccess<'__de>;
            },
            quote! {
                fn __bc_de<__D>(__d: __D) -> ::core::result::Result<Self, __D::Error>
                where
                    __D: ::serde::Deserializer<'__de>,
                {
                    let __val = <#de_ty as ::serde::Deserialize<'__de>>::deserialize(__d)?;
                    #conv_stmts
                    ::core::result::Result::Ok(__val)
                }

                fn __bc_de_adjacent<__M>(
                    __map: &mut __M,
                    __content_field: &'static str,
                ) -> ::core::result::Result<Self, __M::Error>
                where
                    __M: ::serde::de::MapAccess<'__de>,
                {
                    let __val = ::backwards_compat::__private::read_adjacent_content::<__M, #de_ty>(
                        __map,
                        __content_field,
                    )?;
                    #conv_stmts
                    ::core::result::Result::Ok(__val)
                }

                fn __bc_de_seq<__A>(
                    __seq: &mut __A,
                    __exp: &dyn ::serde::de::Expected,
                ) -> ::core::result::Result<Self, __A::Error>
                where
                    __A: ::serde::de::SeqAccess<'__de>,
                {
                    let ::core::option::Option::Some(__val) = __seq.next_element::<#de_ty>()? else {
                        return ::core::result::Result::Err(::serde::de::Error::invalid_length(1, __exp));
                    };
                    #conv_stmts
                    ::core::result::Result::Ok(__val)
                }
            },
        ),
        ArmTaggingKind::External => (
            quote! {
                #[cfg(#pred)]
                fn __bc_de_variant<__V>(__variant: __V) -> ::core::result::Result<Self, __V::Error>
                where
                    __V: ::serde::de::VariantAccess<'__de>;
            },
            quote! {
                fn __bc_de_variant<__V>(__variant: __V) -> ::core::result::Result<Self, __V::Error>
                where
                    __V: ::serde::de::VariantAccess<'__de>,
                {
                    let __val = ::serde::de::VariantAccess::newtype_variant::<#de_ty>(__variant)?;
                    #conv_stmts
                    ::core::result::Result::Ok(__val)
                }
            },
        ),
    };

    quote! {
        #[doc(hidden)]
        #[allow(non_camel_case_types, dead_code, reason = "internal cfg-gated deserialization helper")]
        trait #arm_trait<'__de>: ::core::marker::Sized {
            #trait_methods
        }

        #[cfg(#pred)]
        impl #de_impl_generics #arm_trait<'__de> for #target_ty #arm_wc {
            #impl_methods
        }

        #[cfg(not(#pred))]
        impl #de_impl_generics #arm_trait<'__de> for #target_ty #de_orig_where_clause {}
    }
}

fn emit_untagged_arm_trait(
    arm_trait: &syn::Ident,
    pred: &TokenStream,
    de_ty: &TokenStream,
    conv_stmts: &TokenStream,
    target_ty: &syn::Type,
    de_impl_generics: &syn::ImplGenerics<'_>,
    de_orig_where_clause: Option<&syn::WhereClause>,
) -> TokenStream {
    let mut arm_wc = de_orig_where_clause.cloned().unwrap_or_else(|| syn::WhereClause {
        where_token: syn::token::Where::default(),
        predicates: syn::punctuated::Punctuated::new(),
    });
    arm_wc
        .predicates
        .push(syn::parse_quote!(#de_ty: ::serde::Deserialize<'__de>));

    quote! {
        #[doc(hidden)]
        #[allow(non_camel_case_types, dead_code, reason = "internal cfg-gated untagged deserialization helper")]
        trait #arm_trait<'__de>: ::core::marker::Sized {
            #[cfg(#pred)]
            fn __bc_try_untagged<__D>(
                __d: __D,
            ) -> ::core::result::Result<::core::option::Option<Self>, __D::Error>
            where
                __D: ::serde::Deserializer<'__de>;
        }

        #[cfg(#pred)]
        impl #de_impl_generics #arm_trait<'__de> for #target_ty #arm_wc {
            fn __bc_try_untagged<__D>(
                __d: __D,
            ) -> ::core::result::Result<::core::option::Option<Self>, __D::Error>
            where
                __D: ::serde::Deserializer<'__de>,
            {
                if let ::core::result::Result::Ok(__val) =
                    <#de_ty as ::serde::Deserialize<'__de>>::deserialize(__d)
                {
                    #conv_stmts
                    return ::core::result::Result::Ok(::core::option::Option::Some(__val));
                }
                ::core::result::Result::Ok(::core::option::Option::None)
            }
        }

        #[cfg(not(#pred))]
        impl #de_impl_generics #arm_trait<'__de> for #target_ty #de_orig_where_clause {}
    }
}

struct VisitorContext<'a> {
    input: &'a BackwardsCompatInput,
    version_mode: VersionMode,
    de_impl_generics: &'a syn::ImplGenerics<'a>,
    de_ty_generics: &'a syn::TypeGenerics<'a>,
    de_orig_where_clause: Option<&'a syn::WhereClause>,
    tagged_arms: &'a [TaggedArm<'a>],
    untagged_arms: &'a [UntaggedArm<'a>],
    supported_str_items: &'a [TokenStream],
    current_int: u64,
    has_disabled_init: &'a TokenStream,
    track_migrated: bool,
}

fn emit_unknown_version_expr(
    version_mode: VersionMode,
    tag_expr: &TokenStream,
    current_int: u64,
    supported_str_items: &[TokenStream],
    has_disabled_init: &TokenStream,
) -> TokenStream {
    match version_mode {
        VersionMode::Int => quote! {
            let __supported: &[&str] = &[#(#supported_str_items,)*];
            #has_disabled_init
            ::core::result::Result::Err(
                ::backwards_compat::__private::unknown_int_version_error(
                    #tag_expr,
                    #current_int,
                    __supported,
                    __has_disabled,
                ),
            )
        },
        VersionMode::Str => quote! {
            let __supported: &[&str] = &[#(#supported_str_items,)*];
            #has_disabled_init
            ::core::result::Result::Err(
                ::backwards_compat::__private::unknown_str_version_error(
                    #tag_expr,
                    __supported,
                    __has_disabled,
                ),
            )
        },
    }
}

fn emit_external_visitor_body(
    ctx: &VisitorContext<'_>,
    visitor_where_clause: Option<&syn::WhereClause>,
) -> TokenStream {
    let target_ty = &ctx.input.target_ty;
    let target_name = target_type_name(target_ty);
    let version_mode = ctx.version_mode;
    let de_impl_generics = ctx.de_impl_generics;
    let de_ty_generics = ctx.de_ty_generics;
    let de_orig_where_clause = ctx.de_orig_where_clause;
    let supported_str_items = ctx.supported_str_items;
    let current_int = ctx.current_int;
    let has_disabled_init = ctx.has_disabled_init;

    let value_ty = if ctx.track_migrated {
        quote!(::backwards_compat::Migrated<#target_ty>)
    } else {
        quote!(#target_ty)
    };

    let mut variant_names = Vec::new();
    let mut field_enum_variants = Vec::new();
    let mut str_match_arms = Vec::new();
    let mut visit_enum_arms = Vec::new();

    // Build dense 0..N canonical indices in u64 for `visit_u64` (zero `as` casts).
    let mut idx_stmts = vec![quote!(
        const __BC_EXT_DE_N_0: u64 = 0;
    )];
    let mut u64_match_arms = Vec::new();
    let mut canonical_k = 0usize;

    for (arm_idx, arm) in ctx.tagged_arms.iter().enumerate() {
        let cfg_tokens = cfg_attr(arm.cfg);
        let field_ident = format_ident!("__Arm_{arm_idx}");
        let tag_str = arm.tag.to_tag_string();

        field_enum_variants.push(quote! {
            #cfg_tokens
            #field_ident
        });

        str_match_arms.push(quote! {
            #cfg_tokens
            #tag_str => ::core::result::Result::Ok(__BcExtField::#field_ident),
        });

        if !arm.is_alias {
            variant_names.push(quote! { #cfg_tokens #tag_str });

            let n_cur = format_ident!("__BC_EXT_DE_N_{canonical_k}");
            let n_next = format_ident!("__BC_EXT_DE_N_{}", canonical_k + 1);
            let idx_ident = format_ident!("__BC_EXT_DE_IDX_{canonical_k}");
            if let Some(pred) = arm.cfg {
                idx_stmts.push(quote! {
                    #[cfg(#pred)]
                    const #idx_ident: u64 = #n_cur;
                    #[cfg(#pred)]
                    const #n_next: u64 = #n_cur + 1;
                    #[cfg(not(#pred))]
                    const #n_next: u64 = #n_cur;
                });
            } else {
                idx_stmts.push(quote! {
                    const #idx_ident: u64 = #n_cur;
                    const #n_next: u64 = #n_cur + 1;
                });
            }

            u64_match_arms.push(quote! {
                #cfg_tokens
                #idx_ident => ::core::result::Result::Ok(__BcExtField::#field_ident),
            });
            canonical_k += 1;
        }

        let wrap = wrap_tagged_ok(arm.version_expr.as_ref(), ctx.track_migrated);
        let arm_body = arm.arm_trait.as_ref().map_or_else(
            || {
                let de_ty = &arm.de_ty;
                let conv = &arm.conv_stmts;
                quote! {
                    let __val = ::serde::de::VariantAccess::newtype_variant::<#de_ty>(__variant)?;
                    #conv
                    #wrap
                }
            },
            |arm_trait| {
                quote! {
                    let __val = <#target_ty as #arm_trait<'__de>>::__bc_de_variant(__variant)?;
                    #wrap
                }
            },
        );
        visit_enum_arms.push(quote! {
            #cfg_tokens
            __BcExtField::#field_ident => {
                #arm_body
            }
        });
    }

    let str_fallback = match version_mode {
        VersionMode::Int => quote! {
            _ => {
                let __supported: &[&str] = &[#(#supported_str_items,)*];
                #has_disabled_init
                if let ::core::result::Result::Ok(__n) = __v.parse::<u64>() {
                    ::core::result::Result::Err(
                        ::backwards_compat::__private::unknown_int_version_error(
                            __n,
                            #current_int,
                            __supported,
                            __has_disabled,
                        ),
                    )
                } else {
                    ::core::result::Result::Err(
                        ::backwards_compat::__private::unknown_str_version_error(
                            __v,
                            __supported,
                            __has_disabled,
                        ),
                    )
                }
            }
        },
        VersionMode::Str => quote! {
            _ => {
                let __supported: &[&str] = &[#(#supported_str_items,)*];
                #has_disabled_init
                ::core::result::Result::Err(
                    ::backwards_compat::__private::unknown_str_version_error(
                        __v,
                        __supported,
                        __has_disabled,
                    ),
                )
            }
        },
    };

    quote! {
        #[allow(non_camel_case_types, dead_code, reason = "internal external-tagging variant identifier")]
        enum __BcExtField {
            #(#field_enum_variants,)*
        }

        impl ::backwards_compat::__private::ExtField for __BcExtField {
            fn from_u64<__E>(
                __v: u64,
                __expected: &dyn ::serde::de::Expected,
            ) -> ::core::result::Result<Self, __E>
            where
                __E: ::serde::de::Error,
            {
                #(#idx_stmts)*
                match __v {
                    #(#u64_match_arms)*
                    _ => ::core::result::Result::Err(::serde::de::Error::invalid_value(
                        ::serde::de::Unexpected::Unsigned(__v),
                        __expected,
                    )),
                }
            }

            fn from_str<__E>(__v: &str) -> ::core::result::Result<Self, __E>
            where
                __E: ::serde::de::Error,
            {
                match __v {
                    #(#str_match_arms)*
                    #str_fallback
                }
            }
        }

        #[allow(non_camel_case_types, dead_code, reason = "internal external-tagging visitor")]
        struct __BcVisitor #de_impl_generics (
            ::core::marker::PhantomData<fn() -> #target_ty>,
            ::core::marker::PhantomData<&'__de ()>,
        ) #de_orig_where_clause;

        impl #de_impl_generics ::serde::de::Visitor<'__de> for __BcVisitor #de_ty_generics #visitor_where_clause {
            type Value = #value_ty;

            fn expecting(&self, __formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                __formatter.write_str("an externally tagged versioned value")
            }

            fn visit_enum<__A>(self, __data: __A) -> ::core::result::Result<Self::Value, __A::Error>
            where
                __A: ::serde::de::EnumAccess<'__de>,
            {
                let (::backwards_compat::__private::ExtFieldDeserialize(__field), __variant) =
                    ::serde::de::EnumAccess::variant::<
                        ::backwards_compat::__private::ExtFieldDeserialize<__BcExtField>,
                    >(__data)?;
                match __field {
                    #(#visit_enum_arms,)*
                }
            }
        }

        const __VARIANTS: &'static [&'static str] = &[#(#variant_names,)*];
        let __visitor: __BcVisitor #de_ty_generics = __BcVisitor(
            ::core::marker::PhantomData,
            ::core::marker::PhantomData,
        );
        __deserializer.deserialize_enum(#target_name, __VARIANTS, __visitor)
    }
}

fn emit_visitor_body(
    ctx: &VisitorContext<'_>,
    visitor_where_clause: Option<&syn::WhereClause>,
    accepts_untagged: bool,
) -> TokenStream {
    if ctx.input.external {
        return emit_external_visitor_body(ctx, visitor_where_clause);
    }

    let target_ty = &ctx.input.target_ty;
    let tag_field = &ctx.input.tag_field;
    let content_field_opt = ctx.input.content_field.as_ref();
    let version_mode = ctx.version_mode;
    let tag_de_ty = version_mode.tag_deserialize_ty();
    let tag_param_ty = version_mode.tag_param_ty();
    let tag_mode_ty = version_mode.tag_mode_ty();
    let tag_val_ident = format_ident!("__tag_val");
    let tag_val_arg = version_mode.tag_arg_expr(&tag_val_ident);
    let de_impl_generics = ctx.de_impl_generics;
    let de_ty_generics = ctx.de_ty_generics;
    let de_orig_where_clause = ctx.de_orig_where_clause;
    let supported_str_items = ctx.supported_str_items;
    let current_int = ctx.current_int;
    let has_disabled_init = ctx.has_disabled_init;

    let value_ty = if ctx.track_migrated {
        quote!(::backwards_compat::Migrated<#target_ty>)
    } else {
        quote!(#target_ty)
    };

    let unknown_err_expr = emit_unknown_version_expr(
        version_mode,
        &quote!(__tag),
        current_int,
        supported_str_items,
        has_disabled_init,
    );
    let unknown_version_arm = quote! {
        _ => {
            #unknown_err_expr
        }
    };

    // __bc_dispatch_de: used by internal tagging (fast & slow) and adjacent tagging (slow path).
    let dispatch_de_arms: Vec<TokenStream> = ctx
        .tagged_arms
        .iter()
        .map(|arm| {
            let cfg_tokens = cfg_attr(arm.cfg);
            let pat = version_mode.tag_match_pattern(arm.tag);
            let wrap = wrap_tagged_ok(arm.version_expr.as_ref(), ctx.track_migrated);
            arm.arm_trait.as_ref().map_or_else(
                || {
                    let de_ty = &arm.de_ty;
                    let conv = &arm.conv_stmts;
                    quote! {
                        #cfg_tokens
                        #pat => {
                            let __val = <#de_ty as ::serde::Deserialize<'__de>>::deserialize(__d)?;
                            #conv
                            #wrap
                        }
                    }
                },
                |arm_trait| {
                    quote! {
                        #cfg_tokens
                        #pat => {
                            let __val = <#target_ty as #arm_trait<'__de>>::__bc_de(__d)?;
                            #wrap
                        }
                    }
                },
            )
        })
        .collect();

    let internal_dispatch_impl = quote! {
        impl #de_impl_generics ::backwards_compat::__private::InternalMapDispatch<'__de, #tag_mode_ty>
            for __BcVisitor #de_ty_generics #visitor_where_clause
        {
            type Value = #value_ty;

            fn dispatch_tagged<__D2>(
                __tag: #tag_param_ty,
                __d: __D2,
            ) -> ::core::result::Result<Self::Value, __D2::Error>
            where
                __D2: ::serde::Deserializer<'__de>,
            {
                match __tag {
                    #(#dispatch_de_arms,)*
                    #unknown_version_arm
                }
            }
        }
    };

    // Adjacent-tagging streaming helper (AdjacentMapDispatch impl).
    let adjacent_dispatch_impl = content_field_opt.map_or_else(TokenStream::new, |content_field| {
        let adj_arms: Vec<TokenStream> = ctx
            .tagged_arms
            .iter()
            .map(|arm| {
                let cfg_tokens = cfg_attr(arm.cfg);
                let pat = version_mode.tag_match_pattern(arm.tag);
                let wrap = wrap_tagged_ok(arm.version_expr.as_ref(), ctx.track_migrated);
                arm.arm_trait.as_ref().map_or_else(
                    || {
                        let de_ty = &arm.de_ty;
                        let conv = &arm.conv_stmts;
                        quote! {
                            #cfg_tokens
                            #pat => {
                                let __val = ::backwards_compat::__private::read_adjacent_content::<__M, #de_ty>(
                                    __map,
                                    #content_field,
                                )?;
                                #conv
                                #wrap
                            }
                        }
                    },
                    |arm_trait| {
                        quote! {
                            #cfg_tokens
                            #pat => {
                                let __val = <#target_ty as #arm_trait<'__de>>::__bc_de_adjacent(
                                    __map,
                                    #content_field,
                                )?;
                                #wrap
                            }
                        }
                    },
                )
            })
            .collect();

        quote! {
            impl #de_impl_generics ::backwards_compat::__private::AdjacentMapDispatch<'__de, #tag_mode_ty>
                for __BcVisitor #de_ty_generics #visitor_where_clause
            {
                fn dispatch_adjacent_streaming<__M>(
                    __tag: #tag_param_ty,
                    __map: &mut __M,
                ) -> ::core::result::Result<Self::Value, __M::Error>
                where
                    __M: ::serde::de::MapAccess<'__de>,
                {
                    match __tag {
                        #(#adj_arms,)*
                        #unknown_version_arm
                    }
                }
            }
        }
    });

    // Untagged dispatch logic (if accepts_untagged).
    let is_single_unconditional_untagged = accepts_untagged
        && ctx.untagged_arms.len() == 1
        && ctx.untagged_arms.first().is_some_and(|arm| arm.cfg.is_none());

    let untagged_fallback_closure = if is_single_unconditional_untagged {
        quote! {
            |__content| {
                <Self as ::backwards_compat::__private::UntaggedSingleDispatch<'__de>>::dispatch_de(
                    ::backwards_compat::__private::ContentDeserializer::new(__content),
                )
            }
        }
    } else {
        quote! {
            |__content| {
                <Self as ::backwards_compat::__private::UntaggedMultiDispatch<'__de>>::dispatch_content(
                    __content,
                )
            }
        }
    };

    // visit_map implementation delegating to shared crate-level map scanners.
    let visit_map_method = content_field_opt.map_or_else(
        || {
            if accepts_untagged {
                quote! {
                    fn visit_map<__A>(self, __map: __A) -> ::core::result::Result<Self::Value, __A::Error>
                    where
                        __A: ::serde::de::MapAccess<'__de>,
                    {
                        ::backwards_compat::__private::visit_internal_map_untagged::<
                            #tag_mode_ty,
                            Self,
                            __A,
                            _,
                        >(__map, #tag_field, #untagged_fallback_closure)
                    }
                }
            } else {
                quote! {
                    fn visit_map<__A>(self, __map: __A) -> ::core::result::Result<Self::Value, __A::Error>
                    where
                        __A: ::serde::de::MapAccess<'__de>,
                    {
                        ::backwards_compat::__private::visit_internal_map::<#tag_mode_ty, Self, __A>(
                            __map,
                            #tag_field,
                        )
                    }
                }
            }
        },
        |content_field| {
            if accepts_untagged {
                quote! {
                    fn visit_map<__A>(self, __map: __A) -> ::core::result::Result<Self::Value, __A::Error>
                    where
                        __A: ::serde::de::MapAccess<'__de>,
                    {
                        ::backwards_compat::__private::visit_adjacent_map_untagged::<
                            #tag_mode_ty,
                            Self,
                            __A,
                            _,
                        >(__map, #tag_field, #content_field, #untagged_fallback_closure)
                    }
                }
            } else {
                quote! {
                    fn visit_map<__A>(self, __map: __A) -> ::core::result::Result<Self::Value, __A::Error>
                    where
                        __A: ::serde::de::MapAccess<'__de>,
                    {
                        ::backwards_compat::__private::visit_adjacent_map::<#tag_mode_ty, Self, __A>(
                            __map,
                            #tag_field,
                            #content_field,
                        )
                    }
                }
            }
        },
    );

    let expecting_msg = if accepts_untagged {
        "a versioned map or unversioned value"
    } else if content_field_opt.is_some() {
        "a map or sequence"
    } else {
        "a map"
    };

    let (trait_impl, outer_drive) = if !accepts_untagged {
        let extra_visit_methods = content_field_opt.map_or_else(
            || {
                quote! {
                    fn visit_newtype_struct<__D2>(
                        self,
                        __deserializer: __D2,
                    ) -> ::core::result::Result<Self::Value, __D2::Error>
                    where
                        __D2: ::serde::Deserializer<'__de>,
                    {
                        __deserializer.deserialize_any(self)
                    }
                }
            },
            |content_field| {
                let seq_arms: Vec<TokenStream> = ctx
                    .tagged_arms
                    .iter()
                    .map(|arm| {
                        let cfg_tokens = cfg_attr(arm.cfg);
                        let pat = version_mode.tag_match_pattern(arm.tag);
                        let wrap = wrap_tagged_ok(arm.version_expr.as_ref(), ctx.track_migrated);
                        arm.arm_trait.as_ref().map_or_else(
                            || {
                                let de_ty = &arm.de_ty;
                                let conv = &arm.conv_stmts;
                                quote! {
                                    #cfg_tokens
                                    #pat => {
                                        let ::core::option::Option::Some(__val) = __seq.next_element::<#de_ty>()? else {
                                            return ::core::result::Result::Err(
                                                ::serde::de::Error::invalid_length(1, &self),
                                            );
                                        };
                                        #conv
                                        #wrap
                                    }
                                }
                            },
                            |arm_trait| {
                                quote! {
                                    #cfg_tokens
                                    #pat => {
                                        let __val = <#target_ty as #arm_trait<'__de>>::__bc_de_seq(
                                            &mut __seq,
                                            &self,
                                        )?;
                                        #wrap
                                    }
                                }
                            },
                        )
                    })
                    .collect();

                quote! {
                    fn visit_newtype_struct<__D2>(
                        self,
                        __deserializer: __D2,
                    ) -> ::core::result::Result<Self::Value, __D2::Error>
                    where
                        __D2: ::serde::Deserializer<'__de>,
                    {
                        __deserializer.deserialize_struct("", &[#tag_field, #content_field], self)
                    }

                    fn visit_seq<__A>(self, mut __seq: __A) -> ::core::result::Result<Self::Value, __A::Error>
                    where
                        __A: ::serde::de::SeqAccess<'__de>,
                    {
                        let ::core::option::Option::Some(__tag_val) =
                            __seq.next_element::<#tag_de_ty>()?
                        else {
                            return ::core::result::Result::Err(::serde::de::Error::invalid_length(0, &self));
                        };
                        let __tag = #tag_val_arg;
                        match __tag {
                            #(#seq_arms,)*
                            #unknown_version_arm
                        }
                    }
                }
            },
        );

        let visitor_impl = quote! {
            impl #de_impl_generics ::serde::de::Visitor<'__de> for __BcVisitor #de_ty_generics #visitor_where_clause {
                type Value = #value_ty;

                fn expecting(&self, __formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                    __formatter.write_str(#expecting_msg)
                }

                #visit_map_method
                #extra_visit_methods
            }
        };
        let drive = content_field_opt.map_or_else(
            || quote!(__deserializer.deserialize_any(__visitor)),
            |content_field| {
                quote!(__deserializer.deserialize_struct("", &[#tag_field, #content_field], __visitor))
            },
        );
        (visitor_impl, drive)
    } else if is_single_unconditional_untagged {
        let arm = ctx
            .untagged_arms
            .first()
            .expect("single unconditional untagged arm");
        let de_ty = &arm.de_ty;
        let conv = &arm.conv_stmts;
        let wrap = wrap_untagged_ok(ctx.track_migrated);

        let dispatch_impl = quote! {
            impl #de_impl_generics ::backwards_compat::__private::UntaggedSingleDispatch<'__de>
                for __BcVisitor #de_ty_generics #visitor_where_clause
            {
                type Value = #value_ty;

                fn expecting(&self, __formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                    __formatter.write_str(#expecting_msg)
                }

                #visit_map_method

                fn dispatch_de<__D2>(
                    __d: __D2,
                ) -> ::core::result::Result<Self::Value, __D2::Error>
                where
                    __D2: ::serde::Deserializer<'__de>,
                {
                    let __val = <#de_ty as ::serde::Deserialize<'__de>>::deserialize(__d)?;
                    #conv
                    #wrap
                }
            }
        };
        let drive = quote! {
            __deserializer.deserialize_any(
                ::backwards_compat::__private::UntaggedSingleVisitor(__visitor),
            )
        };
        (dispatch_impl, drive)
    } else {
        let wrap = wrap_untagged_ok(ctx.track_migrated);
        let num_arms = ctx.untagged_arms.len();
        let try_arms: Vec<TokenStream> = ctx
            .untagged_arms
            .iter()
            .enumerate()
            .map(|(idx, arm)| {
                let cfg_tokens = cfg_attr(arm.cfg);
                let is_last_unconditional = idx + 1 == num_arms && arm.cfg.is_none();

                arm.arm_trait.as_ref().map_or_else(
                    || {
                        let de_ty = &arm.de_ty;
                        let conv = &arm.conv_stmts;
                        let deserializer_expr = if is_last_unconditional {
                            quote!(::backwards_compat::__private::ContentDeserializer::<__E>::new(__content))
                        } else {
                            quote!(::backwards_compat::__private::ContentRefDeserializer::<__E>::new(&__content))
                        };
                        quote! {
                            #cfg_tokens
                            if let ::core::result::Result::Ok(__val) =
                                <#de_ty as ::serde::Deserialize<'__de>>::deserialize(#deserializer_expr)
                            {
                                #conv
                                return #wrap;
                            }
                        }
                    },
                    |arm_trait| {
                        quote! {
                            #cfg_tokens
                            if let ::core::option::Option::Some(__val) =
                                <#target_ty as #arm_trait<'__de>>::__bc_try_untagged(
                                    ::backwards_compat::__private::ContentRefDeserializer::<__E>::new(&__content),
                                )?
                            {
                                return #wrap;
                            }
                        }
                    },
                )
            })
            .collect();

        let dispatch_impl = quote! {
            impl #de_impl_generics ::backwards_compat::__private::UntaggedMultiDispatch<'__de>
                for __BcVisitor #de_ty_generics #visitor_where_clause
            {
                type Value = #value_ty;

                fn expecting(&self, __formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                    __formatter.write_str(#expecting_msg)
                }

                #visit_map_method

                fn dispatch_content<__E>(
                    __content: ::backwards_compat::__private::Content<'__de>,
                ) -> ::core::result::Result<Self::Value, __E>
                where
                    __E: ::serde::de::Error,
                {
                    #(#try_arms)*
                    ::core::result::Result::Err(::serde::de::Error::custom(
                        "data did not match any unversioned schema variant",
                    ))
                }
            }
        };
        let drive = quote! {
            __deserializer.deserialize_any(
                ::backwards_compat::__private::UntaggedMultiVisitor(__visitor),
            )
        };
        (dispatch_impl, drive)
    };

    quote! {
        #[allow(non_camel_case_types, dead_code, reason = "internal streaming visitor")]
        struct __BcVisitor #de_impl_generics (
            ::core::marker::PhantomData<fn() -> #target_ty>,
            ::core::marker::PhantomData<&'__de ()>,
        ) #de_orig_where_clause;

        #internal_dispatch_impl
        #adjacent_dispatch_impl

        #trait_impl

        let __visitor: __BcVisitor #de_ty_generics = __BcVisitor(
            ::core::marker::PhantomData,
            ::core::marker::PhantomData,
        );
        #outer_drive
    }
}

pub fn generate_deserialize_core(
    input: &BackwardsCompatInput,
    dag_plan: &DagPlan,
    wire_mode: SelfWireMode<'_>,
    track_migrated: bool,
) -> syn::Result<DeserializeCore> {
    let target_ty = &input.target_ty;
    let version_mode = VersionMode::from_input(input);
    let (orig_impl_generics, ty_generics, orig_where_clause) = input.generics.split_for_impl();
    let is_generic = input.generics.type_params().next().is_some();
    let tagging_kind = if input.external {
        ArmTaggingKind::External
    } else if input.content_field.is_some() {
        ArmTaggingKind::Adjacent
    } else {
        ArmTaggingKind::Internal
    };

    let de_generics = build_de_generics(&input.generics);
    let (de_impl_generics, de_ty_generics, de_orig_where_clause) = de_generics.split_for_impl();

    let mut tagged_arms = Vec::new();
    let mut supported_str_items = Vec::new();
    let mut disabled_checks = Vec::new();
    let mut has_any_gated_tagged = false;
    let mut helper_defs_list = Vec::new();
    let mut migration_pool = MigrationHelperPool::new();

    let mut base_wc = de_orig_where_clause.cloned().unwrap_or_else(|| syn::WhereClause {
        where_token: syn::token::Where::default(),
        predicates: syn::punctuated::Punctuated::new(),
    });
    let mut seen_de_tys: HashSet<String> = HashSet::new();

    for (i, v) in input.versions.iter().enumerate() {
        if v.tag.is_untagged() {
            continue;
        }
        let Some(path) = dag_plan.paths.get(i) else {
            continue;
        };
        let cfg_tokens = cfg_attr(path.cfg.as_ref());
        let de_ty = resolve_de_ty(
            &v.ty,
            target_ty,
            &ty_generics,
            wire_mode,
            "using the domain struct as the active wire format is only supported when using the #[backwards_compat] attribute macro",
        )?;
        let conv_stmts =
            generate_arm_conversion_stmts(&v.ty, target_ty, Some(path), wire_mode, &mut migration_pool);
        let version_expr = track_migrated.then(|| version_mode.tag_expr(&v.tag));

        let arm_trait = if is_generic {
            if let Some(ref pred) = path.cfg {
                let trait_ident = format_ident!("__BcArm_{}", i);
                helper_defs_list.push(emit_tagged_arm_trait(
                    &trait_ident,
                    pred,
                    &de_ty,
                    &conv_stmts,
                    target_ty,
                    &de_impl_generics,
                    de_orig_where_clause,
                    tagging_kind,
                ));
                base_wc
                    .predicates
                    .push(syn::parse_quote!(#target_ty: #trait_ident<'__de>));
                Some(trait_ident)
            } else {
                let key = de_ty.to_string();
                if seen_de_tys.insert(key) {
                    base_wc
                        .predicates
                        .push(syn::parse_quote!(#de_ty: ::serde::Deserialize<'__de>));
                }
                None
            }
        } else {
            None
        };

        tagged_arms.push(TaggedArm {
            tag: &v.tag,
            is_alias: v.is_alias,
            de_ty,
            conv_stmts,
            cfg: path.cfg.as_ref(),
            version_expr,
            arm_trait,
        });

        let disp = tag_display_str(&v.tag);
        supported_str_items.push(quote! { #cfg_tokens #disp });

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
        let de_ty = resolve_de_ty(
            target_ty,
            target_ty,
            &ty_generics,
            wire_mode,
            "using the domain struct as the active wire format is only supported when using the #[backwards_compat] attribute macro",
        )?;
        let conv_stmts =
            generate_arm_conversion_stmts(target_ty, target_ty, None, wire_mode, &mut migration_pool);
        let version_expr = track_migrated.then(|| version_mode.tag_expr(&input.current_version));

        if is_generic {
            let key = de_ty.to_string();
            if seen_de_tys.insert(key) {
                base_wc
                    .predicates
                    .push(syn::parse_quote!(#de_ty: ::serde::Deserialize<'__de>));
            }
        }

        tagged_arms.push(TaggedArm {
            tag: &input.current_version,
            is_alias: false,
            de_ty,
            conv_stmts,
            cfg: None,
            version_expr,
            arm_trait: None,
        });

        let disp = tag_display_str(&input.current_version);
        supported_str_items.push(quote!(#disp));
    }

    let untagged_entries: Vec<_> = input
        .versions
        .iter()
        .enumerate()
        .filter(|(_, v)| v.tag.is_untagged())
        .collect();

    let mut untagged_arms = Vec::new();
    let mut untagged_wc = base_wc.clone();
    let mut seen_untagged_de_tys = seen_de_tys;

    for &(idx, untagged_v) in &untagged_entries {
        let path = dag_plan.paths.get(idx);
        let cfg = path.and_then(|p| p.cfg.as_ref());
        let de_ty = resolve_de_ty(
            &untagged_v.ty,
            target_ty,
            &ty_generics,
            wire_mode,
            "using the domain struct as the unversioned wire format is only supported when using the #[backwards_compat] attribute macro",
        )?;
        let conv_stmts =
            generate_arm_conversion_stmts(&untagged_v.ty, target_ty, path, wire_mode, &mut migration_pool);

        let arm_trait = if is_generic {
            if let Some(pred) = cfg {
                let trait_ident = format_ident!("__BcArm_{}", idx);
                helper_defs_list.push(emit_untagged_arm_trait(
                    &trait_ident,
                    pred,
                    &de_ty,
                    &conv_stmts,
                    target_ty,
                    &de_impl_generics,
                    de_orig_where_clause,
                ));
                untagged_wc
                    .predicates
                    .push(syn::parse_quote!(#target_ty: #trait_ident<'__de>));
                Some(trait_ident)
            } else {
                let key = de_ty.to_string();
                if seen_untagged_de_tys.insert(key) {
                    untagged_wc
                        .predicates
                        .push(syn::parse_quote!(#de_ty: ::serde::Deserialize<'__de>));
                }
                None
            }
        } else {
            None
        };

        untagged_arms.push(UntaggedArm {
            de_ty,
            conv_stmts,
            cfg,
            arm_trait,
        });
    }

    let migration_helpers = migration_pool.emit_helpers(target_ty, &orig_impl_generics, orig_where_clause);
    if !migration_helpers.is_empty() {
        helper_defs_list.push(migration_helpers);
    }

    let current_int = match input.current_version {
        VersionTag::Int(n) => n,
        VersionTag::String(_) | VersionTag::Untagged => 0,
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

    let base_where_clause = non_empty_where_clause(base_wc);
    let untagged_where_clause = non_empty_where_clause(untagged_wc);

    let visitor_ctx = VisitorContext {
        input,
        version_mode,
        de_impl_generics: &de_impl_generics,
        de_ty_generics: &de_ty_generics,
        de_orig_where_clause,
        tagged_arms: &tagged_arms,
        untagged_arms: &untagged_arms,
        supported_str_items: &supported_str_items,
        current_int,
        has_disabled_init: &has_disabled_init,
        track_migrated,
    };

    let untagged_overall_cfg =
        if untagged_arms.is_empty() || untagged_arms.iter().any(|arm| arm.cfg.is_none()) {
            None
        } else {
            combine_cfg_any(untagged_arms.iter().filter_map(|arm| arm.cfg))
        };

    let base_body = if untagged_arms.is_empty() || untagged_overall_cfg.is_some() {
        emit_visitor_body(&visitor_ctx, base_where_clause.as_ref(), false)
    } else {
        TokenStream::new()
    };

    let untagged = if untagged_arms.is_empty() {
        None
    } else {
        let body = emit_visitor_body(&visitor_ctx, untagged_where_clause.as_ref(), true);
        Some(UntaggedDeserializeCore {
            overall_cfg: untagged_overall_cfg,
            where_clause: untagged_where_clause,
            body,
        })
    };

    Ok(DeserializeCore {
        helper_defs: quote!(#(#helper_defs_list)*),
        base_where_clause,
        base_body,
        untagged,
    })
}
