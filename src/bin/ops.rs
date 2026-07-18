use trust_deeds::{OperationCommand, execute_operation_command_from_env};

const USAGE: &str =
    "usage: trust-deeds-ops <status|read-only|enable-writes|scheduler-on|scheduler-off>";

#[tokio::main]
async fn main() {
    #[cfg(feature = "local-db")]
    dotenvy::dotenv().ok();

    let mut args = std::env::args().skip(1);
    let Some(command) = args.next() else {
        eprintln!("{USAGE}");
        std::process::exit(2);
    };
    if matches!(command.as_str(), "-h" | "--help") {
        println!("{USAGE}");
        return;
    }
    if args.next().is_some() {
        eprintln!("{USAGE}");
        std::process::exit(2);
    }
    let command = match command.as_str() {
        "status" => OperationCommand::Status,
        "read-only" => OperationCommand::ReadOnly,
        "enable-writes" => OperationCommand::EnableWrites,
        "scheduler-on" => OperationCommand::SchedulerOn,
        "scheduler-off" => OperationCommand::SchedulerOff,
        _ => {
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    };

    match execute_operation_command_from_env(command).await {
        Ok(control) => match serde_json::to_string(&control) {
            Ok(output) => println!("{output}"),
            Err(_) => {
                eprintln!("trust-deeds-ops: could not encode operation-control status");
                std::process::exit(1);
            }
        },
        Err(_) => {
            // Database URLs and auth tokens must never be reflected by this
            // cutover tool. Detailed provider errors stay out of CLI output.
            eprintln!(
                "trust-deeds-ops: operation failed; verify database configuration, schema, and connectivity"
            );
            std::process::exit(1);
        }
    }
}
