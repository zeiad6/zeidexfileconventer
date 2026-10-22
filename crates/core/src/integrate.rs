//! File-manager integration: "Open with Zeidex" for a selection, in place of macOS Services.
//! Every entry runs `<app> --open <files...>`; the app's single-instance handler takes it from there.

use crate::Result;
use std::path::{Path, PathBuf};

/// (path, contents, executable) for each Linux file manager we support.
pub fn linux_entries(home: &Path, exe: &Path) -> Vec<(PathBuf, String, bool)> {
    let exe = exe.to_string_lossy().replace('"', "\\\"");
    let share = home.join(".local/share");
    vec![
        (
            share.join("nautilus/scripts/Open with Zeidex"),
            format!("#!/bin/sh\nexec \"{exe}\" --open \"$@\"\n"),
            true,
        ),
        (
            share.join("nemo/actions/zeidex.nemo_action"),
            format!("[Nemo Action]\nName=Open with Zeidex\nExec=\"{exe}\" --open %F\nSelection=notnone\nExtensions=any;\n"),
            false,
        ),
        (
            // KF6 only loads service menus that are executable.
            share.join("kio/servicemenus/zeidex.desktop"),
            format!(
                "[Desktop Entry]\nType=Service\nMimeType=all/allfiles;\nActions=zeidex;\nX-KDE-ServiceTypes=KonqPopupMenu/Plugin\n\n\
                 [Desktop Action zeidex]\nName=Open with Zeidex\nIcon=zeidex-convert\nExec=\"{exe}\" --open %F\n"
            ),
            true,
        ),
        (
            share.join("Thunar/sendto/zeidex.desktop"),
            format!(
                "[Desktop Entry]\nType=Application\nName=Open with Zeidex\nExec=\"{exe}\" --open %F\nIcon=document-save-as\nTerminal=false\n"
            ),
            false,
        ),
        (
            home.join(".config/autostart/zeidex.desktop"),
            format!(
                "[Desktop Entry]\nType=Application\nName=Zeidex Convert\nExec=\"{exe}\" --background\nTerminal=false\nX-GNOME-Autostart-enabled=true\n"
            ),
            false,
        ),
    ]
}

/// Installs the integration for the current OS and returns what was written.
pub fn install(exe: &Path) -> Result<Vec<PathBuf>> {
    #[cfg(windows)]
    {
        // A "Send To" shortcut passes the whole selection in one call, unlike a per-file verb.
        let sendto = std::env::var_os("APPDATA")
            .map(|a| PathBuf::from(a).join("Microsoft\\Windows\\SendTo\\Zeidex Convert.lnk"))
            .ok_or_else(|| crate::Error::Unsupported("APPDATA is not set".into()))?;
        let q = |p: &Path| p.to_string_lossy().replace('\'', "''");
        let script = format!(
            "$s=(New-Object -ComObject WScript.Shell).CreateShortcut('{}');$s.TargetPath='{}';$s.Arguments='--open';$s.Save()",
            q(&sendto),
            q(exe)
        );
        let ok = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .status()?
            .success();
        if !ok {
            return Err(crate::Error::Failed {
                tool: "powershell".into(),
                detail: "could not create the shortcut".into(),
            });
        }
        Ok(vec![sendto])
    }
    #[cfg(not(windows))]
    {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| crate::Error::Unsupported("HOME is not set".into()))?;
        let mut written = Vec::new();
        for (path, contents, executable) in linux_entries(&home, exe) {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(&path, contents)?;
            if executable {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
            }
            written.push(path);
        }
        Ok(written)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_quote_the_exe() {
        let e = linux_entries(Path::new("/home/u"), Path::new("/opt/Zeidex Convert/zeidex"));
        assert_eq!(e.len(), 5);
        assert!(e[0].1.contains("exec \"/opt/Zeidex Convert/zeidex\" --open \"$@\""));
        assert!(e[2].0.ends_with("kio/servicemenus/zeidex.desktop") && e[2].2);
        assert!(e[3].0.ends_with("Thunar/sendto/zeidex.desktop") && !e[3].2);
        assert!(e[4].0.ends_with("autostart/zeidex.desktop") && !e[4].2);
    }
}
