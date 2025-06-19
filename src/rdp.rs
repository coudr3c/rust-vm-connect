use std::os::windows::process::CommandExt;
use std::{path::PathBuf, process::Command};
use tokio::sync::oneshot::Receiver;

use crate::messages::ApplicationExitedMessage;
use crate::utils::{CREATE_NO_WINDOW, send_log};

#[derive(Debug)]
pub struct RDPError {
    kind: RDPErrorKind,
    pub msg: String,
}

#[derive(Debug)]
enum RDPErrorKind {
    Kill,
    Spawn,
}

pub struct RDPTaskInstance {
    receiver_app_exit: Receiver<ApplicationExitedMessage>,
    logs_sender: std::sync::mpsc::Sender<String>,
    pub task_handler: std::process::Child,
}

impl RDPTaskInstance {
    pub fn spawn(
        path: String,
        receiver_app_exit: Receiver<ApplicationExitedMessage>,
        logs_sender: std::sync::mpsc::Sender<String>,
    ) -> Result<RDPTaskInstance, RDPError> {
        spawn_rdp(path, &logs_sender)
            .map_err(|e| RDPError {
                kind: RDPErrorKind::Spawn,
                msg: "Failed to start RDP task, ".to_string() + &e.to_string(),
            })
            .map(|c| RDPTaskInstance {
                receiver_app_exit,
                logs_sender,
                task_handler: c,
            })
    }

    pub async fn wait_for_exit_or_task_done(&mut self) -> Result<(), RDPError> {
        loop {
            match self.receiver_app_exit.try_recv() {
                // Error => main App is still running
                Err(_) => {
                    // Check whether RDP task is still running
                    match self.task_handler.try_wait() {
                        Ok(opt) => {
                            if opt.is_some() {
                                send_log(
                                    "RDP Task Instance : RDP task over, stop tunnel and handler"
                                        .into(),
                                    &self.logs_sender,
                                );
                                break;
                            }
                        }
                        Err(_) => {
                            send_log("RDP Task Instance : RDP task exited with error, stop tunnel and handler".into(), &self.logs_sender);
                            break;
                        }
                    }
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                }

                // App exit message has been received
                _ => {
                    if self.task_handler.try_wait().is_err() {
                        return self.task_handler.kill().map_err(|_| RDPError {
                            kind: RDPErrorKind::Kill,
                            msg: "Error while trying to kill RDP task".into(),
                        });
                    }
                }
            }
        }
        Ok(())
    }
}

fn spawn_rdp(
    path: String,
    logs_sender: &std::sync::mpsc::Sender<String>,
) -> Result<std::process::Child, std::io::Error> {
    send_log(
        format!("RDP Task Instance : Launch RDP for file {}", path),
        logs_sender,
    );
    Command::new("cmd")
        .args(["/c", "start", "/wait", "mstsc", &path])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
}
