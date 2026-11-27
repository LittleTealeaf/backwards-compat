mod de;
mod migrate;
#[cfg(feature = "schemars")]
mod schema;
mod ser;

#[cfg(test)]
mod tests;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::dag::DagPlan;
use crate::parse::{BackwardsCompatInput, LegacyAttrWarning, VersionTag};

pub use de::generate_deserialize_core;
#[cfg(feature = "schemars")]
pub use schema::{build_schema_name_and_id, generate_json_schema_body};
pub use ser::generate_serialize_core;

#[cfg(feature = "schemars")]
use schema::generate_json_schema_impl;
use ser::generate_backwards_compat_serialize;

// LEGACY (deprecated): remove in 5.0
/// Emits compile-time `#[deprecated]` warnings for each recorded legacy attribute usage.
pub fn emit_legacy_deprecation_warnings(
    warnings: &[LegacyAttrWarning],
    allow_attrs: &[syn::Attribute],
) -> TokenStream {
    let mut out = TokenStream::new();
    for (idx, w) in warnings.iter().enumerate() {
        let fn_ident = format_ident!("__bc_deprecated_legacy_attr_{idx}", span = w.span);
        let call_ident = format_ident!("__bc_deprecated_legacy_attr_{idx}", span = w.span);
        let note = w.note;
        out.extend(quote! {
            #[deprecated(note = #note)]
            const fn #fn_ident() {}
            #(#allow_attrs)*
            const _: () = #call_ident();
        });
    }
    out
}

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

    let mut extra_helper_defs = Vec::new();
    let (serialize_helper_def, serialize_impl, ser_as_impl) = if input.serialize {
        let ser_core = generate_serialize_core(input, dag_plan, wire_mode, &quote!(self))?;
        let ser_as_impl = generate_backwards_compat_serialize(
            input,
            dag_plan,
            wire_mode,
            version_mode,
            &mut extra_helper_defs,
        )?;
        let serialize_stmt = ser_core.body;
        let ser_where_clause = ser_core.where_clause;
        let serialize_impl = quote! {
            impl #impl_generics ::serde::Serialize for #target_ty #ser_where_clause {
                fn serialize<__S>(&self, __serializer: __S) -> ::core::result::Result<__S::Ok, __S::Error>
                where
                    __S: ::serde::Serializer,
                {
                    #serialize_stmt
                }
            }
        };
        (ser_core.helper_def, serialize_impl, ser_as_impl)
    } else {
        (TokenStream::new(), TokenStream::new(), TokenStream::new())
    };

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

    let (de_helper_defs, deserialize_impls, accepts_untagged_const) = if input.deserialize {
        let de_generics = build_de_generics(&input.generics);
        let (de_impl_generics, _, _) = de_generics.split_for_impl();
        let de_core = generate_deserialize_core(input, dag_plan, wire_mode, true)?;

        let emit_de_pair = |cfg_prefix: TokenStream,
                            where_cl: Option<&syn::WhereClause>,
                            body: &TokenStream| {
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

        let (de_impls, accepts_untagged) = if let Some(ref untagged) = de_core.untagged {
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

        (de_core.helper_defs, de_impls, accepts_untagged)
    } else {
        (
            TokenStream::new(),
            TokenStream::new(),
            quote!(
                const ACCEPTS_UNTAGGED: bool = false;
            ),
        )
    };

    let extra = extra_items.unwrap_or_default();
    let legacy_deprecation_warnings =
        emit_legacy_deprecation_warnings(&input.legacy_warnings, &input.allow_attrs);

    let version_ty = version_mode.version_ty();
    let current_version_expr = version_mode.tag_expr(&input.current_version);
    let serialize_version_expr = version_mode.tag_expr(&input.serialize_version);

    let code = quote! {
        const _: () = {
            #legacy_deprecation_warnings

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

            #serialize_impl

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
