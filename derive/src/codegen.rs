use std::collections::{HashMap, HashSet};

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::dag::{DagPlan, MigrationStep, ResolvedPath, combine_cfg_all, combine_cfg_any};
use crate::parse::{BackwardsCompatInput, SerializableMode, VersionTag};

fn cfg_attr(pred: Option<&TokenStream>) -> TokenStream {
    pred.map_or_else(TokenStream::new, |p| quote!(#[cfg(#p)]))
}

fn target_type_name(target_ty: &syn::Type) -> String {
    if let syn::Type::Path(tp) = target_ty
        && let Some(last) = tp.path.segments.last()
    {
        let s = last.ident.to_string();
        return s.strip_prefix("r#").unwrap_or(&s).to_owned();
    }
    "__SerializeHelper".to_owned()
}

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
struct MigrationHelperPool {
    suffix_to_id: HashMap<Vec<MigrationStepKey>, usize>,
    helpers: Vec<MigrationHelper>,
}

impl MigrationHelperPool {
    fn new() -> Self {
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

    fn emit_helpers(
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

#[derive(Debug, Clone)]
#[allow(dead_code, reason = "fields used in macro code generation and testing")]
pub struct ShadowWireInfo<'a> {
    pub owned_ident: &'a syn::Ident,
    pub borrowed_ident: &'a syn::Ident,
    pub borrowed_ty_in_helper: TokenStream,
    pub construct_borrowed: TokenStream,
    pub is_unit_struct: bool,
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code, reason = "Direct variant is used by field-level codegen and tests")]
pub enum SelfWireMode<'a> {
    Shadow(&'a ShadowWireInfo<'a>),
    Direct,
    Disallowed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VersionMode {
    Int,
    Str,
}

impl VersionMode {
    fn from_input(input: &BackwardsCompatInput) -> Self {
        let inspect = |tag: &VersionTag| match tag {
            VersionTag::Int(_) => Some(Self::Int),
            VersionTag::String(_) => Some(Self::Str),
            VersionTag::Untagged => None,
        };
        for v in &input.versions {
            if let Some(mode) = inspect(&v.tag) {
                return mode;
            }
        }
        inspect(&input.current_version)
            .or_else(|| inspect(&input.serialize_version))
            .unwrap_or(Self::Int)
    }

    fn version_ty(self) -> TokenStream {
        match self {
            Self::Int => quote!(u64),
            Self::Str => quote!(&'static str),
        }
    }

    fn tag_expr(self, tag: &VersionTag) -> TokenStream {
        match (self, tag) {
            (Self::Int, VersionTag::Int(n)) => quote!(#n),
            (Self::Str, VersionTag::String(s)) => quote!(#s),
            _ => unreachable!("tag_expr called with mismatched or untagged version"),
        }
    }

    fn tag_deserialize_ty(self) -> TokenStream {
        match self {
            Self::Int => quote!(u64),
            Self::Str => quote!(::backwards_compat::__private::StrTag<'__de>),
        }
    }

    fn tag_param_ty(self) -> TokenStream {
        match self {
            Self::Int => quote!(u64),
            Self::Str => quote!(&str),
        }
    }

    fn tag_arg_expr(self, ident: &syn::Ident) -> TokenStream {
        match self {
            Self::Int => quote!(#ident),
            Self::Str => quote!(#ident.as_str()),
        }
    }

    fn tag_match_pattern(self, tag: &VersionTag) -> TokenStream {
        match (self, tag) {
            (Self::Int, VersionTag::Int(n)) => quote!(#n),
            (Self::Str, VersionTag::String(s)) => quote!(#s),
            _ => unreachable!("tag_match_pattern called with mismatched or untagged version"),
        }
    }

    fn tag_mode_ty(self) -> TokenStream {
        match self {
            Self::Int => quote!(::backwards_compat::__private::IntTagMode),
            Self::Str => quote!(::backwards_compat::__private::StrTagMode),
        }
    }
}

struct ExtCanonicalVariant<'a> {
    idx_const: syn::Ident,
    tag: &'a VersionTag,
    variant_name: String,
    cfg: Option<TokenStream>,
}

fn collect_external_variants<'a>(
    input: &'a BackwardsCompatInput,
    dag_plan: &DagPlan,
    prefix: &str,
) -> Vec<ExtCanonicalVariant<'a>> {
    let mut variants = Vec::new();
    for (i, v) in input.versions.iter().enumerate() {
        if v.tag.is_untagged() || v.is_alias {
            continue;
        }
        let Some(path) = dag_plan.paths.get(i) else {
            continue;
        };
        let idx_const = format_ident!("{prefix}_IDX_{}", variants.len());
        variants.push(ExtCanonicalVariant {
            idx_const,
            tag: &v.tag,
            variant_name: v.tag.to_tag_string(),
            cfg: path.cfg.clone(),
        });
    }

    let has_declared_current = input
        .versions
        .iter()
        .any(|v| v.tag.matches(&input.current_version));
    if dag_plan.target_is_wire && !has_declared_current {
        let idx_const = format_ident!("{prefix}_IDX_{}", variants.len());
        variants.push(ExtCanonicalVariant {
            idx_const,
            tag: &input.current_version,
            variant_name: input.current_version.to_tag_string(),
            cfg: None,
        });
    }

    variants
}

fn emit_ext_variant_indices(
    variants: &[ExtCanonicalVariant<'_>],
    prefix: &str,
    idx_ty: &TokenStream,
) -> TokenStream {
    let mut stmts = Vec::with_capacity(1 + variants.len() * 3);
    let n0 = format_ident!("{prefix}_N_0");
    stmts.push(quote!(const #n0: #idx_ty = 0;));
    for (k, var) in variants.iter().enumerate() {
        let n_cur = format_ident!("{prefix}_N_{k}");
        let n_next = format_ident!("{prefix}_N_{}", k + 1);
        let idx_ident = &var.idx_const;
        if let Some(ref pred) = var.cfg {
            stmts.push(quote! {
                #[cfg(#pred)]
                const #idx_ident: #idx_ty = #n_cur;
                #[cfg(#pred)]
                const #n_next: #idx_ty = #n_cur + 1;
                #[cfg(not(#pred))]
                const #n_next: #idx_ty = #n_cur;
            });
        } else {
            stmts.push(quote! {
                const #idx_ident: #idx_ty = #n_cur;
                const #n_next: #idx_ty = #n_cur + 1;
            });
        }
    }
    quote!(#(#stmts)*)
}

fn ext_variant_lookup(variants: &[ExtCanonicalVariant<'_>], tag: &VersionTag) -> (TokenStream, String) {
    variants
        .iter()
        .find(|v| v.canonical_tag_matches(tag))
        .map_or_else(
            || (quote!(0u32), tag.to_tag_string()),
            |var| {
                let idx_ident = &var.idx_const;
                let idx_expr = var.cfg.as_ref().map_or_else(
                    || quote!(#idx_ident),
                    |pred| {
                        quote! {
                            {
                                #[cfg(#pred)]
                                { #idx_ident }
                                #[cfg(not(#pred))]
                                { 0u32 }
                            }
                        }
                    },
                );
                (idx_expr, var.variant_name.clone())
            },
        )
}

impl ExtCanonicalVariant<'_> {
    fn canonical_tag_matches(&self, tag: &VersionTag) -> bool {
        self.tag.matches(tag)
    }
}

fn tag_display_str(tag: &VersionTag) -> String {
    match tag {
        VersionTag::Int(n) => n.to_string(),
        VersionTag::String(s) => format!("\"{s}\""),
        VersionTag::Untagged => "_".to_owned(),
    }
}

pub fn build_de_generics(generics: &syn::Generics) -> syn::Generics {
    let mut de_generics = generics.clone();
    let lifetime_idents: Vec<_> = generics.lifetimes().map(|lt| &lt.lifetime).collect();
    let de_lifetime: syn::GenericParam = if lifetime_idents.is_empty() {
        syn::parse_quote!('__de)
    } else {
        syn::parse_quote!('__de: #(#lifetime_idents)+*)
    };
    de_generics.params.insert(0, de_lifetime);
    de_generics
}

fn non_empty_where_clause(wc: syn::WhereClause) -> Option<syn::WhereClause> {
    if wc.predicates.is_empty() { None } else { Some(wc) }
}

fn resolve_de_ty(
    ty: &syn::Type,
    target_ty: &syn::Type,
    ty_generics: &syn::TypeGenerics<'_>,
    wire_mode: SelfWireMode<'_>,
    err_msg: &str,
) -> syn::Result<TokenStream> {
    if ty == target_ty {
        match wire_mode {
            SelfWireMode::Shadow(info) => {
                let owned_ident = info.owned_ident;
                Ok(quote!(#owned_ident #ty_generics))
            }
            SelfWireMode::Direct => Ok(quote!(#target_ty)),
            SelfWireMode::Disallowed => Err(syn::Error::new_spanned(target_ty, err_msg)),
        }
    } else {
        Ok(quote!(#ty))
    }
}

fn generate_arm_conversion_stmts(
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

pub struct SerializeCore {
    pub helper_def: TokenStream,
    pub body: TokenStream,
    pub where_clause: Option<syn::WhereClause>,
}

pub fn generate_serialize_core(
    input: &BackwardsCompatInput,
    dag_plan: &DagPlan,
    wire_mode: SelfWireMode<'_>,
    value_expr: &TokenStream,
) -> syn::Result<SerializeCore> {
    let target_ty = &input.target_ty;
    let target_name = target_type_name(target_ty);
    let tag_field = &input.tag_field;
    let (_, ty_generics, where_clause) = input.generics.split_for_impl();

    let ser_tag_expr = match &input.serialize_version {
        VersionTag::Int(n) => quote!(#n),
        VersionTag::String(s) => quote!(#s),
        VersionTag::Untagged => unreachable!("serialize_version cannot be untagged"),
    };

    let is_internal_unit_shadow = !input.external
        && input.content_field.is_none()
        && dag_plan.serialize_target_is_wire
        && dag_plan.serialize_with_fn.is_none()
        && matches!(wire_mode, SelfWireMode::Shadow(info) if info.is_unit_struct);

    if is_internal_unit_shadow {
        let body = quote! {
            ::backwards_compat::__private::serialize_unit_tagged(
                __serializer,
                #target_name,
                #tag_field,
                &#ser_tag_expr,
            )
        };
        return Ok(SerializeCore {
            helper_def: TokenStream::new(),
            body,
            where_clause: where_clause.cloned(),
        });
    }

    let content_field_opt = input.content_field.as_ref();
    let ext_variants = if input.external {
        collect_external_variants(input, dag_plan, "__BC_EXT_SER")
    } else {
        Vec::new()
    };
    let ext_idx_defs = if input.external {
        emit_ext_variant_indices(&ext_variants, "__BC_EXT_SER", &quote!(u32))
    } else {
        TokenStream::new()
    };
    let (ext_idx_expr, ext_variant_name) = if input.external {
        ext_variant_lookup(&ext_variants, &input.serialize_version)
    } else {
        (TokenStream::new(), String::new())
    };

    let emit_ser_expr = |wire_ref: TokenStream| -> TokenStream {
        if input.external {
            quote! {
                #ext_idx_defs
                ::serde::Serializer::serialize_newtype_variant(
                    __serializer,
                    #target_name,
                    #ext_idx_expr,
                    #ext_variant_name,
                    #wire_ref,
                )
            }
        } else if let Some(content_field) = content_field_opt {
            quote! {
                ::backwards_compat::__private::serialize_adjacently_tagged(
                    __serializer,
                    #target_name,
                    #tag_field,
                    &#ser_tag_expr,
                    #content_field,
                    #wire_ref,
                )
            }
        } else {
            quote! {
                ::backwards_compat::__private::serialize_internally_tagged(
                    __serializer,
                    #target_name,
                    #tag_field,
                    &#ser_tag_expr,
                    #wire_ref,
                )
            }
        }
    };

    let (body, latest_wire_ty_in_helper) = if let Some(ref ser_fn) = dag_plan.serialize_with_fn {
        let ser_ty = if dag_plan.serialize_target_is_wire {
            match wire_mode {
                SelfWireMode::Shadow(info) => {
                    let owned_ident = info.owned_ident;
                    quote!(#owned_ident #ty_generics)
                }
                SelfWireMode::Direct => quote!(#target_ty),
                SelfWireMode::Disallowed => {
                    return Err(syn::Error::new_spanned(
                        &input.target_ty,
                        "using the domain struct as the active wire format is only supported when using the #[backwards_compat] attribute macro",
                    ));
                }
            }
        } else {
            let ty = &dag_plan.serialize_wire_ty;
            quote!(#ty)
        };
        let ser_call = emit_ser_expr(quote!(&latest));
        let s = quote! {
            let latest: #ser_ty = (#ser_fn)(#value_expr);
            #ser_call
        };
        (s, ser_ty)
    } else if dag_plan.serialize_target_is_wire {
        match wire_mode {
            SelfWireMode::Shadow(info) => {
                let construct_borrowed = &info.construct_borrowed;
                let ty_in_helper = &info.borrowed_ty_in_helper;
                let ser_call = emit_ser_expr(quote!(&latest));
                let s = quote! {
                    let latest = #construct_borrowed;
                    #ser_call
                };
                (s, quote!(#ty_in_helper))
            }
            SelfWireMode::Direct => {
                let s = emit_ser_expr(quote!(#value_expr));
                (s, quote!(#target_ty))
            }
            SelfWireMode::Disallowed => {
                return Err(syn::Error::new_spanned(
                    &input.target_ty,
                    "using the domain struct as the active wire format is only supported when using the #[backwards_compat] attribute macro",
                ));
            }
        }
    } else {
        let ty = &dag_plan.serialize_wire_ty;
        let ser_call = emit_ser_expr(quote!(&latest));
        let s = quote! {
            use ::backwards_compat::__private::{
                BorrowedWireConvert as _, FallbackWireConvert as _, WireConvert as __WireConvert,
            };

            let latest: #ty = __WireConvert::<_, #ty>::new(#value_expr).convert_wire();
            #ser_call
        };
        (s, quote!(#ty))
    };

    let mut ser_wc = where_clause.cloned().unwrap_or_else(|| syn::WhereClause {
        where_token: syn::token::Where::default(),
        predicates: syn::punctuated::Punctuated::new(),
    });
    ser_wc.predicates.push(syn::parse_quote!(
        for<'__a> #latest_wire_ty_in_helper: ::serde::Serialize
    ));

    Ok(SerializeCore {
        helper_def: TokenStream::new(),
        body,
        where_clause: Some(ser_wc),
    })
}

struct TaggedArm<'a> {
    tag: &'a VersionTag,
    is_alias: bool,
    de_ty: TokenStream,
    conv_stmts: TokenStream,
    cfg: Option<&'a TokenStream>,
    version_expr: Option<TokenStream>,
    arm_trait: Option<syn::Ident>,
}

struct UntaggedArm<'a> {
    de_ty: TokenStream,
    conv_stmts: TokenStream,
    cfg: Option<&'a TokenStream>,
    arm_trait: Option<syn::Ident>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArmTaggingKind {
    Internal,
    Adjacent,
    External,
}

pub struct UntaggedDeserializeCore {
    pub overall_cfg: Option<TokenStream>,
    pub where_clause: Option<syn::WhereClause>,
    pub body: TokenStream,
}

pub struct DeserializeCore {
    pub helper_defs: TokenStream,
    pub base_where_clause: Option<syn::WhereClause>,
    pub base_body: TokenStream,
    pub untagged: Option<UntaggedDeserializeCore>,
}

fn wrap_tagged_ok(version_expr: Option<&TokenStream>, track_migrated: bool) -> TokenStream {
    if track_migrated {
        let ver = version_expr.expect("version_expr required when track_migrated is true");
        quote! {
            ::core::result::Result::Ok(::backwards_compat::Migrated {
                value: __val,
                source_version: ::core::option::Option::Some(#ver),
            })
        }
    } else {
        quote!(::core::result::Result::Ok(__val))
    }
}

fn wrap_untagged_ok(track_migrated: bool) -> TokenStream {
    if track_migrated {
        quote! {
            ::core::result::Result::Ok(::backwards_compat::Migrated {
                value: __val,
                source_version: ::core::option::Option::None,
            })
        }
    } else {
        quote!(::core::result::Result::Ok(__val))
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "internal helper for cfg-gated generic arm traits"
)]
fn emit_tagged_arm_trait(
    arm_trait: &syn::Ident,
    pred: &TokenStream,
    de_ty: &TokenStream,
    conv_stmts: &TokenStream,
    target_ty: &syn::Type,
    de_impl_generics: &syn::ImplGenerics<'_>,
    de_orig_where_clause: Option<&syn::WhereClause>,
    tagging_kind: ArmTaggingKind,
) -> TokenStream {
    let mut arm_wc = de_orig_where_clause.cloned().unwrap_or_else(|| syn::WhereClause {
        where_token: syn::token::Where::default(),
        predicates: syn::punctuated::Punctuated::new(),
    });
    arm_wc
        .predicates
        .push(syn::parse_quote!(#de_ty: ::serde::Deserialize<'__de>));

    let (trait_methods, impl_methods) = match tagging_kind {
        ArmTaggingKind::Internal => (
            quote! {
                #[cfg(#pred)]
                fn __bc_de<__D>(__d: __D) -> ::core::result::Result<Self, __D::Error>
                where
                    __D: ::serde::Deserializer<'__de>;
            },
            quote! {
                fn __bc_de<__D>(__d: __D) -> ::core::result::Result<Self, __D::Error>
                where
                    __D: ::serde::Deserializer<'__de>,
                {
                    let __val = <#de_ty as ::serde::Deserialize<'__de>>::deserialize(__d)?;
                    #conv_stmts
                    ::core::result::Result::Ok(__val)
                }
            },
        ),
        ArmTaggingKind::Adjacent => (
            quote! {
                #[cfg(#pred)]
                fn __bc_de<__D>(__d: __D) -> ::core::result::Result<Self, __D::Error>
                where
                    __D: ::serde::Deserializer<'__de>;

                #[cfg(#pred)]
                fn __bc_de_adjacent<__M>(
                    __map: &mut __M,
                    __content_field: &'static str,
                ) -> ::core::result::Result<Self, __M::Error>
                where
                    __M: ::serde::de::MapAccess<'__de>;

                #[cfg(#pred)]
                fn __bc_de_seq<__A>(
                    __seq: &mut __A,
                    __exp: &dyn ::serde::de::Expected,
                ) -> ::core::result::Result<Self, __A::Error>
                where
                    __A: ::serde::de::SeqAccess<'__de>;
            },
            quote! {
                fn __bc_de<__D>(__d: __D) -> ::core::result::Result<Self, __D::Error>
                where
                    __D: ::serde::Deserializer<'__de>,
                {
                    let __val = <#de_ty as ::serde::Deserialize<'__de>>::deserialize(__d)?;
                    #conv_stmts
                    ::core::result::Result::Ok(__val)
                }

                fn __bc_de_adjacent<__M>(
                    __map: &mut __M,
                    __content_field: &'static str,
                ) -> ::core::result::Result<Self, __M::Error>
                where
                    __M: ::serde::de::MapAccess<'__de>,
                {
                    let __val = ::backwards_compat::__private::read_adjacent_content::<__M, #de_ty>(
                        __map,
                        __content_field,
                    )?;
                    #conv_stmts
                    ::core::result::Result::Ok(__val)
                }

                fn __bc_de_seq<__A>(
                    __seq: &mut __A,
                    __exp: &dyn ::serde::de::Expected,
                ) -> ::core::result::Result<Self, __A::Error>
                where
                    __A: ::serde::de::SeqAccess<'__de>,
                {
                    let ::core::option::Option::Some(__val) = __seq.next_element::<#de_ty>()? else {
                        return ::core::result::Result::Err(::serde::de::Error::invalid_length(1, __exp));
                    };
                    #conv_stmts
                    ::core::result::Result::Ok(__val)
                }
            },
        ),
        ArmTaggingKind::External => (
            quote! {
                #[cfg(#pred)]
                fn __bc_de_variant<__V>(__variant: __V) -> ::core::result::Result<Self, __V::Error>
                where
                    __V: ::serde::de::VariantAccess<'__de>;
            },
            quote! {
                fn __bc_de_variant<__V>(__variant: __V) -> ::core::result::Result<Self, __V::Error>
                where
                    __V: ::serde::de::VariantAccess<'__de>,
                {
                    let __val = ::serde::de::VariantAccess::newtype_variant::<#de_ty>(__variant)?;
                    #conv_stmts
                    ::core::result::Result::Ok(__val)
                }
            },
        ),
    };

    quote! {
        #[doc(hidden)]
        #[allow(non_camel_case_types, dead_code, reason = "internal cfg-gated deserialization helper")]
        trait #arm_trait<'__de>: ::core::marker::Sized {
            #trait_methods
        }

        #[cfg(#pred)]
        impl #de_impl_generics #arm_trait<'__de> for #target_ty #arm_wc {
            #impl_methods
        }

        #[cfg(not(#pred))]
        impl #de_impl_generics #arm_trait<'__de> for #target_ty #de_orig_where_clause {}
    }
}

fn emit_untagged_arm_trait(
    arm_trait: &syn::Ident,
    pred: &TokenStream,
    de_ty: &TokenStream,
    conv_stmts: &TokenStream,
    target_ty: &syn::Type,
    de_impl_generics: &syn::ImplGenerics<'_>,
    de_orig_where_clause: Option<&syn::WhereClause>,
) -> TokenStream {
    let mut arm_wc = de_orig_where_clause.cloned().unwrap_or_else(|| syn::WhereClause {
        where_token: syn::token::Where::default(),
        predicates: syn::punctuated::Punctuated::new(),
    });
    arm_wc
        .predicates
        .push(syn::parse_quote!(#de_ty: ::serde::Deserialize<'__de>));

    quote! {
        #[doc(hidden)]
        #[allow(non_camel_case_types, dead_code, reason = "internal cfg-gated untagged deserialization helper")]
        trait #arm_trait<'__de>: ::core::marker::Sized {
            #[cfg(#pred)]
            fn __bc_try_untagged<__D>(
                __d: __D,
            ) -> ::core::result::Result<::core::option::Option<Self>, __D::Error>
            where
                __D: ::serde::Deserializer<'__de>;
        }

        #[cfg(#pred)]
        impl #de_impl_generics #arm_trait<'__de> for #target_ty #arm_wc {
            fn __bc_try_untagged<__D>(
                __d: __D,
            ) -> ::core::result::Result<::core::option::Option<Self>, __D::Error>
            where
                __D: ::serde::Deserializer<'__de>,
            {
                if let ::core::result::Result::Ok(__val) =
                    <#de_ty as ::serde::Deserialize<'__de>>::deserialize(__d)
                {
                    #conv_stmts
                    return ::core::result::Result::Ok(::core::option::Option::Some(__val));
                }
                ::core::result::Result::Ok(::core::option::Option::None)
            }
        }

        #[cfg(not(#pred))]
        impl #de_impl_generics #arm_trait<'__de> for #target_ty #de_orig_where_clause {}
    }
}

struct VisitorContext<'a> {
    input: &'a BackwardsCompatInput,
    version_mode: VersionMode,
    de_impl_generics: &'a syn::ImplGenerics<'a>,
    de_ty_generics: &'a syn::TypeGenerics<'a>,
    de_orig_where_clause: Option<&'a syn::WhereClause>,
    tagged_arms: &'a [TaggedArm<'a>],
    untagged_arms: &'a [UntaggedArm<'a>],
    supported_str_items: &'a [TokenStream],
    current_int: u64,
    has_disabled_init: &'a TokenStream,
    track_migrated: bool,
}

fn emit_unknown_version_expr(
    version_mode: VersionMode,
    tag_expr: &TokenStream,
    current_int: u64,
    supported_str_items: &[TokenStream],
    has_disabled_init: &TokenStream,
) -> TokenStream {
    match version_mode {
        VersionMode::Int => quote! {
            let __supported: &[&str] = &[#(#supported_str_items,)*];
            #has_disabled_init
            ::core::result::Result::Err(
                ::backwards_compat::__private::unknown_int_version_error(
                    #tag_expr,
                    #current_int,
                    __supported,
                    __has_disabled,
                ),
            )
        },
        VersionMode::Str => quote! {
            let __supported: &[&str] = &[#(#supported_str_items,)*];
            #has_disabled_init
            ::core::result::Result::Err(
                ::backwards_compat::__private::unknown_str_version_error(
                    #tag_expr,
                    __supported,
                    __has_disabled,
                ),
            )
        },
    }
}

fn emit_external_visitor_body(
    ctx: &VisitorContext<'_>,
    visitor_where_clause: Option<&syn::WhereClause>,
) -> TokenStream {
    let target_ty = &ctx.input.target_ty;
    let target_name = target_type_name(target_ty);
    let version_mode = ctx.version_mode;
    let de_impl_generics = ctx.de_impl_generics;
    let de_ty_generics = ctx.de_ty_generics;
    let de_orig_where_clause = ctx.de_orig_where_clause;
    let supported_str_items = ctx.supported_str_items;
    let current_int = ctx.current_int;
    let has_disabled_init = ctx.has_disabled_init;

    let value_ty = if ctx.track_migrated {
        quote!(::backwards_compat::Migrated<#target_ty>)
    } else {
        quote!(#target_ty)
    };

    let mut variant_names = Vec::new();
    let mut field_enum_variants = Vec::new();
    let mut str_match_arms = Vec::new();
    let mut visit_enum_arms = Vec::new();

    // Build dense 0..N canonical indices in u64 for `visit_u64` (zero `as` casts).
    let mut idx_stmts = vec![quote!(
        const __BC_EXT_DE_N_0: u64 = 0;
    )];
    let mut u64_match_arms = Vec::new();
    let mut canonical_k = 0usize;

    for (arm_idx, arm) in ctx.tagged_arms.iter().enumerate() {
        let cfg_tokens = cfg_attr(arm.cfg);
        let field_ident = format_ident!("__Arm_{arm_idx}");
        let tag_str = arm.tag.to_tag_string();

        field_enum_variants.push(quote! {
            #cfg_tokens
            #field_ident
        });

        str_match_arms.push(quote! {
            #cfg_tokens
            #tag_str => ::core::result::Result::Ok(__BcExtField::#field_ident),
        });

        if !arm.is_alias {
            variant_names.push(quote! { #cfg_tokens #tag_str });

            let n_cur = format_ident!("__BC_EXT_DE_N_{canonical_k}");
            let n_next = format_ident!("__BC_EXT_DE_N_{}", canonical_k + 1);
            let idx_ident = format_ident!("__BC_EXT_DE_IDX_{canonical_k}");
            if let Some(pred) = arm.cfg {
                idx_stmts.push(quote! {
                    #[cfg(#pred)]
                    const #idx_ident: u64 = #n_cur;
                    #[cfg(#pred)]
                    const #n_next: u64 = #n_cur + 1;
                    #[cfg(not(#pred))]
                    const #n_next: u64 = #n_cur;
                });
            } else {
                idx_stmts.push(quote! {
                    const #idx_ident: u64 = #n_cur;
                    const #n_next: u64 = #n_cur + 1;
                });
            }

            u64_match_arms.push(quote! {
                #cfg_tokens
                #idx_ident => ::core::result::Result::Ok(__BcExtField::#field_ident),
            });
            canonical_k += 1;
        }

        let wrap = wrap_tagged_ok(arm.version_expr.as_ref(), ctx.track_migrated);
        let arm_body = arm.arm_trait.as_ref().map_or_else(
            || {
                let de_ty = &arm.de_ty;
                let conv = &arm.conv_stmts;
                quote! {
                    let __val = ::serde::de::VariantAccess::newtype_variant::<#de_ty>(__variant)?;
                    #conv
                    #wrap
                }
            },
            |arm_trait| {
                quote! {
                    let __val = <#target_ty as #arm_trait<'__de>>::__bc_de_variant(__variant)?;
                    #wrap
                }
            },
        );
        visit_enum_arms.push(quote! {
            #cfg_tokens
            __BcExtField::#field_ident => {
                #arm_body
            }
        });
    }

    let str_fallback = match version_mode {
        VersionMode::Int => quote! {
            _ => {
                let __supported: &[&str] = &[#(#supported_str_items,)*];
                #has_disabled_init
                if let ::core::result::Result::Ok(__n) = __v.parse::<u64>() {
                    ::core::result::Result::Err(
                        ::backwards_compat::__private::unknown_int_version_error(
                            __n,
                            #current_int,
                            __supported,
                            __has_disabled,
                        ),
                    )
                } else {
                    ::core::result::Result::Err(
                        ::backwards_compat::__private::unknown_str_version_error(
                            __v,
                            __supported,
                            __has_disabled,
                        ),
                    )
                }
            }
        },
        VersionMode::Str => quote! {
            _ => {
                let __supported: &[&str] = &[#(#supported_str_items,)*];
                #has_disabled_init
                ::core::result::Result::Err(
                    ::backwards_compat::__private::unknown_str_version_error(
                        __v,
                        __supported,
                        __has_disabled,
                    ),
                )
            }
        },
    };

    quote! {
        #[allow(non_camel_case_types, dead_code, reason = "internal external-tagging variant identifier")]
        enum __BcExtField {
            #(#field_enum_variants,)*
        }

        struct __BcExtFieldVisitor;

        impl<'__de> ::serde::de::Visitor<'__de> for __BcExtFieldVisitor {
            type Value = __BcExtField;

            fn expecting(&self, __formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                __formatter.write_str("variant identifier")
            }

            fn visit_u64<__E>(self, __v: u64) -> ::core::result::Result<Self::Value, __E>
            where
                __E: ::serde::de::Error,
            {
                #(#idx_stmts)*
                match __v {
                    #(#u64_match_arms)*
                    _ => ::core::result::Result::Err(::serde::de::Error::invalid_value(
                        ::serde::de::Unexpected::Unsigned(__v),
                        &self,
                    )),
                }
            }

            fn visit_str<__E>(self, __v: &str) -> ::core::result::Result<Self::Value, __E>
            where
                __E: ::serde::de::Error,
            {
                match __v {
                    #(#str_match_arms)*
                    #str_fallback
                }
            }

            fn visit_bytes<__E>(self, __v: &[u8]) -> ::core::result::Result<Self::Value, __E>
            where
                __E: ::serde::de::Error,
            {
                match ::core::str::from_utf8(__v) {
                    ::core::result::Result::Ok(__s) => self.visit_str(__s),
                    ::core::result::Result::Err(_) => ::core::result::Result::Err(
                        ::serde::de::Error::invalid_value(::serde::de::Unexpected::Bytes(__v), &self),
                    ),
                }
            }
        }

        impl<'__de> ::serde::Deserialize<'__de> for __BcExtField {
            fn deserialize<__D2>(__deserializer: __D2) -> ::core::result::Result<Self, __D2::Error>
            where
                __D2: ::serde::Deserializer<'__de>,
            {
                __deserializer.deserialize_identifier(__BcExtFieldVisitor)
            }
        }

        #[allow(non_camel_case_types, dead_code, reason = "internal external-tagging visitor")]
        struct __BcVisitor #de_impl_generics (
            ::core::marker::PhantomData<fn() -> #target_ty>,
            ::core::marker::PhantomData<&'__de ()>,
        ) #de_orig_where_clause;

        impl #de_impl_generics ::serde::de::Visitor<'__de> for __BcVisitor #de_ty_generics #visitor_where_clause {
            type Value = #value_ty;

            fn expecting(&self, __formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                __formatter.write_str("an externally tagged versioned value")
            }

            fn visit_enum<__A>(self, __data: __A) -> ::core::result::Result<Self::Value, __A::Error>
            where
                __A: ::serde::de::EnumAccess<'__de>,
            {
                let (__field, __variant) = ::serde::de::EnumAccess::variant::<__BcExtField>(__data)?;
                match __field {
                    #(#visit_enum_arms,)*
                }
            }
        }

        const __VARIANTS: &'static [&'static str] = &[#(#variant_names,)*];
        let __visitor: __BcVisitor #de_ty_generics = __BcVisitor(
            ::core::marker::PhantomData,
            ::core::marker::PhantomData,
        );
        __deserializer.deserialize_enum(#target_name, __VARIANTS, __visitor)
    }
}

fn emit_visitor_body(
    ctx: &VisitorContext<'_>,
    visitor_where_clause: Option<&syn::WhereClause>,
    accepts_untagged: bool,
) -> TokenStream {
    if ctx.input.external {
        return emit_external_visitor_body(ctx, visitor_where_clause);
    }

    let target_ty = &ctx.input.target_ty;
    let tag_field = &ctx.input.tag_field;
    let content_field_opt = ctx.input.content_field.as_ref();
    let version_mode = ctx.version_mode;
    let tag_de_ty = version_mode.tag_deserialize_ty();
    let tag_param_ty = version_mode.tag_param_ty();
    let tag_mode_ty = version_mode.tag_mode_ty();
    let tag_val_ident = format_ident!("__tag_val");
    let tag_val_arg = version_mode.tag_arg_expr(&tag_val_ident);
    let de_impl_generics = ctx.de_impl_generics;
    let de_ty_generics = ctx.de_ty_generics;
    let de_orig_where_clause = ctx.de_orig_where_clause;
    let supported_str_items = ctx.supported_str_items;
    let current_int = ctx.current_int;
    let has_disabled_init = ctx.has_disabled_init;

    let value_ty = if ctx.track_migrated {
        quote!(::backwards_compat::Migrated<#target_ty>)
    } else {
        quote!(#target_ty)
    };

    let unknown_err_expr = emit_unknown_version_expr(
        version_mode,
        &quote!(__tag),
        current_int,
        supported_str_items,
        has_disabled_init,
    );
    let unknown_version_arm = quote! {
        _ => {
            #unknown_err_expr
        }
    };

    // __bc_dispatch_de: used by internal tagging (fast & slow) and adjacent tagging (slow path).
    let dispatch_de_arms: Vec<TokenStream> = ctx
        .tagged_arms
        .iter()
        .map(|arm| {
            let cfg_tokens = cfg_attr(arm.cfg);
            let pat = version_mode.tag_match_pattern(arm.tag);
            let wrap = wrap_tagged_ok(arm.version_expr.as_ref(), ctx.track_migrated);
            arm.arm_trait.as_ref().map_or_else(
                || {
                    let de_ty = &arm.de_ty;
                    let conv = &arm.conv_stmts;
                    quote! {
                        #cfg_tokens
                        #pat => {
                            let __val = <#de_ty as ::serde::Deserialize<'__de>>::deserialize(__d)?;
                            #conv
                            #wrap
                        }
                    }
                },
                |arm_trait| {
                    quote! {
                        #cfg_tokens
                        #pat => {
                            let __val = <#target_ty as #arm_trait<'__de>>::__bc_de(__d)?;
                            #wrap
                        }
                    }
                },
            )
        })
        .collect();

    let internal_dispatch_impl = quote! {
        impl #de_impl_generics ::backwards_compat::__private::InternalMapDispatch<'__de, #tag_mode_ty>
            for __BcVisitor #de_ty_generics #visitor_where_clause
        {
            type Value = #value_ty;

            fn dispatch_tagged<__D2>(
                __tag: #tag_param_ty,
                __d: __D2,
            ) -> ::core::result::Result<Self::Value, __D2::Error>
            where
                __D2: ::serde::Deserializer<'__de>,
            {
                match __tag {
                    #(#dispatch_de_arms,)*
                    #unknown_version_arm
                }
            }
        }
    };

    // Adjacent-tagging streaming helper (AdjacentMapDispatch impl).
    let adjacent_dispatch_impl = content_field_opt.map_or_else(TokenStream::new, |content_field| {
        let adj_arms: Vec<TokenStream> = ctx
            .tagged_arms
            .iter()
            .map(|arm| {
                let cfg_tokens = cfg_attr(arm.cfg);
                let pat = version_mode.tag_match_pattern(arm.tag);
                let wrap = wrap_tagged_ok(arm.version_expr.as_ref(), ctx.track_migrated);
                arm.arm_trait.as_ref().map_or_else(
                    || {
                        let de_ty = &arm.de_ty;
                        let conv = &arm.conv_stmts;
                        quote! {
                            #cfg_tokens
                            #pat => {
                                let __val = ::backwards_compat::__private::read_adjacent_content::<__M, #de_ty>(
                                    __map,
                                    #content_field,
                                )?;
                                #conv
                                #wrap
                            }
                        }
                    },
                    |arm_trait| {
                        quote! {
                            #cfg_tokens
                            #pat => {
                                let __val = <#target_ty as #arm_trait<'__de>>::__bc_de_adjacent(
                                    __map,
                                    #content_field,
                                )?;
                                #wrap
                            }
                        }
                    },
                )
            })
            .collect();

        quote! {
            impl #de_impl_generics ::backwards_compat::__private::AdjacentMapDispatch<'__de, #tag_mode_ty>
                for __BcVisitor #de_ty_generics #visitor_where_clause
            {
                fn dispatch_adjacent_streaming<__M>(
                    __tag: #tag_param_ty,
                    __map: &mut __M,
                ) -> ::core::result::Result<Self::Value, __M::Error>
                where
                    __M: ::serde::de::MapAccess<'__de>,
                {
                    match __tag {
                        #(#adj_arms,)*
                        #unknown_version_arm
                    }
                }
            }
        }
    });

    // Untagged dispatch logic (if accepts_untagged).
    let is_single_unconditional_untagged = accepts_untagged
        && ctx.untagged_arms.len() == 1
        && ctx.untagged_arms.first().is_some_and(|arm| arm.cfg.is_none());

    let untagged_fallback_closure = if is_single_unconditional_untagged {
        quote! {
            |__content| {
                <Self as ::backwards_compat::__private::UntaggedSingleDispatch<'__de>>::dispatch_de(
                    ::backwards_compat::__private::ContentDeserializer::new(__content),
                )
            }
        }
    } else {
        quote! {
            |__content| {
                <Self as ::backwards_compat::__private::UntaggedMultiDispatch<'__de>>::dispatch_content(
                    __content,
                )
            }
        }
    };

    // visit_map implementation delegating to shared crate-level map scanners.
    let visit_map_method = content_field_opt.map_or_else(
        || {
            if accepts_untagged {
                quote! {
                    fn visit_map<__A>(self, __map: __A) -> ::core::result::Result<Self::Value, __A::Error>
                    where
                        __A: ::serde::de::MapAccess<'__de>,
                    {
                        ::backwards_compat::__private::visit_internal_map_untagged::<
                            #tag_mode_ty,
                            Self,
                            __A,
                            _,
                        >(__map, #tag_field, #untagged_fallback_closure)
                    }
                }
            } else {
                quote! {
                    fn visit_map<__A>(self, __map: __A) -> ::core::result::Result<Self::Value, __A::Error>
                    where
                        __A: ::serde::de::MapAccess<'__de>,
                    {
                        ::backwards_compat::__private::visit_internal_map::<#tag_mode_ty, Self, __A>(
                            __map,
                            #tag_field,
                        )
                    }
                }
            }
        },
        |content_field| {
            if accepts_untagged {
                quote! {
                    fn visit_map<__A>(self, __map: __A) -> ::core::result::Result<Self::Value, __A::Error>
                    where
                        __A: ::serde::de::MapAccess<'__de>,
                    {
                        ::backwards_compat::__private::visit_adjacent_map_untagged::<
                            #tag_mode_ty,
                            Self,
                            __A,
                            _,
                        >(__map, #tag_field, #content_field, #untagged_fallback_closure)
                    }
                }
            } else {
                quote! {
                    fn visit_map<__A>(self, __map: __A) -> ::core::result::Result<Self::Value, __A::Error>
                    where
                        __A: ::serde::de::MapAccess<'__de>,
                    {
                        ::backwards_compat::__private::visit_adjacent_map::<#tag_mode_ty, Self, __A>(
                            __map,
                            #tag_field,
                            #content_field,
                        )
                    }
                }
            }
        },
    );

    let expecting_msg = if accepts_untagged {
        "a versioned map or unversioned value"
    } else if content_field_opt.is_some() {
        "a map or sequence"
    } else {
        "a map"
    };

    let (trait_impl, outer_drive) = if !accepts_untagged {
        let extra_visit_methods = content_field_opt.map_or_else(
            || {
                quote! {
                    fn visit_newtype_struct<__D2>(
                        self,
                        __deserializer: __D2,
                    ) -> ::core::result::Result<Self::Value, __D2::Error>
                    where
                        __D2: ::serde::Deserializer<'__de>,
                    {
                        __deserializer.deserialize_any(self)
                    }
                }
            },
            |content_field| {
                let seq_arms: Vec<TokenStream> = ctx
                    .tagged_arms
                    .iter()
                    .map(|arm| {
                        let cfg_tokens = cfg_attr(arm.cfg);
                        let pat = version_mode.tag_match_pattern(arm.tag);
                        let wrap = wrap_tagged_ok(arm.version_expr.as_ref(), ctx.track_migrated);
                        arm.arm_trait.as_ref().map_or_else(
                            || {
                                let de_ty = &arm.de_ty;
                                let conv = &arm.conv_stmts;
                                quote! {
                                    #cfg_tokens
                                    #pat => {
                                        let ::core::option::Option::Some(__val) = __seq.next_element::<#de_ty>()? else {
                                            return ::core::result::Result::Err(
                                                ::serde::de::Error::invalid_length(1, &self),
                                            );
                                        };
                                        #conv
                                        #wrap
                                    }
                                }
                            },
                            |arm_trait| {
                                quote! {
                                    #cfg_tokens
                                    #pat => {
                                        let __val = <#target_ty as #arm_trait<'__de>>::__bc_de_seq(
                                            &mut __seq,
                                            &self,
                                        )?;
                                        #wrap
                                    }
                                }
                            },
                        )
                    })
                    .collect();

                quote! {
                    fn visit_newtype_struct<__D2>(
                        self,
                        __deserializer: __D2,
                    ) -> ::core::result::Result<Self::Value, __D2::Error>
                    where
                        __D2: ::serde::Deserializer<'__de>,
                    {
                        __deserializer.deserialize_struct("", &[#tag_field, #content_field], self)
                    }

                    fn visit_seq<__A>(self, mut __seq: __A) -> ::core::result::Result<Self::Value, __A::Error>
                    where
                        __A: ::serde::de::SeqAccess<'__de>,
                    {
                        let ::core::option::Option::Some(__tag_val) =
                            __seq.next_element::<#tag_de_ty>()?
                        else {
                            return ::core::result::Result::Err(::serde::de::Error::invalid_length(0, &self));
                        };
                        let __tag = #tag_val_arg;
                        match __tag {
                            #(#seq_arms,)*
                            #unknown_version_arm
                        }
                    }
                }
            },
        );

        let visitor_impl = quote! {
            impl #de_impl_generics ::serde::de::Visitor<'__de> for __BcVisitor #de_ty_generics #visitor_where_clause {
                type Value = #value_ty;

                fn expecting(&self, __formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                    __formatter.write_str(#expecting_msg)
                }

                #visit_map_method
                #extra_visit_methods
            }
        };
        let drive = content_field_opt.map_or_else(
            || quote!(__deserializer.deserialize_any(__visitor)),
            |content_field| {
                quote!(__deserializer.deserialize_struct("", &[#tag_field, #content_field], __visitor))
            },
        );
        (visitor_impl, drive)
    } else if is_single_unconditional_untagged {
        let arm = ctx
            .untagged_arms
            .first()
            .expect("single unconditional untagged arm");
        let de_ty = &arm.de_ty;
        let conv = &arm.conv_stmts;
        let wrap = wrap_untagged_ok(ctx.track_migrated);

        let dispatch_impl = quote! {
            impl #de_impl_generics ::backwards_compat::__private::UntaggedSingleDispatch<'__de>
                for __BcVisitor #de_ty_generics #visitor_where_clause
            {
                type Value = #value_ty;

                fn expecting(&self, __formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                    __formatter.write_str(#expecting_msg)
                }

                #visit_map_method

                fn dispatch_de<__D2>(
                    __d: __D2,
                ) -> ::core::result::Result<Self::Value, __D2::Error>
                where
                    __D2: ::serde::Deserializer<'__de>,
                {
                    let __val = <#de_ty as ::serde::Deserialize<'__de>>::deserialize(__d)?;
                    #conv
                    #wrap
                }
            }
        };
        let drive = quote! {
            __deserializer.deserialize_any(
                ::backwards_compat::__private::UntaggedSingleVisitor(__visitor),
            )
        };
        (dispatch_impl, drive)
    } else {
        let wrap = wrap_untagged_ok(ctx.track_migrated);
        let num_arms = ctx.untagged_arms.len();
        let try_arms: Vec<TokenStream> = ctx
            .untagged_arms
            .iter()
            .enumerate()
            .map(|(idx, arm)| {
                let cfg_tokens = cfg_attr(arm.cfg);
                let is_last_unconditional = idx + 1 == num_arms && arm.cfg.is_none();

                arm.arm_trait.as_ref().map_or_else(
                    || {
                        let de_ty = &arm.de_ty;
                        let conv = &arm.conv_stmts;
                        let deserializer_expr = if is_last_unconditional {
                            quote!(::backwards_compat::__private::ContentDeserializer::<__E>::new(__content))
                        } else {
                            quote!(::backwards_compat::__private::ContentRefDeserializer::<__E>::new(&__content))
                        };
                        quote! {
                            #cfg_tokens
                            if let ::core::result::Result::Ok(__val) =
                                <#de_ty as ::serde::Deserialize<'__de>>::deserialize(#deserializer_expr)
                            {
                                #conv
                                return #wrap;
                            }
                        }
                    },
                    |arm_trait| {
                        quote! {
                            #cfg_tokens
                            if let ::core::option::Option::Some(__val) =
                                <#target_ty as #arm_trait<'__de>>::__bc_try_untagged(
                                    ::backwards_compat::__private::ContentRefDeserializer::<__E>::new(&__content),
                                )?
                            {
                                return #wrap;
                            }
                        }
                    },
                )
            })
            .collect();

        let dispatch_impl = quote! {
            impl #de_impl_generics ::backwards_compat::__private::UntaggedMultiDispatch<'__de>
                for __BcVisitor #de_ty_generics #visitor_where_clause
            {
                type Value = #value_ty;

                fn expecting(&self, __formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                    __formatter.write_str(#expecting_msg)
                }

                #visit_map_method

                fn dispatch_content<__E>(
                    __content: ::backwards_compat::__private::Content<'__de>,
                ) -> ::core::result::Result<Self::Value, __E>
                where
                    __E: ::serde::de::Error,
                {
                    #(#try_arms)*
                    ::core::result::Result::Err(::serde::de::Error::custom(
                        "data did not match any unversioned schema variant",
                    ))
                }
            }
        };
        let drive = quote! {
            __deserializer.deserialize_any(
                ::backwards_compat::__private::UntaggedMultiVisitor(__visitor),
            )
        };
        (dispatch_impl, drive)
    };

    quote! {
        #[allow(non_camel_case_types, dead_code, reason = "internal streaming visitor")]
        struct __BcVisitor #de_impl_generics (
            ::core::marker::PhantomData<fn() -> #target_ty>,
            ::core::marker::PhantomData<&'__de ()>,
        ) #de_orig_where_clause;

        #internal_dispatch_impl
        #adjacent_dispatch_impl

        #trait_impl

        let __visitor: __BcVisitor #de_ty_generics = __BcVisitor(
            ::core::marker::PhantomData,
            ::core::marker::PhantomData,
        );
        #outer_drive
    }
}

pub fn generate_deserialize_core(
    input: &BackwardsCompatInput,
    dag_plan: &DagPlan,
    wire_mode: SelfWireMode<'_>,
    track_migrated: bool,
) -> syn::Result<DeserializeCore> {
    let target_ty = &input.target_ty;
    let version_mode = VersionMode::from_input(input);
    let (orig_impl_generics, ty_generics, orig_where_clause) = input.generics.split_for_impl();
    let is_generic = input.generics.type_params().next().is_some();
    let tagging_kind = if input.external {
        ArmTaggingKind::External
    } else if input.content_field.is_some() {
        ArmTaggingKind::Adjacent
    } else {
        ArmTaggingKind::Internal
    };

    let de_generics = build_de_generics(&input.generics);
    let (de_impl_generics, de_ty_generics, de_orig_where_clause) = de_generics.split_for_impl();

    let mut tagged_arms = Vec::new();
    let mut supported_str_items = Vec::new();
    let mut disabled_checks = Vec::new();
    let mut has_any_gated_tagged = false;
    let mut helper_defs_list = Vec::new();
    let mut migration_pool = MigrationHelperPool::new();

    let mut base_wc = de_orig_where_clause.cloned().unwrap_or_else(|| syn::WhereClause {
        where_token: syn::token::Where::default(),
        predicates: syn::punctuated::Punctuated::new(),
    });
    let mut seen_de_tys: HashSet<String> = HashSet::new();

    for (i, v) in input.versions.iter().enumerate() {
        if v.tag.is_untagged() {
            continue;
        }
        let Some(path) = dag_plan.paths.get(i) else {
            continue;
        };
        let cfg_tokens = cfg_attr(path.cfg.as_ref());
        let de_ty = resolve_de_ty(
            &v.ty,
            target_ty,
            &ty_generics,
            wire_mode,
            "using the domain struct as the active wire format is only supported when using the #[backwards_compat] attribute macro",
        )?;
        let conv_stmts =
            generate_arm_conversion_stmts(&v.ty, target_ty, Some(path), wire_mode, &mut migration_pool);
        let version_expr = track_migrated.then(|| version_mode.tag_expr(&v.tag));

        let arm_trait = if is_generic {
            if let Some(ref pred) = path.cfg {
                let trait_ident = format_ident!("__BcArm_{}", i);
                helper_defs_list.push(emit_tagged_arm_trait(
                    &trait_ident,
                    pred,
                    &de_ty,
                    &conv_stmts,
                    target_ty,
                    &de_impl_generics,
                    de_orig_where_clause,
                    tagging_kind,
                ));
                base_wc
                    .predicates
                    .push(syn::parse_quote!(#target_ty: #trait_ident<'__de>));
                Some(trait_ident)
            } else {
                let key = de_ty.to_string();
                if seen_de_tys.insert(key) {
                    base_wc
                        .predicates
                        .push(syn::parse_quote!(#de_ty: ::serde::Deserialize<'__de>));
                }
                None
            }
        } else {
            None
        };

        tagged_arms.push(TaggedArm {
            tag: &v.tag,
            is_alias: v.is_alias,
            de_ty,
            conv_stmts,
            cfg: path.cfg.as_ref(),
            version_expr,
            arm_trait,
        });

        let disp = tag_display_str(&v.tag);
        supported_str_items.push(quote! { #cfg_tokens #disp });

        if let Some(ref pred) = path.cfg {
            has_any_gated_tagged = true;
            disabled_checks.push(quote! { #[cfg(not(#pred))] () });
        }
    }

    let has_declared_current = input
        .versions
        .iter()
        .any(|v| v.tag.matches(&input.current_version));

    if dag_plan.target_is_wire && !has_declared_current {
        let de_ty = resolve_de_ty(
            target_ty,
            target_ty,
            &ty_generics,
            wire_mode,
            "using the domain struct as the active wire format is only supported when using the #[backwards_compat] attribute macro",
        )?;
        let conv_stmts =
            generate_arm_conversion_stmts(target_ty, target_ty, None, wire_mode, &mut migration_pool);
        let version_expr = track_migrated.then(|| version_mode.tag_expr(&input.current_version));

        if is_generic {
            let key = de_ty.to_string();
            if seen_de_tys.insert(key) {
                base_wc
                    .predicates
                    .push(syn::parse_quote!(#de_ty: ::serde::Deserialize<'__de>));
            }
        }

        tagged_arms.push(TaggedArm {
            tag: &input.current_version,
            is_alias: false,
            de_ty,
            conv_stmts,
            cfg: None,
            version_expr,
            arm_trait: None,
        });

        let disp = tag_display_str(&input.current_version);
        supported_str_items.push(quote!(#disp));
    }

    let untagged_entries: Vec<_> = input
        .versions
        .iter()
        .enumerate()
        .filter(|(_, v)| v.tag.is_untagged())
        .collect();

    let mut untagged_arms = Vec::new();
    let mut untagged_wc = base_wc.clone();
    let mut seen_untagged_de_tys = seen_de_tys;

    for &(idx, untagged_v) in &untagged_entries {
        let path = dag_plan.paths.get(idx);
        let cfg = path.and_then(|p| p.cfg.as_ref());
        let de_ty = resolve_de_ty(
            &untagged_v.ty,
            target_ty,
            &ty_generics,
            wire_mode,
            "using the domain struct as the unversioned wire format is only supported when using the #[backwards_compat] attribute macro",
        )?;
        let conv_stmts =
            generate_arm_conversion_stmts(&untagged_v.ty, target_ty, path, wire_mode, &mut migration_pool);

        let arm_trait = if is_generic {
            if let Some(pred) = cfg {
                let trait_ident = format_ident!("__BcArm_{}", idx);
                helper_defs_list.push(emit_untagged_arm_trait(
                    &trait_ident,
                    pred,
                    &de_ty,
                    &conv_stmts,
                    target_ty,
                    &de_impl_generics,
                    de_orig_where_clause,
                ));
                untagged_wc
                    .predicates
                    .push(syn::parse_quote!(#target_ty: #trait_ident<'__de>));
                Some(trait_ident)
            } else {
                let key = de_ty.to_string();
                if seen_untagged_de_tys.insert(key) {
                    untagged_wc
                        .predicates
                        .push(syn::parse_quote!(#de_ty: ::serde::Deserialize<'__de>));
                }
                None
            }
        } else {
            None
        };

        untagged_arms.push(UntaggedArm {
            de_ty,
            conv_stmts,
            cfg,
            arm_trait,
        });
    }

    let migration_helpers = migration_pool.emit_helpers(target_ty, &orig_impl_generics, orig_where_clause);
    if !migration_helpers.is_empty() {
        helper_defs_list.push(migration_helpers);
    }

    let current_int = match input.current_version {
        VersionTag::Int(n) => n,
        VersionTag::String(_) | VersionTag::Untagged => 0,
    };

    let has_disabled_init = if has_any_gated_tagged {
        quote! {
            let __disabled: &[()] = &[#(#disabled_checks,)*];
            let __has_disabled = !__disabled.is_empty();
        }
    } else {
        quote! {
            let __has_disabled = false;
        }
    };

    let base_where_clause = non_empty_where_clause(base_wc);
    let untagged_where_clause = non_empty_where_clause(untagged_wc);

    let visitor_ctx = VisitorContext {
        input,
        version_mode,
        de_impl_generics: &de_impl_generics,
        de_ty_generics: &de_ty_generics,
        de_orig_where_clause,
        tagged_arms: &tagged_arms,
        untagged_arms: &untagged_arms,
        supported_str_items: &supported_str_items,
        current_int,
        has_disabled_init: &has_disabled_init,
        track_migrated,
    };

    let untagged_overall_cfg =
        if untagged_arms.is_empty() || untagged_arms.iter().any(|arm| arm.cfg.is_none()) {
            None
        } else {
            combine_cfg_any(untagged_arms.iter().filter_map(|arm| arm.cfg))
        };

    let base_body = if untagged_arms.is_empty() || untagged_overall_cfg.is_some() {
        emit_visitor_body(&visitor_ctx, base_where_clause.as_ref(), false)
    } else {
        TokenStream::new()
    };

    let untagged = if untagged_arms.is_empty() {
        None
    } else {
        let body = emit_visitor_body(&visitor_ctx, untagged_where_clause.as_ref(), true);
        Some(UntaggedDeserializeCore {
            overall_cfg: untagged_overall_cfg,
            where_clause: untagged_where_clause,
            body,
        })
    };

    Ok(DeserializeCore {
        helper_defs: quote!(#(#helper_defs_list)*),
        base_where_clause,
        base_body,
        untagged,
    })
}

struct SerializableArm {
    tag: VersionTag,
    cfg: Option<TokenStream>,
    body: TokenStream,
    bound_ty: Option<TokenStream>,
}

fn generate_backwards_compat_serialize(
    input: &BackwardsCompatInput,
    dag_plan: &DagPlan,
    wire_mode: SelfWireMode<'_>,
    version_mode: VersionMode,
    helper_defs_list: &mut Vec<TokenStream>,
) -> syn::Result<TokenStream> {
    let target_ty = &input.target_ty;
    let target_name = target_type_name(target_ty);
    let tag_field = &input.tag_field;
    let content_field_opt = input.content_field.as_ref();
    let is_adjacent = !input.external && content_field_opt.is_some();
    let is_generic = input.generics.type_params().next().is_some();
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let ext_variants = if input.external {
        collect_external_variants(input, dag_plan, "__BC_EXT_AS")
    } else {
        Vec::new()
    };
    let ext_idx_defs = if input.external {
        emit_ext_variant_indices(&ext_variants, "__BC_EXT_AS", &quote!(u32))
    } else {
        TokenStream::new()
    };

    let mut ser_as_wc = where_clause.cloned().unwrap_or_else(|| syn::WhereClause {
        where_token: syn::token::Where::default(),
        predicates: syn::punctuated::Punctuated::new(),
    });
    let mut seen_bound_tys: HashSet<String> = HashSet::new();
    let mut arms: Vec<(usize, SerializableArm)> = Vec::new();

    let build_arm = |tag: &VersionTag,
                     ty: &syn::Type,
                     cfg: Option<TokenStream>,
                     is_ser_ver: bool,
                     serializable: Option<&SerializableMode>|
     -> syn::Result<SerializableArm> {
        let tag_lit = match tag {
            VersionTag::Int(n) => quote!(#n),
            VersionTag::String(s) => quote!(#s),
            VersionTag::Untagged => unreachable!("serializable arm cannot be untagged"),
        };

        let emit_ser_call = |wire_ref: TokenStream| -> TokenStream {
            if input.external {
                let (ext_idx_expr, ext_variant_name) = ext_variant_lookup(&ext_variants, tag);
                quote! {
                    ::serde::Serializer::serialize_newtype_variant(
                        __serializer,
                        #target_name,
                        #ext_idx_expr,
                        #ext_variant_name,
                        #wire_ref,
                    )
                }
            } else {
                content_field_opt.map_or_else(
                    || {
                        quote! {
                            ::backwards_compat::__private::serialize_internally_tagged(
                                __serializer,
                                #target_name,
                                #tag_field,
                                &#tag_lit,
                                #wire_ref,
                            )
                        }
                    },
                    |content_field| {
                        quote! {
                            ::backwards_compat::__private::serialize_adjacently_tagged(
                                __serializer,
                                #target_name,
                                #tag_field,
                                &#tag_lit,
                                #content_field,
                                #wire_ref,
                            )
                        }
                    },
                )
            }
        };

        let resolve_owned_wire_ty = |v_ty: &syn::Type| -> syn::Result<TokenStream> {
            if v_ty == target_ty {
                match wire_mode {
                    SelfWireMode::Shadow(info) => {
                        let owned_ident = info.owned_ident;
                        Ok(quote!(#owned_ident #ty_generics))
                    }
                    SelfWireMode::Direct => Ok(quote!(#target_ty)),
                    SelfWireMode::Disallowed => Err(syn::Error::new_spanned(
                        &input.target_ty,
                        "using the domain struct as the active wire format is only supported when using the #[backwards_compat] attribute macro",
                    )),
                }
            } else {
                Ok(quote!(#v_ty))
            }
        };

        let (body, bound_ty) = match serializable {
            Some(SerializableMode::With(fn_path)) => {
                let wire_ty = resolve_owned_wire_ty(ty)?;
                let ser_call = emit_ser_call(quote!(&__wire_val));
                (
                    quote! {
                        let __wire_val: #wire_ty = (#fn_path)(self);
                        #ser_call
                    },
                    Some(wire_ty),
                )
            }
            Some(SerializableMode::TryWith(fn_path)) => {
                let wire_ty = resolve_owned_wire_ty(ty)?;
                let ser_call = emit_ser_call(quote!(&__wire_val));
                (
                    quote! {
                        let __wire_val: #wire_ty = (#fn_path)(self).map_err(::serde::ser::Error::custom)?;
                        #ser_call
                    },
                    Some(wire_ty),
                )
            }
            Some(SerializableMode::Infallible) | None => {
                if is_ser_ver && let Some(ref ser_fn) = input.serialize_with_fn {
                    let wire_ty = resolve_owned_wire_ty(ty)?;
                    let ser_call = emit_ser_call(quote!(&__wire_val));
                    (
                        quote! {
                            let __wire_val: #wire_ty = (#ser_fn)(self);
                            #ser_call
                        },
                        Some(wire_ty),
                    )
                } else if ty == target_ty {
                    match wire_mode {
                        SelfWireMode::Shadow(info) => {
                            if !is_adjacent && !input.external && info.is_unit_struct {
                                (emit_ser_call(quote!(&())), None)
                            } else {
                                let construct_borrowed = &info.construct_borrowed;
                                let ty_in_helper = &info.borrowed_ty_in_helper;
                                let ser_call = emit_ser_call(quote!(&__wire_val));
                                (
                                    quote! {
                                        let __wire_val = #construct_borrowed;
                                        #ser_call
                                    },
                                    Some(quote!(#ty_in_helper)),
                                )
                            }
                        }
                        SelfWireMode::Direct => (emit_ser_call(quote!(self)), Some(quote!(#target_ty))),
                        SelfWireMode::Disallowed => {
                            return Err(syn::Error::new_spanned(
                                &input.target_ty,
                                "using the domain struct as the active wire format is only supported when using the #[backwards_compat] attribute macro",
                            ));
                        }
                    }
                } else {
                    let wire_ty = quote!(#ty);
                    let ser_call = emit_ser_call(quote!(&__wire_val));
                    (
                        quote! {
                            #[allow(unused_imports)]
                            use ::backwards_compat::__private::{
                                BorrowedWireConvert as _,
                                FallbackWireConvert as _,
                                WireConvert as __WireConvert,
                            };
                            let __wire_val: #wire_ty = __WireConvert::<_, #wire_ty>::new(self).convert_wire();
                            #ser_call
                        },
                        Some(wire_ty),
                    )
                }
            }
        };

        Ok(SerializableArm {
            tag: tag.clone(),
            cfg,
            body,
            bound_ty,
        })
    };

    for (i, v) in input.versions.iter().enumerate() {
        if v.tag.is_untagged() {
            continue;
        }
        let is_ser_ver = v.tag.matches(&input.serialize_version);
        let is_cur_wire = dag_plan.target_is_wire && v.tag.matches(&input.current_version);
        if !(is_ser_ver || is_cur_wire || v.serializable.is_some()) {
            continue;
        }
        let cfg = combine_cfg_all(v.cfgs.iter().map(|c| &c.tokens));
        arms.push((
            i,
            build_arm(&v.tag, &v.ty, cfg, is_ser_ver, v.serializable.as_ref())?,
        ));
    }

    let has_declared_current = input
        .versions
        .iter()
        .any(|v| v.tag.matches(&input.current_version));
    if dag_plan.target_is_wire && !has_declared_current {
        let is_ser_ver = input.current_version.matches(&input.serialize_version);
        arms.push((
            input.versions.len(),
            build_arm(&input.current_version, target_ty, None, is_ser_ver, None)?,
        ));
    }

    let mut serializable_version_items = Vec::new();
    let mut match_arms = Vec::new();

    for (idx, arm) in arms {
        let cfg_tokens = cfg_attr(arm.cfg.as_ref());
        let ver_expr = version_mode.tag_expr(&arm.tag);
        serializable_version_items.push(quote!(#cfg_tokens #ver_expr));

        if is_generic && let Some(ref pred) = arm.cfg {
            let trait_ident = format_ident!("__BcSerArm_{}", idx);
            let mut arm_wc = where_clause.cloned().unwrap_or_else(|| syn::WhereClause {
                where_token: syn::token::Where::default(),
                predicates: syn::punctuated::Punctuated::new(),
            });
            if let Some(ref bty) = arm.bound_ty {
                arm_wc
                    .predicates
                    .push(syn::parse_quote!(for<'__a> #bty: ::serde::Serialize));
            }
            let arm_body = &arm.body;
            helper_defs_list.push(quote! {
                #[doc(hidden)]
                #[allow(non_camel_case_types, dead_code, reason = "internal cfg-gated serialization helper")]
                trait #trait_ident {
                    #[cfg(#pred)]
                    fn __bc_ser<__S>(
                        &self,
                        __serializer: __S,
                    ) -> ::core::result::Result<__S::Ok, __S::Error>
                    where
                        __S: ::serde::Serializer;
                }

                #[cfg(#pred)]
                impl #impl_generics #trait_ident for #target_ty #arm_wc {
                    fn __bc_ser<__S>(
                        &self,
                        __serializer: __S,
                    ) -> ::core::result::Result<__S::Ok, __S::Error>
                    where
                        __S: ::serde::Serializer,
                    {
                        #arm_body
                    }
                }

                #[cfg(not(#pred))]
                impl #impl_generics #trait_ident for #target_ty #where_clause {}
            });
            ser_as_wc
                .predicates
                .push(syn::parse_quote!(#target_ty: #trait_ident));
            match_arms.push(quote! {
                #cfg_tokens
                #ver_expr => <#target_ty as #trait_ident>::__bc_ser(self, __serializer),
            });
        } else {
            if arm.cfg.is_none()
                && let Some(ref bty) = arm.bound_ty
            {
                let key = bty.to_string();
                if seen_bound_tys.insert(key) {
                    ser_as_wc
                        .predicates
                        .push(syn::parse_quote!(for<'__a> #bty: ::serde::Serialize));
                }
            }
            let arm_body = &arm.body;
            match_arms.push(quote! {
                #cfg_tokens
                #ver_expr => {
                    #arm_body
                }
            });
        }
    }

    let ser_as_where_clause = non_empty_where_clause(ser_as_wc);

    Ok(quote! {
        #ext_idx_defs

        impl #impl_generics ::backwards_compat::BackwardsCompatSerialize for #target_ty #ser_as_where_clause {
            const SERIALIZABLE_VERSIONS: &'static [Self::Version] = &[#(#serializable_version_items,)*];

            fn serialize_as_version<__S>(
                &self,
                __version: Self::Version,
                __serializer: __S,
            ) -> ::core::result::Result<__S::Ok, __S::Error>
            where
                __S: ::serde::Serializer,
            {
                match __version {
                    #(#match_arms)*
                    _ => ::core::result::Result::Err(::serde::ser::Error::custom(
                        ::backwards_compat::__private::format!(
                            "version {__version} is not supported for serialization"
                        ),
                    )),
                }
            }
        }
    })
}

#[cfg(feature = "schemars")]
#[allow(
    clippy::too_many_arguments,
    reason = "shared JSON Schema generation across container and field-level helpers"
)]
pub fn generate_json_schema_body(
    input: &BackwardsCompatInput,
    dag_plan: &DagPlan,
    wire_mode: SelfWireMode<'_>,
    impl_self_ty: &syn::Type,
    impl_generics: &syn::ImplGenerics<'_>,
    orig_where_clause: Option<&syn::WhereClause>,
    schema_wc: &mut syn::WhereClause,
    helper_defs_list: &mut Vec<TokenStream>,
) -> syn::Result<TokenStream> {
    let target_ty = &input.target_ty;
    let tag_field = &input.tag_field;
    let content_field_opt = input.content_field.as_ref();
    let is_generic = input.generics.type_params().next().is_some();
    let (_, ty_generics, _) = input.generics.split_for_impl();

    let mut seen_schema_tys: HashSet<String> = HashSet::new();
    let mut push_stmts = Vec::new();

    let mut push_arm =
        |idx: usize, de_ty: TokenStream, cfg: Option<&TokenStream>, schema_expr: TokenStream| {
            let cfg_tokens = cfg_attr(cfg);
            if is_generic && let Some(pred) = cfg {
                let trait_ident = format_ident!("__BcSchemaArm_{}", idx);
                let mut arm_wc = orig_where_clause.cloned().unwrap_or_else(|| syn::WhereClause {
                    where_token: syn::token::Where::default(),
                    predicates: syn::punctuated::Punctuated::new(),
                });
                arm_wc.predicates.push(syn::parse_quote!(
                    #de_ty: ::backwards_compat::__private::schemars::JsonSchema
                ));
                helper_defs_list.push(quote! {
                    #[doc(hidden)]
                    #[allow(non_camel_case_types, dead_code, reason = "internal cfg-gated schemars helper")]
                    trait #trait_ident {
                        #[cfg(#pred)]
                        fn __bc_schema(
                            __gen: &mut ::backwards_compat::__private::schemars::SchemaGenerator,
                        ) -> ::backwards_compat::__private::schemars::Schema;
                    }

                    #[cfg(#pred)]
                    impl #impl_generics #trait_ident for #impl_self_ty #arm_wc {
                        fn __bc_schema(
                            __gen: &mut ::backwards_compat::__private::schemars::SchemaGenerator,
                        ) -> ::backwards_compat::__private::schemars::Schema {
                            #schema_expr
                        }
                    }

                    #[cfg(not(#pred))]
                    impl #impl_generics #trait_ident for #impl_self_ty #orig_where_clause {}
                });
                schema_wc
                    .predicates
                    .push(syn::parse_quote!(#impl_self_ty: #trait_ident));
                push_stmts.push(quote! {
                    #cfg_tokens
                    __schemas.push(<Self as #trait_ident>::__bc_schema(__gen));
                });
            } else {
                if is_generic && cfg.is_none() {
                    let key = de_ty.to_string();
                    if seen_schema_tys.insert(key) {
                        schema_wc.predicates.push(syn::parse_quote!(
                            #de_ty: ::backwards_compat::__private::schemars::JsonSchema
                        ));
                    }
                }
                push_stmts.push(quote! {
                    #cfg_tokens
                    __schemas.push(#schema_expr);
                });
            }
        };

    let build_tagged_expr = |de_ty: &TokenStream, tag: &VersionTag| -> TokenStream {
        if input.external {
            let variant_name = tag.to_tag_string();
            return quote! {
                ::backwards_compat::__private::schema_externally_tagged(
                    __gen.subschema_for::<#de_ty>(),
                    #variant_name,
                )
            };
        }
        let tag_schema_val = match tag {
            VersionTag::Int(n) => quote!(::backwards_compat::__private::TagSchemaValue::Int(#n)),
            VersionTag::String(s) => quote!(::backwards_compat::__private::TagSchemaValue::Str(#s)),
            VersionTag::Untagged => unreachable!("tagged arm cannot be untagged"),
        };
        content_field_opt.map_or_else(
            || {
                quote! {
                    ::backwards_compat::__private::schema_internally_tagged(
                        <#de_ty as ::backwards_compat::__private::schemars::JsonSchema>::json_schema(__gen),
                        #tag_field,
                        #tag_schema_val,
                    )
                }
            },
            |content_field| {
                quote! {
                    ::backwards_compat::__private::schema_adjacently_tagged(
                        __gen.subschema_for::<#de_ty>(),
                        #tag_field,
                        #content_field,
                        #tag_schema_val,
                    )
                }
            },
        )
    };

    for (i, v) in input.versions.iter().enumerate() {
        if v.tag.is_untagged() {
            continue;
        }
        let Some(path) = dag_plan.paths.get(i) else {
            continue;
        };
        let de_ty = resolve_de_ty(
            &v.ty,
            target_ty,
            &ty_generics,
            wire_mode,
            "using the domain struct as the active wire format is only supported when using the #[backwards_compat] attribute macro",
        )?;
        let expr = build_tagged_expr(&de_ty, &v.tag);
        push_arm(i, de_ty, path.cfg.as_ref(), expr);
    }

    let has_declared_current = input
        .versions
        .iter()
        .any(|v| v.tag.matches(&input.current_version));
    if dag_plan.target_is_wire && !has_declared_current {
        let de_ty = resolve_de_ty(
            target_ty,
            target_ty,
            &ty_generics,
            wire_mode,
            "using the domain struct as the active wire format is only supported when using the #[backwards_compat] attribute macro",
        )?;
        let expr = build_tagged_expr(&de_ty, &input.current_version);
        push_arm(input.versions.len(), de_ty, None, expr);
    }

    for (idx, untagged_v) in input.versions.iter().enumerate() {
        if !untagged_v.tag.is_untagged() {
            continue;
        }
        let path = dag_plan.paths.get(idx);
        let cfg = path.and_then(|p| p.cfg.as_ref());
        let de_ty = resolve_de_ty(
            &untagged_v.ty,
            target_ty,
            &ty_generics,
            wire_mode,
            "using the domain struct as the unversioned wire format is only supported when using the #[backwards_compat] attribute macro",
        )?;
        let expr = quote!(__gen.subschema_for::<#de_ty>());
        push_arm(idx, de_ty, cfg, expr);
    }

    Ok(quote! {
        let mut __schemas = ::backwards_compat::__private::Vec::new();
        #(#push_stmts)*
        ::backwards_compat::__private::schema_one_of(__schemas)
    })
}

#[cfg(feature = "schemars")]
pub fn build_schema_name_and_id(
    base_name: &str,
    generics: &syn::Generics,
    schema_wc: &mut syn::WhereClause,
) -> (TokenStream, TokenStream) {
    let mut name_fmt_parts = Vec::new();
    let mut name_args = Vec::new();
    let mut id_fmt_parts = Vec::new();
    let mut id_args = Vec::new();

    for param in &generics.params {
        match param {
            syn::GenericParam::Type(tp) => {
                let ident = &tp.ident;
                schema_wc.predicates.push(syn::parse_quote!(
                    #ident: ::backwards_compat::__private::schemars::JsonSchema
                ));
                name_fmt_parts.push("{}");
                name_args.push(quote!(
                    <#ident as ::backwards_compat::__private::schemars::JsonSchema>::schema_name()
                ));
                id_fmt_parts.push("{}");
                id_args.push(quote!(
                    <#ident as ::backwards_compat::__private::schemars::JsonSchema>::schema_id()
                ));
            }
            syn::GenericParam::Const(cp) => {
                let ident = &cp.ident;
                name_fmt_parts.push("{}");
                name_args.push(quote!(#ident));
                id_fmt_parts.push("{}");
                id_args.push(quote!(#ident));
            }
            syn::GenericParam::Lifetime(_) => {}
        }
    }

    if name_args.is_empty() {
        (
            quote! {
                ::backwards_compat::__private::schemars::_private::alloc::borrow::Cow::Borrowed(#base_name)
            },
            quote! {
                ::backwards_compat::__private::schemars::_private::alloc::borrow::Cow::Borrowed(
                    ::core::concat!(::core::module_path!(), "::", #base_name),
                )
            },
        )
    } else {
        let name_fmt = format!("{}_{}", base_name, name_fmt_parts.join("_"));
        let id_fmt = format!("{{}}::{}<{}>", base_name, id_fmt_parts.join(", "));
        (
            quote! {
                ::backwards_compat::__private::schemars::_private::alloc::borrow::Cow::Owned(
                    ::backwards_compat::__private::format!(#name_fmt, #(#name_args),*),
                )
            },
            quote! {
                ::backwards_compat::__private::schemars::_private::alloc::borrow::Cow::Owned(
                    ::backwards_compat::__private::format!(
                        #id_fmt,
                        ::core::module_path!(),
                        #(#id_args),*
                    ),
                )
            },
        )
    }
}

#[cfg(feature = "schemars")]
fn generate_json_schema_impl(
    input: &BackwardsCompatInput,
    dag_plan: &DagPlan,
    wire_mode: SelfWireMode<'_>,
    helper_defs_list: &mut Vec<TokenStream>,
) -> syn::Result<TokenStream> {
    let target_ty = &input.target_ty;
    let target_name = target_type_name(target_ty);
    let (impl_generics, _, where_clause) = input.generics.split_for_impl();

    let mut schema_wc = where_clause.cloned().unwrap_or_else(|| syn::WhereClause {
        where_token: syn::token::Where::default(),
        predicates: syn::punctuated::Punctuated::new(),
    });

    let (schema_name_body, schema_id_body) =
        build_schema_name_and_id(&target_name, &input.generics, &mut schema_wc);
    let json_schema_body = generate_json_schema_body(
        input,
        dag_plan,
        wire_mode,
        target_ty,
        &impl_generics,
        where_clause,
        &mut schema_wc,
        helper_defs_list,
    )?;

    let schema_where_clause = non_empty_where_clause(schema_wc);

    Ok(quote! {
        impl #impl_generics ::backwards_compat::__private::schemars::JsonSchema
            for #target_ty #schema_where_clause
        {
            fn schema_name() -> ::backwards_compat::__private::schemars::_private::alloc::borrow::Cow<'static, str> {
                #schema_name_body
            }

            fn schema_id() -> ::backwards_compat::__private::schemars::_private::alloc::borrow::Cow<'static, str> {
                #schema_id_body
            }

            fn json_schema(
                __gen: &mut ::backwards_compat::__private::schemars::SchemaGenerator,
            ) -> ::backwards_compat::__private::schemars::Schema {
                #json_schema_body
            }
        }
    })
}

pub fn generate_backwards_compat(
    input: &BackwardsCompatInput,
    dag_plan: &DagPlan,
    wire_mode: SelfWireMode<'_>,
    extra_items: Option<TokenStream>,
    derive_json_schema: bool,
) -> syn::Result<TokenStream> {
    #[cfg(not(feature = "schemars"))]
    let _ = derive_json_schema;

    let target_ty = &input.target_ty;
    let tag_field = &input.tag_field;
    let tagging_expr = if input.external {
        quote!(::backwards_compat::Tagging::External)
    } else if let Some(ref content) = input.content_field {
        quote!(::backwards_compat::Tagging::Adjacent {
            tag: #tag_field,
            content: #content,
        })
    } else {
        quote!(::backwards_compat::Tagging::Internal {
            tag: #tag_field,
        })
    };
    let version_mode = VersionMode::from_input(input);
    let (impl_generics, _, where_clause) = input.generics.split_for_impl();

    let de_generics = build_de_generics(&input.generics);
    let (de_impl_generics, _, _) = de_generics.split_for_impl();

    let ser_core = generate_serialize_core(input, dag_plan, wire_mode, &quote!(self))?;
    let de_core = generate_deserialize_core(input, dag_plan, wire_mode, true)?;

    let mut extra_helper_defs = Vec::new();
    let ser_as_impl = generate_backwards_compat_serialize(
        input,
        dag_plan,
        wire_mode,
        version_mode,
        &mut extra_helper_defs,
    )?;

    #[cfg(feature = "schemars")]
    let json_schema_impl = if derive_json_schema {
        generate_json_schema_impl(input, dag_plan, wire_mode, &mut extra_helper_defs)?
    } else {
        TokenStream::new()
    };
    #[cfg(not(feature = "schemars"))]
    let json_schema_impl = TokenStream::new();

    let mut supported_version_items = Vec::new();
    for (i, v) in input.versions.iter().enumerate() {
        if v.tag.is_untagged() {
            continue;
        }
        let Some(path) = dag_plan.paths.get(i) else {
            continue;
        };
        let cfg_tokens = cfg_attr(path.cfg.as_ref());
        let version_expr = version_mode.tag_expr(&v.tag);
        supported_version_items.push(quote! { #cfg_tokens #version_expr });
    }
    let has_declared_current = input
        .versions
        .iter()
        .any(|v| v.tag.matches(&input.current_version));
    if dag_plan.target_is_wire && !has_declared_current {
        let version_expr = version_mode.tag_expr(&input.current_version);
        supported_version_items.push(quote!(#version_expr));
    }

    let emit_de_pair = |cfg_prefix: TokenStream, where_cl: Option<&syn::WhereClause>, body: &TokenStream| {
        quote! {
            #cfg_prefix
            impl #de_impl_generics ::backwards_compat::BackwardsCompatDeserialize<'__de> for #target_ty #where_cl {
                fn deserialize_migrated<__D>(
                    __deserializer: __D,
                ) -> ::core::result::Result<::backwards_compat::Migrated<Self>, __D::Error>
                where
                    __D: ::serde::Deserializer<'__de>,
                {
                    #body
                }
            }

            #cfg_prefix
            impl #de_impl_generics ::serde::Deserialize<'__de> for #target_ty #where_cl {
                fn deserialize<__D>(__deserializer: __D) -> ::core::result::Result<Self, __D::Error>
                where
                    __D: ::serde::Deserializer<'__de>,
                {
                    <Self as ::backwards_compat::BackwardsCompatDeserialize<'__de>>::deserialize_migrated(__deserializer)
                        .map(|__m| __m.value)
                }
            }
        }
    };

    let (deserialize_impls, accepts_untagged_const) = if let Some(ref untagged) = de_core.untagged {
        untagged.overall_cfg.as_ref().map_or_else(
            || {
                (
                    emit_de_pair(TokenStream::new(), untagged.where_clause.as_ref(), &untagged.body),
                    quote!(
                        const ACCEPTS_UNTAGGED: bool = true;
                    ),
                )
            },
            |pred| {
                let enabled = emit_de_pair(
                    quote!(#[cfg(#pred)]),
                    untagged.where_clause.as_ref(),
                    &untagged.body,
                );
                let disabled = emit_de_pair(
                    quote!(#[cfg(not(#pred))]),
                    de_core.base_where_clause.as_ref(),
                    &de_core.base_body,
                );
                (
                    quote! {
                        #enabled
                        #disabled
                    },
                    quote! {
                        #[cfg(#pred)]
                        const ACCEPTS_UNTAGGED: bool = true;
                        #[cfg(not(#pred))]
                        const ACCEPTS_UNTAGGED: bool = false;
                    },
                )
            },
        )
    } else {
        (
            emit_de_pair(
                TokenStream::new(),
                de_core.base_where_clause.as_ref(),
                &de_core.base_body,
            ),
            quote!(
                const ACCEPTS_UNTAGGED: bool = false;
            ),
        )
    };

    let extra = extra_items.unwrap_or_default();
    let serialize_helper_def = ser_core.helper_def;
    let serialize_stmt = ser_core.body;
    let ser_where_clause = ser_core.where_clause;
    let de_helper_defs = de_core.helper_defs;

    let version_ty = version_mode.version_ty();
    let current_version_expr = version_mode.tag_expr(&input.current_version);
    let serialize_version_expr = version_mode.tag_expr(&input.serialize_version);

    let code = quote! {
        const _: () = {
            #extra

            #de_helper_defs

            #(#extra_helper_defs)*

            #serialize_helper_def

            impl #impl_generics ::backwards_compat::BackwardsCompat for #target_ty #where_clause {
                type Version = #version_ty;
                const CURRENT_VERSION: Self::Version = #current_version_expr;
                const SERIALIZE_VERSION: Self::Version = #serialize_version_expr;
                const SUPPORTED_VERSIONS: &'static [Self::Version] = &[#(#supported_version_items,)*];
                #accepts_untagged_const
                const TAGGING: ::backwards_compat::Tagging = #tagging_expr;
            }

            impl #impl_generics ::serde::Serialize for #target_ty #ser_where_clause {
                fn serialize<__S>(&self, __serializer: __S) -> ::core::result::Result<__S::Ok, __S::Error>
                where
                    __S: ::serde::Serializer,
                {
                    #serialize_stmt
                }
            }

            #ser_as_impl

            #deserialize_impls

            #json_schema_impl
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
    use crate::dag::resolve_dag;
    use crate::parse::BackwardsCompatArgs;

    fn parse_test_input(args_tokens: proc_macro2::TokenStream) -> BackwardsCompatInput {
        let args: BackwardsCompatArgs = syn::parse2(args_tokens).unwrap();
        let target_ty: syn::Type = syn::parse_quote!(TargetModel);
        let vis: syn::Visibility = syn::parse_quote!(pub);
        args.into_input(target_ty, vis, syn::Generics::default()).unwrap()
    }

    fn generate_for_test(
        input: &BackwardsCompatInput,
        wire_mode: SelfWireMode<'_>,
        extra_items: Option<TokenStream>,
    ) -> syn::Result<TokenStream> {
        let dag_plan = resolve_dag(input)?;
        generate_backwards_compat(input, &dag_plan, wire_mode, extra_items, false)
    }

    #[test]
    fn test_generate_backwards_compat_streaming_match_arms_no_helpers() {
        let input = parse_test_input(quote! {
            versions = [
                1: ModelV1 => 2,
                #[fallible]
                2: ModelV2 => 3,
                3: ModelV3,
            ]
        });

        let generated = generate_for_test(&input, SelfWireMode::Disallowed, None).unwrap();
        let rendered = generated.to_string();

        // No __VersionHelper, __UntaggedHelper, or VariantContentDeserializer
        assert!(!rendered.contains("__VersionHelper"));
        assert!(!rendered.contains("__UntaggedHelper"));
        assert!(!rendered.contains("VariantContentDeserializer"));

        // Uses streaming Visitor + generalized visit_internal_map
        assert!(rendered.contains("__deserializer . deserialize_any (__visitor)"));
        assert!(rendered.contains(
            "visit_internal_map :: < :: backwards_compat :: __private :: IntTagMode , Self , __A > (__map , \"version\" ,)"
        ));
        assert!(!rendered.contains("RemainingMapDeserializer :: new (__map)"));

        // Direct deserialize + non-generic tail-chained migration helpers
        assert!(rendered.contains("< ModelV1 as :: serde :: Deserialize < '__de >> :: deserialize (__d) ?"));
        assert!(rendered.contains("fn __bc_migrate_0 (__val : ModelV3) -> TargetModel"));
        assert!(rendered.contains(
            "fn __bc_migrate_1 (__val : ModelV2) -> :: core :: result :: Result < TargetModel , :: backwards_compat :: __private :: String >"
        ));
        assert!(rendered.contains(
            "fn __bc_migrate_2 (__val : ModelV1) -> :: core :: result :: Result < TargetModel , :: backwards_compat :: __private :: String >"
        ));
        assert!(rendered.contains("let __val : TargetModel = __bc_migrate_2 (__val) . map_err (:: serde :: de :: Error :: custom) ? ;"));
        assert!(rendered.contains("let __val : TargetModel = __bc_migrate_1 (__val) . map_err (:: serde :: de :: Error :: custom) ? ;"));
        assert!(rendered.contains("let __val : TargetModel = __bc_migrate_0 (__val) ;"));
        assert!(rendered.contains("< ModelV2 as :: serde :: Deserialize < '__de >> :: deserialize (__d) ?"));
        assert!(rendered.contains("< ModelV3 as :: serde :: Deserialize < '__de >> :: deserialize (__d) ?"));
        assert!(!rendered.contains("impl :: core :: convert :: From <"));
        assert!(!rendered.contains("impl :: core :: convert :: TryFrom <"));

        syn::parse2::<syn::File>(generated).unwrap();
    }

    #[test]
    fn test_serialize_helper_derives_and_no_version_helper() {
        let input = parse_test_input(quote! {
            versions = [
                1: ModelV1 => 2,
                2: ModelV2 => 3,
                3: ModelV3,
            ]
        });

        let generated = generate_for_test(&input, SelfWireMode::Disallowed, None).unwrap();
        let rendered = generated.to_string();

        assert!(!rendered.contains("# [derive (:: serde :: Deserialize)]"));
        assert!(!rendered.contains("# [derive (:: serde :: Serialize)]"));
        assert!(rendered.contains("serialize_internally_tagged"));
        assert!(!rendered.contains(":: std ::"));
    }

    #[test]
    fn test_generate_backwards_compat_target_is_wire_with_shadow() {
        let input = parse_test_input(quote! {
            versions = [
                1: ModelV1 => 2,
                2: ModelV2 => 3,
                3: Self,
            ]
        });

        let owned_shadow = format_ident!("TargetModelWire");
        let borrowed_shadow = format_ident!("TargetModelBorrowedWire");
        let shadow_info = ShadowWireInfo {
            owned_ident: &owned_shadow,
            borrowed_ident: &borrowed_shadow,
            borrowed_ty_in_helper: quote!(TargetModelBorrowedWire<'__a>),
            construct_borrowed: quote!(TargetModelBorrowedWire {}),
            is_unit_struct: false,
        };
        let generated = generate_for_test(&input, SelfWireMode::Shadow(&shadow_info), None).unwrap();
        let rendered = generated.to_string();

        assert!(
            rendered
                .contains("< TargetModelWire as :: serde :: Deserialize < '__de >> :: deserialize (__d) ?")
        );
        assert!(rendered.contains("let __val : TargetModel = :: core :: convert :: Into :: into (__val) ;"));
        assert!(rendered.contains("serialize_internally_tagged"));
        assert!(rendered.contains("for < '__a > TargetModelBorrowedWire < '__a > : :: serde :: Serialize"));
        syn::parse2::<syn::File>(generated).unwrap();
    }

    #[test]
    fn test_generate_backwards_compat_target_is_wire_direct_mode() {
        let input = parse_test_input(quote! {
            versions = [
                1: ModelV1 => 2,
                2: Self,
            ]
        });
        let dag_plan = resolve_dag(&input).unwrap();

        let ser = generate_serialize_core(&input, &dag_plan, SelfWireMode::Direct, &quote!(__value)).unwrap();
        let h = &ser.helper_def;
        let b = &ser.body;
        let ser_str = quote!(#h fn f() { #b }).to_string();
        assert!(ser_str.contains("serialize_internally_tagged"));
        assert!(ser_str.contains("__value"));

        let de = generate_deserialize_core(&input, &dag_plan, SelfWireMode::Direct, false).unwrap();
        let de_str = de.base_body.to_string();
        assert!(
            de_str.contains("< TargetModel as :: serde :: Deserialize < '__de >> :: deserialize (__d) ?")
        );
        assert!(!de_str.contains("Migrated"));
    }

    #[test]
    fn test_generate_backwards_compat_target_is_wire_without_shadow_errors() {
        let input = parse_test_input(quote! {
            versions = [
                1: ModelV1 => 2,
                2: ModelV2 => 3,
                3: Self,
            ]
        });

        let result = generate_for_test(&input, SelfWireMode::Disallowed, None);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "using the domain struct as the active wire format is only supported when using the #[backwards_compat] attribute macro"
        );
    }

    #[test]
    fn test_generate_backwards_compat_generics_and_where_clause() {
        let args: BackwardsCompatArgs = syn::parse2(quote! {
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

        let generated = generate_for_test(&input, SelfWireMode::Disallowed, None).unwrap();
        let rendered = generated.to_string();

        assert!(!rendered.contains("__VersionHelper"));
        assert!(rendered.contains(
            "impl < T : Clone + 'static > :: serde :: Serialize for GenericModel < T > where T : std :: fmt :: Debug , for < '__a > GenericModelV2 < T > : :: serde :: Serialize"
        ));
        assert!(rendered.contains(
            "impl < T : Clone + 'static > :: serde :: Serialize for GenericModel < T > where T : std :: fmt :: Debug"
        ));
        assert!(rendered.contains(
            "impl < '__de , T : Clone + 'static > :: serde :: Deserialize < '__de > for GenericModel < T > where T : std :: fmt :: Debug , GenericModelV1 < T > : :: serde :: Deserialize < '__de > , GenericModelV2 < T > : :: serde :: Deserialize < '__de >"
        ));
        syn::parse2::<syn::File>(generated).unwrap();
    }

    #[test]
    fn test_generate_backwards_compat_generic_cfg_gated_arm_trait() {
        let args: BackwardsCompatArgs = syn::parse2(quote! {
            versions = [
                #[cfg(feature = "v1")]
                1: GenericModelV1<T> => 2,
                2: GenericModelV2<T>,
            ]
        })
        .unwrap();
        let target_ty: syn::Type = syn::parse_quote!(GenericModel<T>);
        let vis: syn::Visibility = syn::parse_quote!(pub);
        let generics: syn::Generics = syn::parse_quote!(<T>);
        let input = args.into_input(target_ty, vis, generics).unwrap();

        let generated = generate_for_test(&input, SelfWireMode::Disallowed, None).unwrap();
        let rendered = generated.to_string();

        assert!(rendered.contains("trait __BcArm_0 < '__de >"));
        assert!(rendered.contains(
            "# [cfg (feature = \"v1\")] impl < '__de , T > __BcArm_0 < '__de > for GenericModel < T > where GenericModelV1 < T > : :: serde :: Deserialize < '__de >"
        ));
        assert!(rendered.contains(
            "# [cfg (not (feature = \"v1\"))] impl < '__de , T > __BcArm_0 < '__de > for GenericModel < T > { }"
        ));
        assert!(rendered.contains("GenericModel < T > : __BcArm_0 < '__de >"));
        syn::parse2::<syn::File>(generated).unwrap();
    }

    #[test]
    fn test_generate_backwards_compat_adjacent_tagging() {
        let args: BackwardsCompatArgs = syn::parse2(quote! {
            tag = "type",
            content = "data",
            versions = [
                1: ModelV1 => 2,
                2: ModelV2,
            ]
        })
        .unwrap();
        let target_ty: syn::Type = syn::parse_quote!(TargetModel);
        let vis: syn::Visibility = syn::parse_quote!(pub);
        let input = args.into_input(target_ty, vis, syn::Generics::default()).unwrap();

        let generated = generate_for_test(&input, SelfWireMode::Disallowed, None).unwrap();
        let rendered = generated.to_string();

        assert!(
            rendered
                .contains("__deserializer . deserialize_struct (\"\" , & [\"type\" , \"data\"] , __visitor)")
        );
        assert!(rendered.contains(
            "visit_adjacent_map :: < :: backwards_compat :: __private :: IntTagMode , Self , __A > (__map , \"type\" , \"data\" ,)"
        ));
        assert!(rendered.contains("read_adjacent_content :: < __M , ModelV1 >"));
        assert!(rendered.contains("fn visit_seq < __A >"));
        assert!(rendered.contains("serialize_adjacently_tagged"));
        assert!(!rendered.contains("struct __SerializeHelper"));
        assert!(!rendered.contains("struct __BcAdjSerHelper"));
        syn::parse2::<syn::File>(generated).unwrap();
    }

    #[test]
    fn test_generate_backwards_compat_explicit_wire_autoref() {
        let args: BackwardsCompatArgs = syn::parse2(quote! {
            versions = [
                1: ModelV1 => 2,
                2: ModelV2,
            ]
        })
        .unwrap();
        let target_ty: syn::Type = syn::parse_quote!(TargetModel);
        let vis: syn::Visibility = syn::parse_quote!(pub);
        let input = args.into_input(target_ty, vis, syn::Generics::default()).unwrap();

        let generated = generate_for_test(&input, SelfWireMode::Disallowed, None).unwrap();
        let rendered = generated.to_string();

        assert!(rendered.contains("use :: backwards_compat :: __private :: { BorrowedWireConvert as _ , FallbackWireConvert as _ , WireConvert as __WireConvert , } ;"));
        assert!(rendered.contains(
            "let latest : ModelV2 = __WireConvert :: < _ , ModelV2 > :: new (self) . convert_wire () ;"
        ));
    }

    #[test]
    fn test_generate_backwards_compat_cfg_on_match_arms() {
        let input = parse_test_input(quote! {
            versions = [
                #[cfg(feature = "v1")]
                1: ModelV1 => 2,
                #[cfg(feature = "v2")]
                #[fallible]
                2: ModelV2 => 3,
                3: ModelV3,
            ]
        });

        let generated = generate_for_test(&input, SelfWireMode::Disallowed, None).unwrap();
        let rendered = generated.to_string();

        assert!(!rendered.contains("__VersionHelper"));
        assert!(rendered.contains("# [cfg (all (feature = \"v1\" , feature = \"v2\"))] 1u64 =>"));
        assert!(rendered.contains("# [cfg (feature = \"v2\")] 2u64 =>"));
        assert!(rendered.contains("3u64 =>"));

        syn::parse2::<syn::File>(generated).unwrap();
    }

    #[test]
    fn test_generate_backwards_compat_gated_untagged_explicit_type() {
        let input = parse_test_input(quote! {
            versions = [
                #[cfg(test)]
                _: ModelV0 => 1,
                1: ModelV1,
            ]
        });

        let generated = generate_for_test(&input, SelfWireMode::Disallowed, None).unwrap();
        let rendered = generated.to_string();

        assert!(!rendered.contains("__UntaggedHelper"));
        assert!(!rendered.contains("__VersionHelper"));
        assert!(
            rendered.contains(
                "# [cfg (test)] impl < '__de > :: serde :: Deserialize < '__de > for TargetModel {"
            )
        );
        assert!(rendered.contains(
            "# [cfg (not (test))] impl < '__de > :: serde :: Deserialize < '__de > for TargetModel {"
        ));
        assert!(rendered.contains("# [cfg (test)] const ACCEPTS_UNTAGGED : bool = true ;"));
        assert!(rendered.contains("# [cfg (not (test))] const ACCEPTS_UNTAGGED : bool = false ;"));
        // Single cfg-gated untagged arm uses ContentRefDeserializer (never moves __content)
        assert!(rendered.contains("ContentRefDeserializer :: < __E > :: new (& __content)"));

        syn::parse2::<syn::File>(generated).unwrap();
    }

    #[test]
    fn test_generate_backwards_compat_gated_untagged_self_with_shadow() {
        let input = parse_test_input(quote! {
            versions = [
                #[cfg(test)]
                _: Self,
                1: ModelV1 => 2,
                2: Self,
            ]
        });

        let owned_shadow = format_ident!("TargetModelWire");
        let borrowed_shadow = format_ident!("TargetModelBorrowedWire");
        let shadow_info = ShadowWireInfo {
            owned_ident: &owned_shadow,
            borrowed_ident: &borrowed_shadow,
            borrowed_ty_in_helper: quote!(TargetModelBorrowedWire<'__a>),
            construct_borrowed: quote!(TargetModelBorrowedWire {}),
            is_unit_struct: false,
        };

        let generated = generate_for_test(&input, SelfWireMode::Shadow(&shadow_info), None).unwrap();
        let rendered = generated.to_string();

        assert!(rendered.contains(
            "< TargetModelWire as :: serde :: Deserialize < '__de >> :: deserialize (:: backwards_compat :: __private :: ContentRefDeserializer :: < __E > :: new (& __content))"
        ));
        assert!(
            rendered.contains(
                "# [cfg (test)] impl < '__de > :: serde :: Deserialize < '__de > for TargetModel {"
            )
        );
        assert!(rendered.contains(
            "# [cfg (not (test))] impl < '__de > :: serde :: Deserialize < '__de > for TargetModel {"
        ));

        syn::parse2::<syn::File>(generated).unwrap();
    }

    #[test]
    fn test_generate_backwards_compat_single_unconditional_untagged_zero_buffer() {
        let input = parse_test_input(quote! {
            versions = [
                _: LegacyV0 => 1,
                1: ModelV1,
            ]
        });

        let generated = generate_for_test(&input, SelfWireMode::Disallowed, None).unwrap();
        let rendered = generated.to_string();

        assert!(
            rendered.contains(
                "__deserializer . deserialize_any (:: backwards_compat :: __private :: UntaggedSingleVisitor (__visitor) ,)"
            )
        );
        assert!(rendered.contains("UntaggedSingleDispatch < '__de > for __BcVisitor"));
        assert!(rendered.contains("fn dispatch_de"));
        assert!(!rendered.contains("UntaggedMultiDispatch"));
        assert!(!rendered.contains("fn dispatch_content"));
        assert!(rendered.contains("visit_internal_map_untagged"));
        assert!(rendered.contains("IntTagMode"));
        assert!(!rendered.contains(":: std ::"));
        syn::parse2::<syn::File>(generated).unwrap();

        let str_untagged = parse_test_input(quote! {
            versions = [
                _: LegacyV0 => "v1",
                "v1": ModelV1,
            ]
        });
        let str_rendered = generate_for_test(&str_untagged, SelfWireMode::Disallowed, None)
            .unwrap()
            .to_string();
        assert!(str_rendered.contains("visit_internal_map_untagged"));
        assert!(str_rendered.contains("StrTagMode"));
    }

    #[test]
    fn test_generate_backwards_compat_version_modes() {
        // All-int -> u64
        let int_input = parse_test_input(quote! {
            versions = [
                1: ModelV1 => 2,
                2: ModelV2,
            ]
        });
        let int_gen = generate_for_test(&int_input, SelfWireMode::Disallowed, None).unwrap();
        let int_rendered = int_gen.to_string();
        assert!(int_rendered.contains("type Version = u64 ;"));
        assert!(int_rendered.contains("const CURRENT_VERSION : Self :: Version = 2u64 ;"));
        assert!(int_rendered.contains("const SERIALIZE_VERSION : Self :: Version = 2u64 ;"));
        assert!(
            int_rendered
                .contains("const SUPPORTED_VERSIONS : & 'static [Self :: Version] = & [1u64 , 2u64 ,] ;")
        );
        syn::parse2::<syn::File>(int_gen).unwrap();

        // All-str -> &'static str
        let str_input = parse_test_input(quote! {
            versions = [
                "v1": ModelV1 => "v2",
                "v2": ModelV2,
            ]
        });
        let str_gen = generate_for_test(&str_input, SelfWireMode::Disallowed, None).unwrap();
        let str_rendered = str_gen.to_string();
        assert!(str_rendered.contains("type Version = & 'static str ;"));
        assert!(str_rendered.contains("const CURRENT_VERSION : Self :: Version = \"v2\" ;"));
        assert!(str_rendered.contains("const SERIALIZE_VERSION : Self :: Version = \"v2\" ;"));
        assert!(str_rendered.contains("StrTagMode"));
        syn::parse2::<syn::File>(str_gen).unwrap();
    }

    #[test]
    fn test_generate_backwards_compat_serialize_helper_variants() {
        // Internal unit struct uses serialize_unit_tagged without generating __SerializeHelper
        let unit_input = parse_test_input(quote! {
            versions = [
                1: Self,
            ]
        });
        let owned_shadow = format_ident!("TargetModelWire");
        let borrowed_shadow = format_ident!("TargetModelBorrowedWire");
        let unit_shadow = ShadowWireInfo {
            owned_ident: &owned_shadow,
            borrowed_ident: &borrowed_shadow,
            borrowed_ty_in_helper: quote!(TargetModelBorrowedWire),
            construct_borrowed: quote!(TargetModelBorrowedWire),
            is_unit_struct: true,
        };
        let unit_gen = generate_for_test(&unit_input, SelfWireMode::Shadow(&unit_shadow), None).unwrap();
        let unit_rendered = unit_gen.to_string();
        assert!(unit_rendered.contains("serialize_unit_tagged"));
        assert!(!unit_rendered.contains("struct __SerializeHelper"));
        assert!(!unit_rendered.contains("__content :"));
        syn::parse2::<syn::File>(unit_gen).unwrap();

        // Adjacent tagging uses serialize_adjacently_tagged without generating __SerializeHelper
        let adj_input = parse_test_input(quote! {
            tag = "ver",
            content = "data",
            versions = [
                1: ModelV1 => 2,
                2: ModelV2,
            ]
        });
        let adj_gen = generate_for_test(&adj_input, SelfWireMode::Disallowed, None).unwrap();
        let adj_rendered = adj_gen.to_string();
        assert!(adj_rendered.contains("serialize_adjacently_tagged"));
        assert!(adj_rendered.contains("\"data\""));
        assert!(!adj_rendered.contains("struct __SerializeHelper"));
        assert!(!adj_rendered.contains("struct __BcAdjSerHelper"));
        syn::parse2::<syn::File>(adj_gen).unwrap();

        // #[serialize] on older version
        let ser_old_input = parse_test_input(quote! {
            versions = [
                #[serialize]
                1: ModelV1 => 2,
                #[current]
                2: ModelV2,
            ]
        });
        let ser_old_gen = generate_for_test(&ser_old_input, SelfWireMode::Disallowed, None).unwrap();
        let ser_old_rendered = ser_old_gen.to_string();
        assert!(ser_old_rendered.contains("const CURRENT_VERSION : Self :: Version = 2u64 ;"));
        assert!(ser_old_rendered.contains("const SERIALIZE_VERSION : Self :: Version = 1u64 ;"));
        assert!(ser_old_rendered.contains("serialize_internally_tagged"));
        assert!(ser_old_rendered.contains("& 1u64"));
        syn::parse2::<syn::File>(ser_old_gen).unwrap();

        // #[serialize(with = to_v1)] on older version
        let ser_with_input = parse_test_input(quote! {
            versions = [
                #[serialize(with = to_v1)]
                1: ModelV1 => 2,
                #[current]
                2: ModelV2,
            ]
        });
        let ser_with_gen = generate_for_test(&ser_with_input, SelfWireMode::Disallowed, None).unwrap();
        let ser_with_rendered = ser_with_gen.to_string();
        assert!(ser_with_rendered.contains("let latest : ModelV1 = (to_v1) (self) ;"));
        syn::parse2::<syn::File>(ser_with_gen).unwrap();
    }

    #[test]
    fn test_generate_backwards_compat_one_step_with_fn_in_match_arm() {
        let owned_shadow = format_ident!("TargetModelWire");
        let borrowed_shadow = format_ident!("TargetModelBorrowedWire");
        let shadow_info = ShadowWireInfo {
            owned_ident: &owned_shadow,
            borrowed_ident: &borrowed_shadow,
            borrowed_ty_in_helper: quote!(TargetModelBorrowedWire<'__a>),
            construct_borrowed: quote!(TargetModelBorrowedWire {}),
            is_unit_struct: false,
        };

        // Infallible 1-step #[with(convert_v1)]
        let infallible_input = parse_test_input(quote! {
            versions = [
                #[with(convert_v1)]
                1: ModelV1 => 2,
                2: Self,
            ]
        });
        let gen1 = generate_for_test(&infallible_input, SelfWireMode::Shadow(&shadow_info), None).unwrap();
        let rendered1 = gen1.to_string();
        assert!(
            rendered1.contains("fn __bc_migrate_0 (__val : ModelV1) -> TargetModel { (convert_v1) (__val) }")
        );
        assert!(rendered1.contains("let __val : TargetModel = __bc_migrate_0 (__val) ;"));
        syn::parse2::<syn::File>(gen1).unwrap();

        // Fallible 1-step #[fallible] #[with(try_convert_v1)]
        let fallible_input = parse_test_input(quote! {
            versions = [
                #[fallible]
                #[with(try_convert_v1)]
                1: ModelV1 => 2,
                2: Self,
            ]
        });
        let gen2 = generate_for_test(&fallible_input, SelfWireMode::Shadow(&shadow_info), None).unwrap();
        let rendered2 = gen2.to_string();
        assert!(rendered2.contains(
            "fn __bc_migrate_0 (__val : ModelV1) -> :: core :: result :: Result < TargetModel , :: backwards_compat :: __private :: String >"
        ));
        assert!(rendered2.contains("(try_convert_v1) (__val) . map_err"));
        assert!(rendered2.contains(
            "let __val : TargetModel = __bc_migrate_0 (__val) . map_err (:: serde :: de :: Error :: custom) ? ;"
        ));
        syn::parse2::<syn::File>(gen2).unwrap();
    }

    #[test]
    fn test_migration_helpers_tail_share_and_skip_unused_base_body() {
        let input = parse_test_input(quote! {
            versions = [
                _: ModelV1 => 1,
                1: ModelV1 => 3,
                2: ModelV2 => 3,
                3: ModelV3,
            ]
        });
        let dag_plan = resolve_dag(&input).unwrap();
        let de_core = generate_deserialize_core(&input, &dag_plan, SelfWireMode::Disallowed, true).unwrap();

        // Unconditional untagged skips building unused base_body.
        assert!(de_core.base_body.is_empty());
        assert!(de_core.untagged.is_some());

        // Tail-sharing: V1->V3->Target, V2->V3->Target, V3->Target, and untagged V1->V3->Target
        // emit exactly 3 helpers (__bc_migrate_0 for V3, __bc_migrate_1 for V1, __bc_migrate_2 for V2).
        let helpers_str = de_core.helper_defs.to_string();
        assert!(helpers_str.contains("fn __bc_migrate_0 (__val : ModelV3) -> TargetModel"));
        assert!(helpers_str.contains("fn __bc_migrate_1 (__val : ModelV1) -> TargetModel"));
        assert!(helpers_str.contains("fn __bc_migrate_2 (__val : ModelV2) -> TargetModel"));
        assert!(!helpers_str.contains("__bc_migrate_3"));
    }

    #[test]
    fn test_generate_backwards_compat_multiple_untagged_entries() {
        // Mixed gated + unconditional -> first uses ContentRefDeserializer, last uses owned ContentDeserializer
        let mixed_input = parse_test_input(quote! {
            versions = [
                #[cfg(test)]
                _: LegacyA => 1,
                _: LegacyB => 1,
                1: ModelV1,
            ]
        });
        let mixed_gen = generate_for_test(&mixed_input, SelfWireMode::Disallowed, None).unwrap();
        let mixed_rendered = mixed_gen.to_string();
        assert!(!mixed_rendered.contains("__UntaggedHelper"));
        assert!(mixed_rendered.contains(
            "# [cfg (test)] if let :: core :: result :: Result :: Ok (__val) = < LegacyA as :: serde :: Deserialize < '__de >> :: deserialize (:: backwards_compat :: __private :: ContentRefDeserializer :: < __E > :: new (& __content))"
        ));
        assert!(mixed_rendered.contains(
            "if let :: core :: result :: Result :: Ok (__val) = < LegacyB as :: serde :: Deserialize < '__de >> :: deserialize (:: backwards_compat :: __private :: ContentDeserializer :: < __E > :: new (__content))"
        ));
        assert!(mixed_rendered.contains("const ACCEPTS_UNTAGGED : bool = true ;"));
        syn::parse2::<syn::File>(mixed_gen).unwrap();

        // Multiple gated with different predicates -> combined with any(p1, p2) and both use ContentRefDeserializer
        let gated_input = parse_test_input(quote! {
            versions = [
                #[cfg(feature = "a")]
                _: LegacyA => 1,
                #[cfg(feature = "b")]
                _: LegacyB => 1,
                1: ModelV1,
            ]
        });
        let gated_gen = generate_for_test(&gated_input, SelfWireMode::Disallowed, None).unwrap();
        let gated_rendered = gated_gen.to_string();
        assert!(gated_rendered.contains(
            "# [cfg (any (feature = \"a\" , feature = \"b\"))] impl < '__de > :: serde :: Deserialize < '__de > for TargetModel"
        ));
        assert!(gated_rendered.contains(
            "# [cfg (not (any (feature = \"a\" , feature = \"b\")))] impl < '__de > :: serde :: Deserialize < '__de > for TargetModel"
        ));
        assert!(gated_rendered.contains(
            "# [cfg (feature = \"b\")] if let :: core :: result :: Result :: Ok (__val) = < LegacyB as :: serde :: Deserialize < '__de >> :: deserialize (:: backwards_compat :: __private :: ContentRefDeserializer :: < __E > :: new (& __content))"
        ));
        syn::parse2::<syn::File>(gated_gen).unwrap();
    }

    #[test]
    fn test_generate_backwards_compat_explicit_self_uses_shadow_without_duplicate() {
        let input = parse_test_input(quote! {
            versions = [
                1: ModelV1,
                2: Self,
            ]
        });

        let owned_shadow = format_ident!("TargetModelWire");
        let borrowed_shadow = format_ident!("TargetModelBorrowedWire");
        let shadow_info = ShadowWireInfo {
            owned_ident: &owned_shadow,
            borrowed_ident: &borrowed_shadow,
            borrowed_ty_in_helper: quote!(TargetModelBorrowedWire<'__a>),
            construct_borrowed: quote!(TargetModelBorrowedWire {}),
            is_unit_struct: false,
        };

        let generated = generate_for_test(&input, SelfWireMode::Shadow(&shadow_info), None).unwrap();
        let rendered = generated.to_string();

        assert!(
            rendered
                .contains("< TargetModelWire as :: serde :: Deserialize < '__de >> :: deserialize (__d) ?")
        );
        assert!(!rendered.contains("< TargetModel as :: serde :: Deserialize"));
        syn::parse2::<syn::File>(generated).unwrap();
    }

    #[test]
    fn test_generate_backwards_compat_dump_string_shadow_and_missing_shadow_guards() {
        // dump: true
        let dump_input = parse_test_input(quote! {
            dump,
            versions = [
                1: ModelV1,
                2: ModelV2,
            ]
        });
        let dump_gen = generate_for_test(&dump_input, SelfWireMode::Disallowed, None).unwrap();
        syn::parse2::<syn::File>(dump_gen).unwrap();

        // String current_version with shadow wire
        let str_shadow_input = parse_test_input(quote! {
            versions = [
                "1.0": ModelV1,
                "2.0": Self,
            ]
        });
        let owned_shadow = format_ident!("TargetModelWire");
        let borrowed_shadow = format_ident!("TargetModelBorrowedWire");
        let shadow_info = ShadowWireInfo {
            owned_ident: &owned_shadow,
            borrowed_ident: &borrowed_shadow,
            borrowed_ty_in_helper: quote!(TargetModelBorrowedWire<'__a>),
            construct_borrowed: quote!(TargetModelBorrowedWire {}),
            is_unit_struct: false,
        };
        let str_gen = generate_for_test(&str_shadow_input, SelfWireMode::Shadow(&shadow_info), None).unwrap();
        let str_rendered = str_gen.to_string();
        assert!(str_rendered.contains("const CURRENT_VERSION : Self :: Version = \"2.0\" ;"));
        assert!(
            str_rendered
                .contains("< TargetModelWire as :: serde :: Deserialize < '__de >> :: deserialize (__d) ?")
        );
        syn::parse2::<syn::File>(str_gen).unwrap();

        // Untagged Self without shadow_info -> error
        let untagged_self_input = parse_test_input(quote! {
            versions = [
                _: Self,
                1: ModelV1,
            ]
        });
        let err_untagged =
            generate_for_test(&untagged_self_input, SelfWireMode::Disallowed, None).unwrap_err();
        assert!(
            err_untagged
                .to_string()
                .contains("using the domain struct as the unversioned wire format")
        );

        // serialize_with_fn + serialize_target_is_wire without shadow_info -> error
        let ser_with_self_input = parse_test_input(quote! {
            versions = [
                #[serialize(with = my_ser)] 1: Self,
                #[current] 2: ModelV2,
            ]
        });
        let err_ser_with =
            generate_for_test(&ser_with_self_input, SelfWireMode::Disallowed, None).unwrap_err();
        assert!(
            err_ser_with
                .to_string()
                .contains("using the domain struct as the active wire format")
        );

        // Direct serialize_with_fn + serialize_target_is_wire guard (when version arm is non-Self)
        let mut ser_with_guard_input = parse_test_input(quote! {
            versions = [
                1: ModelV1,
                2: ModelV2,
            ]
        });
        ser_with_guard_input.serialize_version = crate::parse::VersionTag::Int(99);
        ser_with_guard_input.serialize_with_fn = Some(syn::parse_quote!(my_ser));
        let err_ser_with_guard =
            generate_for_test(&ser_with_guard_input, SelfWireMode::Disallowed, None).unwrap_err();
        assert!(
            err_ser_with_guard
                .to_string()
                .contains("using the domain struct as the active wire format")
        );

        // Direct serialize_target_is_wire (without serialize_with_fn) guard
        let mut ser_bare_guard_input = parse_test_input(quote! {
            versions = [
                1: ModelV1,
                2: ModelV2,
            ]
        });
        ser_bare_guard_input.serialize_version = crate::parse::VersionTag::Int(99);
        let err_ser_bare_guard =
            generate_for_test(&ser_bare_guard_input, SelfWireMode::Disallowed, None).unwrap_err();
        assert!(
            err_ser_bare_guard
                .to_string()
                .contains("using the domain struct as the active wire format")
        );
    }

    #[test]
    fn test_generate_backwards_compat_tagging_consts_and_version_matching() {
        // Int internal mode
        let int_input = parse_test_input(quote! {
            tag = "v",
            versions = [1: V1, 2: V2]
        });
        let int_gen = generate_for_test(&int_input, SelfWireMode::Disallowed, None).unwrap();
        let int_str = int_gen.to_string();
        assert!(int_str.contains(
            "const TAGGING : :: backwards_compat :: Tagging = :: backwards_compat :: Tagging :: Internal { tag : \"v\" , } ;"
        ));
        assert!(int_str.contains(
            "visit_internal_map :: < :: backwards_compat :: __private :: IntTagMode , Self , __A > (__map , \"v\" ,)"
        ));
        assert!(int_str.contains("match __tag { 1u64 =>"));

        // Str mode with adjacent content
        let str_input = parse_test_input(quote! {
            tag = "ver",
            content = "payload",
            versions = ["a": V1, "b": V2]
        });
        let str_gen = generate_for_test(&str_input, SelfWireMode::Disallowed, None).unwrap();
        let str_str = str_gen.to_string();
        assert!(str_str.contains(
            "const TAGGING : :: backwards_compat :: Tagging = :: backwards_compat :: Tagging :: Adjacent { tag : \"ver\" , content : \"payload\" , } ;"
        ));
        assert!(str_str.contains(
            "visit_adjacent_map :: < :: backwards_compat :: __private :: StrTagMode , Self , __A > (__map , \"ver\" , \"payload\" ,)"
        ));
        assert!(str_str.contains(
            "__seq . next_element :: < :: backwards_compat :: __private :: StrTag < '__de > > () ?"
        ));
        assert!(str_str.contains("match __tag { \"a\" =>"));

        // External mode
        let ext_input = parse_test_input(quote! {
            external,
            versions = [
                #[serializable] 1 | 10: V1 => 3,
                #[cfg(feature = "v2")] #[serializable] 2: V2 => 3,
                #[current] 3: V3,
            ]
        });
        let ext_gen = generate_for_test(&ext_input, SelfWireMode::Disallowed, None).unwrap();
        let ext_str = ext_gen.to_string();
        assert!(ext_str.contains(
            "const TAGGING : :: backwards_compat :: Tagging = :: backwards_compat :: Tagging :: External ;"
        ));
        assert!(ext_str.contains("serialize_newtype_variant"));
        assert!(ext_str.contains("deserialize_enum"));
        assert!(ext_str.contains("deserialize_identifier"));
        assert!(!ext_str.contains("deserialize_any"));
        assert!(ext_str.contains("const __VARIANTS : & 'static [& 'static str] = & [\"1\" , # [cfg (feature = \"v2\")] \"2\" , \"3\" ,] ;"));
        syn::parse2::<syn::File>(ext_gen).unwrap();
    }

    #[test]
    fn test_generate_backwards_compat_serialize_and_serializable_arms() {
        let owned_shadow = format_ident!("TargetModelWire");
        let borrowed_shadow = format_ident!("TargetModelBorrowedWire");
        let shadow_info = ShadowWireInfo {
            owned_ident: &owned_shadow,
            borrowed_ident: &borrowed_shadow,
            borrowed_ty_in_helper: quote!(TargetModelBorrowedWire<'__a>),
            construct_borrowed: quote!(TargetModelBorrowedWire {}),
            is_unit_struct: false,
        };

        let input = parse_test_input(quote! {
            versions = [
                #[serializable] 1: ModelV1 => 2,
                #[serializable(with = to_v2)] 2: ModelV2 => 3,
                #[serializable(try_with = try_to_v3)] 3: ModelV3 => 4,
                #[current] 4: Self,
            ]
        });
        let generated = generate_for_test(&input, SelfWireMode::Shadow(&shadow_info), None).unwrap();
        let rendered = generated.to_string();

        assert!(rendered.contains("impl :: backwards_compat :: BackwardsCompatSerialize for TargetModel"));
        assert!(rendered.contains(
            "const SERIALIZABLE_VERSIONS : & 'static [Self :: Version] = & [1u64 , 2u64 , 3u64 , 4u64 ,] ;"
        ));
        assert!(rendered.contains(
            "let __wire_val : ModelV1 = __WireConvert :: < _ , ModelV1 > :: new (self) . convert_wire () ;"
        ));
        assert!(rendered.contains("let __wire_val : ModelV2 = (to_v2) (self) ;"));
        assert!(rendered.contains(
            "let __wire_val : ModelV3 = (try_to_v3) (self) . map_err (:: serde :: ser :: Error :: custom) ? ;"
        ));
        assert!(rendered.contains("is not supported for serialization"));
        syn::parse2::<syn::File>(generated).unwrap();
    }

    #[test]
    fn test_generate_backwards_compat_serialize_generic_cfg_arm_trait() {
        let args: BackwardsCompatArgs = syn::parse2(quote! {
            versions = [
                #[cfg(feature = "v1")]
                #[serializable]
                1: GenericV1<T> => 2,
                #[current]
                2: GenericV2<T>,
            ]
        })
        .unwrap();
        let target_ty: syn::Type = syn::parse_quote!(GenericModel<T>);
        let vis: syn::Visibility = syn::parse_quote!(pub);
        let generics: syn::Generics = syn::parse_quote!(<T>);
        let input = args.into_input(target_ty, vis, generics).unwrap();

        let generated = generate_for_test(&input, SelfWireMode::Disallowed, None).unwrap();
        let rendered = generated.to_string();

        assert!(rendered.contains("trait __BcSerArm_0"));
        assert!(
            rendered.contains("# [cfg (feature = \"v1\")] impl < T > __BcSerArm_0 for GenericModel < T >")
        );
        assert!(
            rendered.contains(
                "# [cfg (not (feature = \"v1\"))] impl < T > __BcSerArm_0 for GenericModel < T > { }"
            )
        );
        assert!(rendered.contains("GenericModel < T > : __BcSerArm_0"));
        syn::parse2::<syn::File>(generated).unwrap();
    }

    #[cfg(feature = "schemars")]
    #[test]
    fn test_generate_backwards_compat_schemars_internal_and_adjacent() {
        let internal = parse_test_input(quote! {
            versions = [
                _: LegacyV0 => 1,
                1: ModelV1 => 2,
                #[current] 2: Self,
            ]
        });
        let owned_shadow = format_ident!("TargetModelWire");
        let borrowed_shadow = format_ident!("TargetModelBorrowedWire");
        let shadow_info = ShadowWireInfo {
            owned_ident: &owned_shadow,
            borrowed_ident: &borrowed_shadow,
            borrowed_ty_in_helper: quote!(TargetModelBorrowedWire<'__a>),
            construct_borrowed: quote!(TargetModelBorrowedWire {}),
            is_unit_struct: false,
        };
        let dag_plan = resolve_dag(&internal).unwrap();
        let generated = generate_backwards_compat(
            &internal,
            &dag_plan,
            SelfWireMode::Shadow(&shadow_info),
            None,
            true,
        )
        .unwrap();
        let rendered = generated.to_string();

        assert!(
            rendered
                .contains("impl :: backwards_compat :: __private :: schemars :: JsonSchema for TargetModel")
        );
        assert!(rendered.contains("schema_internally_tagged"));
        assert!(rendered.contains("TagSchemaValue :: Int (1u64)"));
        assert!(rendered.contains("TagSchemaValue :: Int (2u64)"));
        assert!(rendered.contains("schema_one_of"));
        syn::parse2::<syn::File>(generated).unwrap();

        let adj = parse_test_input(quote! {
            tag = "ver",
            content = "data",
            versions = [
                "v1": ModelV1,
                "v2": ModelV2,
            ]
        });
        let adj_plan = resolve_dag(&adj).unwrap();
        let adj_gen =
            generate_backwards_compat(&adj, &adj_plan, SelfWireMode::Disallowed, None, true).unwrap();
        let adj_rendered = adj_gen.to_string();
        assert!(adj_rendered.contains("schema_adjacently_tagged"));
        assert!(adj_rendered.contains("TagSchemaValue :: Str (\"v1\")"));
        syn::parse2::<syn::File>(adj_gen).unwrap();

        let ext = parse_test_input(quote! {
            external,
            versions = [
                1: ModelV1,
                2: ModelV2,
            ]
        });
        let ext_plan = resolve_dag(&ext).unwrap();
        let ext_gen =
            generate_backwards_compat(&ext, &ext_plan, SelfWireMode::Disallowed, None, true).unwrap();
        let ext_rendered = ext_gen.to_string();
        assert!(ext_rendered.contains("schema_externally_tagged"));
        assert!(ext_rendered.contains("\"1\""));
        assert!(ext_rendered.contains("\"2\""));
        syn::parse2::<syn::File>(ext_gen).unwrap();
    }

    #[test]
    fn test_generate_backwards_compat_external_generic_cfg_arm_trait() {
        let args: BackwardsCompatArgs = syn::parse2(quote! {
            external,
            versions = [
                #[cfg(feature = "v1")]
                #[serializable]
                "v1": GenericV1<T> => "v2",
                #[current]
                "v2": GenericV2<T>,
            ]
        })
        .unwrap();
        let target_ty: syn::Type = syn::parse_quote!(GenericModel<T>);
        let vis: syn::Visibility = syn::parse_quote!(pub);
        let generics: syn::Generics = syn::parse_quote!(<T>);
        let input = args.into_input(target_ty, vis, generics).unwrap();

        let generated = generate_for_test(&input, SelfWireMode::Disallowed, None).unwrap();
        let rendered = generated.to_string();

        assert!(rendered.contains("fn __bc_de_variant"));
        assert!(rendered.contains("trait __BcSerArm_0"));
        assert!(!rendered.contains("deserialize_any"));
        syn::parse2::<syn::File>(generated).unwrap();
    }
}
