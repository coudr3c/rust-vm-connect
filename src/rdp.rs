use std::{process::{Command}};

pub fn spawn_rdp() -> Result<std::process::Child, std::io::Error> {
    Command::new("cmd")
        .args(["/C", "echo hello"])
        .spawn()
}