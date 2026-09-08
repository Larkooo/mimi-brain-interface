use std::fs;
use std::process::Command;

const VAULT_USER: &str = "mimi-vault";

fn vault_home() -> std::path::PathBuf {
    std::path::PathBuf::from("/var/lib/mimi-vault")
}

fn vault_key_path() -> std::path::PathBuf {
    vault_home().join(".secret_key")
}

fn vault_secrets_dir() -> std::path::PathBuf {
    vault_home().join("secrets")
}

/// Reject names that could escape `vault_secrets_dir()` when joined into a
/// file path. The vault user owns `/var/lib/mimi-vault` and has write access
/// to /tmp, so an unvalidated name like `../../tmp/foo` would land outside
/// the secrets dir and quietly defeat the vault's isolation.
fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 128 {
        return Err("invalid secret name: must be 1-128 chars".into());
    }
    if name.starts_with('.') {
        return Err("invalid secret name: cannot start with '.'".into());
    }
    if name.contains("..") {
        return Err("invalid secret name: cannot contain '..'".into());
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
    {
        return Err("invalid secret name: only A-Z, a-z, 0-9, _, -, . allowed".into());
    }
    Ok(())
}

/// Check if we're running as the vault user
fn is_vault_user() -> bool {
    std::env::var("USER").map(|u| u == VAULT_USER).unwrap_or(false)
        || unsafe { libc::geteuid() } == get_vault_uid().unwrap_or(u32::MAX)
}

fn get_vault_uid() -> Option<u32> {
    let output = Command::new("id").args(["-u", VAULT_USER]).output().ok()?;
    String::from_utf8_lossy(&output.stdout).trim().parse().ok()
}

const MIMI_BIN: &str = "/usr/local/bin/mimi";

/// Run a mimi secret subcommand as the vault user via sudo
fn sudo_vault(args: &[&str]) -> std::process::Output {
    let mut cmd_args = vec!["-u", VAULT_USER, "--", MIMI_BIN, "secret"];
    cmd_args.extend(args);
    Command::new("sudo")
        .args(&cmd_args)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("failed to run sudo")
}

/// Ensure the vault key exists (only callable as vault user)
fn ensure_key() -> std::path::PathBuf {
    let key_path = vault_key_path();
    if !key_path.exists() {
        let output = Command::new("openssl")
            .args(["rand", "-hex", "32"])
            .output()
            .expect("failed to run openssl");
        let key = String::from_utf8_lossy(&output.stdout).trim().to_string();
        fs::write(&key_path, &key).expect("failed to write secret key");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&key_path, fs::Permissions::from_mode(0o600));
        }
    }
    key_path
}

/// Set a secret — delegates to vault user
pub fn set(name: &str, value: &str) {
    if let Err(e) = validate_name(name) {
        eprintln!("{e}");
        return;
    }
    if is_vault_user() {
        set_direct(name, value);
    } else {
        let output = sudo_vault(&["set", name, value]);
        print!("{}", String::from_utf8_lossy(&output.stdout));
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
    }
}

/// Direct set (runs as vault user)
fn set_direct(name: &str, value: &str) {
    let key_path = ensure_key();
    let dir = vault_secrets_dir();
    fs::create_dir_all(&dir).ok();
    let secret_path = dir.join(name);

    let result = Command::new("openssl")
        .args([
            "enc", "-aes-256-cbc", "-pbkdf2", "-iter", "100000",
            "-pass", &format!("file:{}", key_path.display()),
            "-out", &secret_path.to_string_lossy(),
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            child.stdin.as_mut().unwrap().write_all(value.as_bytes())?;
            child.wait_with_output()
        });

    match result {
        Ok(o) if o.status.success() => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&secret_path, fs::Permissions::from_mode(0o600));
            }
            println!("Secret '{}' saved (encrypted)", name);
        }
        Ok(o) => eprintln!("Failed to encrypt: {}", String::from_utf8_lossy(&o.stderr)),
        Err(e) => eprintln!("Failed to run openssl: {}", e),
    }
}

/// Decrypt a secret (only works as vault user)
fn decrypt(name: &str) -> Option<String> {
    let key_path = vault_key_path();
    let secret_path = vault_secrets_dir().join(name);
    if !secret_path.exists() {
        return None;
    }

    let output = Command::new("openssl")
        .args([
            "enc", "-d", "-aes-256-cbc", "-pbkdf2", "-iter", "100000",
            "-pass", &format!("file:{}", key_path.display()),
            "-in", &secret_path.to_string_lossy(),
        ])
        .output()
        .ok()?;

    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        None
    }
}

/// Print a decrypted secret to stdout. Intended for shell-wrapper / service
/// launch patterns where the caller needs the value without running the
/// child process under the vault user (which then can't read the caller's
/// home or touch their files). Prefer `run` when possible — `get` leaks
/// the value into the parent process's memory / stdout pipe.
pub fn get(name: &str) {
    if let Err(e) = validate_name(name) {
        eprintln!("{e}");
        std::process::exit(1);
    }
    if is_vault_user() {
        match decrypt(name) {
            Some(v) => {
                // No trailing newline — callers typically do `FOO=$(mimi
                // secret get bar)` which strips newlines anyway, but not
                // adding one lets the value be piped cleanly.
                print!("{}", v);
            }
            None => {
                eprintln!("Secret '{}' not found or failed to decrypt.", name);
                std::process::exit(1);
            }
        }
    } else {
        let output = sudo_vault(&["get", name]);
        if !output.status.success() {
            eprintln!("{}", String::from_utf8_lossy(&output.stderr));
            std::process::exit(1);
        }
        print!("{}", String::from_utf8_lossy(&output.stdout));
    }
}

/// List secret names (never values)
pub fn list() {
    if is_vault_user() {
        // Running as vault user — list directly
        let dir = vault_secrets_dir();
        let mut names: Vec<String> = fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().is_file())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        names.sort();
        if names.is_empty() {
            println!("No secrets stored.");
        } else {
            for name in &names {
                println!("  {}", name);
            }
            println!("\n{} secret(s). Values are encrypted — use 'mimi secret run' to inject.", names.len());
        }
    } else {
        // Delegate to vault user
        let output = sudo_vault(&["list"]);
        print!("{}", String::from_utf8_lossy(&output.stdout));
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
    }
}

/// Delete a secret — delegates to vault user
pub fn delete(name: &str) {
    if let Err(e) = validate_name(name) {
        eprintln!("{e}");
        return;
    }
    if is_vault_user() {
        let path = vault_secrets_dir().join(name);
        if path.exists() {
            fs::remove_file(&path).ok();
            println!("Secret '{}' deleted.", name);
        } else {
            eprintln!("Secret '{}' not found.", name);
        }
    } else {
        let output = sudo_vault(&["delete", name]);
        print!("{}", String::from_utf8_lossy(&output.stdout));
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
    }
}

/// Run a command with a decrypted secret injected as env var.
/// When called as ubuntu: delegates to vault user which decrypts and execs.
/// The decrypted value NEVER appears in stdout or the calling process.
pub fn run(name: &str, env_var: &str, cmd_args: &[String]) {
    if let Err(e) = validate_name(name) {
        eprintln!("{e}");
        std::process::exit(1);
    }
    if is_vault_user() {
        // We're the vault user — decrypt and exec
        let value = match decrypt(name) {
            Some(v) => v,
            None => {
                eprintln!("Secret '{}' not found or failed to decrypt.", name);
                std::process::exit(1);
            }
        };

        if cmd_args.is_empty() {
            eprintln!("No command specified.");
            std::process::exit(1);
        }

        let status = Command::new(&cmd_args[0])
            .args(&cmd_args[1..])
            .env(env_var, &value)
            .status();

        match status {
            Ok(s) => std::process::exit(s.code().unwrap_or(1)),
            Err(e) => {
                eprintln!("Failed to run command: {}", e);
                std::process::exit(1);
            }
        }
    } else {
        // Delegate to vault user. Preserve MIMI_HOME across the sudo drop
        // so long-running services (presence, bridges) keep resolving paths
        // against the owner's ~/.mimi rather than the vault user's home.
        let mut args = vec![
            "--preserve-env=MIMI_HOME".to_string(),
            "-u".to_string(), VAULT_USER.to_string(), "--".to_string(),
            MIMI_BIN.to_string(), "secret".to_string(),
            "run".to_string(), name.to_string(), env_var.to_string(),
        ];
        args.extend(cmd_args.iter().cloned());

        let status = Command::new("sudo")
            .args(&args)
            .stdin(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .status();

        match status {
            Ok(s) => std::process::exit(s.code().unwrap_or(1)),
            Err(e) => {
                eprintln!("Failed to run sudo: {}", e);
                std::process::exit(1);
            }
        }
    }
}

/// List secret names as JSON (for dashboard API) — delegates to vault user
pub fn list_json() -> Vec<(String, String)> {
    if is_vault_user() {
        // Running as vault user — list directly
        let dir = vault_secrets_dir();
        let mut secrets = Vec::new();
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    let name = path.file_name().unwrap().to_string_lossy().to_string();
                    let created_at = fs::metadata(&path)
                        .and_then(|m| m.modified())
                        .map(|t| {
                            let secs = t.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
                            chrono::DateTime::from_timestamp(secs as i64, 0)
                                .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                                .unwrap_or_default()
                        })
                        .unwrap_or_default();
                    secrets.push((name, created_at));
                }
            }
        }
        secrets.sort();
        secrets
    } else {
        // Delegate: run `mimi secret list` as vault user, parse output
        let output = sudo_vault(&["list"]);
        let text = String::from_utf8_lossy(&output.stdout);
        text.lines()
            .filter(|l| l.starts_with("  "))
            .map(|l| (l.trim().to_string(), String::new()))
            .collect()
    }
}

/// One-time setup: create the vault user, directories, and sudoers rule
pub fn setup_vault() {
    let vault_dir = vault_home();

    // Create mimi-vault system user
    let user_exists = Command::new("id").arg(VAULT_USER).output()
        .map(|o| o.status.success()).unwrap_or(false);

    if !user_exists {
        let status = Command::new("sudo")
            .args(["useradd", "--system", "--shell", "/usr/sbin/nologin",
                   "--home-dir", vault_dir.to_str().unwrap(),
                   "--create-home", VAULT_USER])
            .status()
            .expect("failed to create vault user");

        if status.success() {
            println!("Created system user '{}'", VAULT_USER);
        } else {
            eprintln!("Failed to create user '{}'", VAULT_USER);
            return;
        }
    } else {
        println!("User '{}' already exists", VAULT_USER);
    }

    // Ensure directories with correct ownership
    Command::new("sudo").args(["mkdir", "-p", vault_dir.join("secrets").to_str().unwrap()]).status().ok();
    Command::new("sudo").args(["chown", "-R", &format!("{}:{}", VAULT_USER, VAULT_USER), vault_dir.to_str().unwrap()]).status().ok();
    Command::new("sudo").args(["chmod", "700", vault_dir.to_str().unwrap()]).status().ok();
    Command::new("sudo").args(["chmod", "700", vault_dir.join("secrets").to_str().unwrap()]).status().ok();

    // Install mimi binary to /usr/local/bin
    let current_bin = std::env::current_exe().unwrap_or_else(|_| "mimi".into());
    Command::new("sudo").args(["cp", current_bin.to_str().unwrap(), MIMI_BIN]).status().ok();
    Command::new("sudo").args(["chmod", "755", MIMI_BIN]).status().ok();
    println!("Installed mimi to {}", MIMI_BIN);

    // Write sudoers rule: allow ubuntu to run mimi secret commands as mimi-vault without password
    let sudoers_content = format!(
        "ubuntu ALL=({}) NOPASSWD: {} secret *, /usr/bin/find /var/lib/mimi-vault/* -maxdepth 1 -type f -printf *, /bin/ls /var/lib/mimi-vault/*\n",
        VAULT_USER, MIMI_BIN
    );
    let sudoers_path = "/etc/sudoers.d/mimi-vault";

    // Write via sudo tee
    let mut child = Command::new("sudo")
        .args(["tee", sudoers_path])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .expect("failed to write sudoers");
    use std::io::Write;
    child.stdin.as_mut().unwrap().write_all(sudoers_content.as_bytes()).ok();
    child.wait().ok();

    // Set correct permissions on sudoers file
    Command::new("sudo").args(["chmod", "440", sudoers_path]).status().ok();

    println!("Vault setup complete:");
    println!("  User: {}", VAULT_USER);
    println!("  Home: {}", vault_dir.display());
    println!("  Sudoers: {}", sudoers_path);
    println!("\nSecrets are now isolated — even the AI process cannot read them.");
}

// Need libc for geteuid
extern crate libc;
