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
/// `#[backwards_compat]` automatically generates `serde::Serialize` and `serde::Deserialize`
/// directly for your target domain model, creates zero-allocation wire representations, and generates
/// transitive `From<V>` and `TryFrom<V>` conversions from historical versions.
///
/// # Macro Parameters Reference
///
/// - **`tag = "..."` / `tag("...")`** *(default: `"version"`)*:
///   The serialized field name used to store the version identifier.
/// - **`content = "..."` / `content("...")`** *(optional)*:
///   Configures Serde's adjacent tagging (`#[serde(tag = "...", content = "...")]`).
///   Essential for tuple structs, scalar types, and newtypes that cannot use internal tagging.
/// - **`version = ...` / `version(...)`** *(optional)*:
///   The current active schema version (integer literal like `2` or string literal like `"2.0"`).
///   Defaults to the highest version declared in `versions`, or `1` if empty.
/// - **`error = ErrorType` / `error(ErrorType)`** *(optional)*:
///   Custom error type returned by generated `TryFrom` implementations for fallible migrations.
///   Defaults to `Box<dyn std::error::Error + Send + Sync + 'static>`.
/// - **`dump`** *(optional)*:
///   Prints the macro's generated token stream to `stderr` during compilation for inspection.
/// - **`versions(...)` / `versions = [...]` / `versions = { ... }`**:
///   Declares the list of historical schemas and transition paths.
///
/// # Version Entry Directives
///
/// - **`tag: Type`**: Sequential chain step (e.g. `1: ModelV1 => 2: ModelV2 => 3`).
/// - **`tag: Type => target_tag`**: Custom DAG transition jumping directly to a target version.
/// - **`#[fallible]`, `#[r#try]`, `#[try_from]`, `#[try_into]`, `#[falliable]`**:
///   Marks a transition as fallible, generating `TryFrom` instead of `From`.
///
/// # Key Capabilities
///
/// 1. **Zero-Copy / Zero-Allocation Serialization**: Domain structs do not need `#[derive(Clone)]`.
///    Serialization borrows fields directly using generated shadow borrowed structs.
/// 2. **Generics & Lifetimes Support**: Fully supports generic types, lifetime parameters, and `where` clauses.
/// 3. **Implicit vs Explicit Wire Formats**: Automatically synthesizes private shadow wire structs
///    (`__<Target>ShadowWire` and `__<Target>BorrowedWire`) when the domain model is the active wire format.
/// 4. **Transitive In-Code Conversions**: Automatically implements `From<V>` / `TryFrom<V>` for all historical
///    versions `V` by composing the shortest path in the transition graph.
///
/// # Examples
///
/// ### 1. Sequential Infallible Migrations (Implicit Wire)
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
/// #[backwards_compat(tag = "version", version = 2, versions(1: ConfigV1))]
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
/// ### 2. Fallible Migrations with Custom Error
///
/// ```rust,ignore
/// #[backwards_compat(
///     tag = "ver",
///     version = 2,
///     error = MyError,
///     versions(#[try_from] 1: ServerV1)
/// )]
/// pub struct Server {
///     pub port: u16,
/// }
/// ```
///
/// ### 3. Adjacent Tagging for Tuple Structs / Scalars
///
/// ```rust,ignore
/// #[backwards_compat(
///     tag = "type",
///     content = "data",
///     version = 2,
///     versions(1: ScalarV1)
/// )]
/// pub struct ScalarTarget(pub String);
/// ```
#[proc_macro_attribute]
pub fn backwards_compat(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = syn::parse_macro_input!(attr as BackwardsCompatArgs);
    let item_struct = syn::parse_macro_input!(item as syn::ItemStruct);

    let mut cleaned_item_struct = item_struct.clone();
    cleaned_item_struct
        .attrs
        .retain(|attr| !attr.path().is_ident("serde"));
    clean_derive_attrs(&mut cleaned_item_struct.attrs);
    for field in &mut cleaned_item_struct.fields {
        field.attrs.retain(|attr| !attr.path().is_ident("serde"));
    }

    let target_ident = &item_struct.ident;
    let (_, ty_generics, _) = item_struct.generics.split_for_impl();
    let target_ty: syn::Type = syn::parse_quote!(#target_ident #ty_generics);

    let input = match args.into_input(target_ty, item_struct.vis.clone(), item_struct.generics.clone()) {
        Ok(i) => i,
        Err(err) => return err.to_compile_error().into(),
    };

    let dag_plan = match dag::resolve_dag(&input) {
        Ok(p) => p,
        Err(err) => return err.to_compile_error().into(),
    };

    if dag_plan.target_is_wire {
        let call_site_span = proc_macro2::Span::call_site();
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
            let mut borrowed_generics = item_struct.generics.clone();
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

            let (borrowed_impl_generics, _, borrowed_where_clause) = borrowed_generics.split_for_impl();

            let mut borrowed_args =
                syn::punctuated::Punctuated::<syn::GenericArgument, syn::Token![,]>::new();
            borrowed_args.push(syn::GenericArgument::Lifetime(syn::parse_quote!('__a)));
            for param in &item_struct.generics.params {
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

        let shadow_info = codegen::ShadowWireInfo {
            owned_ident: &shadow_ident,
            borrowed_ident: &borrowed_ident,
            borrowed_ty_in_helper,
            construct_borrowed,
        };

        let codegen_tokens =
            match codegen::generate_backwards_compat(&input, Some(&shadow_info), Some(shadow_tokens)) {
                Ok(t) => t,
                Err(err) => return err.to_compile_error().into(),
            };

        quote::quote! {
            #cleaned_item_struct
            #codegen_tokens
        }
        .into()
    } else {
        let codegen_tokens = match codegen::generate_backwards_compat(&input, None, None) {
            Ok(t) => t,
            Err(err) => return err.to_compile_error().into(),
        };
        quote::quote! {
            #cleaned_item_struct
            #codegen_tokens
        }
        .into()
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
}
