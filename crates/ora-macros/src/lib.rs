use darling::FromDeriveInput;
use proc_macro_error::proc_macro_error;
use quote::quote;

use crate::task::TaskDerive;

extern crate proc_macro;

mod task;

#[proc_macro_error]
#[proc_macro_derive(Task, attributes(task))]
pub fn derive_task(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let task = match syn::parse::<syn::DeriveInput>(input.clone()) {
        Ok(task) => task,
        Err(e) => {
            proc_macro_error::emit_call_site_error!(e.to_string());
            return input;
        }
    };
    let task = match TaskDerive::from_derive_input(&task) {
        Ok(task) => task,
        Err(e) => {
            proc_macro_error::emit_call_site_error!(e.to_string());
            return input;
        }
    };

    quote!(#task).into()
}
