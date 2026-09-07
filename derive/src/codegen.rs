use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{Ident, Type, parse_quote};

use crate::parse::{BackwardsCompatInput, VersionEntry};

pub fn generate_backwards_compat(input: BackwardsCompatInput) -> syn::Result<TokenStream> {
    let _vis = &input.vis;
    let enum_ident = &input.enum_ident;
    let versions = &input.versions;
    let n = versions.len();

    let latest_idx = versions.iter().position(|v| v.latest).unwrap_or(n - 1);
    let latest_entry = &versions[latest_idx];
    let latest_ident = &latest_entry.ident;
    let latest_ty = &latest_entry.ty;

    let target_ty: Type = match &input.target {
        Some(t) => t.clone(),
        None => latest_ty.clone(),
    };

    let is_custom_target = input.target.is_some();
    let is_fallible = versions.iter().any(|v| v.fallible) || input.error.is_some();

    let error_ty: Type = match &input.error {
        Some(e) => e.clone(),
        None => {
            if is_fallible {
                parse_quote!(
                    ::std::boxed::Box<
                        dyn ::std::error::Error + ::core::marker::Send + ::core::marker::Sync,
                    >
                )
            } else {
                parse_quote!(::core::convert::Infallible)
            }
        }
    };

    let has_any_untagged = input.untagged_enum_level || versions.iter().any(|v| v.untagged);
    let has_any_tagged = !input.untagged_enum_level && versions.iter().any(|v| !v.untagged_only);

    let mut outer_derives = Vec::new();
    let mut other_attrs = Vec::new();
    for attr in &input.outer_attrs {
        if attr.path().is_ident("derive") {
            outer_derives.push(attr.clone());
        } else {
            other_attrs.push(attr.clone());
        }
    }
    if outer_derives.is_empty() {
        outer_derives.push(parse_quote!(#[derive(Debug, Clone)]));
    }

    // Generate the upgrade expressions for each version
    // For version i: step i -> i+1 -> ... -> latest -> target
    let mut upgrade_chains = Vec::new();
    for i in 0..n {
        let chain =
            generate_chain_for_index(i, versions, &target_ty, is_custom_target, is_fallible);
        upgrade_chains.push(chain);
    }

    let code = if let Some(tagged_ident) = &input.tagged_ident {
        generate_split_enums(
            &input,
            tagged_ident,
            &target_ty,
            &error_ty,
            is_fallible,
            &outer_derives,
            &other_attrs,
            &upgrade_chains,
            latest_ident,
            latest_ty,
        )?
    } else if has_any_untagged && has_any_tagged {
        generate_hybrid_single_enum(
            &input,
            &target_ty,
            &error_ty,
            is_fallible,
            &outer_derives,
            &other_attrs,
            &upgrade_chains,
            latest_ident,
            latest_ty,
        )?
    } else if has_any_untagged {
        generate_pure_untagged_enum(
            &input,
            &target_ty,
            &error_ty,
            is_fallible,
            &outer_derives,
            &other_attrs,
            &upgrade_chains,
            latest_ident,
            latest_ty,
        )?
    } else {
        generate_pure_tagged_enum(
            &input,
            &target_ty,
            &error_ty,
            is_fallible,
            &outer_derives,
            &other_attrs,
            &upgrade_chains,
            latest_ident,
            latest_ty,
        )?
    };

    if input.dump {
        eprintln!(
            "=== backwards_compat output for {} ===\n{}\n======================================",
            enum_ident, code
        );
    }

    Ok(code)
}

fn generate_chain_for_index(
    start_idx: usize,
    versions: &[VersionEntry],
    target_ty: &Type,
    is_custom_target: bool,
    is_fallible: bool,
) -> TokenStream {
    let n = versions.len();
    let mut steps = Vec::new();

    steps.push(quote! {
        let cur = val;
    });

    for j in start_idx..(n - 1) {
        let next_ty = &versions[j + 1].ty;
        if versions[j].fallible {
            steps.push(quote! {
                let cur: #next_ty = ::core::convert::TryInto::try_into(cur).map_err(::core::convert::Into::into)?;
            });
        } else {
            steps.push(quote! {
                let cur: #next_ty = ::core::convert::Into::into(cur);
            });
        }
    }

    let latest_entry = &versions[n - 1];
    if is_custom_target {
        if latest_entry.fallible {
            steps.push(quote! {
                let target: #target_ty = ::core::convert::TryInto::try_into(cur).map_err(::core::convert::Into::into)?;
            });
        } else {
            steps.push(quote! {
                let target: #target_ty = ::core::convert::Into::into(cur);
            });
        }
        if is_fallible {
            steps.push(quote! { ::core::result::Result::Ok(target) });
        } else {
            steps.push(quote! { target });
        }
    } else {
        if is_fallible {
            steps.push(quote! { ::core::result::Result::Ok(cur) });
        } else {
            steps.push(quote! { cur });
        }
    }

    quote! {
        {
            #(#steps)*
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn generate_split_enums(
    input: &BackwardsCompatInput,
    tagged_ident: &Ident,
    target_ty: &Type,
    error_ty: &Type,
    is_fallible: bool,
    outer_derives: &[syn::Attribute],
    other_attrs: &[syn::Attribute],
    upgrade_chains: &[TokenStream],
    latest_ident: &Ident,
    latest_ty: &Type,
) -> syn::Result<TokenStream> {
    let vis = &input.vis;
    let untagged_ident = input.untagged_ident.as_ref().unwrap_or(&input.enum_ident);
    let tag_field = input.tag.as_deref().unwrap_or("version");

    // Filter variants for tagged enum (!untagged_only)
    let tagged_versions: Vec<(usize, &VersionEntry)> = input
        .versions
        .iter()
        .enumerate()
        .filter(|(_, v)| !v.untagged_only)
        .collect();

    let mut tagged_variants = Vec::new();
    let mut tagged_match_arms = Vec::new();

    for (orig_idx, v) in tagged_versions.iter().rev() {
        let v_ident = &v.ident;
        let v_ty = &v.ty;
        let tag_str = &v.tag;
        let v_attrs = &v.outer_attrs;
        tagged_variants.push(quote! {
            #(#v_attrs)*
            #[serde(rename = #tag_str)]
            #v_ident(#v_ty)
        });

        let chain = &upgrade_chains[*orig_idx];
        tagged_match_arms.push(quote! {
            #tagged_ident::#v_ident(val) => #chain,
        });
    }

    // Filter variants for untagged enum (marked untagged)
    // Order: newest to oldest
    let untagged_versions: Vec<(usize, &VersionEntry)> = input
        .versions
        .iter()
        .enumerate()
        .filter(|(_, v)| v.untagged)
        .collect();

    let mut untagged_variants = Vec::new();
    let mut untagged_match_arms = Vec::new();
    let mut serialize_arms = Vec::new();

    // First variant of untagged enum is Tagged(tagged_ident)
    untagged_variants.push(quote! {
        Tagged(#tagged_ident)
    });
    serialize_arms.push(quote! {
        #untagged_ident::Tagged(tagged) => ::serde::Serialize::serialize(tagged, serializer),
    });

    if is_fallible {
        untagged_match_arms.push(quote! {
            #untagged_ident::Tagged(tagged) => ::core::convert::TryInto::try_into(tagged),
        });
    } else {
        untagged_match_arms.push(quote! {
            #untagged_ident::Tagged(tagged) => ::core::convert::Into::into(tagged),
        });
    }

    for (orig_idx, v) in untagged_versions.iter().rev() {
        let v_ident = &v.ident;
        let v_ty = &v.ty;
        let v_attrs = &v.outer_attrs;
        untagged_variants.push(quote! {
            #(#v_attrs)*
            #v_ident(#v_ty)
        });

        let chain = &upgrade_chains[*orig_idx];
        untagged_match_arms.push(quote! {
            #untagged_ident::#v_ident(val) => #chain,
        });

        serialize_arms.push(quote! {
            #untagged_ident::#v_ident(val) => ::serde::Serialize::serialize(val, serializer),
        });
    }

    let tagged_serde_attr = match &input.content {
        Some(content) => quote! { #[serde(tag = #tag_field, content = #content)] },
        None => quote! { #[serde(tag = #tag_field)] },
    };

    let upgrade_impls = if is_fallible {
        quote! {
            impl ::core::convert::TryFrom<#tagged_ident> for #target_ty {
                type Error = #error_ty;

                fn try_from(value: #tagged_ident) -> ::core::result::Result<Self, Self::Error> {
                    match value {
                        #(#tagged_match_arms)*
                    }
                }
            }

            impl ::core::convert::TryFrom<#untagged_ident> for #target_ty {
                type Error = #error_ty;

                fn try_from(value: #untagged_ident) -> ::core::result::Result<Self, Self::Error> {
                    match value {
                        #(#untagged_match_arms)*
                    }
                }
            }

            impl #tagged_ident {
                pub fn upgrade(self) -> ::core::result::Result<#target_ty, #error_ty> {
                    ::core::convert::TryInto::try_into(self)
                }
            }

            impl #untagged_ident {
                pub fn upgrade(self) -> ::core::result::Result<#target_ty, #error_ty> {
                    ::core::convert::TryInto::try_into(self)
                }
            }
        }
    } else {
        quote! {
            impl ::core::convert::From<#tagged_ident> for #target_ty {
                fn from(value: #tagged_ident) -> Self {
                    match value {
                        #(#tagged_match_arms)*
                    }
                }
            }

            impl ::core::convert::From<#untagged_ident> for #target_ty {
                fn from(value: #untagged_ident) -> Self {
                    match value {
                        #(#untagged_match_arms)*
                    }
                }
            }

            impl #tagged_ident {
                pub fn upgrade(self) -> #target_ty {
                    ::core::convert::Into::into(self)
                }
            }

            impl #untagged_ident {
                pub fn upgrade(self) -> #target_ty {
                    ::core::convert::Into::into(self)
                }
            }
        }
    };

    Ok(quote! {
        #(#outer_derives)*
        #(#other_attrs)*
        #[derive(::serde::Serialize, ::serde::Deserialize)]
        #tagged_serde_attr
        #[allow(non_camel_case_types)]
        #vis enum #tagged_ident {
            #(#tagged_variants,)*
        }

        impl ::core::convert::From<#target_ty> for #tagged_ident {
            fn from(value: #target_ty) -> Self {
                let latest: #latest_ty = ::core::convert::Into::into(value);
                Self::#latest_ident(latest)
            }
        }

        #(#outer_derives)*
        #(#other_attrs)*
        #[derive(::serde::Deserialize)]
        #[serde(untagged)]
        #[allow(non_camel_case_types)]
        #vis enum #untagged_ident {
            #(#untagged_variants,)*
        }

        impl ::core::convert::From<#target_ty> for #untagged_ident {
            fn from(value: #target_ty) -> Self {
                Self::Tagged(::core::convert::Into::into(value))
            }
        }

        #upgrade_impls

        impl ::serde::Serialize for #untagged_ident {
            fn serialize<S>(&self, serializer: S) -> ::core::result::Result<S::Ok, S::Error>
            where
                S: ::serde::Serializer,
            {
                match self {
                    #(#serialize_arms)*
                }
            }
        }
    })
}

#[allow(clippy::too_many_arguments)]
fn generate_hybrid_single_enum(
    input: &BackwardsCompatInput,
    target_ty: &Type,
    error_ty: &Type,
    is_fallible: bool,
    outer_derives: &[syn::Attribute],
    other_attrs: &[syn::Attribute],
    upgrade_chains: &[TokenStream],
    latest_ident: &Ident,
    latest_ty: &Type,
) -> syn::Result<TokenStream> {
    let vis = &input.vis;
    let enum_ident = &input.enum_ident;
    let tag_field = input.tag.as_deref().unwrap_or("version");

    let tagged_ref_helper_ident = format_ident!("__{}TaggedRefHelper", enum_ident);
    let tagged_owned_helper_ident = format_ident!("__{}TaggedOwnedHelper", enum_ident);
    let untagged_helper_ident = format_ident!("__{}UntaggedHelper", enum_ident);

    let mut main_variants = Vec::new();
    let mut main_upgrade_arms = Vec::new();

    for (i, v) in input.versions.iter().enumerate() {
        let v_ident = &v.ident;
        let v_ty = &v.ty;
        let v_attrs = &v.outer_attrs;
        main_variants.push(quote! {
            #(#v_attrs)*
            #v_ident(#v_ty)
        });

        let chain = &upgrade_chains[i];
        main_upgrade_arms.push(quote! {
            #enum_ident::#v_ident(val) => #chain,
        });
    }

    // Tagged variants (!untagged_only)
    let mut tagged_ref_variants = Vec::new();
    let mut tagged_owned_variants = Vec::new();
    let mut from_tagged_owned_arms = Vec::new();

    for v in input.versions.iter().filter(|v| !v.untagged_only) {
        let v_ident = &v.ident;
        let v_ty = &v.ty;
        let tag_str = &v.tag;

        tagged_ref_variants.push(quote! {
            #[serde(rename = #tag_str)]
            #v_ident(&'a #v_ty)
        });

        tagged_owned_variants.push(quote! {
            #[serde(rename = #tag_str)]
            #v_ident(#v_ty)
        });

        from_tagged_owned_arms.push(quote! {
            #tagged_owned_helper_ident::#v_ident(val) => #enum_ident::#v_ident(val),
        });
    }

    // Untagged helper variants (newest to oldest)
    let mut untagged_helper_variants = Vec::new();
    let mut from_untagged_helper_arms = Vec::new();

    untagged_helper_variants.push(quote! {
        Tagged(#tagged_owned_helper_ident)
    });
    from_untagged_helper_arms.push(quote! {
        #untagged_helper_ident::Tagged(tagged) => match tagged {
            #(#from_tagged_owned_arms)*
        },
    });

    for v in input.versions.iter().rev().filter(|v| v.untagged) {
        let v_ident = &v.ident;
        let v_ty = &v.ty;

        untagged_helper_variants.push(quote! {
            #v_ident(#v_ty)
        });

        from_untagged_helper_arms.push(quote! {
            #untagged_helper_ident::#v_ident(val) => #enum_ident::#v_ident(val),
        });
    }

    let mut serialize_arms = Vec::new();
    for v in &input.versions {
        let v_ident = &v.ident;
        if !v.untagged_only {
            serialize_arms.push(quote! {
                #enum_ident::#v_ident(val) => {
                    let helper = #tagged_ref_helper_ident::#v_ident(val);
                    ::serde::Serialize::serialize(&helper, serializer)
                }
            });
        } else {
            serialize_arms.push(quote! {
                #enum_ident::#v_ident(val) => {
                    ::serde::Serialize::serialize(val, serializer)
                }
            });
        }
    }

    let tagged_serde_attr = match &input.content {
        Some(content) => quote! { #[serde(tag = #tag_field, content = #content)] },
        None => quote! { #[serde(tag = #tag_field)] },
    };

    let upgrade_impl = if is_fallible {
        quote! {
            impl ::core::convert::TryFrom<#enum_ident> for #target_ty {
                type Error = #error_ty;

                fn try_from(value: #enum_ident) -> ::core::result::Result<Self, Self::Error> {
                    match value {
                        #(#main_upgrade_arms)*
                    }
                }
            }

            impl #enum_ident {
                pub fn upgrade(self) -> ::core::result::Result<#target_ty, #error_ty> {
                    ::core::convert::TryInto::try_into(self)
                }
            }
        }
    } else {
        quote! {
            impl ::core::convert::From<#enum_ident> for #target_ty {
                fn from(value: #enum_ident) -> Self {
                    match value {
                        #(#main_upgrade_arms)*
                    }
                }
            }

            impl #enum_ident {
                pub fn upgrade(self) -> #target_ty {
                    ::core::convert::Into::into(self)
                }
            }
        }
    };

    Ok(quote! {
        #(#outer_derives)*
        #(#other_attrs)*
        #[allow(non_camel_case_types)]
        #vis enum #enum_ident {
            #(#main_variants,)*
        }

        #[derive(::serde::Serialize)]
        #tagged_serde_attr
        #[allow(non_camel_case_types)]
        enum #tagged_ref_helper_ident<'a> {
            #(#tagged_ref_variants,)*
        }

        #[derive(::serde::Deserialize)]
        #tagged_serde_attr
        #[allow(non_camel_case_types)]
        enum #tagged_owned_helper_ident {
            #(#tagged_owned_variants,)*
        }

        #[derive(::serde::Deserialize)]
        #[serde(untagged)]
        #[allow(non_camel_case_types)]
        enum #untagged_helper_ident {
            #(#untagged_helper_variants,)*
        }

        impl ::core::convert::From<#target_ty> for #enum_ident {
            fn from(value: #target_ty) -> Self {
                let latest: #latest_ty = ::core::convert::Into::into(value);
                Self::#latest_ident(latest)
            }
        }

        #upgrade_impl

        impl ::serde::Serialize for #enum_ident {
            fn serialize<S>(&self, serializer: S) -> ::core::result::Result<S::Ok, S::Error>
            where
                S: ::serde::Serializer,
            {
                match self {
                    #(#serialize_arms)*
                }
            }
        }

        impl<'de> ::serde::Deserialize<'de> for #enum_ident {
            fn deserialize<D>(deserializer: D) -> ::core::result::Result<Self, D::Error>
            where
                D: ::serde::Deserializer<'de>,
            {
                let helper = <#untagged_helper_ident as ::serde::Deserialize>::deserialize(deserializer)?;
                let res = match helper {
                    #(#from_untagged_helper_arms)*
                };
                ::core::result::Result::Ok(res)
            }
        }
    })
}

#[allow(clippy::too_many_arguments)]
fn generate_pure_tagged_enum(
    input: &BackwardsCompatInput,
    target_ty: &Type,
    error_ty: &Type,
    is_fallible: bool,
    outer_derives: &[syn::Attribute],
    other_attrs: &[syn::Attribute],
    upgrade_chains: &[TokenStream],
    latest_ident: &Ident,
    latest_ty: &Type,
) -> syn::Result<TokenStream> {
    let vis = &input.vis;
    let enum_ident = &input.enum_ident;
    let tag_field = input.tag.as_deref().unwrap_or("version");

    let mut variants = Vec::new();
    let mut upgrade_arms = Vec::new();

    for (i, v) in input.versions.iter().enumerate() {
        let v_ident = &v.ident;
        let v_ty = &v.ty;
        let tag_str = &v.tag;
        let v_attrs = &v.outer_attrs;
        variants.push(quote! {
            #(#v_attrs)*
            #[serde(rename = #tag_str)]
            #v_ident(#v_ty)
        });

        let chain = &upgrade_chains[i];
        upgrade_arms.push(quote! {
            #enum_ident::#v_ident(val) => #chain,
        });
    }

    let tagged_serde_attr = match &input.content {
        Some(content) => quote! { #[serde(tag = #tag_field, content = #content)] },
        None => quote! { #[serde(tag = #tag_field)] },
    };

    let upgrade_impl = if is_fallible {
        quote! {
            impl ::core::convert::TryFrom<#enum_ident> for #target_ty {
                type Error = #error_ty;

                fn try_from(value: #enum_ident) -> ::core::result::Result<Self, Self::Error> {
                    match value {
                        #(#upgrade_arms)*
                    }
                }
            }

            impl #enum_ident {
                pub fn upgrade(self) -> ::core::result::Result<#target_ty, #error_ty> {
                    ::core::convert::TryInto::try_into(self)
                }
            }
        }
    } else {
        quote! {
            impl ::core::convert::From<#enum_ident> for #target_ty {
                fn from(value: #enum_ident) -> Self {
                    match value {
                        #(#upgrade_arms)*
                    }
                }
            }

            impl #enum_ident {
                pub fn upgrade(self) -> #target_ty {
                    ::core::convert::Into::into(self)
                }
            }
        }
    };

    Ok(quote! {
        #(#outer_derives)*
        #(#other_attrs)*
        #[derive(::serde::Serialize, ::serde::Deserialize)]
        #tagged_serde_attr
        #[allow(non_camel_case_types)]
        #vis enum #enum_ident {
            #(#variants,)*
        }

        impl ::core::convert::From<#target_ty> for #enum_ident {
            fn from(value: #target_ty) -> Self {
                let latest: #latest_ty = ::core::convert::Into::into(value);
                Self::#latest_ident(latest)
            }
        }

        #upgrade_impl
    })
}

#[allow(clippy::too_many_arguments)]
fn generate_pure_untagged_enum(
    input: &BackwardsCompatInput,
    target_ty: &Type,
    error_ty: &Type,
    is_fallible: bool,
    outer_derives: &[syn::Attribute],
    other_attrs: &[syn::Attribute],
    upgrade_chains: &[TokenStream],
    latest_ident: &Ident,
    latest_ty: &Type,
) -> syn::Result<TokenStream> {
    let vis = &input.vis;
    let enum_ident = &input.enum_ident;

    let mut variants = Vec::new();
    let mut upgrade_arms = Vec::new();

    // Order variants reverse (newest first) for serde untagged deserialization
    for (i, v) in input.versions.iter().enumerate().rev() {
        let v_ident = &v.ident;
        let v_ty = &v.ty;
        let v_attrs = &v.outer_attrs;
        variants.push(quote! {
            #(#v_attrs)*
            #v_ident(#v_ty)
        });

        let chain = &upgrade_chains[i];
        upgrade_arms.push(quote! {
            #enum_ident::#v_ident(val) => #chain,
        });
    }

    let upgrade_impl = if is_fallible {
        quote! {
            impl ::core::convert::TryFrom<#enum_ident> for #target_ty {
                type Error = #error_ty;

                fn try_from(value: #enum_ident) -> ::core::result::Result<Self, Self::Error> {
                    match value {
                        #(#upgrade_arms)*
                    }
                }
            }

            impl #enum_ident {
                pub fn upgrade(self) -> ::core::result::Result<#target_ty, #error_ty> {
                    ::core::convert::TryInto::try_into(self)
                }
            }
        }
    } else {
        quote! {
            impl ::core::convert::From<#enum_ident> for #target_ty {
                fn from(value: #enum_ident) -> Self {
                    match value {
                        #(#upgrade_arms)*
                    }
                }
            }

            impl #enum_ident {
                pub fn upgrade(self) -> #target_ty {
                    ::core::convert::Into::into(self)
                }
            }
        }
    };

    Ok(quote! {
        #(#outer_derives)*
        #(#other_attrs)*
        #[derive(::serde::Serialize, ::serde::Deserialize)]
        #[serde(untagged)]
        #[allow(non_camel_case_types)]
        #vis enum #enum_ident {
            #(#variants,)*
        }

        impl ::core::convert::From<#target_ty> for #enum_ident {
            fn from(value: #target_ty) -> Self {
                let latest: #latest_ty = ::core::convert::Into::into(value);
                Self::#latest_ident(latest)
            }
        }

        #upgrade_impl
    })
}
