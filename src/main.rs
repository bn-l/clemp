//! clemp — CLI entry point for cloning and configuring claude-template.

use anyhow::Result;
use clap::Parser;
use clemp::{cleanup, clone_repo, get_repo_url, list_available, run_setup, split_multi_values, Cli, CLONE_DIR};
use std::fs;
use std::path::Path;

fn main() -> Result<()> {
    let mut cli = Cli::parse();
    cli.hooks = split_multi_values(cli.hooks);
    cli.mcp = split_multi_values(cli.mcp);
    cli.commands = split_multi_values(cli.commands);
    cli.githooks = split_multi_values(cli.githooks);
    let clone_dir = Path::new(CLONE_DIR);

    let repo_url = get_repo_url()?;

    println!("Cloning {}...", repo_url);
    clone_repo(&repo_url)?;

    if let Some(ref category) = cli.list {
        let result = list_available(category, clone_dir);
        let _ = fs::remove_dir_all(clone_dir);
        print!("{}", result?);
        return Ok(());
    }

    let gitignore_existed = Path::new(".gitignore").exists();

    if let Err(e) = run_setup(&cli, clone_dir) {
        eprintln!("Removing {} due to error...", clone_dir.display());
        let _ = fs::remove_dir_all(clone_dir);
        if !gitignore_existed {
            let _ = fs::remove_file(".gitignore");
        }
        return Err(e);
    }

    println!("Cleaning up...");
    cleanup(clone_dir)?;

    println!("Done! Claude template configured for your project.");
    Ok(())
}
