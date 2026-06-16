//! Shell discovery: enumerate available shell programs for the current OS.

use std::env;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct ShellProfile {
    pub program: String,
    pub args: Vec<String>,
}

pub fn available_shell_profiles() -> Vec<ShellProfile> {
    let mut profiles = Vec::new();

    if cfg!(windows) {
        push_if_available(&mut profiles, "pwsh.exe", ["-NoLogo"]);
        push_if_available(&mut profiles, "powershell.exe", ["-NoLogo"]);
        push_if_available(&mut profiles, "cmd.exe", []);
        push_if_available(
            &mut profiles,
            r"C:\Program Files\Git\bin\bash.exe",
            ["--login"],
        );
    } else {
        if let Ok(shell) = env::var("SHELL") {
            profiles.push(ShellProfile {
                program: shell,
                args: Vec::new(),
            });
        }
        push_if_available(&mut profiles, "/bin/bash", ["--login"]);
        push_if_available(&mut profiles, "/bin/zsh", ["-l"]);
        push_if_available(&mut profiles, "/usr/bin/fish", ["-l"]);
        push_if_available(&mut profiles, "/bin/sh", []);
    }

    if profiles.is_empty() {
        profiles.push(ShellProfile {
            program: if cfg!(windows) {
                env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_owned())
            } else {
                "/bin/sh".to_owned()
            },
            args: Vec::new(),
        });
    }

    profiles
}

fn push_if_available<const N: usize>(
    profiles: &mut Vec<ShellProfile>,
    program: &str,
    args: [&str; N],
) {
    if program_available(program) {
        profiles.push(ShellProfile {
            program: program.to_owned(),
            args: args.into_iter().map(str::to_owned).collect(),
        });
    }
}

fn program_available(program: &str) -> bool {
    if Path::new(program).is_file() {
        return true;
    }

    if program.contains(['/', '\\']) {
        return false;
    }

    env::var_os("PATH")
        .map(|paths| env::split_paths(&paths).any(|path| path.join(program).is_file()))
        .unwrap_or(false)
}
