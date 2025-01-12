//! Macros for the ora-client crate.

use proc_macro::TokenStream;

use quote::{quote, ToTokens};
use syn::{parse_macro_input, DeriveInput, Expr};

use darling::FromDeriveInput;

#[derive(FromDeriveInput)]
#[darling(attributes(ora))]
struct JobType {
    #[darling(default = default_output)]
    output: syn::Type,
    #[darling(default)]
    retries: Option<Expr>,
    #[darling(default)]
    timeout: Option<String>,

    #[darling(default)]
    default_labels: Option<syn::Expr>,

    ident: syn::Ident,
}

fn default_output() -> syn::Type {
    syn::parse_quote! { () }
}

impl ToTokens for JobType {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let output = &self.output;
        let ident_str = self.ident.to_string();

        let retry_policy = match &self.retries {
            Some(retries) => quote! {
                fn default_retry_policy() -> ora::job_definition::RetryPolicy {
                    ora::job_definition::RetryPolicy {
                        retries: #retries,
                    }
                }
            },
            None => quote! {},
        };

        let timeout_policy = match &self.timeout {
            Some(timeout) => {
                let timeout_seconds = humantime::parse_duration(timeout).unwrap().as_secs();

                quote! {
                    fn default_timeout_policy() -> ora::job_definition::TimeoutPolicy {
                        ora::job_definition::TimeoutPolicy {
                            timeout: Some(std::time::Duration::from_secs(#timeout_seconds as _)),
                            base_time: ora::job_definition::TimeoutBaseTime::StartTime,
                        }
                    }
                }
            }
            None => quote! {},
        };

        let default_labels = match &self.default_labels {
            Some(default_labels) => quote! {
                fn default_labels() -> std::collections::HashMap<String, String> {
                    #default_labels
                }
            },
            None => quote! {},
        };

        let ident = &self.ident;
        tokens.extend(quote! {
            impl ora::JobType for #ident {
                type Output = #output;

                fn id() -> &'static str {
                    #ident_str
                }

                #retry_policy
                #timeout_policy
                #default_labels
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
