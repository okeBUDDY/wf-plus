// The Windows version resource, icon and manifest, from src/app_info.rs.
// Included by build.rs (Windows targets) and usable by a check tool that
// writes and compiles the .rc elsewhere.

/// The resource for `wf+.exe`; `ico` is the icon file.
pub fn windows_resource(ico: &str) -> winresource::WindowsResource {
    use app_info::*;
    let mut res = winresource::WindowsResource::new();
    res.set_icon(ico)
        .set_language(LANGUAGE)
        .set_version_info(winresource::VersionInfo::FILEVERSION, version_u64())
        .set_version_info(winresource::VersionInfo::PRODUCTVERSION, version_u64())
        .set("FileVersion", VERSION_TEXT)
        .set("ProductVersion", VERSION_TEXT)
        .set("ProductName", NAME)
        .set("FileDescription", NAME)
        .set("CompanyName", COMPANY)
        .set("LegalCopyright", COPYRIGHT)
        .set("Comments", COMMENTS)
        .set("OriginalFilename", ORIGINAL_FILENAME)
        .set("InternalName", INTERNAL_NAME)
        .set_manifest(WINDOWS_MANIFEST);
    res
}
