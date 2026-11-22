use proc_macro::TokenStream;

mod codegen;
mod dag;
mod parse;

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
    let args = syn::parse_macro_input!(attr as BackwardsCompatArgs);
    let item = syn::parse_macro_input!(item as syn::Item);

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

            let target_ident = item_struct.ident.clone();
            let vis = item_struct.vis.clone();
            let generics = item_struct.generics.clone();
            (
                quote::quote!(#cleaned_item_struct),
                target_ident,
                vis,
                generics,
                Box::new(move || build_struct_shadow_wire(&item_struct)),
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

            let target_ident = item_enum.ident.clone();
            let vis = item_enum.vis.clone();
            let generics = item_enum.generics.clone();
            (
                quote::quote!(#cleaned_item_enum),
                target_ident,
                vis,
                generics,
                Box::new(move || build_enum_shadow_wire(&item_enum)),
            )
        }
        other => {
            return syn::Error::new_spanned(
                other,
                "#[backwards_compat] can only be applied to structs or enums",
            )
            .to_compile_error()
            .into();
        }
    };

    let (_, ty_generics, _) = generics.split_for_impl();
    let target_ty: syn::Type = syn::parse_quote!(#target_ident #ty_generics);

    let input = match args.into_input(target_ty, vis, generics) {
        Ok(i) => i,
        Err(err) => return err.to_compile_error().into(),
    };

    let dag_plan = match dag::resolve_dag(&input) {
        Ok(p) => p,
        Err(err) => return err.to_compile_error().into(),
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
                Err(err) => return err.to_compile_error().into(),
            };

        quote::quote! {
            #cleaned_item_tokens
            #codegen_tokens
        }
        .into()
    } else {
        let codegen_tokens = match codegen::generate_backwards_compat(&input, None, None) {
            Ok(t) => t,
            Err(err) => return err.to_compile_error().into(),
        };
        quote::quote! {
            #cleaned_item_tokens
            #codegen_tokens
        }
        .into()
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
}
