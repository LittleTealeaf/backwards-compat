use std::collections::HashMap;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use super::{SelfWireMode, cfg_attr};
use crate::dag::{MigrationStep, ResolvedPath, combine_cfg_any};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct MigrationStepKey {
    from_ty: syn::Type,
    to_ty: syn::Type,
    fallible: bool,
    with_fn: Option<syn::ExprPath>,
}

impl MigrationStepKey {
    fn from_step(step: &MigrationStep) -> Self {
        Self {
            from_ty: step.from_ty.clone(),
            to_ty: step.to_ty.clone(),
            fallible: step.fallible,
            with_fn: step.with_fn.clone(),
        }
    }
}

struct MigrationHelper {
    ident: syn::Ident,
    step: MigrationStep,
    next_helper: Option<(usize, bool)>,
    is_fallible: bool,
    unconditional: bool,
    cfgs: Vec<TokenStream>,
}

/// Deduplicates migration path suffixes into non-generic (with respect to `D: Deserializer`)
/// tail-chained helper functions, reducing $O(N^2)$ inlined step sequences to $O(N)$ shared helpers.
pub(super) struct MigrationHelperPool {
    suffix_to_id: HashMap<Vec<MigrationStepKey>, usize>,
    helpers: Vec<MigrationHelper>,
}

impl MigrationHelperPool {
    pub(super) fn new() -> Self {
        Self {
            suffix_to_id: HashMap::new(),
            helpers: Vec::new(),
        }
    }

    fn register_path(
        &mut self,
        steps: &[MigrationStep],
        path_cfg: Option<&TokenStream>,
    ) -> Option<(syn::Ident, bool)> {
        if steps.is_empty() {
            return None;
        }
        let step_keys: Vec<MigrationStepKey> = steps.iter().map(MigrationStepKey::from_step).collect();
        let mut child: Option<(usize, bool)> = None;

        for idx in (0..steps.len()).rev() {
            let Some(suffix_key) = step_keys.get(idx..).map(<[MigrationStepKey]>::to_vec) else {
                continue;
            };
            let id = if let Some(&existing_id) = self.suffix_to_id.get(&suffix_key) {
                if let Some(helper) = self.helpers.get_mut(existing_id) {
                    match path_cfg {
                        None => {
                            helper.unconditional = true;
                            helper.cfgs.clear();
                        }
                        Some(pred) if !helper.unconditional => {
                            helper.cfgs.push(pred.clone());
                        }
                        Some(_) => {}
                    }
                }
                existing_id
            } else {
                let Some(step) = steps.get(idx) else {
                    continue;
                };
                let new_id = self.helpers.len();
                let ident = format_ident!("__bc_migrate_{new_id}");
                let is_fallible = step.fallible || child.is_some_and(|(_, child_f)| child_f);
                let (unconditional, cfgs) =
                    path_cfg.map_or_else(|| (true, Vec::new()), |pred| (false, vec![pred.clone()]));
                self.helpers.push(MigrationHelper {
                    ident,
                    step: step.clone(),
                    next_helper: child,
                    is_fallible,
                    unconditional,
                    cfgs,
                });
                self.suffix_to_id.insert(suffix_key, new_id);
                new_id
            };
            let is_fallible = self.helpers.get(id).is_some_and(|h| h.is_fallible);
            child = Some((id, is_fallible));
        }

        child.and_then(|(root_id, is_fallible)| {
            self.helpers.get(root_id).map(|h| (h.ident.clone(), is_fallible))
        })
    }

    pub(super) fn emit_helpers(
        &self,
        target_ty: &syn::Type,
        impl_generics: &syn::ImplGenerics<'_>,
        where_clause: Option<&syn::WhereClause>,
    ) -> TokenStream {
        let mut defs = Vec::with_capacity(self.helpers.len());
        for helper in &self.helpers {
            let fn_ident = &helper.ident;
            let from_ty = &helper.step.from_ty;
            let to_ty = &helper.step.to_ty;
            let cfg_tokens = if helper.unconditional {
                TokenStream::new()
            } else {
                let combined = combine_cfg_any(helper.cfgs.iter());
                cfg_attr(combined.as_ref())
            };

            let ret_ty = if helper.is_fallible {
                quote!(::core::result::Result<#target_ty, ::backwards_compat::__private::String>)
            } else {
                quote!(#target_ty)
            };

            let step_expr = match (helper.step.with_fn.as_ref(), helper.step.fallible) {
                (Some(fn_path), true) => quote! {
                    (#fn_path)(__val).map_err(|__e| ::backwards_compat::__private::format!("{__e}"))?
                },
                (Some(fn_path), false) => quote! {
                    (#fn_path)(__val)
                },
                (None, true) => quote! {
                    ::core::convert::TryInto::<#to_ty>::try_into(__val)
                        .map_err(|__e| ::backwards_compat::__private::format!("{__e}"))?
                },
                (None, false) => quote! {
                    ::core::convert::Into::<#to_ty>::into(__val)
                },
            };

            let body = match helper.next_helper {
                None => {
                    if helper.is_fallible {
                        quote! {
                            let __next: #to_ty = #step_expr;
                            ::core::result::Result::Ok(__next)
                        }
                    } else {
                        step_expr
                    }
                }
                Some((child_id, child_fallible)) => {
                    let child_ident = &self
                        .helpers
                        .get(child_id)
                        .expect("child migration helper index must be valid")
                        .ident;
                    if child_fallible {
                        quote! {
                            let __next: #to_ty = #step_expr;
                            #child_ident(__next)
                        }
                    } else if helper.is_fallible {
                        quote! {
                            let __next: #to_ty = #step_expr;
                            ::core::result::Result::Ok(#child_ident(__next))
                        }
                    } else {
                        quote! {
                            let __next: #to_ty = #step_expr;
                            #child_ident(__next)
                        }
                    }
                }
            };

            defs.push(quote! {
                #cfg_tokens
                #[allow(dead_code, reason = "internal migration helper")]
                fn #fn_ident #impl_generics (__val: #from_ty) -> #ret_ty #where_clause {
                    #body
                }
            });
        }
        quote!(#(#defs)*)
    }
}

pub(super) fn generate_arm_conversion_stmts(
    ty: &syn::Type,
    target_ty: &syn::Type,
    path: Option<&ResolvedPath>,
    wire_mode: SelfWireMode<'_>,
    migration_pool: &mut MigrationHelperPool,
) -> TokenStream {
    if ty == target_ty {
        return match wire_mode {
            SelfWireMode::Shadow(_) => quote! {
                let __val: #target_ty = ::core::convert::Into::into(__val);
            },
            SelfWireMode::Direct | SelfWireMode::Disallowed => TokenStream::new(),
        };
    }
    let Some(p) = path else {
        return TokenStream::new();
    };
    let Some((fn_ident, is_fallible)) = migration_pool.register_path(&p.steps, p.cfg.as_ref()) else {
        return TokenStream::new();
    };
    if is_fallible {
        quote! {
            let __val: #target_ty = #fn_ident(__val).map_err(::serde::de::Error::custom)?;
        }
    } else {
        quote! {
            let __val: #target_ty = #fn_ident(__val);
        }
    }
}
