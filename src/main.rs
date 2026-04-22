//! clemp — CLI entry point. Dispatches to the default setup, `update`, or
//! `list` subcommand and owns clone + cleanup around each.

use anyhow::Result;
use clap::Parser;
use clemp::{
    cleanup, clone_repo, compute_manifest, get_repo_url, list_available, normalize_setup_args,
    run_setup, run_update, Cli, CliCommand, Lockfile, OriginalCommand, CLONE_DIR, LOCKFILE_NAME,
};
use std::fs;
use std::path::Path;

fn main() -> Result<()> {
    let cli = Cli::parse();
    let clone_dir = Path::new(CLONE_DIR);

    match cli.command {
        Some(CliCommand::List { category }) => run_list(category, clone_dir),
        Some(CliCommand::Update(mut args)) => {
            normalize_setup_args(&mut args.setup);
            run_update_cmd(args, clone_dir)
        }
        None => {
            let mut args = cli.setup;
            normalize_setup_args(&mut args);
            run_setup_cmd(args, clone_dir)
        }
    }
}

fn run_list(category: Option<String>, clone_dir: &Path) -> Result<()> {
    let repo_url = get_repo_url()?;
    println!("Cloning {}...", repo_url);
    clone_repo(&repo_url)?;
    let cat = category.unwrap_or_else(|| "all".to_string());
    let result = list_available(&cat, clone_dir);
    let _ = fs::remove_dir_all(clone_dir);
    print!("{}", result?);
    Ok(())
}

fn run_setup_cmd(args: clemp::SetupArgs, clone_dir: &Path) -> Result<()> {
    let cwd = Path::new(".");

    if Lockfile::path(cwd).exists() {
        eprintln!(
            "Note: {LOCKFILE_NAME} already exists in this directory. If this project was previously\n\
             set up with clemp, you probably want `clemp update` instead — it preserves your edits.\n"
        );
    }

    let repo_url = get_repo_url()?;

    println!("Cloning {}...", repo_url);
    let template_sha = clone_repo(&repo_url)?;

    let gitignore_existed = Path::new(".gitignore").exists();
    let install_git_hooks = Path::new(".git").is_dir();

    let result = (|| {
        let resolved = run_setup(&args, clone_dir, cwd, true, install_git_hooks)?;
        let files = compute_manifest(&args, &resolved, clone_dir, cwd)?;
        Lockfile {
            template_repo: repo_url.clone(),
            template_sha: template_sha.clone(),
            original_command: OriginalCommand::from_setup(&args),
            files,
        }
        .save(cwd)?;
        Ok::<_, anyhow::Error>(())
    })();

    if let Err(e) = result {
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

fn run_update_cmd(args: clemp::UpdateArgs, clone_dir: &Path) -> Result<()> {
    let cwd = Path::new(".");
    let lock = Lockfile::load(cwd)?.ok_or_else(|| {
        anyhow::anyhow!(
            "No {LOCKFILE_NAME} found in current directory.\nThis doesn't look like a clemp-configured project — run `clemp <args>` to set one up first."
        )
    })?;

    let repo_url = lock.template_repo.clone();
    println!("Cloning {}...", repo_url);
    let template_sha = clone_repo(&repo_url)?;

    let result = run_update(&args, clone_dir, &template_sha, &repo_url);

    let _ = cleanup(clone_dir);
    result
}
