use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::dag::resolve_dag;
use crate::parse::BackwardsCompatInput;

pub fn generate_backwards_compat(input: BackwardsCompatInput) -> syn::Result<TokenStream> {
    let dag_plan = resolve_dag(&input)?;

    let target_ty = &input.target_ty;
    let tag_field = &input.tag_field;

    let mut helper_variants = Vec::new();
    let mut match_arms = Vec::new();

    for (i, v) in input.versions.iter().enumerate() {
        let var_ident = format_ident!("__V_{}", i);
        let tag_str = v.tag.to_tag_string();
        let ty = &v.ty;

        helper_variants.push(quote! {
            #[serde(rename = #tag_str)]
            #var_ident(#ty)
        });

        let path = &dag_plan.paths[i];
        let mut step_tokens = Vec::new();

        for step in &path.steps {
            let to_ty = &step.to_ty;
            if step.fallible {
                step_tokens.push(quote! {
                    let cur: #to_ty = ::core::convert::TryInto::try_into(cur).map_err(::serde::de::Error::custom)?;
                });
            } else {
                step_tokens.push(quote! {
                    let cur: #to_ty = ::core::convert::Into::into(cur);
                });
            }
        }

        match_arms.push(quote! {
            __VersionHelper::#var_ident(val) => {
                let cur = val;
                #(#step_tokens)*
                ::core::result::Result::Ok(cur)
            }
        });
    }

    if dag_plan.target_is_wire {
        let current_var_ident = format_ident!("__V_{}", input.versions.len());
        let current_tag_str = input.current_version.to_tag_string();

        helper_variants.push(quote! {
            #[serde(rename = #current_tag_str)]
            #current_var_ident(#target_ty)
        });

        match_arms.push(quote! {
            __VersionHelper::#current_var_ident(val) => ::core::result::Result::Ok(val)
        });
    }

    let latest_var_ident = format_ident!("__V_{}", dag_plan.latest_variant_idx);
    let latest_wire_ty = &dag_plan.latest_wire_ty;

    let serialize_stmt = if dag_plan.target_is_wire {
        quote! {
            let latest: #latest_wire_ty = ::core::clone::Clone::clone(self);
            let helper = __VersionHelper::#latest_var_ident(latest);
            ::serde::Serialize::serialize(&helper, __serializer)
        }
    } else {
        quote! {
            let latest: #latest_wire_ty = ::core::convert::Into::into(::core::clone::Clone::clone(self));
            let helper = __VersionHelper::#latest_var_ident(latest);
            ::serde::Serialize::serialize(&helper, __serializer)
        }
    };

    let code = quote! {
        const _: () = {
            use ::serde::de::Error as _;

            #[allow(non_camel_case_types)]
            #[derive(::serde::Serialize, ::serde::Deserialize)]
            #[serde(tag = #tag_field)]
            enum __VersionHelper {
                #(#helper_variants,)*
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
