use crate::messages::{ ApplicationExitedMessage, SSMTunnelLaunchedMessage };
use crate::errors::SSMError;
use crate::utils::send_log;
use crate::launch_ssm_tunnel;

use tokio::sync::oneshot::{ Sender, Receiver };
use tokio::sync::oneshot;



pub struct TunnelTaskInstance {
    pub stop_sender: Sender<ApplicationExitedMessage>,
    pub stop_ack_receiver: Receiver<ApplicationExitedMessage>,
    pub tunnel_created_receiver: Option<Receiver<SSMTunnelLaunchedMessage>>,
    pub task_handler: tokio::task::JoinHandle<Result<(), SSMError>>,
    pub logs_sender: std::sync::mpsc::Sender<String>
}

impl TunnelTaskInstance {
    pub fn spawn(
        target: String,
        logs_sender: std::sync::mpsc::Sender<String>,
    ) -> Self {
        let (tx_tunnel_launched, rx_tunnel_launched) = oneshot::channel();
        let (tx_exit_ssm, rx_exit_ssm) = oneshot::channel();
        let (tx_exit_ssm_ack, rx_exit_ssm_ack) = oneshot::channel();

        let ssm_tunnel_task: tokio::task::JoinHandle<Result<(), SSMError>> = tokio::spawn(
            launch_ssm_tunnel(target, tx_tunnel_launched, rx_exit_ssm, tx_exit_ssm_ack, logs_sender.clone())
        );

        return TunnelTaskInstance {
            stop_sender: tx_exit_ssm,
            stop_ack_receiver: rx_exit_ssm_ack,
            tunnel_created_receiver: Some(rx_tunnel_launched),
            task_handler: ssm_tunnel_task,
            logs_sender: logs_sender
        }
    }

    fn abort(self: &Self) {
        self.task_handler.abort();
    }

    pub async fn stop(self: Self) {
        // Send exit msg to SSM Tunnel
        self.stop_sender.send(ApplicationExitedMessage).expect("ApplicationExitedMessage (SSM Tunnel) receiver was dropped");
        send_log("Handler : SSM Tunnel application exit sent".into(), &self.logs_sender);
        // Wait for SSM tunnel to send ack msg and stop
        self.stop_ack_receiver.await.expect("SSM Tunnel ack sender dropped");
        send_log("Handler : SSM Tunnel Ack received".into(), &self.logs_sender);
        self.task_handler.await.expect("Error while joining ssm tunnel task").expect("Error while running launch ssm tunnel");
    }
}