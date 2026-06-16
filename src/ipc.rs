//! Single-instance IPC over a local TCP port. The server decodes
//! [`IpcCommand`]s and forwards them to a handler closure, keeping it free of
//! any UI-framework types.

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use crate::protocol::IpcCommand;

pub const IPC_PORT: u16 = 15973;
pub const IPC_HOST: &str = "127.0.0.1";

/// Try to hand a command to an already-running instance. Returns true if a
/// running instance accepted it.
pub fn send_ipc_command(command: &IpcCommand) -> bool {
    let addr = format!("{}:{}", IPC_HOST, IPC_PORT);
    if let Ok(mut stream) = TcpStream::connect_timeout(
        &addr.parse().expect("valid IPC address"),
        Duration::from_millis(500),
    ) {
        let json = serde_json::to_string(command).unwrap_or_default();
        if stream.write_all(json.as_bytes()).is_ok() {
            let _ = stream.shutdown(Shutdown::Write);
            return true;
        }
    }
    false
}

/// Bind the IPC port and forward decoded commands to `handler`. No-op if the
/// port is already taken (another instance owns it).
pub fn start_ipc_server<F>(handler: F)
where
    F: Fn(IpcCommand) + Send + 'static,
{
    let Ok(listener) = TcpListener::bind((IPC_HOST, IPC_PORT)) else {
        return;
    };

    thread::spawn(move || {
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => handle_ipc_stream(stream, &handler),
                Err(error) => {
                    eprintln!("IPC server stopped: {error}");
                    break;
                }
            }
        }
    });
}

fn handle_ipc_stream<F>(mut stream: TcpStream, handler: &F)
where
    F: Fn(IpcCommand) + Send + 'static,
{
    stream
        .set_read_timeout(Some(Duration::from_millis(500)))
        .ok();
    let mut text = String::new();
    if stream.read_to_string(&mut text).is_err() || text.trim().is_empty() {
        return;
    }
    match serde_json::from_str::<IpcCommand>(text.trim()) {
        Ok(command) => handler(command),
        Err(error) => eprintln!("invalid IPC command: {error}"),
    }
}
