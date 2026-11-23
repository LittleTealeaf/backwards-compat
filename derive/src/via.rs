use syn::{
    ExprPath, Ident, Token, Type,
    parse::{Parse, ParseStream},
    spanned::Spanned as _,
};

/// Parsed arguments to the `#[backwards_compat_via(ProxyType, ...)]` attribute macro.
#[derive(Debug, Clone)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent parsed attribute flags on #[backwards_compat_via(...)]"
)]
pub struct ViaArgs {
    pub proxy_ty: Type,
    pub try_from: bool,
    pub try_into: bool,
    pub from_fn: Option<ExprPath>,
    pub to_fn: Option<ExprPath>,
    pub dump: bool,
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
        if input.is_empty() {
            return Err(syn::Error::new(
                input.span(),
                "expected a proxy type as the first argument to #[backwards_compat_via(...)]",
            ));
        }

        let proxy_ty: Type = input.parse()?;
        if matches!(&proxy_ty, Type::Path(tp) if tp.qself.is_none() && tp.path.is_ident("Self")) {
            return Err(syn::Error::new(
                proxy_ty.span(),
                "proxy type in #[backwards_compat_via(...)] cannot be `Self` (would cause infinite recursion)",
            ));
        }

        if !input.is_empty() {
            let _comma: Token![,] = input.parse()?;
        }

        let mut try_from = false;
        let mut try_into = false;
        let mut seen_try_from = false;
        let mut seen_try_into = false;
        let mut seen_fallible = false;
        let mut from_fn: Option<ExprPath> = None;
        let mut to_fn: Option<ExprPath> = None;
        let mut dump = false;

        while !input.is_empty() {
            let ident: Ident = input.parse()?;
            match ident.to_string().as_str() {
                "try_from" => {
                    reject_flag_value(input, "try_from")?;
                    if seen_fallible {
                        return Err(syn::Error::new(
                            ident.span(),
                            "`try_from` cannot be combined with `fallible` (which already enables both `try_from` and `try_into`)",
                        ));
                    }
                    if seen_try_from {
                        return Err(syn::Error::new(
                            ident.span(),
                            "duplicate `try_from` option in #[backwards_compat_via(...)]",
                        ));
                    }
                    seen_try_from = true;
                    try_from = true;
                }
                "try_into" => {
                    reject_flag_value(input, "try_into")?;
                    if seen_fallible {
                        return Err(syn::Error::new(
                            ident.span(),
                            "`try_into` cannot be combined with `fallible` (which already enables both `try_from` and `try_into`)",
                        ));
                    }
                    if seen_try_into {
                        return Err(syn::Error::new(
                            ident.span(),
                            "duplicate `try_into` option in #[backwards_compat_via(...)]",
                        ));
                    }
                    seen_try_into = true;
                    try_into = true;
                }
                "fallible" => {
                    reject_flag_value(input, "fallible")?;
                    if seen_fallible {
                        return Err(syn::Error::new(
                            ident.span(),
                            "duplicate `fallible` option in #[backwards_compat_via(...)]",
                        ));
                    }
                    if seen_try_from || seen_try_into {
                        return Err(syn::Error::new(
                            ident.span(),
                            "`fallible` cannot be combined with `try_from` or `try_into`",
                        ));
                    }
                    seen_fallible = true;
                    try_from = true;
                    try_into = true;
                }
                "from" => {
                    if from_fn.is_some() {
                        return Err(syn::Error::new(
                            ident.span(),
                            "duplicate `from` option in #[backwards_compat_via(...)]",
                        ));
                    }
                    from_fn = Some(parse_fn_path_option(input, "from", ident.span())?);
                }
                "to" => {
                    if to_fn.is_some() {
                        return Err(syn::Error::new(
                            ident.span(),
                            "duplicate `to` option in #[backwards_compat_via(...)]",
                        ));
                    }
                    to_fn = Some(parse_fn_path_option(input, "to", ident.span())?);
                }
                "dump" => {
                    reject_flag_value(input, "dump")?;
                    if dump {
                        return Err(syn::Error::new(
                            ident.span(),
                            "duplicate `dump` option in #[backwards_compat_via(...)]",
                        ));
                    }
                    dump = true;
                }
                other => {
                    return Err(syn::Error::new(
                        ident.span(),
                        format!(
                            "unknown option `{other}` in #[backwards_compat_via(...)]; expected one of: `try_from`, `try_into`, `fallible`, `from`, `to`, `dump`"
                        ),
                    ));
                }
            }

            if !input.is_empty() {
                let _comma: Token![,] = input.parse()?;
            }
        }

        Ok(Self {
            proxy_ty,
            try_from,
            try_into,
            from_fn,
            to_fn,
            dump,
        })
    }
}

fn make_where_clause(orig: Option<&syn::WhereClause>) -> syn::WhereClause {
    orig.cloned().unwrap_or_else(|| syn::WhereClause {
        where_token: syn::token::Where::default(),
        predicates: syn::punctuated::Punctuated::new(),
    })
}

/// Generates `BackwardsCompat`, `BackwardsCompatDeserialize`, `Deserialize`, and `Serialize`
/// implementations for `target_ident` delegating through `args.proxy_ty`.
pub fn generate_via(
    args: &ViaArgs,
    target_ident: &Ident,
    generics: &syn::Generics,
) -> proc_macro2::TokenStream {
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let target_ty = quote::quote!(#target_ident #ty_generics);
    let proxy_ty = &args.proxy_ty;

    let mut bc_where_clause = make_where_clause(where_clause);
    bc_where_clause
        .predicates
        .push(syn::parse_quote!(#proxy_ty: ::backwards_compat::BackwardsCompat));

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

    let code = quote::quote! {
        const _: () = {
            impl #impl_generics ::backwards_compat::BackwardsCompat for #target_ty #bc_where_clause {
                type Version = <#proxy_ty as ::backwards_compat::BackwardsCompat>::Version;
                const CURRENT_VERSION: Self::Version = <#proxy_ty as ::backwards_compat::BackwardsCompat>::CURRENT_VERSION;
                const SERIALIZE_VERSION: Self::Version = <#proxy_ty as ::backwards_compat::BackwardsCompat>::SERIALIZE_VERSION;
                const SUPPORTED_VERSIONS: &'static [Self::Version] = <#proxy_ty as ::backwards_compat::BackwardsCompat>::SUPPORTED_VERSIONS;
                const ACCEPTS_UNTAGGED: bool = <#proxy_ty as ::backwards_compat::BackwardsCompat>::ACCEPTS_UNTAGGED;
            }

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

            impl #impl_generics ::serde::Serialize for #target_ty #ser_where_clause {
                fn serialize<__S>(&self, __serializer: __S) -> ::core::result::Result<__S::Ok, __S::Error>
                where
                    __S: ::serde::Serializer,
                {
                    #to_proxy_stmt
                    ::serde::Serialize::serialize(&__proxy, __serializer)
                }
            }
        };
    };

    if args.dump {
        eprintln!(
            "=== backwards_compat_via output for {} ===\n{}\n==========================================",
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
        let simple = parse_via("Proxy").unwrap();
        assert_eq!(simple.proxy_ty.to_token_stream().to_string(), "Proxy");
        assert!(!simple.try_from);
        assert!(!simple.try_into);
        assert!(simple.from_fn.is_none());
        assert!(simple.to_fn.is_none());
        assert!(!simple.dump);

        let trailing = parse_via("Proxy,").unwrap();
        assert_eq!(trailing.proxy_ty.to_token_stream().to_string(), "Proxy");
        assert!(!trailing.try_from);
        assert!(!trailing.try_into);

        let generic = parse_via("crate::wire::Proxy<'a, T>").unwrap();
        assert_eq!(
            generic.proxy_ty.to_token_stream().to_string(),
            "crate :: wire :: Proxy < 'a , T >"
        );
    }

    #[test]
    fn test_parse_fallibility_and_dump_flags() {
        let tf = parse_via("Proxy, try_from").unwrap();
        assert!(tf.try_from);
        assert!(!tf.try_into);
        assert!(!tf.dump);

        let ti = parse_via("Proxy, try_into,").unwrap();
        assert!(!ti.try_from);
        assert!(ti.try_into);

        let both = parse_via("Proxy, try_from, try_into").unwrap();
        assert!(both.try_from);
        assert!(both.try_into);

        let both_rev = parse_via("Proxy, try_into, try_from").unwrap();
        assert!(both_rev.try_from);
        assert!(both_rev.try_into);

        let fallible = parse_via("Proxy, fallible").unwrap();
        assert!(fallible.try_from);
        assert!(fallible.try_into);

        let dump = parse_via("Proxy, dump").unwrap();
        assert!(dump.dump);
        assert!(!dump.try_from);
        assert!(!dump.try_into);
    }

    #[test]
    fn test_parse_from_to_fn_paths_and_arbitrary_order() {
        let eq_style = parse_via("Proxy, from = crate::conv::decode, to = crate::conv::encode").unwrap();
        assert_eq!(
            eq_style.from_fn.unwrap().to_token_stream().to_string(),
            "crate :: conv :: decode"
        );
        assert_eq!(
            eq_style.to_fn.unwrap().to_token_stream().to_string(),
            "crate :: conv :: encode"
        );

        let paren_style = parse_via("Proxy, from(decode_fn), to(encode_fn)").unwrap();
        assert_eq!(
            paren_style.from_fn.unwrap().to_token_stream().to_string(),
            "decode_fn"
        );
        assert_eq!(
            paren_style.to_fn.unwrap().to_token_stream().to_string(),
            "encode_fn"
        );

        let mixed =
            parse_via("crate::Proxy<'a, T>, dump, to(to_proxy), try_from, from = from_proxy,").unwrap();
        assert_eq!(
            mixed.proxy_ty.to_token_stream().to_string(),
            "crate :: Proxy < 'a , T >"
        );
        assert!(mixed.dump);
        assert!(mixed.try_from);
        assert!(!mixed.try_into);
        assert_eq!(mixed.from_fn.unwrap().to_token_stream().to_string(), "from_proxy");
        assert_eq!(mixed.to_fn.unwrap().to_token_stream().to_string(), "to_proxy");

        let fallible_combo = parse_via("Proxy, to = t_fn, fallible, dump, from(f_fn),").unwrap();
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
    fn test_parse_errors_empty_self_unknown_and_missing_comma() {
        assert!(parse_err("").contains("expected a proxy type"));
        assert!(parse_err("Self").contains("cannot be `Self`"));
        assert!(parse_err("Self, dump").contains("cannot be `Self`"));

        let unknown = parse_err("Proxy, bogus");
        assert!(unknown.contains("unknown option `bogus`"));

        assert!(parse_err("Proxy try_from").contains("expected `,`"));
        assert!(parse_err("Proxy, try_from dump").contains("expected `,`"));
    }

    #[test]
    fn test_parse_errors_duplicates_and_fallible_conflicts() {
        assert!(parse_err("Proxy, try_from, try_from").contains("duplicate `try_from`"));
        assert!(parse_err("Proxy, try_into, try_into").contains("duplicate `try_into`"));
        assert!(parse_err("Proxy, fallible, fallible").contains("duplicate `fallible`"));
        assert!(parse_err("Proxy, from = a, from(b)").contains("duplicate `from`"));
        assert!(parse_err("Proxy, to(a), to = b").contains("duplicate `to`"));
        assert!(parse_err("Proxy, dump, dump").contains("duplicate `dump`"));

        assert!(parse_err("Proxy, fallible, try_from").contains("cannot be combined with `fallible`"));
        assert!(parse_err("Proxy, fallible, try_into").contains("cannot be combined with `fallible`"));
        assert!(
            parse_err("Proxy, try_from, fallible")
                .contains("cannot be combined with `try_from` or `try_into`")
        );
        assert!(
            parse_err("Proxy, try_into, fallible")
                .contains("cannot be combined with `try_from` or `try_into`")
        );
    }

    #[test]
    fn test_parse_errors_bad_fn_path_and_flag_values() {
        assert!(parse_err("Proxy, from").contains("expected `from = <path>` or `from(<path>)`"));
        assert!(parse_err("Proxy, to").contains("expected `to = <path>` or `to(<path>)`"));
        assert!(parse_err("Proxy, from(my_fn extra)").contains("unexpected trailing tokens in `from(...)`"));
        assert!(parse_err("Proxy, to(my_fn, extra)").contains("unexpected trailing tokens in `to(...)`"));

        for flag in ["try_from", "try_into", "fallible", "dump"] {
            let eq_err = parse_err(&format!("Proxy, {flag} = true"));
            assert!(
                eq_err.contains(&format!("`{flag}` does not take a value")),
                "unexpected error for `{flag} = true`: {eq_err}"
            );

            let paren_err = parse_err(&format!("Proxy, {flag}(true)"));
            assert!(
                paren_err.contains(&format!("`{flag}` does not take a value")),
                "unexpected error for `{flag}(true)`: {paren_err}"
            );
        }
    }

    fn render_via(args_str: &str, item_str: &str) -> String {
        let args = parse_via(args_str).unwrap();
        let item: syn::DeriveInput = syn::parse_str(item_str).unwrap();
        let ts = super::generate_via(&args, &item.ident, &item.generics);
        let parsed: syn::File = syn::parse2(ts.clone()).expect("generated code must be valid Rust syntax");
        assert_eq!(parsed.items.len(), 1);
        ts.to_string()
    }

    #[test]
    fn test_generate_via_default_infallible() {
        let rendered = render_via("Proxy", "struct Domain { val: u32 }");

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
        let rendered = render_via("Proxy, try_from", "struct Domain { val: u32 }");

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
        let rendered = render_via("Proxy, try_into", "struct Domain { val: u32 }");

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
        let rendered = render_via("Proxy, fallible", "struct Domain { val: u32 }");

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
            "Proxy, from = conv::from_proxy, to(conv::to_proxy)",
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
            "Proxy, try_from, from = conv::try_from_proxy, to = conv::to_proxy",
            "struct Domain { val: u32 }",
        );
        assert!(try_from_only.contains(
            "(conv :: try_from_proxy) (__migrated . value) . map_err (:: serde :: de :: Error :: custom) ?"
        ));
        assert!(try_from_only.contains("let __proxy : Proxy = (conv :: to_proxy) (self) ;"));
        assert!(!try_from_only.contains("Self : :: core :: convert :: TryFrom"));
        assert!(!try_from_only.contains("serde :: ser :: Error :: custom"));

        let try_into_only = render_via(
            "Proxy, try_into, from(conv::from_proxy), to(conv::try_to_proxy)",
            "struct Domain { val: u32 }",
        );
        assert!(try_into_only.contains("(conv :: from_proxy) (__migrated . value)"));
        assert!(try_into_only.contains(
            "let __proxy : Proxy = (conv :: try_to_proxy) (self) . map_err (:: serde :: ser :: Error :: custom) ? ;"
        ));
        assert!(!try_into_only.contains("Self : :: core :: convert :: From"));
        assert!(!try_into_only.contains("serde :: de :: Error :: custom"));

        let fallible = render_via(
            "Proxy, fallible, from = conv::try_from_proxy, to = conv::try_to_proxy",
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
            "Proxy<'a, T>, fallible",
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
        let rendered = render_via("Proxy, dump", "struct DumpDomain { val: u32 }");
        assert!(rendered.contains("impl :: backwards_compat :: BackwardsCompat for DumpDomain"));
        assert!(rendered.contains("impl :: serde :: Serialize for DumpDomain"));
    }
}
