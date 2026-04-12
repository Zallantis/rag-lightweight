use clap::Parser;
use rag_lightweight::cli;
use rag_lightweight::config::{Cli, Command};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&cli.log_level)),
        )
        .init();

    match cli.command {
        Command::Serve { host, port } => {
            cli::serve::run(host, port, cli.db_path).await?;
        }
        Command::Ingest {
            path,
            extensions,
            exclude,
            source,
            max_tokens,
        } => {
            cli::ingest::run(path, extensions, exclude, source, max_tokens, cli.db_path).await?;
        }
        Command::Embed { batch_size, force } => {
            cli::embed::run(batch_size, force, cli.db_path).await?;
        }
        Command::Status => {
            cli::status::run(cli.db_path).await?;
        }
    }

    Ok(())
}
