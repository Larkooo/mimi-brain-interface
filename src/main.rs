mod brain;
mod channels;
mod claude;
mod commands;
mod dashboard;
mod paths;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "mimi", about = "Autonomous AI assistant powered by Claude Code")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// First-time setup: initialize ~/.mimi/ and brain.db
    Setup,
    /// Show session status (context length, uptime, model)
    Status,
    /// Start the web dashboard
    Dashboard {
        /// Port to serve on
        #[arg(short, long, default_value = "3131")]
        port: u16,
    },
    /// Query and manage the knowledge graph
    Brain {
        #[command(subcommand)]
        command: BrainCommands,
    },
    /// Manage MCP servers (wraps claude mcp)
    Mcp {
        #[command(subcommand)]
        command: McpCommands,
    },
    /// Manage channels (telegram, imessage, etc.)
    Channel {
        #[command(subcommand)]
        command: ChannelCommands,
    },
    /// Manage plugins (wraps claude plugin)
    Plugin {
        /// Arguments passed to claude plugin
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },
    /// Edit Mimi's config
    Config,
    /// Backup ~/.mimi/ data
    Backup,
    /// Run a self-reflection cycle (prefrontal cortex)
    Reflect,
    /// Audit own codebase and propose improvements via PR
    Audit,
    /// Pull latest master, rebuild, and install the new binary/dashboard
    Update,
    /// Manage encrypted secrets (isolated vault)
    Secret {
        #[command(subcommand)]
        command: SecretCommands,
    },
}

#[derive(Subcommand)]
enum BrainCommands {
    /// Show brain statistics
    Stats,
    /// Run a SQL query against brain.db
    Query {
        /// SQL query to run
        sql: String,
    },
    /// Add an entity
    Add {
        /// Entity type (person, company, service, concept, etc.)
        #[arg(short = 't', long)]
        r#type: String,
        /// Entity name
        #[arg(short, long)]
        name: String,
        /// JSON properties
        #[arg(short, long, default_value = "{}")]
        properties: String,
    },
    /// Link two entities
    Link {
        /// Source entity ID
        source: i64,
        /// Relationship type
        rel: String,
        /// Target entity ID
        target: i64,
    },
    /// Delete an entity (cascades to relationships and memory refs)
    Delete {
        /// Entity ID to delete
        id: i64,
    },
    /// Search entities by text
    Search {
        /// Search query
        query: String,
    },
    /// List entities, optionally filtered by type
    List {
        /// Filter by entity type
        #[arg(short = 't', long)]
        r#type: Option<String>,
    },
}

#[derive(Subcommand)]
enum McpCommands {
    /// List MCP servers
    List,
    /// Add an MCP server
    Add {
        /// Arguments passed to claude mcp add
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },
    /// Remove an MCP server
    Remove {
        /// Server name
        name: String,
    },
}

#[derive(Subcommand)]
enum ChannelCommands {
    /// List configured channels
    List,
    /// Add a channel
    Add {
        /// Channel type (telegram, imessage, etc.)
        r#type: String,
    },
    /// Configure a channel with a bot token
    Configure {
        /// Channel type (telegram, discord, etc.)
        r#type: String,
        /// Bot token
        token: String,
    },
    /// Start a channel bot in the foreground (bridges Telegram ↔ a persistent claude session)
    Start {
        /// Channel type (currently only "telegram")
        r#type: String,
    },
    /// Stop a running channel bot (reads pidfile, sends SIGTERM)
    Stop {
        /// Channel type (currently only "telegram")
        r#type: String,
    },
    /// Remove a channel
    Remove {
        /// Channel name
        name: String,
    },
}

#[derive(Subcommand)]
enum SecretCommands {
    /// Store an encrypted secret
    Set {
        /// Secret name
        name: String,
        /// Secret value
        value: String,
    },
    /// List stored secrets (names only, never values)
    List,
    /// Delete a secret
    Delete {
        /// Secret name
        name: String,
    },
    /// Run a command with a secret injected as an env var
    Run {
        /// Secret name
        name: String,
        /// Environment variable name to inject
        env_var: String,
        /// Command and arguments to run
        #[arg(trailing_var_arg = true)]
        cmd: Vec<String>,
    },
    /// One-time setup: create vault user and directories
    Setup,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match cli.command {
        None => commands::launch::run(),
        Some(Commands::Setup) => commands::setup::run(),
        Some(Commands::Status) => commands::status::run(),
        Some(Commands::Dashboard { port }) => commands::dashboard::run(port).await,
        Some(Commands::Brain { command }) => match command {
            BrainCommands::Stats => commands::brain::stats(),
            BrainCommands::Query { sql } => commands::brain::query(&sql),
            BrainCommands::Add {
                r#type,
                name,
                properties,
            } => commands::brain::add(&r#type, &name, &properties),
            BrainCommands::Delete { id } => commands::brain::delete(id),
            BrainCommands::Link {
                source,
                rel,
                target,
            } => commands::brain::link(source, &rel, target),
            BrainCommands::Search { query } => commands::brain::search(&query),
            BrainCommands::List { r#type } => commands::brain::list(r#type.as_deref()),
        },
        Some(Commands::Mcp { command }) => match command {
            McpCommands::List => claude::mcp(&["list"]),
            McpCommands::Add { args } => {
                let mut cmd_args = vec!["add".to_string()];
                cmd_args.extend(args);
                let refs: Vec<&str> = cmd_args.iter().map(|s| s.as_str()).collect();
                claude::mcp(&refs);
            }
            McpCommands::Remove { name } => claude::mcp(&["remove", &name]),
        },
        Some(Commands::Channel { command }) => match command {
            ChannelCommands::List => commands::channel::list(),
            ChannelCommands::Add { r#type } => {
                if let Err(e) = commands::channel::add(&r#type) {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
            ChannelCommands::Configure { r#type, token } => {
                if let Err(e) = commands::channel::configure(&r#type, &token) {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
            ChannelCommands::Start { r#type } => {
                let result = match r#type.as_str() {
                    "telegram" => channels::telegram::start().await,
                    other => Err(format!("unknown or unsupported channel: {other}")),
                };
                if let Err(e) = result {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
            ChannelCommands::Stop { r#type } => {
                let result = match r#type.as_str() {
                    "telegram" => channels::telegram::stop(),
                    other => Err(format!("unknown or unsupported channel: {other}")),
                };
                if let Err(e) = result {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
            ChannelCommands::Remove { name } => commands::channel::remove(&name),
        },
        Some(Commands::Plugin { args }) => {
            let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            claude::plugin(&refs);
        }
        Some(Commands::Config) => commands::config::run(),
        Some(Commands::Backup) => commands::backup::run(),
        Some(Commands::Reflect) => commands::reflect::run(),
        Some(Commands::Audit) => commands::audit::run(),
        Some(Commands::Update) => commands::update::run(),
        Some(Commands::Secret { command }) => match command {
            SecretCommands::Set { name, value } => commands::secret::set(&name, &value),
            SecretCommands::List => commands::secret::list(),
            SecretCommands::Delete { name } => commands::secret::delete(&name),
            SecretCommands::Run { name, env_var, cmd } => {
                commands::secret::run(&name, &env_var, &cmd);
            }
            SecretCommands::Setup => commands::secret::setup_vault(),
        },
    }
}
