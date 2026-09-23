//! Accessibility (AX) field reader: FieldContext.swift `capture` / `replace`, plus window titles for call
//! detection and the Accessibility trust check.

use std::ffi::c_void;
use std::ptr::NonNull;

use jez_core::model::AppInfo;
use jez_core::traits::FieldReader;
use jez_core::FieldContext;
use objc2_application_services::{
    kAXTrustedCheckOptionPrompt, AXError, AXIsProcessTrustedWithOptions, AXUIElement, AXValue, AXValueType,
};
use objc2_core_foundation::{CFArray, CFBoolean, CFDictionary, CFRange, CFRetained, CFString, CFType};

use crate::text16::{context_from_value, truncate_title};

const FOCUSED_WINDOW: &str = "AXFocusedWindow";
const FOCUSED_ELEMENT: &str = "AXFocusedUIElement";
const TITLE: &str = "AXTitle";
const VALUE: &str = "AXValue";
const SELECTED_RANGE: &str = "AXSelectedTextRange";
const SELECTED_TEXT: &str = "AXSelectedText";
const WINDOWS: &str = "AXWindows";

pub(crate) fn app_element(pid: i32) -> CFRetained<AXUIElement> {
    unsafe { AXUIElement::new_application(pid) }
}

pub(crate) fn attribute(element: &AXUIElement, name: &str) -> Option<CFRetained<CFType>> {
    let key = CFString::from_str(name);
    let mut out: *const CFType = std::ptr::null();
    let err = unsafe { element.copy_attribute_value(&key, NonNull::from(&mut out)) };
    if err != AXError::Success {
        return None;
    }
    NonNull::new(out as *mut CFType).map(|p| unsafe { CFRetained::from_raw(p) })
}

pub(crate) fn attribute_string(element: &AXUIElement, name: &str) -> Option<String> {
    attribute(element, name)?.downcast::<CFString>().ok().map(|s| s.to_string())
}

fn attribute_element(element: &AXUIElement, name: &str) -> Option<CFRetained<AXUIElement>> {
    attribute(element, name)?.downcast::<AXUIElement>().ok()
}

fn attribute_range(element: &AXUIElement, name: &str) -> Option<CFRange> {
    let value = attribute(element, name)?.downcast::<AXValue>().ok()?;
    let mut range = CFRange { location: 0, length: 0 };
    let ok = unsafe { value.value(AXValueType::CFRange, NonNull::from(&mut range).cast::<c_void>()) };
    ok.then_some(range)
}

fn is_settable(element: &AXUIElement, name: &str) -> bool {
    let key = CFString::from_str(name);
    let mut settable: u8 = 0;
    let err = unsafe { element.is_attribute_settable(&key, NonNull::from(&mut settable)) };
    err == AXError::Success && settable != 0
}

fn set_range(element: &AXUIElement, location: usize, length: usize) -> bool {
    let mut range = CFRange { location: location as isize, length: length as isize };
    let Some(value) = (unsafe { AXValue::new(AXValueType::CFRange, NonNull::from(&mut range).cast::<c_void>()) })
    else {
        return false;
    };
    let key = CFString::from_str(SELECTED_RANGE);
    unsafe { element.set_attribute_value(&key, &value) == AXError::Success }
}

/// Titles of all windows of the app with this pid.
pub(crate) fn window_titles(pid: i32) -> Vec<String> {
    let app = app_element(pid);
    let Some(windows) = attribute(&app, WINDOWS).and_then(|w| w.downcast::<CFArray>().ok()) else {
        return Vec::new();
    };
    // SAFETY: kAXWindows is documented to hold AXUIElements; each is still checked by downcast.
    let windows: CFRetained<CFArray<CFType>> = unsafe { CFRetained::cast_unchecked(windows) };
    windows
        .to_vec()
        .into_iter()
        .filter_map(|w| w.downcast::<AXUIElement>().ok())
        .filter_map(|w| attribute_string(&w, TITLE))
        .collect()
}

pub(crate) fn is_trusted(prompt: bool) -> bool {
    let key: &CFString = unsafe { kAXTrustedCheckOptionPrompt };
    let value = CFBoolean::new(prompt);
    let dict = CFDictionary::<CFString, CFBoolean>::from_slices(&[key], &[value]);
    unsafe { AXIsProcessTrustedWithOptions(Some(dict.as_opaque())) }
}

pub struct AxFieldReader;

impl FieldReader for AxFieldReader {
    fn capture(&self) -> FieldContext {
        let Some(front) = super::apps::frontmost() else { return FieldContext::default() };
        let app = app_element(front.pid);
        let title = attribute_element(&app, FOCUSED_WINDOW)
            .and_then(|w| attribute_string(&w, TITLE))
            .map(|t| truncate_title(&t))
            .unwrap_or_default();
        let Some(focused) = attribute_element(&app, FOCUSED_ELEMENT) else {
            return FieldContext { window_title: title, ..Default::default() };
        };
        let value = match attribute_string(&focused, VALUE) {
            Some(v) if !v.is_empty() => v,
            _ => return FieldContext { window_title: title, ..Default::default() },
        };
        let location = attribute_range(&focused, SELECTED_RANGE).map(|r| r.location);
        context_from_value(title, &value, location)
    }

    /// Replaces `length` UTF-16 units at `location` in the focused field with `text` through Accessibility, in
    /// one operation and without keystrokes. Only Apple's own text views are trusted with this: browsers and
    /// Electron apps report the attributes as settable but apply them in surprising ways (one app inserted the
    /// text nine times).
    fn replace(&self, location: usize, length: usize, _expecting: &str, text: &str) -> bool {
        let Some(front) = super::apps::frontmost() else { return false };
        if !front.bundle_id.starts_with("com.apple.") {
            return false;
        }
        let app = app_element(front.pid);
        let Some(focused) = attribute_element(&app, FOCUSED_ELEMENT) else { return false };
        if !is_settable(&focused, SELECTED_RANGE) || !is_settable(&focused, SELECTED_TEXT) {
            return false;
        }
        let Some(current) = attribute_string(&focused, VALUE) else { return false };
        let current: Vec<u16> = current.encode_utf16().collect();
        if location + length > current.len() {
            return false;
        }
        let live_units = current[location..location + length].to_vec();
        if !set_range(&focused, location, length) {
            return false;
        }
        let key = CFString::from_str(SELECTED_TEXT);
        let new_text = CFString::from_str(text);
        let inserted = unsafe { focused.set_attribute_value(&key, &new_text) } == AXError::Success;
        // Read back: the text must now sit at `location`.
        let units: Vec<u16> = attribute_string(&focused, VALUE).unwrap_or_default().encode_utf16().collect();
        let t: Vec<u16> = text.encode_utf16().collect();
        let ok = inserted && location + t.len() <= units.len() && units[location..location + t.len()] == t[..];
        if ok {
            set_range(&focused, location + t.len(), 0); // caret after the inserted text
            return true;
        }
        // Undo any half-applied state for the keystroke fallback: caret back at the end of the live text if it is
        // still there, so backspaces remove exactly the live text and nothing else.
        if location + live_units.len() <= units.len() && units[location..location + live_units.len()] == live_units[..] {
            set_range(&focused, location + live_units.len(), 0);
        }
        false
    }

    fn frontmost_app(&self) -> Option<AppInfo> {
        let front = super::apps::frontmost()?;
        Some(AppInfo { bundle_id: front.bundle_id, name: front.name, icon: None })
    }

    fn is_trusted(&self, prompt: bool) -> bool {
        is_trusted(prompt)
    }
}
