use proc_macro::TokenStream;
use syn::parse_macro_input;

mod codegen;
mod parse;

use parse::BackwardsCompatInput;

/// Declarative macro for backwards compatibility schema versioning.
///
/// # Example
/// ```rust,ignore
/// backwards_compat! {
///     pub enum ConfigVersion {
///         v1 = ConfigV1,
///         v2 = ConfigV2,
///         v3 = ConfigV3,
///     }
/// }
/// ```
#[proc_macro]
pub fn backwards_compat(input: TokenStream) -> TokenStream {
    let parsed = parse_macro_input!(input as BackwardsCompatInput);
    match codegen::generate_backwards_compat(parsed) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}
