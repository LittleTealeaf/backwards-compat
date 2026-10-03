use std::collections::HashSet;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::dag::{DagPlan, resolve_dag};
use crate::parse::BackwardsCompatInput;

pub fn generate_conversions(input: &BackwardsCompatInput, dag_plan: &DagPlan) -> TokenStream {
    let target_ty = &input.target_ty;
    let target_ty_str = quote!(#target_ty).to_string();

    let mut seen_types = HashSet::new();
    let mut impls = Vec::new();

    let error_ty_tokens = match &input.error_ty {
        Some(err_ty) => quote!(#err_ty),
        None => {
            quote!(
                ::std::boxed::Box<
                    dyn ::std::error::Error + ::core::marker::Send + ::core::marker::Sync + 'static,
                >
            )
        }
    };

    for (i, v) in input.versions.iter().enumerate() {
        let v_ty = &v.ty;
        let v_ty_str = quote!(#v_ty).to_string();

        if v_ty_str == target_ty_str {
            continue;
        }

        let path = &dag_plan.paths[i];
        if path.steps.len() <= 1 {
            continue;
        }

        if !seen_types.insert(v_ty_str) {
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
                impl ::core::convert::TryFrom<#v_ty> for #target_ty {
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
                impl ::core::convert::From<#v_ty> for #target_ty {
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

pub fn generate_backwards_compat(
    input: BackwardsCompatInput,
    shadow_struct: Option<&syn::Ident>,
    extra_items: Option<TokenStream>,
) -> syn::Result<TokenStream> {
    let dag_plan = resolve_dag(&input)?;

    let target_ty = &input.target_ty;
    let tag_field = &input.tag_field;

    let mut helper_variants = Vec::new();
    let mut match_arms = Vec::new();

    let target_ty_str = quote!(#target_ty).to_string();
    for (i, v) in input.versions.iter().enumerate() {
        let var_ident = format_ident!("__V_{}", i);
        let tag_str = v.tag.to_tag_string();
        let ty = &v.ty;

        helper_variants.push(quote! {
            #[serde(rename = #tag_str)]
            #var_ident(#ty)
        });

        let path = &dag_plan.paths[i];
        let ty_str = quote!(#ty).to_string();

        if ty_str == target_ty_str || path.steps.is_empty() {
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
        if let Some(shadow_ident) = shadow_struct {
            let current_var_ident = format_ident!("__V_{}", input.versions.len());
            let current_tag_str = input.current_version.to_tag_string();

            helper_variants.push(quote! {
                #[serde(rename = #current_tag_str)]
                #current_var_ident(#shadow_ident)
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

    let latest_wire_ty = if dag_plan.target_is_wire {
        let shadow_ident = shadow_struct.unwrap();
        quote!(#shadow_ident)
    } else {
        let ty = &dag_plan.latest_wire_ty;
        quote!(#ty)
    };
    let current_tag_str = input.current_version.to_tag_string();

    let serialize_stmt = quote! {
        let latest: #latest_wire_ty = ::core::convert::Into::into(::core::clone::Clone::clone(self));
        let helper = __SerializeHelper::__Latest(&latest);
        ::serde::Serialize::serialize(&helper, __serializer)
    };

    let conversions = generate_conversions(&input, &dag_plan);
    let extra = extra_items.unwrap_or_default();

    let code = quote! {
        const _: () = {
            use ::serde::de::Error as _;

            #extra

            #[allow(non_camel_case_types, dead_code)]
            #[derive(::serde::Deserialize)]
            #[serde(tag = #tag_field)]
            enum __VersionHelper {
                #(#helper_variants,)*
            }

            #[allow(non_camel_case_types, dead_code)]
            #[derive(::serde::Serialize)]
            #[serde(tag = #tag_field)]
            enum __SerializeHelper<'__a> {
                #[serde(rename = #current_tag_str)]
                __Latest(&'__a #latest_wire_ty),
            }

            impl ::serde::Serialize for #target_ty {
                fn serialize<__S>(&self, __serializer: __S) -> ::core::result::Result<__S::Ok, __S::Error>
                where
                    __S: ::serde::Serializer,
                {
                    #serialize_stmt
                }
            }

            impl<'de> ::serde::Deserialize<'de> for #target_ty {
                fn deserialize<__D>(__deserializer: __D) -> ::core::result::Result<Self, __D::Error>
                where
                    __D: ::serde::Deserializer<'de>,
                {
                    let helper = <__VersionHelper as ::serde::Deserialize>::deserialize(__deserializer)?;
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

    #[test]
    fn test_generate_conversions_infallible() {
        let input: BackwardsCompatInput = syn::parse2(quote! {
            #[version = 2]
            pub compat TargetModel {
                1: ModelV1 => 2,
                2: ModelV2,
            }
        })
        .unwrap();

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
        let input: BackwardsCompatInput = syn::parse2(quote! {
            #[version = 2]
            pub compat TargetModel {
                #[fallible]
                1: ModelV1 => 2,
                2: ModelV2,
            }
        })
        .unwrap();

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
        let input: BackwardsCompatInput = syn::parse2(quote! {
            #[version = 2]
            #[error = CustomError]
            pub compat TargetModel {
                #[fallible]
                1: ModelV1 => 2,
                2: ModelV2,
            }
        })
        .unwrap();

        let dag_plan = resolve_dag(&input).unwrap();
        let conversions = generate_conversions(&input, &dag_plan);
        let rendered = conversions.to_string();

        assert!(rendered.contains("impl :: core :: convert :: TryFrom < ModelV1 > for TargetModel"));
        assert!(rendered.contains("type Error = CustomError"));
        assert!(rendered.contains(":: core :: convert :: TryInto :: < ModelV2 > :: try_into (cur) ?"));
    }

    #[test]
    fn test_generate_conversions_skips_target_and_duplicates() {
        let input: BackwardsCompatInput = syn::parse2(quote! {
            #[version = 3]
            pub compat TargetModel {
                1: ModelV1 => 2,
                2: ModelV1 => 3,
                3: ModelV2,
            }
        })
        .unwrap();

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
        let input: BackwardsCompatInput = syn::parse2(quote! {
            #[version = 3]
            pub compat TargetModel {
                1: ModelV1 => 2,
                #[fallible]
                2: ModelV2 => 3,
                3: ModelV3,
            }
        })
        .unwrap();

        let generated = generate_backwards_compat(input, None, None).unwrap();
        let rendered = generated.to_string();

        assert!(rendered.contains("__VersionHelper :: __V_0 (val) => :: core :: convert :: TryInto :: < TargetModel > :: try_into (val) . map_err (:: serde :: de :: Error :: custom)"));
        assert!(rendered.contains("__VersionHelper :: __V_1 (val) => :: core :: convert :: TryInto :: < TargetModel > :: try_into (val) . map_err (:: serde :: de :: Error :: custom)"));
        assert!(rendered.contains("__VersionHelper :: __V_2 (val) => :: core :: result :: Result :: Ok (:: core :: convert :: Into :: into (val))"));
    }

    #[test]
    fn test_serialize_helper_and_version_helper_derives() {
        let input: BackwardsCompatInput = syn::parse2(quote! {
            #[version = 3]
            pub compat TargetModel {
                1: ModelV1 => 2,
                2: ModelV2 => 3,
                3: ModelV3,
            }
        })
        .unwrap();

        let generated = generate_backwards_compat(input, None, None).unwrap();
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
        let input: BackwardsCompatInput = syn::parse2(quote! {
            #[version = 3]
            pub compat TargetModel {
                1: ModelV1 => 2,
                2: ModelV2 => 3,
            }
        })
        .unwrap();

        let shadow = format_ident!("TargetModelWire");
        let generated = generate_backwards_compat(input, Some(&shadow), None).unwrap();
        let rendered = generated.to_string();

        assert!(rendered.contains("__VersionHelper :: __V_2 (val) => :: core :: result :: Result :: Ok (:: core :: convert :: Into :: into (val))"));
        assert!(rendered.contains(
            "enum __SerializeHelper < '__a > { # [serde (rename = \"3\")] __Latest (& '__a TargetModelWire) , }"
        ));
    }

    #[test]
    fn test_generate_backwards_compat_target_is_wire_without_shadow_errors() {
        let input: BackwardsCompatInput = syn::parse2(quote! {
            #[version = 3]
            pub compat TargetModel {
                1: ModelV1 => 2,
                2: ModelV2 => 3,
            }
        })
        .unwrap();

        let result = generate_backwards_compat(input, None, None);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "using the domain struct as the active wire format is only supported when using the #[backwards_compat] attribute macro"
        );
    }
}
