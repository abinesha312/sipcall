use anyhow::Result;
use clap::{Parser, Subcommand};

mod sip;
mod rtp;
mod audio;

#[derive(Parser)]
#[command(name = "sipcall")]
#[command(about = "Standalone SIP phone - no hosted services", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[command(about = "Listen for incoming SIP calls")]
    Listen {
        #[arg(long, default_value = "0.0.0.0:5060")]
        bind: String,
    },
    #[command(about = "Make a SIP call to target (sip:user@host:port or host:port)")]
    Call {
        target: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Listen { bind } => {
            println!("Listening for SIP calls on {}", bind);
            sip::listen(&bind).await
        }
        Commands::Call { target } => {
            println!("Calling {}", target);
            sip::call(&target).await
        }
    }
}
