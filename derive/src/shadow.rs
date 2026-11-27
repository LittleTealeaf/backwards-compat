use crate::field;
use crate::is_forwarded_attr;
#[cfg(feature = "schemars")]
use crate::{field_has_schemars_with, schemars_attrs_have_crate};

pub struct BuiltShadowWire {
    pub shadow_ident: syn::Ident,
    pub borrowed_ident: syn::Ident,
    pub borrowed_ty_in_helper: proc_macro2::TokenStream,
    pub construct_borrowed: proc_macro2::TokenStream,
    pub shadow_tokens: proc_macro2::TokenStream,
    pub is_unit_struct: bool,
}

pub fn build_borrowed_generics(
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

pub fn should_mark_transparent(item_struct: &syn::ItemStruct) -> bool {
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

pub fn build_struct_shadow_wire(
    item_struct: &syn::ItemStruct,
    field_specs: &[field::FieldCompatSpec],
    derive_json_schema: bool,
) -> BuiltShadowWire {
    #[cfg(not(feature = "schemars"))]
    let _ = derive_json_schema;

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
            #[cfg(feature = "schemars")]
            if derive_json_schema && !field_has_schemars_with(&field.attrs) {
                let schema_ty_str = spec.schema_helper_ty_str();
                field
                    .attrs
                    .push(syn::parse_quote!(#[schemars(with = #schema_ty_str)]));
            }
        }
        if let Some(ref mut ident) = field.ident {
            ident.set_span(call_site_span);
        }
    }

    #[cfg(feature = "schemars")]
    let (shadow_derive_attr, shadow_schemars_crate_attr) = if derive_json_schema {
        let crate_attr = if schemars_attrs_have_crate(&preserved_attrs) {
            proc_macro2::TokenStream::new()
        } else {
            quote::quote!(#[schemars(crate = "::backwards_compat::__private::schemars")])
        };
        (
            quote::quote!(#[derive(::serde::Deserialize, ::backwards_compat::__private::schemars::JsonSchema)]),
            crate_attr,
        )
    } else {
        (
            quote::quote!(#[derive(::serde::Deserialize)]),
            proc_macro2::TokenStream::new(),
        )
    };
    #[cfg(not(feature = "schemars"))]
    let (shadow_derive_attr, shadow_schemars_crate_attr) = (
        quote::quote!(#[derive(::serde::Deserialize)]),
        proc_macro2::TokenStream::new(),
    );

    let owned_struct_def = quote::quote! {
        #[allow(non_camel_case_types, dead_code, missing_debug_implementations)]
        #[doc(hidden)]
        #shadow_derive_attr
        #shadow_schemars_crate_attr
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
                impl #impl_generics ::core::convert::From<#shadow_ident #ty_generics> for #target_ident #ty_generics #where_clause {
                    fn from(val: #shadow_ident #ty_generics) -> Self {
                        Self ( #( val.#indices ),* )
                    }
                }
            }
        }
        syn::Fields::Unit => {
            quote::quote! {
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

pub fn build_enum_shadow_wire(
    item_enum: &syn::ItemEnum,
    field_specs: &[field::FieldCompatSpec],
    derive_json_schema: bool,
) -> BuiltShadowWire {
    #[cfg(not(feature = "schemars"))]
    let _ = derive_json_schema;

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
                #[cfg(feature = "schemars")]
                if derive_json_schema && !field_has_schemars_with(&field.attrs) {
                    let schema_ty_str = spec.schema_helper_ty_str();
                    field
                        .attrs
                        .push(syn::parse_quote!(#[schemars(with = #schema_ty_str)]));
                }
            }
            if let Some(ref mut ident) = field.ident {
                ident.set_span(call_site_span);
            }
        }
    }

    #[cfg(feature = "schemars")]
    let (shadow_derive_attr, shadow_schemars_crate_attr) = if derive_json_schema {
        let crate_attr = if schemars_attrs_have_crate(&preserved_attrs) {
            proc_macro2::TokenStream::new()
        } else {
            quote::quote!(#[schemars(crate = "::backwards_compat::__private::schemars")])
        };
        (
            quote::quote!(#[derive(::serde::Deserialize, ::backwards_compat::__private::schemars::JsonSchema)]),
            crate_attr,
        )
    } else {
        (
            quote::quote!(#[derive(::serde::Deserialize)]),
            proc_macro2::TokenStream::new(),
        )
    };
    #[cfg(not(feature = "schemars"))]
    let (shadow_derive_attr, shadow_schemars_crate_attr) = (
        quote::quote!(#[derive(::serde::Deserialize)]),
        proc_macro2::TokenStream::new(),
    );

    let owned_enum_def = quote::quote! {
        #[allow(non_camel_case_types, dead_code, missing_debug_implementations)]
        #[doc(hidden)]
        #shadow_derive_attr
        #shadow_schemars_crate_attr
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
