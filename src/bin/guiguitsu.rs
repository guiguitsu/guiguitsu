use anyhow::Result;
use guiguitsu::cli::{self, CliCommand};

fn main() -> Result<()> {
    let cmd = cli::parse_command(std::env::args().skip(1).collect())?;
    match cmd {
        CliCommand::Help => {
            cli::print_help("guiguitsu");
            Ok(())
        }
        CliCommand::Run { repo_path, print_merge_command, verbose } => {
            guiguitsu::gui::run_app(repo_path, print_merge_command, verbose)
        }
        other => {
            cli::dispatch_non_gui(other)?;
            Ok(())
        }
    }
}
