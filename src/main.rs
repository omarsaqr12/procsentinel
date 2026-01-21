// Project: Linux Process Manager
mod process;
mod ui;
mod graph;
mod process_log;
mod scripting_rules;
mod process_group;
mod container_view;
mod namespace_view;
mod scheduler;
mod filter_parser;
mod profile;
mod alert;
mod criu_manager;
mod coordinator;
mod agent;
mod gui;

use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "lpm")]
#[command(about = "Linux Process Manager", long_about = None)]
struct Args {
    /// Run in GUI mode instead of TUI
    #[arg(short, long)]
    gui: bool,

    /// Run as a lightweight agent for remote monitoring
    #[arg(short, long)]
    agent: bool,

    /// Port for the agent to listen on (default: 3000)
    #[arg(short, long, default_value_t = 3000)]
    port: u16,
}

//main to start the application
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    
    if args.agent {
        let agent = agent::Agent::new(args.port);
        agent.start().await?;
        Ok(())
    } else if args.gui {
        gui::run_gui()
    } else {
        ui::ui_renderer()
    }
}

