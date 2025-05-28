#![allow(clippy::result_large_err)]
use tokio::sync::oneshot::{Receiver, Sender};
use core::{f32};
use eframe::{egui};
use tokio::sync::oneshot;

mod ssm;
mod rdp;

mod tasks_handler;

mod messages;
use messages::{ ApplicationExitedMessage };

mod errors;

mod utils;
use utils::send_log;

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
                            tasks_handler::start(target,rx_exit, logs_sender)
                        )
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
}
