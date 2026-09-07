use syn::{
    Attribute, Expr, ExprLit, Ident, Lit, Meta, Token, Type, Visibility, braced,
    parse::{Parse, ParseStream},
};

#[derive(Debug, Clone)]
pub struct VersionEntry {
    pub outer_attrs: Vec<Attribute>,
    pub ident: Ident,
    pub ty: Type,
    pub tag: String,
    pub untagged: bool,
    pub untagged_only: bool,
    pub fallible: bool,
    pub latest: bool,
}

#[derive(Debug, Clone)]
pub struct BackwardsCompatInput {
    pub outer_attrs: Vec<Attribute>,
    pub vis: Visibility,
    pub enum_ident: Ident,
    pub tag: Option<String>,
    pub content: Option<String>,
    pub untagged_enum_level: bool,
    pub target: Option<Type>,
    pub error: Option<Type>,
    pub tagged_ident: Option<Ident>,
    pub untagged_ident: Option<Ident>,
    pub dump: bool,
    pub versions: Vec<VersionEntry>,
}

impl Parse for BackwardsCompatInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let raw_attrs = input.call(Attribute::parse_outer)?;

        let mut tag = Some("version".to_string());
        let mut content = None;
        let mut untagged_enum_level = false;
        let mut target = None;
        let mut error = None;
        let mut tagged_ident = None;
        let mut untagged_ident = None;
        let mut dump = false;
        let mut forwarded_attrs = Vec::new();

        for attr in raw_attrs {
            if attr.path().is_ident("tag") {
                match &attr.meta {
                    Meta::NameValue(nv) => {
                        if let Expr::Lit(ExprLit {
                            lit: Lit::Str(s), ..
                        }) = &nv.value
                        {
                            let val = s.value();
                            if val == "none" || val == "untagged" {
                                untagged_enum_level = true;
                                tag = None;
                            } else {
                                tag = Some(val);
                            }
                        } else {
                            return Err(syn::Error::new_spanned(
                                &nv.value,
                                "expected string literal for tag, e.g. #[tag = \"version\"]",
                            ));
                        }
                    }
                    Meta::Path(_) => {
                        // #[tag] without value
                        tag = Some("version".to_string());
                    }
                    Meta::List(list) => {
                        let s: syn::LitStr = list.parse_args()?;
                        tag = Some(s.value());
                    }
                }
            } else if attr.path().is_ident("content") {
                if let Meta::NameValue(nv) = &attr.meta
                    && let Expr::Lit(ExprLit {
                        lit: Lit::Str(s), ..
                    }) = &nv.value
                {
                    content = Some(s.value());
                }
            } else if attr.path().is_ident("untagged") {
                untagged_enum_level = true;
                tag = None;
            } else if attr.path().is_ident("dump") {
                dump = true;
            } else if attr.path().is_ident("target") {
                parse_target_attr(&attr, &mut target, &mut error)?;
            } else if attr.path().is_ident("error") {
                if let Meta::NameValue(nv) = &attr.meta {
                    let ty_str = quote::quote!(#nv.value).to_string();
                    let parsed_ty: Type = syn::parse_str(&ty_str)?;
                    error = Some(parsed_ty);
                } else if let Meta::List(list) = &attr.meta {
                    let parsed_ty: Type = list.parse_args()?;
                    error = Some(parsed_ty);
                }
            } else if attr.path().is_ident("tagged") {
                if let Meta::NameValue(nv) = &attr.meta {
                    if let Expr::Path(ep) = &nv.value {
                        tagged_ident = ep.path.get_ident().cloned();
                    }
                } else if let Meta::List(list) = &attr.meta {
                    let id: Ident = list.parse_args()?;
                    tagged_ident = Some(id);
                }
            } else if attr.path().is_ident("untagged_enum") || attr.path().is_ident("untagged_name")
            {
                if let Meta::NameValue(nv) = &attr.meta {
                    if let Expr::Path(ep) = &nv.value {
                        untagged_ident = ep.path.get_ident().cloned();
                    }
                } else if let Meta::List(list) = &attr.meta {
                    let id: Ident = list.parse_args()?;
                    untagged_ident = Some(id);
                }
            } else {
                forwarded_attrs.push(attr);
            }
        }

        let vis: Visibility = input.parse()?;

        // Optional `enum` keyword
        if input.peek(Token![enum]) {
            input.parse::<Token![enum]>()?;
        }

        let enum_ident: Ident = input.parse()?;

        let content_stream;
        braced!(content_stream in input);

        let mut versions = Vec::new();
        while !content_stream.is_empty() {
            let version_attrs = content_stream.call(Attribute::parse_outer)?;

            let mut ver_tag = None;
            let mut untagged = false;
            let mut untagged_only = false;
            let mut fallible = false;
            let mut latest = false;
            let mut ver_forwarded_attrs = Vec::new();

            for attr in version_attrs {
                if attr.path().is_ident("tag") || attr.path().is_ident("version") {
                    match &attr.meta {
                        Meta::NameValue(nv) => {
                            if let Expr::Lit(ExprLit { lit, .. }) = &nv.value {
                                match lit {
                                    Lit::Str(s) => ver_tag = Some(s.value()),
                                    Lit::Int(i) => ver_tag = Some(i.to_string()),
                                    _ => {}
                                }
                            }
                        }
                        Meta::List(list) => {
                            let s: syn::LitStr = list.parse_args()?;
                            ver_tag = Some(s.value());
                        }
                        _ => {}
                    }
                } else if attr.path().is_ident("untagged") {
                    untagged = true;
                } else if attr.path().is_ident("untagged_only") {
                    untagged = true;
                    untagged_only = true;
                } else if attr.path().is_ident("fallible") || attr.path().is_ident("try_into") {
                    fallible = true;
                } else if attr.path().is_ident("latest") {
                    latest = true;
                } else {
                    ver_forwarded_attrs.push(attr);
                }
            }

            let ident: Ident = content_stream.parse()?;

            let ty: Type = if content_stream.peek(Token![=]) {
                content_stream.parse::<Token![=]>()?;
                content_stream.parse()?
            } else if content_stream.peek(syn::token::Paren) {
                let paren_stream;
                syn::parenthesized!(paren_stream in content_stream);
                paren_stream.parse()?
            } else {
                return Err(
                    content_stream.error("expected `=` or `(Type)` after version identifier")
                );
            };

            // Optional trailing comma or semicolon
            if content_stream.peek(Token![,]) {
                content_stream.parse::<Token![,]>()?;
            } else if content_stream.peek(Token![;]) {
                content_stream.parse::<Token![;]>()?;
            }

            // Derive default tag from ident if not specified
            let tag_val = ver_tag.unwrap_or_else(|| {
                let s = ident.to_string();
                // If it starts with 'v' or 'V' followed by digits, e.g. "v1" or "V5", strip the 'v'
                if (s.starts_with('v') || s.starts_with('V'))
                    && s.len() > 1
                    && s[1..].chars().all(|c| c.is_ascii_digit())
                {
                    s[1..].to_string()
                } else {
                    s
                }
            });

            versions.push(VersionEntry {
                outer_attrs: ver_forwarded_attrs,
                ident,
                ty,
                tag: tag_val,
                untagged,
                untagged_only,
                fallible,
                latest,
            });
        }

        if versions.is_empty() {
            return Err(syn::Error::new_spanned(
                &enum_ident,
                "backwards_compat enum must define at least one version",
            ));
        }

        // Mark the last version as latest if none was explicitly marked
        if !versions.iter().any(|v| v.latest)
            && let Some(last) = versions.last_mut()
        {
            last.latest = true;
        }

        Ok(BackwardsCompatInput {
            outer_attrs: forwarded_attrs,
            vis,
            enum_ident,
            tag,
            content,
            untagged_enum_level,
            target,
            error,
            tagged_ident,
            untagged_ident,
            dump,
            versions,
        })
    }
}

fn parse_target_attr(
    attr: &Attribute,
    target: &mut Option<Type>,
    error: &mut Option<Type>,
) -> syn::Result<()> {
    match &attr.meta {
        Meta::NameValue(nv) => {
            let ty_str = quote::quote!(#nv.value).to_string();
            let parsed_ty: Type = syn::parse_str(&ty_str)?;
            *target = Some(parsed_ty);
        }
        Meta::List(list) => {
            list.parse_args_with(|input: ParseStream| {
                while !input.is_empty() {
                    if input.peek(Ident) && input.peek2(Token![=]) {
                        let key: Ident = input.parse()?;
                        input.parse::<Token![=]>()?;
                        let ty: Type = input.parse()?;
                        if key == "target" {
                            *target = Some(ty);
                        } else if key == "error" {
                            *error = Some(ty);
                        }
                    } else {
                        let ty: Type = input.parse()?;
                        *target = Some(ty);
                    }
                    if input.peek(Token![,]) {
                        input.parse::<Token![,]>()?;
                    }
                }
                Ok(())
            })?;
        }
        Meta::Path(_) => {}
    }
    Ok(())
}
