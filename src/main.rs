// Project: Linux Process Manager
mod agent;
mod alert;
mod container_view;
mod coordinator;
mod criu_manager;
mod filter_parser;
mod graph;
mod gui;
mod namespace_view;
mod process;
mod process_group;
mod process_log;
mod profile;
mod scheduler;
mod scripting_rules;
mod ui;

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
