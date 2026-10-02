use proc_macro::TokenStream;
use syn::parse_macro_input;

mod codegen;
mod dag;
mod parse;

use parse::{BackwardsCompatArgs, BackwardsCompatInput};

/// Declarative macro for backwards compatibility schema versioning.
///
/// This macro allows you to define a backwards compatible enum separately
/// from the target model, useful when you don't want to attach an attribute macro.
///
/// # Example
/// ```rust,ignore
/// backwards_compat_decl! {
///     #[tag = "version", version = 2]
///     compat Config {
///         1: ConfigV1,
///         2: ConfigV2,
///     }
/// }
/// ```
#[proc_macro]
pub fn backwards_compat_decl(input: TokenStream) -> TokenStream {
    let parsed = parse_macro_input!(input as BackwardsCompatInput);
    match codegen::generate_backwards_compat(parsed, None) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Attribute macro for seamless, zero-boilerplate backwards compatibility versioning.
///
/// Applies schema versioning to a struct directly.
///
/// # Example
/// ```rust,ignore
/// #[backwards_compat(tag = "version", version = 2, versions(1: ConfigV1))]
/// pub struct Config {
///     pub field: String,
/// }
/// ```
#[proc_macro_attribute]
pub fn backwards_compat(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = syn::parse_macro_input!(attr as BackwardsCompatArgs);
    let item_struct = syn::parse_macro_input!(item as syn::ItemStruct);

    let mut cleaned_item_struct = item_struct.clone();
    cleaned_item_struct
        .attrs
        .retain(|attr| !attr.path().is_ident("serde"));
    for field in &mut cleaned_item_struct.fields {
        field.attrs.retain(|attr| !attr.path().is_ident("serde"));
    }

    let target_ident = &item_struct.ident;
    let target_ty: syn::Type = syn::parse_quote!(#target_ident);

    let input = match args.into_input(target_ty, item_struct.vis.clone()) {
        Ok(i) => i,
        Err(err) => return err.to_compile_error().into(),
    };

    let dag_plan = match dag::resolve_dag(&input) {
        Ok(p) => p,
        Err(err) => return err.to_compile_error().into(),
    };

    if dag_plan.target_is_wire {
        let shadow_ident = quote::format_ident!("__{}ShadowWire", target_ident);

        let vis = &item_struct.vis;
        let fields = &item_struct.fields;
        let generics = &item_struct.generics;
        let semi = &item_struct.semi_token;

        let mut preserved_attrs = Vec::new();
        for attr in &item_struct.attrs {
            if attr.path().is_ident("doc") || attr.path().is_ident("serde") {
                preserved_attrs.push(attr.clone());
            }
        }

        let shadow_struct_def = quote::quote! {
            #[allow(non_camel_case_types, dead_code)]
            #[doc(hidden)]
            #[derive(::serde::Serialize, ::serde::Deserialize)]
            #(#preserved_attrs)*
            #vis struct #shadow_ident #generics #fields #semi
        };

        let from_impls = match fields {
            syn::Fields::Named(named) => {
                let field_idents: Vec<_> = named.named.iter().map(|f| f.ident.as_ref().unwrap()).collect();
                quote::quote! {
                    impl ::core::convert::From<#target_ident> for #shadow_ident {
                        fn from(val: #target_ident) -> Self {
                            Self { #( #field_idents: val.#field_idents ),* }
                        }
                    }
                    impl ::core::convert::From<#shadow_ident> for #target_ident {
                        fn from(val: #shadow_ident) -> Self {
                            Self { #( #field_idents: val.#field_idents ),* }
                        }
                    }
                }
            }
            syn::Fields::Unnamed(unnamed) => {
                let indices: Vec<_> = (0..unnamed.unnamed.len()).map(syn::Index::from).collect();
                quote::quote! {
                    impl ::core::convert::From<#target_ident> for #shadow_ident {
                        fn from(val: #target_ident) -> Self {
                            Self ( #( val.#indices ),* )
                        }
                    }
                    impl ::core::convert::From<#shadow_ident> for #target_ident {
                        fn from(val: #shadow_ident) -> Self {
                            Self ( #( val.#indices ),* )
                        }
                    }
                }
            }
            syn::Fields::Unit => {
                quote::quote! {
                    impl ::core::convert::From<#target_ident> for #shadow_ident {
                        fn from(_val: #target_ident) -> Self {
                            Self
                        }
                    }
                    impl ::core::convert::From<#shadow_ident> for #target_ident {
                        fn from(_val: #shadow_ident) -> Self {
                            Self
                        }
                    }
                }
            }
        };

        let codegen_tokens = match codegen::generate_backwards_compat(input, Some(&shadow_ident)) {
            Ok(t) => t,
            Err(err) => return err.to_compile_error().into(),
        };

        quote::quote! {
            #cleaned_item_struct
            #shadow_struct_def
            #from_impls
            #codegen_tokens
        }
        .into()
    } else {
        let codegen_tokens = match codegen::generate_backwards_compat(input, None) {
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
