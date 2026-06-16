//! PTY session management. Decoupled from any UI framework via the
//! [`PtyEventSink`] trait: this module spawns a shell, pumps its output and
//! exit status into the sink, and never references Tauri/Iced/etc.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

#[cfg(windows)]
use std::os::windows::io::RawHandle;

use anyhow::{Context, Result};
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};

use crate::shell::ShellProfile;

pub const PTY_READ_BUFFER_BYTES: usize = 32 * 1024;

/// Sink for events produced by a running PTY. Implemented by the active UI
/// adapter (Tauri today, Iced after migration). Must be cheap to clone and
/// `Send` so it can move into the reader/wait threads.
pub trait PtyEventSink: Send + Clone + 'static {
    /// Raw bytes read from the PTY master for `pane_id`.
    fn on_output(&self, pane_id: u32, data: &[u8]);
    /// The shell process for `pane_id` has exited with the given status string.
    fn on_exit(&self, pane_id: u32, status: Option<String>);
}

pub struct TerminalSession {
    pub master: Option<Box<dyn MasterPty + Send>>,
    pub writer: Option<Box<dyn Write + Send>>,
    pub killer: Option<Box<dyn ChildKiller + Send + Sync>>,
    #[cfg(windows)]
    pub job: Option<WinJob>,
    pub process_id: Option<u32>,
    pub cols: u16,
    pub rows: u16,
    pub pixel_width: u16,
    pub pixel_height: u16,
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        self.writer.take();
        if let Some(mut killer) = self.killer.take() {
            let _ = killer.kill();
        }
        thread::sleep(Duration::from_millis(150));
        self.master.take();
        #[cfg(windows)]
        self.job.take();
    }
}

#[cfg(windows)]
pub struct WinJob {
    handle: isize,
}

#[cfg(windows)]
impl Drop for WinJob {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.handle as _);
        }
    }
}

#[cfg(windows)]
fn create_process_job(process: Option<RawHandle>) -> Option<WinJob> {
    use std::ffi::c_void;
    use std::mem::size_of;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    let process = process?;
    unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            return None;
        }
        let mut info = std::mem::zeroed::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let configured = SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &mut info as *mut _ as *mut c_void,
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        ) != 0;
        let assigned = configured && AssignProcessToJobObject(job, process as _) != 0;
        if assigned {
            Some(WinJob {
                handle: job as isize,
            })
        } else {
            windows_sys::Win32::Foundation::CloseHandle(job);
            None
        }
    }
}

pub fn pty_size(cols: u16, rows: u16, pixel_width: u16, pixel_height: u16) -> PtySize {
    PtySize {
        cols: cols.max(1),
        rows: rows.max(1),
        pixel_width: pixel_width.max(cols.max(1)),
        pixel_height: pixel_height.max(rows.max(1)),
    }
}

/// Spawn a shell in a fresh PTY. Output and exit notifications are delivered to
/// `sink`. The reader and wait threads own a clone of the sink for their
/// lifetime.
pub fn spawn_terminal_session<S: PtyEventSink>(
    pane_id: u32,
    shell: ShellProfile,
    cwd: Option<PathBuf>,
    cols: u16,
    rows: u16,
    pixel_width: u16,
    pixel_height: u16,
    sink: S,
) -> Result<TerminalSession> {
    let cols = cols.max(1);
    let rows = rows.max(1);
    let pixel_width = pixel_width.max(cols);
    let pixel_height = pixel_height.max(rows);
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(pty_size(cols, rows, pixel_width, pixel_height))
        .context("failed to open PTY")?;

    let mut command = CommandBuilder::new(shell.program);
    command.args(shell.args);
    command.env_remove("NO_COLOR");
    command.env_remove("CI");
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");
    command.env("TERM_PROGRAM", "vibeterm");
    command.env("TERM_PROGRAM_VERSION", env!("CARGO_PKG_VERSION"));
    command.env("VIBETERM", "1");
    command.env("WT_SESSION", "VibeTerm");
    command.env("ConEmuANSI", "ON");
    command.env("CLICOLOR", "1");
    command.env("CLICOLOR_FORCE", "1");
    command.env("FORCE_COLOR", "3");
    if let Some(cwd) = cwd {
        command.cwd(cwd.as_os_str());
    }

    let mut child = pair
        .slave
        .spawn_command(command)
        .context("failed to spawn shell")?;
    let process_id = child.process_id();
    #[cfg(windows)]
    let job = create_process_job(child.as_raw_handle());
    let killer = child.clone_killer();
    let mut reader = pair
        .master
        .try_clone_reader()
        .context("failed to clone PTY reader")?;
    let writer = pair
        .master
        .take_writer()
        .context("failed to take PTY writer")?;
    let master = pair.master;
    drop(pair.slave);
    master
        .resize(pty_size(cols, rows, pixel_width, pixel_height))
        .context("failed to resize PTY")?;

    let output_sink = sink.clone();
    thread::spawn(move || {
        let mut buffer = vec![0u8; PTY_READ_BUFFER_BYTES];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(size) => {
                    output_sink.on_output(pane_id, &buffer[..size]);
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
    });

    thread::spawn(move || {
        let status = child.wait().ok().map(|status| status.to_string());
        sink.on_exit(pane_id, status);
    });

    Ok(TerminalSession {
        master: Some(master),
        writer: Some(writer),
        killer: Some(killer),
        #[cfg(windows)]
        job,
        process_id,
        cols,
        rows,
        pixel_width,
        pixel_height,
    })
}
