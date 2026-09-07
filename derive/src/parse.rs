use syn::{
    Attribute, Ident, Token, Type, Visibility, braced,
    parse::{Parse, ParseStream},
};

/// Represents a version tag, which can be either an integer or a string literal.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum VersionTag {
    Int(u64),
    String(String),
}

impl VersionTag {
    /// Returns the string representation of the version tag.
    pub fn to_tag_string(&self) -> String {
        match self {
            VersionTag::Int(i) => i.to_string(),
            VersionTag::String(s) => s.clone(),
        }
    }

    /// Checks if two version tags represent the same tag value (either exact match or matching string representation).
    pub fn matches(&self, other: &VersionTag) -> bool {
        self == other || self.to_tag_string() == other.to_tag_string()
    }

    /// Helper to construct a compile error associated with a specific span.
    #[allow(dead_code)]
    pub fn error(&self, span: proc_macro2::Span, msg: impl std::fmt::Display) -> syn::Error {
        syn::Error::new(span, msg)
    }

    /// Parse a VersionTag from a ParseStream, returning the tag and its span.
    pub fn parse(input: ParseStream) -> syn::Result<(Self, proc_macro2::Span)> {
        if input.peek(syn::Ident) {
            return Err(syn::Error::new(
                input.span(),
                "version keys must be string or integer literals (e.g. 1 or \"1\"), not identifiers like v1",
            ));
        }

        if input.peek(syn::LitInt) {
            let lit: syn::LitInt = input.parse()?;
            let val = lit.base10_parse::<u64>()?;
            Ok((VersionTag::Int(val), lit.span()))
        } else if input.peek(syn::LitStr) {
            let lit: syn::LitStr = input.parse()?;
            Ok((VersionTag::String(lit.value()), lit.span()))
        } else {
            Err(syn::Error::new(
                input.span(),
                "expected integer or string literal for version tag",
            ))
        }
    }
}

impl std::fmt::Display for VersionTag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VersionTag::Int(i) => write!(f, "{}", i),
            VersionTag::String(s) => write!(f, "{}", s),
        }
    }
}

impl quote::ToTokens for VersionTag {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        match self {
            VersionTag::Int(i) => {
                let lit = syn::LitInt::new(&i.to_string(), proc_macro2::Span::call_site());
                lit.to_tokens(tokens);
            }
            VersionTag::String(s) => {
                let lit = syn::LitStr::new(s, proc_macro2::Span::call_site());
                lit.to_tokens(tokens);
            }
        }
    }
}

/// Represents a single version entry inside the compat macro body.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct VersionEntry {
    pub outer_attrs: Vec<Attribute>,
    pub tag: VersionTag,
    pub tag_span: proc_macro2::Span,
    pub ty: Type,
    pub fallible: bool,
    pub explicit_next: Option<(VersionTag, proc_macro2::Span)>,
}

/// Represents the parsed input of a `backwards_compat!` macro invocation.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct BackwardsCompatInput {
    pub outer_attrs: Vec<Attribute>,
    pub vis: Visibility,
    pub target_ty: Type,
    pub tag_field: String,
    pub current_version: VersionTag,
    pub current_version_span: proc_macro2::Span,
    pub error_ty: Option<Type>,
    pub dump: bool,
    pub versions: Vec<VersionEntry>,
}

impl Parse for BackwardsCompatInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut tag_field = "version".to_string();
        let mut current_version_opt: Option<(VersionTag, proc_macro2::Span)> = None;
        let mut error_ty = None;
        let mut dump = false;
        let mut forwarded_attrs = Vec::new();

        while input.peek(Token![#]) {
            input.parse::<Token![#]>()?;
            let content;
            syn::bracketed!(content in input);
            while !content.is_empty() {
                if content.peek(Ident) {
                    let fork = content.fork();
                    let ident: Ident = fork.parse()?;
                    let ident_str = ident.to_string();
                    if ident_str == "tag" {
                        content.parse::<Ident>()?;
                        if content.peek(Token![=]) {
                            content.parse::<Token![=]>()?;
                            let s: syn::LitStr = content.parse()?;
                            tag_field = s.value();
                        } else if content.peek(syn::token::Paren) {
                            let inner;
                            syn::parenthesized!(inner in content);
                            let s: syn::LitStr = inner.parse()?;
                            tag_field = s.value();
                        } else {
                            tag_field = "version".to_string();
                        }
                    } else if ident_str == "version" {
                        content.parse::<Ident>()?;
                        if current_version_opt.is_some() {
                            return Err(syn::Error::new(
                                ident.span(),
                                "duplicate #[version = ...] attribute",
                            ));
                        }
                        if content.peek(Token![=]) {
                            content.parse::<Token![=]>()?;
                            let (v, span) = VersionTag::parse(&content)?;
                            current_version_opt = Some((v, span));
                        } else if content.peek(syn::token::Paren) {
                            let inner;
                            syn::parenthesized!(inner in content);
                            let (v, span) = VersionTag::parse(&inner)?;
                            current_version_opt = Some((v, span));
                        } else {
                            return Err(syn::Error::new(
                                ident.span(),
                                "#[version] attribute requires a value, e.g. #[version = 1] or #[version = \"1\"]",
                            ));
                        }
                    } else if ident_str == "error" {
                        content.parse::<Ident>()?;
                        if content.peek(Token![=]) {
                            content.parse::<Token![=]>()?;
                            let ty: Type = content.parse()?;
                            error_ty = Some(ty);
                        } else if content.peek(syn::token::Paren) {
                            let inner;
                            syn::parenthesized!(inner in content);
                            let ty: Type = inner.parse()?;
                            error_ty = Some(ty);
                        } else {
                            return Err(syn::Error::new(
                                ident.span(),
                                "expected type for error attribute, e.g. #[error = CustomError]",
                            ));
                        }
                    } else if ident_str == "dump" {
                        content.parse::<Ident>()?;
                        dump = true;
                    } else if ident_str == "untagged" {
                        return Err(syn::Error::new(
                            ident.span(),
                            "#[untagged] is obsolete and no longer supported",
                        ));
                    } else if ident_str == "tagged" {
                        return Err(syn::Error::new(
                            ident.span(),
                            "#[tagged] is obsolete and no longer supported",
                        ));
                    } else if ident_str == "untagged_enum" || ident_str == "untagged_name" {
                        return Err(syn::Error::new(
                            ident.span(),
                            "#[untagged_enum] is obsolete and no longer supported",
                        ));
                    } else if ident_str == "target" {
                        return Err(syn::Error::new(
                            ident.span(),
                            "#[target] is obsolete; specify the target type with `compat TargetType` syntax",
                        ));
                    } else if ident_str == "content" {
                        return Err(syn::Error::new(
                            ident.span(),
                            "#[content] is obsolete and no longer supported",
                        ));
                    } else {
                        let meta: syn::Meta = content.parse()?;
                        forwarded_attrs.push(syn::parse_quote!(#[#meta]));
                    }
                } else {
                    let meta: syn::Meta = content.parse()?;
                    forwarded_attrs.push(syn::parse_quote!(#[#meta]));
                }

                if content.peek(Token![,]) {
                    content.parse::<Token![,]>()?;
                }
            }
        }

        let vis: Visibility = input.parse()?;

        if input.peek(Token![enum]) {
            return Err(syn::Error::new(
                input.span(),
                "expected `compat`, found `enum`. The macro syntax has changed to `compat TargetModel { ... }`",
            ));
        }

        let compat_ident: Ident = input.parse()?;
        if compat_ident != "compat" {
            return Err(syn::Error::new(
                compat_ident.span(),
                format!("expected `compat`, found `{}`", compat_ident),
            ));
        }

        let target_ty: Type = input.parse()?;

        let content_stream;
        braced!(content_stream in input);

        let mut versions = Vec::new();
        while !content_stream.is_empty() {
            let outer_attrs = content_stream.call(Attribute::parse_outer)?;
            let mut fallible = false;
            let mut forwarded_version_attrs = Vec::new();

            for attr in outer_attrs {
                if attr.path().is_ident("fallible") || attr.path().is_ident("try_into") {
                    fallible = true;
                } else {
                    forwarded_version_attrs.push(attr);
                }
            }

            let (tag, tag_span) = VersionTag::parse(&content_stream)?;

            content_stream.parse::<Token![:]>()?;

            let ty: Type = content_stream.parse()?;

            let mut explicit_next = None;
            if content_stream.peek(Token![=>]) {
                content_stream.parse::<Token![=>]>()?;
                let (next_tag, next_span) = VersionTag::parse(&content_stream)?;
                explicit_next = Some((next_tag, next_span));
            }

            while content_stream.peek(Token![,]) || content_stream.peek(Token![;]) {
                if content_stream.peek(Token![,]) {
                    content_stream.parse::<Token![,]>()?;
                } else {
                    content_stream.parse::<Token![;]>()?;
                }
            }

            versions.push(VersionEntry {
                outer_attrs: forwarded_version_attrs,
                tag,
                tag_span,
                ty,
                fallible,
                explicit_next,
            });
        }

        if versions.is_empty() {
            return Err(syn::Error::new_spanned(
                &target_ty,
                "at least one version entry must be declared",
            ));
        }

        let (current_version, current_version_span) = match current_version_opt {
            Some((v, span)) => {
                let matches_declared = versions.iter().any(|entry| entry.tag.matches(&v));
                let matches_explicit_next = versions.iter().any(|entry| {
                    entry
                        .explicit_next
                        .as_ref()
                        .map(|(next_tag, _)| next_tag.matches(&v))
                        .unwrap_or(false)
                });
                let is_implicit_terminal = versions
                    .last()
                    .map(|entry| entry.explicit_next.is_none())
                    .unwrap_or(false);

                if !matches_declared && !matches_explicit_next && !is_implicit_terminal {
                    return Err(syn::Error::new(
                        span,
                        format!("version `{}` does not match any declared version", v),
                    ));
                }
                (v, span)
            }
            None => {
                let last = versions.last().unwrap();
                (last.tag.clone(), last.tag_span)
            }
        };

        // Validation on parse:
        // Ensure no duplicate version tags among declared versions.
        for (i, entry) in versions.iter().enumerate() {
            for prev in &versions[..i] {
                if entry.tag.matches(&prev.tag) {
                    return Err(syn::Error::new(
                        entry.tag_span,
                        format!("duplicate version tag `{}`", entry.tag),
                    ));
                }
            }
        }

        Ok(BackwardsCompatInput {
            outer_attrs: forwarded_attrs,
            vis,
            target_ty,
            tag_field,
            current_version,
            current_version_span,
            error_ty,
            dump,
            versions,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_compat_with_integers() {
        let input = quote::quote! {
            #[version = 3]
            pub compat TargetModel {
                1: ModelV1 => 2,
                2: ModelV2 => 3,
            }
        };
        let parsed: BackwardsCompatInput = syn::parse2(input).unwrap();
        assert_eq!(parsed.tag_field, "version");
        assert_eq!(parsed.current_version, VersionTag::Int(3));
        assert_eq!(parsed.versions.len(), 2);
        assert_eq!(parsed.versions[0].tag, VersionTag::Int(1));
        assert_eq!(
            parsed.versions[0].explicit_next.as_ref().unwrap().0,
            VersionTag::Int(2)
        );
        assert_eq!(parsed.versions[1].tag, VersionTag::Int(2));
        assert_eq!(
            parsed.versions[1].explicit_next.as_ref().unwrap().0,
            VersionTag::Int(3)
        );
        assert!(!parsed.dump);
        assert!(parsed.error_ty.is_none());
    }

    #[test]
    fn test_parse_valid_compat_with_strings() {
        let input = quote::quote! {
            #[tag = "schema"]
            #[version = "v3"]
            #[dump]
            compat TargetModel {
                "v1": ModelV1 => "v2",
                #[fallible]
                "v2": ModelV2 => "v3",
            }
        };
        let parsed: BackwardsCompatInput = syn::parse2(input).unwrap();
        assert_eq!(parsed.tag_field, "schema");
        assert_eq!(parsed.current_version, VersionTag::String("v3".to_string()));
        assert_eq!(parsed.versions.len(), 2);
        assert_eq!(parsed.versions[0].tag, VersionTag::String("v1".to_string()));
        assert_eq!(
            parsed.versions[0].explicit_next.as_ref().unwrap().0,
            VersionTag::String("v2".to_string())
        );
        assert!(!parsed.versions[0].fallible);
        assert_eq!(parsed.versions[1].tag, VersionTag::String("v2".to_string()));
        assert!(parsed.versions[1].fallible);
        assert!(parsed.dump);
    }

    #[test]
    fn test_parse_error_ty() {
        let input = quote::quote! {
            #[version = 2]
            #[error = MyError]
            pub compat TargetModel {
                1: ModelV1,
            }
        };
        let parsed: BackwardsCompatInput = syn::parse2(input).unwrap();
        assert!(parsed.error_ty.is_some());
    }

    #[test]
    fn test_default_current_version() {
        let input = quote::quote! {
            pub compat TargetModel {
                1: ModelV1,
                2: ModelV2,
            }
        };
        let parsed = syn::parse2::<BackwardsCompatInput>(input).unwrap();
        assert_eq!(parsed.current_version, VersionTag::Int(2));
    }

    #[test]
    fn test_empty_versions_error() {
        let input = quote::quote! {
            compat TargetModel {}
        };
        let err = syn::parse2::<BackwardsCompatInput>(input).unwrap_err();
        assert_eq!(err.to_string(), "at least one version entry must be declared");
    }

    #[test]
    fn test_duplicate_version_tag_error() {
        let input = quote::quote! {
            #[version = 3]
            compat TargetModel {
                1: ModelV1,
                1: ModelV1Dup,
            }
        };
        let err = syn::parse2::<BackwardsCompatInput>(input).unwrap_err();
        assert_eq!(err.to_string(), "duplicate version tag `1`");
    }

    #[test]
    fn test_version_matches_current_allowed() {
        let input = quote::quote! {
            #[version = 3]
            compat TargetModel {
                1: ModelV1,
                3: ModelV3,
            }
        };
        let parsed = syn::parse2::<BackwardsCompatInput>(input).unwrap();
        assert_eq!(parsed.current_version, VersionTag::Int(3));
        assert_eq!(parsed.versions.len(), 2);
    }

    #[test]
    fn test_invalid_current_version_error() {
        let input = quote::quote! {
            #[version = 99]
            compat TargetModel {
                1: ModelV1 => 2,
                2: ModelV2 => 1,
            }
        };
        let err = syn::parse2::<BackwardsCompatInput>(input).unwrap_err();
        assert_eq!(
            err.to_string(),
            "version `99` does not match any declared version"
        );
    }

    #[test]
    fn test_ident_version_key_error() {
        let input = quote::quote! {
            #[version = 3]
            compat TargetModel {
                v1: ModelV1,
            }
        };
        let err = syn::parse2::<BackwardsCompatInput>(input).unwrap_err();
        assert_eq!(
            err.to_string(),
            "version keys must be string or integer literals (e.g. 1 or \"1\"), not identifiers like v1"
        );
    }

    #[test]
    fn test_obsolete_attributes_error() {
        let obsolete_attrs = vec![
            quote::quote!(#[untagged]),
            quote::quote!(#[tagged = "foo"]),
            quote::quote!(#[untagged_enum]),
            quote::quote!(#[target = Foo]),
            quote::quote!(#[content = "bar"]),
        ];

        for attr in obsolete_attrs {
            let input = quote::quote! {
                #attr
                #[version = 2]
                compat TargetModel {
                    1: ModelV1,
                }
            };
            assert!(
                syn::parse2::<BackwardsCompatInput>(input).is_err(),
                "Expected error for obsolete attribute"
            );
        }
    }
}
