use std::{process::{Command}};
use tokio::sync::oneshot::Receiver;

use crate::{messages::ApplicationExitedMessage, utils::send_log};

#[derive(Debug)]
pub struct RDPError {
    kind: RDPErrorKind,
    pub msg: String
}

#[derive(Debug)]
enum RDPErrorKind {
    Kill,
    Spawn
}

pub struct RDPTaskInstance {
    receiver_app_exit: Receiver<ApplicationExitedMessage>,
    logs_sender: std::sync::mpsc::Sender<String>,
    pub task_handler: std::process::Child
}

impl RDPTaskInstance {
    pub fn spawn(
        receiver_app_exit: Receiver<ApplicationExitedMessage>,
        logs_sender: std::sync::mpsc::Sender<String>
    ) -> Result<RDPTaskInstance, RDPError> {
        spawn_rdp()
            .map_err(|_| RDPError { kind: RDPErrorKind::Spawn, msg: "Failed to start RDP task".into() })
            .map(|c| {
                RDPTaskInstance {
                    receiver_app_exit,
                    logs_sender,
                    task_handler: c
                }
            })
    }

    pub async fn wait_for_exit_or_task_done(
        &mut self,
    ) -> Result<(), RDPError> {
        loop {
            match self.receiver_app_exit.try_recv() {
                // Error => main App is still running
                Err(_) => {
                    // Check whether RDP task is still running
                    match self.task_handler.try_wait() {
                        Ok(opt) => {
                            if opt.is_some() {
                                send_log("Task handler : RDP task over, stop tunnel and handler".into(), &self.logs_sender);
                            }
                        }
                        Err(_) => {
                            send_log("Task handler : RDP task exited with error, stop tunnel and handler".into(), &self.logs_sender);
                        }
                    }
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                }

                // App exit message has been received
                _ => {
                    if self.task_handler.try_wait().is_err() {
                        return self.task_handler.kill()
                            .map_err(|_| RDPError { kind: RDPErrorKind::Kill, msg: "Error while trying to kill RDP task".into()})
                    }
                }
            }
        }
    }
}

fn spawn_rdp() -> Result<std::process::Child, std::io::Error> {
    Command::new("start")
        .args(["/wait", "mstsc.exe", "/prompt"])
        .spawn()
}