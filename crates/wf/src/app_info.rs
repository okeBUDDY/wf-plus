// The application's identity, in one place: the Windows version resource
// (build.rs), the window title and about text, and the Android build read
// it. Keep `VERSION` in step with the crate version (a test checks).

/// The product's name.
pub const NAME: &str = "Warning Forever+";
/// Warning Forever+'s own version (semantic: major.minor.patch), not the
/// original's: every release is based on the original's last version,
/// [`BASED_ON`]. Shown with the name, never alone.
pub const VERSION: [u16; 3] = [1, 2, 0];
pub const VERSION_TEXT: &str = "1.2.0";
/// The version of Warning Forever this is a port of (its data.bin).
pub const BASED_ON: &str = "1.08";
/// The full version line: "Warning Forever+ 1.2.0 (based on Warning
/// Forever 1.08)".
pub fn version_line() -> String {
    format!("{NAME} {VERSION_TEXT} (based on Warning Forever {BASED_ON})")
}
/// The original's maker (its readme's "Hikware").
pub const COMPANY: &str = "Hikware";
/// The original's own credit line.
pub const AUTHOR: &str = "Hikoza.T.Ohkubo";
pub const COPYRIGHT: &str = "\u{a9} 2003-2012 Hikoza.T.Ohkubo";
/// Who made Warning Forever+.
pub const UPDATER: &str = "okeBUDDY and Claude Opus 5.5";
/// The Windows version resource's Comments.
/// (A literal: keep its "1.08" in step with [`BASED_ON`]; a test checks.)
pub const COMMENTS: &str = "Warning Forever+ by okeBUDDY and Claude Opus 5.5 (based on Warning Forever 1.08)";
pub const ORIGINAL_FILENAME: &str = "wf+.exe";
pub const INTERNAL_NAME: &str = "wf+";
/// The resource language: US English.
pub const LANGUAGE: u16 = 0x0409;

/// The Windows application manifest: per-monitor (v2) DPI awareness, no
/// elevation, Windows 10/11. No XML declaration: winresource writes each
/// line padded with spaces, and a declaration must start the document.
pub const WINDOWS_MANIFEST: &str = r#"<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0" xmlns:asmv3="urn:schemas-microsoft-com:asm.v3">
<assemblyIdentity type="win32" name="okeBUDDY.WarningForeverPlus" version="1.2.0.0"/>
<trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
<security>
<requestedPrivileges>
<requestedExecutionLevel level="asInvoker" uiAccess="false"/>
</requestedPrivileges>
</security>
</trustInfo>
<compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
<application>
<supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
</application>
</compatibility>
<asmv3:application>
<asmv3:windowsSettings>
<dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
<dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
</asmv3:windowsSettings>
</asmv3:application>
</assembly>
"#;

/// The version as the Windows resource packs it (major.minor.patch.0).
pub const fn version_u64() -> u64 {
    (VERSION[0] as u64) << 48 | (VERSION[1] as u64) << 32 | (VERSION[2] as u64) << 16
}
