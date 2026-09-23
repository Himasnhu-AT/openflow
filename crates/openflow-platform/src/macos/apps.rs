//! Running and installed apps (NSWorkspace), their icons as PNG data URLs, and their window titles (AX).

use std::collections::HashMap;
use std::sync::Mutex;

use base64::Engine;
use openflow_core::model::AppInfo;
use openflow_core::traits::Apps;
use objc2::rc::Retained;
use objc2::AllocAnyThread;
use objc2_app_kit::{
    NSApplicationActivationPolicy, NSBitmapImageFileType, NSBitmapImageRep, NSImage, NSRunningApplication,
    NSWorkspace,
};
use objc2_core_foundation::{CFRetained, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGBitmapContextCreateImage, CGColorSpace, CGContext, CGImage, CGImageAlphaInfo,
    CGInterpolationQuality,
};
use objc2_foundation::{NSDictionary, NSFileManager, NSPoint, NSRect, NSSize, NSString};

const OWN_BUNDLE: &str = "com.himanshuat.openflow";
const ICON_SIZE: f64 = 64.0;

pub(crate) struct Front {
    pub pid: i32,
    pub bundle_id: String,
    pub name: String,
}

pub(crate) fn frontmost() -> Option<Front> {
    let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
    Some(Front {
        pid: app.processIdentifier(),
        bundle_id: app.bundleIdentifier().map(|s| s.to_string()).unwrap_or_default(),
        name: app.localizedName().map(|s| s.to_string()).unwrap_or_default(),
    })
}

/// PNG data URL of an image, drawn at 64×64 points.
fn png_data_url(image: &NSImage) -> Option<String> {
    let mut rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(ICON_SIZE, ICON_SIZE));
    let cg = unsafe { image.CGImageForProposedRect_context_hints(&mut rect, None, None) }?;
    // The best representation is usually the @2x one: scale it down to 64 px.
    let small = if CGImage::width(Some(&cg)) > ICON_SIZE as usize { scaled(&cg) } else { None };
    let rep = NSBitmapImageRep::initWithCGImage(NSBitmapImageRep::alloc(), small.as_deref().unwrap_or(&cg));
    let props = NSDictionary::new();
    let data = unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &props) }?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(data.to_vec());
    Some(format!("data:image/png;base64,{b64}"))
}

fn scaled(image: &CGImage) -> Option<CFRetained<CGImage>> {
    let side = ICON_SIZE as usize;
    let space = CGColorSpace::new_device_rgb()?;
    let ctx = unsafe {
        CGBitmapContextCreate(
            std::ptr::null_mut(),
            side,
            side,
            8,
            0,
            Some(&space),
            CGImageAlphaInfo::PremultipliedLast.0,
        )
    }?;
    CGContext::set_interpolation_quality(Some(&ctx), CGInterpolationQuality::High);
    let rect = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(ICON_SIZE, ICON_SIZE));
    CGContext::draw_image(Some(&ctx), rect, Some(image));
    CGBitmapContextCreateImage(Some(&ctx))
}

fn app_url_path(bundle_id: &str) -> Option<Retained<NSString>> {
    let url = NSWorkspace::sharedWorkspace().URLForApplicationWithBundleIdentifier(&NSString::from_str(bundle_id))?;
    url.path()
}

/// Theme.swift `appDisplayName`: the installed app's display name without ".app", else the last id component.
pub fn display_name(bundle_id: &str) -> String {
    if let Some(path) = app_url_path(bundle_id) {
        return NSFileManager::defaultManager().displayNameAtPath(&path).to_string().replace(".app", "");
    }
    bundle_id.rsplit('.').next().unwrap_or(bundle_id).to_string()
}

#[derive(Default)]
pub struct MacApps {
    /// Icons are expensive to render; cached per bundle id (None = no icon available).
    icons: Mutex<HashMap<String, Option<String>>>,
}

impl MacApps {
    fn icon_for(&self, bundle_id: &str, running: Option<&NSRunningApplication>) -> Option<String> {
        if let Some(hit) = self.icons.lock().unwrap().get(bundle_id) {
            return hit.clone();
        }
        let image = running.and_then(|a| a.icon()).or_else(|| {
            let path = app_url_path(bundle_id)?;
            Some(NSWorkspace::sharedWorkspace().iconForFile(&path))
        });
        let icon = image.and_then(|i| png_data_url(&i));
        self.icons.lock().unwrap().insert(bundle_id.to_string(), icon.clone());
        icon
    }
}

fn running_regular() -> Vec<Retained<NSRunningApplication>> {
    NSWorkspace::sharedWorkspace()
        .runningApplications()
        .iter()
        .filter(|a| {
            a.activationPolicy() == NSApplicationActivationPolicy::Regular
                && !a.isTerminated()
                && a.bundleIdentifier().is_some_and(|b| b.to_string() != OWN_BUNDLE)
        })
        .collect()
}

impl Apps for MacApps {
    /// Regular (Dock) apps other than OpenFlow, sorted by name (Pages.swift `runningApps`).
    fn running(&self) -> Vec<AppInfo> {
        let mut out: Vec<AppInfo> = running_regular()
            .iter()
            .map(|a| {
                let bundle_id = a.bundleIdentifier().map(|s| s.to_string()).unwrap_or_default();
                AppInfo {
                    icon: self.icon_for(&bundle_id, Some(a)),
                    name: a.localizedName().map(|s| s.to_string()).unwrap_or_default(),
                    bundle_id,
                }
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    /// Running or installed app. None when the bundle id is neither.
    fn info(&self, bundle_id: &str) -> Option<AppInfo> {
        let running = NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str(bundle_id));
        let first = running.iter().next();
        if first.is_none() && app_url_path(bundle_id).is_none() {
            return None;
        }
        let name = match &first {
            Some(a) => a.localizedName().map(|s| s.to_string()).unwrap_or_else(|| display_name(bundle_id)),
            None => display_name(bundle_id),
        };
        Some(AppInfo { bundle_id: bundle_id.to_string(), name, icon: self.icon_for(bundle_id, first.as_deref()) })
    }

    fn window_titles(&self, bundle_id: &str) -> Vec<String> {
        NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str(bundle_id))
            .iter()
            .flat_map(|a| super::ax::window_titles(a.processIdentifier()))
            .collect()
    }
}
