#![allow(missing_docs)]

mod codegen;
mod flags;

fn main() {
    match flags::Xtask::from_env_or_exit().subcommand {
        flags::XtaskCmd::Codegen(codegen) => match codegen.subcommand {
            flags::CodegenCmd::Proto(_) => {
                codegen::generate_proto();
            }
        },
    }
}
