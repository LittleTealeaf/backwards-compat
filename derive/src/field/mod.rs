use std::collections::HashSet;

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::spanned::Spanned as _;

use crate::codegen::{self, SelfWireMode};
use crate::dag;
use crate::parse::BackwardsCompatArgs;

/// Extracted and validated specification for a single field annotated with `#[backwards_compat(...)]`.
#[derive(Debug, Clone)]
pub struct FieldCompatSpec {
    pub variant_idx: Option<usize>,
    pub field_idx: usize,
    pub helper_ident: syn::Ident,
    #[allow(dead_code, reason = "used when schemars feature is enabled")]
    pub schema_helper_ident: syn::Ident,
    pub field_ty: syn::Type,
    pub filtered_generics: syn::Generics,
    pub cfgs: Vec<syn::Attribute>,
    pub args: BackwardsCompatArgs,
}

impl FieldCompatSpec {
    #[cfg(feature = "schemars")]
    pub fn schema_helper_ty_str(&self) -> String {
        let (_, ty_generics, _) = self.filtered_generics.split_for_impl();
        let ident = &self.schema_helper_ident;
        quote!(#ident #ty_generics).to_string()
    }
}

#[derive(Debug, Default)]
struct OuterParams {
    lifetimes: HashSet<String>,
    types: HashSet<String>,
    consts: HashSet<String>,
}

impl OuterParams {
    fn from_generics(generics: &syn::Generics) -> Self {
        let mut out = Self::default();
        for param in &generics.params {
            match param {
                syn::GenericParam::Lifetime(lp) => {
                    out.lifetimes.insert(lp.lifetime.ident.to_string());
                }
                syn::GenericParam::Type(tp) => {
                    out.types.insert(tp.ident.to_string());
                }
                syn::GenericParam::Const(cp) => {
                    out.consts.insert(cp.ident.to_string());
                }
            }
        }
        out
    }
}

#[derive(Debug, Default)]
struct UsedParams {
    lifetimes: HashSet<String>,
    types: HashSet<String>,
    consts: HashSet<String>,
}

impl UsedParams {
    fn is_empty(&self) -> bool {
        self.lifetimes.is_empty() && self.types.is_empty() && self.consts.is_empty()
    }

    fn is_subset_of(&self, other: &Self) -> bool {
        self.lifetimes.is_subset(&other.lifetimes)
            && self.types.is_subset(&other.types)
            && self.consts.is_subset(&other.consts)
    }

    fn record_lifetime(&mut self, lt: &syn::Lifetime, outer: &OuterParams, shadowed: &HashSet<String>) {
        let name = lt.ident.to_string();
        if !shadowed.contains(&name) && outer.lifetimes.contains(&name) {
            self.lifetimes.insert(name);
        }
    }

    fn record_ident_type_or_const(&mut self, ident: &syn::Ident, outer: &OuterParams, allow_const: bool) {
        let name = ident.to_string();
        if outer.types.contains(&name) {
            self.types.insert(name);
        } else if allow_const && outer.consts.contains(&name) {
            self.consts.insert(name);
        }
    }
}

fn strip_raw_prefix(ident: &syn::Ident) -> String {
    let s = ident.to_string();
    s.strip_prefix("r#").unwrap_or(&s).to_owned()
}

fn is_bare_self(ty: &syn::Type) -> bool {
    matches!(
        ty,
        syn::Type::Path(tp) if tp.qself.is_none() && tp.path.is_ident("Self")
    )
}

fn check_serde_attr_conflicts(attrs: &[syn::Attribute]) -> syn::Result<()> {
    for attr in attrs {
        if !attr.path().is_ident("serde") {
            continue;
        }
        let mut conflict: Option<syn::Error> = None;
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("with")
                || meta.path.is_ident("serialize_with")
                || meta.path.is_ident("deserialize_with")
            {
                let name = meta
                    .path
                    .get_ident()
                    .map_or_else(|| "with".to_owned(), ToString::to_string);
                conflict = Some(syn::Error::new_spanned(
                    &meta.path,
                    format!("`#[backwards_compat]` on a field conflicts with `#[serde({name} = ...)]`"),
                ));
                return Err(syn::Error::new(meta.path.span(), "conflict"));
            }
            if meta.input.peek(syn::Token![=]) {
                let value = meta.value()?;
                let _: syn::Expr = value.parse()?;
            } else if meta.input.peek(syn::token::Paren) {
                let content;
                syn::parenthesized!(content in meta.input);
                let _: TokenStream = content.parse()?;
            }
            Ok(())
        });
        if let Some(err) = conflict {
            return Err(err);
        }
    }
    Ok(())
}

fn parse_field_bc_attr(attr: &syn::Attribute) -> syn::Result<BackwardsCompatArgs> {
    let mut args = match &attr.meta {
        syn::Meta::Path(_) => syn::parse2::<BackwardsCompatArgs>(TokenStream::new())?,
        syn::Meta::List(list) => syn::parse2::<BackwardsCompatArgs>(list.tokens.clone())?,
        syn::Meta::NameValue(_) => {
            return Err(syn::Error::new_spanned(
                attr,
                "expected `#[backwards_compat]` or `#[backwards_compat(...)]`",
            ));
        }
    };
    if args.serialize_specified || args.deserialize_specified {
        return Err(syn::Error::new_spanned(
            attr,
            "`serialize` and `deserialize` options are only supported on struct/enum-level `#[backwards_compat(...)]`, not on field-level attributes",
        ));
    }
    if !args.external && args.content_field.is_none() {
        args.content_field = Some("data".to_owned());
    }
    Ok(args)
}

fn walk_path_arguments(
    args: &syn::PathArguments,
    outer: &OuterParams,
    shadowed: &HashSet<String>,
    used: &mut UsedParams,
) {
    match args {
        syn::PathArguments::None => {}
        syn::PathArguments::AngleBracketed(ab) => {
            for arg in &ab.args {
                walk_generic_arg(arg, outer, shadowed, used);
            }
        }
        syn::PathArguments::Parenthesized(p) => {
            for input in &p.inputs {
                walk_type(&input.ty, outer, shadowed, used);
            }
            walk_return_type(&p.output, outer, shadowed, used);
        }
    }
}

fn walk_path(path: &syn::Path, outer: &OuterParams, shadowed: &HashSet<String>, used: &mut UsedParams) {
    for seg in &path.segments {
        walk_path_arguments(&seg.arguments, outer, shadowed, used);
    }
}

fn walk_return_type(
    ret: &syn::ReturnType,
    outer: &OuterParams,
    shadowed: &HashSet<String>,
    used: &mut UsedParams,
) {
    if let syn::ReturnType::Type(_, ty) = ret {
        walk_type(ty, outer, shadowed, used);
    }
}

fn extend_shadowed_lifetimes(
    base: &HashSet<String>,
    bound_lifetimes: Option<&syn::BoundLifetimes>,
) -> HashSet<String> {
    let mut next = base.clone();
    if let Some(bl) = bound_lifetimes {
        for param in &bl.lifetimes {
            if let syn::GenericParam::Lifetime(lp) = param {
                next.insert(lp.lifetime.ident.to_string());
            }
        }
    }
    next
}

fn walk_type_param_bound(
    bound: &syn::TypeParamBound,
    outer: &OuterParams,
    shadowed: &HashSet<String>,
    used: &mut UsedParams,
) {
    match bound {
        syn::TypeParamBound::Trait(tb) => {
            let inner_shadowed = extend_shadowed_lifetimes(shadowed, tb.lifetimes.as_ref());
            walk_path(&tb.path, outer, &inner_shadowed, used);
        }
        syn::TypeParamBound::Lifetime(lt) => {
            used.record_lifetime(lt, outer, shadowed);
        }
        syn::TypeParamBound::PreciseCapture(pc) => {
            for param in &pc.params {
                match param {
                    syn::CapturedParam::Lifetime(lt) => used.record_lifetime(lt, outer, shadowed),
                    syn::CapturedParam::Ident(ident) => used.record_ident_type_or_const(ident, outer, true),
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

fn walk_generic_arg(
    arg: &syn::GenericArgument,
    outer: &OuterParams,
    shadowed: &HashSet<String>,
    used: &mut UsedParams,
) {
    match arg {
        syn::GenericArgument::Lifetime(lt) => used.record_lifetime(lt, outer, shadowed),
        syn::GenericArgument::Type(ty) => walk_type(ty, outer, shadowed, used),
        syn::GenericArgument::Const(expr) => walk_expr(expr, outer, shadowed, used),
        syn::GenericArgument::AssocType(assoc) => {
            if let Some(ref gen_args) = assoc.generics {
                for ga in &gen_args.args {
                    walk_generic_arg(ga, outer, shadowed, used);
                }
            }
            walk_type(&assoc.ty, outer, shadowed, used);
        }
        syn::GenericArgument::AssocConst(assoc) => {
            if let Some(ref gen_args) = assoc.generics {
                for ga in &gen_args.args {
                    walk_generic_arg(ga, outer, shadowed, used);
                }
            }
            walk_expr(&assoc.value, outer, shadowed, used);
        }
        syn::GenericArgument::Constraint(constraint) => {
            if let Some(ref gen_args) = constraint.generics {
                for ga in &gen_args.args {
                    walk_generic_arg(ga, outer, shadowed, used);
                }
            }
            for b in &constraint.bounds {
                walk_type_param_bound(b, outer, shadowed, used);
            }
        }
        _ => {}
    }
}

fn walk_expr(expr: &syn::Expr, outer: &OuterParams, shadowed: &HashSet<String>, used: &mut UsedParams) {
    match expr {
        syn::Expr::Path(ep) => {
            if let Some(ref qself) = ep.qself {
                walk_type(&qself.ty, outer, shadowed, used);
            } else if ep.path.leading_colon.is_none()
                && let Some(first) = ep.path.segments.first()
            {
                let is_single = ep.path.segments.len() == 1;
                used.record_ident_type_or_const(&first.ident, outer, is_single);
            }
            walk_path(&ep.path, outer, shadowed, used);
        }
        syn::Expr::Binary(eb) => {
            walk_expr(&eb.left, outer, shadowed, used);
            walk_expr(&eb.right, outer, shadowed, used);
        }
        syn::Expr::Unary(eu) => walk_expr(&eu.expr, outer, shadowed, used),
        syn::Expr::Paren(ep) => walk_expr(&ep.expr, outer, shadowed, used),
        syn::Expr::Group(eg) => walk_expr(&eg.expr, outer, shadowed, used),
        syn::Expr::Cast(ec) => {
            walk_expr(&ec.expr, outer, shadowed, used);
            walk_type(&ec.ty, outer, shadowed, used);
        }
        syn::Expr::Block(eb) => {
            for stmt in &eb.block.stmts {
                if let syn::Stmt::Expr(e, _) = stmt {
                    walk_expr(e, outer, shadowed, used);
                }
            }
        }
        syn::Expr::Call(ec) => {
            walk_expr(&ec.func, outer, shadowed, used);
            for arg in &ec.args {
                walk_expr(arg, outer, shadowed, used);
            }
        }
        syn::Expr::Tuple(et) => {
            for elem in &et.elems {
                walk_expr(elem, outer, shadowed, used);
            }
        }
        syn::Expr::Array(ea) => {
            for elem in &ea.elems {
                walk_expr(elem, outer, shadowed, used);
            }
        }
        syn::Expr::Repeat(er) => {
            walk_expr(&er.expr, outer, shadowed, used);
            walk_expr(&er.len, outer, shadowed, used);
        }
        syn::Expr::Reference(er) => walk_expr(&er.expr, outer, shadowed, used),
        syn::Expr::Field(ef) => walk_expr(&ef.base, outer, shadowed, used),
        syn::Expr::Index(ei) => {
            walk_expr(&ei.expr, outer, shadowed, used);
            walk_expr(&ei.index, outer, shadowed, used);
        }
        syn::Expr::If(ei) => {
            walk_expr(&ei.cond, outer, shadowed, used);
            for stmt in &ei.then_branch.stmts {
                if let syn::Stmt::Expr(e, _) = stmt {
                    walk_expr(e, outer, shadowed, used);
                }
            }
            if let Some((_, ref else_branch)) = ei.else_branch {
                walk_expr(else_branch, outer, shadowed, used);
            }
        }
        _ => {}
    }
}

fn walk_type(ty: &syn::Type, outer: &OuterParams, shadowed: &HashSet<String>, used: &mut UsedParams) {
    match ty {
        syn::Type::Path(tp) => {
            if let Some(ref qself) = tp.qself {
                walk_type(&qself.ty, outer, shadowed, used);
            } else if tp.path.leading_colon.is_none()
                && let Some(first) = tp.path.segments.first()
            {
                // Single-segment paths in generic argument position can be const params (e.g. `Buf<N>`).
                let is_single = tp.path.segments.len() == 1;
                used.record_ident_type_or_const(&first.ident, outer, is_single);
            }
            walk_path(&tp.path, outer, shadowed, used);
        }
        syn::Type::Reference(tr) => {
            if let Some(ref lt) = tr.lifetime {
                used.record_lifetime(lt, outer, shadowed);
            }
            walk_type(&tr.elem, outer, shadowed, used);
        }
        syn::Type::Slice(ts) => walk_type(&ts.elem, outer, shadowed, used),
        syn::Type::Array(ta) => {
            walk_type(&ta.elem, outer, shadowed, used);
            walk_expr(&ta.len, outer, shadowed, used);
        }
        syn::Type::Tuple(tt) => {
            for elem in &tt.elems {
                walk_type(elem, outer, shadowed, used);
            }
        }
        syn::Type::Paren(tp) => walk_type(&tp.elem, outer, shadowed, used),
        syn::Type::Group(tg) => walk_type(&tg.elem, outer, shadowed, used),
        syn::Type::Ptr(tp) => walk_type(&tp.elem, outer, shadowed, used),
        syn::Type::FnPtr(bf) => {
            let inner_shadowed = extend_shadowed_lifetimes(shadowed, bf.lifetimes.as_ref());
            for input in &bf.inputs {
                walk_type(&input.ty, outer, &inner_shadowed, used);
            }
            walk_return_type(&bf.output, outer, &inner_shadowed, used);
        }
        syn::Type::TraitObject(to) => {
            for b in &to.bounds {
                walk_type_param_bound(b, outer, shadowed, used);
            }
        }
        syn::Type::ImplTrait(it) => {
            for b in &it.bounds {
                walk_type_param_bound(b, outer, shadowed, used);
            }
        }
        _ => {}
    }
}

fn collect_used_in_type(ty: &syn::Type, outer: &OuterParams) -> UsedParams {
    let mut used = UsedParams::default();
    let shadowed = HashSet::new();
    walk_type(ty, outer, &shadowed, &mut used);
    used
}

fn collect_used_in_where_predicate(pred: &syn::WherePredicate, outer: &OuterParams) -> UsedParams {
    let mut used = UsedParams::default();
    let shadowed = HashSet::new();
    match pred {
        syn::WherePredicate::Type(pt) => {
            let inner_shadowed = extend_shadowed_lifetimes(&shadowed, pt.lifetimes.as_ref());
            walk_type(&pt.bounded_ty, outer, &inner_shadowed, &mut used);
            for b in &pt.bounds {
                walk_type_param_bound(b, outer, &inner_shadowed, &mut used);
            }
        }
        syn::WherePredicate::Lifetime(pl) => {
            used.record_lifetime(&pl.lifetime, outer, &shadowed);
            for b in &pl.bounds {
                used.record_lifetime(b, outer, &shadowed);
            }
        }
        _ => {}
    }
    used
}

fn validate_historical_version_generics(
    outer_generics: &syn::Generics,
    outer: &OuterParams,
    field_used: &UsedParams,
    args: &BackwardsCompatArgs,
) -> syn::Result<()> {
    for v in &args.versions {
        if is_bare_self(&v.ty) {
            continue;
        }
        let v_used = collect_used_in_type(&v.ty, outer);
        for param in &outer_generics.params {
            match param {
                syn::GenericParam::Lifetime(lp) => {
                    let name = lp.lifetime.ident.to_string();
                    if v_used.lifetimes.contains(&name) && !field_used.lifetimes.contains(&name) {
                        return Err(syn::Error::new_spanned(
                            &v.ty,
                            format!(
                                "historical version type references outer lifetime `'{name}` which is not referenced by this field's type"
                            ),
                        ));
                    }
                }
                syn::GenericParam::Type(tp) => {
                    let name = tp.ident.to_string();
                    if v_used.types.contains(&name) && !field_used.types.contains(&name) {
                        return Err(syn::Error::new_spanned(
                            &v.ty,
                            format!(
                                "historical version type references outer generic type parameter `{name}` which is not referenced by this field's type"
                            ),
                        ));
                    }
                }
                syn::GenericParam::Const(cp) => {
                    let name = cp.ident.to_string();
                    if v_used.consts.contains(&name) && !field_used.consts.contains(&name) {
                        return Err(syn::Error::new_spanned(
                            &v.ty,
                            format!(
                                "historical version type references outer const parameter `{name}` which is not referenced by this field's type"
                            ),
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}

fn filter_generics(
    outer_generics: &syn::Generics,
    outer: &OuterParams,
    field_used: &UsedParams,
) -> syn::Generics {
    let mut filtered_params = syn::punctuated::Punctuated::new();
    let shadowed = HashSet::new();

    for param in &outer_generics.params {
        match param {
            syn::GenericParam::Lifetime(lp) => {
                let name = lp.lifetime.ident.to_string();
                if field_used.lifetimes.contains(&name) {
                    let mut cloned = lp.clone();
                    cloned.bounds = lp
                        .bounds
                        .iter()
                        .filter(|b| {
                            let b_name = b.ident.to_string();
                            !outer.lifetimes.contains(&b_name) || field_used.lifetimes.contains(&b_name)
                        })
                        .cloned()
                        .collect();
                    if cloned.bounds.is_empty() {
                        cloned.colon_token = None;
                    }
                    filtered_params.push(syn::GenericParam::Lifetime(cloned));
                }
            }
            syn::GenericParam::Type(tp) => {
                let name = tp.ident.to_string();
                if field_used.types.contains(&name) {
                    let mut cloned = tp.clone();
                    cloned.default = None;
                    cloned.bounds = tp
                        .bounds
                        .iter()
                        .filter(|b| {
                            let mut b_used = UsedParams::default();
                            walk_type_param_bound(b, outer, &shadowed, &mut b_used);
                            b_used.is_subset_of(field_used)
                        })
                        .cloned()
                        .collect();
                    if cloned.bounds.is_empty() {
                        cloned.colon_token = None;
                    }
                    filtered_params.push(syn::GenericParam::Type(cloned));
                }
            }
            syn::GenericParam::Const(cp) => {
                let name = cp.ident.to_string();
                if field_used.consts.contains(&name) {
                    let mut cloned = cp.clone();
                    cloned.default = None;
                    filtered_params.push(syn::GenericParam::Const(cloned));
                }
            }
        }
    }

    let where_clause = outer_generics.where_clause.as_ref().and_then(|wc| {
        let mut kept = syn::punctuated::Punctuated::new();
        for pred in &wc.predicates {
            let pred_used = collect_used_in_where_predicate(pred, outer);
            if !pred_used.is_empty() && pred_used.is_subset_of(field_used) {
                kept.push(pred.clone());
            }
        }
        (!kept.is_empty()).then_some(syn::WhereClause {
            where_token: wc.where_token,
            predicates: kept,
        })
    });

    let has_params = !filtered_params.is_empty();
    syn::Generics {
        lt_token: has_params.then(syn::token::Lt::default),
        params: filtered_params,
        gt_token: has_params.then(syn::token::Gt::default),
        where_clause,
    }
}

fn extract_single_field(
    field: &syn::Field,
    variant_idx: Option<usize>,
    field_idx: usize,
    helper_ident: syn::Ident,
    outer_generics: &syn::Generics,
    outer: &OuterParams,
    extra_cfgs: &[syn::Attribute],
) -> syn::Result<Option<FieldCompatSpec>> {
    let mut bc_attrs = field
        .attrs
        .iter()
        .filter(|a| a.path().is_ident("backwards_compat"));

    let Some(first_bc) = bc_attrs.next() else {
        return Ok(None);
    };
    if let Some(second_bc) = bc_attrs.next() {
        return Err(syn::Error::new_spanned(
            second_bc,
            "duplicate `#[backwards_compat]` attribute on field",
        ));
    }

    check_serde_attr_conflicts(&field.attrs)?;
    let args = parse_field_bc_attr(first_bc)?;

    let field_used = collect_used_in_type(&field.ty, outer);
    validate_historical_version_generics(outer_generics, outer, &field_used, &args)?;
    let filtered_generics = filter_generics(outer_generics, outer, &field_used);

    let mut cfgs: Vec<syn::Attribute> = extra_cfgs.to_vec();
    cfgs.extend(field.attrs.iter().filter(|a| a.path().is_ident("cfg")).cloned());
    let schema_helper_ident = format_ident!("{}_Schema", helper_ident, span = Span::call_site());

    Ok(Some(FieldCompatSpec {
        variant_idx,
        field_idx,
        helper_ident,
        schema_helper_ident,
        field_ty: field.ty.clone(),
        filtered_generics,
        cfgs,
        args,
    }))
}

/// Extracts and validates all field-level `#[backwards_compat(...)]` specs from a struct.
pub fn extract_struct_field_compat(item_struct: &syn::ItemStruct) -> syn::Result<Vec<FieldCompatSpec>> {
    let outer = OuterParams::from_generics(&item_struct.generics);
    let type_name = strip_raw_prefix(&item_struct.ident);
    let call_site = Span::call_site();
    let mut specs = Vec::new();

    for (field_idx, field) in item_struct.fields.iter().enumerate() {
        let helper_ident = field.ident.as_ref().map_or_else(
            || format_ident!("__BcField_{}_{}", type_name, field_idx, span = call_site),
            |f_ident| {
                format_ident!(
                    "__BcField_{}_{}",
                    type_name,
                    strip_raw_prefix(f_ident),
                    span = call_site
                )
            },
        );
        if let Some(spec) = extract_single_field(
            field,
            None,
            field_idx,
            helper_ident,
            &item_struct.generics,
            &outer,
            &[],
        )? {
            specs.push(spec);
        }
    }

    Ok(specs)
}

/// Extracts and validates all field-level `#[backwards_compat(...)]` specs from an enum.
pub fn extract_enum_field_compat(item_enum: &syn::ItemEnum) -> syn::Result<Vec<FieldCompatSpec>> {
    let outer = OuterParams::from_generics(&item_enum.generics);
    let enum_name = strip_raw_prefix(&item_enum.ident);
    let call_site = Span::call_site();
    let mut specs = Vec::new();

    for (variant_idx, variant) in item_enum.variants.iter().enumerate() {
        let variant_name = strip_raw_prefix(&variant.ident);
        let variant_cfgs: Vec<syn::Attribute> = variant
            .attrs
            .iter()
            .filter(|a| a.path().is_ident("cfg"))
            .cloned()
            .collect();

        for (field_idx, field) in variant.fields.iter().enumerate() {
            let helper_ident = field.ident.as_ref().map_or_else(
                || {
                    format_ident!(
                        "__BcField_{}_{}_{}",
                        enum_name,
                        variant_name,
                        field_idx,
                        span = call_site
                    )
                },
                |f_ident| {
                    format_ident!(
                        "__BcField_{}_{}_{}",
                        enum_name,
                        variant_name,
                        strip_raw_prefix(f_ident),
                        span = call_site
                    )
                },
            );
            if let Some(spec) = extract_single_field(
                field,
                Some(variant_idx),
                field_idx,
                helper_ident,
                &item_enum.generics,
                &outer,
                &variant_cfgs,
            )? {
                specs.push(spec);
            }
        }
    }

    Ok(specs)
}

fn with_bound(where_clause: Option<&syn::WhereClause>, extra: syn::WherePredicate) -> syn::WhereClause {
    let mut wc = where_clause.cloned().unwrap_or_else(|| syn::WhereClause {
        where_token: syn::token::Where::default(),
        predicates: syn::punctuated::Punctuated::new(),
    });
    wc.predicates.push(extra);
    wc
}

/// Generates the `__BcField_...` helper struct and its `serialize` / `serialize_ref` / `deserialize` methods
/// (and optionally a `__BcField_..._Schema` `JsonSchema` helper when `derive_json_schema` is true).
pub fn generate_field_helper(
    spec: &FieldCompatSpec,
    derive_json_schema: bool,
    enable_serialize: bool,
    enable_deserialize: bool,
) -> syn::Result<TokenStream> {
    #[cfg(not(feature = "schemars"))]
    let _ = derive_json_schema;

    let input = spec.args.clone().into_input(
        spec.field_ty.clone(),
        syn::Visibility::Inherited,
        spec.filtered_generics.clone(),
    )?;
    let dag_plan = dag::resolve_dag(&input)?;

    let helper_ident = &spec.helper_ident;
    let field_ty = &spec.field_ty;
    let cfgs = &spec.cfgs;

    #[cfg(feature = "schemars")]
    let (schema_struct_def, schema_impl_items) = if derive_json_schema {
        let schema_helper_ident = &spec.schema_helper_ident;
        let (filtered_impl_generics, filtered_ty_generics, filtered_where_clause) =
            spec.filtered_generics.split_for_impl();
        let schema_helper_ty: syn::Type = syn::parse_quote!(#schema_helper_ident #filtered_ty_generics);
        let mut schema_wc = filtered_where_clause
            .cloned()
            .unwrap_or_else(|| syn::WhereClause {
                where_token: syn::token::Where::default(),
                predicates: syn::punctuated::Punctuated::new(),
            });
        let mut schema_helper_defs = Vec::new();
        let (schema_name_body, schema_id_body) = codegen::build_schema_name_and_id(
            &helper_ident.to_string(),
            &spec.filtered_generics,
            &mut schema_wc,
        );
        let json_schema_body = codegen::generate_json_schema_body(
            &input,
            &dag_plan,
            SelfWireMode::Direct,
            &schema_helper_ty,
            &filtered_impl_generics,
            filtered_where_clause,
            &mut schema_wc,
            &mut schema_helper_defs,
        )?;
        let schema_where_clause = (!schema_wc.predicates.is_empty()).then_some(schema_wc);

        let def = quote! {
            #(#cfgs)*
            #[doc(hidden)]
            #[allow(non_camel_case_types, dead_code, reason = "internal field-level schemars helper")]
            struct #schema_helper_ident #filtered_impl_generics (
                ::core::marker::PhantomData<fn() -> #field_ty>,
            ) #filtered_where_clause;
        };
        let impl_tokens = quote! {
            #(#schema_helper_defs)*

            impl #filtered_impl_generics ::backwards_compat::__private::schemars::JsonSchema
                for #schema_helper_ident #filtered_ty_generics #schema_where_clause
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
        };
        (def, impl_tokens)
    } else {
        (TokenStream::new(), TokenStream::new())
    };
    #[cfg(not(feature = "schemars"))]
    let (schema_struct_def, schema_impl_items) = (TokenStream::new(), TokenStream::new());

    let (ser_helper_def, serialize_fns) = if enable_serialize {
        let ser_core =
            codegen::generate_serialize_core(&input, &dag_plan, SelfWireMode::Direct, &quote!(__value))?;
        let mut ser_fn_generics = input.generics.clone();
        ser_fn_generics.params.push(syn::parse_quote!(__S));
        let (ser_fn_impl_generics, _, _) = ser_fn_generics.split_for_impl();
        let ser_wc = with_bound(
            ser_core.where_clause.as_ref(),
            syn::parse_quote!(__S: ::serde::Serializer),
        );
        let ser_body = ser_core.body;
        let fns = quote! {
            fn serialize #ser_fn_impl_generics (
                __value: &#field_ty,
                __serializer: __S,
            ) -> ::core::result::Result<__S::Ok, __S::Error>
            #ser_wc
            {
                #ser_body
            }

            fn serialize_ref #ser_fn_impl_generics (
                __value: &&#field_ty,
                __serializer: __S,
            ) -> ::core::result::Result<__S::Ok, __S::Error>
            #ser_wc
            {
                Self::serialize(*__value, __serializer)
            }
        };
        (ser_core.helper_def, fns)
    } else {
        (TokenStream::new(), TokenStream::new())
    };

    let (de_helper_defs, deserialize_fns) = if enable_deserialize {
        let de_core = codegen::generate_deserialize_core(&input, &dag_plan, SelfWireMode::Direct, false)?;
        let mut de_fn_generics = codegen::build_de_generics(&input.generics);
        de_fn_generics.params.push(syn::parse_quote!(__D));
        let (de_fn_impl_generics, _, _) = de_fn_generics.split_for_impl();

        let emit_de_fn =
            |cfg_prefix: TokenStream, where_cl: Option<&syn::WhereClause>, body: &TokenStream| {
                let de_wc = with_bound(where_cl, syn::parse_quote!(__D: ::serde::Deserializer<'__de>));
                quote! {
                    #cfg_prefix
                    fn deserialize #de_fn_impl_generics (
                        __deserializer: __D,
                    ) -> ::core::result::Result<#field_ty, __D::Error>
                    #de_wc
                    {
                        #body
                    }
                }
            };

        let fns = if let Some(ref untagged) = de_core.untagged {
            untagged.overall_cfg.as_ref().map_or_else(
                || emit_de_fn(TokenStream::new(), untagged.where_clause.as_ref(), &untagged.body),
                |pred| {
                    let enabled = emit_de_fn(
                        quote!(#[cfg(#pred)]),
                        untagged.where_clause.as_ref(),
                        &untagged.body,
                    );
                    let disabled = emit_de_fn(
                        quote!(#[cfg(not(#pred))]),
                        de_core.base_where_clause.as_ref(),
                        &de_core.base_body,
                    );
                    quote! {
                        #enabled
                        #disabled
                    }
                },
            )
        } else {
            emit_de_fn(
                TokenStream::new(),
                de_core.base_where_clause.as_ref(),
                &de_core.base_body,
            )
        };

        (de_core.helper_defs, fns)
    } else {
        (TokenStream::new(), TokenStream::new())
    };

    let code = quote! {
        #(#cfgs)*
        #[doc(hidden)]
        #[allow(non_camel_case_types, dead_code, reason = "internal field-level backwards_compat helper")]
        struct #helper_ident;

        #schema_struct_def

        #(#cfgs)*
        const _: () = {
            #ser_helper_def
            #de_helper_defs
            #schema_impl_items

            #[allow(dead_code, reason = "internal field-level backwards_compat helper methods")]
            impl #helper_ident {
                #serialize_fns

                #deserialize_fns
            }
        };
    };

    if input.dump {
        eprintln!(
            "=== backwards_compat field helper {helper_ident} ===\n{code}\n======================================"
        );
    }

    Ok(code)
}

#[cfg(test)]
mod tests;
