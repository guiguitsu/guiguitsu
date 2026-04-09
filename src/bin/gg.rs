use anyhow::{Result, bail};
use guiguitsu::cli::{self, CliCommand};

fn main() -> Result<()> {
    let cmd = cli::parse_command(std::env::args().skip(1).collect())?;
    match cmd {
        CliCommand::Help => {
            cli::print_help("gg");
            Ok(())
        }
        CliCommand::Run { .. } => {
            bail!("gg is the terminal-only build of guiguitsu and has no GUI; use the `guiguitsu` binary instead, or one of the subcommands (stacks, rebase, advance, move, new-stack, create-config, --remove-stack, --reread-stacks)");
        }
        other => {
            cli::dispatch_non_gui(other)?;
            Ok(())
        }
    }
}
