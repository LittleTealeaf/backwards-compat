use proc_macro::TokenStream;

mod codegen;
mod dag;
mod field;
mod parse;
mod via;

use parse::BackwardsCompatArgs;

fn is_serde_derive(path: &syn::Path, name: &str) -> bool {
    let segments: Vec<_> = path.segments.iter().collect();
    match segments.as_slice() {
        [single] => single.ident == name,
        [first, second] => first.ident == "serde" && second.ident == name,
        _ => false,
    }
}

fn is_forwarded_attr(attr: &syn::Attribute) -> bool {
    const FORWARDED_ATTRS: &[&str] = &["serde", "allow", "warn", "deny", "forbid", "cfg", "cfg_attr"];
    FORWARDED_ATTRS.iter().any(|&name| attr.path().is_ident(name))
}

fn clean_derive_attrs(attrs: &mut Vec<syn::Attribute>) {
    attrs.retain_mut(|attr| {
        if !attr.path().is_ident("derive") {
            return true;
        }
        let Ok(nested) =
            attr.parse_args_with(syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated)
        else {
            return true;
        };

        let retained: Vec<syn::Path> = nested
            .into_iter()
            .filter(|path| !is_serde_derive(path, "Serialize") && !is_serde_derive(path, "Deserialize"))
            .collect();

        if retained.is_empty() {
            return false;
        }
        *attr = syn::parse_quote!(#[derive(#(#retained),*)]);
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
/// - **`dump`** *(optional)*:
///   Prints the macro's generated token stream to `stderr` during compilation for inspection.
/// - **`versions(...)` / `versions = [...]` / `versions = { ... }`**:
///   Declares the list of schema versions and transition paths. The active current version is the
///   entry marked `#[current]`, or defaults to the last non-untagged primary entry (or `1` if empty).
/// - **`via = ProxyType` / `via(ProxyType)`** *(proxy redirection mode; mutually exclusive with `tag`, `content`, `versions`, and field-level `#[backwards_compat]`)*:
///   Delegates `Serialize`, `Deserialize`, `BackwardsCompat`, and `BackwardsCompatDeserialize`
///   through a separate versioned proxy type. Supports `try_from`, `try_into`, `fallible`,
///   `from = fn_path` / `from(fn_path)`, `to = fn_path` / `to(fn_path)`, and `dump`.
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
///   supporting primitives, foreign types (`#[with(...)]`), `Option<T>`, `Vec<T>`, generics,
///   lifetimes, const generics, and multiple fields of the same type with independent chains.
///
/// # Version Entry Directives
///
/// - **`tag: Type`**: Sequential chain step (e.g. `1: ModelV1, 2: ModelV2, 3: Self`). Integer tags
///   (`1`) and string tags (`"1"`) are matched strictly as native types.
/// - **`tag1 | tag2: Type`**: Inline tag aliases mapping multiple wire tags (including mixed integer
///   and string tags such as `1 | "1": ModelV1`) to the same type and migration path. The first tag
///   is canonical for `#[current]` / `#[serialize]`.
/// - **`_: Type` / `_: Self`**: Pre-backwards-compat (unversioned) entry used when the version tag
///   field is absent. Multiple `_` entries are supported and tried in declaration order (`#[serde(untagged)]`).
/// - **`tag: Type => target_tag` / `tag: Type => Self`**: Custom DAG transition jumping directly to
///   another declared version or directly to `Self`.
/// - **`#[current]`**: Explicitly designates the active current schema version (required only when
///   it is not the last primary entry in `versions`).
/// - **`#[serialize]` / `#[serialize(with = fn_path)]`**: Overrides which version is emitted during
///   serialization (useful for rolling deployments). Defaults to the current version.
/// - **`#[fallible]`**: Marks a transition as fallible, using `TryFrom` (or a `Result`-returning
///   `#[with]` function). Error types only need to implement `core::fmt::Display`.
/// - **`#[with(fn_path)]`**: Uses a custom conversion function `fn(FromTy) -> ToTy` (or
///   `fn(FromTy) -> Result<ToTy, E>` with `#[fallible]`) instead of `From` / `TryFrom`.
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
///         #[fallible] #[with(parse_port)] 1: String,
///         2: Self,
///     ))]
///     pub port: u16,
/// }
/// ```
///
/// ### 3. Fallible Migrations with `#[with]`
///
/// ```rust,ignore
/// #[backwards_compat(
///     tag = "ver",
///     versions(
///         #[fallible] #[with(parse_v1)] 1: ServerV1,
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
///         #[serialize(with = to_v1)] 1 | "1": ConfigV1,
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

fn backwards_compat_impl(
    attr: proc_macro2::TokenStream,
    item: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    struct PreparedItem {
        cleaned_item_tokens: proc_macro2::TokenStream,
        target_ident: syn::Ident,
        vis: syn::Visibility,
        generics: syn::Generics,
        field_helper_tokens: Vec<proc_macro2::TokenStream>,
        built_shadow_fn: Box<dyn FnOnce() -> BuiltShadowWire>,
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

    let prepared = match item {
        syn::Item::Struct(mut item_struct) => {
            let field_specs = match field::extract_struct_field_compat(&item_struct) {
                Ok(specs) => specs,
                Err(err) => return err.to_compile_error(),
            };
            let mut helpers = Vec::with_capacity(field_specs.len());
            for spec in &field_specs {
                match field::generate_field_helper(spec) {
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
                    }
                }
                return quote::quote! {
                    #item_struct
                    #(#helpers)*
                };
            }

            let mut cleaned_item_struct = item_struct.clone();
            cleaned_item_struct
                .attrs
                .retain(|attr| !attr.path().is_ident("serde"));
            clean_derive_attrs(&mut cleaned_item_struct.attrs);
            for field in &mut cleaned_item_struct.fields {
                field.attrs.retain(|attr| {
                    !attr.path().is_ident("serde") && !attr.path().is_ident("backwards_compat")
                });
            }

            let target_ident = item_struct.ident.clone();
            let vis = item_struct.vis.clone();
            let generics = item_struct.generics.clone();
            PreparedItem {
                cleaned_item_tokens: quote::quote!(#cleaned_item_struct),
                target_ident,
                vis,
                generics,
                field_helper_tokens: helpers,
                built_shadow_fn: Box::new(move || build_struct_shadow_wire(&item_struct, &field_specs)),
            }
        }
        syn::Item::Enum(mut item_enum) => {
            let field_specs = match field::extract_enum_field_compat(&item_enum) {
                Ok(specs) => specs,
                Err(err) => return err.to_compile_error(),
            };
            let mut helpers = Vec::with_capacity(field_specs.len());
            for spec in &field_specs {
                match field::generate_field_helper(spec) {
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
                    }
                }
                return quote::quote! {
                    #item_enum
                    #(#helpers)*
                };
            }

            let mut cleaned_item_enum = item_enum.clone();
            cleaned_item_enum
                .attrs
                .retain(|attr| !attr.path().is_ident("serde"));
            clean_derive_attrs(&mut cleaned_item_enum.attrs);
            for variant in &mut cleaned_item_enum.variants {
                variant.attrs.retain(|attr| !attr.path().is_ident("serde"));
                for field in &mut variant.fields {
                    field.attrs.retain(|attr| {
                        !attr.path().is_ident("serde") && !attr.path().is_ident("backwards_compat")
                    });
                }
            }

            let target_ident = item_enum.ident.clone();
            let vis = item_enum.vis.clone();
            let generics = item_enum.generics.clone();
            PreparedItem {
                cleaned_item_tokens: quote::quote!(#cleaned_item_enum),
                target_ident,
                vis,
                generics,
                field_helper_tokens: helpers,
                built_shadow_fn: Box::new(move || build_enum_shadow_wire(&item_enum, &field_specs)),
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
        target_ident,
        vis,
        generics,
        field_helper_tokens,
        built_shadow_fn,
    } = prepared;

    let (_, ty_generics, _) = generics.split_for_impl();
    let target_ty: syn::Type = syn::parse_quote!(#target_ident #ty_generics);

    let input = match args.into_input(target_ty, vis, generics) {
        Ok(i) => i,
        Err(err) => return err.to_compile_error(),
    };

    let dag_plan = match dag::resolve_dag(&input) {
        Ok(p) => p,
        Err(err) => return err.to_compile_error(),
    };

    let needs_shadow_wire = dag_plan.target_is_wire
        || dag_plan.serialize_target_is_wire
        || input.versions.iter().any(|v| v.ty == input.target_ty);

    if needs_shadow_wire {
        let built = built_shadow_fn();
        let shadow_info = codegen::ShadowWireInfo {
            owned_ident: &built.shadow_ident,
            borrowed_ident: &built.borrowed_ident,
            borrowed_ty_in_helper: built.borrowed_ty_in_helper,
            construct_borrowed: built.construct_borrowed,
            is_unit_struct: built.is_unit_struct,
        };

        let codegen_tokens = match codegen::generate_backwards_compat(
            &input,
            codegen::SelfWireMode::Shadow(&shadow_info),
            Some(built.shadow_tokens),
        ) {
            Ok(t) => t,
            Err(err) => return err.to_compile_error(),
        };

        quote::quote! {
            #cleaned_item_tokens
            #codegen_tokens
            #(#field_helper_tokens)*
        }
    } else {
        let codegen_tokens =
            match codegen::generate_backwards_compat(&input, codegen::SelfWireMode::Disallowed, None) {
                Ok(t) => t,
                Err(err) => return err.to_compile_error(),
            };
        quote::quote! {
            #cleaned_item_tokens
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
            clean_derive_attrs(&mut item_struct.attrs);
            let codegen_tokens = via::generate_via(&args, &item_struct.ident, &item_struct.generics);
            quote::quote! {
                #item_struct
                #codegen_tokens
            }
        }
        syn::Item::Enum(mut item_enum) => {
            for variant in &item_enum.variants {
                if let Some(err_tokens) = reject_field_compat_attrs(&variant.fields) {
                    return err_tokens;
                }
            }
            clean_derive_attrs(&mut item_enum.attrs);
            let codegen_tokens = via::generate_via(&args, &item_enum.ident, &item_enum.generics);
            quote::quote! {
                #item_enum
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

struct BuiltShadowWire {
    shadow_ident: syn::Ident,
    borrowed_ident: syn::Ident,
    borrowed_ty_in_helper: proc_macro2::TokenStream,
    construct_borrowed: proc_macro2::TokenStream,
    shadow_tokens: proc_macro2::TokenStream,
    is_unit_struct: bool,
}

fn build_borrowed_generics(
    generics: &syn::Generics,
    borrowed_ident: &syn::Ident,
) -> (syn::Generics, proc_macro2::TokenStream) {
    let mut borrowed_generics = generics.clone();
    let wire_lifetime: syn::Lifetime = syn::parse_quote!('__wire);
    let wire_lifetime_param: syn::GenericParam = syn::parse_quote!('__wire);
    borrowed_generics.params.insert(0, wire_lifetime_param);
    for param in &mut borrowed_generics.params {
        match param {
            syn::GenericParam::Type(type_param) => {
                type_param
                    .bounds
                    .push(syn::TypeParamBound::Lifetime(wire_lifetime.clone()));
            }
            syn::GenericParam::Lifetime(lifetime_def) => {
                if lifetime_def.lifetime != wire_lifetime {
                    lifetime_def.bounds.push(wire_lifetime.clone());
                }
            }
            syn::GenericParam::Const(_) => {}
        }
    }

    let mut borrowed_args = syn::punctuated::Punctuated::<syn::GenericArgument, syn::Token![,]>::new();
    borrowed_args.push(syn::GenericArgument::Lifetime(syn::parse_quote!('__a)));
    for param in &generics.params {
        match param {
            syn::GenericParam::Type(tp) => {
                let ident = &tp.ident;
                borrowed_args.push(syn::GenericArgument::Type(syn::parse_quote!(#ident)));
            }
            syn::GenericParam::Lifetime(lp) => {
                let lifetime = &lp.lifetime;
                borrowed_args.push(syn::GenericArgument::Lifetime(lifetime.clone()));
            }
            syn::GenericParam::Const(cp) => {
                let ident = &cp.ident;
                borrowed_args.push(syn::GenericArgument::Const(syn::parse_quote!(#ident)));
            }
        }
    }

    let ty_in_helper = quote::quote!(#borrowed_ident<#borrowed_args>);
    (borrowed_generics, ty_in_helper)
}

fn should_mark_transparent(item_struct: &syn::ItemStruct) -> bool {
    let syn::Fields::Unnamed(ref unnamed) = item_struct.fields else {
        return false;
    };
    if unnamed.unnamed.len() != 1 {
        return false;
    }
    if item_struct.attrs.iter().any(|attr| attr.path().is_ident("serde")) {
        return false;
    }
    if let Some(field) = unnamed.unnamed.first() {
        for attr in &field.attrs {
            if attr.path().is_ident("serde") {
                let mut has_skip = false;
                let _ = attr.parse_nested_meta(|meta| {
                    if meta.path.is_ident("skip")
                        || meta.path.is_ident("skip_serializing")
                        || meta.path.is_ident("skip_deserializing")
                    {
                        has_skip = true;
                    }
                    Ok(())
                });
                if has_skip {
                    return false;
                }
            }
        }
    }
    true
}

fn build_struct_shadow_wire(
    item_struct: &syn::ItemStruct,
    field_specs: &[field::FieldCompatSpec],
) -> BuiltShadowWire {
    let call_site_span = proc_macro2::Span::call_site();
    let target_ident = &item_struct.ident;
    let shadow_ident = quote::format_ident!("__{}ShadowWire", target_ident, span = call_site_span);
    let borrowed_ident = quote::format_ident!("__{}BorrowedWire", target_ident, span = call_site_span);
    let find_spec = |idx: usize| {
        field_specs
            .iter()
            .find(|s| s.variant_idx.is_none() && s.field_idx == idx)
    };

    let generics = &item_struct.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let semi = &item_struct.semi_token;

    let preserved_attrs: Vec<_> = item_struct
        .attrs
        .iter()
        .filter(|attr| is_forwarded_attr(attr))
        .cloned()
        .collect();

    let transparent_attr = if should_mark_transparent(item_struct) {
        quote::quote!(#[serde(transparent)])
    } else {
        proc_macro2::TokenStream::new()
    };

    let mut shadow_fields = item_struct.fields.clone();
    for (idx, field) in shadow_fields.iter_mut().enumerate() {
        field.attrs.retain(is_forwarded_attr);
        if let Some(spec) = find_spec(idx) {
            let de_with = format!("{}::deserialize", spec.helper_ident);
            field
                .attrs
                .push(syn::parse_quote!(#[serde(deserialize_with = #de_with)]));
        }
        if let Some(ref mut ident) = field.ident {
            ident.set_span(call_site_span);
        }
    }

    let owned_struct_def = quote::quote! {
        #[allow(non_camel_case_types, dead_code, missing_debug_implementations)]
        #[doc(hidden)]
        #[derive(::serde::Deserialize)]
        #transparent_attr
        #(#preserved_attrs)*
        struct #shadow_ident #generics #shadow_fields #semi
    };

    let has_fields = !item_struct.fields.is_empty() && !matches!(item_struct.fields, syn::Fields::Unit);

    let (borrowed_struct_def, construct_borrowed, borrowed_ty_in_helper) = if has_fields {
        let (borrowed_generics, ty_in_helper) = build_borrowed_generics(generics, &borrowed_ident);
        let (borrowed_impl_generics, _, borrowed_where_clause) = borrowed_generics.split_for_impl();

        match &item_struct.fields {
            syn::Fields::Named(named) => {
                let mut borrowed_named = Vec::new();
                let mut field_idents = Vec::new();
                for (i, f) in named.named.iter().enumerate() {
                    let mut field_attrs: Vec<_> =
                        f.attrs.iter().filter(|a| is_forwarded_attr(a)).cloned().collect();
                    if let Some(spec) = find_spec(i) {
                        let ser_with = format!("{}::serialize_ref", spec.helper_ident);
                        field_attrs.push(syn::parse_quote!(#[serde(serialize_with = #ser_with)]));
                    }
                    let field_ident = f.ident.as_ref().unwrap();
                    let mut borrowed_field_ident = field_ident.clone();
                    borrowed_field_ident.set_span(call_site_span);
                    let field_ty = &f.ty;
                    borrowed_named.push(quote::quote! {
                        #(#field_attrs)*
                        #borrowed_field_ident: &'__wire #field_ty
                    });
                    field_idents.push(field_ident);
                }
                let s_def = quote::quote! {
                    #[allow(non_camel_case_types, dead_code, missing_debug_implementations)]
                    #[doc(hidden)]
                    #[derive(::serde::Serialize)]
                    #(#preserved_attrs)*
                    struct #borrowed_ident #borrowed_impl_generics #borrowed_where_clause {
                        #(#borrowed_named,)*
                    }
                };
                let c_borrowed = quote::quote! {
                    #borrowed_ident { #( #field_idents: &self.#field_idents ),* }
                };
                (s_def, c_borrowed, ty_in_helper)
            }
            syn::Fields::Unnamed(unnamed) => {
                let mut borrowed_unnamed = Vec::new();
                let mut indices = Vec::new();
                for (i, f) in unnamed.unnamed.iter().enumerate() {
                    let mut field_attrs: Vec<_> =
                        f.attrs.iter().filter(|a| is_forwarded_attr(a)).cloned().collect();
                    if let Some(spec) = find_spec(i) {
                        let ser_with = format!("{}::serialize_ref", spec.helper_ident);
                        field_attrs.push(syn::parse_quote!(#[serde(serialize_with = #ser_with)]));
                    }
                    let field_ty = &f.ty;
                    borrowed_unnamed.push(quote::quote! {
                        #(#field_attrs)*
                        &'__wire #field_ty
                    });
                    indices.push(syn::Index::from(i));
                }
                let s_def = quote::quote! {
                    #[allow(non_camel_case_types, dead_code, missing_debug_implementations)]
                    #[doc(hidden)]
                    #[derive(::serde::Serialize)]
                    #transparent_attr
                    #(#preserved_attrs)*
                    struct #borrowed_ident #borrowed_impl_generics ( #(#borrowed_unnamed,)* ) #borrowed_where_clause;
                };
                let c_borrowed = quote::quote! {
                    #borrowed_ident ( #( &self.#indices ),* )
                };
                (s_def, c_borrowed, ty_in_helper)
            }
            syn::Fields::Unit => unreachable!(),
        }
    } else {
        let s_def = quote::quote! {
            #[allow(non_camel_case_types, dead_code, missing_debug_implementations)]
            #[doc(hidden)]
            #[derive(::serde::Serialize)]
            #(#preserved_attrs)*
            struct #borrowed_ident #generics #where_clause;
        };
        let c_borrowed = quote::quote! {
            #borrowed_ident
        };
        let ty_in_helper = quote::quote!(#borrowed_ident #ty_generics);
        (s_def, c_borrowed, ty_in_helper)
    };

    let from_impls = match &shadow_fields {
        syn::Fields::Named(named) => {
            let field_idents: Vec<_> = named.named.iter().map(|f| f.ident.as_ref().unwrap()).collect();
            quote::quote! {
                impl #impl_generics ::core::convert::From<#target_ident #ty_generics> for #shadow_ident #ty_generics #where_clause {
                    fn from(val: #target_ident #ty_generics) -> Self {
                        Self { #( #field_idents: val.#field_idents ),* }
                    }
                }
                impl #impl_generics ::core::convert::From<#shadow_ident #ty_generics> for #target_ident #ty_generics #where_clause {
                    fn from(val: #shadow_ident #ty_generics) -> Self {
                        Self { #( #field_idents: val.#field_idents ),* }
                    }
                }
            }
        }
        syn::Fields::Unnamed(unnamed) => {
            let indices: Vec<_> = (0..unnamed.unnamed.len()).map(syn::Index::from).collect();
            quote::quote! {
                impl #impl_generics ::core::convert::From<#target_ident #ty_generics> for #shadow_ident #ty_generics #where_clause {
                    fn from(val: #target_ident #ty_generics) -> Self {
                        Self ( #( val.#indices ),* )
                    }
                }
                impl #impl_generics ::core::convert::From<#shadow_ident #ty_generics> for #target_ident #ty_generics #where_clause {
                    fn from(val: #shadow_ident #ty_generics) -> Self {
                        Self ( #( val.#indices ),* )
                    }
                }
            }
        }
        syn::Fields::Unit => {
            quote::quote! {
                impl #impl_generics ::core::convert::From<#target_ident #ty_generics> for #shadow_ident #ty_generics #where_clause {
                    fn from(_val: #target_ident #ty_generics) -> Self {
                        Self
                    }
                }
                impl #impl_generics ::core::convert::From<#shadow_ident #ty_generics> for #target_ident #ty_generics #where_clause {
                    fn from(_val: #shadow_ident #ty_generics) -> Self {
                        Self
                    }
                }
            }
        }
    };

    let shadow_tokens = quote::quote! {
        #owned_struct_def
        #borrowed_struct_def
        #from_impls
    };

    BuiltShadowWire {
        shadow_ident,
        borrowed_ident,
        borrowed_ty_in_helper,
        construct_borrowed,
        shadow_tokens,
        is_unit_struct: matches!(item_struct.fields, syn::Fields::Unit),
    }
}

fn build_enum_shadow_wire(
    item_enum: &syn::ItemEnum,
    field_specs: &[field::FieldCompatSpec],
) -> BuiltShadowWire {
    let call_site_span = proc_macro2::Span::call_site();
    let target_ident = &item_enum.ident;
    let shadow_ident = quote::format_ident!("__{}ShadowWire", target_ident, span = call_site_span);
    let borrowed_ident = quote::format_ident!("__{}BorrowedWire", target_ident, span = call_site_span);
    let find_spec = |v_idx: usize, f_idx: usize| {
        field_specs
            .iter()
            .find(|s| s.variant_idx == Some(v_idx) && s.field_idx == f_idx)
    };

    let generics = &item_enum.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let preserved_attrs: Vec<_> = item_enum
        .attrs
        .iter()
        .filter(|attr| is_forwarded_attr(attr))
        .cloned()
        .collect();

    let mut owned_variants = item_enum.variants.clone();
    for (v_idx, variant) in owned_variants.iter_mut().enumerate() {
        variant.attrs.retain(is_forwarded_attr);
        for (f_idx, field) in variant.fields.iter_mut().enumerate() {
            field.attrs.retain(is_forwarded_attr);
            if let Some(spec) = find_spec(v_idx, f_idx) {
                let de_with = format!("{}::deserialize", spec.helper_ident);
                field
                    .attrs
                    .push(syn::parse_quote!(#[serde(deserialize_with = #de_with)]));
            }
            if let Some(ref mut ident) = field.ident {
                ident.set_span(call_site_span);
            }
        }
    }

    let owned_enum_def = quote::quote! {
        #[allow(non_camel_case_types, dead_code, missing_debug_implementations)]
        #[doc(hidden)]
        #[derive(::serde::Deserialize)]
        #(#preserved_attrs)*
        enum #shadow_ident #generics #where_clause {
            #owned_variants
        }
    };

    let has_fields = item_enum
        .variants
        .iter()
        .any(|v| !v.fields.is_empty() && !matches!(v.fields, syn::Fields::Unit));

    let mut borrowed_variants = Vec::new();
    let mut construct_borrowed_arms = Vec::new();
    let mut to_shadow_arms = Vec::new();
    let mut from_shadow_arms = Vec::new();

    for (v_idx, variant) in item_enum.variants.iter().enumerate() {
        let v_ident = &variant.ident;
        let v_forwarded_attrs: Vec<_> = variant
            .attrs
            .iter()
            .filter(|a| is_forwarded_attr(a))
            .cloned()
            .collect();
        let v_cfg_attrs: Vec<_> = variant
            .attrs
            .iter()
            .filter(|a| a.path().is_ident("cfg"))
            .cloned()
            .collect();

        match &variant.fields {
            syn::Fields::Unit => {
                borrowed_variants.push(quote::quote! {
                    #(#v_forwarded_attrs)*
                    #v_ident
                });
                construct_borrowed_arms.push(quote::quote! {
                    #(#v_cfg_attrs)*
                    Self::#v_ident => #borrowed_ident::#v_ident
                });
                to_shadow_arms.push(quote::quote! {
                    #(#v_cfg_attrs)*
                    #target_ident::#v_ident => Self::#v_ident
                });
                from_shadow_arms.push(quote::quote! {
                    #(#v_cfg_attrs)*
                    #shadow_ident::#v_ident => Self::#v_ident
                });
            }
            syn::Fields::Unnamed(unnamed) => {
                let mut borrowed_fields = Vec::new();
                let mut bindings = Vec::new();
                for (i, f) in unnamed.unnamed.iter().enumerate() {
                    let mut f_attrs: Vec<_> =
                        f.attrs.iter().filter(|a| is_forwarded_attr(a)).cloned().collect();
                    if let Some(spec) = find_spec(v_idx, i) {
                        let ser_with = format!("{}::serialize_ref", spec.helper_ident);
                        f_attrs.push(syn::parse_quote!(#[serde(serialize_with = #ser_with)]));
                    }
                    let f_ty = &f.ty;
                    borrowed_fields.push(quote::quote! {
                        #(#f_attrs)*
                        &'__wire #f_ty
                    });
                    bindings.push(quote::format_ident!("__f{}", i, span = call_site_span));
                }
                borrowed_variants.push(quote::quote! {
                    #(#v_forwarded_attrs)*
                    #v_ident(#(#borrowed_fields,)*)
                });
                construct_borrowed_arms.push(quote::quote! {
                    #(#v_cfg_attrs)*
                    Self::#v_ident(#(#bindings),*) => #borrowed_ident::#v_ident(#(#bindings),*)
                });
                to_shadow_arms.push(quote::quote! {
                    #(#v_cfg_attrs)*
                    #target_ident::#v_ident(#(#bindings),*) => Self::#v_ident(#(#bindings),*)
                });
                from_shadow_arms.push(quote::quote! {
                    #(#v_cfg_attrs)*
                    #shadow_ident::#v_ident(#(#bindings),*) => Self::#v_ident(#(#bindings),*)
                });
            }
            syn::Fields::Named(named) => {
                let mut borrowed_fields = Vec::new();
                let mut field_idents = Vec::new();
                for (i, f) in named.named.iter().enumerate() {
                    let mut f_attrs: Vec<_> =
                        f.attrs.iter().filter(|a| is_forwarded_attr(a)).cloned().collect();
                    if let Some(spec) = find_spec(v_idx, i) {
                        let ser_with = format!("{}::serialize_ref", spec.helper_ident);
                        f_attrs.push(syn::parse_quote!(#[serde(serialize_with = #ser_with)]));
                    }
                    let f_ident = f.ident.as_ref().unwrap();
                    let mut borrowed_f_ident = f_ident.clone();
                    borrowed_f_ident.set_span(call_site_span);
                    let f_ty = &f.ty;
                    borrowed_fields.push(quote::quote! {
                        #(#f_attrs)*
                        #borrowed_f_ident: &'__wire #f_ty
                    });
                    field_idents.push(borrowed_f_ident);
                }
                borrowed_variants.push(quote::quote! {
                    #(#v_forwarded_attrs)*
                    #v_ident { #(#borrowed_fields,)* }
                });
                construct_borrowed_arms.push(quote::quote! {
                    #(#v_cfg_attrs)*
                    Self::#v_ident { #(#field_idents),* } => #borrowed_ident::#v_ident { #(#field_idents),* }
                });
                to_shadow_arms.push(quote::quote! {
                    #(#v_cfg_attrs)*
                    #target_ident::#v_ident { #(#field_idents),* } => Self::#v_ident { #(#field_idents),* }
                });
                from_shadow_arms.push(quote::quote! {
                    #(#v_cfg_attrs)*
                    #shadow_ident::#v_ident { #(#field_idents),* } => Self::#v_ident { #(#field_idents),* }
                });
            }
        }
    }

    let (borrowed_enum_def, borrowed_ty_in_helper) = if has_fields {
        let (borrowed_generics, ty_in_helper) = build_borrowed_generics(generics, &borrowed_ident);
        let (borrowed_impl_generics, _, borrowed_where_clause) = borrowed_generics.split_for_impl();
        let def = quote::quote! {
            #[allow(non_camel_case_types, dead_code, missing_debug_implementations)]
            #[doc(hidden)]
            #[derive(::serde::Serialize)]
            #(#preserved_attrs)*
            enum #borrowed_ident #borrowed_impl_generics #borrowed_where_clause {
                #(#borrowed_variants,)*
            }
        };
        (def, ty_in_helper)
    } else {
        let def = quote::quote! {
            #[allow(non_camel_case_types, dead_code, missing_debug_implementations)]
            #[doc(hidden)]
            #[derive(::serde::Serialize)]
            #(#preserved_attrs)*
            enum #borrowed_ident #generics #where_clause {
                #(#borrowed_variants,)*
            }
        };
        let ty_in_helper = quote::quote!(#borrowed_ident #ty_generics);
        (def, ty_in_helper)
    };

    let construct_borrowed = if item_enum.variants.is_empty() {
        quote::quote!(match *self {})
    } else {
        quote::quote!(match self { #(#construct_borrowed_arms,)* })
    };

    let from_impls = quote::quote! {
        impl #impl_generics ::core::convert::From<#target_ident #ty_generics> for #shadow_ident #ty_generics #where_clause {
            fn from(val: #target_ident #ty_generics) -> Self {
                match val {
                    #(#to_shadow_arms,)*
                }
            }
        }
        impl #impl_generics ::core::convert::From<#shadow_ident #ty_generics> for #target_ident #ty_generics #where_clause {
            fn from(val: #shadow_ident #ty_generics) -> Self {
                match val {
                    #(#from_shadow_arms,)*
                }
            }
        }
    };

    let shadow_tokens = quote::quote! {
        #owned_enum_def
        #borrowed_enum_def
        #from_impls
    };

    BuiltShadowWire {
        shadow_ident,
        borrowed_ident,
        borrowed_ty_in_helper,
        construct_borrowed,
        shadow_tokens,
        is_unit_struct: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_serde_derive_variants() {
        let p1: syn::Path = syn::parse_quote!(Serialize);
        let p2: syn::Path = syn::parse_quote!(serde::Serialize);
        let p3: syn::Path = syn::parse_quote!(::serde::Serialize);
        let p4: syn::Path = syn::parse_quote!(Deserialize);
        let p5: syn::Path = syn::parse_quote!(serde::Deserialize);
        let p6: syn::Path = syn::parse_quote!(::serde::Deserialize);
        let p_other: syn::Path = syn::parse_quote!(other::Serialize);
        let p_debug: syn::Path = syn::parse_quote!(Debug);

        assert!(is_serde_derive(&p1, "Serialize"));
        assert!(is_serde_derive(&p2, "Serialize"));
        assert!(is_serde_derive(&p3, "Serialize"));
        assert!(!is_serde_derive(&p1, "Deserialize"));

        assert!(is_serde_derive(&p4, "Deserialize"));
        assert!(is_serde_derive(&p5, "Deserialize"));
        assert!(is_serde_derive(&p6, "Deserialize"));

        assert!(!is_serde_derive(&p_other, "Serialize"));
        assert!(!is_serde_derive(&p_debug, "Serialize"));
    }

    #[test]
    fn test_is_forwarded_attr() {
        let doc_attr: syn::Attribute = syn::parse_quote!(#[doc = "documentation"]);
        let serde_attr: syn::Attribute = syn::parse_quote!(#[serde(rename = "foo")]);
        let allow_attr: syn::Attribute = syn::parse_quote!(#[allow(dead_code)]);
        let warn_attr: syn::Attribute = syn::parse_quote!(#[warn(unused)]);
        let deny_attr: syn::Attribute = syn::parse_quote!(#[deny(clippy::all)]);
        let forbid_attr: syn::Attribute = syn::parse_quote!(#[forbid(unsafe_code)]);
        let cfg_attr: syn::Attribute = syn::parse_quote!(#[cfg(feature = "extra")]);
        let cfg_attr_attr: syn::Attribute = syn::parse_quote!(#[cfg_attr(test, derive(Debug))]);
        let derive_attr: syn::Attribute = syn::parse_quote!(#[derive(Debug)]);
        let custom_attr: syn::Attribute = syn::parse_quote!(#[custom_attribute]);

        assert!(!is_forwarded_attr(&doc_attr));
        assert!(is_forwarded_attr(&serde_attr));
        assert!(is_forwarded_attr(&allow_attr));
        assert!(is_forwarded_attr(&warn_attr));
        assert!(is_forwarded_attr(&deny_attr));
        assert!(is_forwarded_attr(&forbid_attr));
        assert!(is_forwarded_attr(&cfg_attr));
        assert!(is_forwarded_attr(&cfg_attr_attr));

        assert!(!is_forwarded_attr(&derive_attr));
        assert!(!is_forwarded_attr(&custom_attr));
    }

    #[test]
    fn test_clean_derive_attrs() {
        let mut attrs = vec![
            syn::parse_quote!(#[derive(Debug, Serialize, Clone, ::serde::Deserialize)]),
            syn::parse_quote!(#[allow(unused)]),
        ];
        clean_derive_attrs(&mut attrs);
        assert_eq!(attrs.len(), 2);
        let formatted = quote::quote!(#(#attrs)*).to_string();
        assert!(formatted.contains("Debug"));
        assert!(formatted.contains("Clone"));
        assert!(!formatted.contains("Serialize"));
        assert!(!formatted.contains("Deserialize"));

        let mut all_serde_attrs =
            vec![syn::parse_quote!(#[derive(Serialize, Deserialize, serde::Serialize)])];
        clean_derive_attrs(&mut all_serde_attrs);
        assert_eq!(all_serde_attrs.len(), 0);
    }

    #[test]
    fn test_build_enum_shadow_wire_mixed_variants_generics_and_cfg() {
        let item_enum: syn::ItemEnum = syn::parse_quote! {
            #[serde(rename_all = "snake_case")]
            pub enum MyEnum<'a, T: Clone> {
                Empty,
                Tuple(&'a str, T),
                Struct {
                    id: u32,
                    payload: T,
                },
                #[cfg(feature = "extra")]
                Extra(u64),
            }
        };

        let built = build_enum_shadow_wire(&item_enum, &[]);
        assert!(!built.is_unit_struct);

        let shadow_str = built.shadow_tokens.to_string();
        let construct_str = built.construct_borrowed.to_string();
        let helper_ty_str = built.borrowed_ty_in_helper.to_string();

        assert!(shadow_str.contains("enum __MyEnumShadowWire < 'a , T : Clone >"));
        assert!(
            shadow_str.contains("enum __MyEnumBorrowedWire < '__wire , 'a : '__wire , T : Clone + '__wire >")
        );
        assert!(helper_ty_str.contains("__MyEnumBorrowedWire < '__a , 'a , T >"));
        assert!(shadow_str.contains("& '__wire & 'a str"));
        assert!(shadow_str.contains("& '__wire T"));
        assert!(shadow_str.contains("id : & '__wire u32"));
        assert!(shadow_str.contains("# [cfg (feature = \"extra\")] Extra (& '__wire u64 ,)"));
        assert!(
            shadow_str
                .contains("# [cfg (feature = \"extra\")] MyEnum :: Extra (__f0) => Self :: Extra (__f0)")
        );
        assert!(shadow_str.contains(
            "# [cfg (feature = \"extra\")] __MyEnumShadowWire :: Extra (__f0) => Self :: Extra (__f0)"
        ));
        assert!(construct_str.contains(
            "# [cfg (feature = \"extra\")] Self :: Extra (__f0) => __MyEnumBorrowedWire :: Extra (__f0)"
        ));

        syn::parse2::<syn::File>(built.shadow_tokens).unwrap();
        syn::parse2::<syn::Expr>(built.construct_borrowed).unwrap();
    }

    #[test]
    fn test_build_enum_shadow_wire_all_unit_no_wire_lifetime() {
        let item_enum: syn::ItemEnum = syn::parse_quote! {
            pub enum Status {
                Active,
                Inactive,
            }
        };

        let built = build_enum_shadow_wire(&item_enum, &[]);
        assert!(!built.is_unit_struct);

        let shadow_str = built.shadow_tokens.to_string();
        let helper_ty_str = built.borrowed_ty_in_helper.to_string();

        assert!(!shadow_str.contains("'__wire"));
        assert!(!helper_ty_str.contains("'__a"));
        assert_eq!(helper_ty_str, "__StatusBorrowedWire");

        syn::parse2::<syn::File>(built.shadow_tokens).unwrap();
        syn::parse2::<syn::Expr>(built.construct_borrowed).unwrap();
    }

    #[test]
    fn test_build_enum_shadow_wire_empty_enum() {
        let item_enum: syn::ItemEnum = syn::parse_quote! {
            pub enum Never {}
        };

        let built = build_enum_shadow_wire(&item_enum, &[]);
        assert!(!built.is_unit_struct);

        let shadow_str = built.shadow_tokens.to_string();
        let construct_str = built.construct_borrowed.to_string();

        assert!(!shadow_str.contains("'__wire"));
        assert_eq!(construct_str, "match * self { }");
        assert!(shadow_str.contains("match val { }"));

        syn::parse2::<syn::File>(built.shadow_tokens).unwrap();
        syn::parse2::<syn::Expr>(built.construct_borrowed).unwrap();
    }

    #[test]
    fn test_build_borrowed_generics_with_const_param() {
        let item: syn::ItemStruct = syn::parse_quote! {
            pub struct Buf<const N: usize>([u8; N]);
        };
        let borrowed_ident = quote::format_ident!("__BufBorrowedWire");
        let (borrowed_generics, ty_in_helper) = build_borrowed_generics(&item.generics, &borrowed_ident);

        assert_eq!(
            quote::quote!(#borrowed_generics).to_string(),
            "< '__wire , const N : usize >"
        );
        assert_eq!(ty_in_helper.to_string(), "__BufBorrowedWire < '__a , N >");
    }

    #[test]
    fn test_should_mark_transparent_edge_cases() {
        let plain: syn::ItemStruct = syn::parse_quote! { struct Plain(u32); };
        assert!(should_mark_transparent(&plain));

        let multi: syn::ItemStruct = syn::parse_quote! { struct Multi(u32, u32); };
        assert!(!should_mark_transparent(&multi));

        let named: syn::ItemStruct = syn::parse_quote! { struct Named { a: u32 } };
        assert!(!should_mark_transparent(&named));

        let container_serde: syn::ItemStruct = syn::parse_quote! {
            #[serde(rename_all = "camelCase")]
            struct ContainerSerde(u32);
        };
        assert!(!should_mark_transparent(&container_serde));

        let field_skip: syn::ItemStruct = syn::parse_quote! {
            struct FieldSkip(#[serde(skip)] u32);
        };
        assert!(!should_mark_transparent(&field_skip));

        let field_skip_ser: syn::ItemStruct = syn::parse_quote! {
            struct FieldSkipSer(#[serde(skip_serializing)] u32);
        };
        assert!(!should_mark_transparent(&field_skip_ser));

        let field_skip_de: syn::ItemStruct = syn::parse_quote! {
            struct FieldSkipDe(#[serde(skip_deserializing)] u32);
        };
        assert!(!should_mark_transparent(&field_skip_de));

        let field_non_skip: syn::ItemStruct = syn::parse_quote! {
            struct FieldDefault(#[serde(default)] u32);
        };
        assert!(should_mark_transparent(&field_non_skip));
    }

    #[test]
    fn test_backwards_compat_impl_error_paths() {
        // Non-struct/non-enum item
        let non_item = backwards_compat_impl(
            quote::quote!(versions(1: Self)),
            quote::quote!(
                fn foo() {}
            ),
        )
        .to_string();
        assert!(non_item.contains("compile_error !"));
        assert!(non_item.contains("can only be applied to structs or enums"));

        // Invalid attr syntax
        let bad_attr = backwards_compat_impl(
            quote::quote!(unknown_key = 123),
            quote::quote!(
                struct S {
                    a: u32,
                }
            ),
        )
        .to_string();
        assert!(bad_attr.contains("compile_error !"));

        // Invalid into_input (duplicate version tags)
        let dup_tags = backwards_compat_impl(
            quote::quote!(versions(1: V1, 1: V2)),
            quote::quote!(
                struct S {
                    a: u32,
                }
            ),
        )
        .to_string();
        assert!(dup_tags.contains("compile_error !"));

        // Invalid resolve_dag (cycle)
        let cycle = backwards_compat_impl(
            quote::quote!(versions(1: V1 => 2, 2: V2 => 1, 3: Self)),
            quote::quote!(
                struct S {
                    a: u32,
                }
            ),
        )
        .to_string();
        assert!(cycle.contains("compile_error !"));
        assert!(cycle.contains("cycle detected"));
    }

    #[test]
    fn test_backwards_compat_via_impl_paths() {
        // Valid struct with conflicting derives stripped
        let struct_ts = backwards_compat_impl(
            quote::quote!(via = Proxy, try_from),
            quote::quote! {
                #[derive(Debug, Clone, Serialize, Deserialize)]
                pub struct Domain { pub a: u32 }
            },
        );
        let struct_str = struct_ts.to_string();
        assert!(!struct_str.contains("compile_error !"));
        assert!(struct_str.contains("impl :: backwards_compat :: BackwardsCompat for Domain"));
        assert!(!struct_str.contains("derive (Debug , Clone , Serialize"));
        syn::parse2::<syn::File>(struct_ts).unwrap();

        // Valid enum with `via(...)` syntax and arbitrary option order
        let enum_ts = backwards_compat_impl(
            quote::quote!(fallible, via(ProxyEnum)),
            quote::quote! {
                #[derive(Debug, serde::Serialize, serde::Deserialize)]
                pub enum DomainEnum { A, B(u32) }
            },
        );
        let enum_str = enum_ts.to_string();
        assert!(!enum_str.contains("compile_error !"));
        assert!(enum_str.contains("impl :: backwards_compat :: BackwardsCompat for DomainEnum"));
        syn::parse2::<syn::File>(enum_ts).unwrap();

        // Non-struct/non-enum item
        let fn_err = backwards_compat_impl(
            quote::quote!(via = Proxy),
            quote::quote!(
                fn foo() {}
            ),
        )
        .to_string();
        assert!(fn_err.contains("compile_error !"));
        assert!(fn_err.contains("can only be applied to structs or enums"));

        // Invalid attr syntax (`via = Self`, bare `via`, mixing with `tag`)
        let self_err = backwards_compat_impl(
            quote::quote!(via = Self),
            quote::quote!(
                struct Domain;
            ),
        )
        .to_string();
        assert!(self_err.contains("compile_error !"));
        assert!(self_err.contains("cannot be `Self`"));

        let bare_via_err = backwards_compat_impl(
            quote::quote!(via),
            quote::quote!(
                struct Domain;
            ),
        )
        .to_string();
        assert!(bare_via_err.contains("compile_error !"));
        assert!(bare_via_err.contains("expected `via = <Type>` or `via(<Type>)`"));

        let mixed_err = backwards_compat_impl(
            quote::quote!(tag = "v", via = Proxy),
            quote::quote!(
                struct Domain;
            ),
        )
        .to_string();
        assert!(mixed_err.contains("compile_error !"));
        assert!(mixed_err.contains("`tag` cannot be combined with `via`"));

        // Field-level #[backwards_compat(...)] is rejected on struct and enum with `via`
        let struct_field_err = backwards_compat_impl(
            quote::quote!(via = Proxy),
            quote::quote! {
                pub struct Domain {
                    #[backwards_compat(versions(1: u32, 2: Self))]
                    pub a: u64,
                }
            },
        )
        .to_string();
        assert!(struct_field_err.contains("compile_error !"));
        assert!(struct_field_err.contains("field-level `#[backwards_compat(...)]` cannot be used"));

        let enum_field_err = backwards_compat_impl(
            quote::quote!(via = Proxy),
            quote::quote! {
                pub enum DomainEnum {
                    A(#[backwards_compat(versions(1: u32, 2: Self))] u64),
                }
            },
        )
        .to_string();
        assert!(enum_field_err.contains("compile_error !"));
        assert!(enum_field_err.contains("field-level `#[backwards_compat(...)]` cannot be used"));

        // Nested `#[with(via)]` inside `versions(...)` must NOT trigger `via` mode
        let nested_with_via = backwards_compat_impl(
            quote::quote!(tag = "v", versions(#[with(via)] 1: V1, 2: Self)),
            quote::quote! {
                pub struct Domain { pub a: u32 }
            },
        );
        let nested_str = nested_with_via.to_string();
        assert!(!nested_str.contains("compile_error !"));
        assert!(nested_str.contains("impl :: backwards_compat :: BackwardsCompat for Domain"));
        syn::parse2::<syn::File>(nested_with_via).unwrap();

        // Invalid item syntax
        let bad_item =
            backwards_compat_impl(quote::quote!(via = Proxy), quote::quote!(struct !!!)).to_string();
        assert!(bad_item.contains("compile_error !"));
    }

    #[test]
    fn test_field_level_mode1_bare_with_versioned_fields_struct_and_enum() {
        let struct_ts = backwards_compat_impl(
            quote::quote!(),
            quote::quote! {
                #[derive(Debug, Serialize, Deserialize)]
                #[serde(rename_all = "camelCase")]
                pub struct Config {
                    #[serde(rename = "p")]
                    #[backwards_compat(versions(1: u32, 2: Self))]
                    pub port: u16,
                    pub host: String,
                }
            },
        );
        let s = struct_ts.to_string();
        assert!(!s.contains("compile_error !"));
        assert!(!s.contains("impl :: backwards_compat :: BackwardsCompat for Config"));
        assert!(s.contains("derive (Debug , Serialize , Deserialize)"));
        assert!(s.contains("rename_all = \"camelCase\""));
        assert!(s.contains("rename = \"p\""));
        assert!(s.contains("with = \"__BcField_Config_port\""));
        assert!(!s.contains("backwards_compat (versions"));
        assert!(s.contains("struct __BcField_Config_port ;"));
        syn::parse2::<syn::File>(struct_ts).unwrap();

        let enum_ts = backwards_compat_impl(
            quote::quote!(),
            quote::quote! {
                #[derive(Serialize, Deserialize)]
                pub enum Msg {
                    Ping,
                    Data(#[backwards_compat(versions(1: u32, 2: Self))] u64),
                    Named {
                        #[backwards_compat(versions(1: Self))]
                        r#type: String,
                    },
                }
            },
        );
        let e = enum_ts.to_string();
        assert!(!e.contains("compile_error !"));
        assert!(!e.contains("impl :: backwards_compat :: BackwardsCompat for Msg"));
        assert!(e.contains("with = \"__BcField_Msg_Data_0\""));
        assert!(e.contains("with = \"__BcField_Msg_Named_type\""));
        assert!(e.contains("struct __BcField_Msg_Data_0 ;"));
        assert!(e.contains("struct __BcField_Msg_Named_type ;"));
        syn::parse2::<syn::File>(enum_ts).unwrap();
    }

    #[test]
    fn test_field_level_mode2_bare_without_versioned_fields() {
        let ts = backwards_compat_impl(
            quote::quote!(),
            quote::quote! {
                #[derive(Debug, Serialize, Deserialize)]
                pub struct Plain {
                    pub a: u32,
                }
            },
        );
        let s = ts.to_string();
        assert!(!s.contains("compile_error !"));
        assert!(s.contains("impl :: backwards_compat :: BackwardsCompat for Plain"));
        assert!(!s.contains("__BcField_"));
        syn::parse2::<syn::File>(ts).unwrap();
    }

    #[test]
    fn test_field_level_mode3_combined_outer_and_field_versioning() {
        let struct_ts = backwards_compat_impl(
            quote::quote!(tag = "v", versions(1: Self)),
            quote::quote! {
                #[derive(Debug, Serialize, Deserialize)]
                pub struct Combined {
                    #[backwards_compat(versions(1: u32, 2: Self))]
                    pub port: u16,
                    pub host: String,
                }
            },
        );
        let s = struct_ts.to_string();
        assert!(!s.contains("compile_error !"));
        assert!(s.contains("impl :: backwards_compat :: BackwardsCompat for Combined"));
        assert!(s.contains("deserialize_with = \"__BcField_Combined_port::deserialize\""));
        assert!(s.contains("serialize_with = \"__BcField_Combined_port::serialize_ref\""));
        assert!(s.contains("struct __BcField_Combined_port ;"));
        assert!(!s.contains("backwards_compat (versions"));
        syn::parse2::<syn::File>(struct_ts).unwrap();

        let enum_ts = backwards_compat_impl(
            quote::quote!(tag = "v", content = "d", versions(1: Self)),
            quote::quote! {
                pub enum CombinedEnum {
                    A(#[backwards_compat(versions(1: u32, 2: Self))] u64),
                    B {
                        #[backwards_compat(versions(1: Self))]
                        x: String,
                    },
                }
            },
        );
        let e = enum_ts.to_string();
        assert!(!e.contains("compile_error !"));
        assert!(e.contains("impl :: backwards_compat :: BackwardsCompat for CombinedEnum"));
        assert!(e.contains("deserialize_with = \"__BcField_CombinedEnum_A_0::deserialize\""));
        assert!(e.contains("serialize_with = \"__BcField_CombinedEnum_A_0::serialize_ref\""));
        assert!(e.contains("deserialize_with = \"__BcField_CombinedEnum_B_x::deserialize\""));
        assert!(e.contains("serialize_with = \"__BcField_CombinedEnum_B_x::serialize_ref\""));
        syn::parse2::<syn::File>(enum_ts).unwrap();
    }

    #[test]
    fn test_field_level_error_propagation() {
        let dup = backwards_compat_impl(
            quote::quote!(),
            quote::quote! {
                pub struct S {
                    #[backwards_compat]
                    #[backwards_compat]
                    pub a: u32,
                }
            },
        )
        .to_string();
        assert!(dup.contains("compile_error !"));
        assert!(dup.contains("duplicate"));

        let conflict = backwards_compat_impl(
            quote::quote!(),
            quote::quote! {
                pub struct S {
                    #[serde(with = "other")]
                    #[backwards_compat]
                    pub a: u32,
                }
            },
        )
        .to_string();
        assert!(conflict.contains("compile_error !"));
        assert!(conflict.contains("conflicts with"));

        let unref_gen = backwards_compat_impl(
            quote::quote!(),
            quote::quote! {
                pub struct S<T, U> {
                    #[backwards_compat(versions(1: Old<U>, 2: Self))]
                    pub a: T,
                    pub b: U,
                }
            },
        )
        .to_string();
        assert!(unref_gen.contains("compile_error !"));
        assert!(unref_gen.contains("outer generic type parameter `U`"));
    }
}
