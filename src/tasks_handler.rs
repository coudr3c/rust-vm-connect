use crate::ssm::TunnelTaskInstance;
use crate::messages::{ ApplicationExitedMessage, SSMTunnelLaunchedMessage };
use crate::utils::send_log;
use crate::rdp::spawn_rdp;

use tokio::sync::oneshot::{ Sender, Receiver };

pub async fn start(
    target: String,
    mut rx_app_exit: Receiver<ApplicationExitedMessage>,
    logs_sender: std::sync::mpsc::Sender<String>
) {

    let mut tunnel_task_instance = TunnelTaskInstance::spawn(target, logs_sender.clone());

    // Wait for tunnel to be set up
    // take() because when value has been received it is invalidated
    if tunnel_task_instance.tunnel_created_receiver.take().unwrap().await.expect("SSMTunnelLaunchedMessage sender was dropped").ok {
        send_log("Should spawn RDP now, just wait for now".into(), &logs_sender);
    } else {
        send_log("Tunnel start error".into(), &logs_sender);
        return
    }

    send_log("Try to spawn RDP".into(), &logs_sender);
    spawn_rdp().map(async |mut rdp_task| {    
        loop {
            match rx_app_exit.try_recv() {
                // Error => main App is still running
                Err(_) => {
                    // Check whether RDP task is still running
                    if rdp_task.try_wait().is_ok() {
                        send_log("RDP task over, stop tunnel and handler".into(), &logs_sender);
                        break
                    }
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                }

                // App exit message has been received
                _ => {
                    if rdp_task.try_wait().is_err() {
                        rdp_task.kill().expect("Error while trying to kill RDP task");
                    }
                    break
                }
            }
        }
    }).expect("Error while trying to spawn RDP command").await;

    tunnel_task_instance.stop().await;
    send_log("Stop handler".into(), &logs_sender);
}