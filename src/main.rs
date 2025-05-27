#![allow(clippy::result_large_err)]

use aws_config::{meta::region::RegionProviderChain, BehaviorVersion};
use aws_sdk_ssm::{config::{Region}, error::SdkError, operation::{start_session::{StartSessionError, StartSessionOutput}, terminate_session::TerminateSessionOutput}, Client};
use clap::Parser;
use tokio::sync::oneshot::{error::TryRecvError, Receiver, Sender};
use core::{f32};
use std::{io::{BufRead, BufReader}, process::{Command, Stdio}, time::Duration};
use serde::Serialize;
use eframe::{egui};
use tokio::sync::oneshot;

use crate::tunnel_task_instance::TunnelTaskInstance;
use crate::messages::{ ApplicationExitedMessage, SSMTunnelLaunchedMessage };
use crate::errors::{ SSMError, SSMErrorKind };
use crate::utils::send_log;

pub mod tunnel_task_instance;
pub mod messages;
pub mod errors;
pub mod utils;
#[derive(Serialize)]
struct ResponseJson {
    SessionId: String,
    TokenValue: String,
    StreamUrl: String
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
async fn start_session(target: String, client: &Client) -> Result<StartSessionOutput, SdkError<StartSessionError>> {
    // i-0a6eb481a98d54b72 : VM2
    // i-0f30a1dd89600b0dc : VM1
    let resp = client.start_session()
        .target(target)
        .document_name("AWS-StartPortForwardingSession")
        .parameters("localPortNumber", vec!["55678".to_string()])
        .parameters("portNumber", vec!["3389".to_string()])
        .send()
        .await
    ;

    resp
}

// Terminates a SSM session
// snippet-start:[ssm.rust.start-session]
async fn terminate_session(client: &Client, session_id: Option<String>) -> Result<Option<String>, aws_sdk_ssm::Error> {
    let resp = client.terminate_session()
        .set_session_id(session_id)
        .send()
        .await?
    ;

    let session_id: Option<String> = match resp  {
        TerminateSessionOutput {session_id, ..} => session_id
    };

    Ok(session_id)
}

// fn ctrl_channel() -> Result<Receiver<()>, ctrlc::Error> {
//     let (sender, receiver) = bounded(100);
//     ctrlc::set_handler(move || {
//         let _ = sender.send(());
//     })?;

//     Ok(receiver)
// }

async fn initiate_aws_client() -> Client {
    let Opt { region, verbose: _} = Opt::parse();

    let region_provider = RegionProviderChain::first_try(region.map(Region::new))
        .or_default_provider()
        .or_else(Region::new("eu-west-1"));

    let shared_config = aws_config::defaults(BehaviorVersion::latest()).region(region_provider).load().await;
    
    Client::new(&shared_config)
}

async fn initiate_ssm_port_forwarding(start_session_output: &StartSessionOutput) -> Result<std::process::Child, SSMError> {

    // create ssm plugin json message
    let response = ResponseJson {
        SessionId: start_session_output.session_id().unwrap().into(),
        TokenValue: start_session_output.token_value().unwrap().into(), // Assuming `token` is defined elsewhere
        StreamUrl: start_session_output.stream_url().unwrap().into(),
    };

    let response_string = match serde_json::to_string(&response) {
        Ok(res) => res,
        Err(_) => return Err(SSMError { kind: SSMErrorKind::StartSessionError, msg: "Error when attempting to create response string".into() })
    };

    let mut session_manager_plugin = Command::new("session-manager-plugin");
    let run_command_output = session_manager_plugin
        .args([
            response_string,
            "eu-west-1".into(),
            "StartSession".into(),
        ])
        .stdout(Stdio::piped())
        .spawn();

    match run_command_output {
        Ok(c) => Ok(c),
        Err(_) => return Err(SSMError { kind: SSMErrorKind::CommandSpawnError, msg: "Error when attempting to session manager spawn command thread".into() })
    }

}

fn output_tunnel(buf_reader: &mut BufReader<std::process::ChildStdout>, logs_sender: &std::sync::mpsc::Sender<String>) -> bool {
    //let mut buf = [0; 10];

    let mut buf = String::new();

    // Empty line
    buf_reader.read_line(&mut buf).expect("Error while reading tunnel process line");
    // "Starting session"
    buf_reader.read_line(&mut buf).expect("Error while reading tunnel process line");
    //  Port xxx opened
    buf_reader.read_line(&mut buf).expect("Error while reading tunnel process line");
    // Waiting for connections...
    //let mut last_buf = [0; "Waiting for connections...".len()];
    //buf_reader.read_exact(&mut last_buf).expect("Error while reading tunnel process line");
    buf_reader.read_line(&mut buf).expect("Error while reading tunnel process line");
    // Next line is pending, don't need it

    send_log(buf.clone().into(), &logs_sender);
    //println!("{}", std::str::from_utf8(&last_buf).unwrap());

    buf.contains("Waiting for connections...")
}

fn spawn_rdp() -> Result<std::process::Child, std::io::Error> {
    Command::new("cmd")
        .args(["/C", "echo hello"])
        .spawn()
}

async fn launch_ssm_tunnel(
    vm_target: String,
    tx_tunnel_launched: Sender<SSMTunnelLaunchedMessage>,
    rx_app_exit: Receiver<ApplicationExitedMessage>,
    tx_app_exit_ack: Sender<ApplicationExitedMessage>,
    logs_sender: std::sync::mpsc::Sender<String>) -> Result<(), SSMError> {

    let aws_client = initiate_aws_client().await;

    let start_session_output = match start_session(vm_target, &aws_client).await {
        Ok(s) => s,
        _ => return Err(SSMError { kind: SSMErrorKind::StartSessionError, msg: "Error when starting session".into() })
    };

    let mut tunnel_child = match initiate_ssm_port_forwarding(&start_session_output).await {
        Ok(c) => c,
        Err(e) => panic!("{}", e.msg)
    };

    let stdout = tunnel_child.stdout.take().expect("handle present");
    let mut buf_reader = std::io::BufReader::new(stdout);

    let ret = output_tunnel(&mut buf_reader, &logs_sender);

    tx_tunnel_launched.send(SSMTunnelLaunchedMessage{ok: ret}).expect("SMTunnelLaunchedMessage receiver was dropped");

    rx_app_exit.await.expect("ApplicationExitedMessage sender was dropped (ssm thread)");
    send_log("SSM Tunnel : stop app received".into(), &logs_sender);
    tx_app_exit_ack.send(ApplicationExitedMessage).expect("SSM App exit ack receiver dropped");
    send_log("SSM Tunnel : stop app ack sent, terminating session and stopping".into(), &logs_sender);

    match terminate_session(&aws_client, start_session_output.session_id.clone()).await {
        Ok(_) => send_log("SSM Tunnel : Session terminated".into(), &logs_sender),
        _ => send_log("SSM Tunnel : Error while trying to terminate session".into(), &logs_sender)
    }
    tunnel_child.kill().expect("Error while killing ssm thread");
    send_log("SSM Tunnel : child killed".into(), &logs_sender);

    Ok(())
}

struct EguiApp {
    username: String,
    pwd: String,
    vm_target: String,
    disabled: bool,
    logs_output: String,
    application_exit_sender: Option<Sender<ApplicationExitedMessage>>,
    logs_receiver: std::sync::mpsc::Receiver<String>,
    logs_sender: std::sync::mpsc::Sender<String>,
    vm1_target: String,
    vm2_target: String,
    join_handler: Option<std::thread::JoinHandle<()>>,
    handler_running: bool
}

impl Default for EguiApp {
    fn default() -> Self {

        let (logs_sender, logs_receiver) = std::sync::mpsc::channel();

        Self {
            username: "Administrator".to_owned(),
            pwd: "".into(),
            disabled: false,
            logs_output: "LOGS :\n".into(),
            application_exit_sender: None,
            logs_receiver: logs_receiver,
            logs_sender: logs_sender,
            vm_target: "i-0f30a1dd89600b0dc".into(),
            vm1_target: "i-0f30a1dd89600b0dc".into(),
            vm2_target: "i-0a6eb481a98d54b72".into(),
            join_handler: None,
            handler_running: false
        }
    }
}

impl eframe::App for EguiApp {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        send_log("Terminate session...".into(), &self.logs_sender);
        self.application_exit_sender.take().map_or_else(|| send_log("No session to stop".into(), &self.logs_sender) , |tx| {
            tx.send(ApplicationExitedMessage).expect("ApplicationExitedMessage receiver was dropped");
            self.join_handler.take().map_or_else(|| send_log("No thread to stop".into(), &self.logs_sender), |h|  {
                send_log("Egui app : stop app msg sent to handler, waiting for handler thread to stop.".into(), &self.logs_sender);
                h.join().expect("Error while joining handler thread");
                send_log("Handler thread done".into(), &self.logs_sender);
            });
        });
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {

        let taken_handler = self.join_handler.take_if(|handler| handler.is_finished());

        if taken_handler.is_some() {
            taken_handler.unwrap().join().expect("Error while joining handler thread");
            self.disabled = false;
            self.handler_running = false;
            self.application_exit_sender = None;
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Connection VM SSM");
            ui.horizontal(|ui| {
                let username_label = ui.label("Nom d'Utilisateur VM : ");
                ui.text_edit_singleline(&mut self.username)
                    .labelled_by(username_label.id);
            });
            ui.horizontal(|ui| {
                ui.label("Mot de passe VM : ");
                ui.add(egui::TextEdit::singleline(&mut self.pwd).password(true));
            });
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.vm_target, self.vm1_target.clone(), "VM 1");
                ui.selectable_value(&mut self.vm_target, self.vm2_target.clone(), "VM 2");
            });
            ui.horizontal(|ui| {
                if ui.add_enabled(
                    !self.disabled, 
                    egui::Button::new("Connection")
                )
                .on_disabled_hover_text("Tunnel déjà lancé")
                .clicked() {

                    let (tx_exit, rx_exit) = oneshot::channel();

                    self.application_exit_sender = Some(tx_exit);
                    
                    let target = self.vm_target.clone();
                    let logs_sender = self.logs_sender.clone();

                    // Spawns a thread that spawns a tokio task so that
                    // the gui stays synchronous while still making sure tasks are done
                    self.join_handler = Some(std::thread::spawn(move || {
                        tokio::runtime::Builder::new_multi_thread()
                            .enable_all()
                            .build()
                            .unwrap()
                            .block_on(
                            tasks_handler(target,rx_exit, logs_sender)
                        );
                    }));

                    self.disabled = true;
                    self.handler_running = true;
                }
            }
            );
            egui::ScrollArea::vertical().max_width(f32::INFINITY).stick_to_bottom(true).show(ui, |ui| {
                ui.group(|ui| {

                    let new_log = self.logs_receiver.try_recv().unwrap_or_default();
                    self.logs_output.push_str(&new_log);

                    ui.label(&self.logs_output);
                    ui.set_width(ui.available_width());
                });
            })

        });
    }
}

async fn tasks_handler(
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

// snippet-end:[ssm.rust.describe-parameters]

/// Lists the names of your AWS Systems Manager parameters in the Region.
/// # Arguments
///
/// * `[-r REGION]` - The Region in which the client is created.
///    If not supplied, uses the value of the **AWS_REGION** environment variable.
///    If the environment variable is not set, defaults to **us-west-2**.
/// * `[-v]` - Whether to display additional information.
#[tokio::main]
async fn main() -> eframe::Result {

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([600f32, 800f32]).with_min_inner_size([600f32, 800f32]),
        ..Default::default()
    };

    eframe::run_native(
        "Connection VM",
        options,
        Box::new(|cc| {
            Ok(Box::<EguiApp>::default())
        }),
    )
    //tracing_subscriber::fmt::init();

    
    // Connect to the WebSocket URL
    //let (ws_stream, _) = connect_async(wss_url).await?;
    //let (mut write, _) = ws_stream.split();

    // Send the JSON message to the WebSocket
    //write.send(Message::Text(response_string)).await?;

    //let mut stdout = run_command_output.stdout.as_mut();

    // let ctrl_c_events: Receiver<()> = ctrl_channel().unwrap();

    // loop {
    //     //let mut str = String::new();
    //     //stdout.read_to_string(&mut str);
    //     sleep(time::Duration::from_secs(1));

    //     select! {
    //         recv(ctrl_c_events) -> _ => {
    //             println!();
    //             send_log("Terminate session...".into(), &logs_sender);
    //             run_command_output.kill()?;
    //             break;
    //         }
    //     }
    // }
    
    // send_log("Terminated".into(), &logs_sender);

    // Ok(())
}
