use std::path::Path;

const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "WattSeal";

/// Returns whether this platform supports native login startup registration.
pub const fn is_supported() -> bool {
    cfg!(target_os = "windows")
}

/// Returns whether WattSeal currently has a per-user Windows startup entry.
#[cfg(target_os = "windows")]
pub fn is_enabled() -> bool {
    registry_command()
        .args(["query", RUN_KEY, "/v", VALUE_NAME])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// Enables or disables per-user startup.
///
/// Enabling registers the current executable with `--background`, reusing WattSeal's
/// existing tray-only startup mode. The HKCU Run key does not require administrator
/// privileges.
#[cfg(target_os = "windows")]
pub fn set_enabled(enabled: bool) -> Result<(), String> {
    if enabled {
        let exe = std::env::current_exe().map_err(|e| format!("Unable to locate WattSeal executable: {e}"))?;
        let startup_command = startup_command_for_exe(&exe);
        let status = registry_command()
            .args([
                "add",
                RUN_KEY,
                "/v",
                VALUE_NAME,
                "/t",
                "REG_SZ",
                "/d",
                &startup_command,
                "/f",
            ])
            .status()
            .map_err(|e| format!("Unable to register WattSeal startup entry: {e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err("Windows rejected the WattSeal startup registry entry".to_string())
        }
    } else {
        if !is_enabled() {
            return Ok(());
        }
        let status = registry_command()
            .args(["delete", RUN_KEY, "/v", VALUE_NAME, "/f"])
            .status()
            .map_err(|e| format!("Unable to remove WattSeal startup entry: {e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err("Windows rejected removal of the WattSeal startup registry entry".to_string())
        }
    }
}

#[cfg(target_os = "windows")]
fn startup_command_for_exe(exe: &Path) -> String {
    format!("\"{}\" --background", exe.display())
}

#[cfg(target_os = "windows")]
fn registry_command() -> std::process::Command {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let mut command = std::process::Command::new("reg.exe");
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

#[cfg(not(target_os = "windows"))]
pub fn is_enabled() -> bool {
    false
}

#[cfg(not(target_os = "windows"))]
pub fn set_enabled(_enabled: bool) -> Result<(), String> {
    Err("Native startup registration is currently implemented for Windows only".to_string())
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    #[test]
    fn startup_command_quotes_executable_and_uses_background_mode() {
        let command = startup_command_for_exe(Path::new(r"C:\Program Files\WattSeal\WattSeal.exe"));
        assert_eq!(
            command,
            r#""C:\Program Files\WattSeal\WattSeal.exe" --background"#
        );
    }
}
