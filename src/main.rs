mod cli;
mod commands;
mod config;
mod constants;
mod context;
mod editor;
mod hydrate;
mod model;
mod mutagen;
mod open;
mod process;
mod session;
mod ssh;
mod target;
mod util;

use anyhow::Result;
use clap::{CommandFactory, Parser};
use cli::{Cli, Commands};

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Open(open)) => {
            let mut options = cli.open_options;
            options.merge(open.options);
            open::open_target(&open.target, &options)
        }
        Some(Commands::Capabilities(command)) => commands::capabilities(command),
        Some(Commands::Hydrate(command)) => hydrate::hydrate(command),
        Some(Commands::List(command)) => commands::list(command),
        Some(Commands::Status(command)) => commands::status(command),
        Some(Commands::Flush(command)) => commands::flush(command),
        Some(Commands::Stop(command)) => commands::stop(command),
        Some(Commands::Watch(command)) => commands::watch(command),
        None => {
            let Some(target) = cli.target else {
                Cli::command().print_help()?;
                println!();
                return Ok(());
            };
            open::open_target(&target, &cli.open_options)
        }
    }
}

#[cfg(test)]
mod tests;
