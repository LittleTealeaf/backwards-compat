use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::dag::{DagPlan, MigrationStep, ResolvedPath, combine_cfg_any, resolve_dag};
use crate::parse::{BackwardsCompatInput, VersionTag};

fn cfg_attr(pred: Option<&TokenStream>) -> TokenStream {
    pred.map_or_else(TokenStream::new, |p| quote!(#[cfg(#p)]))
}

fn generate_inline_step_expr(step: &MigrationStep) -> TokenStream {
    let to_ty = &step.to_ty;
    match (step.with_fn.as_ref(), step.fallible) {
        (Some(fn_path), true) => quote! {
            let __cur: #to_ty = (#fn_path)(__cur).map_err(::serde::de::Error::custom)?;
        },
        (Some(fn_path), false) => quote! {
            let __cur: #to_ty = (#fn_path)(__cur);
        },
        (None, true) => quote! {
            let __cur: #to_ty = ::core::convert::TryInto::<#to_ty>::try_into(__cur).map_err(::serde::de::Error::custom)?;
        },
        (None, false) => quote! {
            let __cur: #to_ty = ::core::convert::Into::<#to_ty>::into(__cur);
        },
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

#[derive(Debug, Clone, Copy)]
#[allow(dead_code, reason = "Direct variant is used by field-level codegen and tests")]
pub enum SelfWireMode<'a> {
    Shadow(&'a ShadowWireInfo<'a>),
    Direct,
    Disallowed,
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

pub fn build_de_generics(generics: &syn::Generics) -> syn::Generics {
    let mut de_generics = generics.clone();
    let lifetime_idents: Vec<_> = generics.lifetimes().map(|lt| &lt.lifetime).collect();
    let de_lifetime: syn::GenericParam = if lifetime_idents.is_empty() {
        syn::parse_quote!('__de)
    } else {
        syn::parse_quote!('__de: #(#lifetime_idents)+*)
    };
    de_generics.params.insert(0, de_lifetime);
    de_generics
}

fn non_empty_where_clause(wc: syn::WhereClause) -> Option<syn::WhereClause> {
    if wc.predicates.is_empty() { None } else { Some(wc) }
}

fn resolve_de_ty(
    ty: &syn::Type,
    target_ty: &syn::Type,
    ty_generics: &syn::TypeGenerics<'_>,
    wire_mode: SelfWireMode<'_>,
    err_msg: &str,
) -> syn::Result<TokenStream> {
    if ty == target_ty {
        match wire_mode {
            SelfWireMode::Shadow(info) => {
                let owned_ident = info.owned_ident;
                Ok(quote!(#owned_ident #ty_generics))
            }
            SelfWireMode::Direct => Ok(quote!(#target_ty)),
            SelfWireMode::Disallowed => Err(syn::Error::new_spanned(target_ty, err_msg)),
        }
    } else {
        Ok(quote!(#ty))
    }
}

fn generate_arm_conversion_stmts(
    ty: &syn::Type,
    target_ty: &syn::Type,
    path: Option<&ResolvedPath>,
    wire_mode: SelfWireMode<'_>,
) -> TokenStream {
    if ty == target_ty {
        return match wire_mode {
            SelfWireMode::Shadow(_) => quote! {
                let __val: #target_ty = ::core::convert::Into::into(__val);
            },
            SelfWireMode::Direct | SelfWireMode::Disallowed => TokenStream::new(),
        };
    }
    let Some(p) = path else {
        return TokenStream::new();
    };
    if p.steps.is_empty() {
        return TokenStream::new();
    }
    let step_tokens: Vec<_> = p.steps.iter().map(generate_inline_step_expr).collect();
    quote! {
        let __cur = __val;
        #(#step_tokens)*
        let __val: #target_ty = __cur;
    }
}

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
    let tag_field = &input.tag_field;
    let (_, ty_generics, where_clause) = input.generics.split_for_impl();

    let (ser_tag_ty, ser_tag_expr) = match &input.serialize_version {
        VersionTag::Int(n) => (quote!(u64), quote!(#n)),
        VersionTag::String(s) => (quote!(&'static str), quote!(#s)),
        VersionTag::Untagged => unreachable!("serialize_version cannot be untagged"),
    };

    let is_internal_unit_shadow = input.content_field.is_none()
        && dag_plan.serialize_target_is_wire
        && dag_plan.serialize_with_fn.is_none()
        && matches!(wire_mode, SelfWireMode::Shadow(info) if info.is_unit_struct);

    if is_internal_unit_shadow {
        let helper_def = quote! {
            #[doc(hidden)]
            #[allow(non_camel_case_types, dead_code, reason = "internal serialization helper")]
            #[derive(::serde::Serialize)]
            struct __SerializeHelper {
                #[serde(rename = #tag_field)]
                __tag: #ser_tag_ty,
            }
        };
        let body = quote! {
            let helper = __SerializeHelper { __tag: #ser_tag_expr };
            ::serde::Serialize::serialize(&helper, __serializer)
        };
        return Ok(SerializeCore {
            helper_def,
            body,
            where_clause: where_clause.cloned(),
        });
    }

    let is_adjacent = input.content_field.is_some();

    let (body, latest_wire_ty_in_helper) = if let Some(ref ser_fn) = dag_plan.serialize_with_fn {
        let ser_ty = if dag_plan.serialize_target_is_wire {
            match wire_mode {
                SelfWireMode::Shadow(info) => {
                    let owned_ident = info.owned_ident;
                    quote!(#owned_ident #ty_generics)
                }
                SelfWireMode::Direct => quote!(#target_ty),
                SelfWireMode::Disallowed => {
                    return Err(syn::Error::new_spanned(
                        &input.target_ty,
                        "using the domain struct as the active wire format is only supported when using the #[backwards_compat] attribute macro",
                    ));
                }
            }
        } else {
            let ty = &dag_plan.serialize_wire_ty;
            quote!(#ty)
        };
        let s = if is_adjacent {
            quote! {
                let latest: #ser_ty = (#ser_fn)(#value_expr);
                let helper = __SerializeHelper { __tag: #ser_tag_expr, __content: &latest };
                ::serde::Serialize::serialize(&helper, __serializer)
            }
        } else {
            quote! {
                let latest: #ser_ty = (#ser_fn)(#value_expr);
                ::backwards_compat::__private::serialize_internally_tagged(
                    __serializer,
                    "__SerializeHelper",
                    #tag_field,
                    &#ser_tag_expr,
                    &latest,
                )
            }
        };
        (s, ser_ty)
    } else if dag_plan.serialize_target_is_wire {
        match wire_mode {
            SelfWireMode::Shadow(info) => {
                let construct_borrowed = &info.construct_borrowed;
                let ty_in_helper = &info.borrowed_ty_in_helper;
                let s = if is_adjacent {
                    quote! {
                        let latest = #construct_borrowed;
                        let helper = __SerializeHelper { __tag: #ser_tag_expr, __content: &latest };
                        ::serde::Serialize::serialize(&helper, __serializer)
                    }
                } else {
                    quote! {
                        let latest = #construct_borrowed;
                        ::backwards_compat::__private::serialize_internally_tagged(
                            __serializer,
                            "__SerializeHelper",
                            #tag_field,
                            &#ser_tag_expr,
                            &latest,
                        )
                    }
                };
                (s, quote!(#ty_in_helper))
            }
            SelfWireMode::Direct => {
                let s = if is_adjacent {
                    quote! {
                        let helper = __SerializeHelper { __tag: #ser_tag_expr, __content: #value_expr };
                        ::serde::Serialize::serialize(&helper, __serializer)
                    }
                } else {
                    quote! {
                        ::backwards_compat::__private::serialize_internally_tagged(
                            __serializer,
                            "__SerializeHelper",
                            #tag_field,
                            &#ser_tag_expr,
                            #value_expr,
                        )
                    }
                };
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
        let s = if is_adjacent {
            quote! {
                use ::backwards_compat::__private::{
                    BorrowedWireConvert as _, FallbackWireConvert as _, WireConvert as __WireConvert,
                };

                let latest: #ty = __WireConvert::<_, #ty>::new(#value_expr).convert_wire();
                let helper = __SerializeHelper { __tag: #ser_tag_expr, __content: &latest };
                ::serde::Serialize::serialize(&helper, __serializer)
            }
        } else {
            quote! {
                use ::backwards_compat::__private::{
                    BorrowedWireConvert as _, FallbackWireConvert as _, WireConvert as __WireConvert,
                };

                let latest: #ty = __WireConvert::<_, #ty>::new(#value_expr).convert_wire();
                ::backwards_compat::__private::serialize_internally_tagged(
                    __serializer,
                    "__SerializeHelper",
                    #tag_field,
                    &#ser_tag_expr,
                    &latest,
                )
            }
        };
        (s, quote!(#ty))
    };

    if let Some(content) = input.content_field.as_ref() {
        let mut ser_helper_generics = input.generics.clone();
        let ser_helper_lifetime: syn::GenericParam = syn::parse_quote!('__a);
        ser_helper_generics.params.insert(0, ser_helper_lifetime);
        let (_, ser_helper_ty_generics, _) = ser_helper_generics.split_for_impl();

        let helper_def = quote! {
            #[doc(hidden)]
            #[allow(non_camel_case_types, dead_code, reason = "internal serialization helper")]
            #[derive(::serde::Serialize)]
            struct __SerializeHelper #ser_helper_generics #where_clause {
                #[serde(rename = #tag_field)]
                __tag: #ser_tag_ty,
                #[serde(rename = #content)]
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

        Ok(SerializeCore {
            helper_def,
            body,
            where_clause: Some(ser_wc),
        })
    } else {
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
}

struct TaggedArm<'a> {
    tag: &'a VersionTag,
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

fn tag_match_pattern(tag: &VersionTag) -> TokenStream {
    match tag {
        VersionTag::Int(n) => quote!((::core::option::Option::Some(#n), _)),
        VersionTag::String(s) => quote!((_, ::core::option::Option::Some(#s))),
        VersionTag::Untagged => unreachable!("tagged arm cannot be untagged"),
    }
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
    is_adjacent: bool,
) -> TokenStream {
    let mut arm_wc = de_orig_where_clause.cloned().unwrap_or_else(|| syn::WhereClause {
        where_token: syn::token::Where::default(),
        predicates: syn::punctuated::Punctuated::new(),
    });
    arm_wc
        .predicates
        .push(syn::parse_quote!(#de_ty: ::serde::Deserialize<'__de>));

    let adjacent_trait_methods = if is_adjacent {
        quote! {
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
        }
    } else {
        TokenStream::new()
    };

    let adjacent_impl_methods = if is_adjacent {
        quote! {
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
        }
    } else {
        TokenStream::new()
    };

    quote! {
        #[doc(hidden)]
        #[allow(non_camel_case_types, dead_code, reason = "internal cfg-gated deserialization helper")]
        trait #arm_trait<'__de>: ::core::marker::Sized {
            #[cfg(#pred)]
            fn __bc_de<__D>(__d: __D) -> ::core::result::Result<Self, __D::Error>
            where
                __D: ::serde::Deserializer<'__de>;
            #adjacent_trait_methods
        }

        #[cfg(#pred)]
        impl #de_impl_generics #arm_trait<'__de> for #target_ty #arm_wc {
            fn __bc_de<__D>(__d: __D) -> ::core::result::Result<Self, __D::Error>
            where
                __D: ::serde::Deserializer<'__de>,
            {
                let __val = <#de_ty as ::serde::Deserialize<'__de>>::deserialize(__d)?;
                #conv_stmts
                ::core::result::Result::Ok(__val)
            }
            #adjacent_impl_methods
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

fn emit_single_untagged_non_map_visits() -> TokenStream {
    quote! {
        fn visit_bool<__E>(self, __v: bool) -> ::core::result::Result<Self::Value, __E>
        where
            __E: ::serde::de::Error,
        {
            Self::__bc_dispatch_untagged_de(
                ::backwards_compat::__private::ContentDeserializer::<__E>::new(
                    ::backwards_compat::__private::Content::Bool(__v),
                ),
            )
        }

        fn visit_i8<__E>(self, __v: i8) -> ::core::result::Result<Self::Value, __E>
        where
            __E: ::serde::de::Error,
        {
            Self::__bc_dispatch_untagged_de(
                ::backwards_compat::__private::ContentDeserializer::<__E>::new(
                    ::backwards_compat::__private::Content::I8(__v),
                ),
            )
        }

        fn visit_i16<__E>(self, __v: i16) -> ::core::result::Result<Self::Value, __E>
        where
            __E: ::serde::de::Error,
        {
            Self::__bc_dispatch_untagged_de(
                ::backwards_compat::__private::ContentDeserializer::<__E>::new(
                    ::backwards_compat::__private::Content::I16(__v),
                ),
            )
        }

        fn visit_i32<__E>(self, __v: i32) -> ::core::result::Result<Self::Value, __E>
        where
            __E: ::serde::de::Error,
        {
            Self::__bc_dispatch_untagged_de(
                ::backwards_compat::__private::ContentDeserializer::<__E>::new(
                    ::backwards_compat::__private::Content::I32(__v),
                ),
            )
        }

        fn visit_i64<__E>(self, __v: i64) -> ::core::result::Result<Self::Value, __E>
        where
            __E: ::serde::de::Error,
        {
            Self::__bc_dispatch_untagged_de(
                ::backwards_compat::__private::ContentDeserializer::<__E>::new(
                    ::backwards_compat::__private::Content::I64(__v),
                ),
            )
        }

        fn visit_i128<__E>(self, __v: i128) -> ::core::result::Result<Self::Value, __E>
        where
            __E: ::serde::de::Error,
        {
            Self::__bc_dispatch_untagged_de(
                ::backwards_compat::__private::ContentDeserializer::<__E>::new(
                    ::backwards_compat::__private::Content::I128(__v),
                ),
            )
        }

        fn visit_u8<__E>(self, __v: u8) -> ::core::result::Result<Self::Value, __E>
        where
            __E: ::serde::de::Error,
        {
            Self::__bc_dispatch_untagged_de(
                ::backwards_compat::__private::ContentDeserializer::<__E>::new(
                    ::backwards_compat::__private::Content::U8(__v),
                ),
            )
        }

        fn visit_u16<__E>(self, __v: u16) -> ::core::result::Result<Self::Value, __E>
        where
            __E: ::serde::de::Error,
        {
            Self::__bc_dispatch_untagged_de(
                ::backwards_compat::__private::ContentDeserializer::<__E>::new(
                    ::backwards_compat::__private::Content::U16(__v),
                ),
            )
        }

        fn visit_u32<__E>(self, __v: u32) -> ::core::result::Result<Self::Value, __E>
        where
            __E: ::serde::de::Error,
        {
            Self::__bc_dispatch_untagged_de(
                ::backwards_compat::__private::ContentDeserializer::<__E>::new(
                    ::backwards_compat::__private::Content::U32(__v),
                ),
            )
        }

        fn visit_u64<__E>(self, __v: u64) -> ::core::result::Result<Self::Value, __E>
        where
            __E: ::serde::de::Error,
        {
            Self::__bc_dispatch_untagged_de(
                ::backwards_compat::__private::ContentDeserializer::<__E>::new(
                    ::backwards_compat::__private::Content::U64(__v),
                ),
            )
        }

        fn visit_u128<__E>(self, __v: u128) -> ::core::result::Result<Self::Value, __E>
        where
            __E: ::serde::de::Error,
        {
            Self::__bc_dispatch_untagged_de(
                ::backwards_compat::__private::ContentDeserializer::<__E>::new(
                    ::backwards_compat::__private::Content::U128(__v),
                ),
            )
        }

        fn visit_f32<__E>(self, __v: f32) -> ::core::result::Result<Self::Value, __E>
        where
            __E: ::serde::de::Error,
        {
            Self::__bc_dispatch_untagged_de(
                ::backwards_compat::__private::ContentDeserializer::<__E>::new(
                    ::backwards_compat::__private::Content::F32(__v),
                ),
            )
        }

        fn visit_f64<__E>(self, __v: f64) -> ::core::result::Result<Self::Value, __E>
        where
            __E: ::serde::de::Error,
        {
            Self::__bc_dispatch_untagged_de(
                ::backwards_compat::__private::ContentDeserializer::<__E>::new(
                    ::backwards_compat::__private::Content::F64(__v),
                ),
            )
        }

        fn visit_char<__E>(self, __v: char) -> ::core::result::Result<Self::Value, __E>
        where
            __E: ::serde::de::Error,
        {
            Self::__bc_dispatch_untagged_de(
                ::backwards_compat::__private::ContentDeserializer::<__E>::new(
                    ::backwards_compat::__private::Content::Char(__v),
                ),
            )
        }

        fn visit_str<__E>(self, __v: &str) -> ::core::result::Result<Self::Value, __E>
        where
            __E: ::serde::de::Error,
        {
            Self::__bc_dispatch_untagged_de(
                ::backwards_compat::__private::ContentDeserializer::<__E>::new(
                    ::backwards_compat::__private::Content::String(::std::borrow::ToOwned::to_owned(__v)),
                ),
            )
        }

        fn visit_borrowed_str<__E>(self, __v: &'__de str) -> ::core::result::Result<Self::Value, __E>
        where
            __E: ::serde::de::Error,
        {
            Self::__bc_dispatch_untagged_de(
                ::backwards_compat::__private::ContentDeserializer::<__E>::new(
                    ::backwards_compat::__private::Content::Str(__v),
                ),
            )
        }

        fn visit_string<__E>(self, __v: ::std::string::String) -> ::core::result::Result<Self::Value, __E>
        where
            __E: ::serde::de::Error,
        {
            Self::__bc_dispatch_untagged_de(
                ::backwards_compat::__private::ContentDeserializer::<__E>::new(
                    ::backwards_compat::__private::Content::String(__v),
                ),
            )
        }

        fn visit_bytes<__E>(self, __v: &[u8]) -> ::core::result::Result<Self::Value, __E>
        where
            __E: ::serde::de::Error,
        {
            Self::__bc_dispatch_untagged_de(
                ::backwards_compat::__private::ContentDeserializer::<__E>::new(
                    ::backwards_compat::__private::Content::ByteBuf(__v.to_vec()),
                ),
            )
        }

        fn visit_borrowed_bytes<__E>(self, __v: &'__de [u8]) -> ::core::result::Result<Self::Value, __E>
        where
            __E: ::serde::de::Error,
        {
            Self::__bc_dispatch_untagged_de(
                ::backwards_compat::__private::ContentDeserializer::<__E>::new(
                    ::backwards_compat::__private::Content::Bytes(__v),
                ),
            )
        }

        fn visit_byte_buf<__E>(self, __v: ::std::vec::Vec<u8>) -> ::core::result::Result<Self::Value, __E>
        where
            __E: ::serde::de::Error,
        {
            Self::__bc_dispatch_untagged_de(
                ::backwards_compat::__private::ContentDeserializer::<__E>::new(
                    ::backwards_compat::__private::Content::ByteBuf(__v),
                ),
            )
        }

        fn visit_unit<__E>(self) -> ::core::result::Result<Self::Value, __E>
        where
            __E: ::serde::de::Error,
        {
            Self::__bc_dispatch_untagged_de(
                ::backwards_compat::__private::ContentDeserializer::<__E>::new(
                    ::backwards_compat::__private::Content::Unit,
                ),
            )
        }

        fn visit_none<__E>(self) -> ::core::result::Result<Self::Value, __E>
        where
            __E: ::serde::de::Error,
        {
            Self::__bc_dispatch_untagged_de(
                ::backwards_compat::__private::ContentDeserializer::<__E>::new(
                    ::backwards_compat::__private::Content::None,
                ),
            )
        }

        fn visit_some<__D2>(self, __d: __D2) -> ::core::result::Result<Self::Value, __D2::Error>
        where
            __D2: ::serde::Deserializer<'__de>,
        {
            let __inner = <::backwards_compat::__private::Content<'__de> as ::serde::Deserialize>::deserialize(__d)?;
            Self::__bc_dispatch_untagged_de(
                ::backwards_compat::__private::ContentDeserializer::new(
                    ::backwards_compat::__private::Content::Some(::std::boxed::Box::new(__inner)),
                ),
            )
        }

        fn visit_newtype_struct<__D2>(self, __d: __D2) -> ::core::result::Result<Self::Value, __D2::Error>
        where
            __D2: ::serde::Deserializer<'__de>,
        {
            __d.deserialize_any(self)
        }

        fn visit_seq<__A>(self, __seq: __A) -> ::core::result::Result<Self::Value, __A::Error>
        where
            __A: ::serde::de::SeqAccess<'__de>,
        {
            Self::__bc_dispatch_untagged_de(
                ::backwards_compat::__private::OptionForwardingDeserializer::new(
                    ::serde::de::value::SeqAccessDeserializer::new(__seq),
                ),
            )
        }

        fn visit_enum<__A>(self, __data: __A) -> ::core::result::Result<Self::Value, __A::Error>
        where
            __A: ::serde::de::EnumAccess<'__de>,
        {
            Self::__bc_dispatch_untagged_de(
                ::backwards_compat::__private::OptionForwardingDeserializer::new(
                    ::serde::de::value::EnumAccessDeserializer::new(__data),
                ),
            )
        }
    }
}

fn emit_multi_untagged_non_map_visits() -> TokenStream {
    quote! {
        fn visit_bool<__E>(self, __v: bool) -> ::core::result::Result<Self::Value, __E>
        where
            __E: ::serde::de::Error,
        {
            Self::__bc_dispatch_untagged_content(::backwards_compat::__private::Content::Bool(__v))
        }

        fn visit_i8<__E>(self, __v: i8) -> ::core::result::Result<Self::Value, __E>
        where
            __E: ::serde::de::Error,
        {
            Self::__bc_dispatch_untagged_content(::backwards_compat::__private::Content::I8(__v))
        }

        fn visit_i16<__E>(self, __v: i16) -> ::core::result::Result<Self::Value, __E>
        where
            __E: ::serde::de::Error,
        {
            Self::__bc_dispatch_untagged_content(::backwards_compat::__private::Content::I16(__v))
        }

        fn visit_i32<__E>(self, __v: i32) -> ::core::result::Result<Self::Value, __E>
        where
            __E: ::serde::de::Error,
        {
            Self::__bc_dispatch_untagged_content(::backwards_compat::__private::Content::I32(__v))
        }

        fn visit_i64<__E>(self, __v: i64) -> ::core::result::Result<Self::Value, __E>
        where
            __E: ::serde::de::Error,
        {
            Self::__bc_dispatch_untagged_content(::backwards_compat::__private::Content::I64(__v))
        }

        fn visit_i128<__E>(self, __v: i128) -> ::core::result::Result<Self::Value, __E>
        where
            __E: ::serde::de::Error,
        {
            Self::__bc_dispatch_untagged_content(::backwards_compat::__private::Content::I128(__v))
        }

        fn visit_u8<__E>(self, __v: u8) -> ::core::result::Result<Self::Value, __E>
        where
            __E: ::serde::de::Error,
        {
            Self::__bc_dispatch_untagged_content(::backwards_compat::__private::Content::U8(__v))
        }

        fn visit_u16<__E>(self, __v: u16) -> ::core::result::Result<Self::Value, __E>
        where
            __E: ::serde::de::Error,
        {
            Self::__bc_dispatch_untagged_content(::backwards_compat::__private::Content::U16(__v))
        }

        fn visit_u32<__E>(self, __v: u32) -> ::core::result::Result<Self::Value, __E>
        where
            __E: ::serde::de::Error,
        {
            Self::__bc_dispatch_untagged_content(::backwards_compat::__private::Content::U32(__v))
        }

        fn visit_u64<__E>(self, __v: u64) -> ::core::result::Result<Self::Value, __E>
        where
            __E: ::serde::de::Error,
        {
            Self::__bc_dispatch_untagged_content(::backwards_compat::__private::Content::U64(__v))
        }

        fn visit_u128<__E>(self, __v: u128) -> ::core::result::Result<Self::Value, __E>
        where
            __E: ::serde::de::Error,
        {
            Self::__bc_dispatch_untagged_content(::backwards_compat::__private::Content::U128(__v))
        }

        fn visit_f32<__E>(self, __v: f32) -> ::core::result::Result<Self::Value, __E>
        where
            __E: ::serde::de::Error,
        {
            Self::__bc_dispatch_untagged_content(::backwards_compat::__private::Content::F32(__v))
        }

        fn visit_f64<__E>(self, __v: f64) -> ::core::result::Result<Self::Value, __E>
        where
            __E: ::serde::de::Error,
        {
            Self::__bc_dispatch_untagged_content(::backwards_compat::__private::Content::F64(__v))
        }

        fn visit_char<__E>(self, __v: char) -> ::core::result::Result<Self::Value, __E>
        where
            __E: ::serde::de::Error,
        {
            Self::__bc_dispatch_untagged_content(::backwards_compat::__private::Content::Char(__v))
        }

        fn visit_str<__E>(self, __v: &str) -> ::core::result::Result<Self::Value, __E>
        where
            __E: ::serde::de::Error,
        {
            Self::__bc_dispatch_untagged_content(
                ::backwards_compat::__private::Content::String(::std::borrow::ToOwned::to_owned(__v)),
            )
        }

        fn visit_borrowed_str<__E>(self, __v: &'__de str) -> ::core::result::Result<Self::Value, __E>
        where
            __E: ::serde::de::Error,
        {
            Self::__bc_dispatch_untagged_content(::backwards_compat::__private::Content::Str(__v))
        }

        fn visit_string<__E>(self, __v: ::std::string::String) -> ::core::result::Result<Self::Value, __E>
        where
            __E: ::serde::de::Error,
        {
            Self::__bc_dispatch_untagged_content(::backwards_compat::__private::Content::String(__v))
        }

        fn visit_bytes<__E>(self, __v: &[u8]) -> ::core::result::Result<Self::Value, __E>
        where
            __E: ::serde::de::Error,
        {
            Self::__bc_dispatch_untagged_content(::backwards_compat::__private::Content::ByteBuf(__v.to_vec()))
        }

        fn visit_borrowed_bytes<__E>(self, __v: &'__de [u8]) -> ::core::result::Result<Self::Value, __E>
        where
            __E: ::serde::de::Error,
        {
            Self::__bc_dispatch_untagged_content(::backwards_compat::__private::Content::Bytes(__v))
        }

        fn visit_byte_buf<__E>(self, __v: ::std::vec::Vec<u8>) -> ::core::result::Result<Self::Value, __E>
        where
            __E: ::serde::de::Error,
        {
            Self::__bc_dispatch_untagged_content(::backwards_compat::__private::Content::ByteBuf(__v))
        }

        fn visit_unit<__E>(self) -> ::core::result::Result<Self::Value, __E>
        where
            __E: ::serde::de::Error,
        {
            Self::__bc_dispatch_untagged_content(::backwards_compat::__private::Content::Unit)
        }

        fn visit_none<__E>(self) -> ::core::result::Result<Self::Value, __E>
        where
            __E: ::serde::de::Error,
        {
            Self::__bc_dispatch_untagged_content(::backwards_compat::__private::Content::None)
        }

        fn visit_some<__D2>(self, __d: __D2) -> ::core::result::Result<Self::Value, __D2::Error>
        where
            __D2: ::serde::Deserializer<'__de>,
        {
            let __inner = <::backwards_compat::__private::Content<'__de> as ::serde::Deserialize>::deserialize(__d)?;
            Self::__bc_dispatch_untagged_content(
                ::backwards_compat::__private::Content::Some(::std::boxed::Box::new(__inner)),
            )
        }

        fn visit_newtype_struct<__D2>(self, __d: __D2) -> ::core::result::Result<Self::Value, __D2::Error>
        where
            __D2: ::serde::Deserializer<'__de>,
        {
            __d.deserialize_any(self)
        }

        fn visit_seq<__A>(self, mut __seq: __A) -> ::core::result::Result<Self::Value, __A::Error>
        where
            __A: ::serde::de::SeqAccess<'__de>,
        {
            let mut __vec = ::std::vec::Vec::with_capacity(__seq.size_hint().unwrap_or(0));
            while let ::core::option::Option::Some(__elem) =
                __seq.next_element::<::backwards_compat::__private::Content<'__de>>()?
            {
                __vec.push(__elem);
            }
            Self::__bc_dispatch_untagged_content(::backwards_compat::__private::Content::Seq(__vec))
        }
    }
}

struct VisitorContext<'a> {
    input: &'a BackwardsCompatInput,
    de_impl_generics: &'a syn::ImplGenerics<'a>,
    de_ty_generics: &'a syn::TypeGenerics<'a>,
    de_orig_where_clause: Option<&'a syn::WhereClause>,
    tagged_arms: &'a [TaggedArm<'a>],
    untagged_arms: &'a [UntaggedArm<'a>],
    supported_str_items: &'a [TokenStream],
    current_int_opt: &'a TokenStream,
    has_disabled_init: &'a TokenStream,
    track_migrated: bool,
}

fn emit_visitor_body(
    ctx: &VisitorContext<'_>,
    visitor_where_clause: Option<&syn::WhereClause>,
    accepts_untagged: bool,
) -> TokenStream {
    let target_ty = &ctx.input.target_ty;
    let tag_field = &ctx.input.tag_field;
    let content_field_opt = ctx.input.content_field.as_ref();
    let de_impl_generics = ctx.de_impl_generics;
    let de_ty_generics = ctx.de_ty_generics;
    let de_orig_where_clause = ctx.de_orig_where_clause;
    let supported_str_items = ctx.supported_str_items;
    let current_int_opt = ctx.current_int_opt;
    let has_disabled_init = ctx.has_disabled_init;

    let value_ty = if ctx.track_migrated {
        quote!(::backwards_compat::Migrated<#target_ty>)
    } else {
        quote!(#target_ty)
    };

    let unknown_version_arm = quote! {
        _ => {
            let __supported: &[&str] = &[#(#supported_str_items,)*];
            #has_disabled_init
            ::core::result::Result::Err(
                ::backwards_compat::__private::unknown_version_error(
                    __tag,
                    #current_int_opt,
                    __supported,
                    __has_disabled,
                ),
            )
        }
    };

    // __bc_dispatch_de: used by internal tagging (fast & slow) and adjacent tagging (slow path).
    let dispatch_de_arms: Vec<TokenStream> = ctx
        .tagged_arms
        .iter()
        .map(|arm| {
            let cfg_tokens = cfg_attr(arm.cfg);
            let pat = tag_match_pattern(arm.tag);
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

    let dispatch_de_method = quote! {
        fn __bc_dispatch_de<__D2>(
            __tag: &::backwards_compat::__private::TagValue<'__de>,
            __d: __D2,
        ) -> ::core::result::Result<#value_ty, __D2::Error>
        where
            __D2: ::serde::Deserializer<'__de>,
        {
            match (__tag.as_u64(), __tag.as_str()) {
                #(#dispatch_de_arms,)*
                #unknown_version_arm
            }
        }
    };

    // Adjacent-tagging streaming helper (__bc_dispatch_adjacent_streaming).
    let dispatch_adjacent_method = content_field_opt.map_or_else(TokenStream::new, |content_field| {
        let adj_arms: Vec<TokenStream> = ctx
            .tagged_arms
            .iter()
            .map(|arm| {
                let cfg_tokens = cfg_attr(arm.cfg);
                let pat = tag_match_pattern(arm.tag);
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
            fn __bc_dispatch_adjacent_streaming<__M>(
                __tag: &::backwards_compat::__private::TagValue<'__de>,
                __map: &mut __M,
            ) -> ::core::result::Result<#value_ty, __M::Error>
            where
                __M: ::serde::de::MapAccess<'__de>,
            {
                match (__tag.as_u64(), __tag.as_str()) {
                    #(#adj_arms,)*
                    #unknown_version_arm
                }
            }
        }
    });

    // Untagged dispatch helper method (if accepts_untagged).
    let is_single_unconditional_untagged = accepts_untagged
        && ctx.untagged_arms.len() == 1
        && ctx.untagged_arms.first().is_some_and(|arm| arm.cfg.is_none());

    let dispatch_untagged_method = if !accepts_untagged {
        TokenStream::new()
    } else if is_single_unconditional_untagged {
        let arm = ctx
            .untagged_arms
            .first()
            .expect("single unconditional untagged arm");
        let de_ty = &arm.de_ty;
        let conv = &arm.conv_stmts;
        let wrap = wrap_untagged_ok(ctx.track_migrated);
        quote! {
            fn __bc_dispatch_untagged_de<__D2>(
                __d: __D2,
            ) -> ::core::result::Result<#value_ty, __D2::Error>
            where
                __D2: ::serde::Deserializer<'__de>,
            {
                let __val = <#de_ty as ::serde::Deserialize<'__de>>::deserialize(__d)?;
                #conv
                #wrap
            }
        }
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

        quote! {
            fn __bc_dispatch_untagged_content<__E>(
                __content: ::backwards_compat::__private::Content<'__de>,
            ) -> ::core::result::Result<#value_ty, __E>
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

    let untagged_map_fallback = if !accepts_untagged {
        quote!(::core::result::Result::Err(::serde::de::Error::missing_field(#tag_field)))
    } else if is_single_unconditional_untagged {
        quote! {
            Self::__bc_dispatch_untagged_de(
                ::backwards_compat::__private::ContentDeserializer::new(
                    ::backwards_compat::__private::Content::Map(__prefix),
                ),
            )
        }
    } else {
        quote! {
            Self::__bc_dispatch_untagged_content(
                ::backwards_compat::__private::Content::Map(__prefix),
            )
        }
    };

    // visit_map implementation.
    let visit_map_method = content_field_opt.map_or_else(
        || {
            quote! {
                fn visit_map<__A>(self, mut __map: __A) -> ::core::result::Result<Self::Value, __A::Error>
                where
                    __A: ::serde::de::MapAccess<'__de>,
                {
                    let mut __prefix: ::std::vec::Vec<(
                        ::backwards_compat::__private::Content<'__de>,
                        ::backwards_compat::__private::Content<'__de>,
                    )> = ::std::vec::Vec::new();

                    while let ::core::option::Option::Some(__key) = __map.next_key_seed(
                        ::backwards_compat::__private::InternalKey::new(#tag_field),
                    )? {
                        match __key {
                            ::backwards_compat::__private::InternalKeyResult::Tag => {
                                let __tag = __map.next_value::<::backwards_compat::__private::TagValue<'__de>>()?;
                                if __prefix.is_empty() {
                                    return Self::__bc_dispatch_de(
                                        &__tag,
                                        ::backwards_compat::__private::RemainingMapDeserializer::new(__map),
                                    );
                                }
                                return Self::__bc_dispatch_de(
                                    &__tag,
                                    ::backwards_compat::__private::RemainingMapDeserializer::new(
                                        ::backwards_compat::__private::ChainedMapAccess::new(__prefix, __map),
                                    ),
                                );
                            }
                            ::backwards_compat::__private::InternalKeyResult::Other(__k) => {
                                let __v = __map.next_value::<::backwards_compat::__private::Content<'__de>>()?;
                                __prefix.push((__k, __v));
                            }
                        }
                    }

                    #untagged_map_fallback
                }
            }
        },
        |content_field| {
            if accepts_untagged {
                quote! {
                    fn visit_map<__A>(self, mut __map: __A) -> ::core::result::Result<Self::Value, __A::Error>
                    where
                        __A: ::serde::de::MapAccess<'__de>,
                    {
                        let mut __prefix: ::std::vec::Vec<(
                            ::backwards_compat::__private::Content<'__de>,
                            ::backwards_compat::__private::Content<'__de>,
                        )> = ::std::vec::Vec::new();
                        let mut __buffered_content_idx: ::core::option::Option<usize> = ::core::option::Option::None;

                        while let ::core::option::Option::Some(__key) = __map.next_key_seed(
                            ::backwards_compat::__private::AdjacentKey::new(#tag_field, #content_field, true),
                        )? {
                            match __key {
                                ::backwards_compat::__private::AdjacentKeyResult::Tag => {
                                    let __tag = __map.next_value::<::backwards_compat::__private::TagValue<'__de>>()?;
                                    if let ::core::option::Option::Some(__idx) = __buffered_content_idx {
                                        let (_, __content) = __prefix.swap_remove(__idx);
                                        let __res = Self::__bc_dispatch_de(
                                            &__tag,
                                            ::backwards_compat::__private::ContentDeserializer::new(__content),
                                        )?;
                                        ::backwards_compat::__private::drain_map(&mut __map)?;
                                        return ::core::result::Result::Ok(__res);
                                    }
                                    return Self::__bc_dispatch_adjacent_streaming(&__tag, &mut __map);
                                }
                                ::backwards_compat::__private::AdjacentKeyResult::Content => {
                                    let __v = __map.next_value::<::backwards_compat::__private::Content<'__de>>()?;
                                    if __buffered_content_idx.is_none() {
                                        __buffered_content_idx = ::core::option::Option::Some(__prefix.len());
                                    }
                                    __prefix.push((
                                        ::backwards_compat::__private::Content::Str(#content_field),
                                        __v,
                                    ));
                                }
                                ::backwards_compat::__private::AdjacentKeyResult::Other(__k) => {
                                    let __v = __map.next_value::<::backwards_compat::__private::Content<'__de>>()?;
                                    __prefix.push((__k, __v));
                                }
                                ::backwards_compat::__private::AdjacentKeyResult::Ignored => {
                                    let _ = __map.next_value::<::serde::de::IgnoredAny>()?;
                                }
                            }
                        }

                        #untagged_map_fallback
                    }
                }
            } else {
                quote! {
                    fn visit_map<__A>(self, mut __map: __A) -> ::core::result::Result<Self::Value, __A::Error>
                    where
                        __A: ::serde::de::MapAccess<'__de>,
                    {
                        let mut __buffered_content: ::core::option::Option<
                            ::backwards_compat::__private::Content<'__de>,
                        > = ::core::option::Option::None;

                        while let ::core::option::Option::Some(__key) = __map.next_key_seed(
                            ::backwards_compat::__private::AdjacentKey::new(#tag_field, #content_field, false),
                        )? {
                            match __key {
                                ::backwards_compat::__private::AdjacentKeyResult::Tag => {
                                    let __tag = __map.next_value::<::backwards_compat::__private::TagValue<'__de>>()?;
                                    if let ::core::option::Option::Some(__content) = __buffered_content {
                                        let __res = Self::__bc_dispatch_de(
                                            &__tag,
                                            ::backwards_compat::__private::ContentDeserializer::new(__content),
                                        )?;
                                        ::backwards_compat::__private::drain_map(&mut __map)?;
                                        return ::core::result::Result::Ok(__res);
                                    }
                                    return Self::__bc_dispatch_adjacent_streaming(&__tag, &mut __map);
                                }
                                ::backwards_compat::__private::AdjacentKeyResult::Content => {
                                    if __buffered_content.is_some() {
                                        return ::core::result::Result::Err(
                                            ::serde::de::Error::duplicate_field(#content_field),
                                        );
                                    }
                                    __buffered_content = ::core::option::Option::Some(
                                        __map.next_value::<::backwards_compat::__private::Content<'__de>>()?,
                                    );
                                }
                                ::backwards_compat::__private::AdjacentKeyResult::Ignored
                                | ::backwards_compat::__private::AdjacentKeyResult::Other(_) => {
                                    let _ = __map.next_value::<::serde::de::IgnoredAny>()?;
                                }
                            }
                        }

                        ::core::result::Result::Err(::serde::de::Error::missing_field(#tag_field))
                    }
                }
            }
        },
    );

    // Extra visitor methods (non-map untagged visits, or newtype unwrapping + adjacent visit_seq).
    let extra_visit_methods = if accepts_untagged {
        if is_single_unconditional_untagged {
            emit_single_untagged_non_map_visits()
        } else {
            emit_multi_untagged_non_map_visits()
        }
    } else if let Some(content_field) = content_field_opt {
        let seq_arms: Vec<TokenStream> = ctx
            .tagged_arms
            .iter()
            .map(|arm| {
                let cfg_tokens = cfg_attr(arm.cfg);
                let pat = tag_match_pattern(arm.tag);
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
                    __seq.next_element::<::backwards_compat::__private::TagValue<'__de>>()?
                else {
                    return ::core::result::Result::Err(::serde::de::Error::invalid_length(0, &self));
                };
                let __tag = &__tag_val;
                match (__tag.as_u64(), __tag.as_str()) {
                    #(#seq_arms,)*
                    #unknown_version_arm
                }
            }
        }
    } else {
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
    };

    let expecting_msg = if accepts_untagged {
        "a versioned map or unversioned value"
    } else if content_field_opt.is_some() {
        "a map or sequence"
    } else {
        "a map"
    };

    let outer_drive = if accepts_untagged {
        quote!(__deserializer.deserialize_any(__visitor))
    } else if let Some(content_field) = content_field_opt {
        quote!(__deserializer.deserialize_struct("", &[#tag_field, #content_field], __visitor))
    } else {
        quote!(__deserializer.deserialize_any(__visitor))
    };

    quote! {
        #[allow(non_camel_case_types, dead_code, reason = "internal streaming visitor")]
        struct __BcVisitor #de_impl_generics (
            ::core::marker::PhantomData<fn() -> #target_ty>,
            ::core::marker::PhantomData<&'__de ()>,
        ) #de_orig_where_clause;

        impl #de_impl_generics __BcVisitor #de_ty_generics #visitor_where_clause {
            #dispatch_de_method
            #dispatch_adjacent_method
            #dispatch_untagged_method
        }

        impl #de_impl_generics ::serde::de::Visitor<'__de> for __BcVisitor #de_ty_generics #visitor_where_clause {
            type Value = #value_ty;

            fn expecting(&self, __formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                __formatter.write_str(#expecting_msg)
            }

            #visit_map_method
            #extra_visit_methods
        }

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
    let (_, ty_generics, _) = input.generics.split_for_impl();
    let is_generic = input.generics.type_params().next().is_some();
    let is_adjacent = input.content_field.is_some();

    let de_generics = build_de_generics(&input.generics);
    let (de_impl_generics, de_ty_generics, de_orig_where_clause) = de_generics.split_for_impl();

    let mut tagged_arms = Vec::new();
    let mut supported_str_items = Vec::new();
    let mut disabled_checks = Vec::new();
    let mut has_any_gated_tagged = false;
    let mut helper_defs_list = Vec::new();

    let mut base_wc = de_orig_where_clause.cloned().unwrap_or_else(|| syn::WhereClause {
        where_token: syn::token::Where::default(),
        predicates: syn::punctuated::Punctuated::new(),
    });
    let mut seen_de_tys: Vec<String> = Vec::new();

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
        let conv_stmts = generate_arm_conversion_stmts(&v.ty, target_ty, Some(path), wire_mode);
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
                    is_adjacent,
                ));
                base_wc
                    .predicates
                    .push(syn::parse_quote!(#target_ty: #trait_ident<'__de>));
                Some(trait_ident)
            } else {
                let key = de_ty.to_string();
                if !seen_de_tys.contains(&key) {
                    seen_de_tys.push(key);
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
        let conv_stmts = generate_arm_conversion_stmts(target_ty, target_ty, None, wire_mode);
        let version_expr = track_migrated.then(|| version_mode.tag_expr(&input.current_version));

        if is_generic {
            let key = de_ty.to_string();
            if !seen_de_tys.contains(&key) {
                seen_de_tys.push(key);
                base_wc
                    .predicates
                    .push(syn::parse_quote!(#de_ty: ::serde::Deserialize<'__de>));
            }
        }

        tagged_arms.push(TaggedArm {
            tag: &input.current_version,
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
        let conv_stmts = generate_arm_conversion_stmts(&untagged_v.ty, target_ty, path, wire_mode);

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
                if !seen_untagged_de_tys.contains(&key) {
                    seen_untagged_de_tys.push(key);
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

    let base_where_clause = non_empty_where_clause(base_wc);
    let untagged_where_clause = non_empty_where_clause(untagged_wc);

    let visitor_ctx = VisitorContext {
        input,
        de_impl_generics: &de_impl_generics,
        de_ty_generics: &de_ty_generics,
        de_orig_where_clause,
        tagged_arms: &tagged_arms,
        untagged_arms: &untagged_arms,
        supported_str_items: &supported_str_items,
        current_int_opt: &current_int_opt,
        has_disabled_init: &has_disabled_init,
        track_migrated,
    };

    let base_body = emit_visitor_body(&visitor_ctx, base_where_clause.as_ref(), false);

    let untagged = if untagged_arms.is_empty() {
        None
    } else {
        let overall_cfg = if untagged_arms.iter().any(|arm| arm.cfg.is_none()) {
            None
        } else {
            combine_cfg_any(untagged_arms.iter().filter_map(|arm| arm.cfg))
        };
        let body = emit_visitor_body(&visitor_ctx, untagged_where_clause.as_ref(), true);
        Some(UntaggedDeserializeCore {
            overall_cfg,
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

pub fn generate_backwards_compat(
    input: &BackwardsCompatInput,
    wire_mode: SelfWireMode<'_>,
    extra_items: Option<TokenStream>,
) -> syn::Result<TokenStream> {
    let dag_plan = resolve_dag(input)?;

    let target_ty = &input.target_ty;
    let version_mode = VersionMode::from_input(input);
    let (impl_generics, _, where_clause) = input.generics.split_for_impl();

    let de_generics = build_de_generics(&input.generics);
    let (de_impl_generics, _, _) = de_generics.split_for_impl();

    let ser_core = generate_serialize_core(input, &dag_plan, wire_mode, &quote!(self))?;
    let de_core = generate_deserialize_core(input, &dag_plan, wire_mode, true)?;

    let mut supported_version_items = Vec::new();
    for (i, v) in input.versions.iter().enumerate() {
        if v.tag.is_untagged() {
            continue;
        }
        let Some(path) = dag_plan.paths.get(i) else {
            continue;
        };
        let cfg_tokens = cfg_attr(path.cfg.as_ref());
        let version_expr = version_mode.tag_expr(&v.tag);
        supported_version_items.push(quote! { #cfg_tokens #version_expr });
    }
    let has_declared_current = input
        .versions
        .iter()
        .any(|v| v.tag.matches(&input.current_version));
    if dag_plan.target_is_wire && !has_declared_current {
        let version_expr = version_mode.tag_expr(&input.current_version);
        supported_version_items.push(quote!(#version_expr));
    }

    let emit_de_pair = |cfg_prefix: TokenStream, where_cl: Option<&syn::WhereClause>, body: &TokenStream| {
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

    let (deserialize_impls, accepts_untagged_const) = if let Some(ref untagged) = de_core.untagged {
        untagged.overall_cfg.as_ref().map_or_else(
            || {
                (
                    emit_de_pair(TokenStream::new(), untagged.where_clause.as_ref(), &untagged.body),
                    quote!(
                        const ACCEPTS_UNTAGGED: bool = true;
                    ),
                )
            },
            |pred| {
                let enabled = emit_de_pair(
                    quote!(#[cfg(#pred)]),
                    untagged.where_clause.as_ref(),
                    &untagged.body,
                );
                let disabled = emit_de_pair(
                    quote!(#[cfg(not(#pred))]),
                    de_core.base_where_clause.as_ref(),
                    &de_core.base_body,
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
        )
    } else {
        (
            emit_de_pair(
                TokenStream::new(),
                de_core.base_where_clause.as_ref(),
                &de_core.base_body,
            ),
            quote!(
                const ACCEPTS_UNTAGGED: bool = false;
            ),
        )
    };

    let extra = extra_items.unwrap_or_default();
    let serialize_helper_def = ser_core.helper_def;
    let serialize_stmt = ser_core.body;
    let ser_where_clause = ser_core.where_clause;
    let de_helper_defs = de_core.helper_defs;

    let version_ty = version_mode.version_ty();
    let current_version_expr = version_mode.tag_expr(&input.current_version);
    let serialize_version_expr = version_mode.tag_expr(&input.serialize_version);

    let code = quote! {
        const _: () = {
            #extra

            #de_helper_defs

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
    fn test_generate_backwards_compat_streaming_match_arms_no_helpers() {
        let input = parse_test_input(quote! {
            versions = [
                1: ModelV1 => 2,
                #[fallible]
                2: ModelV2 => 3,
                3: ModelV3,
            ]
        });

        let generated = generate_backwards_compat(&input, SelfWireMode::Disallowed, None).unwrap();
        let rendered = generated.to_string();

        // No __VersionHelper, __UntaggedHelper, or VariantContentDeserializer
        assert!(!rendered.contains("__VersionHelper"));
        assert!(!rendered.contains("__UntaggedHelper"));
        assert!(!rendered.contains("VariantContentDeserializer"));

        // Uses streaming Visitor + InternalKey + RemainingMapDeserializer + ChainedMapAccess
        assert!(rendered.contains("__deserializer . deserialize_any (__visitor)"));
        assert!(rendered.contains("InternalKey :: new (\"version\")"));
        assert!(rendered.contains("RemainingMapDeserializer :: new (__map)"));
        assert!(rendered.contains("ChainedMapAccess :: new (__prefix , __map)"));

        // Direct deserialize + inlined migration chains per arm
        assert!(rendered.contains("< ModelV1 as :: serde :: Deserialize < '__de >> :: deserialize (__d) ?"));
        assert!(rendered.contains(
            "let __cur : ModelV2 = :: core :: convert :: Into :: < ModelV2 > :: into (__cur) ; let __cur : ModelV3 = :: core :: convert :: TryInto :: < ModelV3 > :: try_into (__cur) . map_err (:: serde :: de :: Error :: custom) ? ; let __cur : TargetModel = :: core :: convert :: Into :: < TargetModel > :: into (__cur) ;"
        ));
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

        let generated = generate_backwards_compat(&input, SelfWireMode::Disallowed, None).unwrap();
        let rendered = generated.to_string();

        assert!(!rendered.contains("# [derive (:: serde :: Deserialize)]"));
        assert!(!rendered.contains("# [derive (:: serde :: Serialize)]"));
        assert!(rendered.contains("serialize_internally_tagged"));
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
        let generated = generate_backwards_compat(&input, SelfWireMode::Shadow(&shadow_info), None).unwrap();
        let rendered = generated.to_string();

        assert!(
            rendered
                .contains("< TargetModelWire as :: serde :: Deserialize < '__de >> :: deserialize (__d) ?")
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
        assert!(
            de_str.contains("< TargetModel as :: serde :: Deserialize < '__de >> :: deserialize (__d) ?")
        );
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

        let result = generate_backwards_compat(&input, SelfWireMode::Disallowed, None);
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

        let generated = generate_backwards_compat(&input, SelfWireMode::Disallowed, None).unwrap();
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

        let generated = generate_backwards_compat(&input, SelfWireMode::Disallowed, None).unwrap();
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

        let generated = generate_backwards_compat(&input, SelfWireMode::Disallowed, None).unwrap();
        let rendered = generated.to_string();

        assert!(
            rendered
                .contains("__deserializer . deserialize_struct (\"\" , & [\"type\" , \"data\"] , __visitor)")
        );
        assert!(rendered.contains("AdjacentKey :: new (\"type\" , \"data\" , false)"));
        assert!(rendered.contains("read_adjacent_content :: < __M , ModelV1 >"));
        assert!(rendered.contains("fn visit_seq < __A >"));
        assert!(rendered.contains(
            "struct __SerializeHelper < '__a > { # [serde (rename = \"type\")] __tag : u64 , # [serde (rename = \"data\")] __content : & '__a ModelV2 , }"
        ));
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

        let generated = generate_backwards_compat(&input, SelfWireMode::Disallowed, None).unwrap();
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

        let generated = generate_backwards_compat(&input, SelfWireMode::Disallowed, None).unwrap();
        let rendered = generated.to_string();

        assert!(!rendered.contains("__VersionHelper"));
        assert!(rendered.contains(
            "# [cfg (all (feature = \"v1\" , feature = \"v2\"))] (:: core :: option :: Option :: Some (1u64) , _) =>"
        ));
        assert!(
            rendered
                .contains("# [cfg (feature = \"v2\")] (:: core :: option :: Option :: Some (2u64) , _) =>")
        );
        assert!(rendered.contains("(:: core :: option :: Option :: Some (3u64) , _) =>"));

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

        let generated = generate_backwards_compat(&input, SelfWireMode::Disallowed, None).unwrap();
        let rendered = generated.to_string();

        assert!(!rendered.contains("__UntaggedHelper"));
        assert!(!rendered.contains("__VersionHelper"));
        assert!(
            rendered.contains(
                "# [cfg (test)] impl < '__de > :: serde :: Deserialize < '__de > for TargetModel {"
            )
        );
        assert!(rendered.contains(
            "# [cfg (not (test))] impl < '__de > :: serde :: Deserialize < '__de > for TargetModel {"
        ));
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

        let generated = generate_backwards_compat(&input, SelfWireMode::Shadow(&shadow_info), None).unwrap();
        let rendered = generated.to_string();

        assert!(rendered.contains(
            "< TargetModelWire as :: serde :: Deserialize < '__de >> :: deserialize (:: backwards_compat :: __private :: ContentRefDeserializer :: < __E > :: new (& __content))"
        ));
        assert!(
            rendered.contains(
                "# [cfg (test)] impl < '__de > :: serde :: Deserialize < '__de > for TargetModel {"
            )
        );
        assert!(rendered.contains(
            "# [cfg (not (test))] impl < '__de > :: serde :: Deserialize < '__de > for TargetModel {"
        ));

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

        let generated = generate_backwards_compat(&input, SelfWireMode::Disallowed, None).unwrap();
        let rendered = generated.to_string();

        assert!(rendered.contains("__deserializer . deserialize_any (__visitor)"));
        assert!(rendered.contains("fn __bc_dispatch_untagged_de"));
        assert!(!rendered.contains("fn __bc_dispatch_untagged_content"));
        assert!(rendered.contains("Content :: Str (__v)"));
        assert!(rendered.contains("OptionForwardingDeserializer :: new"));
        assert!(rendered.contains("SeqAccessDeserializer :: new (__seq)"));
        assert!(rendered.contains("EnumAccessDeserializer :: new (__data)"));
        syn::parse2::<syn::File>(generated).unwrap();
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
        let int_gen = generate_backwards_compat(&int_input, SelfWireMode::Disallowed, None).unwrap();
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
        let str_gen = generate_backwards_compat(&str_input, SelfWireMode::Disallowed, None).unwrap();
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
        let mixed_gen = generate_backwards_compat(&mixed_input, SelfWireMode::Disallowed, None).unwrap();
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
        let unit_gen =
            generate_backwards_compat(&unit_input, SelfWireMode::Shadow(&unit_shadow), None).unwrap();
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
        let adj_gen = generate_backwards_compat(&adj_input, SelfWireMode::Disallowed, None).unwrap();
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
        let ser_old_gen = generate_backwards_compat(&ser_old_input, SelfWireMode::Disallowed, None).unwrap();
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
        let ser_with_gen =
            generate_backwards_compat(&ser_with_input, SelfWireMode::Disallowed, None).unwrap();
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
        let gen1 =
            generate_backwards_compat(&infallible_input, SelfWireMode::Shadow(&shadow_info), None).unwrap();
        let rendered1 = gen1.to_string();
        assert!(rendered1.contains("let __cur : TargetModel = (convert_v1) (__cur) ;"));
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
        let gen2 =
            generate_backwards_compat(&fallible_input, SelfWireMode::Shadow(&shadow_info), None).unwrap();
        let rendered2 = gen2.to_string();
        assert!(rendered2.contains(
            "let __cur : TargetModel = (try_convert_v1) (__cur) . map_err (:: serde :: de :: Error :: custom) ? ;"
        ));
        syn::parse2::<syn::File>(gen2).unwrap();
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
        let mixed_gen = generate_backwards_compat(&mixed_input, SelfWireMode::Disallowed, None).unwrap();
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
        let gated_gen = generate_backwards_compat(&gated_input, SelfWireMode::Disallowed, None).unwrap();
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

        let generated = generate_backwards_compat(&input, SelfWireMode::Shadow(&shadow_info), None).unwrap();
        let rendered = generated.to_string();

        assert!(
            rendered
                .contains("< TargetModelWire as :: serde :: Deserialize < '__de >> :: deserialize (__d) ?")
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
        let dump_gen = generate_backwards_compat(&dump_input, SelfWireMode::Disallowed, None).unwrap();
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
        let str_gen =
            generate_backwards_compat(&str_shadow_input, SelfWireMode::Shadow(&shadow_info), None).unwrap();
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
        let err_untagged =
            generate_backwards_compat(&untagged_self_input, SelfWireMode::Disallowed, None).unwrap_err();
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
        let err_ser_with =
            generate_backwards_compat(&ser_with_self_input, SelfWireMode::Disallowed, None).unwrap_err();
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
            generate_backwards_compat(&ser_with_guard_input, SelfWireMode::Disallowed, None).unwrap_err();
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
            generate_backwards_compat(&ser_bare_guard_input, SelfWireMode::Disallowed, None).unwrap_err();
        assert!(
            err_ser_bare_guard
                .to_string()
                .contains("using the domain struct as the active wire format")
        );
    }
}
