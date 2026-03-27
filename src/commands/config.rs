use crate::paths;
use std::fs;

pub fn run() {
    let path = paths::config_file();

    if !path.exists() {
        eprintln!("No config found. Run `mimi setup` first.");
        std::process::exit(1);
    }

    let content = fs::read_to_string(&path).expect("failed to read config");
    println!("Config ({})\n", path.display());
    println!("{}", content);
    println!("\nEdit directly at: {}", path.display());
}
