//! Macros for the ora crate.
#![allow(clippy::needless_continue)]

use proc_macro::TokenStream;

use quote::{quote, ToTokens};
use syn::{parse_macro_input, DeriveInput, LitStr};

use darling::FromDeriveInput;

#[derive(FromDeriveInput)]
#[darling(attributes(ora))]
struct JobType {
    #[darling(default = default_output)]
    output: syn::Type,
    namespace: Option<LitStr>,
    name: Option<LitStr>,

    ident: syn::Ident,
}

fn default_output() -> syn::Type {
    syn::parse_quote! { () }
}

impl ToTokens for JobType {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let output = &self.output;

        let job_type_str = match (&self.name, &self.namespace) {
            (None, None) => {
                panic!(r#"#[ora(name = "..")] or #[ora(namespace = "..")] attributes are required"#)
            }
            (Some(_), Some(_)) => {
                panic!(
                    r#"#[ora(name = "..")] and #[ora(namespace = "..")] attributes are mutually exclusive"#
                )
            }
            (None, Some(ns)) => {
                let mut job_type_str = ns.value();

                if !job_type_str.is_empty() {
                    job_type_str.push('.');
                }

                job_type_str.push_str(&self.ident.to_string());
                job_type_str
            }
            (Some(name), None) => name.value(),
        };

        let ident = &self.ident;
        tokens.extend(quote! {
            impl ora::JobType for #ident {
                type Output = #output;

                fn job_type_id() -> ora::JobTypeId {
                    const ID: ora::JobTypeId = ora::JobTypeId::new_const(#job_type_str);
                    ID
                }
            }
        });
    }
}

/// Derive macro for the `JobType` trait.
#[allow(clippy::missing_panics_doc)]
#[proc_macro_derive(JobType, attributes(ora))]
pub fn job_type_derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let job_type = JobType::from_derive_input(&input).unwrap();

    quote! {#job_type}.into()
}
