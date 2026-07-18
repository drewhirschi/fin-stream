use crate::{
    db::AppContext,
    operations::{OperationControl, OperationRepository, utc_now_millis},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperationCommand {
    Status,
    ReadOnly,
    EnableWrites,
    SchedulerOn,
    SchedulerOff,
}

pub async fn execute_operation_command_from_env(
    command: OperationCommand,
) -> anyhow::Result<OperationControl> {
    let context = AppContext::connect_for_operations_from_env().await?;
    let connection = context.connection().await?;
    let repository = OperationRepository::new(&connection);
    let control = match command {
        OperationCommand::Status => repository.control().await?,
        OperationCommand::ReadOnly => repository.enter_read_only(&utc_now_millis()).await?,
        OperationCommand::EnableWrites => repository.enable_writes(&utc_now_millis()).await?,
        OperationCommand::SchedulerOn => {
            repository
                .set_scheduler_enabled(true, &utc_now_millis())
                .await?
        }
        OperationCommand::SchedulerOff => {
            repository
                .set_scheduler_enabled(false, &utc_now_millis())
                .await?
        }
    };
    Ok(control)
}
