pub fn send_log(s: String, logs_sender: &std::sync::mpsc::Sender<String>) {
    println!("{}", &s);
    logs_sender.send(s + "\n").expect("Error sending log");
}