use syn::{
    ExprPath, Ident, Token, Type,
    parse::{Parse, ParseStream},
    spanned::Spanned as _,
};

/// Parsed arguments to the `#[backwards_compat(via = ProxyType, ...)]` attribute macro.
#[derive(Debug, Clone)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent parsed attribute flags on #[backwards_compat(via = ...)]"
)]
pub struct ViaArgs {
    pub proxy_ty: Type,
    pub try_from: bool,
    pub try_into: bool,
    pub from_fn: Option<ExprPath>,
    pub to_fn: Option<ExprPath>,
    pub dump: bool,
    pub serialize: bool,
    pub deserialize: bool,
}

fn reject_flag_value(input: ParseStream<'_>, flag_name: &str) -> syn::Result<()> {
    if input.peek(Token![=]) || input.peek(syn::token::Paren) {
        return Err(syn::Error::new(
            input.span(),
            format!("`{flag_name}` does not take a value"),
        ));
    }
    Ok(())
}

fn parse_via_type_option(input: ParseStream<'_>, ident_span: proc_macro2::Span) -> syn::Result<Type> {
    let proxy_ty: Type = if input.peek(Token![=]) {
        let _eq: Token![=] = input.parse()?;
        input.parse()?
    } else if input.peek(syn::token::Paren) {
        let content;
        syn::parenthesized!(content in input);
        let ty: Type = content.parse()?;
        if !content.is_empty() {
            return Err(syn::Error::new(
                content.span(),
                "unexpected trailing tokens in `via(...)`",
            ));
        }
        ty
    } else {
        return Err(syn::Error::new(
            ident_span,
            "expected `via = <Type>` or `via(<Type>)`",
        ));
    };

    if matches!(&proxy_ty, Type::Path(tp) if tp.qself.is_none() && tp.path.is_ident("Self")) {
        return Err(syn::Error::new(
            proxy_ty.span(),
            "proxy type in #[backwards_compat(via = ...)] cannot be `Self` (would cause infinite recursion)",
        ));
    }

    Ok(proxy_ty)
}

fn parse_fn_path_option(
    input: ParseStream<'_>,
    name: &str,
    ident_span: proc_macro2::Span,
) -> syn::Result<ExprPath> {
    if input.peek(Token![=]) {
        let _eq: Token![=] = input.parse()?;
        input.parse::<ExprPath>()
    } else if input.peek(syn::token::Paren) {
        let content;
        syn::parenthesized!(content in input);
        let path: ExprPath = content.parse()?;
        if !content.is_empty() {
            return Err(syn::Error::new(
                content.span(),
                format!("unexpected trailing tokens in `{name}(...)`"),
            ));
        }
        Ok(path)
    } else {
        Err(syn::Error::new(
            ident_span,
            format!("expected `{name} = <path>` or `{name}(<path>)`"),
        ))
    }
}

impl Parse for ViaArgs {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut proxy_ty: Option<Type> = None;
        let mut try_from = false;
        let mut try_into = false;
        let mut try_from_span = None;
        let mut try_into_span = None;
        let mut try_from_has_fn = false;
        let mut try_into_has_fn = false;
        let mut fallible_span = None;
        let mut from_fn: Option<ExprPath> = None;
        let mut from_fn_span = None;
        let mut to_fn: Option<ExprPath> = None;
        let mut to_fn_span = None;
        let mut dump = false;
        let mut serialize = true;
        let mut serialize_span = None;
        let mut deserialize = true;
        let mut deserialize_span = None;

        while !input.is_empty() {
            let ident: Ident = input.parse()?;
            match ident.to_string().as_str() {
                "via" => {
                    if proxy_ty.is_some() {
                        return Err(syn::Error::new(
                            ident.span(),
                            "duplicate `via` option in #[backwards_compat(...)]",
                        ));
                    }
                    proxy_ty = Some(parse_via_type_option(input, ident.span())?);
                }
                "try_from" => {
                    if fallible_span.is_some() {
                        return Err(syn::Error::new(
                            ident.span(),
                            "`try_from` cannot be combined with `fallible` (which already enables both `try_from` and `try_into`)",
                        ));
                    }
                    if try_from_span.is_some() {
                        return Err(syn::Error::new(
                            ident.span(),
                            "duplicate `try_from` option in #[backwards_compat(...)]",
                        ));
                    }
                    if input.peek(Token![=]) || input.peek(syn::token::Paren) {
                        let path = parse_fn_path_option(input, "try_from", ident.span())?;
                        if from_fn_span.is_some() {
                            return Err(syn::Error::new(
                                ident.span(),
                                "`from` cannot be combined with `try_from = <path>`",
                            ));
                        }
                        from_fn = Some(path);
                        try_from_has_fn = true;
                    }
                    try_from_span = Some(ident.span());
                    try_from = true;
                }
                "try_into" => {
                    if fallible_span.is_some() {
                        return Err(syn::Error::new(
                            ident.span(),
                            "`try_into` cannot be combined with `fallible` (which already enables both `try_from` and `try_into`)",
                        ));
                    }
                    if try_into_span.is_some() {
                        return Err(syn::Error::new(
                            ident.span(),
                            "duplicate `try_into` option in #[backwards_compat(...)]",
                        ));
                    }
                    if input.peek(Token![=]) || input.peek(syn::token::Paren) {
                        let path = parse_fn_path_option(input, "try_into", ident.span())?;
                        if to_fn_span.is_some() {
                            return Err(syn::Error::new(
                                ident.span(),
                                "`to` cannot be combined with `try_into = <path>`",
                            ));
                        }
                        to_fn = Some(path);
                        try_into_has_fn = true;
                    }
                    try_into_span = Some(ident.span());
                    try_into = true;
                }
                "fallible" => {
                    reject_flag_value(input, "fallible")?;
                    if fallible_span.is_some() {
                        return Err(syn::Error::new(
                            ident.span(),
                            "duplicate `fallible` option in #[backwards_compat(...)]",
                        ));
                    }
                    if try_from_span.is_some() || try_into_span.is_some() {
                        return Err(syn::Error::new(
                            ident.span(),
                            "`fallible` cannot be combined with `try_from` or `try_into`",
                        ));
                    }
                    fallible_span = Some(ident.span());
                    try_from = true;
                    try_into = true;
                }
                "from" => {
                    if try_from_has_fn {
                        return Err(syn::Error::new(
                            ident.span(),
                            "`from` cannot be combined with `try_from = <path>`",
                        ));
                    }
                    if from_fn_span.is_some() {
                        return Err(syn::Error::new(
                            ident.span(),
                            "duplicate `from` option in #[backwards_compat(...)]",
                        ));
                    }
                    from_fn = Some(parse_fn_path_option(input, "from", ident.span())?);
                    from_fn_span = Some(ident.span());
                }
                "to" => {
                    if try_into_has_fn {
                        return Err(syn::Error::new(
                            ident.span(),
                            "`to` cannot be combined with `try_into = <path>`",
                        ));
                    }
                    if to_fn_span.is_some() {
                        return Err(syn::Error::new(
                            ident.span(),
                            "duplicate `to` option in #[backwards_compat(...)]",
                        ));
                    }
                    to_fn = Some(parse_fn_path_option(input, "to", ident.span())?);
                    to_fn_span = Some(ident.span());
                }
                "serialize" => {
                    if serialize_span.is_some() {
                        return Err(syn::Error::new(
                            ident.span(),
                            "duplicate `serialize` option in #[backwards_compat(...)]",
                        ));
                    }
                    serialize = crate::parse::parse_bool_option(input, "serialize", ident.span())?;
                    serialize_span = Some(ident.span());
                }
                "deserialize" => {
                    if deserialize_span.is_some() {
                        return Err(syn::Error::new(
                            ident.span(),
                            "duplicate `deserialize` option in #[backwards_compat(...)]",
                        ));
                    }
                    deserialize = crate::parse::parse_bool_option(input, "deserialize", ident.span())?;
                    deserialize_span = Some(ident.span());
                }
                "dump" => {
                    reject_flag_value(input, "dump")?;
                    if dump {
                        return Err(syn::Error::new(
                            ident.span(),
                            "duplicate `dump` option in #[backwards_compat(...)]",
                        ));
                    }
                    dump = true;
                }
                "tag" | "content" | "versions" | "external" => {
                    return Err(syn::Error::new(
                        ident.span(),
                        format!("`{ident}` cannot be combined with `via` in #[backwards_compat(...)]"),
                    ));
                }
                other => {
                    return Err(syn::Error::new(
                        ident.span(),
                        format!(
                            "unknown option `{other}` in #[backwards_compat(via = ...)]; expected one of: `via`, `try_from`, `try_into`, `fallible`, `from`, `to`, `serialize`, `deserialize`, `dump`"
                        ),
                    ));
                }
            }

            if !input.is_empty() {
                let _comma: Token![,] = input.parse()?;
            }
        }

        let Some(proxy_ty) = proxy_ty else {
            return Err(syn::Error::new(
                input.span(),
                "expected `via = <Type>` or `via(<Type>)` in #[backwards_compat(...)]",
            ));
        };

        if !serialize && !deserialize {
            let span = serialize_span
                .or(deserialize_span)
                .unwrap_or_else(proc_macro2::Span::call_site);
            return Err(syn::Error::new(
                span,
                "cannot disable both `serialize` and `deserialize` in `#[backwards_compat(...)]`; at least one must be enabled",
            ));
        }

        if !serialize {
            if let Some(span) = to_fn_span {
                return Err(syn::Error::new(span, "cannot use `to` when `serialize = false`"));
            }
            if let Some(span) = try_into_span {
                return Err(syn::Error::new(
                    span,
                    "cannot use `try_into` when `serialize = false`",
                ));
            }
            if let Some(span) = fallible_span {
                return Err(syn::Error::new(
                    span,
                    "cannot use `fallible` when `serialize = false`; use `try_from` instead",
                ));
            }
        }

        if !deserialize {
            if let Some(span) = from_fn_span {
                return Err(syn::Error::new(
                    span,
                    "cannot use `from` when `deserialize = false`",
                ));
            }
            if let Some(span) = try_from_span {
                return Err(syn::Error::new(
                    span,
                    "cannot use `try_from` when `deserialize = false`",
                ));
            }
            if let Some(span) = fallible_span {
                return Err(syn::Error::new(
                    span,
                    "cannot use `fallible` when `deserialize = false`; use `try_into` instead",
                ));
            }
        }

        Ok(Self {
            proxy_ty,
            try_from,
            try_into,
            from_fn,
            to_fn,
            dump,
            serialize,
            deserialize,
        })
    }
}

fn make_where_clause(orig: Option<&syn::WhereClause>) -> syn::WhereClause {
    orig.cloned().unwrap_or_else(|| syn::WhereClause {
        where_token: syn::token::Where::default(),
        predicates: syn::punctuated::Punctuated::new(),
    })
}

/// Generates `BackwardsCompat`, `BackwardsCompatDeserialize`, `BackwardsCompatSerialize`,
/// `Deserialize`, `Serialize`, and (optionally) `JsonSchema` implementations for `target_ident`
/// delegating through `args.proxy_ty`.
pub fn generate_via(
    args: &ViaArgs,
    target_ident: &Ident,
    generics: &syn::Generics,
    derive_json_schema: bool,
) -> proc_macro2::TokenStream {
    #[cfg(not(feature = "schemars"))]
    let _ = derive_json_schema;

    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let target_ty = quote::quote!(#target_ident #ty_generics);
    let proxy_ty = &args.proxy_ty;

    let mut bc_where_clause = make_where_clause(where_clause);
    bc_where_clause
        .predicates
        .push(syn::parse_quote!(#proxy_ty: ::backwards_compat::BackwardsCompat));

    let deserialize_impls = if args.deserialize {
        let mut de_generics = generics.clone();
        let lifetime_idents: Vec<_> = generics.lifetimes().map(|lt| &lt.lifetime).collect();
        let de_lifetime: syn::GenericParam = if lifetime_idents.is_empty() {
            syn::parse_quote!('__de)
        } else {
            syn::parse_quote!('__de: #(#lifetime_idents)+*)
        };
        de_generics.params.insert(0, de_lifetime);
        let (de_impl_generics, _, de_orig_where_clause) = de_generics.split_for_impl();

        let mut de_where_clause = make_where_clause(de_orig_where_clause);
        de_where_clause.predicates.push(syn::parse_quote!(
            #proxy_ty: ::backwards_compat::BackwardsCompatDeserialize<'__de>
        ));
        if args.from_fn.is_none() {
            if args.try_from {
                de_where_clause.predicates.push(syn::parse_quote!(
                    Self: ::core::convert::TryFrom<#proxy_ty>
                ));
                de_where_clause.predicates.push(syn::parse_quote!(
                    <Self as ::core::convert::TryFrom<#proxy_ty>>::Error: ::core::fmt::Display
                ));
            } else {
                de_where_clause.predicates.push(syn::parse_quote!(
                    Self: ::core::convert::From<#proxy_ty>
                ));
            }
        }

        let from_expr = match (&args.from_fn, args.try_from) {
            (None, false) => quote::quote! {
                <Self as ::core::convert::From<#proxy_ty>>::from(__migrated.value)
            },
            (None, true) => quote::quote! {
                <Self as ::core::convert::TryFrom<#proxy_ty>>::try_from(__migrated.value)
                    .map_err(::serde::de::Error::custom)?
            },
            (Some(fn_path), false) => quote::quote! {
                (#fn_path)(__migrated.value)
            },
            (Some(fn_path), true) => quote::quote! {
                (#fn_path)(__migrated.value)
                    .map_err(::serde::de::Error::custom)?
            },
        };

        quote::quote! {
            impl #de_impl_generics ::backwards_compat::BackwardsCompatDeserialize<'__de> for #target_ty #de_where_clause {
                fn deserialize_migrated<__D>(
                    __deserializer: __D,
                ) -> ::core::result::Result<::backwards_compat::Migrated<Self>, __D::Error>
                where
                    __D: ::serde::Deserializer<'__de>,
                {
                    let __migrated = <#proxy_ty as ::backwards_compat::BackwardsCompatDeserialize<'__de>>::deserialize_migrated(__deserializer)?;
                    let __value = #from_expr;
                    ::core::result::Result::Ok(::backwards_compat::Migrated {
                        value: __value,
                        source_version: __migrated.source_version,
                    })
                }
            }

            impl #de_impl_generics ::serde::Deserialize<'__de> for #target_ty #de_where_clause {
                fn deserialize<__D>(__deserializer: __D) -> ::core::result::Result<Self, __D::Error>
                where
                    __D: ::serde::Deserializer<'__de>,
                {
                    <Self as ::backwards_compat::BackwardsCompatDeserialize<'__de>>::deserialize_migrated(__deserializer)
                        .map(::backwards_compat::Migrated::into_inner)
                }
            }
        }
    } else {
        proc_macro2::TokenStream::new()
    };

    let serialize_impls = if args.serialize {
        let mut ser_where_clause = make_where_clause(where_clause);
        ser_where_clause
            .predicates
            .push(syn::parse_quote!(#proxy_ty: ::serde::Serialize));

        let to_proxy_stmt = match (&args.to_fn, args.try_into) {
            (None, false) => quote::quote! {
                #[allow(unused_imports)]
                use ::backwards_compat::__private::{
                    BorrowedWireConvert as _,
                    FallbackWireConvert as _,
                    WireConvert as __WireConvert,
                };
                let __proxy: #proxy_ty = __WireConvert::<_, #proxy_ty>::new(self).convert_wire();
            },
            (None, true) => quote::quote! {
                #[allow(unused_imports)]
                use ::backwards_compat::__private::{
                    BorrowedTryWireConvert as _,
                    FallbackTryWireConvert as _,
                    TryWireConvert as __TryWireConvert,
                };
                let __proxy: #proxy_ty = __TryWireConvert::<_, #proxy_ty>::new(self)
                    .try_convert_wire()
                    .map_err(::serde::ser::Error::custom)?;
            },
            (Some(fn_path), false) => quote::quote! {
                let __proxy: #proxy_ty = (#fn_path)(self);
            },
            (Some(fn_path), true) => quote::quote! {
                let __proxy: #proxy_ty = (#fn_path)(self)
                    .map_err(::serde::ser::Error::custom)?;
            },
        };

        let mut ser_as_where_clause = make_where_clause(where_clause);
        ser_as_where_clause
            .predicates
            .push(syn::parse_quote!(#proxy_ty: ::backwards_compat::BackwardsCompatSerialize));

        quote::quote! {
            impl #impl_generics ::serde::Serialize for #target_ty #ser_where_clause {
                fn serialize<__S>(&self, __serializer: __S) -> ::core::result::Result<__S::Ok, __S::Error>
                where
                    __S: ::serde::Serializer,
                {
                    #to_proxy_stmt
                    ::serde::Serialize::serialize(&__proxy, __serializer)
                }
            }

            impl #impl_generics ::backwards_compat::BackwardsCompatSerialize for #target_ty #ser_as_where_clause {
                const SERIALIZABLE_VERSIONS: &'static [Self::Version] =
                    <#proxy_ty as ::backwards_compat::BackwardsCompatSerialize>::SERIALIZABLE_VERSIONS;

                fn serialize_as_version<__S>(
                    &self,
                    __version: Self::Version,
                    __serializer: __S,
                ) -> ::core::result::Result<__S::Ok, __S::Error>
                where
                    __S: ::serde::Serializer,
                {
                    #to_proxy_stmt
                    <#proxy_ty as ::backwards_compat::BackwardsCompatSerialize>::serialize_as_version(
                        &__proxy,
                        __version,
                        __serializer,
                    )
                }
            }
        }
    } else {
        proc_macro2::TokenStream::new()
    };

    #[cfg(feature = "schemars")]
    let json_schema_impl = if derive_json_schema {
        let mut schema_where_clause = make_where_clause(where_clause);
        schema_where_clause.predicates.push(syn::parse_quote!(
            #proxy_ty: ::backwards_compat::__private::schemars::JsonSchema
        ));
        quote::quote! {
            impl #impl_generics ::backwards_compat::__private::schemars::JsonSchema for #target_ty #schema_where_clause {
                fn inline_schema() -> bool {
                    <#proxy_ty as ::backwards_compat::__private::schemars::JsonSchema>::inline_schema()
                }

                fn schema_name() -> ::backwards_compat::__private::schemars::_private::alloc::borrow::Cow<'static, str> {
                    <#proxy_ty as ::backwards_compat::__private::schemars::JsonSchema>::schema_name()
                }

                fn schema_id() -> ::backwards_compat::__private::schemars::_private::alloc::borrow::Cow<'static, str> {
                    <#proxy_ty as ::backwards_compat::__private::schemars::JsonSchema>::schema_id()
                }

                fn json_schema(
                    __gen: &mut ::backwards_compat::__private::schemars::SchemaGenerator,
                ) -> ::backwards_compat::__private::schemars::Schema {
                    <#proxy_ty as ::backwards_compat::__private::schemars::JsonSchema>::json_schema(__gen)
                }
            }
        }
    } else {
        proc_macro2::TokenStream::new()
    };
    #[cfg(not(feature = "schemars"))]
    let json_schema_impl = proc_macro2::TokenStream::new();

    let code = quote::quote! {
        const _: () = {
            impl #impl_generics ::backwards_compat::BackwardsCompat for #target_ty #bc_where_clause {
                type Version = <#proxy_ty as ::backwards_compat::BackwardsCompat>::Version;
                const CURRENT_VERSION: Self::Version = <#proxy_ty as ::backwards_compat::BackwardsCompat>::CURRENT_VERSION;
                const SERIALIZE_VERSION: Self::Version = <#proxy_ty as ::backwards_compat::BackwardsCompat>::SERIALIZE_VERSION;
                const SUPPORTED_VERSIONS: &'static [Self::Version] = <#proxy_ty as ::backwards_compat::BackwardsCompat>::SUPPORTED_VERSIONS;
                const ACCEPTS_UNTAGGED: bool = <#proxy_ty as ::backwards_compat::BackwardsCompat>::ACCEPTS_UNTAGGED;
                const TAGGING: ::backwards_compat::Tagging = <#proxy_ty as ::backwards_compat::BackwardsCompat>::TAGGING;
            }

            #deserialize_impls

            #serialize_impls

            #json_schema_impl
        };
    };

    if args.dump {
        eprintln!(
            "=== backwards_compat(via = ...) output for {} ===\n{}\n==========================================",
            quote::quote!(#target_ty),
            code
        );
    }

    code
}

#[cfg(test)]
mod tests {
    use super::ViaArgs;
    use quote::ToTokens as _;

    fn parse_via(input: &str) -> syn::Result<ViaArgs> {
        syn::parse_str::<ViaArgs>(input)
    }

    fn parse_err(input: &str) -> String {
        parse_via(input)
            .expect_err(&format!("expected error for `{input}`"))
            .to_string()
    }

    #[test]
    fn test_parse_simple_and_generic_proxy_types() {
        let simple = parse_via("via = Proxy").unwrap();
        assert_eq!(simple.proxy_ty.to_token_stream().to_string(), "Proxy");
        assert!(!simple.try_from);
        assert!(!simple.try_into);
        assert!(simple.from_fn.is_none());
        assert!(simple.to_fn.is_none());
        assert!(!simple.dump);

        let paren = parse_via("via(Proxy)").unwrap();
        assert_eq!(paren.proxy_ty.to_token_stream().to_string(), "Proxy");
        assert!(!paren.try_from);
        assert!(!paren.try_into);

        let trailing = parse_via("via = Proxy,").unwrap();
        assert_eq!(trailing.proxy_ty.to_token_stream().to_string(), "Proxy");
        assert!(!trailing.try_from);
        assert!(!trailing.try_into);

        let generic = parse_via("via = crate::wire::Proxy<'a, T>").unwrap();
        assert_eq!(
            generic.proxy_ty.to_token_stream().to_string(),
            "crate :: wire :: Proxy < 'a , T >"
        );

        let generic_paren = parse_via("via(crate::wire::Proxy<'a, T>)").unwrap();
        assert_eq!(
            generic_paren.proxy_ty.to_token_stream().to_string(),
            "crate :: wire :: Proxy < 'a , T >"
        );
    }

    #[test]
    fn test_parse_fallibility_and_dump_flags() {
        let tf = parse_via("via = Proxy, try_from").unwrap();
        assert!(tf.try_from);
        assert!(!tf.try_into);
        assert!(!tf.dump);

        let ti = parse_via("via(Proxy), try_into,").unwrap();
        assert!(!ti.try_from);
        assert!(ti.try_into);

        let both = parse_via("via = Proxy, try_from, try_into").unwrap();
        assert!(both.try_from);
        assert!(both.try_into);

        let both_rev = parse_via("try_into, via = Proxy, try_from").unwrap();
        assert!(both_rev.try_from);
        assert!(both_rev.try_into);

        let fallible = parse_via("fallible, via(Proxy)").unwrap();
        assert!(fallible.try_from);
        assert!(fallible.try_into);

        let dump = parse_via("dump, via = Proxy").unwrap();
        assert!(dump.dump);
        assert!(!dump.try_from);
        assert!(!dump.try_into);
    }

    #[test]
    fn test_parse_from_to_fn_paths_and_arbitrary_order() {
        let eq_style =
            parse_via("via = Proxy, from = crate::conv::decode, to = crate::conv::encode").unwrap();
        assert_eq!(
            eq_style.from_fn.unwrap().to_token_stream().to_string(),
            "crate :: conv :: decode"
        );
        assert_eq!(
            eq_style.to_fn.unwrap().to_token_stream().to_string(),
            "crate :: conv :: encode"
        );

        let paren_style = parse_via("from(decode_fn), via(Proxy), to(encode_fn)").unwrap();
        assert_eq!(
            paren_style.from_fn.unwrap().to_token_stream().to_string(),
            "decode_fn"
        );
        assert_eq!(
            paren_style.to_fn.unwrap().to_token_stream().to_string(),
            "encode_fn"
        );

        let mixed =
            parse_via("dump, to(to_proxy), try_from, via = crate::Proxy<'a, T>, from = from_proxy,").unwrap();
        assert_eq!(
            mixed.proxy_ty.to_token_stream().to_string(),
            "crate :: Proxy < 'a , T >"
        );
        assert!(mixed.dump);
        assert!(mixed.try_from);
        assert!(!mixed.try_into);
        assert_eq!(mixed.from_fn.unwrap().to_token_stream().to_string(), "from_proxy");
        assert_eq!(mixed.to_fn.unwrap().to_token_stream().to_string(), "to_proxy");

        let fallible_combo = parse_via("to = t_fn, fallible, dump, from(f_fn), via(Proxy),").unwrap();
        assert!(fallible_combo.try_from);
        assert!(fallible_combo.try_into);
        assert!(fallible_combo.dump);
        assert_eq!(
            fallible_combo.from_fn.unwrap().to_token_stream().to_string(),
            "f_fn"
        );
        assert_eq!(
            fallible_combo.to_fn.unwrap().to_token_stream().to_string(),
            "t_fn"
        );
    }

    #[test]
    fn test_parse_errors_empty_self_unknown_missing_comma_and_mixing() {
        assert!(parse_err("").contains("expected `via = <Type>` or `via(<Type>)`"));
        assert!(parse_err("dump").contains("expected `via = <Type>` or `via(<Type>)`"));
        assert!(parse_err("via").contains("expected `via = <Type>` or `via(<Type>)`"));
        assert!(parse_err("via(Proxy extra)").contains("unexpected trailing tokens in `via(...)`"));
        assert!(parse_err("via = Self").contains("cannot be `Self`"));
        assert!(parse_err("via(Self), dump").contains("cannot be `Self`"));

        let unknown = parse_err("via = Proxy, bogus");
        assert!(unknown.contains("unknown option `bogus`"));

        assert!(parse_err("via = Proxy try_from").contains("expected `,`"));
        assert!(parse_err("via = Proxy, try_from dump").contains("expected `,`"));

        // Mixing `via` with schema-versioning options
        assert!(parse_err("via = Proxy, tag = \"v\"").contains("`tag` cannot be combined with `via`"));
        assert!(parse_err("tag = \"v\", via = Proxy").contains("`tag` cannot be combined with `via`"));
        assert!(
            parse_err("via = Proxy, content = \"d\"").contains("`content` cannot be combined with `via`")
        );
        assert!(
            parse_err("via = Proxy, versions(1: Self)").contains("`versions` cannot be combined with `via`")
        );
        assert!(parse_err("via = Proxy, external").contains("`external` cannot be combined with `via`"));
    }

    #[test]
    fn test_parse_errors_duplicates_and_fallible_conflicts() {
        assert!(parse_err("via = Proxy, via(Other)").contains("duplicate `via`"));
        assert!(parse_err("via = Proxy, try_from, try_from").contains("duplicate `try_from`"));
        assert!(parse_err("via = Proxy, try_into, try_into").contains("duplicate `try_into`"));
        assert!(parse_err("via = Proxy, fallible, fallible").contains("duplicate `fallible`"));
        assert!(parse_err("via = Proxy, from = a, from(b)").contains("duplicate `from`"));
        assert!(parse_err("via = Proxy, to(a), to = b").contains("duplicate `to`"));
        assert!(parse_err("via = Proxy, dump, dump").contains("duplicate `dump`"));

        assert!(parse_err("via = Proxy, fallible, try_from").contains("cannot be combined with `fallible`"));
        assert!(parse_err("via = Proxy, fallible, try_into").contains("cannot be combined with `fallible`"));
        assert!(
            parse_err("via = Proxy, try_from, fallible")
                .contains("cannot be combined with `try_from` or `try_into`")
        );
        assert!(
            parse_err("via = Proxy, try_into, fallible")
                .contains("cannot be combined with `try_from` or `try_into`")
        );
    }

    #[test]
    fn test_parse_errors_bad_fn_path_and_flag_values() {
        assert!(parse_err("via = Proxy, from").contains("expected `from = <path>` or `from(<path>)`"));
        assert!(parse_err("via = Proxy, to").contains("expected `to = <path>` or `to(<path>)`"));
        assert!(
            parse_err("via = Proxy, from(my_fn extra)").contains("unexpected trailing tokens in `from(...)`")
        );
        assert!(
            parse_err("via = Proxy, to(my_fn, extra)").contains("unexpected trailing tokens in `to(...)`")
        );

        for flag in ["fallible", "dump"] {
            let eq_err = parse_err(&format!("via = Proxy, {flag} = true"));
            assert!(
                eq_err.contains(&format!("`{flag}` does not take a value")),
                "unexpected error for `{flag} = true`: {eq_err}"
            );

            let paren_err = parse_err(&format!("via = Proxy, {flag}(true)"));
            assert!(
                paren_err.contains(&format!("`{flag}` does not take a value")),
                "unexpected error for `{flag}(true)`: {paren_err}"
            );
        }
    }

    fn render_via(args_str: &str, item_str: &str) -> String {
        let args = parse_via(args_str).unwrap();
        let item: syn::DeriveInput = syn::parse_str(item_str).unwrap();
        let ts = super::generate_via(&args, &item.ident, &item.generics, false);
        let parsed: syn::File = syn::parse2(ts.clone()).expect("generated code must be valid Rust syntax");
        assert_eq!(parsed.items.len(), 1);
        ts.to_string()
    }

    #[test]
    fn test_generate_via_default_infallible() {
        let rendered = render_via("via = Proxy", "struct Domain { val: u32 }");

        assert!(rendered.contains("impl :: backwards_compat :: BackwardsCompat for Domain"));
        assert!(rendered.contains("Proxy : :: backwards_compat :: BackwardsCompat"));
        assert!(rendered.contains(
            "impl < '__de > :: backwards_compat :: BackwardsCompatDeserialize < '__de > for Domain"
        ));
        assert!(rendered.contains("impl < '__de > :: serde :: Deserialize < '__de > for Domain"));
        assert!(rendered.contains("impl :: serde :: Serialize for Domain"));
        assert!(rendered.contains("Self : :: core :: convert :: From < Proxy >"));
        assert!(
            rendered.contains("< Self as :: core :: convert :: From < Proxy >> :: from (__migrated . value)")
        );
        assert!(rendered.contains("WireConvert as __WireConvert"));
        assert!(rendered.contains(
            "let __proxy : Proxy = __WireConvert :: < _ , Proxy > :: new (self) . convert_wire () ;"
        ));
        assert!(!rendered.contains("TryFrom"));
        assert!(!rendered.contains("TryWireConvert"));
    }

    #[test]
    fn test_generate_via_try_from() {
        let rendered = render_via("via = Proxy, try_from", "struct Domain { val: u32 }");

        assert!(rendered.contains("Self : :: core :: convert :: TryFrom < Proxy >"));
        assert!(rendered.contains(
            "< Self as :: core :: convert :: TryFrom < Proxy > > :: Error : :: core :: fmt :: Display"
        ));
        assert!(rendered.contains(
            "< Self as :: core :: convert :: TryFrom < Proxy >> :: try_from (__migrated . value) . map_err (:: serde :: de :: Error :: custom) ?"
        ));
        assert!(rendered.contains(
            "let __proxy : Proxy = __WireConvert :: < _ , Proxy > :: new (self) . convert_wire () ;"
        ));
        assert!(!rendered.contains("TryWireConvert"));
        assert!(!rendered.contains("serde :: ser :: Error :: custom"));
    }

    #[test]
    fn test_generate_via_try_into() {
        let rendered = render_via("via(Proxy), try_into", "struct Domain { val: u32 }");

        assert!(rendered.contains("Self : :: core :: convert :: From < Proxy >"));
        assert!(
            rendered.contains("< Self as :: core :: convert :: From < Proxy >> :: from (__migrated . value)")
        );
        assert!(rendered.contains("TryWireConvert as __TryWireConvert"));
        assert!(rendered.contains(
            "let __proxy : Proxy = __TryWireConvert :: < _ , Proxy > :: new (self) . try_convert_wire () . map_err (:: serde :: ser :: Error :: custom) ? ;"
        ));
        assert!(!rendered.contains("TryFrom"));
        assert!(!rendered.contains("serde :: de :: Error :: custom"));
    }

    #[test]
    fn test_generate_via_fallible() {
        let rendered = render_via("fallible, via = Proxy", "struct Domain { val: u32 }");

        assert!(rendered.contains("Self : :: core :: convert :: TryFrom < Proxy >"));
        assert!(rendered.contains(
            "< Self as :: core :: convert :: TryFrom < Proxy > > :: Error : :: core :: fmt :: Display"
        ));
        assert!(rendered.contains(
            "< Self as :: core :: convert :: TryFrom < Proxy >> :: try_from (__migrated . value) . map_err (:: serde :: de :: Error :: custom) ?"
        ));
        assert!(rendered.contains(
            "let __proxy : Proxy = __TryWireConvert :: < _ , Proxy > :: new (self) . try_convert_wire () . map_err (:: serde :: ser :: Error :: custom) ? ;"
        ));
        assert!(!rendered.contains("core :: convert :: From < Proxy >"));
        assert!(!rendered.contains("WireConvert as __WireConvert"));
    }

    #[test]
    fn test_generate_via_custom_from_and_to_fns() {
        let infallible = render_via(
            "via = Proxy, from = conv::from_proxy, to(conv::to_proxy)",
            "struct Domain { val: u32 }",
        );
        assert!(infallible.contains("(conv :: from_proxy) (__migrated . value)"));
        assert!(infallible.contains("let __proxy : Proxy = (conv :: to_proxy) (self) ;"));
        assert!(!infallible.contains("Self : :: core :: convert :: From"));
        assert!(!infallible.contains("Self : :: core :: convert :: TryFrom"));
        assert!(!infallible.contains("WireConvert"));
        assert!(!infallible.contains("TryWireConvert"));
        assert!(!infallible.contains("map_err"));

        let try_from_only = render_via(
            "via = Proxy, try_from, from = conv::try_from_proxy, to = conv::to_proxy",
            "struct Domain { val: u32 }",
        );
        assert!(try_from_only.contains(
            "(conv :: try_from_proxy) (__migrated . value) . map_err (:: serde :: de :: Error :: custom) ?"
        ));
        assert!(try_from_only.contains("let __proxy : Proxy = (conv :: to_proxy) (self) ;"));
        assert!(!try_from_only.contains("Self : :: core :: convert :: TryFrom"));
        assert!(!try_from_only.contains("serde :: ser :: Error :: custom"));

        let try_into_only = render_via(
            "via(Proxy), try_into, from(conv::from_proxy), to(conv::try_to_proxy)",
            "struct Domain { val: u32 }",
        );
        assert!(try_into_only.contains("(conv :: from_proxy) (__migrated . value)"));
        assert!(try_into_only.contains(
            "let __proxy : Proxy = (conv :: try_to_proxy) (self) . map_err (:: serde :: ser :: Error :: custom) ? ;"
        ));
        assert!(!try_into_only.contains("Self : :: core :: convert :: From"));
        assert!(!try_into_only.contains("serde :: de :: Error :: custom"));

        let fallible = render_via(
            "via = Proxy, fallible, from = conv::try_from_proxy, to = conv::try_to_proxy",
            "struct Domain { val: u32 }",
        );
        assert!(fallible.contains(
            "(conv :: try_from_proxy) (__migrated . value) . map_err (:: serde :: de :: Error :: custom) ?"
        ));
        assert!(fallible.contains(
            "let __proxy : Proxy = (conv :: try_to_proxy) (self) . map_err (:: serde :: ser :: Error :: custom) ? ;"
        ));
        assert!(!fallible.contains("Self : :: core :: convert :: TryFrom"));
        assert!(!fallible.contains("TryWireConvert"));
    }

    #[test]
    fn test_generate_via_generics_lifetimes_and_where_clause() {
        let rendered = render_via(
            "via = Proxy<'a, T>, fallible",
            "struct GenericDomain<'a, T: Clone> where T: core::fmt::Debug { name: &'a str, val: T }",
        );

        assert!(rendered.contains(
            "impl < 'a , T : Clone > :: backwards_compat :: BackwardsCompat for GenericDomain < 'a , T > where T : core :: fmt :: Debug , Proxy < 'a , T > : :: backwards_compat :: BackwardsCompat"
        ));
        assert!(rendered.contains(
            "impl < '__de : 'a , 'a , T : Clone > :: backwards_compat :: BackwardsCompatDeserialize < '__de > for GenericDomain < 'a , T > where T : core :: fmt :: Debug , Proxy < 'a , T > : :: backwards_compat :: BackwardsCompatDeserialize < '__de >"
        ));
        assert!(rendered.contains(
            "impl < '__de : 'a , 'a , T : Clone > :: serde :: Deserialize < '__de > for GenericDomain < 'a , T > where T : core :: fmt :: Debug"
        ));
        assert!(rendered.contains(
            "impl < 'a , T : Clone > :: serde :: Serialize for GenericDomain < 'a , T > where T : core :: fmt :: Debug , Proxy < 'a , T > : :: serde :: Serialize"
        ));
    }

    #[test]
    fn test_generate_via_dump_flag() {
        let rendered = render_via("via = Proxy, dump", "struct DumpDomain { val: u32 }");
        assert!(rendered.contains("impl :: backwards_compat :: BackwardsCompat for DumpDomain"));
        assert!(rendered.contains("impl :: serde :: Serialize for DumpDomain"));
        assert!(rendered.contains("const TAGGING : :: backwards_compat :: Tagging = < Proxy as :: backwards_compat :: BackwardsCompat > :: TAGGING"));
        assert!(rendered.contains("impl :: backwards_compat :: BackwardsCompatSerialize for DumpDomain"));
        assert!(rendered.contains("serialize_as_version (& __proxy , __version , __serializer ,)"));
    }

    #[cfg(feature = "schemars")]
    #[test]
    fn test_generate_via_schemars_delegation() {
        let args = parse_via("via = Proxy").unwrap();
        let item: syn::DeriveInput = syn::parse_str("struct Domain<T> { val: T }").unwrap();
        let ts = super::generate_via(&args, &item.ident, &item.generics, true);
        let rendered = ts.to_string();
        assert!(rendered.contains(
            "impl < T > :: backwards_compat :: __private :: schemars :: JsonSchema for Domain < T >"
        ));
        assert!(rendered.contains("Proxy : :: backwards_compat :: __private :: schemars :: JsonSchema"));
        assert!(rendered.contains(
            "< Proxy as :: backwards_compat :: __private :: schemars :: JsonSchema > :: json_schema (__gen)"
        ));
        syn::parse2::<syn::File>(ts).unwrap();
    }
}
