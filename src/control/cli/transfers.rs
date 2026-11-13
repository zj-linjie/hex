use super::{ControlCommand, ControlReply, ControlRequest, Output, call, print};
use clap::Subcommand;

#[derive(Subcommand, Debug)]
pub enum TransfersCommand {
    /// Copy a complete queued transfer from an absolute directory.
    Import {
        #[arg(long)]
        dir: std::path::PathBuf,
        #[command(flatten)]
        output: Output,
    },
}

pub(super) fn run(command: TransfersCommand) -> u8 {
    let TransfersCommand::Import { dir, output } = command;
    let reply = call(ControlRequest::new(ControlCommand::TransfersImport { dir }))
        .map_or_else(|reply| reply, |response| response.reply);
    match reply {
        ControlReply::TransferImport { result } => {
            if output.json {
                match serde_json::to_string(&result) {
                    Ok(json) => println!("{json}"),
                    Err(_) => return super::fail("could not serialize transfer result"),
                }
            } else {
                println!("{}", human(&result));
            }
            0
        }
        reply => print(reply, output),
    }
}

pub(super) fn human(result: &crate::transfers::ImportReply) -> &'static str {
    match (result.imported, result.reason.as_deref()) {
        (true, _) => "Transfer received.",
        (_, Some("held")) => "Transfer is waiting for Sync to Mac.",
        (_, Some("already")) => "Transfer is already on this Mac.",
        _ => "Transfer was not imported.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct Cli {
        #[command(subcommand)]
        command: crate::control::cli::ControlCli,
    }

    #[test]
    fn launchd_transfers_command_parses_exactly() -> Result<(), clap::Error> {
        let cli = Cli::try_parse_from([
            "hex",
            "transfers",
            "import",
            "--dir",
            "/tmp/transfer",
            "--json",
        ])?;
        assert!(matches!(cli.command,
            crate::control::cli::ControlCli::Transfers {
                command: TransfersCommand::Import { dir, output: Output { json: true } }
            } if dir == std::path::Path::new("/tmp/transfer")));
        Ok(())
    }
}
