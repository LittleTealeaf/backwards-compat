use proc_macro::TokenStream;

mod codegen;
mod dag;
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
/// `#[backwards_compat]` can be applied to either a `struct` or an `enum`. It automatically generates
/// `serde::Serialize`, `serde::Deserialize`, `backwards_compat::BackwardsCompat`,
/// `backwards_compat::BackwardsCompatDeserialize`, zero-allocation wire representations, and
/// transitive `From<V>` / `TryFrom<V>` conversions from historical versions.
///
/// # Macro Parameters Reference
///
/// - **`tag = "..."` / `tag("...")`** *(default: `"version"`)*:
///   The serialized field name used to store the version identifier (native integer or string).
/// - **`content = "..."` / `content("...")`** *(optional)*:
///   Configures adjacent tagging (`{"tag": ..., "content": ...}`).
///   Essential for tuple structs, scalar types, newtypes, and enums that cannot be flattened into a map.
/// - **`error = ErrorType` / `error(ErrorType)`** *(optional)*:
///   Custom error type returned by generated `TryFrom` implementations for fallible migrations.
///   Defaults to `Box<dyn std::error::Error + Send + Sync + 'static>`.
/// - **`dump`** *(optional)*:
///   Prints the macro's generated token stream to `stderr` during compilation for inspection.
/// - **`versions(...)` / `versions = [...]` / `versions = { ... }`**:
///   Declares the list of schema versions and transition paths. The active current version is the
///   entry marked `#[current]`, or defaults to the last non-untagged primary entry (or `1` if empty).
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
///   `#[with]` function).
/// - **`#[with(fn_path)]`**: Uses a custom conversion function `fn(FromTy) -> ToTy` (or
///   `fn(FromTy) -> Result<ToTy, E>` with `#[fallible]`) instead of `From` / `TryFrom`.
/// - **`#[cfg(...)]`**: Conditionally compiles a historical or unversioned (`_`) version entry,
///   including its deserialization arm and generated `From`/`TryFrom` conversion. Multiple
///   `#[cfg(...)]` attributes on an entry or along a path are combined with `all(...)`. Neither the
///   active `#[current]` version nor the `#[serialize]` version may be gated, and unconditional
///   entries cannot migrate through a gated entry.
///
/// # Key Capabilities
///
/// 1. **Struct & Enum Domain Models**: Works on named structs, tuple structs, unit structs, and enums.
/// 2. **Zero-Copy / Zero-Allocation Serialization**: Domain models do not need `#[derive(Clone)]`.
///    Serialization borrows fields directly using generated shadow borrowed structs/enums and native
///    integer or string version tags.
/// 3. **Generics & Lifetimes Support**: Fully supports generic types, lifetime parameters, and `where` clauses.
/// 4. **Metadata & Opt-In Migration Tracking**: Implements `BackwardsCompat` (`CURRENT_VERSION`,
///    `SERIALIZE_VERSION`, `SUPPORTED_VERSIONS`, `ACCEPTS_UNTAGGED`) and `Migrated<T>`.
/// 5. **Transitive In-Code Conversions**: Automatically implements `From<V>` / `TryFrom<V>` for all
///    multi-step historical versions `V`.
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
/// ### 2. Fallible Migrations with Custom Error & `#[with]`
///
/// ```rust,ignore
/// #[backwards_compat(
///     tag = "ver",
///     error = MyError,
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
/// ### 3. Adjacent Tagging for Tuple Structs, Scalars, and Enums
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
/// ### 4. Rolling Deployments (`#[serialize]`) & Inline Tag Aliases (`|`)
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
/// ### 5. Feature-Gated Historical Versions (`#[cfg(...)]`)
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

fn backwards_compat_impl(
    attr: proc_macro2::TokenStream,
    item: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let args = match syn::parse2::<BackwardsCompatArgs>(attr) {
        Ok(a) => a,
        Err(err) => return err.to_compile_error(),
    };
    let item = match syn::parse2::<syn::Item>(item) {
        Ok(i) => i,
        Err(err) => return err.to_compile_error(),
    };

    let (cleaned_item_tokens, target_ident, vis, generics, built_shadow_fn): (
        proc_macro2::TokenStream,
        syn::Ident,
        syn::Visibility,
        syn::Generics,
        Box<dyn FnOnce() -> BuiltShadowWire>,
    ) = match item {
        syn::Item::Struct(item_struct) => {
            let mut cleaned_item_struct = item_struct.clone();
            cleaned_item_struct
                .attrs
                .retain(|attr| !attr.path().is_ident("serde"));
            clean_derive_attrs(&mut cleaned_item_struct.attrs);
            for field in &mut cleaned_item_struct.fields {
                field.attrs.retain(|attr| !attr.path().is_ident("serde"));
            }

            let shadow_struct = respan_to_call_site(&item_struct);
            let target_ident = shadow_struct.ident.clone();
            let vis = shadow_struct.vis.clone();
            let generics = shadow_struct.generics.clone();
            (
                quote::quote!(#cleaned_item_struct),
                target_ident,
                vis,
                generics,
                Box::new(move || build_struct_shadow_wire(&shadow_struct)),
            )
        }
        syn::Item::Enum(item_enum) => {
            let mut cleaned_item_enum = item_enum.clone();
            cleaned_item_enum
                .attrs
                .retain(|attr| !attr.path().is_ident("serde"));
            clean_derive_attrs(&mut cleaned_item_enum.attrs);
            for variant in &mut cleaned_item_enum.variants {
                variant.attrs.retain(|attr| !attr.path().is_ident("serde"));
                for field in &mut variant.fields {
                    field.attrs.retain(|attr| !attr.path().is_ident("serde"));
                }
            }

            let shadow_enum = respan_to_call_site(&item_enum);
            let target_ident = shadow_enum.ident.clone();
            let vis = shadow_enum.vis.clone();
            let generics = shadow_enum.generics.clone();
            (
                quote::quote!(#cleaned_item_enum),
                target_ident,
                vis,
                generics,
                Box::new(move || build_enum_shadow_wire(&shadow_enum)),
            )
        }
        other => {
            return syn::Error::new_spanned(
                other,
                "#[backwards_compat] can only be applied to structs or enums",
            )
            .to_compile_error();
        }
    };

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

        let codegen_tokens =
            match codegen::generate_backwards_compat(&input, Some(&shadow_info), Some(built.shadow_tokens)) {
                Ok(t) => t,
                Err(err) => return err.to_compile_error(),
            };

        quote::quote! {
            #cleaned_item_tokens
            #codegen_tokens
        }
    } else {
        let codegen_tokens = match codegen::generate_backwards_compat(&input, None, None) {
            Ok(t) => t,
            Err(err) => return err.to_compile_error(),
        };
        quote::quote! {
            #cleaned_item_tokens
            #codegen_tokens
        }
    }
}

/// Redirects `serde::Serialize`, `serde::Deserialize`, `backwards_compat::BackwardsCompat`, and
/// `backwards_compat::BackwardsCompatDeserialize` for a domain model through a separate versioned
/// proxy type.
///
/// Use `#[backwards_compat_via(ProxyType, ...)]` when you want to keep your domain `struct` or
/// `enum` completely decoupled from its serialized wire schema (for example, enforcing domain
/// invariants, using non-serializable internal representations, or keeping versioned DTOs in a
/// separate module) while still automatically inheriting all `backwards_compat` capabilities
/// (version tags, schema migrations, `Migrated<T>` tracking, `#[cfg(...)]`-gated versions, and
/// rolling deployments) from `ProxyType`.
///
/// # Macro Parameters Reference
///
/// - **`ProxyType`** *(required, first positional argument)*:
///   The versioned proxy type (typically annotated with `#[backwards_compat(...)]`) that handles
///   serialization, deserialization, and schema metadata. Cannot be bare `Self`.
/// - **`try_from`** *(optional flag)*:
///   Makes the deserialization conversion (`ProxyType -> Self`) fallible. Uses
///   `TryFrom<ProxyType>` (or a `Result`-returning `from` function) and converts errors via
///   `serde::de::Error::custom`. Cannot be combined with `fallible`.
/// - **`try_into`** *(optional flag)*:
///   Makes the serialization conversion (`&Self -> ProxyType` / `Self -> ProxyType`) fallible.
///   Uses `TryInto<ProxyType>` (or a `Result`-returning `to` function) and converts errors via
///   `serde::ser::Error::custom`. Cannot be combined with `fallible`.
/// - **`fallible`** *(optional flag)*:
///   Shorthand for enabling both `try_from` and `try_into`. Cannot be combined with `try_from` or
///   `try_into`.
/// - **`from = fn_path` / `from(fn_path)`** *(optional)*:
///   Uses a custom conversion function instead of `From` / `TryFrom` during deserialization:
///   - Without `try_from` / `fallible`: `fn(ProxyType) -> Self`
///   - With `try_from` / `fallible`: `fn(ProxyType) -> Result<Self, E>` where `E: Display`
/// - **`to = fn_path` / `to(fn_path)`** *(optional)*:
///   Uses a custom conversion function instead of `Into` / `TryInto` during serialization:
///   - Without `try_into` / `fallible`: `fn(&Self) -> ProxyType`
///   - With `try_into` / `fallible`: `fn(&Self) -> Result<ProxyType, E>` where `E: Display`
/// - **`dump`** *(optional flag)*:
///   Prints the macro's generated token stream to `stderr` during compilation for inspection.
///
/// # Conversion & Autoref Rules
///
/// During serialization (when `to` is not specified), `#[backwards_compat_via]` uses autoref
/// specialization so domain types do **not** need to implement `Clone` when a borrowed conversion
/// is available:
///
/// - **Infallible (`try_into` not set)**: prefers `for<'a> &'a Self: Into<ProxyType>` (zero-clone),
///   falling back to `Self: Clone + Into<ProxyType>`.
/// - **Fallible (`try_into` or `fallible` set)**: prefers `for<'a> &'a Self: TryInto<ProxyType>`
///   (zero-clone) with `Error: Display`, falling back to `Self: Clone + TryInto<ProxyType>` with
///   `Error: Display`.
///
/// Any fallible error type must implement `core::fmt::Display`; errors are formatted and reported
/// through `serde::de::Error::custom` or `serde::ser::Error::custom`.
///
/// # Generated Trait Implementations
///
/// Applying `#[backwards_compat_via(ProxyType, ...)]` to a `struct` or `enum` generates:
///
/// 1. `backwards_compat::BackwardsCompat` — delegates `Version`, `CURRENT_VERSION`,
///    `SERIALIZE_VERSION`, `SUPPORTED_VERSIONS`, and `ACCEPTS_UNTAGGED` directly to `ProxyType`.
/// 2. `backwards_compat::BackwardsCompatDeserialize<'de>` — deserializes `Migrated<ProxyType>`,
///    converts the inner value into `Self`, and preserves `source_version` so `Migrated<Self>`
///    works seamlessly.
/// 3. `serde::Deserialize<'de>` — delegates to `BackwardsCompatDeserialize::deserialize_migrated`.
/// 4. `serde::Serialize` — converts `&self` into `ProxyType` and serializes it.
///
/// Any `Serialize` or `Deserialize` entries in `#[derive(...)]` on the annotated item are
/// automatically stripped to prevent conflicting trait implementations. `#[serde(...)]` attributes
/// on the domain type are intentionally **not** stripped so misplaced serde attributes are caught
/// at compile time.
///
/// # Examples
///
/// ### 1. Basic Infallible Domain Model with Zero-Clone Serialization
///
/// ```rust,ignore
/// use backwards_compat::{backwards_compat, backwards_compat_via};
/// use serde::Deserialize;
///
/// #[derive(Deserialize)]
/// pub struct UserWireV1 {
///     pub name: String,
/// }
///
/// #[backwards_compat(tag = "version", versions(1: UserWireV1, 2: Self))]
/// pub struct UserWire {
///     pub name: String,
///     pub role: String,
/// }
///
/// impl From<UserWireV1> for UserWire {
///     fn from(v1: UserWireV1) -> Self {
///         Self { name: v1.name, role: "member".into() }
///     }
/// }
///
/// // Domain type does not need #[derive(Clone, Serialize, Deserialize)].
/// #[backwards_compat_via(UserWire)]
/// pub struct User {
///     pub name: String,
///     pub role: String,
/// }
///
/// impl From<UserWire> for User {
///     fn from(w: UserWire) -> Self {
///         Self { name: w.name, role: w.role }
///     }
/// }
///
/// impl From<&User> for UserWire {
///     fn from(u: &User) -> Self {
///         Self { name: u.name.clone(), role: u.role.clone() }
///     }
/// }
/// ```
///
/// ### 2. Fallible Validation (`try_from`, `try_into`, `fallible`)
///
/// ```rust,ignore
/// use backwards_compat::{backwards_compat, backwards_compat_via};
///
/// #[backwards_compat(tag = "version", versions(1: Self))]
/// pub struct PortWire {
///     pub port: u32,
/// }
///
/// #[backwards_compat_via(PortWire, fallible)]
/// pub struct ValidPort(u16);
///
/// impl TryFrom<PortWire> for ValidPort {
///     type Error = &'static str;
///     fn try_from(w: PortWire) -> Result<Self, Self::Error> {
///         let port = u16::try_from(w.port).map_err(|_| "port out of range")?;
///         if port == 0 {
///             return Err("port cannot be zero");
///         }
///         Ok(Self(port))
///     }
/// }
///
/// impl TryFrom<&ValidPort> for PortWire {
///     type Error = &'static str;
///     fn try_from(p: &ValidPort) -> Result<Self, Self::Error> {
///         Ok(Self { port: u32::from(p.0) })
///     }
/// }
/// ```
///
/// ### 3. Custom Conversion Functions (`from` & `to`)
///
/// ```rust,ignore
/// use backwards_compat::backwards_compat_via;
///
/// fn decode_cfg(w: ConfigWire) -> Result<Config, String> {
///     if w.host.is_empty() {
///         return Err("host must not be empty".into());
///     }
///     Ok(Config { host: w.host })
/// }
///
/// fn encode_cfg(c: &Config) -> ConfigWire {
///     ConfigWire { host: c.host.clone() }
/// }
///
/// #[backwards_compat_via(ConfigWire, try_from, from = decode_cfg, to = encode_cfg)]
/// pub struct Config {
///     pub host: String,
/// }
/// ```
///
/// ### 4. Generics and Lifetimes
///
/// ```rust,ignore
/// use backwards_compat::{backwards_compat, backwards_compat_via};
///
/// #[backwards_compat(tag = "version", versions(1: Self))]
/// pub struct EnvelopeWire<'a, T> {
///     pub label: &'a str,
///     pub payload: T,
/// }
///
/// #[backwards_compat_via(EnvelopeWire<'a, T>)]
/// pub struct Envelope<'a, T> {
///     pub label: &'a str,
///     pub payload: T,
/// }
/// ```
#[proc_macro_attribute]
pub fn backwards_compat_via(attr: TokenStream, item: TokenStream) -> TokenStream {
    backwards_compat_via_impl(attr.into(), item.into()).into()
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
            clean_derive_attrs(&mut item_struct.attrs);
            let codegen_struct = respan_to_call_site(&item_struct);
            let codegen_tokens = via::generate_via(&args, &codegen_struct.ident, &codegen_struct.generics);
            quote::quote! {
                #item_struct
                #codegen_tokens
            }
        }
        syn::Item::Enum(mut item_enum) => {
            clean_derive_attrs(&mut item_enum.attrs);
            let codegen_enum = respan_to_call_site(&item_enum);
            let codegen_tokens = via::generate_via(&args, &codegen_enum.ident, &codegen_enum.generics);
            quote::quote! {
                #item_enum
                #codegen_tokens
            }
        }
        other => syn::Error::new_spanned(
            other,
            "#[backwards_compat_via] can only be applied to structs or enums",
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

fn build_struct_shadow_wire(item_struct: &syn::ItemStruct) -> BuiltShadowWire {
    let call_site_span = proc_macro2::Span::call_site();
    let target_ident = &item_struct.ident;
    let shadow_ident = quote::format_ident!("__{}ShadowWire", target_ident, span = call_site_span);
    let borrowed_ident = quote::format_ident!("__{}BorrowedWire", target_ident, span = call_site_span);

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
    for field in &mut shadow_fields {
        field.attrs.retain(is_forwarded_attr);
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
                for f in &named.named {
                    let field_attrs: Vec<_> =
                        f.attrs.iter().filter(|a| is_forwarded_attr(a)).cloned().collect();
                    let field_ident = f.ident.as_ref().unwrap();
                    let mut borrowed_field_ident = field_ident.clone();
                    borrowed_field_ident.set_span(call_site_span);
                    let field_ty = &f.ty;
                    borrowed_named.push(quote::quote! {
                        #(#field_attrs)*
                        #borrowed_field_ident: &'__wire #field_ty
                    });
                    field_idents.push(borrowed_field_ident);
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
                    let field_attrs: Vec<_> =
                        f.attrs.iter().filter(|a| is_forwarded_attr(a)).cloned().collect();
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

fn build_enum_shadow_wire(item_enum: &syn::ItemEnum) -> BuiltShadowWire {
    let call_site_span = proc_macro2::Span::call_site();
    let target_ident = &item_enum.ident;
    let shadow_ident = quote::format_ident!("__{}ShadowWire", target_ident, span = call_site_span);
    let borrowed_ident = quote::format_ident!("__{}BorrowedWire", target_ident, span = call_site_span);

    let generics = &item_enum.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let preserved_attrs: Vec<_> = item_enum
        .attrs
        .iter()
        .filter(|attr| is_forwarded_attr(attr))
        .cloned()
        .collect();

    let mut owned_variants = item_enum.variants.clone();
    for variant in &mut owned_variants {
        variant.attrs.retain(is_forwarded_attr);
        for field in &mut variant.fields {
            field.attrs.retain(is_forwarded_attr);
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

    for variant in &item_enum.variants {
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
                    let f_attrs: Vec<_> = f.attrs.iter().filter(|a| is_forwarded_attr(a)).cloned().collect();
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
                for f in &named.named {
                    let f_attrs: Vec<_> = f.attrs.iter().filter(|a| is_forwarded_attr(a)).cloned().collect();
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
    fn test_respan_to_call_site_preserves_syntax() {
        let item_struct: syn::ItemStruct = syn::parse_quote! {
            #[serde(rename_all = "camelCase")]
            pub struct Record<'a, T: Clone>
            where
                T: core::fmt::Debug,
            {
                pub label: &'a str,
                pub value: T,
            }
        };
        let respanned = respan_to_call_site(&item_struct);
        assert_eq!(
            quote::quote!(#respanned).to_string(),
            quote::quote!(#item_struct).to_string()
        );

        let item_enum: syn::ItemEnum = syn::parse_quote! {
            pub enum Event<T> {
                Ping,
                Tuple(u32, T),
                Named { text: String },
            }
        };
        let respanned_enum = respan_to_call_site(&item_enum);
        assert_eq!(
            quote::quote!(#respanned_enum).to_string(),
            quote::quote!(#item_enum).to_string()
        );
    }

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

        let built = build_enum_shadow_wire(&item_enum);
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

        let built = build_enum_shadow_wire(&item_enum);
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

        let built = build_enum_shadow_wire(&item_enum);
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
        let struct_ts = backwards_compat_via_impl(
            quote::quote!(Proxy, try_from),
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

        // Valid enum
        let enum_ts = backwards_compat_via_impl(
            quote::quote!(ProxyEnum, fallible),
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
        let fn_err = backwards_compat_via_impl(
            quote::quote!(Proxy),
            quote::quote!(
                fn foo() {}
            ),
        )
        .to_string();
        assert!(fn_err.contains("compile_error !"));
        assert!(fn_err.contains("can only be applied to structs or enums"));

        // Invalid attr syntax (`Self` and empty)
        let self_err = backwards_compat_via_impl(
            quote::quote!(Self),
            quote::quote!(
                struct Domain;
            ),
        )
        .to_string();
        assert!(self_err.contains("compile_error !"));

        let empty_err = backwards_compat_via_impl(
            quote::quote!(),
            quote::quote!(
                struct Domain;
            ),
        )
        .to_string();
        assert!(empty_err.contains("compile_error !"));

        // Invalid item syntax
        let bad_item = backwards_compat_via_impl(quote::quote!(Proxy), quote::quote!(struct !!!)).to_string();
        assert!(bad_item.contains("compile_error !"));
    }
}
