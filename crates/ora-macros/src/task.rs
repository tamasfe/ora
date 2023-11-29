use darling::FromDeriveInput;
use proc_macro2::TokenStream;
use quote::{quote, ToTokens};
use syn::{parse_quote, LitStr, Type};

#[derive(FromDeriveInput)]
#[darling(attributes(task), forward_attrs(doc))]
pub struct TaskDerive {
    #[darling(default)]
    extra: bool,
    #[darling(default)]
    no_schema: bool,
    #[darling(default)]
    output: Option<Type>,
    #[darling(default)]
    timeout: Option<LitStr>,
    ident: syn::Ident,
    attrs: Vec<syn::Attribute>,
    generics: syn::Generics,
}

impl ToTokens for TaskDerive {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let TaskDerive {
            ident,
            generics,
            attrs,
            extra,
            timeout,
            no_schema,
            output,
        } = self;

        let unit_ty: Type = parse_quote! { () };

        let output_ty = output.as_ref().unwrap_or(&unit_ty);

        let input_schema = if *no_schema {
            quote! { None }
        } else {
            quote! {
                Some(ora::__private::serde_json::to_value(ora::__private::schemars::schema_for!(Self)).unwrap())
            }
        };

        let output_schema = if *no_schema {
            quote! { None }
        } else {
            quote! {
                Some(ora::__private::serde_json::to_value(ora::__private::schemars::schema_for!(Self::Output)).unwrap())
            }
        };

        let timeout = if let Some(timeout) = timeout {
            match humantime::parse_duration(timeout.value().as_str()) {
                Ok(d) => {
                    let timeout_nanos = d.as_nanos();
                    let timeout_nanos = i64::try_from(timeout_nanos).unwrap_or(i64::MAX);

                    quote! {
                        ora::TimeoutPolicy::from_target(
                            ora::__private::time::Duration::nanoseconds(#timeout_nanos)
                        )
                    }
                }
                Err(_) => {
                    proc_macro_error::emit_error!(
                        timeout,
                        "invalid duration, expected something like `1h30m`"
                    );
                    quote! {
                        ora::defaults::task_timeout::<Self>()
                    }
                }
            }
        } else {
            quote! {
                ora::defaults::task_timeout::<Self>()
            }
        };

        let mut doc_comments = String::new();

        for attr in attrs {
            if let syn::Meta::NameValue(nv) = &attr.meta {
                if nv.path.is_ident("doc") {
                    if let syn::Expr::Lit(lit) = &nv.value {
                        if let syn::Lit::Str(s) = &lit.lit {
                            let line = s.value();
                            let line = line.as_str();
                            let line = line.strip_prefix(' ').unwrap_or(line);
                            doc_comments.push_str(line);
                            doc_comments.push('\n');
                        }
                    }
                }
            }
        }

        let description = if doc_comments.is_empty() {
            quote!(None)
        } else {
            let doc_str = LitStr::new(&doc_comments, ident.span());
            quote!(Some(#doc_str.into()))
        };

        let metadata = quote! {
            ora::TaskMetadata {
                description: #description,
                input_schema: #input_schema,
                output_schema: #output_schema,
                ..Default::default()
            }
        };

        let provided_functions = if *extra {
            quote! {
                fn worker_selector() -> ora::WorkerSelector {
                    <Self as ora::DeriveTaskExtra>::worker_selector(
                        ora::defaults::task_worker_selector::<Self>()
                    )
                }
                fn format() -> ora::TaskDataFormat {
                    <Self as ora::DeriveTaskExtra>::format(
                        ora::defaults::task_data_format::<Self>()
                    )
                }
                fn timeout() -> ora::TimeoutPolicy {
                    <Self as ora::DeriveTaskExtra>::timeout(
                        #timeout
                    )
                }
                fn metadata() -> ora::TaskMetadata {
                    <Self as ora::DeriveTaskExtra>::metadata(#metadata)
                }
            }
        } else {
            quote! {
                fn metadata() -> ora::TaskMetadata {
                    #metadata
                }

                fn timeout() -> ora::TimeoutPolicy {
                    #timeout
                }
            }
        };

        let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

        tokens.extend(quote! {
            impl<#impl_generics> ora::Task for #ident<#ty_generics> #where_clause {
                type Output = #output_ty;

                #provided_functions
            }
        });
    }
}
