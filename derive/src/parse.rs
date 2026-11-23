use quote::ToTokens as _;
use syn::{
    Attribute, Ident, Token, Type, Visibility,
    parse::{Parse, ParseStream},
    spanned::Spanned as _,
};

/// Represents a version tag, which can be unversioned (`_`), an integer, or a string literal.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum VersionTag {
    Untagged,
    Int(u64),
    String(String),
}

impl VersionTag {
    /// Returns true if this version tag represents the unversioned (`_`) pre-backwards-compat entry.
    pub const fn is_untagged(&self) -> bool {
        matches!(self, Self::Untagged)
    }

    /// Returns the string representation of the version tag.
    #[allow(dead_code, reason = "helper for formatting version tags")]
    pub fn to_tag_string(&self) -> String {
        match self {
            Self::Untagged => "_".to_owned(),
            Self::Int(i) => i.to_string(),
            Self::String(s) => s.clone(),
        }
    }

    /// Checks if two version tags are strictly identical (`1` and `"1"` are distinct).
    pub fn matches(&self, other: &Self) -> bool {
        self == other
    }

    /// Helper to construct a compile error associated with a specific span.
    #[allow(dead_code, reason = "helper function for error reporting")]
    pub fn error<D: core::fmt::Display>(span: proc_macro2::Span, msg: D) -> syn::Error {
        syn::Error::new(span, msg)
    }

    /// Parse a concrete (`Int` or `String`) `VersionTag` from a `ParseStream`, disallowing `_`.
    pub fn parse_concrete(input: ParseStream, context: &str) -> syn::Result<(Self, proc_macro2::Span)> {
        let (tag, span) = Self::parse(input)?;
        if tag.is_untagged() {
            return Err(syn::Error::new(
                span,
                format!("unversioned `_` cannot be used as {context}"),
            ));
        }
        Ok((tag, span))
    }

    /// Parse a `VersionTag` from a `ParseStream`, returning the tag and its span.
    pub fn parse(input: ParseStream) -> syn::Result<(Self, proc_macro2::Span)> {
        if input.peek(Token![_]) {
            let underscore: Token![_] = input.parse()?;
            return Ok((Self::Untagged, underscore.span));
        }

        if input.peek(syn::Ident) {
            let ident: Ident = input.parse()?;
            return Err(syn::Error::new(
                ident.span(),
                "version keys must be string or integer literals (e.g. 1 or \"1\"), or `_` for unversioned, not identifiers like v1",
            ));
        }

        if input.peek(syn::LitInt) {
            let lit: syn::LitInt = input.parse()?;
            let val = lit.base10_parse::<u64>()?;
            Ok((Self::Int(val), lit.span()))
        } else if input.peek(syn::LitStr) {
            let lit: syn::LitStr = input.parse()?;
            Ok((Self::String(lit.value()), lit.span()))
        } else {
            Err(syn::Error::new(
                input.span(),
                "expected integer or string literal (or `_` for unversioned) for version tag",
            ))
        }
    }
}

impl core::fmt::Display for VersionTag {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Untagged => write!(f, "_"),
            Self::Int(i) => write!(f, "{i}"),
            Self::String(s) => write!(f, "{s}"),
        }
    }
}

impl quote::ToTokens for VersionTag {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        match self {
            Self::Untagged => {
                let underscore = Token![_](proc_macro2::Span::call_site());
                underscore.to_tokens(tokens);
            }
            Self::Int(i) => {
                let lit = syn::LitInt::new(&i.to_string(), proc_macro2::Span::call_site());
                lit.to_tokens(tokens);
            }
            Self::String(s) => {
                let lit = syn::LitStr::new(s, proc_macro2::Span::call_site());
                lit.to_tokens(tokens);
            }
        }
    }
}

/// Represents a parsed `#[cfg(...)]` predicate attached to a version entry.
#[derive(Debug, Clone)]
#[allow(dead_code, reason = "fields used across macro validation and code generation")]
pub struct CfgPredicate {
    pub tokens: proc_macro2::TokenStream,
    pub span: proc_macro2::Span,
}

/// Represents an explicit `=> <target>` transition target on a version entry.
#[derive(Debug, Clone)]
#[allow(dead_code, reason = "variants used in DAG resolution")]
pub enum ExplicitNext {
    Tag(VersionTag, proc_macro2::Span),
    SelfTarget(proc_macro2::Span),
    Type(Box<Type>, proc_macro2::Span),
}

/// Represents a single version entry inside the compat macro body.
#[derive(Debug, Clone)]
#[allow(dead_code, reason = "fields used across macro parsing and validation")]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent parsed attribute flags on a version entry"
)]
pub struct VersionEntry {
    pub cfgs: Vec<CfgPredicate>,
    pub tag: VersionTag,
    pub tag_span: proc_macro2::Span,
    pub ty: Type,
    pub fallible: bool,
    pub is_current: bool,
    pub is_serialize: bool,
    pub is_alias: bool,
    pub serialize_with_fn: Option<syn::ExprPath>,
    pub with_fn: Option<syn::ExprPath>,
    pub explicit_next: Option<ExplicitNext>,
}

/// Arguments parsed from the `#[backwards_compat(...)]` attribute.
#[derive(Debug, Clone)]
#[allow(dead_code, reason = "fields used in macro attribute parsing")]
pub struct BackwardsCompatArgs {
    pub tag_field: String,
    pub content_field: Option<String>,
    pub error_ty: Option<syn::Type>,
    pub dump: bool,
    pub versions: Vec<VersionEntry>,
}

impl Parse for BackwardsCompatArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut tag_field = "version".to_owned();
        let mut content_field = None;
        let mut error_ty = None;
        let mut dump = false;
        let mut versions = Vec::new();

        while !input.is_empty() {
            let ident: Ident = input.parse()?;
            let ident_str = ident.to_string();

            if ident_str == "tag" {
                if input.peek(Token![=]) {
                    input.parse::<Token![=]>()?;
                    let s: syn::LitStr = input.parse()?;
                    tag_field = s.value();
                } else if input.peek(syn::token::Paren) {
                    let inner;
                    syn::parenthesized!(inner in input);
                    let s: syn::LitStr = inner.parse()?;
                    tag_field = s.value();
                } else {
                    return Err(syn::Error::new(ident.span(), "expected `=` or `()` for tag"));
                }
            } else if ident_str == "content" {
                if input.peek(Token![=]) {
                    input.parse::<Token![=]>()?;
                    let s: syn::LitStr = input.parse()?;
                    content_field = Some(s.value());
                } else if input.peek(syn::token::Paren) {
                    let inner;
                    syn::parenthesized!(inner in input);
                    let s: syn::LitStr = inner.parse()?;
                    content_field = Some(s.value());
                } else {
                    return Err(syn::Error::new(ident.span(), "expected `=` or `()` for content"));
                }
            } else if ident_str == "version" {
                return Err(syn::Error::new(
                    ident.span(),
                    "top-level `version` was removed in 2.0; declare it inside `versions(...)` using `#[current]` or `N: Self`",
                ));
            } else if ident_str == "error" {
                if input.peek(Token![=]) {
                    input.parse::<Token![=]>()?;
                    error_ty = Some(input.parse()?);
                } else if input.peek(syn::token::Paren) {
                    let inner;
                    syn::parenthesized!(inner in input);
                    error_ty = Some(inner.parse()?);
                } else {
                    return Err(syn::Error::new(ident.span(), "expected `=` or `()` for error"));
                }
            } else if ident_str == "dump" {
                dump = true;
            } else if ident_str == "versions" {
                let content;
                if input.peek(syn::token::Paren) {
                    syn::parenthesized!(content in input);
                } else if input.peek(Token![=]) && input.peek2(syn::token::Bracket) {
                    input.parse::<Token![=]>()?;
                    syn::bracketed!(content in input);
                } else if input.peek(Token![=]) && input.peek2(syn::token::Brace) {
                    input.parse::<Token![=]>()?;
                    syn::braced!(content in input);
                } else {
                    return Err(syn::Error::new(
                        ident.span(),
                        "expected `(...)`, `= [...]`, or `= { ... }` for versions",
                    ));
                }

                while !content.is_empty() {
                    let outer_attrs = content.call(Attribute::parse_outer)?;
                    let mut fallible = false;
                    let mut is_current = false;
                    let mut current_attr_span = None;
                    let mut is_serialize = false;
                    let mut serialize_attr_span = None;
                    let mut serialize_with_fn = None;
                    let mut with_fn = None;
                    let mut cfgs = Vec::new();

                    for attr in outer_attrs {
                        if attr.path().is_ident("fallible") {
                            fallible = true;
                        } else if attr.path().is_ident("current") {
                            if !matches!(attr.meta, syn::Meta::Path(_)) {
                                return Err(syn::Error::new_spanned(
                                    attr,
                                    "expected bare `#[current]` attribute",
                                ));
                            }
                            if is_current {
                                return Err(syn::Error::new_spanned(
                                    attr,
                                    "duplicate `#[current]` attribute on version entry",
                                ));
                            }
                            is_current = true;
                            current_attr_span = Some(attr.span());
                        } else if attr.path().is_ident("serialize") {
                            if is_serialize {
                                return Err(syn::Error::new_spanned(
                                    attr,
                                    "duplicate `#[serialize]` attribute on version entry",
                                ));
                            }
                            match &attr.meta {
                                syn::Meta::Path(_) => {
                                    is_serialize = true;
                                    serialize_attr_span = Some(attr.span());
                                }
                                syn::Meta::List(_) => {
                                    let path: syn::ExprPath = attr.parse_args_with(|args: ParseStream| {
                                        let kw: Ident = args.parse()?;
                                        if kw != "with" {
                                            return Err(syn::Error::new(
                                                kw.span(),
                                                "expected `with = <fn_path>` inside `#[serialize(...)]`",
                                            ));
                                        }
                                        args.parse::<Token![=]>()?;
                                        let fn_path: syn::ExprPath = args.parse()?;
                                        if !args.is_empty() {
                                            return Err(syn::Error::new(
                                                args.span(),
                                                "unexpected trailing tokens in `#[serialize(with = ...)]`",
                                            ));
                                        }
                                        Ok(fn_path)
                                    })?;
                                    is_serialize = true;
                                    serialize_attr_span = Some(attr.span());
                                    serialize_with_fn = Some(path);
                                }
                                syn::Meta::NameValue(_) => {
                                    return Err(syn::Error::new_spanned(
                                        attr,
                                        "expected `#[serialize]` or `#[serialize(with = fn_path)]`",
                                    ));
                                }
                            }
                        } else if attr.path().is_ident("with") {
                            if with_fn.is_some() {
                                return Err(syn::Error::new_spanned(
                                    attr,
                                    "duplicate `#[with(...)]` attribute on version entry",
                                ));
                            }
                            let path: syn::ExprPath = attr.parse_args()?;
                            with_fn = Some(path);
                        } else if attr.path().is_ident("cfg") {
                            match &attr.meta {
                                syn::Meta::List(list) if !list.tokens.is_empty() => {
                                    cfgs.push(CfgPredicate {
                                        tokens: list.tokens.clone(),
                                        span: attr.span(),
                                    });
                                }
                                _ => {
                                    return Err(syn::Error::new_spanned(
                                        attr,
                                        "expected `#[cfg(...)]` with a non-empty predicate",
                                    ));
                                }
                            }
                        } else if attr.path().is_ident("doc") {
                            // Silently ignore doc comments on version entries.
                        } else {
                            return Err(syn::Error::new_spanned(
                                &attr,
                                format!(
                                    "unsupported attribute `{}` on version entry; only `#[cfg(...)]`, `#[current]`, `#[serialize]`, `#[fallible]`, and `#[with(...)]` are supported",
                                    attr.path().to_token_stream()
                                ),
                            ));
                        }
                    }

                    let mut tags = vec![VersionTag::parse(&content)?];
                    while content.peek(Token![|]) {
                        content.parse::<Token![|]>()?;
                        tags.push(VersionTag::parse(&content)?);
                    }

                    if is_current && let Some((_, untagged_span)) = tags.iter().find(|(t, _)| t.is_untagged())
                    {
                        return Err(syn::Error::new(
                            current_attr_span.unwrap_or(*untagged_span),
                            "unversioned `_` cannot be marked `#[current]`",
                        ));
                    }
                    if is_serialize
                        && let Some((_, untagged_span)) = tags.iter().find(|(t, _)| t.is_untagged())
                    {
                        return Err(syn::Error::new(
                            serialize_attr_span.unwrap_or(*untagged_span),
                            "unversioned `_` cannot be marked `#[serialize]`",
                        ));
                    }

                    content.parse::<Token![:]>()?;
                    let ty: Type = content.parse()?;

                    let explicit_next = if content.peek(Token![=>]) {
                        content.parse::<Token![=>]>()?;
                        if content.peek(Token![Self]) && !content.peek2(Token![::]) {
                            let kw: Token![Self] = content.parse()?;
                            Some(ExplicitNext::SelfTarget(kw.span))
                        } else if content.peek(Token![_])
                            || content.peek(syn::LitInt)
                            || content.peek(syn::LitStr)
                        {
                            let (next_tag, next_span) =
                                VersionTag::parse_concrete(&content, "target of `=>`")?;
                            Some(ExplicitNext::Tag(next_tag, next_span))
                        } else {
                            let next_ty: Type = content.parse()?;
                            let next_span = next_ty.span();
                            Some(ExplicitNext::Type(Box::new(next_ty), next_span))
                        }
                    } else {
                        None
                    };

                    while content.peek(Token![,]) || content.peek(Token![;]) {
                        if content.peek(Token![,]) {
                            content.parse::<Token![,]>()?;
                        } else {
                            content.parse::<Token![;]>()?;
                        }
                    }

                    for (idx, (tag, tag_span)) in tags.into_iter().enumerate() {
                        versions.push(VersionEntry {
                            cfgs: cfgs.clone(),
                            tag,
                            tag_span,
                            ty: ty.clone(),
                            fallible,
                            is_current: idx == 0 && is_current,
                            is_serialize: idx == 0 && is_serialize,
                            is_alias: idx > 0,
                            serialize_with_fn: if idx == 0 { serialize_with_fn.take() } else { None },
                            with_fn: with_fn.clone(),
                            explicit_next: explicit_next.clone(),
                        });
                    }
                }
            } else {
                return Err(syn::Error::new(
                    ident.span(),
                    format!("unknown attribute argument `{ident_str}`"),
                ));
            }

            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            }
        }

        Ok(Self {
            tag_field,
            content_field,
            error_ty,
            dump,
            versions,
        })
    }
}

impl BackwardsCompatArgs {
    pub fn into_input(
        self,
        target_ty: syn::Type,
        vis: syn::Visibility,
        generics: syn::Generics,
    ) -> syn::Result<BackwardsCompatInput> {
        let mut versions = self.versions;
        for entry in &mut versions {
            if let syn::Type::Path(ref tp) = entry.ty
                && tp.qself.is_none()
                && tp.path.is_ident("Self")
            {
                entry.ty = target_ty.clone();
            }
        }

        let mut current_entry: Option<&VersionEntry> = None;
        for entry in &versions {
            if entry.is_current {
                if current_entry.is_some() {
                    return Err(syn::Error::new(
                        entry.tag_span,
                        "duplicate `#[current]` attribute; at most one version may be marked `#[current]`",
                    ));
                }
                current_entry = Some(entry);
            }
        }

        let mut serialize_entry: Option<&VersionEntry> = None;
        for entry in &versions {
            if entry.is_serialize {
                if serialize_entry.is_some() {
                    return Err(syn::Error::new(
                        entry.tag_span,
                        "duplicate `#[serialize]` attribute; at most one version may be marked `#[serialize]`",
                    ));
                }
                serialize_entry = Some(entry);
            }
        }

        let (current_version, current_version_span) = current_entry
            .or_else(|| {
                versions
                    .iter()
                    .rfind(|entry| !entry.tag.is_untagged() && !entry.is_alias)
            })
            .or_else(|| versions.iter().rfind(|entry| !entry.tag.is_untagged()))
            .map_or_else(
                || (VersionTag::Int(1), proc_macro2::Span::call_site()),
                |entry| (entry.tag.clone(), entry.tag_span),
            );

        let (serialize_version, serialize_version_span, serialize_with_fn) = serialize_entry.map_or_else(
            || (current_version.clone(), current_version_span, None),
            |entry| (entry.tag.clone(), entry.tag_span, entry.serialize_with_fn.clone()),
        );

        for entry in &versions {
            if entry.tag.matches(&current_version)
                && let Some(first_cfg) = entry.cfgs.first()
            {
                return Err(syn::Error::new(
                    first_cfg.span,
                    format!(
                        "active current version `{}` cannot be conditionally compiled with `#[cfg(...)]`",
                        entry.tag
                    ),
                ));
            }
            if entry.tag.matches(&serialize_version)
                && let Some(first_cfg) = entry.cfgs.first()
            {
                return Err(syn::Error::new(
                    first_cfg.span,
                    format!(
                        "serialize version `{}` cannot be conditionally compiled with `#[cfg(...)]`",
                        entry.tag
                    ),
                ));
            }
        }

        for (i, entry) in versions.iter().enumerate() {
            if entry.tag.is_untagged() {
                continue;
            }
            if let Some(prev_slice) = versions.get(..i) {
                for prev in prev_slice {
                    if entry.tag.matches(&prev.tag) {
                        return Err(syn::Error::new(
                            entry.tag_span,
                            format!("duplicate version tag `{}`", entry.tag),
                        ));
                    }
                }
            }
        }

        Ok(BackwardsCompatInput {
            vis,
            target_ty,
            generics,
            tag_field: self.tag_field,
            content_field: self.content_field,
            current_version,
            current_version_span,
            serialize_version,
            serialize_version_span,
            serialize_with_fn,
            error_ty: self.error_ty,
            dump: self.dump,
            versions,
        })
    }
}

/// Represents the resolved input of a `#[backwards_compat(...)]` macro attribute.
#[derive(Debug, Clone)]
#[allow(dead_code, reason = "fields used in macro code generation")]
pub struct BackwardsCompatInput {
    pub vis: Visibility,
    pub target_ty: Type,
    pub generics: syn::Generics,
    pub tag_field: String,
    pub content_field: Option<String>,
    pub current_version: VersionTag,
    pub current_version_span: proc_macro2::Span,
    pub serialize_version: VersionTag,
    pub serialize_version_span: proc_macro2::Span,
    pub serialize_with_fn: Option<syn::ExprPath>,
    pub error_ty: Option<Type>,
    pub dump: bool,
    pub versions: Vec<VersionEntry>,
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, reason = "index accesses in unit test assertions")]
mod tests {
    use super::*;

    #[test]
    fn test_parse_args_basic() {
        let input = quote::quote! {
            tag = "schema_version",
            dump,
            versions = [
                1: ModelV1 => 2,
                2: ModelV2 => 3,
                #[current] 3: ModelV3,
            ]
        };
        let args: BackwardsCompatArgs = syn::parse2(input).unwrap();
        assert_eq!(args.tag_field, "schema_version");
        assert!(args.dump);
        assert_eq!(args.versions.len(), 3);
        assert_eq!(args.versions[0].tag, VersionTag::Int(1));
        assert!(args.versions[2].is_current);
    }

    #[test]
    fn test_parse_args_paren() {
        let input = quote::quote! {
            tag("schema_version"),
            versions(
                1: ModelV1 => 2,
                #[fallible] 2: ModelV2 => 3,
                3: Self,
            )
        };
        let args: BackwardsCompatArgs = syn::parse2(input).unwrap();
        assert_eq!(args.tag_field, "schema_version");
        assert_eq!(args.versions.len(), 3);
        assert!(args.versions[1].fallible);
    }

    #[test]
    fn test_parse_args_with_strings() {
        let input = quote::quote! {
            tag = "schema",
            dump,
            versions(
                "v1": ModelV1 => "v2",
                #[fallible] "v2": ModelV2 => "v3",
                "v3": Self,
            )
        };
        let args: BackwardsCompatArgs = syn::parse2(input).unwrap();
        assert_eq!(args.tag_field, "schema");
        assert_eq!(args.versions.len(), 3);
        assert_eq!(args.versions[0].tag, VersionTag::String("v1".to_owned()));
        assert!(matches!(
            args.versions[0].explicit_next,
            Some(ExplicitNext::Tag(VersionTag::String(ref s), _)) if s == "v2"
        ));
        assert!(!args.versions[0].fallible);
        assert_eq!(args.versions[1].tag, VersionTag::String("v2".to_owned()));
        assert!(args.versions[1].fallible);
        assert!(args.dump);
    }

    #[test]
    fn test_parse_args_error_ty() {
        let input1 = quote::quote! {
            error = MyCustomError,
            versions = [ 1: ModelV1 ]
        };
        let args1: BackwardsCompatArgs = syn::parse2(input1).unwrap();
        assert!(args1.error_ty.is_some());

        let input2 = quote::quote! {
            error(MyCustomError),
            versions = [ 1: ModelV1 ]
        };
        let args2: BackwardsCompatArgs = syn::parse2(input2).unwrap();
        assert!(args2.error_ty.is_some());
    }

    #[test]
    fn test_parse_args_braced_versions() {
        let input = quote::quote! {
            versions = {
                1: ModelV1 => 2,
                2: ModelV2,
            }
        };
        let args: BackwardsCompatArgs = syn::parse2(input).unwrap();
        assert_eq!(args.versions.len(), 2);
    }

    #[test]
    fn test_parse_args_into_input_success() {
        let input = quote::quote! {
            tag = "custom_tag",
            dump,
            versions = [
                1: ModelV1 => 2,
                #[current] 2: ModelV2,
            ]
        };
        let args: BackwardsCompatArgs = syn::parse2(input).unwrap();
        let target_ty: syn::Type = syn::parse_quote!(TargetModel);
        let vis: syn::Visibility = syn::parse_quote!(pub);
        let compat_input = args.into_input(target_ty, vis, syn::Generics::default()).unwrap();

        assert_eq!(compat_input.tag_field, "custom_tag");
        assert_eq!(compat_input.current_version, VersionTag::Int(2));
        assert!(compat_input.dump);
        assert_eq!(compat_input.versions.len(), 2);
    }

    #[test]
    fn test_parse_args_into_input_default_version() {
        let input = quote::quote! {
            versions = [
                1: ModelV1 => 2,
                2: ModelV2,
            ]
        };
        let args: BackwardsCompatArgs = syn::parse2(input).unwrap();
        let target_ty: syn::Type = syn::parse_quote!(TargetModel);
        let vis: syn::Visibility = syn::Visibility::Inherited;
        let compat_input = args.into_input(target_ty, vis, syn::Generics::default()).unwrap();

        assert_eq!(compat_input.current_version, VersionTag::Int(2));
    }

    #[test]
    fn test_parse_args_into_input_empty_versions_allowed() {
        let input = quote::quote! {
            versions = []
        };
        let args: BackwardsCompatArgs = syn::parse2(input).unwrap();
        let target_ty: syn::Type = syn::parse_quote!(TargetModel);
        let vis: syn::Visibility = syn::Visibility::Inherited;
        let compat_input = args.into_input(target_ty, vis, syn::Generics::default()).unwrap();
        assert_eq!(compat_input.current_version, VersionTag::Int(1));
        assert!(compat_input.versions.is_empty());
    }

    #[test]
    fn test_parse_args_into_input_duplicate_version_tag_error() {
        let input = quote::quote! {
            versions = [
                1: ModelV1,
                1: ModelV1Dup,
            ]
        };
        let args: BackwardsCompatArgs = syn::parse2(input).unwrap();
        let target_ty: syn::Type = syn::parse_quote!(TargetModel);
        let vis: syn::Visibility = syn::Visibility::Inherited;
        let err = args
            .into_input(target_ty, vis, syn::Generics::default())
            .unwrap_err();
        assert_eq!(err.to_string(), "duplicate version tag `1`");
    }

    #[test]
    fn test_parse_args_top_level_version_removed_error() {
        let input = quote::quote! {
            version = 99,
            versions = [
                1: ModelV1 => 2,
                2: ModelV2,
            ]
        };
        let err = syn::parse2::<BackwardsCompatArgs>(input).unwrap_err();
        assert!(err.to_string().contains("top-level `version` was removed in 2.0"));
    }

    #[test]
    fn test_parse_args_invalid_keys() {
        let input = quote::quote! {
            unknown_arg = 123
        };
        let err = syn::parse2::<BackwardsCompatArgs>(input).unwrap_err();
        assert_eq!(err.to_string(), "unknown attribute argument `unknown_arg`");
    }

    #[test]
    fn test_parse_args_invalid_syntax() {
        let input_tag = quote::quote! {
            tag 123
        };
        let err_tag = syn::parse2::<BackwardsCompatArgs>(input_tag).unwrap_err();
        assert_eq!(err_tag.to_string(), "expected `=` or `()` for tag");

        let input_error = quote::quote! {
            error 123
        };
        let err_error = syn::parse2::<BackwardsCompatArgs>(input_error).unwrap_err();
        assert_eq!(err_error.to_string(), "expected `=` or `()` for error");

        let input_content = quote::quote! {
            content 123
        };
        let err_content = syn::parse2::<BackwardsCompatArgs>(input_content).unwrap_err();
        assert_eq!(err_content.to_string(), "expected `=` or `()` for content");

        let input_versions = quote::quote! {
            versions: [1: ModelV1]
        };
        let err_versions = syn::parse2::<BackwardsCompatArgs>(input_versions).unwrap_err();
        assert_eq!(
            err_versions.to_string(),
            "expected `(...)`, `= [...]`, or `= { ... }` for versions"
        );
    }

    #[test]
    fn test_parse_args_content_eq() {
        let input = quote::quote! {
            content = "data",
            versions = [1: ModelV1]
        };
        let args: BackwardsCompatArgs = syn::parse2(input).unwrap();
        assert_eq!(args.content_field.as_deref(), Some("data"));

        let target_ty: syn::Type = syn::parse_quote!(TargetModel);
        let vis: syn::Visibility = syn::Visibility::Inherited;
        let compat_input = args.into_input(target_ty, vis, syn::Generics::default()).unwrap();
        assert_eq!(compat_input.content_field.as_deref(), Some("data"));
    }

    #[test]
    fn test_parse_args_content_paren() {
        let input = quote::quote! {
            content("payload"),
            versions = [1: ModelV1]
        };
        let args: BackwardsCompatArgs = syn::parse2(input).unwrap();
        assert_eq!(args.content_field.as_deref(), Some("payload"));

        let target_ty: syn::Type = syn::parse_quote!(TargetModel);
        let vis: syn::Visibility = syn::Visibility::Inherited;
        let compat_input = args.into_input(target_ty, vis, syn::Generics::default()).unwrap();
        assert_eq!(compat_input.content_field.as_deref(), Some("payload"));
    }

    #[test]
    fn test_parse_args_content_omitted_is_none() {
        let input = quote::quote! {
            tag = "schema",
            versions = [1: ModelV1]
        };
        let args: BackwardsCompatArgs = syn::parse2(input).unwrap();
        assert_eq!(args.content_field, None);

        let target_ty: syn::Type = syn::parse_quote!(TargetModel);
        let vis: syn::Visibility = syn::Visibility::Inherited;
        let compat_input = args.into_input(target_ty, vis, syn::Generics::default()).unwrap();
        assert_eq!(compat_input.content_field, None);
    }

    #[test]
    fn test_parse_args_content_with_tag_and_version() {
        let input = quote::quote! {
            tag = "type",
            content = "data",
            dump,
            versions = [
                1: ModelV1 => 2,
                2: ModelV2,
            ]
        };
        let args: BackwardsCompatArgs = syn::parse2(input).unwrap();
        assert_eq!(args.tag_field, "type");
        assert_eq!(args.content_field.as_deref(), Some("data"));
        assert!(args.dump);

        let target_ty: syn::Type = syn::parse_quote!(TargetModel);
        let vis: syn::Visibility = syn::parse_quote!(pub);
        let compat_input = args.into_input(target_ty, vis, syn::Generics::default()).unwrap();
        assert_eq!(compat_input.tag_field, "type");
        assert_eq!(compat_input.content_field.as_deref(), Some("data"));
        assert_eq!(compat_input.current_version, VersionTag::Int(2));
        assert!(compat_input.dump);
        assert_eq!(compat_input.versions.len(), 2);
    }

    #[test]
    fn test_parse_args_ident_version_key_error() {
        let input = quote::quote! {
            versions = [
                v1: ModelV1,
            ]
        };
        let err = syn::parse2::<BackwardsCompatArgs>(input).unwrap_err();
        assert_eq!(
            err.to_string(),
            "version keys must be string or integer literals (e.g. 1 or \"1\"), or `_` for unversioned, not identifiers like v1"
        );
    }

    #[test]
    fn test_parse_args_untagged_wildcard() {
        let input = quote::quote! {
            versions = [
                _: ModelV0,
                1: ModelV1,
            ]
        };
        let args: BackwardsCompatArgs = syn::parse2(input).unwrap();
        assert_eq!(args.versions.len(), 2);
        assert_eq!(args.versions[0].tag, VersionTag::Untagged);
        assert_eq!(args.versions[1].tag, VersionTag::Int(1));
    }

    #[test]
    fn test_parse_args_untagged_self_normalized_and_default_version() {
        let input = quote::quote! {
            versions(_: Self)
        };
        let args: BackwardsCompatArgs = syn::parse2(input).unwrap();
        let target_ty: syn::Type = syn::parse_quote!(TargetModel<T>);
        let vis: syn::Visibility = syn::parse_quote!(pub);
        let compat_input = args
            .into_input(target_ty.clone(), vis, syn::Generics::default())
            .unwrap();
        assert_eq!(compat_input.current_version, VersionTag::Int(1));
        assert_eq!(compat_input.versions.len(), 1);
        assert_eq!(compat_input.versions[0].tag, VersionTag::Untagged);
        assert_eq!(compat_input.versions[0].ty, target_ty);
    }

    #[test]
    fn test_parse_args_untagged_invalid_contexts() {
        let err2 = syn::parse2::<BackwardsCompatArgs>(quote::quote! {
            versions(1: ModelV1 => _)
        })
        .unwrap_err();
        assert_eq!(
            err2.to_string(),
            "unversioned `_` cannot be used as target of `=>`"
        );
    }

    #[test]
    fn test_parse_args_fallible_attribute() {
        let input = quote::quote! {
            versions = [
                #[fallible] 1: V1 => 2,
                2: V2,
            ]
        };
        let args: BackwardsCompatArgs = syn::parse2(input).unwrap();
        assert_eq!(args.versions.len(), 2);
        assert!(args.versions[0].fallible);
        assert!(!args.versions[1].fallible);
    }

    #[test]
    fn test_parse_args_cfg_single() {
        let input = quote::quote! {
            versions = [
                #[cfg(test)]
                1: ModelV1 => 2,
                #[cfg(feature = "compat")]
                2: ModelV2 => 3,
                3: ModelV3,
            ]
        };
        let args: BackwardsCompatArgs = syn::parse2(input).unwrap();
        assert_eq!(args.versions.len(), 3);

        assert_eq!(args.versions[0].cfgs.len(), 1);
        assert_eq!(args.versions[0].cfgs[0].tokens.to_string(), "test");
        assert!(!args.versions[0].fallible);

        assert_eq!(args.versions[1].cfgs.len(), 1);
        assert_eq!(
            args.versions[1].cfgs[0].tokens.to_string(),
            quote::quote!(feature = "compat").to_string()
        );
        assert!(!args.versions[1].fallible);

        assert!(args.versions[2].cfgs.is_empty());
    }

    #[test]
    fn test_parse_args_cfg_multiple_on_single_entry() {
        let input = quote::quote! {
            versions = [
                #[cfg(feature = "v1")]
                #[cfg(not(target_arch = "wasm32"))]
                #[cfg(all(unix, debug_assertions))]
                1: ModelV1 => 2,
                2: ModelV2,
            ]
        };
        let args: BackwardsCompatArgs = syn::parse2(input).unwrap();
        assert_eq!(args.versions.len(), 2);
        assert_eq!(args.versions[0].cfgs.len(), 3);
        assert_eq!(
            args.versions[0].cfgs[0].tokens.to_string(),
            quote::quote!(feature = "v1").to_string()
        );
        assert_eq!(
            args.versions[0].cfgs[1].tokens.to_string(),
            quote::quote!(not(target_arch = "wasm32")).to_string()
        );
        assert_eq!(
            args.versions[0].cfgs[2].tokens.to_string(),
            quote::quote!(all(unix, debug_assertions)).to_string()
        );
        assert!(args.versions[1].cfgs.is_empty());
    }

    #[test]
    fn test_parse_args_cfg_combined_with_fallible_both_orders() {
        let input = quote::quote! {
            versions = [
                #[cfg(feature = "compat")]
                #[fallible]
                1: ModelV1 => 2,
                #[fallible]
                #[cfg(test)]
                2: ModelV2 => 3,
                3: ModelV3,
            ]
        };
        let args: BackwardsCompatArgs = syn::parse2(input).unwrap();
        assert_eq!(args.versions.len(), 3);

        assert!(args.versions[0].fallible);
        assert_eq!(args.versions[0].cfgs.len(), 1);
        assert_eq!(
            args.versions[0].cfgs[0].tokens.to_string(),
            quote::quote!(feature = "compat").to_string()
        );

        assert!(args.versions[1].fallible);
        assert_eq!(args.versions[1].cfgs.len(), 1);
        assert_eq!(args.versions[1].cfgs[0].tokens.to_string(), "test");

        assert!(!args.versions[2].fallible);
        assert!(args.versions[2].cfgs.is_empty());
    }

    #[test]
    fn test_parse_args_doc_comments_silently_ignored() {
        let input = quote::quote! {
            versions = [
                /// Legacy version 1.
                #[cfg(feature = "v1")]
                1: ModelV1 => 2,
                #[doc = "Explicit doc attribute"]
                #[fallible]
                2: ModelV2 => 3,
                /// Current version.
                3: ModelV3,
            ]
        };
        let args: BackwardsCompatArgs = syn::parse2(input).unwrap();
        assert_eq!(args.versions.len(), 3);

        assert_eq!(args.versions[0].cfgs.len(), 1);
        assert_eq!(
            args.versions[0].cfgs[0].tokens.to_string(),
            quote::quote!(feature = "v1").to_string()
        );
        assert!(!args.versions[0].fallible);

        assert!(args.versions[1].cfgs.is_empty());
        assert!(args.versions[1].fallible);

        assert!(args.versions[2].cfgs.is_empty());
        assert!(!args.versions[2].fallible);
    }

    #[test]
    fn test_parse_args_invalid_cfg_forms_error() {
        let invalid_cases = [
            quote::quote! {
                versions = [
                    #[cfg]
                    1: ModelV1,
                ]
            },
            quote::quote! {
                versions = [
                    #[cfg = "foo"]
                    1: ModelV1,
                ]
            },
            quote::quote! {
                versions = [
                    #[cfg()]
                    1: ModelV1,
                ]
            },
        ];

        for input in invalid_cases {
            let err = syn::parse2::<BackwardsCompatArgs>(input).unwrap_err();
            assert_eq!(
                err.to_string(),
                "expected `#[cfg(...)]` with a non-empty predicate"
            );
        }
    }

    #[test]
    fn test_parse_args_unsupported_attributes_error() {
        let err_typo = syn::parse2::<BackwardsCompatArgs>(quote::quote! {
            versions = [
                #[cgf(test)]
                1: ModelV1,
            ]
        })
        .unwrap_err();
        assert_eq!(
            err_typo.to_string(),
            "unsupported attribute `cgf` on version entry; only `#[cfg(...)]`, `#[current]`, `#[serialize]`, `#[fallible]`, and `#[with(...)]` are supported"
        );

        let err_serde = syn::parse2::<BackwardsCompatArgs>(quote::quote! {
            versions = [
                #[serde(rename = "x")]
                1: ModelV1,
            ]
        })
        .unwrap_err();
        assert_eq!(
            err_serde.to_string(),
            "unsupported attribute `serde` on version entry; only `#[cfg(...)]`, `#[current]`, `#[serialize]`, `#[fallible]`, and `#[with(...)]` are supported"
        );

        let err_path = syn::parse2::<BackwardsCompatArgs>(quote::quote! {
            versions = [
                #[foo::bar]
                1: ModelV1,
            ]
        })
        .unwrap_err();
        assert_eq!(
            err_path.to_string(),
            format!(
                "unsupported attribute `{}` on version entry; only `#[cfg(...)]`, `#[current]`, `#[serialize]`, `#[fallible]`, and `#[with(...)]` are supported",
                quote::quote!(foo::bar)
            )
        );
    }

    #[test]
    fn test_into_input_rejects_cfg_on_explicit_current_version() {
        let input = quote::quote! {
            versions = [
                1: V1 => 2,
                #[current]
                #[cfg(test)]
                2: V2,
            ]
        };
        let args: BackwardsCompatArgs = syn::parse2(input).unwrap();
        let target_ty: syn::Type = syn::parse_quote!(TargetModel);
        let vis: syn::Visibility = syn::Visibility::Inherited;
        let err = args
            .into_input(target_ty, vis, syn::Generics::default())
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "active current version `2` cannot be conditionally compiled with `#[cfg(...)]`"
        );
    }

    #[test]
    fn test_into_input_rejects_cfg_on_implicit_current_version() {
        let input = quote::quote! {
            versions = [
                1: V1 => 2,
                #[cfg(feature = "v2")]
                2: V2,
            ]
        };
        let args: BackwardsCompatArgs = syn::parse2(input).unwrap();
        let target_ty: syn::Type = syn::parse_quote!(TargetModel);
        let vis: syn::Visibility = syn::Visibility::Inherited;
        let err = args
            .into_input(target_ty, vis, syn::Generics::default())
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "active current version `2` cannot be conditionally compiled with `#[cfg(...)]`"
        );
    }

    #[test]
    fn test_into_input_allows_all_historical_versions_gated_with_self_current() {
        let input = quote::quote! {
            versions = [
                #[cfg(feature = "v1")]
                1: V1 => 2,
                #[cfg(feature = "v2")]
                2: V2 => 3,
                3: Self,
            ]
        };
        let args: BackwardsCompatArgs = syn::parse2(input).unwrap();
        let target_ty: syn::Type = syn::parse_quote!(TargetModel);
        let vis: syn::Visibility = syn::Visibility::Inherited;
        let compat_input = args.into_input(target_ty, vis, syn::Generics::default()).unwrap();
        assert_eq!(compat_input.current_version, VersionTag::Int(3));
        assert_eq!(compat_input.versions.len(), 3);
        assert_eq!(compat_input.versions[0].cfgs.len(), 1);
        assert_eq!(compat_input.versions[1].cfgs.len(), 1);
        assert!(compat_input.versions[2].cfgs.is_empty());
    }

    #[test]
    fn test_into_input_allows_cfg_on_historical_versions_with_ungated_current() {
        let input = quote::quote! {
            versions = [
                #[cfg(feature = "legacy")]
                _: V0 => 1,
                #[cfg(feature = "v1")]
                1: V1 => 2,
                2: V2,
            ]
        };
        let args: BackwardsCompatArgs = syn::parse2(input).unwrap();
        let target_ty: syn::Type = syn::parse_quote!(TargetModel);
        let vis: syn::Visibility = syn::Visibility::Inherited;
        let compat_input = args.into_input(target_ty, vis, syn::Generics::default()).unwrap();
        assert_eq!(compat_input.current_version, VersionTag::Int(2));
        assert_eq!(compat_input.versions.len(), 3);
        assert_eq!(compat_input.versions[0].cfgs.len(), 1);
        assert_eq!(compat_input.versions[1].cfgs.len(), 1);
        assert!(compat_input.versions[2].cfgs.is_empty());
    }

    #[test]
    fn test_strict_version_tag_matches_and_duplicates() {
        assert!(!VersionTag::Int(1).matches(&VersionTag::String("1".to_owned())));
        assert!(VersionTag::Int(1).matches(&VersionTag::Int(1)));
        assert!(VersionTag::String("1".to_owned()).matches(&VersionTag::String("1".to_owned())));

        // 1 | "1" is allowed because Int(1) != String("1")
        let args: BackwardsCompatArgs = syn::parse2(quote::quote! {
            versions = [
                1 | "1": V1,
            ]
        })
        .unwrap();
        let compat_input = args
            .into_input(
                syn::parse_quote!(TargetModel),
                syn::Visibility::Inherited,
                syn::Generics::default(),
            )
            .unwrap();
        assert_eq!(compat_input.versions.len(), 2);
        assert_eq!(compat_input.versions[0].tag, VersionTag::Int(1));
        assert_eq!(compat_input.versions[1].tag, VersionTag::String("1".to_owned()));

        // Duplicate int tag 1 | 1 fails
        let dup_int: BackwardsCompatArgs = syn::parse2(quote::quote! {
            versions = [
                1 | 1: V1,
            ]
        })
        .unwrap();
        let err_int = dup_int
            .into_input(
                syn::parse_quote!(TargetModel),
                syn::Visibility::Inherited,
                syn::Generics::default(),
            )
            .unwrap_err();
        assert_eq!(err_int.to_string(), "duplicate version tag `1`");

        // Duplicate string tag "1" | "1" fails
        let dup_str: BackwardsCompatArgs = syn::parse2(quote::quote! {
            versions = [
                "1" | "1": V1,
            ]
        })
        .unwrap();
        let err_str = dup_str
            .into_input(
                syn::parse_quote!(TargetModel),
                syn::Visibility::Inherited,
                syn::Generics::default(),
            )
            .unwrap_err();
        assert_eq!(err_str.to_string(), "duplicate version tag `1`");
    }

    #[test]
    fn test_removed_legacy_aliases_and_top_level_version_error() {
        for key in [quote::quote!(none), quote::quote!(None), quote::quote!(untagged)] {
            let err = syn::parse2::<BackwardsCompatArgs>(quote::quote! {
                versions = [
                    #key: V0,
                    1: V1,
                ]
            })
            .unwrap_err();
            assert_eq!(
                err.to_string(),
                "version keys must be string or integer literals (e.g. 1 or \"1\"), or `_` for unversioned, not identifiers like v1"
            );
        }

        for attr in [
            quote::quote!(#[try_from]),
            quote::quote!(#[falliable]),
            quote::quote!(#[r#try]),
        ] {
            let err = syn::parse2::<BackwardsCompatArgs>(quote::quote! {
                versions = [
                    #attr 1: V1,
                ]
            })
            .unwrap_err();
            assert!(err.to_string().contains("unsupported attribute"));
        }

        let err_version = syn::parse2::<BackwardsCompatArgs>(quote::quote! {
            version = 2,
            versions = [1: V1]
        })
        .unwrap_err();
        assert_eq!(
            err_version.to_string(),
            "top-level `version` was removed in 2.0; declare it inside `versions(...)` using `#[current]` or `N: Self`"
        );
    }

    #[test]
    fn test_parse_explicit_next_self_target() {
        let args: BackwardsCompatArgs = syn::parse2(quote::quote! {
            versions = [
                1: V1 => Self,
                2: V2,
            ]
        })
        .unwrap();
        assert!(matches!(
            args.versions[0].explicit_next,
            Some(ExplicitNext::SelfTarget(_))
        ));
        assert!(args.versions[1].explicit_next.is_none());
    }

    #[test]
    fn test_parse_and_validate_current_attr() {
        // Non-last entry marked #[current]
        let args: BackwardsCompatArgs = syn::parse2(quote::quote! {
            versions = [
                #[current] 1: V1,
                2: V2 => 1,
            ]
        })
        .unwrap();
        let compat_input = args
            .into_input(
                syn::parse_quote!(TargetModel),
                syn::Visibility::Inherited,
                syn::Generics::default(),
            )
            .unwrap();
        assert_eq!(compat_input.current_version, VersionTag::Int(1));
        assert_eq!(compat_input.serialize_version, VersionTag::Int(1));

        // Duplicate #[current] on same entry
        let err_same = syn::parse2::<BackwardsCompatArgs>(quote::quote! {
            versions = [#[current] #[current] 1: V1]
        })
        .unwrap_err();
        assert_eq!(
            err_same.to_string(),
            "duplicate `#[current]` attribute on version entry"
        );

        // Non-bare #[current(...)]
        let err_args = syn::parse2::<BackwardsCompatArgs>(quote::quote! {
            versions = [#[current(foo)] 1: V1]
        })
        .unwrap_err();
        assert_eq!(err_args.to_string(), "expected bare `#[current]` attribute");

        // Duplicate #[current] across entries
        let dup_across: BackwardsCompatArgs = syn::parse2(quote::quote! {
            versions = [
                #[current] 1: V1,
                #[current] 2: V2,
            ]
        })
        .unwrap();
        let err_across = dup_across
            .into_input(
                syn::parse_quote!(TargetModel),
                syn::Visibility::Inherited,
                syn::Generics::default(),
            )
            .unwrap_err();
        assert_eq!(
            err_across.to_string(),
            "duplicate `#[current]` attribute; at most one version may be marked `#[current]`"
        );

        // #[current] on _
        let err_untagged = syn::parse2::<BackwardsCompatArgs>(quote::quote! {
            versions = [#[current] _: V0]
        })
        .unwrap_err();
        assert_eq!(
            err_untagged.to_string(),
            "unversioned `_` cannot be marked `#[current]`"
        );

        // #[current] with #[cfg(...)]
        let cfg_current: BackwardsCompatArgs = syn::parse2(quote::quote! {
            versions = [
                #[current]
                #[cfg(test)]
                1: V1,
                2: V2,
            ]
        })
        .unwrap();
        let err_cfg = cfg_current
            .into_input(
                syn::parse_quote!(TargetModel),
                syn::Visibility::Inherited,
                syn::Generics::default(),
            )
            .unwrap_err();
        assert_eq!(
            err_cfg.to_string(),
            "active current version `1` cannot be conditionally compiled with `#[cfg(...)]`"
        );
    }

    #[test]
    fn test_parse_and_validate_serialize_attr() {
        // Bare #[serialize] on non-current entry
        let bare: BackwardsCompatArgs = syn::parse2(quote::quote! {
            versions = [
                #[serialize] 1: V1 => 2,
                #[current] 2: V2,
            ]
        })
        .unwrap();
        let bare_input = bare
            .into_input(
                syn::parse_quote!(TargetModel),
                syn::Visibility::Inherited,
                syn::Generics::default(),
            )
            .unwrap();
        assert_eq!(bare_input.current_version, VersionTag::Int(2));
        assert_eq!(bare_input.serialize_version, VersionTag::Int(1));
        assert!(bare_input.serialize_with_fn.is_none());

        // #[serialize(with = my_mod::to_v6)]
        let with_fn: BackwardsCompatArgs = syn::parse2(quote::quote! {
            versions = [
                #[serialize(with = my_mod::to_v6)] 6: V6 => 7,
                #[current] 7: V7,
            ]
        })
        .unwrap();
        let with_input = with_fn
            .into_input(
                syn::parse_quote!(TargetModel),
                syn::Visibility::Inherited,
                syn::Generics::default(),
            )
            .unwrap();
        assert_eq!(with_input.serialize_version, VersionTag::Int(6));
        assert_eq!(
            with_input
                .serialize_with_fn
                .as_ref()
                .unwrap()
                .to_token_stream()
                .to_string(),
            quote::quote!(my_mod::to_v6).to_string()
        );

        // Duplicate #[serialize] on same entry
        let err_same = syn::parse2::<BackwardsCompatArgs>(quote::quote! {
            versions = [#[serialize] #[serialize] 1: V1]
        })
        .unwrap_err();
        assert_eq!(
            err_same.to_string(),
            "duplicate `#[serialize]` attribute on version entry"
        );

        // Duplicate #[serialize] across entries
        let dup_across: BackwardsCompatArgs = syn::parse2(quote::quote! {
            versions = [
                #[serialize] 1: V1 => 2,
                #[serialize] 2: V2,
            ]
        })
        .unwrap();
        let err_across = dup_across
            .into_input(
                syn::parse_quote!(TargetModel),
                syn::Visibility::Inherited,
                syn::Generics::default(),
            )
            .unwrap_err();
        assert_eq!(
            err_across.to_string(),
            "duplicate `#[serialize]` attribute; at most one version may be marked `#[serialize]`"
        );

        // Invalid args
        let err_bad_kw = syn::parse2::<BackwardsCompatArgs>(quote::quote! {
            versions = [#[serialize(foo = bar)] 1: V1]
        })
        .unwrap_err();
        assert_eq!(
            err_bad_kw.to_string(),
            "expected `with = <fn_path>` inside `#[serialize(...)]`"
        );

        let err_trailing = syn::parse2::<BackwardsCompatArgs>(quote::quote! {
            versions = [#[serialize(with = my_fn extra)] 1: V1]
        })
        .unwrap_err();
        assert_eq!(
            err_trailing.to_string(),
            "unexpected trailing tokens in `#[serialize(with = ...)]`"
        );

        let err_nv = syn::parse2::<BackwardsCompatArgs>(quote::quote! {
            versions = [#[serialize = "v1"] 1: V1]
        })
        .unwrap_err();
        assert_eq!(
            err_nv.to_string(),
            "expected `#[serialize]` or `#[serialize(with = fn_path)]`"
        );

        // #[serialize] on _
        let err_untagged = syn::parse2::<BackwardsCompatArgs>(quote::quote! {
            versions = [#[serialize] _: V0, 1: V1]
        })
        .unwrap_err();
        assert_eq!(
            err_untagged.to_string(),
            "unversioned `_` cannot be marked `#[serialize]`"
        );

        // #[serialize] with #[cfg(...)]
        let cfg_ser: BackwardsCompatArgs = syn::parse2(quote::quote! {
            versions = [
                #[serialize]
                #[cfg(test)]
                1: V1 => 2,
                #[current]
                2: V2,
            ]
        })
        .unwrap();
        let err_cfg = cfg_ser
            .into_input(
                syn::parse_quote!(TargetModel),
                syn::Visibility::Inherited,
                syn::Generics::default(),
            )
            .unwrap_err();
        assert_eq!(
            err_cfg.to_string(),
            "serialize version `1` cannot be conditionally compiled with `#[cfg(...)]`"
        );
    }

    #[test]
    fn test_parse_and_validate_with_attr() {
        let args: BackwardsCompatArgs = syn::parse2(quote::quote! {
            versions = [
                #[with(my_mod::upgrade)] 1: V1 => 2,
                2: V2,
            ]
        })
        .unwrap();
        assert_eq!(
            args.versions[0]
                .with_fn
                .as_ref()
                .unwrap()
                .to_token_stream()
                .to_string(),
            quote::quote!(my_mod::upgrade).to_string()
        );
        assert!(args.versions[1].with_fn.is_none());

        // Duplicate #[with(...)]
        let err_dup = syn::parse2::<BackwardsCompatArgs>(quote::quote! {
            versions = [#[with(a)] #[with(b)] 1: V1]
        })
        .unwrap_err();
        assert_eq!(
            err_dup.to_string(),
            "duplicate `#[with(...)]` attribute on version entry"
        );

        // Invalid syntax
        for invalid in [
            quote::quote! { versions = [#[with] 1: V1] },
            quote::quote! { versions = [#[with = "a"] 1: V1] },
            quote::quote! { versions = [#[with()] 1: V1] },
        ] {
            let _ = syn::parse2::<BackwardsCompatArgs>(invalid).unwrap_err();
        }
    }

    #[test]
    fn test_parse_inline_pipe_aliases_and_multiple_untagged() {
        let args: BackwardsCompatArgs = syn::parse2(quote::quote! {
            versions = [
                _: LegacyA => 6,
                _: LegacyB => 6,
                #[cfg(test)]
                #[with(my_mod::up)]
                3 | "v3": V3 => Self,
                #[serialize(with = my_mod::ser)]
                5 | "5": V5 => 6,
                #[current]
                6 | "6": V6,
            ]
        })
        .unwrap();

        let compat_input = args
            .into_input(
                syn::parse_quote!(TargetModel),
                syn::Visibility::Inherited,
                syn::Generics::default(),
            )
            .unwrap();

        assert_eq!(compat_input.versions.len(), 8);
        assert_eq!(compat_input.versions[0].tag, VersionTag::Untagged);
        assert_eq!(compat_input.versions[1].tag, VersionTag::Untagged);

        // 3 | "v3" both inherit cfg, with_fn, and => Self
        assert_eq!(compat_input.versions[2].tag, VersionTag::Int(3));
        assert_eq!(compat_input.versions[3].tag, VersionTag::String("v3".to_owned()));
        assert_eq!(compat_input.versions[2].cfgs.len(), 1);
        assert_eq!(compat_input.versions[3].cfgs.len(), 1);
        assert!(compat_input.versions[2].with_fn.is_some());
        assert!(compat_input.versions[3].with_fn.is_some());
        assert!(matches!(
            compat_input.versions[2].explicit_next,
            Some(ExplicitNext::SelfTarget(_))
        ));
        assert!(matches!(
            compat_input.versions[3].explicit_next,
            Some(ExplicitNext::SelfTarget(_))
        ));

        // #[serialize(with = ...)] applies only to first tag (5), not alias ("5")
        assert_eq!(compat_input.serialize_version, VersionTag::Int(5));
        assert!(compat_input.versions[4].is_serialize);
        assert!(compat_input.versions[4].serialize_with_fn.is_some());
        assert!(!compat_input.versions[5].is_serialize);
        assert!(compat_input.versions[5].serialize_with_fn.is_none());

        // #[current] applies only to first tag (6), not alias ("6")
        assert_eq!(compat_input.current_version, VersionTag::Int(6));
        assert!(compat_input.versions[6].is_current);
        assert!(!compat_input.versions[7].is_current);
    }

    #[test]
    fn test_default_current_version_prefers_primary_tag_over_pipe_alias() {
        // When #[current] is omitted and last entry is `2 | "2": Self`, default to primary `Int(2)`
        let args1: BackwardsCompatArgs = syn::parse2(quote::quote! {
            versions(
                1: V1,
                2 | "2": Self,
            )
        })
        .unwrap();
        let input1 = args1
            .into_input(
                syn::parse_quote!(TargetModel),
                syn::Visibility::Inherited,
                syn::Generics::default(),
            )
            .unwrap();
        assert_eq!(input1.current_version, VersionTag::Int(2));
        assert_eq!(input1.serialize_version, VersionTag::Int(2));
        assert!(!input1.versions[1].is_alias);
        assert!(input1.versions[2].is_alias);

        // When primary tag is `_` (`_ | 2: Self`), falls back to concrete alias `Int(2)`
        let args2: BackwardsCompatArgs = syn::parse2(quote::quote! {
            versions(
                _ | 2: Self,
            )
        })
        .unwrap();
        let input2 = args2
            .into_input(
                syn::parse_quote!(TargetModel),
                syn::Visibility::Inherited,
                syn::Generics::default(),
            )
            .unwrap();
        assert_eq!(input2.current_version, VersionTag::Int(2));
        assert_eq!(input2.serialize_version, VersionTag::Int(2));
    }

    #[test]
    fn test_version_tag_helpers_to_tokens_display_and_invalid_literal() {
        let untagged = VersionTag::Untagged;
        let int_tag = VersionTag::Int(42);
        let str_tag = VersionTag::String("v1.0".to_owned());

        assert_eq!(untagged.to_tag_string(), "_");
        assert_eq!(int_tag.to_tag_string(), "42");
        assert_eq!(str_tag.to_tag_string(), "v1.0");

        assert_eq!(untagged.to_string(), "_");
        assert_eq!(int_tag.to_string(), "42");
        assert_eq!(str_tag.to_string(), "v1.0");

        assert_eq!(untagged.to_token_stream().to_string(), "_");
        assert_eq!(int_tag.to_token_stream().to_string(), "42");
        assert_eq!(str_tag.to_token_stream().to_string(), "\"v1.0\"");

        let err = VersionTag::error(proc_macro2::Span::call_site(), "boom");
        assert_eq!(err.to_string(), "boom");

        let bad_lit = syn::parse2::<BackwardsCompatArgs>(quote::quote! {
            versions(3.14: V1)
        })
        .unwrap_err();
        assert!(bad_lit.to_string().contains("expected integer or string literal"));
    }

    #[test]
    fn test_parse_explicit_next_type_targets() {
        let args: BackwardsCompatArgs = syn::parse2(quote::quote! {
            versions(
                1: V1 => V2,
                2: V2 => path::to::V3,
                3: V3 => V4<'a, T>,
                4: V4 => TargetModel,
                5 | "v5": V5 => TargetModel,
            )
        })
        .unwrap();

        assert_eq!(args.versions.len(), 6);

        let expected = [
            quote::quote!(V2).to_string(),
            quote::quote!(path::to::V3).to_string(),
            quote::quote!(V4<'a, T>).to_string(),
            quote::quote!(TargetModel).to_string(),
            quote::quote!(TargetModel).to_string(),
            quote::quote!(TargetModel).to_string(),
        ];

        for (entry, expected_ty) in args.versions.iter().zip(expected) {
            match &entry.explicit_next {
                Some(ExplicitNext::Type(ty, _)) => {
                    assert_eq!(ty.to_token_stream().to_string(), expected_ty);
                }
                other => panic!("expected ExplicitNext::Type({expected_ty}), got {other:?}"),
            }
        }
    }

    #[test]
    fn test_parse_explicit_next_self_associated_type() {
        let args: BackwardsCompatArgs = syn::parse2(quote::quote! {
            versions(
                1: V1 => Self::Item,
                2: V2 => Self,
            )
        })
        .unwrap();

        assert_eq!(args.versions.len(), 2);

        match &args.versions[0].explicit_next {
            Some(ExplicitNext::Type(ty, _)) => {
                assert_eq!(
                    ty.to_token_stream().to_string(),
                    quote::quote!(Self::Item).to_string()
                );
            }
            other => panic!("expected ExplicitNext::Type(Self::Item), got {other:?}"),
        }

        assert!(matches!(
            args.versions[1].explicit_next,
            Some(ExplicitNext::SelfTarget(_))
        ));
    }

    #[test]
    fn test_parse_explicit_next_existing_forms() {
        let args: BackwardsCompatArgs = syn::parse2(quote::quote! {
            versions(
                1: V1 => 2,
                "v1": V1Str => "v2",
                3: V3 => Self,
                4: V4,
            )
        })
        .unwrap();

        assert_eq!(args.versions.len(), 4);
        assert!(matches!(
            args.versions[0].explicit_next,
            Some(ExplicitNext::Tag(VersionTag::Int(2), _))
        ));
        assert!(matches!(
            args.versions[1].explicit_next,
            Some(ExplicitNext::Tag(VersionTag::String(ref s), _)) if s == "v2"
        ));
        assert!(matches!(
            args.versions[2].explicit_next,
            Some(ExplicitNext::SelfTarget(_))
        ));
        assert!(args.versions[3].explicit_next.is_none());
    }

    #[test]
    fn test_parse_explicit_next_errors() {
        let err_untagged = syn::parse2::<BackwardsCompatArgs>(quote::quote! {
            versions(1: V1 => _)
        })
        .unwrap_err();
        assert_eq!(
            err_untagged.to_string(),
            "unversioned `_` cannot be used as target of `=>`"
        );

        let _ = syn::parse2::<BackwardsCompatArgs>(quote::quote! {
            versions(1: V1 => ,)
        })
        .unwrap_err();

        let _ = syn::parse2::<BackwardsCompatArgs>(quote::quote! {
            versions(1: V1 =>)
        })
        .unwrap_err();
    }
}
