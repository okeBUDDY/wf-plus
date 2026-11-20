// The application's identity, in one place: the Windows version resource
// (build.rs), the window title and about text, and the Android build read
// it. Keep `VERSION` in step with the crate version (a test checks).

/// The product's name.
pub const NAME: &str = "Warning Forever+";
/// The original's version, which the port carries.
pub const VERSION: [u16; 4] = [1, 0, 8, 0];
pub const VERSION_TEXT: &str = "1.0.8.0";
/// The original's maker (its readme's "Hikware").
pub const COMPANY: &str = "Hikware";
/// The original's own credit line.
pub const AUTHOR: &str = "Hikoza.T.Ohkubo";
pub const COPYRIGHT: &str = "\u{a9} 2003-2012 Hikoza.T.Ohkubo";
/// Who updated the port.
pub const UPDATER: &str = "okeBUDDY";
/// The Windows version resource's Comments.
pub const COMMENTS: &str = "Updated by okeBUDDY";
pub const ORIGINAL_FILENAME: &str = "wf+.exe";
pub const INTERNAL_NAME: &str = "wf+";
/// The resource language: US English.
pub const LANGUAGE: u16 = 0x0409;

/// The Windows application manifest: per-monitor (v2) DPI awareness, no
/// elevation, Windows 10/11. No XML declaration: winresource writes each
/// line padded with spaces, and a declaration must start the document.
pub const WINDOWS_MANIFEST: &str = r#"<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0" xmlns:asmv3="urn:schemas-microsoft-com:asm.v3">
<assemblyIdentity type="win32" name="okeBUDDY.WarningForeverPlus" version="1.0.8.0"/>
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

/// The version as the Windows resource packs it.
pub const fn version_u64() -> u64 {
    (VERSION[0] as u64) << 48 | (VERSION[1] as u64) << 32 | (VERSION[2] as u64) << 16 | VERSION[3] as u64
}
