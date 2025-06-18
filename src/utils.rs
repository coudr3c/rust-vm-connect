use std::fs::File;
use std::fs::OpenOptions;
use std::io::{BufWriter, Write};

// TODO handle sync
pub fn send_log(s: String, logs_sender: &std::sync::mpsc::Sender<String>) {
    // open file in append mode
    let file_write_res = OpenOptions::new()
        .write(true)
        .create(true)
        .append(true)
        .open("vm-connect-logs.txt")
        .and_then(|mut f| f.write_all(("\n".to_string() + &s).as_bytes()));
    println!("{}", &s);
    logs_sender.send(s + "\n").expect("Error sending log");
}
