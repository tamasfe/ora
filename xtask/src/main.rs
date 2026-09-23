#![allow(missing_docs)]

mod codegen;
mod flags;
mod ui;

fn main() {
    match flags::Xtask::from_env_or_exit().subcommand {
        flags::XtaskCmd::Codegen(codegen) => match codegen.subcommand {
            flags::CodegenCmd::Proto(_) => {
                codegen::generate_proto();
            }
        },
        flags::XtaskCmd::Ui(ui) => match ui.subcommand {
            flags::UiCmd::Build(_) => {
                ui::build();
            }
        },
    }
}
