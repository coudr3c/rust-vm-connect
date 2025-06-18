#![allow(clippy::result_large_err)]
use core::f32;
use eframe::egui;
use std::ffi::{OsStr, OsString};
use std::sync::mpsc::Sender as LogsSender;
use std::{fs, io, path::PathBuf};
use tokio::sync::oneshot;
use tokio::sync::oneshot::Sender;

mod rdp;
mod ssm;

mod tasks_handler;

mod messages;
use messages::ApplicationExitedMessage;

mod errors;

mod utils;
use utils::send_log;

const RDP_EXTENSION: &str = "rdp";
const VM_TARGET_1: &str = "i-0f30a1dd89600b0dc";
const VM_TARGET_2: &str = "i-0a6eb481a98d54b72";

fn read_files_in_directory_with_extension(
    extension: &str,
    logs_sender: &LogsSender<String>,
) -> io::Result<Vec<PathBuf>> {
    let file_paths = fs::read_dir(".")?
        .map(|res| res.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, io::Error>>()?;

    Ok(file_paths
        .into_iter()
        .filter(|path| path.extension().is_some_and(|ext| ext == extension))
        .collect::<Vec<PathBuf>>())
}

fn find_rdp_files(logs_sender: &LogsSender<String>) -> Vec<PathBuf> {
    match read_files_in_directory_with_extension(RDP_EXTENSION, logs_sender) {
        Ok(vec) => vec,
        Err(e) => {
            send_log(
                format!("find_rdp_files error : {}", e.to_string()),
                logs_sender,
            );
            vec![]
        }
    }
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
    selected_rdp_file: Option<PathBuf>,
    join_handler: Option<std::thread::JoinHandle<Result<(), tasks_handler::TaskHandlerError>>>,
    handler_running: bool,
    rdp_files: Vec<PathBuf>,
}

impl Default for EguiApp {
    fn default() -> Self {
        let (logs_sender, logs_receiver) = std::sync::mpsc::channel();
        let rdp_files = find_rdp_files(&logs_sender);
        let first_file_name = rdp_files
            .first()
            .and_then(|path| path.file_name())
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();

        Self {
            username: "Administrator".to_owned(),
            pwd: "".into(),
            disabled: false,
            logs_output: "LOGS :\n".into(),
            application_exit_sender: None,
            logs_receiver,
            logs_sender,
            vm_target: VM_TARGET_1.into(),
            vm1_target: VM_TARGET_1.into(),
            vm2_target: VM_TARGET_2.into(),
            selected_rdp_file: rdp_files.first().map(|path| path.to_owned()),
            join_handler: None,
            handler_running: false,
            rdp_files: rdp_files,
        }
    }
}

impl eframe::App for EguiApp {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        send_log("Egui app : Terminate session...".into(), &self.logs_sender);
        self.application_exit_sender.take().map_or_else(|| send_log("No session to stop".into(), &self.logs_sender) , |tx| {
            tx.send(ApplicationExitedMessage).expect("ApplicationExitedMessage receiver was dropped");
            self.join_handler.take().map_or_else(|| send_log("No thread to stop".into(), &self.logs_sender), |h|  {
                send_log("Egui app : stop app msg sent to handler, waiting for handler thread to stop.".into(), &self.logs_sender);
                let join_result = h.join().expect("Error while joining handler thread");
                if let Err(e) = join_result {
                    send_log("Egui app : ".to_string() + &e.msg, &self.logs_sender);
                }
                send_log("Egui app : Handler thread done".into(), &self.logs_sender);
            });
        });
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Try to see whether task handler is done, if so reactivate app
        let taken_handler = self.join_handler.take_if(|handler| handler.is_finished());

        if taken_handler.is_some() {
            let a = taken_handler
                .unwrap()
                .join()
                .expect("Error while joining handler thread");
            if let Err(e) = a {
                send_log(
                    "Egui app : ".to_string() + &e.kind.to_string() + " : " + &e.msg,
                    &self.logs_sender,
                );
            }
            self.disabled = false;
            self.handler_running = false;
            self.application_exit_sender = None;
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Connection VM SSM");
            // ui.horizontal(|ui| {
            //     let username_label = ui.label("Nom d'Utilisateur VM : ");
            //     ui.text_edit_singleline(&mut self.username)
            //         .labelled_by(username_label.id);
            // });
            // ui.horizontal(|ui| {
            //     ui.label("Mot de passe VM : ");
            //     ui.add(egui::TextEdit::singleline(&mut self.pwd).password(true));
            // });
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.vm_target, self.vm1_target.clone(), "VM 1");
                ui.selectable_value(&mut self.vm_target, self.vm2_target.clone(), "VM 2");
            });
            egui::ComboBox::from_label("Choisir une connection RDP")
                .selected_text(
                    &self
                        .selected_rdp_file.as_ref()
                        .and_then(|path| path.file_name())
                        .and_then(|os_str| os_str.to_str())
                        .map(|s| s.to_string())
                        .unwrap_or_default(),
                )
                .show_ui(ui, |ui| {
                    self.rdp_files
                        .iter()
                        .filter_map(|path| path.file_name().map(|file_name| (path, file_name)))
                        .filter_map(|(path, os_str)| os_str.to_str().map(|s| (path, s)))
                        .for_each(|(path, file_name)| {
                            ui.selectable_value(
                                &mut self.selected_rdp_file,
                                Some(path.to_owned()),
                                file_name,
                            );
                        });
                });
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!self.disabled, egui::Button::new("Connection"))
                    .on_disabled_hover_text("Tunnel déjà lancé")
                    .clicked()
                {
                    let rdp_file_path_str_opt = self.selected_rdp_file.as_ref().and_then(|path| path.to_str()).map(|s| s.to_string());

                    if let Some(rdp_file_path_str) = rdp_file_path_str_opt {

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
                                .block_on(tasks_handler::start(
                                    target,
                                    rdp_file_path_str,
                                    rx_exit,
                                    logs_sender,
                                ))
                        }));

                        self.disabled = true;
                        self.handler_running = true;
                    } else {
                        let logs_sender = self.logs_sender.clone();
                        send_log("GUI : Error while trying to launch connection, file invalid or does not exist".into(), &logs_sender);
                    }
                }
            });
            egui::ScrollArea::vertical()
                .max_width(f32::INFINITY)
                .stick_to_bottom(true)
                .show(ui, |ui| {
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

#[tokio::main]
async fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([500f32, 300f32])
            .with_min_inner_size([400f32, 200f32]),
        ..Default::default()
    };

    eframe::run_native(
        "Connection VM",
        options,
        Box::new(|_cc| Ok(Box::<EguiApp>::default())),
    )
}
