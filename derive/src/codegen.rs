use std::collections::HashSet;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::dag::{DagPlan, resolve_dag};
use crate::parse::BackwardsCompatInput;

pub fn generate_conversions(input: &BackwardsCompatInput, dag_plan: &DagPlan) -> TokenStream {
    let target_ty = &input.target_ty;
    let (impl_generics, _, where_clause) = input.generics.split_for_impl();

    let mut seen_types = HashSet::new();
    let mut impls = Vec::new();

    let error_ty_tokens = input.error_ty.as_ref().map_or_else(
        || {
            quote!(
                ::std::boxed::Box<
                    dyn ::std::error::Error + ::core::marker::Send + ::core::marker::Sync + 'static,
                >
            )
        },
        |err_ty| quote!(#err_ty),
    );

    for (i, v) in input.versions.iter().enumerate() {
        let v_ty = &v.ty;

        if v_ty == target_ty {
            continue;
        }

        let Some(path) = dag_plan.paths.get(i) else {
            continue;
        };
        if path.steps.len() <= 1 {
            continue;
        }

        if !seen_types.insert(v_ty) {
            continue;
        }

        if path.is_fallible() {
            let mut step_tokens = Vec::new();
            for step in &path.steps {
                let to_ty = &step.to_ty;
                if step.fallible {
                    if input.error_ty.is_some() {
                        step_tokens.push(quote! {
                            let cur: #to_ty = ::core::convert::TryInto::<#to_ty>::try_into(cur)?;
                        });
                    } else {
                        step_tokens.push(quote! {
                            let cur: #to_ty = ::core::convert::TryInto::<#to_ty>::try_into(cur).map_err(::core::convert::Into::<#error_ty_tokens>::into)?;
                        });
                    }
                } else {
                    step_tokens.push(quote! {
                        let cur: #to_ty = ::core::convert::Into::<#to_ty>::into(cur);
                    });
                }
            }

            impls.push(quote! {
                impl #impl_generics ::core::convert::TryFrom<#v_ty> for #target_ty #where_clause {
                    type Error = #error_ty_tokens;

                    fn try_from(val: #v_ty) -> ::core::result::Result<Self, Self::Error> {
                        let cur = val;
                        #(#step_tokens)*
                        ::core::result::Result::Ok(cur)
                    }
                }
            });
        } else {
            let mut step_tokens = Vec::new();
            for step in &path.steps {
                let to_ty = &step.to_ty;
                step_tokens.push(quote! {
                    let cur: #to_ty = ::core::convert::Into::<#to_ty>::into(cur);
                });
            }

            impls.push(quote! {
                impl #impl_generics ::core::convert::From<#v_ty> for #target_ty #where_clause {
                    fn from(val: #v_ty) -> Self {
                        let cur = val;
                        #(#step_tokens)*
                        cur
                    }
                }
            });
        }
    }

    quote! {
        #(#impls)*
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code, reason = "fields used in macro code generation and testing")]
pub struct ShadowWireInfo<'a> {
    pub owned_ident: &'a syn::Ident,
    pub borrowed_ident: &'a syn::Ident,
    pub borrowed_ty_in_helper: TokenStream,
    pub construct_borrowed: TokenStream,
}

pub fn generate_backwards_compat(
    input: &BackwardsCompatInput,
    shadow_info: Option<&ShadowWireInfo<'_>>,
    extra_items: Option<TokenStream>,
) -> syn::Result<TokenStream> {
    let dag_plan = resolve_dag(input)?;

    let target_ty = &input.target_ty;
    let tag_field = &input.tag_field;

    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let mut helper_variants = Vec::new();
    let mut match_arms = Vec::new();

    let has_lifetimes = input.generics.lifetimes().next().is_some();
    let borrow_attr = if has_lifetimes {
        quote!(#[serde(borrow)])
    } else {
        quote!()
    };

    for (i, v) in input.versions.iter().enumerate() {
        let var_ident = format_ident!("__V_{}", i);
        let tag_str = v.tag.to_tag_string();
        let ty = &v.ty;

        helper_variants.push(quote! {
            #[serde(rename = #tag_str)]
            #var_ident(#borrow_attr #ty)
        });

        let Some(path) = dag_plan.paths.get(i) else {
            continue;
        };

        if ty == target_ty || path.steps.is_empty() {
            match_arms.push(quote! {
                __VersionHelper::#var_ident(val) => ::core::result::Result::Ok(val)
            });
        } else if !path.is_fallible() {
            match_arms.push(quote! {
                __VersionHelper::#var_ident(val) => ::core::result::Result::Ok(::core::convert::Into::into(val))
            });
        } else {
            match_arms.push(quote! {
                __VersionHelper::#var_ident(val) => ::core::convert::TryInto::<#target_ty>::try_into(val).map_err(::serde::de::Error::custom)
            });
        }
    }

    if dag_plan.target_is_wire {
        if let Some(info) = shadow_info {
            let current_var_ident = format_ident!("__V_{}", input.versions.len());
            let current_tag_str = input.current_version.to_tag_string();
            let owned_ident = info.owned_ident;

            helper_variants.push(quote! {
                #[serde(rename = #current_tag_str)]
                #current_var_ident(#borrow_attr #owned_ident #ty_generics)
            });

            match_arms.push(quote! {
                __VersionHelper::#current_var_ident(val) => ::core::result::Result::Ok(::core::convert::Into::into(val))
            });
        } else {
            return Err(syn::Error::new_spanned(
                &input.target_ty,
                "using the domain struct as the active wire format is only supported when using the #[backwards_compat] attribute macro",
            ));
        }
    }

    let (serialize_stmt, latest_wire_ty_in_helper) = if dag_plan.target_is_wire {
        let info = shadow_info.unwrap();
        let construct_borrowed = &info.construct_borrowed;
        let ty_in_helper = &info.borrowed_ty_in_helper;

        let stmt = quote! {
            let latest = #construct_borrowed;
            let helper = __SerializeHelper::__Latest(&latest);
            ::serde::Serialize::serialize(&helper, __serializer)
        };
        (stmt, quote!(#ty_in_helper))
    } else {
        let ty = &dag_plan.latest_wire_ty;
        let stmt = quote! {
            use ::backwards_compat::__private::{
                BorrowedWireConvert as _, FallbackWireConvert as _, WireConvert as __WireConvert,
            };

            let latest: #ty = (&__WireConvert(self)).convert_wire();
            let helper = __SerializeHelper::__Latest(&latest);
            ::serde::Serialize::serialize(&helper, __serializer)
        };
        let ty_in_helper = quote!(#ty);
        (stmt, ty_in_helper)
    };

    let current_tag_str = input.current_version.to_tag_string();

    let conversions = generate_conversions(input, &dag_plan);
    let extra = extra_items.unwrap_or_default();

    let mut de_generics = input.generics.clone();
    let lifetime_idents: Vec<_> = input.generics.lifetimes().map(|lt| &lt.lifetime).collect();
    let de_lifetime: syn::GenericParam = if lifetime_idents.is_empty() {
        syn::parse_quote!('__de)
    } else {
        syn::parse_quote!('__de: #(#lifetime_idents)+*)
    };
    de_generics.params.insert(0, de_lifetime);
    let (de_impl_generics, _, de_orig_where_clause) = de_generics.split_for_impl();

    let mut de_where_clause = de_orig_where_clause.cloned().unwrap_or_else(|| syn::WhereClause {
        where_token: syn::token::Where::default(),
        predicates: syn::punctuated::Punctuated::new(),
    });
    de_where_clause.predicates.push(syn::parse_quote!(
        __VersionHelper #ty_generics: ::serde::Deserialize<'__de>
    ));

    let mut ser_helper_generics = input.generics.clone();
    let ser_helper_lifetime: syn::GenericParam = syn::parse_quote!('__a);
    ser_helper_generics.params.insert(0, ser_helper_lifetime);
    let (_, ser_helper_ty_generics, _) = ser_helper_generics.split_for_impl();

    let mut ser_where_clause = where_clause.cloned().unwrap_or_else(|| syn::WhereClause {
        where_token: syn::token::Where::default(),
        predicates: syn::punctuated::Punctuated::new(),
    });
    ser_where_clause.predicates.push(syn::parse_quote!(
        for<'__a> __SerializeHelper #ser_helper_ty_generics: ::serde::Serialize
    ));

    let generics = &input.generics;

    let serde_tag_attr = input.content_field.as_ref().map_or_else(
        || {
            quote! {
                #[serde(tag = #tag_field)]
            }
        },
        |content| {
            quote! {
                #[serde(tag = #tag_field, content = #content)]
            }
        },
    );

    let code = quote! {
        const _: () = {
            use ::serde::de::Error as _;

            #extra

            #[doc(hidden)]
            #[allow(non_camel_case_types, dead_code)]
            #[derive(::serde::Deserialize)]
            #serde_tag_attr
            enum __VersionHelper #generics #where_clause {
                #(#helper_variants,)*
            }

            #[doc(hidden)]
            #[allow(non_camel_case_types, dead_code)]
            #[derive(::serde::Serialize)]
            #serde_tag_attr
            enum __SerializeHelper #ser_helper_generics #where_clause {
                #[serde(rename = #current_tag_str)]
                __Latest(&'__a #latest_wire_ty_in_helper),
            }

            impl #impl_generics ::serde::Serialize for #target_ty #ser_where_clause {
                fn serialize<__S>(&self, __serializer: __S) -> ::core::result::Result<__S::Ok, __S::Error>
                where
                    __S: ::serde::Serializer,
                {
                    #serialize_stmt
                }
            }

            impl #de_impl_generics ::serde::Deserialize<'__de> for #target_ty #de_where_clause {
                fn deserialize<__D>(__deserializer: __D) -> ::core::result::Result<Self, __D::Error>
                where
                    __D: ::serde::Deserializer<'__de>,
                {
                    let helper = <__VersionHelper #ty_generics as ::serde::Deserialize>::deserialize(__deserializer)?;
                    match helper {
                        #(#match_arms,)*
                    }
                }
            }

            #conversions
        };
    };

    if input.dump {
        eprintln!(
            "=== backwards_compat output for {} ===\n{}\n======================================",
            quote!(#target_ty),
            code
        );
    }

    Ok(code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::BackwardsCompatArgs;

    fn parse_test_input(args_tokens: proc_macro2::TokenStream) -> BackwardsCompatInput {
        let args: BackwardsCompatArgs = syn::parse2(args_tokens).unwrap();
        let target_ty: syn::Type = syn::parse_quote!(TargetModel);
        let vis: syn::Visibility = syn::parse_quote!(pub);
        args.into_input(target_ty, vis, syn::Generics::default()).unwrap()
    }

    #[test]
    fn test_generate_conversions_infallible() {
        let input = parse_test_input(quote! {
            version = 2,
            versions = [
                1: ModelV1 => 2,
                2: ModelV2,
            ]
        });

        let dag_plan = resolve_dag(&input).unwrap();
        let conversions = generate_conversions(&input, &dag_plan);
        let rendered = conversions.to_string();

        assert!(rendered.contains("impl :: core :: convert :: From < ModelV1 > for TargetModel"));
        // ModelV2 is a single step (2 => TargetModel), so it is not generated
        assert!(!rendered.contains("From < ModelV2 >"));
        assert!(!rendered.contains("TryFrom"));
    }

    #[test]
    fn test_generate_conversions_fallible_default_error() {
        let input = parse_test_input(quote! {
            version = 2,
            versions = [
                #[fallible]
                1: ModelV1 => 2,
                2: ModelV2,
            ]
        });

        let dag_plan = resolve_dag(&input).unwrap();
        let conversions = generate_conversions(&input, &dag_plan);
        let rendered = conversions.to_string();
        assert!(rendered.contains("impl :: core :: convert :: TryFrom < ModelV1 > for TargetModel"));
        assert!(rendered.contains("type Error = :: std :: boxed :: Box < dyn :: std :: error :: Error + :: core :: marker :: Send + :: core :: marker :: Sync"));
        assert!(rendered.contains("'static"));
        assert!(!rendered.contains("From < ModelV2 >"));
    }

    #[test]
    fn test_generate_conversions_fallible_custom_error() {
        let input = parse_test_input(quote! {
            version = 2,
            error = CustomError,
            versions = [
                #[fallible]
                1: ModelV1 => 2,
                2: ModelV2,
            ]
        });

        let dag_plan = resolve_dag(&input).unwrap();
        let conversions = generate_conversions(&input, &dag_plan);
        let rendered = conversions.to_string();

        assert!(rendered.contains("impl :: core :: convert :: TryFrom < ModelV1 > for TargetModel"));
        assert!(rendered.contains("type Error = CustomError"));
        assert!(rendered.contains(":: core :: convert :: TryInto :: < ModelV2 > :: try_into (cur) ?"));
    }

    #[test]
    fn test_generate_conversions_skips_target_and_duplicates() {
        let input = parse_test_input(quote! {
            version = 3,
            versions = [
                1: ModelV1 => 2,
                2: ModelV1 => 3,
                3: ModelV2,
            ]
        });

        let dag_plan = resolve_dag(&input).unwrap();
        let conversions = generate_conversions(&input, &dag_plan);
        let rendered = conversions.to_string();

        // ModelV1 should only be implemented once
        let matches: Vec<_> = rendered.match_indices("From < ModelV1 >").collect();
        assert_eq!(matches.len(), 1);

        // ModelV2 (1 step) should not have a From implementation
        assert!(!rendered.contains("From < ModelV2 > for TargetModel"));
    }

    #[test]
    fn test_generate_backwards_compat_match_arms() {
        let input = parse_test_input(quote! {
            version = 3,
            versions = [
                1: ModelV1 => 2,
                #[fallible]
                2: ModelV2 => 3,
                3: ModelV3,
            ]
        });

        let generated = generate_backwards_compat(&input, None, None).unwrap();
        let rendered = generated.to_string();

        assert!(rendered.contains("__VersionHelper :: __V_0 (val) => :: core :: convert :: TryInto :: < TargetModel > :: try_into (val) . map_err (:: serde :: de :: Error :: custom)"));
        assert!(rendered.contains("__VersionHelper :: __V_1 (val) => :: core :: convert :: TryInto :: < TargetModel > :: try_into (val) . map_err (:: serde :: de :: Error :: custom)"));
        assert!(rendered.contains("__VersionHelper :: __V_2 (val) => :: core :: result :: Result :: Ok (:: core :: convert :: Into :: into (val))"));
    }

    #[test]
    fn test_serialize_helper_and_version_helper_derives() {
        let input = parse_test_input(quote! {
            version = 3,
            versions = [
                1: ModelV1 => 2,
                2: ModelV2 => 3,
                3: ModelV3,
            ]
        });

        let generated = generate_backwards_compat(&input, None, None).unwrap();
        let rendered = generated.to_string();

        // __VersionHelper only derives Deserialize
        assert!(rendered.contains("# [derive (:: serde :: Deserialize)]"));
        // __SerializeHelper derives Serialize and only references ModelV3
        assert!(rendered.contains("# [derive (:: serde :: Serialize)]"));
        assert!(rendered.contains(
            "enum __SerializeHelper < '__a > { # [serde (rename = \"3\")] __Latest (& '__a ModelV3) , }"
        ));
    }

    #[test]
    fn test_generate_backwards_compat_target_is_wire_with_shadow() {
        let input = parse_test_input(quote! {
            version = 3,
            versions = [
                1: ModelV1 => 2,
                2: ModelV2 => 3,
            ]
        });

        let owned_shadow = format_ident!("TargetModelWire");
        let borrowed_shadow = format_ident!("TargetModelBorrowedWire");
        let shadow_info = ShadowWireInfo {
            owned_ident: &owned_shadow,
            borrowed_ident: &borrowed_shadow,
            borrowed_ty_in_helper: quote!(TargetModelBorrowedWire<'__a>),
            construct_borrowed: quote!(TargetModelBorrowedWire {}),
        };
        let generated = generate_backwards_compat(&input, Some(&shadow_info), None).unwrap();
        let rendered = generated.to_string();

        assert!(rendered.contains("__VersionHelper :: __V_2 (val) => :: core :: result :: Result :: Ok (:: core :: convert :: Into :: into (val))"));
        assert!(rendered.contains(
            "enum __SerializeHelper < '__a > { # [serde (rename = \"3\")] __Latest (& '__a TargetModelBorrowedWire < '__a >) , }"
        ));
    }

    #[test]
    fn test_generate_backwards_compat_target_is_wire_without_shadow_errors() {
        let input = parse_test_input(quote! {
            version = 3,
            versions = [
                1: ModelV1 => 2,
                2: ModelV2 => 3,
            ]
        });

        let result = generate_backwards_compat(&input, None, None);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "using the domain struct as the active wire format is only supported when using the #[backwards_compat] attribute macro"
        );
    }

    #[test]
    fn test_generate_backwards_compat_generics_and_where_clause() {
        let args: BackwardsCompatArgs = syn::parse2(quote! {
            version = 2,
            versions = [
                1: GenericModelV1<T> => 2,
                2: GenericModelV2<T>,
            ]
        })
        .unwrap();
        let target_ty: syn::Type = syn::parse_quote!(GenericModel<T>);
        let vis: syn::Visibility = syn::parse_quote!(pub);
        let mut generics: syn::Generics = syn::parse_quote!(<T: Clone + 'static>);
        generics.where_clause = syn::parse_quote!(where T: std::fmt::Debug);
        let input = args.into_input(target_ty, vis, generics).unwrap();

        let generated = generate_backwards_compat(&input, None, None).unwrap();
        let rendered = generated.to_string();

        assert!(
            rendered.contains("enum __VersionHelper < T : Clone + 'static > where T : std :: fmt :: Debug")
        );
        assert!(
            rendered.contains(
                "enum __SerializeHelper < '__a , T : Clone + 'static > where T : std :: fmt :: Debug"
            )
        );
        assert!(rendered.contains("impl < T : Clone + 'static > :: serde :: Serialize for GenericModel < T > where T : std :: fmt :: Debug"));
        assert!(rendered.contains("impl < '__de , T : Clone + 'static > :: serde :: Deserialize < '__de > for GenericModel < T > where T : std :: fmt :: Debug"));
    }

    #[test]
    fn test_generate_backwards_compat_adjacent_tagging() {
        let args: BackwardsCompatArgs = syn::parse2(quote! {
            tag = "type",
            content = "data",
            version = 2,
            versions = [
                1: ModelV1 => 2,
                2: ModelV2,
            ]
        })
        .unwrap();
        let target_ty: syn::Type = syn::parse_quote!(TargetModel);
        let vis: syn::Visibility = syn::parse_quote!(pub);
        let input = args.into_input(target_ty, vis, syn::Generics::default()).unwrap();

        let generated = generate_backwards_compat(&input, None, None).unwrap();
        let rendered = generated.to_string();

        assert!(rendered.contains("# [serde (tag = \"type\" , content = \"data\")] enum __VersionHelper"));
        assert!(rendered.contains("# [serde (tag = \"type\" , content = \"data\")] enum __SerializeHelper"));
    }

    #[test]
    fn test_generate_backwards_compat_explicit_wire_autoref() {
        let args: BackwardsCompatArgs = syn::parse2(quote! {
            version = 2,
            versions = [
                1: ModelV1 => 2,
                2: ModelV2,
            ]
        })
        .unwrap();
        let target_ty: syn::Type = syn::parse_quote!(TargetModel);
        let vis: syn::Visibility = syn::parse_quote!(pub);
        let input = args.into_input(target_ty, vis, syn::Generics::default()).unwrap();

        let generated = generate_backwards_compat(&input, None, None).unwrap();
        let rendered = generated.to_string();

        assert!(rendered.contains("use :: backwards_compat :: __private :: { BorrowedWireConvert as _ , FallbackWireConvert as _ , WireConvert as __WireConvert } ;"));
        assert!(rendered.contains("let latest : ModelV2 = (& __WireConvert (self)) . convert_wire () ;"));
    }
}
