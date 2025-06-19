use aws_config::{BehaviorVersion, meta::region::RegionProviderChain};
use aws_sdk_ssm::operation::start_session::StartSessionError;
use aws_sdk_ssm::{
    Client,
    config::Region,
    error::SdkError,
    operation::{start_session::StartSessionOutput, terminate_session::TerminateSessionOutput},
};
use clap::Parser;
use serde::Serialize;
use std::io::Read;
use std::os::windows::process::CommandExt;
use std::{
    io::{BufRead, BufReader},
    process::{Command, Stdio},
};
use tokio::sync::oneshot;
use tokio::sync::oneshot::error::RecvError;
use tokio::sync::oneshot::{Receiver, Sender};

use crate::messages::{ApplicationExitedMessage, SSMTunnelLaunchedMessage};
use crate::utils::{CREATE_NO_WINDOW, send_log};

const LOCAL_PORT_NUMBER: &str = "9090";
const REMOTE_PORT_NUMBER: &str = "3389";

#[derive(Debug)]
pub struct SSMError {
    pub kind: SSMErrorKind,
    pub msg: String,
}

#[derive(Debug)]
pub enum SSMErrorKind {
    StartSession,
    CommandSpawn,
    Serde,
    IO,
    TerminateSession,
    TokioChannel,
    CommandKill,
}

pub struct TunnelTaskInstance {
    pub stop_sender: Sender<ApplicationExitedMessage>,
    pub stop_ack_receiver: Receiver<ApplicationExitedMessage>,
    pub tunnel_created_receiver: Option<Receiver<SSMTunnelLaunchedMessage>>,
    pub task_handler: tokio::task::JoinHandle<Result<(), SSMError>>,
    pub logs_sender: std::sync::mpsc::Sender<String>,
}

impl TunnelTaskInstance {
    pub fn spawn(
        target: String,
        local_port_number: String,
        logs_sender: std::sync::mpsc::Sender<String>,
    ) -> Self {
        send_log(
            "TunnelTaskInstance : Starting instance...".into(),
            &logs_sender,
        );
        let (tx_tunnel_launched, rx_tunnel_launched) = oneshot::channel();
        let (tx_exit_ssm, rx_exit_ssm) = oneshot::channel();
        let (tx_exit_ssm_ack, rx_exit_ssm_ack) = oneshot::channel();

        send_log(
            "TunnelTaskInstance : Spawning tunnel...".into(),
            &logs_sender,
        );

        let ssm_tunnel_task: tokio::task::JoinHandle<Result<(), SSMError>> =
            tokio::spawn(launch_ssm_tunnel(
                target,
                tx_tunnel_launched,
                rx_exit_ssm,
                tx_exit_ssm_ack,
                local_port_number,
                logs_sender.clone(),
            ));

        send_log("TunnelTaskInstance : Tunnel spawned".into(), &logs_sender);

        TunnelTaskInstance {
            stop_sender: tx_exit_ssm,
            stop_ack_receiver: rx_exit_ssm_ack,
            tunnel_created_receiver: Some(rx_tunnel_launched),
            task_handler: ssm_tunnel_task,
            logs_sender,
        }
    }

    pub async fn stop(self) -> Result<(), SSMError> {
        // Send exit msg to SSM Tunnel
        send(self.stop_sender, ApplicationExitedMessage)?;
        send_log(
            "TunnelTaskInstance : SSM Tunnel application exit sent".into(),
            &self.logs_sender,
        );
        // Wait for SSM tunnel to send ack msg and stop
        receive(self.stop_ack_receiver.await)?;
        send_log(
            "TunnelTaskInstance : SSM Tunnel Ack received".into(),
            &self.logs_sender,
        );
        receive(self.task_handler.await)
    }
}

async fn try_or_terminate_session<T>(
    res: Result<T, SSMError>,
    client: &Client,
    session_id: Option<String>,
) -> Result<T, SSMError> {
    match res {
        Err(e) => terminate_session_with_error(e, client, session_id).await,
        x => x,
    }
}

pub async fn launch_ssm_tunnel(
    vm_target: String,
    tx_tunnel_launched: Sender<SSMTunnelLaunchedMessage>,
    rx_app_exit: Receiver<ApplicationExitedMessage>,
    tx_app_exit_ack: Sender<ApplicationExitedMessage>,
    local_port_number: String,
    logs_sender: std::sync::mpsc::Sender<String>,
) -> Result<(), SSMError> {
    send_log(
        "TunnelTaskInstance/launch_ssm_tunnel : Initiate aws client...".into(),
        &logs_sender,
    );
    let aws_client = initiate_aws_client().await;
    send_log(
        "TunnelTaskInstance/launch_ssm_tunnel : Initiate aws client OK".into(),
        &logs_sender,
    );

    send_log(
        "TunnelTaskInstance/launch_ssm_tunnel : Start session...".into(),
        &logs_sender,
    );
    let start_session_output = match start_session(vm_target, &aws_client, local_port_number).await
    {
        Ok(s) => s,
        _ => {
            return Err(SSMError {
                kind: SSMErrorKind::StartSession,
                msg: "launch_ssm_tunnel : Error when starting session".into(),
            });
        }
    };

    send_log(
        "TunnelTaskInstance/launch_ssm_tunnel : Start session OK".into(),
        &logs_sender,
    );

    send_log(
        "TunnelTaskInstance/launch_ssm_tunnel : Initiate SSM port forwarding...".into(),
        &logs_sender,
    );
    let mut tunnel_child = try_or_terminate_session(
        initiate_ssm_port_forwarding(&start_session_output).await,
        &aws_client,
        start_session_output.session_id.clone(),
    )
    .await?;
    send_log(
        "TunnelTaskInstance/launch_ssm_tunnel : Initiate SSM port forwarding OK".into(),
        &logs_sender,
    );

    /*
     * DEACTIVATED FOR NOW
     */
    // let stdout =
    //     match tunnel_child.stdout.take() {
    //         Some(c) => c,
    //         _ => {
    //             match terminate_session(&aws_client, start_session_output.session_id.clone()).await {
    //             Ok(_) => return Err(SSMError { kind: SSMErrorKind::IO, msg: "launch_ssm_tunnel : Error while attempting to unwrap tunnel child stdout".into() }),
    //             _ => return Err(SSMError { kind: SSMErrorKind::IO, msg: "launch_ssm_tunnel : Error while attempting to unwrap tunnel child stdout AND trying to terminate session".into() })
    //         }
    //         }
    //     };

    //let mut buf_reader = std::io::BufReader::new(stdout);

    // send_log(
    //     "TunnelTaskInstance/launch_ssm_tunnel : Try output tunnel stdout...".into(),
    //     &logs_sender,
    // );

    // output_tunnel(&mut buf_reader, &logs_sender)?;

    // send_log(
    //     "TunnelTaskInstance/launch_ssm_tunnel : Output tunnel stdout OK".into(),
    //     &logs_sender,
    // );
    send_log(
        "TunnelTaskInstance/launch_ssm_tunnel : Send SSMTunnelLaunchedMessage".into(),
        &logs_sender,
    );

    send_or_terminate_session(
        tx_tunnel_launched,
        SSMTunnelLaunchedMessage { ok: true },
        &aws_client,
        start_session_output.session_id.clone(),
    )
    .await?;

    send_log(
        "TunnelTaskInstance/launch_ssm_tunnel : Wait/receive app exit message...".into(),
        &logs_sender,
    );

    receive_or_terminate_session(
        rx_app_exit.await,
        &aws_client,
        start_session_output.session_id.clone(),
    )
    .await?;

    send_log(
        "TunnelTaskInstance/launch_ssm_tunnel : Wait/receive app exit message OK".into(),
        &logs_sender,
    );

    send_log("SSM Tunnel : stop app received".into(), &logs_sender);

    send_or_terminate_session(
        tx_app_exit_ack,
        ApplicationExitedMessage,
        &aws_client,
        start_session_output.session_id.clone(),
    )
    .await?;

    send_log(
        "SSM Tunnel : stop app ack sent, terminating session and stopping".into(),
        &logs_sender,
    );

    match terminate_session(&aws_client, start_session_output.session_id.clone()).await {
        Ok(_) => send_log("SSM Tunnel : Session terminated".into(), &logs_sender),
        _ => send_log(
            "SSM Tunnel : Error while trying to terminate session".into(),
            &logs_sender,
        ),
    }

    match tunnel_child.kill() {
        Ok(_) => Ok(()),
        _ => Err(SSMError {
            kind: SSMErrorKind::CommandKill,
            msg: "Unable to kill tunnel command task".into(),
        }),
    }?;

    send_log("SSM Tunnel : child killed".into(), &logs_sender);

    Ok(())
}

fn receive<T, E>(res: Result<T, E>) -> Result<(), SSMError> {
    res.map_err(|_| SSMError {
        kind: SSMErrorKind::TokioChannel,
        msg: "Unable to receive message, channel down".into(),
    })
    .map(|_| ())
}

async fn receive_or_terminate_session<T>(
    res: Result<T, RecvError>,
    client: &Client,
    session_id: Option<String>,
) -> Result<(), SSMError> {
    let recv_res = receive(res);
    match recv_res {
        Err(e) => terminate_session_with_error(e, client, session_id).await,
        Ok(_) => Ok(()),
    }
}

fn send<T>(tx: Sender<T>, content: T) -> Result<(), SSMError> {
    tx.send(content).map_err(|_e| SSMError {
        kind: SSMErrorKind::TokioChannel,
        msg: "Unable to send message, channel down".into(),
    })
}

async fn send_or_terminate_session<T>(
    tx: Sender<T>,
    content: T,
    client: &Client,
    session_id: Option<String>,
) -> Result<(), SSMError> {
    let res = send(tx, content);
    match res {
        Err(e) => terminate_session_with_error(e, client, session_id).await,
        Ok(c) => Ok(c),
    }
}

async fn initiate_aws_client() -> Client {
    let Opt { region, verbose: _ } = Opt::parse();

    let region_provider = RegionProviderChain::first_try(region.map(Region::new))
        .or_default_provider()
        .or_else(Region::new("eu-west-1"));

    let shared_config = aws_config::defaults(BehaviorVersion::latest())
        .region(region_provider)
        .load()
        .await;

    Client::new(&shared_config)
}

#[derive(Debug, Parser)]
struct Opt {
    /// The AWS Region.
    #[structopt(short, long)]
    region: Option<String>,

    /// Whether to display additional information.
    #[structopt(short, long)]
    verbose: bool,
}

// Starts a SSM session
// snippet-start:[ssm.rust.start-session]
async fn start_session(
    target: String,
    client: &Client,
    local_port_number: String,
) -> Result<StartSessionOutput, SdkError<StartSessionError>> {
    client
        .start_session()
        .target(target)
        .document_name("AWS-StartPortForwardingSession")
        .parameters("localPortNumber", vec![local_port_number])
        .parameters("portNumber", vec![REMOTE_PORT_NUMBER.to_string()])
        .send()
        .await
}

async fn initiate_ssm_port_forwarding(
    start_session_output: &StartSessionOutput,
) -> Result<std::process::Child, SSMError> {
    // create ssm plugin json message
    let response = ResponseJson {
        SessionId: start_session_output.session_id().unwrap().into(),
        TokenValue: start_session_output.token_value().unwrap().into(), // Assuming `token` is defined elsewhere
        StreamUrl: start_session_output.stream_url().unwrap().into(),
    };

    let response_string =
        match serde_json::to_string(&response) {
            Ok(res) => res,
            Err(_) => return Err(SSMError {
                kind: SSMErrorKind::Serde,
                msg:
                    "initiate_ssm_port_forwarding : Error when attempting to create response string"
                        .into(),
            }),
        };

    let mut session_manager_plugin = Command::new("session-manager-plugin");
    let run_command_output = session_manager_plugin
        .args([response_string, "eu-west-1".into(), "StartSession".into()])
        .creation_flags(CREATE_NO_WINDOW)
        //.stdout(Stdio::piped())
        .spawn();

    match run_command_output {
        Ok(c) => Ok(c),
        Err(_) => Err(SSMError { kind: SSMErrorKind::CommandSpawn, msg: "initiate_ssm_port_forwarding : Error when attempting to spawn session manager plugin command thread".into() })
    }
}

/**
 * Wonky stuff, if AWS SSM changes its log output, it might make the following break
 */
fn output_tunnel(
    buf_reader: &mut BufReader<std::process::ChildStdout>,
    logs_sender: &std::sync::mpsc::Sender<String>,
) -> Result<(), SSMError> {
    send_log(
        "TunnelTaskInstance/output_tunnel : Read buffer...".into(),
        &logs_sender,
    );

    for _ in 1..5 {
        let mut buf = String::new();
        buf_reader.read_line(&mut buf).map_err(|_| SSMError {
            kind: SSMErrorKind::IO,
            msg: "output_tunnel : Error while reading tunnel process line".into(),
        })?;

        if buf.contains("Waiting for connections...") {
            send_log(
                "TunnelTaskInstance/output_tunnel : Read buffer OK".into(),
                &logs_sender,
            );
            send_log(buf, logs_sender);
            return Ok(());
        }
        send_log(buf, logs_sender);
    }

    Err(SSMError {
        kind: SSMErrorKind::IO,
        msg: "output_tunnel : Error while reading tunnel process line".into(),
    })
}

// Terminates a SSM session
// snippet-start:[ssm.rust.start-session]
async fn terminate_session(
    client: &Client,
    session_id: Option<String>,
) -> Result<Option<String>, aws_sdk_ssm::Error> {
    let resp = client
        .terminate_session()
        .set_session_id(session_id)
        .send()
        .await?;

    let TerminateSessionOutput { session_id, .. } = resp;

    Ok(session_id)
}

async fn terminate_session_with_error<T>(
    err: SSMError,
    client: &Client,
    session_id: Option<String>,
) -> Result<T, SSMError> {
    match terminate_session(client, session_id).await {
        Ok(_) => Err(err),
        Err(_) => {
            let msg =
                "terminate_session_with_error : Error while trying to terminate SSM session AND "
                    .to_string()
                    + &err.msg;
            Err(SSMError {
                kind: SSMErrorKind::TerminateSession,
                msg,
            })
        }
    }
}

#[derive(Serialize)]
struct ResponseJson {
    SessionId: String,
    TokenValue: String,
    StreamUrl: String,
}
