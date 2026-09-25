//! Launch at login through `SMAppService.mainAppService` (macOS 13+), the API the App Store accepts. The Tauri
//! autostart plugin wrote a LaunchAgent plist into ~/Library/LaunchAgents instead, which the App Sandbox forbids;
//! an agent left by it is removed here and its setting carried over.

#[cfg(target_os = "macos")]
mod imp {
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2::msg_send;

    #[link(name = "ServiceManagement", kind = "framework")]
    unsafe extern "C" {}

    /// SMAppServiceStatus.
    const ENABLED: isize = 1;
    const REQUIRES_APPROVAL: isize = 2;

    fn main_app_service() -> Option<&'static AnyObject> {
        let class = AnyClass::get(c"SMAppService")?;
        // SAFETY: public class method returning the shared, never-deallocated main-app service object.
        let service: *mut AnyObject = unsafe { msg_send![class, mainAppService] };
        unsafe { service.as_ref() }
    }

    fn status() -> isize {
        // SAFETY: `status` is a public NSInteger property of SMAppService.
        main_app_service().map(|s| unsafe { msg_send![s, status] }).unwrap_or(0)
    }

    pub fn is_enabled() -> bool {
        matches!(status(), ENABLED | REQUIRES_APPROVAL)
    }

    pub fn set_enabled(enabled: bool) -> Result<(), String> {
        let service = main_app_service().ok_or("Login items are not available")?;
        let mut error: *mut AnyObject = std::ptr::null_mut();
        // SAFETY: public SMAppService methods taking an NSError out-pointer; the error, if any, is autoreleased.
        let ok: bool = unsafe {
            if enabled {
                msg_send![service, registerAndReturnError: &mut error]
            } else {
                msg_send![service, unregisterAndReturnError: &mut error]
            }
        };
        if ok || (!enabled && status() != ENABLED) {
            return Ok(());
        }
        // SAFETY: on failure `error` is an autoreleased NSError; `code` is a public NSInteger property.
        let code: isize = unsafe { error.as_ref() }.map(|e| unsafe { msg_send![e, code] }).unwrap_or(0);
        Err(format!("Couldn't change the login item (error {code}). Check System Settings › General › Login Items."))
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    pub fn is_enabled() -> bool {
        false
    }
    pub fn set_enabled(_: bool) -> Result<(), String> {
        Err("Launch at login is macOS-only for now".into())
    }
}

pub use imp::{is_enabled, set_enabled};

/// The LaunchAgent the old autostart plugin wrote. When present (never in the sandbox, which can't see it), it
/// becomes a login item and the plist is deleted.
pub fn migrate_legacy_agent() {
    let Some(home) = dirs::home_dir() else { return };
    let plist = home.join("Library/LaunchAgents/OpenFlow.plist");
    if !plist.exists() {
        return;
    }
    match set_enabled(true) {
        Ok(()) => {
            let _ = std::fs::remove_file(&plist);
            tracing::info!("Launch at login moved from a LaunchAgent to a login item");
        }
        Err(e) => tracing::warn!("Launch at login migration failed: {e}"),
    }
}
