mod vmf;
mod settings;
mod cli;

use std::process;
use cli::{Cli, Command};

fn main() {
    let cli = match Cli::parse_args() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("vamorasys: {}", e.message);
            eprintln!("Run 'vamorasys --help' for usage.");
            process::exit(1);
        }
    };

    let result = match cli.command {
        Command::Settings { action, category, key, value, system } => {
            settings::handle(action, &category, key.as_deref(), value.as_deref(), system)
        }
        Command::Vmf { file, action, key, value } => {
            vmf::handle_direct(&file, action, &key, value.as_deref())
        }
        Command::Check { category } => {
            settings::check(&category)
        }
        Command::Exec { name } => {
            run_from_dir("/etc/VamoraSys/bin", &name, "binary")
        }
        Command::Utility { name } => {
            run_from_dir("/etc/VamoraSys/utilities", &name, "utility")
        }
        Command::Info => {
            print_info();
            Ok(())
        }
        Command::Help => {
            print_help();
            Ok(())
        }
    };

    match result {
        Ok(()) => process::exit(0),
        Err(e) => {
            eprintln!("vamorasys: {}", e.message);
            process::exit(e.code);
        }
    }
}

fn run_from_dir(dir: &str, name: &str, kind: &str) -> Result<(), cli::VamoraError> {
    use std::os::unix::fs::PermissionsExt;

    let path = std::path::PathBuf::from(dir).join(name);
    if !path.exists() {
        return Err(cli::VamoraError::new(1, format!("{} '{}' not found in {}", kind, name, dir)));
    }

    let meta = std::fs::metadata(&path).map_err(|e| {
        cli::VamoraError::new(3, format!("cannot read {}: {}", path.display(), e))
    })?;

    if meta.permissions().mode() & 0o111 == 0 {
        return Err(cli::VamoraError::new(3, format!("'{}' is not executable", path.display())));
    }

    let status = std::process::Command::new(&path)
        .status()
        .map_err(|e| cli::VamoraError::new(1, format!("failed to execute '{}': {}", path.display(), e)))?;

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }

    Ok(())
}

fn print_info() {
    println!("VamoraSys — Configuration Engine");
    println!("Component of VamoraOS");
    println!();
    println!("  Version  : 1.0.0");
    println!("  Format   : VMF (Vamora Meta File)");
    println!();
    println!("Paths:");
    println!("  System defaults  : /etc/VamoraSys/default/");
    println!("  System binaries  : /etc/VamoraSys/bin/");
    println!("  System utilities : /etc/VamoraSys/utilities/");
    println!("  User settings    : ~/.vamorasys/");
    println!();
    println!("Design:");
    println!("  Stateless binary — no daemon, no background service.");
    println!("  Layer system: user config overrides system defaults.");
    println!("  Categories are defined by apps, not by VamoraSys itself.");
}

fn print_help() {
    println!("vamorasys — VamoraOS Configuration Engine");
    println!();
    println!("USAGE:");
    println!("  vamorasys settings <action> <category> <key> [value] [--system]");
    println!("  vamorasys vmf <file> <action> <key> [value]");
    println!("  vamorasys check <category>");
    println!("  vamorasys exec <binary>");
    println!("  vamorasys utility <utility>");
    println!("  vamorasys info");
    println!();
    println!("SETTINGS ACTIONS:");
    println!("  put <category> <key> <value>   Add or update a key");
    println!("  get <category> <key>           Read a value (with fallback)");
    println!("  delete <category> <key>        Remove a key from user layer");
    println!("  list <category>                Show merged settings for a category");
    println!();
    println!("VMF ACTIONS:");
    println!("  put <key> <value>              Write a key directly to a .vmf file");
    println!("  get <key>                      Read a key from a .vmf file");
    println!("  delete <key>                   Delete a key from a .vmf file");
    println!("  list                           List all keys in a .vmf file");
    println!();
    println!("FLAGS:");
    println!("  --system                       Operate on system defaults (requires root)");
    println!();
    println!("EXIT CODES:");
    println!("  0  Success");
    println!("  1  File, category, or executable not found");
    println!("  2  Key not found in either layer");
    println!("  3  Permission denied");
    println!("  4  Invalid VMF syntax");
}
