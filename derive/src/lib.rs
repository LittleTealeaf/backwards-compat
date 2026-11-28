use proc_macro::TokenStream;

mod codegen;
mod dag;
mod field;
mod parse;
mod shadow;
mod via;

#[cfg(test)]
mod tests;

use parse::BackwardsCompatArgs;

fn is_serde_derive(path: &syn::Path, name: &str) -> bool {
    let segments: Vec<_> = path.segments.iter().collect();
    match segments.as_slice() {
        [single] => single.ident == name,
        [first, second] => first.ident == "serde" && second.ident == name,
        _ => false,
    }
}

#[cfg(feature = "schemars")]
fn is_json_schema_derive(path: &syn::Path) -> bool {
    let segments: Vec<_> = path.segments.iter().collect();
    match segments.as_slice() {
        [single] => single.ident == "JsonSchema",
        [first, second] => first.ident == "schemars" && second.ident == "JsonSchema",
        _ => false,
    }
}

#[cfg(feature = "schemars")]
fn has_json_schema_derive(attrs: &[syn::Attribute]) -> bool {
    for attr in attrs {
        if !attr.path().is_ident("derive") {
            continue;
        }
        let Ok(nested) =
            attr.parse_args_with(syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated)
        else {
            continue;
        };
        if nested.iter().any(is_json_schema_derive) {
            return true;
        }
    }
    false
}

#[cfg(feature = "schemars")]
pub(crate) fn schemars_attrs_have_crate(attrs: &[syn::Attribute]) -> bool {
    for attr in attrs {
        if !attr.path().is_ident("schemars") {
            continue;
        }
        let mut found = false;
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("crate") {
                found = true;
            }
            if meta.input.peek(syn::Token![=]) {
                let _: syn::Expr = meta.value()?.parse()?;
            } else if meta.input.peek(syn::token::Paren) {
                let content;
                syn::parenthesized!(content in meta.input);
                let _: proc_macro2::TokenStream = content.parse()?;
            }
            Ok(())
        });
        if found {
            return true;
        }
    }
    false
}

#[cfg(feature = "schemars")]
pub(crate) fn field_has_schemars_with(attrs: &[syn::Attribute]) -> bool {
    for attr in attrs {
        if !attr.path().is_ident("schemars") {
            continue;
        }
        let mut found = false;
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("with") || meta.path.is_ident("schema_with") {
                found = true;
            }
            if meta.input.peek(syn::Token![=]) {
                let _: syn::Expr = meta.value()?.parse()?;
            } else if meta.input.peek(syn::token::Paren) {
                let content;
                syn::parenthesized!(content in meta.input);
                let _: proc_macro2::TokenStream = content.parse()?;
            }
            Ok(())
        });
        if found {
            return true;
        }
    }
    false
}

pub(crate) fn is_forwarded_attr(attr: &syn::Attribute) -> bool {
    #[cfg(feature = "schemars")]
    const FORWARDED_ATTRS: &[&str] = &[
        "serde", "schemars", "allow", "warn", "deny", "forbid", "cfg", "cfg_attr",
    ];
    #[cfg(not(feature = "schemars"))]
    const FORWARDED_ATTRS: &[&str] = &["serde", "allow", "warn", "deny", "forbid", "cfg", "cfg_attr"];
    FORWARDED_ATTRS.iter().any(|&name| attr.path().is_ident(name))
}

fn is_stripped_domain_attr(attr: &syn::Attribute, keep_serde_attrs: bool) -> bool {
    #[cfg(feature = "schemars")]
    if attr.path().is_ident("schemars") {
        return true;
    }
    !keep_serde_attrs && attr.path().is_ident("serde")
}

fn has_serde_derive(attrs: &[syn::Attribute]) -> bool {
    for attr in attrs {
        if !attr.path().is_ident("derive") {
            continue;
        }
        let Ok(nested) =
            attr.parse_args_with(syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated)
        else {
            continue;
        };
        if nested
            .iter()
            .any(|path| is_serde_derive(path, "Serialize") || is_serde_derive(path, "Deserialize"))
        {
            return true;
        }
    }
    false
}

fn clean_derive_attrs(attrs: &mut Vec<syn::Attribute>, strip_serialize: bool, strip_deserialize: bool) {
    attrs.retain_mut(|attr| {
        if !attr.path().is_ident("derive") {
            return true;
        }
        let Ok(nested) =
            attr.parse_args_with(syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated)
        else {
            return true;
        };

        let original_len = nested.len();
        let retained: Vec<syn::Path> = nested
            .into_iter()
            .filter(|path| {
                let is_ser = strip_serialize && is_serde_derive(path, "Serialize");
                let is_de = strip_deserialize && is_serde_derive(path, "Deserialize");
                #[cfg(feature = "schemars")]
                let is_schema = is_json_schema_derive(path);
                #[cfg(not(feature = "schemars"))]
                let is_schema = false;
                !is_ser && !is_de && !is_schema
            })
            .collect();

        if retained.is_empty() {
            return false;
        }
        if retained.len() == original_len {
            return true;
        }
        // Preserve the original `#[derive(...)]` outer tokens/spans by updating only the inner
        // token list in place. Rebuilding the whole attribute via `parse_quote!` replaces those
        // spans with `Span::call_site()`, which breaks rust-analyzer's rename on fields/variants.
        if let syn::Meta::List(ref mut list) = attr.meta {
            list.tokens = quote::quote!(#(#retained),*);
        } else {
            *attr = syn::parse_quote!(#[derive(#(#retained),*)]);
        }
        true
    });
}

/// Declarative, compile-time verified schema versioning and backwards compatibility for Serde and domain models.
///
/// `#[backwards_compat]` can be applied to a `struct` or `enum` (for type-level versioning, proxy
/// redirection via `via = ProxyType`, or bare `#[backwards_compat]` to enable field-level versioning
/// on an unversioned container), as well as to individual struct or enum fields. It automatically
/// generates `serde::Serialize`, `serde::Deserialize`, `backwards_compat::BackwardsCompat`,
/// `backwards_compat::BackwardsCompatDeserialize`, streaming-first deserializers, zero-allocation
/// wire representations, and always-inlined migration chains from historical versions.
///
/// # Macro Parameters Reference
///
/// - **`tag = "..."` / `tag("...")`** *(default: `"version"`)*:
///   The serialized field name used to store the version identifier (native integer or string).
/// - **`content = "..."` / `content("...")`** *(default: none at type level; `"data"` on fields)*:
///   Configures adjacent tagging (`{"tag": ..., "content": ...}`).
///   Essential for tuple structs, scalar types, newtypes, and enums that cannot be flattened into a map.
///   Field-level `#[backwards_compat(...)]` automatically defaults to `content = "data"`.
/// - **`serialize = true | false` / `serialize(true | false)`** *(default: `true`; struct/enum level only)*:
///   Controls whether `serde::Serialize` and `backwards_compat::BackwardsCompatSerialize` (plus the
///   borrowed shadow wire type) are generated. Setting `serialize = false` allows fields that do not
///   implement `Serialize` or lets you provide a manual or plain `#[derive(Serialize)]` impl.
///   Cannot be combined with `deserialize = false` or with field-level `#[backwards_compat(...)]`.
/// - **`deserialize = true | false` / `deserialize(true | false)`** *(default: `true`; struct/enum level only)*:
///   Controls whether `serde::Deserialize` and `backwards_compat::BackwardsCompatDeserialize` (plus
///   the deserializing shadow wire derive and migration chains) are generated. Setting
///   `deserialize = false` allows fields that do not implement `Deserialize` or lets you provide a
///   manual or plain `#[derive(Deserialize)]` impl. Cannot be combined with `serialize = false` or
///   with field-level `#[backwards_compat(...)]`.
/// - **`dump`** *(optional)*:
///   Prints the macro's generated token stream to `stderr` during compilation for inspection.
/// - **`versions(...)` / `versions = [...]` / `versions = { ... }`**:
///   Declares the list of schema versions and transition paths. The active current version is the
///   entry marked `#[current]`, or defaults to the last non-untagged primary entry (or `1` if empty).
/// - **`via = ProxyType` / `via(ProxyType)`** *(proxy redirection mode; mutually exclusive with `tag`, `content`, `versions`, and field-level `#[backwards_compat]`)*:
///   Delegates `Serialize`, `Deserialize`, `BackwardsCompat`, `BackwardsCompatDeserialize`, and
///   `BackwardsCompatSerialize` through a separate versioned proxy type. Supports `try_from`,
///   `try_into`, `fallible`, `from = fn_path` / `from(fn_path)`, `to = fn_path` / `to(fn_path)`,
///   `try_from = fn_path` / `try_from(fn_path)`, `try_into = fn_path` / `try_into(fn_path)`,
///   `serialize = true | false` / `serialize(true | false)`, `deserialize = true | false` /
///   `deserialize(true | false)`, and `dump`.
///
/// # Field-Level Versioning
///
/// Place `#[backwards_compat(...)]` on individual fields of a `struct` or `enum` variant:
/// - **Standalone (unversioned container)**: Place bare `#[backwards_compat]` on the container
///   alongside `#[derive(Serialize, Deserialize)]`.
/// - **Combined (versioned container)**: Combine an outer `#[backwards_compat(versions(...))]`
///   container with field-level `#[backwards_compat(...)]` on its fields (preserving zero-clone
///   serialization via `serialize_ref`).
/// - Each annotated field gets a private same-scope helper with `content = "data"` by default,
///   supporting primitives, foreign types (`#[migrate(with = ...)]` / `#[migrate(try_with = ...)]`),
///   `Option<T>`, `Vec<T>`, generics, lifetimes, const generics, and multiple fields of the same
///   type with independent chains.
///
/// # Version Entry Directives
///
/// - **`tag: Type`**: Sequential chain step (e.g. `1: ModelV1, 2: ModelV2, 3: Self`). Integer tags
///   (`1`) and string tags (`"1"`) are matched strictly as native types.
/// - **`tag1 | tag2: Type`**: Inline tag aliases mapping multiple homogeneous wire tags (e.g.
///   `1 | 10: ModelV1` or `"1" | "v1": ModelV1`) to the same type and migration path. The first tag
///   is canonical for `#[current]` / `#[serialize]`.
/// - **`external`**: Uses externally tagged enum representation (`{ "1": ... }` in JSON, or
///   compact variant indices in positional binary formats like `bincode`, `postcard`, `bitcode`).
///   Mutually exclusive with `tag = ...`, `content = ...`, and `_`.
/// - **`_: Type` / `_: Self`**: Pre-backwards-compat (unversioned) entry used when the version tag
///   field is absent. Multiple `_` entries are supported and tried in declaration order (`#[serde(untagged)]`).
/// - **`tag: Type => target_tag` / `tag: Type => TargetType` / `tag: Type => Self`**: Custom DAG
///   transition jumping directly to another declared version by tag, by a matching declared type
///   (or the domain type itself), or directly to `Self`. Duplicate entries sharing a type
///   automatically inherit its explicit `=>` transition regardless of declaration order.
/// - **`#[current]`**: Explicitly designates the active current schema version (required only when
///   it is not the last primary entry in `versions`).
/// - **`#[migrate(from)]` / `#[migrate(try_from)]` / `#[migrate(with = fn_path)]` / `#[migrate(try_with = fn_path)]`**:
///   Configures how a version entry converts to the next step during deserialization.
///   `#[migrate(from)]` (the default when omitted) uses `From`; `#[migrate(try_from)]` uses
///   `TryFrom`; `#[migrate(with = fn_path)]` calls `fn(FromTy) -> ToTy`; and
///   `#[migrate(try_with = fn_path)]` calls `fn(FromTy) -> Result<ToTy, E>` where
///   `E: core::fmt::Display`. Legacy `#[fallible]` and `#[with(fn_path)]` remain accepted as
///   deprecated aliases (emitting a compiler warning) and will be removed in the next major version.
/// - **`#[serialize]` / `#[serialize(into)]` / `#[serialize(try_into)]` / `#[serialize(with = fn_path)]` / `#[serialize(try_with = fn_path)]`**:
///   Overrides which version is emitted during default serialization (useful for rolling
///   deployments). Bare `#[serialize]` and `#[serialize(into)]` convert via `From<&Self>` (or
///   `Clone` + `From<Self>`); `#[serialize(try_into)]` converts via `TryFrom<&Self>` (or `Clone` +
///   `TryFrom<Self>`); `#[serialize(with = fn_path)]` calls `fn(&Self) -> WireTy`; and
///   `#[serialize(try_with = fn_path)]` calls `fn(&Self) -> Result<WireTy, E>`. Defaults to the
///   current version.
/// - **`#[serializable]` / `#[serializable(into)]` / `#[serializable(try_into)]` / `#[serializable(with = fn_path)]` / `#[serializable(try_with = fn_path)]`**:
///   Marks a historical version entry as a valid runtime target for `BackwardsCompatSerialize::serialize_as_version`
///   and includes it in `BackwardsCompatSerialize::SERIALIZABLE_VERSIONS`. Supports the same four
///   conversion modes as `#[serialize(...)]`.
/// - **`#[cfg(...)]`**: Conditionally compiles a historical or unversioned (`_`) version entry,
///   including its deserialization arm and inlined migration chain. Multiple
///   `#[cfg(...)]` attributes on an entry or along a path are combined with `all(...)`. Neither the
///   active `#[current]` version nor the `#[serialize]` version may be gated, and unconditional
///   entries cannot migrate through a gated entry.
///
/// # Key Capabilities
///
/// 1. **Streaming-First Deserialization**: Zero-buffer fast path when the version tag appears first,
///    prefix-only buffering when out-of-order, direct version dispatch, single-pass untagged
///    fallback, and fail-fast errors on unknown/future/disabled versions.
/// 2. **Struct, Enum, & Field-Level Versioning**: Works on named structs, tuple structs, unit structs,
///    enums, and individual struct/enum fields.
/// 3. **Zero-Copy / Zero-Allocation Serialization**: Domain models do not need `#[derive(Clone)]`.
///    Serialization borrows fields directly using generated shadow borrowed structs/enums and native
///    integer or string version tags.
/// 4. **Generics, Lifetimes, & Const Generics**: Fully supports generic types, lifetime parameters,
///    const generics, and `where` clauses.
/// 5. **Metadata & Opt-In Migration Tracking**: Implements `BackwardsCompat` (`CURRENT_VERSION`,
///    `SERIALIZE_VERSION`, `SUPPORTED_VERSIONS`, `ACCEPTS_UNTAGGED`) and `Migrated<T>`.
/// 6. **Always-Inlined Migrations**: Multi-step migration chains are inlined directly into each
///    version's deserialization arm without requiring extra trait bounds or unified error types;
///    fallible steps only require `core::fmt::Display`.
///
/// # Examples
///
/// ### 1. Sequential Infallible Migrations (`N: Self`)
///
/// ```rust,ignore
/// use backwards_compat::backwards_compat;
/// use serde::Deserialize;
///
/// #[derive(Deserialize)]
/// pub struct ConfigV1 {
///     pub name: String,
/// }
///
/// #[backwards_compat(tag = "version", versions(1: ConfigV1, 2: Self))]
/// pub struct Config {
///     pub name: String,
///     pub port: u16,
/// }
///
/// impl From<ConfigV1> for Config {
///     fn from(v1: ConfigV1) -> Self {
///         Self { name: v1.name, port: 8080 }
///     }
/// }
/// ```
///
/// ### 2. Field-Level Versioning
///
/// ```rust,ignore
/// use backwards_compat::backwards_compat;
/// use serde::{Deserialize, Serialize};
///
/// fn parse_port(s: String) -> Result<u16, core::num::ParseIntError> {
///     s.parse()
/// }
///
/// #[backwards_compat]
/// #[derive(Serialize, Deserialize)]
/// pub struct AppSettings {
///     pub name: String,
///     // Adjacent tagging defaults to tag = "version", content = "data":
///     #[backwards_compat(versions(
///         #[migrate(try_with = parse_port)] 1: String,
///         2: Self,
///     ))]
///     pub port: u16,
/// }
/// ```
///
/// ### 3. Fallible Migrations with `#[migrate(try_with = ...)]`
///
/// ```rust,ignore
/// #[backwards_compat(
///     tag = "ver",
///     versions(
///         #[migrate(try_with = parse_v1)] 1: ServerV1,
///         2: Self,
///     )
/// )]
/// pub struct Server {
///     pub port: u16,
/// }
/// ```
///
/// ### 4. Adjacent Tagging for Tuple Structs, Scalars, and Enums
///
/// ```rust,ignore
/// #[backwards_compat(
///     tag = "type",
///     content = "data",
///     versions(1: EventV1, 2: Self)
/// )]
/// pub enum Event {
///     Ping,
///     Message(String),
/// }
/// ```
///
/// ### 5. Rolling Deployments (`#[serialize]`) & Inline Tag Aliases (`|`)
///
/// ```rust,ignore
/// #[backwards_compat(
///     tag = "version",
///     versions(
///         #[serialize(with = to_v1)] 1 | 10: ConfigV1,
///         #[current] 2: Self,
///     )
/// )]
/// pub struct Config {
///     pub name: String,
///     pub port: u16,
/// }
/// ```
///
/// ### 6. Feature-Gated Historical Versions (`#[cfg(...)]`)
///
/// ```rust,ignore
/// #[cfg(feature = "legacy-v1")]
/// #[derive(Deserialize)]
/// pub struct ConfigV1 {
///     pub name: String,
/// }
///
/// #[backwards_compat(
///     tag = "version",
///     versions(
///         #[cfg(feature = "legacy-v1")] 1: ConfigV1,
///         2: Self,
///     )
/// )]
/// pub struct Config {
///     pub name: String,
///     pub port: u16,
/// }
/// ```
#[proc_macro_attribute]
pub fn backwards_compat(attr: TokenStream, item: TokenStream) -> TokenStream {
    backwards_compat_impl(attr.into(), item.into()).into()
}

fn contains_via_option(attr: &proc_macro2::TokenStream) -> bool {
    let mut iter = attr.clone().into_iter().peekable();
    while let Some(tt) = iter.next() {
        if let proc_macro2::TokenTree::Ident(ident) = tt
            && ident == "via"
        {
            match iter.peek() {
                Some(proc_macro2::TokenTree::Punct(p)) if p.as_char() == '=' => return true,
                Some(proc_macro2::TokenTree::Group(g))
                    if g.delimiter() == proc_macro2::Delimiter::Parenthesis =>
                {
                    return true;
                }
                _ => {}
            }
        }
    }
    false
}

fn respan_tokens(stream: proc_macro2::TokenStream, span: proc_macro2::Span) -> proc_macro2::TokenStream {
    stream
        .into_iter()
        .map(|mut tt| {
            match &mut tt {
                proc_macro2::TokenTree::Group(group) => {
                    let mut new_group =
                        proc_macro2::Group::new(group.delimiter(), respan_tokens(group.stream(), span));
                    new_group.set_span(span);
                    *group = new_group;
                }
                _ => tt.set_span(span),
            }
            tt
        })
        .collect()
}

fn respan_to_call_site<T: quote::ToTokens + syn::parse::Parse>(node: &T) -> T {
    let tokens = respan_tokens(quote::quote!(#node), proc_macro2::Span::call_site());
    syn::parse2(tokens).expect("respanning a parsed syntax node must remain valid syntax")
}

fn respan_generics_to_call_site(generics: &syn::Generics) -> syn::Generics {
    if generics.params.is_empty() && generics.where_clause.is_none() {
        return syn::Generics::default();
    }
    // Note: `syn::Generics::to_tokens` omits `where_clause`, so respan it separately.
    let mut respanned = if generics.params.is_empty() {
        syn::Generics::default()
    } else {
        respan_to_call_site(generics)
    };
    respanned.where_clause = generics.where_clause.as_ref().map(respan_to_call_site);
    respanned
}

/// Emits an uncalled, zero-cost helper function inside `const _: ()` that references each named
/// struct field using its original source span.
///
/// Inside an attribute proc-macro expansion, rust-analyzer's rename-at-definition-site requires
/// the field token to map to a `NameRef` (e.g. a pattern field reference), not just the
/// `RecordField` declaration itself. Destructuring with `<field>: _` avoids taking references
/// (so `#[repr(packed)]` structs remain valid) and avoids introducing a local binding, while
/// keeping `target_ident` and `generics` at `Span::call_site()` so hover/docs are unaffected.
fn generate_struct_rename_anchor(
    item_struct: &syn::ItemStruct,
    target_ident: &syn::Ident,
    generics: &syn::Generics,
) -> proc_macro2::TokenStream {
    let syn::Fields::Named(ref named) = item_struct.fields else {
        return proc_macro2::TokenStream::new();
    };
    if named.named.is_empty() {
        return proc_macro2::TokenStream::new();
    }

    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let field_pats = named.named.iter().filter_map(|field| {
        let field_ident = field.ident.as_ref()?;
        let cfg_attrs: Vec<_> = field.attrs.iter().filter(|a| a.path().is_ident("cfg")).collect();
        Some(quote::quote! {
            #(#cfg_attrs)*
            #field_ident: _,
        })
    });

    quote::quote! {
        const _: () = {
            #[allow(dead_code, unused_variables, deprecated, clippy::all)]
            fn __backwards_compat_ide_rename_anchor #impl_generics (__v: &#target_ident #ty_generics) #where_clause {
                let #target_ident { #(#field_pats)* .. } = __v;
            }
        };
    }
}

/// Emits an uncalled, zero-cost helper function inside `const _: ()` that references each enum
/// variant and named variant field using its original source span so rust-analyzer can rename
/// variants and fields directly from their definition sites.
fn generate_enum_rename_anchor(
    item_enum: &syn::ItemEnum,
    target_ident: &syn::Ident,
    generics: &syn::Generics,
) -> proc_macro2::TokenStream {
    if item_enum.variants.is_empty() {
        return proc_macro2::TokenStream::new();
    }

    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let arms = item_enum.variants.iter().map(|variant| {
        let v_ident = &variant.ident;
        let v_cfg_attrs: Vec<_> = variant
            .attrs
            .iter()
            .filter(|a| a.path().is_ident("cfg"))
            .collect();
        match &variant.fields {
            syn::Fields::Unit => quote::quote! {
                #(#v_cfg_attrs)*
                #target_ident::#v_ident => {}
            },
            syn::Fields::Unnamed(_) => quote::quote! {
                #(#v_cfg_attrs)*
                #target_ident::#v_ident(..) => {}
            },
            syn::Fields::Named(named) => {
                let field_pats = named.named.iter().filter_map(|field| {
                    let f_ident = field.ident.as_ref()?;
                    let f_cfg_attrs: Vec<_> =
                        field.attrs.iter().filter(|a| a.path().is_ident("cfg")).collect();
                    Some(quote::quote! {
                        #(#f_cfg_attrs)*
                        #f_ident: _,
                    })
                });
                quote::quote! {
                    #(#v_cfg_attrs)*
                    #target_ident::#v_ident { #(#field_pats)* .. } => {}
                }
            }
        }
    });

    quote::quote! {
        const _: () = {
            #[allow(dead_code, unused_variables, unreachable_patterns, deprecated, clippy::all)]
            fn __backwards_compat_ide_rename_anchor #impl_generics (__v: &#target_ident #ty_generics) #where_clause {
                match __v {
                    #(#arms)*
                    _ => {}
                }
            }
        };
    }
}

fn check_tag_field_collision(item_struct: &syn::ItemStruct, tag_field: &str) -> syn::Result<()> {
    let syn::Fields::Named(ref named) = item_struct.fields else {
        return Ok(());
    };

    let mut has_rename_all = false;
    for attr in &item_struct.attrs {
        if !attr.path().is_ident("serde") {
            continue;
        }
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("rename_all") {
                has_rename_all = true;
            }
            if meta.input.peek(syn::Token![=]) {
                let _: syn::Expr = meta.value()?.parse()?;
            } else if meta.input.peek(syn::token::Paren) {
                let content;
                syn::parenthesized!(content in meta.input);
                let _: proc_macro2::TokenStream = content.parse()?;
            }
            Ok(())
        });
    }

    for field in &named.named {
        let mut skip_or_flatten = false;
        let mut renames: Vec<String> = Vec::new();
        for attr in &field.attrs {
            if !attr.path().is_ident("serde") {
                continue;
            }
            let _ = attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("skip") || meta.path.is_ident("flatten") {
                    skip_or_flatten = true;
                } else if meta.path.is_ident("rename") {
                    if meta.input.peek(syn::Token![=]) {
                        let lit: syn::LitStr = meta.value()?.parse()?;
                        renames.push(lit.value());
                    } else if meta.input.peek(syn::token::Paren) {
                        meta.parse_nested_meta(|nested| {
                            if (nested.path.is_ident("serialize") || nested.path.is_ident("deserialize"))
                                && nested.input.peek(syn::Token![=])
                            {
                                let lit: syn::LitStr = nested.value()?.parse()?;
                                renames.push(lit.value());
                            } else if nested.input.peek(syn::Token![=]) {
                                let _: syn::Expr = nested.value()?.parse()?;
                            }
                            Ok(())
                        })?;
                    }
                } else if meta.input.peek(syn::Token![=]) {
                    let _: syn::Expr = meta.value()?.parse()?;
                } else if meta.input.peek(syn::token::Paren) {
                    let content;
                    syn::parenthesized!(content in meta.input);
                    let _: proc_macro2::TokenStream = content.parse()?;
                }
                Ok(())
            });
        }

        if skip_or_flatten {
            continue;
        }

        let collides = if !renames.is_empty() {
            renames.iter().any(|r| r == tag_field)
        } else if !has_rename_all {
            field.ident.as_ref().is_some_and(|ident| {
                let s = ident.to_string();
                s.strip_prefix("r#").unwrap_or(&s) == tag_field
            })
        } else {
            false
        };

        if collides {
            return Err(syn::Error::new_spanned(
                field,
                format!(
                    "field wire name `{tag_field}` collides with the `#[backwards_compat]` version tag field `{tag_field}`; rename the field, use `#[backwards_compat(tag = ...)]`, or use adjacent tagging `content = ...`"
                ),
            ));
        }
    }

    Ok(())
}

fn backwards_compat_impl(
    attr: proc_macro2::TokenStream,
    item: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    enum PreparedShadowSource {
        Struct(syn::ItemStruct),
        Enum(syn::ItemEnum),
    }

    struct PreparedItem {
        cleaned_item_tokens: proc_macro2::TokenStream,
        rename_anchor_tokens: proc_macro2::TokenStream,
        target_ident: syn::Ident,
        vis: syn::Visibility,
        generics: syn::Generics,
        derive_json_schema: bool,
        field_specs: Vec<field::FieldCompatSpec>,
        field_helper_tokens: Vec<proc_macro2::TokenStream>,
        shadow_source: PreparedShadowSource,
    }

    if contains_via_option(&attr) {
        return backwards_compat_via_impl(attr, item);
    }

    let attr_is_empty = attr.is_empty();
    let args = match syn::parse2::<BackwardsCompatArgs>(attr) {
        Ok(a) => a,
        Err(err) => return err.to_compile_error(),
    };
    let item = match syn::parse2::<syn::Item>(item) {
        Ok(i) => i,
        Err(err) => return err.to_compile_error(),
    };

    let call_site = proc_macro2::Span::call_site();
    let prepared = match item {
        syn::Item::Struct(mut item_struct) => {
            #[cfg(feature = "schemars")]
            let derive_json_schema = has_json_schema_derive(&item_struct.attrs);
            #[cfg(not(feature = "schemars"))]
            let derive_json_schema = false;

            let field_specs = match field::extract_struct_field_compat(&item_struct) {
                Ok(specs) => specs,
                Err(err) => return err.to_compile_error(),
            };
            let mut helpers = Vec::with_capacity(field_specs.len());
            for spec in &field_specs {
                match field::generate_field_helper(spec, derive_json_schema, args.serialize, args.deserialize)
                {
                    Ok(ts) => helpers.push(ts),
                    Err(err) => return err.to_compile_error(),
                }
            }

            if attr_is_empty && !field_specs.is_empty() {
                for spec in &field_specs {
                    if let Some(f) = item_struct.fields.iter_mut().nth(spec.field_idx) {
                        f.attrs.retain(|a| !a.path().is_ident("backwards_compat"));
                        let helper_str = spec.helper_ident.to_string();
                        f.attrs.push(syn::parse_quote!(#[serde(with = #helper_str)]));
                        #[cfg(feature = "schemars")]
                        if derive_json_schema && !field_has_schemars_with(&f.attrs) {
                            let schema_ty_str = spec.schema_helper_ty_str();
                            f.attrs
                                .push(syn::parse_quote!(#[schemars(with = #schema_ty_str)]));
                        }
                    }
                }
                return quote::quote! {
                    #item_struct
                    #(#helpers)*
                };
            }

            let mut cleaned_item_struct = item_struct.clone();
            clean_derive_attrs(&mut cleaned_item_struct.attrs, args.serialize, args.deserialize);
            let keep_serde_attrs = has_serde_derive(&cleaned_item_struct.attrs);
            cleaned_item_struct
                .attrs
                .retain(|attr| !is_stripped_domain_attr(attr, keep_serde_attrs));
            for field in &mut cleaned_item_struct.fields {
                field.attrs.retain(|attr| {
                    !is_stripped_domain_attr(attr, keep_serde_attrs)
                        && !attr.path().is_ident("backwards_compat")
                });
            }

            let mut target_ident = item_struct.ident.clone();
            target_ident.set_span(call_site);
            let vis = item_struct.vis.clone();
            let generics = respan_generics_to_call_site(&item_struct.generics);
            let rename_anchor_tokens = generate_struct_rename_anchor(&item_struct, &target_ident, &generics);
            PreparedItem {
                cleaned_item_tokens: quote::quote!(#cleaned_item_struct),
                rename_anchor_tokens,
                target_ident,
                vis,
                generics,
                derive_json_schema,
                field_specs,
                field_helper_tokens: helpers,
                shadow_source: PreparedShadowSource::Struct(item_struct),
            }
        }
        syn::Item::Enum(mut item_enum) => {
            #[cfg(feature = "schemars")]
            let derive_json_schema = has_json_schema_derive(&item_enum.attrs);
            #[cfg(not(feature = "schemars"))]
            let derive_json_schema = false;

            let field_specs = match field::extract_enum_field_compat(&item_enum) {
                Ok(specs) => specs,
                Err(err) => return err.to_compile_error(),
            };
            let mut helpers = Vec::with_capacity(field_specs.len());
            for spec in &field_specs {
                match field::generate_field_helper(spec, derive_json_schema, args.serialize, args.deserialize)
                {
                    Ok(ts) => helpers.push(ts),
                    Err(err) => return err.to_compile_error(),
                }
            }

            if attr_is_empty && !field_specs.is_empty() {
                for spec in &field_specs {
                    if let Some(v_idx) = spec.variant_idx
                        && let Some(variant) = item_enum.variants.iter_mut().nth(v_idx)
                        && let Some(f) = variant.fields.iter_mut().nth(spec.field_idx)
                    {
                        f.attrs.retain(|a| !a.path().is_ident("backwards_compat"));
                        let helper_str = spec.helper_ident.to_string();
                        f.attrs.push(syn::parse_quote!(#[serde(with = #helper_str)]));
                        #[cfg(feature = "schemars")]
                        if derive_json_schema && !field_has_schemars_with(&f.attrs) {
                            let schema_ty_str = spec.schema_helper_ty_str();
                            f.attrs
                                .push(syn::parse_quote!(#[schemars(with = #schema_ty_str)]));
                        }
                    }
                }
                return quote::quote! {
                    #item_enum
                    #(#helpers)*
                };
            }

            let mut cleaned_item_enum = item_enum.clone();
            clean_derive_attrs(&mut cleaned_item_enum.attrs, args.serialize, args.deserialize);
            let keep_serde_attrs = has_serde_derive(&cleaned_item_enum.attrs);
            cleaned_item_enum
                .attrs
                .retain(|attr| !is_stripped_domain_attr(attr, keep_serde_attrs));
            for variant in &mut cleaned_item_enum.variants {
                variant
                    .attrs
                    .retain(|attr| !is_stripped_domain_attr(attr, keep_serde_attrs));
                for field in &mut variant.fields {
                    field.attrs.retain(|attr| {
                        !is_stripped_domain_attr(attr, keep_serde_attrs)
                            && !attr.path().is_ident("backwards_compat")
                    });
                }
            }

            let mut target_ident = item_enum.ident.clone();
            target_ident.set_span(call_site);
            let vis = item_enum.vis.clone();
            let generics = respan_generics_to_call_site(&item_enum.generics);
            let rename_anchor_tokens = generate_enum_rename_anchor(&item_enum, &target_ident, &generics);
            PreparedItem {
                cleaned_item_tokens: quote::quote!(#cleaned_item_enum),
                rename_anchor_tokens,
                target_ident,
                vis,
                generics,
                derive_json_schema,
                field_specs,
                field_helper_tokens: helpers,
                shadow_source: PreparedShadowSource::Enum(item_enum),
            }
        }
        other => {
            return syn::Error::new_spanned(
                other,
                "#[backwards_compat] can only be applied to structs or enums",
            )
            .to_compile_error();
        }
    };

    let PreparedItem {
        cleaned_item_tokens,
        rename_anchor_tokens,
        target_ident,
        vis,
        generics,
        derive_json_schema,
        field_specs,
        field_helper_tokens,
        shadow_source,
    } = prepared;

    let (_, ty_generics, _) = generics.split_for_impl();
    let target_ty: syn::Type = syn::parse_quote!(#target_ident #ty_generics);

    let mut input = match args.into_input(target_ty, vis, generics) {
        Ok(i) => i,
        Err(err) => return err.to_compile_error(),
    };
    input.allow_attrs = match &shadow_source {
        PreparedShadowSource::Struct(s) => s
            .attrs
            .iter()
            .filter(|a| a.path().is_ident("allow"))
            .cloned()
            .collect(),
        PreparedShadowSource::Enum(e) => e
            .attrs
            .iter()
            .filter(|a| a.path().is_ident("allow"))
            .cloned()
            .collect(),
    };

    let dag_plan = match dag::resolve_dag(&input) {
        Ok(p) => p,
        Err(err) => return err.to_compile_error(),
    };

    let needs_shadow_wire = dag_plan.target_is_wire
        || dag_plan.serialize_target_is_wire
        || input.versions.iter().any(|v| v.ty == input.target_ty);

    if needs_shadow_wire
        && !input.external
        && input.content_field.is_none()
        && let PreparedShadowSource::Struct(ref orig_struct) = shadow_source
        && let Err(err) = check_tag_field_collision(orig_struct, &input.tag_field)
    {
        return err.to_compile_error();
    }

    if needs_shadow_wire {
        let shadow_opts = shadow::ShadowWireOptions {
            serialize: input.serialize,
            deserialize: input.deserialize,
            derive_json_schema,
        };
        let built = match shadow_source {
            PreparedShadowSource::Struct(item_struct) => {
                let shadow_struct = respan_to_call_site(&item_struct);
                shadow::build_struct_shadow_wire(&shadow_struct, &field_specs, shadow_opts)
            }
            PreparedShadowSource::Enum(item_enum) => {
                let shadow_enum = respan_to_call_site(&item_enum);
                shadow::build_enum_shadow_wire(&shadow_enum, &field_specs, shadow_opts)
            }
        };
        let shadow_info = codegen::ShadowWireInfo {
            owned_ident: &built.shadow_ident,
            borrowed_ident: &built.borrowed_ident,
            borrowed_ty_in_helper: built.borrowed_ty_in_helper,
            construct_borrowed: built.construct_borrowed,
            is_unit_struct: built.is_unit_struct,
        };

        let codegen_tokens = match codegen::generate_backwards_compat(
            &input,
            &dag_plan,
            codegen::SelfWireMode::Shadow(&shadow_info),
            Some(built.shadow_tokens),
            derive_json_schema,
        ) {
            Ok(t) => t,
            Err(err) => return err.to_compile_error(),
        };

        quote::quote! {
            #cleaned_item_tokens
            #rename_anchor_tokens
            #codegen_tokens
            #(#field_helper_tokens)*
        }
    } else {
        let codegen_tokens = match codegen::generate_backwards_compat(
            &input,
            &dag_plan,
            codegen::SelfWireMode::Disallowed,
            None,
            derive_json_schema,
        ) {
            Ok(t) => t,
            Err(err) => return err.to_compile_error(),
        };
        quote::quote! {
            #cleaned_item_tokens
            #rename_anchor_tokens
            #codegen_tokens
            #(#field_helper_tokens)*
        }
    }
}

fn reject_field_compat_attrs<'a>(
    fields: impl IntoIterator<Item = &'a syn::Field>,
) -> Option<proc_macro2::TokenStream> {
    for f in fields {
        if let Some(bc_attr) = f.attrs.iter().find(|a| a.path().is_ident("backwards_compat")) {
            return Some(
                syn::Error::new_spanned(
                    bc_attr,
                    "field-level `#[backwards_compat(...)]` cannot be used on a type with `#[backwards_compat(via = ...)]`",
                )
                .to_compile_error(),
            );
        }
    }
    None
}

fn backwards_compat_via_impl(
    attr: proc_macro2::TokenStream,
    item: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let args = match syn::parse2::<via::ViaArgs>(attr) {
        Ok(a) => a,
        Err(err) => return err.to_compile_error(),
    };
    let item = match syn::parse2::<syn::Item>(item) {
        Ok(i) => i,
        Err(err) => return err.to_compile_error(),
    };

    match item {
        syn::Item::Struct(mut item_struct) => {
            if let Some(err_tokens) = reject_field_compat_attrs(&item_struct.fields) {
                return err_tokens;
            }
            #[cfg(feature = "schemars")]
            let derive_json_schema = has_json_schema_derive(&item_struct.attrs);
            #[cfg(not(feature = "schemars"))]
            let derive_json_schema = false;

            #[cfg(feature = "schemars")]
            if derive_json_schema {
                item_struct.attrs.retain(|a| !a.path().is_ident("schemars"));
                for f in &mut item_struct.fields {
                    f.attrs.retain(|a| !a.path().is_ident("schemars"));
                }
            }
            clean_derive_attrs(&mut item_struct.attrs, args.serialize, args.deserialize);
            let codegen_struct = respan_to_call_site(&item_struct);
            let rename_anchor_tokens =
                generate_struct_rename_anchor(&item_struct, &codegen_struct.ident, &codegen_struct.generics);
            let codegen_tokens = via::generate_via(
                &args,
                &codegen_struct.ident,
                &codegen_struct.generics,
                derive_json_schema,
            );
            quote::quote! {
                #item_struct
                #rename_anchor_tokens
                #codegen_tokens
            }
        }
        syn::Item::Enum(mut item_enum) => {
            for variant in &item_enum.variants {
                if let Some(err_tokens) = reject_field_compat_attrs(&variant.fields) {
                    return err_tokens;
                }
            }
            #[cfg(feature = "schemars")]
            let derive_json_schema = has_json_schema_derive(&item_enum.attrs);
            #[cfg(not(feature = "schemars"))]
            let derive_json_schema = false;

            #[cfg(feature = "schemars")]
            if derive_json_schema {
                item_enum.attrs.retain(|a| !a.path().is_ident("schemars"));
                for variant in &mut item_enum.variants {
                    variant.attrs.retain(|a| !a.path().is_ident("schemars"));
                    for f in &mut variant.fields {
                        f.attrs.retain(|a| !a.path().is_ident("schemars"));
                    }
                }
            }
            clean_derive_attrs(&mut item_enum.attrs, args.serialize, args.deserialize);
            let codegen_enum = respan_to_call_site(&item_enum);
            let rename_anchor_tokens =
                generate_enum_rename_anchor(&item_enum, &codegen_enum.ident, &codegen_enum.generics);
            let codegen_tokens = via::generate_via(
                &args,
                &codegen_enum.ident,
                &codegen_enum.generics,
                derive_json_schema,
            );
            quote::quote! {
                #item_enum
                #rename_anchor_tokens
                #codegen_tokens
            }
        }
        other => syn::Error::new_spanned(
            other,
            "#[backwards_compat] can only be applied to structs or enums",
        )
        .to_compile_error(),
    }
}
