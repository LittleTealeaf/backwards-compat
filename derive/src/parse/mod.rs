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

/// Represents the conversion mode for `#[serialize(...)]` and `#[serializable(...)]` on a version entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SerializeConvertMode {
    /// Infallible conversion via `From<&Self>` (or `Clone` + `From<Self>`).
    Into,
    /// Fallible conversion via `TryFrom<&Self>` (or `Clone` + `TryFrom<Self>`).
    TryInto,
    /// Infallible conversion via a custom function `fn(&Self) -> Target`.
    With(syn::ExprPath),
    /// Fallible conversion via a custom function `fn(&Self) -> Result<Target, E>`.
    TryWith(syn::ExprPath),
}

/// Parse a conversion mode from `#[serialize...]` or `#[serializable...]`.
///
/// Supports bare `#[attr]`, `#[attr(into)]`, `#[attr(try_into)]`, `#[attr(with = fn_path)]`,
/// and `#[attr(try_with = fn_path)]`.
fn parse_serialize_convert_mode(attr: &Attribute, attr_name: &str) -> syn::Result<SerializeConvertMode> {
    match &attr.meta {
        syn::Meta::Path(_) => Ok(SerializeConvertMode::Into),
        syn::Meta::List(_) => attr.parse_args_with(|args: ParseStream| {
            let kw: Ident = args.parse()?;
            let kw_str = kw.to_string();
            match kw_str.as_str() {
                "into" => {
                    if !args.is_empty() {
                        return Err(syn::Error::new(
                            args.span(),
                            format!("unexpected trailing tokens in `#[{attr_name}(into)]`"),
                        ));
                    }
                    Ok(SerializeConvertMode::Into)
                }
                "try_into" => {
                    if !args.is_empty() {
                        return Err(syn::Error::new(
                            args.span(),
                            format!("unexpected trailing tokens in `#[{attr_name}(try_into)]`"),
                        ));
                    }
                    Ok(SerializeConvertMode::TryInto)
                }
                "with" => {
                    args.parse::<Token![=]>()?;
                    let fn_path: syn::ExprPath = args.parse()?;
                    if !args.is_empty() {
                        return Err(syn::Error::new(
                            args.span(),
                            format!("unexpected trailing tokens in `#[{attr_name}(with = ...)]`"),
                        ));
                    }
                    Ok(SerializeConvertMode::With(fn_path))
                }
                "try_with" => {
                    args.parse::<Token![=]>()?;
                    let fn_path: syn::ExprPath = args.parse()?;
                    if !args.is_empty() {
                        return Err(syn::Error::new(
                            args.span(),
                            format!("unexpected trailing tokens in `#[{attr_name}(try_with = ...)]`"),
                        ));
                    }
                    Ok(SerializeConvertMode::TryWith(fn_path))
                }
                _ => Err(syn::Error::new(
                    kw.span(),
                    format!(
                        "expected `into`, `try_into`, `with = <fn_path>`, or `try_with = <fn_path>` inside `#[{attr_name}(...)]`"
                    ),
                )),
            }
        }),
        syn::Meta::NameValue(_) => Err(syn::Error::new_spanned(
            attr,
            format!(
                "expected `#[{attr_name}]`, `#[{attr_name}(into)]`, `#[{attr_name}(try_into)]`, `#[{attr_name}(with = fn_path)]`, or `#[{attr_name}(try_with = fn_path)]`"
            ),
        )),
    }
}

/// Recorded usage of a deprecated legacy attribute syntax for compile-time warning emission.
#[derive(Debug, Clone)]
#[allow(dead_code, reason = "fields used for deprecated warning emission")]
pub struct LegacyAttrWarning {
    /// Span of the legacy attribute token.
    pub span: proc_macro2::Span,
    /// Targeted deprecation note describing the replacement syntax.
    pub note: &'static str,
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
    pub has_migrate_attr: bool,
    pub is_current: bool,
    pub serialize: Option<SerializeConvertMode>,
    pub is_alias: bool,
    pub serializable: Option<SerializeConvertMode>,
    pub with_fn: Option<syn::ExprPath>,
    pub explicit_next: Option<ExplicitNext>,
}

/// Parse a boolean attribute option in either `name = true|false` or `name(true|false)` form.
///
/// Rejects bare `name` without `=` or `()`, non-boolean values, and trailing tokens inside `(...)`.
pub fn parse_bool_option(
    input: ParseStream<'_>,
    name: &str,
    ident_span: proc_macro2::Span,
) -> syn::Result<bool> {
    if input.peek(Token![=]) {
        input.parse::<Token![=]>()?;
        let lit: syn::LitBool = input.parse().map_err(|err| {
            syn::Error::new(
                err.span(),
                format!("expected boolean literal (`true` or `false`) for `{name}`"),
            )
        })?;
        Ok(lit.value)
    } else if input.peek(syn::token::Paren) {
        let inner;
        syn::parenthesized!(inner in input);
        let lit: syn::LitBool = inner.parse().map_err(|err| {
            syn::Error::new(
                err.span(),
                format!("expected boolean literal (`true` or `false`) for `{name}`"),
            )
        })?;
        if !inner.is_empty() {
            return Err(syn::Error::new(
                inner.span(),
                format!("unexpected trailing tokens in `{name}(...)`"),
            ));
        }
        Ok(lit.value)
    } else {
        Err(syn::Error::new(
            ident_span,
            format!("expected `= true|false` or `(true|false)` for `{name}`"),
        ))
    }
}

/// Arguments parsed from the `#[backwards_compat(...)]` attribute.
#[derive(Debug, Clone)]
#[allow(dead_code, reason = "fields used in macro attribute parsing")]
#[allow(clippy::struct_excessive_bools, reason = "independent parsed attribute flags")]
pub struct BackwardsCompatArgs {
    pub tag_field: String,
    pub content_field: Option<String>,
    pub external: bool,
    pub dump: bool,
    pub serialize: bool,
    pub deserialize: bool,
    pub serialize_specified: bool,
    pub deserialize_specified: bool,
    pub versions: Vec<VersionEntry>,
    pub legacy_warnings: Vec<LegacyAttrWarning>,
}

impl Parse for BackwardsCompatArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut tag_field = "version".to_owned();
        let mut tag_field_span = None;
        let mut content_field = None;
        let mut content_field_span = None;
        let mut external = false;
        let mut external_span = None;
        let mut dump = false;
        let mut serialize = true;
        let mut serialize_span = None;
        let mut deserialize = true;
        let mut deserialize_span = None;
        let mut versions = Vec::new();
        let mut legacy_warnings = Vec::new();

        while !input.is_empty() {
            let ident: Ident = input.parse()?;
            let ident_str = ident.to_string();

            if ident_str == "tag" {
                let s: syn::LitStr = if input.peek(Token![=]) {
                    input.parse::<Token![=]>()?;
                    input.parse()?
                } else if input.peek(syn::token::Paren) {
                    let inner;
                    syn::parenthesized!(inner in input);
                    inner.parse()?
                } else {
                    return Err(syn::Error::new(ident.span(), "expected `=` or `()` for tag"));
                };
                if s.value().is_empty() {
                    return Err(syn::Error::new(s.span(), "tag field name cannot be empty"));
                }
                tag_field_span = Some(s.span());
                tag_field = s.value();
            } else if ident_str == "content" {
                let s: syn::LitStr = if input.peek(Token![=]) {
                    input.parse::<Token![=]>()?;
                    input.parse()?
                } else if input.peek(syn::token::Paren) {
                    let inner;
                    syn::parenthesized!(inner in input);
                    inner.parse()?
                } else {
                    return Err(syn::Error::new(ident.span(), "expected `=` or `()` for content"));
                };
                if s.value().is_empty() {
                    return Err(syn::Error::new(s.span(), "content field name cannot be empty"));
                }
                content_field_span = Some(s.span());
                content_field = Some(s.value());
            } else if ident_str == "version" {
                return Err(syn::Error::new(
                    ident.span(),
                    "top-level `version` was removed in 2.0; declare it inside `versions(...)` using `#[current]` or `N: Self`",
                ));
            } else if ident_str == "error" {
                return Err(syn::Error::new(
                    ident.span(),
                    "`error = ...` was removed; migrations are now always inlined and fallible errors only need to implement `core::fmt::Display`",
                ));
            } else if ident_str == "via" {
                return Err(syn::Error::new(
                    ident.span(),
                    "expected `via = <Type>` or `via(<Type>)`",
                ));
            } else if ident_str == "external" {
                if input.peek(Token![=]) || input.peek(syn::token::Paren) {
                    return Err(syn::Error::new(input.span(), "`external` does not take a value"));
                }
                if external {
                    return Err(syn::Error::new(
                        ident.span(),
                        "duplicate `external` attribute argument",
                    ));
                }
                external = true;
                external_span = Some(ident.span());
            } else if ident_str == "dump" {
                dump = true;
            } else if ident_str == "serialize" {
                if serialize_span.is_some() {
                    return Err(syn::Error::new(
                        ident.span(),
                        "duplicate `serialize` attribute argument",
                    ));
                }
                serialize = parse_bool_option(input, "serialize", ident.span())?;
                serialize_span = Some(ident.span());
            } else if ident_str == "deserialize" {
                if deserialize_span.is_some() {
                    return Err(syn::Error::new(
                        ident.span(),
                        "duplicate `deserialize` attribute argument",
                    ));
                }
                deserialize = parse_bool_option(input, "deserialize", ident.span())?;
                deserialize_span = Some(ident.span());
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
                    let mut migrate_attr_span: Option<proc_macro2::Span> = None;
                    let mut legacy_fallible_span: Option<proc_macro2::Span> = None;
                    let mut legacy_with_span: Option<proc_macro2::Span> = None;
                    let mut is_current = false;
                    let mut current_attr_span = None;
                    let mut serialize_entry_mode: Option<SerializeConvertMode> = None;
                    let mut serialize_attr_span = None;
                    let mut serializable: Option<SerializeConvertMode> = None;
                    let mut serializable_attr_span = None;
                    let mut with_fn = None;
                    let mut cfgs = Vec::new();

                    for attr in outer_attrs {
                        if attr.path().is_ident("migrate") {
                            if migrate_attr_span.is_some() {
                                return Err(syn::Error::new_spanned(
                                    attr,
                                    "duplicate `#[migrate(...)]` attribute on version entry",
                                ));
                            }
                            match &attr.meta {
                                syn::Meta::List(_) => {
                                    let (parsed_fallible, parsed_with_fn) =
                                        attr.parse_args_with(|args: ParseStream| {
                                            let kw: Ident = args.parse()?;
                                            let kw_str = kw.to_string();
                                            match kw_str.as_str() {
                                                "from" => {
                                                    if !args.is_empty() {
                                                        return Err(syn::Error::new(
                                                            args.span(),
                                                            "unexpected trailing tokens in `#[migrate(from)]`",
                                                        ));
                                                    }
                                                    Ok((false, None))
                                                }
                                                "try_from" => {
                                                    if !args.is_empty() {
                                                        return Err(syn::Error::new(
                                                            args.span(),
                                                            "unexpected trailing tokens in `#[migrate(try_from)]`",
                                                        ));
                                                    }
                                                    Ok((true, None))
                                                }
                                                "with" => {
                                                    args.parse::<Token![=]>()?;
                                                    let fn_path: syn::ExprPath = args.parse()?;
                                                    if !args.is_empty() {
                                                        return Err(syn::Error::new(
                                                            args.span(),
                                                            "unexpected trailing tokens in `#[migrate(with = ...)]`",
                                                        ));
                                                    }
                                                    Ok((false, Some(fn_path)))
                                                }
                                                "try_with" => {
                                                    args.parse::<Token![=]>()?;
                                                    let fn_path: syn::ExprPath = args.parse()?;
                                                    if !args.is_empty() {
                                                        return Err(syn::Error::new(
                                                            args.span(),
                                                            "unexpected trailing tokens in `#[migrate(try_with = ...)]`",
                                                        ));
                                                    }
                                                    Ok((true, Some(fn_path)))
                                                }
                                                _ => Err(syn::Error::new(
                                                    kw.span(),
                                                    "expected `from`, `try_from`, `with = <fn_path>`, or `try_with = <fn_path>` inside `#[migrate(...)]`",
                                                )),
                                            }
                                        })?;
                                    migrate_attr_span = Some(attr.span());
                                    fallible = parsed_fallible;
                                    with_fn = parsed_with_fn;
                                }
                                syn::Meta::Path(_) | syn::Meta::NameValue(_) => {
                                    return Err(syn::Error::new_spanned(
                                        attr,
                                        "expected `#[migrate(from)]`, `#[migrate(try_from)]`, `#[migrate(with = fn_path)]`, or `#[migrate(try_with = fn_path)]`",
                                    ));
                                }
                            }
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
                            if serialize_entry_mode.is_some() {
                                return Err(syn::Error::new_spanned(
                                    attr,
                                    "duplicate `#[serialize]` attribute on version entry",
                                ));
                            }
                            serialize_entry_mode = Some(parse_serialize_convert_mode(&attr, "serialize")?);
                            serialize_attr_span = Some(attr.span());
                        } else if attr.path().is_ident("serializable") {
                            if serializable.is_some() {
                                return Err(syn::Error::new_spanned(
                                    attr,
                                    "duplicate `#[serializable]` attribute on version entry",
                                ));
                            }
                            serializable = Some(parse_serialize_convert_mode(&attr, "serializable")?);
                            serializable_attr_span = Some(attr.span());
                        // LEGACY (deprecated): remove in 5.0
                        } else if attr.path().is_ident("fallible") {
                            fallible = true;
                            legacy_fallible_span = Some(attr.span());
                        } else if attr.path().is_ident("with") {
                            if legacy_with_span.is_some() {
                                return Err(syn::Error::new_spanned(
                                    attr,
                                    "duplicate `#[with(...)]` attribute on version entry",
                                ));
                            }
                            let path: syn::ExprPath = attr.parse_args()?;
                            with_fn = Some(path);
                            legacy_with_span = Some(attr.span());
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
                                    "unsupported attribute `{}` on version entry; only `#[cfg(...)]`, `#[current]`, `#[migrate(...)]`, `#[serialize]`, `#[serializable]`, `#[fallible]`, and `#[with(...)]` are supported",
                                    attr.path().to_token_stream()
                                ),
                            ));
                        }
                    }

                    if migrate_attr_span.is_some()
                        && let Some(legacy_span) = legacy_fallible_span.or(legacy_with_span)
                    {
                        return Err(syn::Error::new(
                            legacy_span,
                            "cannot combine `#[migrate(...)]` with legacy `#[fallible]` or `#[with(...)]` on the same version entry",
                        ));
                    }

                    // LEGACY (deprecated): remove in 5.0
                    match (legacy_fallible_span, legacy_with_span) {
                        (Some(span), Some(_)) => {
                            legacy_warnings.push(LegacyAttrWarning {
                                span,
                                note: "deprecated: use `#[migrate(try_with = fn_path)]` instead of `#[fallible]` + `#[with(...)]`",
                            });
                        }
                        (Some(span), None) => {
                            legacy_warnings.push(LegacyAttrWarning {
                                span,
                                note: "deprecated: use `#[migrate(try_from)]` instead of `#[fallible]`",
                            });
                        }
                        (None, Some(span)) => {
                            legacy_warnings.push(LegacyAttrWarning {
                                span,
                                note: "deprecated: use `#[migrate(with = fn_path)]` instead of `#[with(...)]`",
                            });
                        }
                        (None, None) => {}
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
                    if serialize_entry_mode.is_some()
                        && let Some((_, untagged_span)) = tags.iter().find(|(t, _)| t.is_untagged())
                    {
                        return Err(syn::Error::new(
                            serialize_attr_span.unwrap_or(*untagged_span),
                            "unversioned `_` cannot be marked `#[serialize]`",
                        ));
                    }
                    if serializable.is_some()
                        && let Some((_, untagged_span)) = tags.iter().find(|(t, _)| t.is_untagged())
                    {
                        return Err(syn::Error::new(
                            serializable_attr_span.unwrap_or(*untagged_span),
                            "unversioned `_` cannot be marked `#[serializable]`",
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
                            has_migrate_attr: migrate_attr_span.is_some(),
                            is_current: idx == 0 && is_current,
                            serialize: if idx == 0 {
                                serialize_entry_mode.take()
                            } else {
                                None
                            },
                            is_alias: idx > 0,
                            serializable: if idx == 0 { serializable.take() } else { None },
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

        if external && let Some(tag_span) = tag_field_span {
            return Err(syn::Error::new(
                external_span.unwrap_or(tag_span),
                "cannot combine `external` with `tag = ...`",
            ));
        }

        if external && let Some(content_span) = content_field_span {
            return Err(syn::Error::new(
                external_span.unwrap_or(content_span),
                "cannot combine `external` with `content = ...`",
            ));
        }

        if content_field.as_deref() == Some(tag_field.as_str()) {
            let span = content_field_span
                .or(tag_field_span)
                .unwrap_or_else(proc_macro2::Span::call_site);
            return Err(syn::Error::new(
                span,
                format!("tag and content field names cannot be the same (`{tag_field}`)"),
            ));
        }

        if !serialize && !deserialize {
            let span = serialize_span
                .or(deserialize_span)
                .unwrap_or_else(proc_macro2::Span::call_site);
            return Err(syn::Error::new(
                span,
                "cannot disable both `serialize` and `deserialize` in `#[backwards_compat(...)]`; at least one must be enabled",
            ));
        }

        Ok(Self {
            tag_field,
            content_field,
            external,
            dump,
            serialize,
            deserialize,
            serialize_specified: serialize_span.is_some(),
            deserialize_specified: deserialize_span.is_some(),
            versions,
            legacy_warnings,
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
        if !self.serialize && !self.deserialize {
            return Err(syn::Error::new(
                proc_macro2::Span::call_site(),
                "cannot disable both `serialize` and `deserialize` in `#[backwards_compat(...)]`; at least one must be enabled",
            ));
        }
        if self.external && self.content_field.is_some() {
            return Err(syn::Error::new(
                proc_macro2::Span::call_site(),
                "cannot combine `external` with `content = ...`",
            ));
        }
        if self.tag_field.is_empty() {
            return Err(syn::Error::new(
                proc_macro2::Span::call_site(),
                "tag field name cannot be empty",
            ));
        }
        if self.content_field.as_deref() == Some("") {
            return Err(syn::Error::new(
                proc_macro2::Span::call_site(),
                "content field name cannot be empty",
            ));
        }
        if self.content_field.as_deref() == Some(self.tag_field.as_str()) {
            return Err(syn::Error::new(
                proc_macro2::Span::call_site(),
                format!(
                    "tag and content field names cannot be the same (`{}`)",
                    self.tag_field
                ),
            ));
        }

        let mut versions = self.versions;
        for entry in &mut versions {
            if let syn::Type::Path(ref tp) = entry.ty
                && tp.qself.is_none()
                && tp.path.is_ident("Self")
            {
                entry.ty = target_ty.clone();
            }
        }

        for entry in &versions {
            if !self.serialize {
                if entry.serialize.is_some() {
                    return Err(syn::Error::new(
                        entry.tag_span,
                        "cannot use `#[serialize]` when `serialize = false`",
                    ));
                }
                if entry.serializable.is_some() {
                    return Err(syn::Error::new(
                        entry.tag_span,
                        "cannot use `#[serializable]` when `serialize = false`",
                    ));
                }
            }
            if !self.deserialize {
                if entry.tag.is_untagged() {
                    return Err(syn::Error::new(
                        entry.tag_span,
                        "cannot use unversioned `_` when `deserialize = false`",
                    ));
                }
                if entry.has_migrate_attr {
                    return Err(syn::Error::new(
                        entry.tag_span,
                        "cannot use `#[migrate(...)]` when `deserialize = false`",
                    ));
                }
                if entry.fallible {
                    return Err(syn::Error::new(
                        entry.tag_span,
                        "cannot use `#[fallible]` when `deserialize = false`",
                    ));
                }
                if entry.with_fn.is_some() {
                    return Err(syn::Error::new(
                        entry.tag_span,
                        "cannot use `#[with(...)]` when `deserialize = false`",
                    ));
                }
                if entry.explicit_next.is_some() {
                    return Err(syn::Error::new(
                        entry.tag_span,
                        "cannot use explicit migration target `=> ...` when `deserialize = false`",
                    ));
                }
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
            if entry.serialize.is_some() {
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

        let (serialize_version, serialize_version_span, serialize_mode) = serialize_entry.map_or_else(
            || {
                (
                    current_version.clone(),
                    current_version_span,
                    SerializeConvertMode::Into,
                )
            },
            |entry| {
                (
                    entry.tag.clone(),
                    entry.tag_span,
                    entry.serialize.clone().unwrap_or(SerializeConvertMode::Into),
                )
            },
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

        let mut first_int_span: Option<proc_macro2::Span> = None;
        let mut first_str_span: Option<proc_macro2::Span> = None;
        for entry in &versions {
            if self.external && entry.tag.is_untagged() {
                return Err(syn::Error::new(
                    entry.tag_span,
                    "unversioned `_` cannot be used with `external` tagging",
                ));
            }
            match entry.tag {
                VersionTag::Int(_) => {
                    if first_str_span.is_some() {
                        return Err(syn::Error::new(
                            entry.tag_span,
                            "cannot mix integer and string version tags in the same #[backwards_compat] type; use either all integer tags or all string tags",
                        ));
                    }
                    first_int_span.get_or_insert(entry.tag_span);
                }
                VersionTag::String(_) => {
                    if first_int_span.is_some() {
                        return Err(syn::Error::new(
                            entry.tag_span,
                            "cannot mix integer and string version tags in the same #[backwards_compat] type; use either all integer tags or all string tags",
                        ));
                    }
                    first_str_span.get_or_insert(entry.tag_span);
                }
                VersionTag::Untagged => {}
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
            external: self.external,
            current_version,
            current_version_span,
            serialize_version,
            serialize_version_span,
            serialize_mode,
            dump: self.dump,
            serialize: self.serialize,
            deserialize: self.deserialize,
            versions,
            legacy_warnings: self.legacy_warnings,
            allow_attrs: Vec::new(),
        })
    }
}

/// Represents the resolved input of a `#[backwards_compat(...)]` macro attribute.
#[derive(Debug, Clone)]
#[allow(dead_code, reason = "fields used in macro code generation")]
#[allow(clippy::struct_excessive_bools, reason = "independent parsed attribute flags")]
pub struct BackwardsCompatInput {
    pub vis: Visibility,
    pub target_ty: Type,
    pub generics: syn::Generics,
    pub tag_field: String,
    pub content_field: Option<String>,
    pub external: bool,
    pub current_version: VersionTag,
    pub current_version_span: proc_macro2::Span,
    pub serialize_version: VersionTag,
    pub serialize_version_span: proc_macro2::Span,
    pub serialize_mode: SerializeConvertMode,
    pub dump: bool,
    pub serialize: bool,
    pub deserialize: bool,
    pub versions: Vec<VersionEntry>,
    pub legacy_warnings: Vec<LegacyAttrWarning>,
    pub allow_attrs: Vec<Attribute>,
}

#[cfg(test)]
mod tests;
