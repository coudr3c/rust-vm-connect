use std::fmt::{Display, Formatter};
use tokio::sync::oneshot::Receiver;

use crate::messages::ApplicationExitedMessage;
use crate::rdp::{RDPError, RDPTaskInstance};
use crate::ssm::{SSMError, TunnelTaskInstance};
use crate::utils::send_log;

#[derive(Debug)]
pub struct TaskHandlerError {
    pub kind: TaskHandlerErrorKind,
    pub msg: String,
}
#[derive(Debug, Eq, PartialEq)]
#[allow(clippy::upper_case_acronyms)]
pub enum TaskHandlerErrorKind {
    SSM,
    RDP,
    RDPAndSSM,
}

impl Display for TaskHandlerErrorKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SSM => write!(f, "SSM Error"),
            Self::RDP => write!(f, "RDP Error"),
            Self::RDPAndSSM => write!(f, "SSM and RDP Error"),
        }
    }
}

pub async fn start(
    target: String,
    rdp_file_path: String,
    rx_app_exit: Receiver<ApplicationExitedMessage>,
    local_port_number: String,
    logs_sender: std::sync::mpsc::Sender<String>,
) -> Result<(), TaskHandlerError> {
    send_log("Task handler : Starting handler...".into(), &logs_sender);
    let mut tunnel_task_instance =
        TunnelTaskInstance::spawn(target, local_port_number, logs_sender.clone());

    send_log("Task handler : Spawned SSM task".into(), &logs_sender);

    // Wait for tunnel to be set up
    // take() because when value has been received it is invalidated
    if tunnel_task_instance
        .tunnel_created_receiver
        .take()
        .unwrap()
        .await
        .map_err(|err| TaskHandlerError {
            kind: TaskHandlerErrorKind::SSM,
            msg: err.to_string(),
        })?
        .ok
    {
        send_log(
            "Task handler : Should spawn RDP now, just wait for now".into(),
            &logs_sender,
        );
    } else {
        send_log("Task handler : Tunnel start error".into(), &logs_sender);
        return Err(TaskHandlerError {
            kind: TaskHandlerErrorKind::SSM,
            msg: "Failed to start SSM tunnel (task created msg received with ok false)".into(),
        });
    }

    send_log("Task handler : Try to spawn RDP".into(), &logs_sender);

    let rdp_task_instance_result =
        RDPTaskInstance::spawn(rdp_file_path, rx_app_exit, logs_sender.clone());

    let result = match rdp_task_instance_result {
        Ok(mut rdp_task_instance) => {
            let rdp_exit_result = rdp_task_instance.wait_for_exit_or_task_done().await;
            let ssm_exit_result = tunnel_task_instance.stop().await;
            combine_ssm_rdp_results(ssm_exit_result, rdp_exit_result)
        }
        Err(e) => combine_ssm_rdp_results(tunnel_task_instance.stop().await, Err(e)),
    };

    send_log("Task handler : Stop handler".into(), &logs_sender);

    result
}

fn combine_ssm_rdp_errors(ssm_err: SSMError, rdp_err: RDPError) -> TaskHandlerError {
    TaskHandlerError {
        kind: TaskHandlerErrorKind::RDPAndSSM,
        msg: "SSM Error : ".to_string() + &ssm_err.msg + "\nRDP Error : " + &rdp_err.msg,
    }
}

fn combine_ssm_rdp_results(
    ssm_res: Result<(), SSMError>,
    rdp_res: Result<(), RDPError>,
) -> Result<(), TaskHandlerError> {
    match (ssm_res, rdp_res) {
        (Ok(_), Ok(_)) => Ok(()),
        (Ok(_), Err(e)) => Err(transform_rdp_error(e)),
        (Err(e), Ok(_)) => Err(transform_ssm_error(e)),
        (Err(e_ssm), Err(e_rdp)) => Err(combine_ssm_rdp_errors(e_ssm, e_rdp)),
    }
}

fn transform_ssm_error(ssm_err: SSMError) -> TaskHandlerError {
    TaskHandlerError {
        kind: TaskHandlerErrorKind::SSM,
        msg: ssm_err.msg,
    }
}

fn transform_rdp_error(rdp_err: RDPError) -> TaskHandlerError {
    TaskHandlerError {
        kind: TaskHandlerErrorKind::RDP,
        msg: rdp_err.msg,
    }
}

#[cfg(test)]
mod tests {
    // Note this useful idiom: importing names from outer (for mod tests) scope.
    use super::*;

    #[test]
    fn test_task_handler_start_with_wrong_target() {
        let (_, rx) = tokio::sync::oneshot::channel();
        let (logs_sender, _) = std::sync::mpsc::channel();
        let start_res = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(start(
                format!("NO TARGET"),
                format!("NO RDP FILE"),
                rx,
                format!("9090"),
                logs_sender,
            ));
        assert!(
            start_res.is_err_and(|e| e.kind == TaskHandlerErrorKind::SSM),
            "Calling task_handler::start with incorrect target did not return SSM error"
        );
    }
}
