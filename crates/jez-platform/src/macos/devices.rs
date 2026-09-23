//! Core Audio input devices (AudioDevices.swift), lid state (IOKit) and "is the mic in use" (for call detection).

use std::ffi::{c_char, c_void, CStr};
use std::ptr::NonNull;

use jez_core::model::InputDevice;
use jez_core::traits::AudioDevices;
use objc2_core_audio::{
    kAudioDevicePropertyDeviceIsRunningSomewhere, kAudioDevicePropertyDeviceUID,
    kAudioDevicePropertyStreamConfiguration, kAudioDevicePropertyTransportType, kAudioDeviceTransportTypeAggregate,
    kAudioDeviceTransportTypeBuiltIn, kAudioDeviceTransportTypeContinuityCaptureWired,
    kAudioDeviceTransportTypeContinuityCaptureWireless, kAudioDeviceTransportTypeVirtual,
    kAudioHardwarePropertyDefaultInputDevice, kAudioHardwarePropertyDevices, kAudioObjectPropertyElementMain,
    kAudioObjectPropertyName, kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyScopeInput,
    kAudioObjectSystemObject, AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize, AudioObjectID,
    AudioObjectPropertyAddress, AudioObjectPropertyScope, AudioObjectPropertySelector,
};
use objc2_core_audio_types::AudioBufferList;
use objc2_core_foundation::{CFBoolean, CFRetained, CFString, CFType};

/// A Core Audio input with its object id (needed to open it).
#[derive(Debug, Clone, PartialEq)]
pub struct MacInput {
    pub id: AudioObjectID,
    pub device: InputDevice,
}

const SYSTEM: AudioObjectID = kAudioObjectSystemObject as AudioObjectID;

fn address(selector: AudioObjectPropertySelector, scope: AudioObjectPropertyScope) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress { mSelector: selector, mScope: scope, mElement: kAudioObjectPropertyElementMain }
}

fn get_u32(id: AudioObjectID, selector: AudioObjectPropertySelector) -> Option<u32> {
    let mut addr = address(selector, kAudioObjectPropertyScopeGlobal);
    let mut value: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(
            id,
            NonNull::from(&mut addr),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
            NonNull::from(&mut value).cast::<c_void>(),
        )
    };
    (status == 0).then_some(value)
}

fn get_string(id: AudioObjectID, selector: AudioObjectPropertySelector) -> Option<String> {
    let mut addr = address(selector, kAudioObjectPropertyScopeGlobal);
    let mut value: *const CFString = std::ptr::null();
    let mut size = std::mem::size_of::<*const CFString>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(
            id,
            NonNull::from(&mut addr),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
            NonNull::from(&mut value).cast::<c_void>(),
        )
    };
    if status != 0 {
        return None;
    }
    let s = unsafe { CFRetained::from_raw(NonNull::new(value as *mut CFString)?) };
    Some(s.to_string())
}

fn input_channels(id: AudioObjectID) -> usize {
    let mut addr = address(kAudioDevicePropertyStreamConfiguration, kAudioObjectPropertyScopeInput);
    let mut size: u32 = 0;
    let status =
        unsafe { AudioObjectGetPropertyDataSize(id, NonNull::from(&mut addr), 0, std::ptr::null(), NonNull::from(&mut size)) };
    if status != 0 || size == 0 {
        return 0;
    }
    // u64 storage keeps the AudioBufferList properly aligned.
    let mut buf = vec![0u64; (size as usize).div_ceil(8)];
    let status = unsafe {
        AudioObjectGetPropertyData(
            id,
            NonNull::from(&mut addr),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
            NonNull::new(buf.as_mut_ptr().cast::<c_void>()).unwrap(),
        )
    };
    if status != 0 {
        return 0;
    }
    let list = buf.as_ptr().cast::<AudioBufferList>();
    unsafe {
        let n = (*list).mNumberBuffers as usize;
        let first = std::ptr::addr_of!((*list).mBuffers).cast::<objc2_core_audio_types::AudioBuffer>();
        (0..n).map(|i| (*first.add(i)).mNumberChannels as usize).sum()
    }
}

fn device_ids() -> Vec<AudioObjectID> {
    let mut addr = address(kAudioHardwarePropertyDevices, kAudioObjectPropertyScopeGlobal);
    let mut size: u32 = 0;
    let status = unsafe {
        AudioObjectGetPropertyDataSize(SYSTEM, NonNull::from(&mut addr), 0, std::ptr::null(), NonNull::from(&mut size))
    };
    if status != 0 {
        return Vec::new();
    }
    let mut ids = vec![0 as AudioObjectID; size as usize / std::mem::size_of::<AudioObjectID>()];
    if ids.is_empty() {
        return ids;
    }
    let status = unsafe {
        AudioObjectGetPropertyData(
            SYSTEM,
            NonNull::from(&mut addr),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
            NonNull::new(ids.as_mut_ptr().cast::<c_void>()).unwrap(),
        )
    };
    if status != 0 {
        return Vec::new();
    }
    ids.truncate(size as usize / std::mem::size_of::<AudioObjectID>());
    ids
}

pub fn default_input_id() -> AudioObjectID {
    get_u32(SYSTEM, kAudioHardwarePropertyDefaultInputDevice).unwrap_or(0)
}

/// All usable microphones, with their Core Audio ids.
pub fn mac_inputs() -> Vec<MacInput> {
    device_ids()
        .into_iter()
        .filter_map(|id| {
            if input_channels(id) == 0 {
                return None;
            }
            let uid = get_string(id, kAudioDevicePropertyDeviceUID)?;
            let name = get_string(id, kAudioObjectPropertyName)?;
            let transport = get_u32(id, kAudioDevicePropertyTransportType).unwrap_or(0);
            // Aggregate and virtual devices (our own meeting tap, loopback drivers) are not microphones.
            if transport == kAudioDeviceTransportTypeAggregate
                || transport == kAudioDeviceTransportTypeVirtual
                || name.starts_with("JezFlow")
            {
                return None;
            }
            let lower = name.to_lowercase();
            let is_continuity = transport == kAudioDeviceTransportTypeContinuityCaptureWired
                || transport == kAudioDeviceTransportTypeContinuityCaptureWireless
                || lower.contains("iphone")
                || lower.contains("ipad");
            Some(MacInput {
                id,
                device: InputDevice { uid, name, is_built_in: transport == kAudioDeviceTransportTypeBuiltIn, is_continuity },
            })
        })
        .collect()
}

/// AudioDevices.resolve on the Core Audio list.
pub fn resolve_mac(preference: &str) -> Option<MacInput> {
    let all = mac_inputs();
    if preference == "builtin" {
        return all.into_iter().find(|d| d.device.is_built_in);
    }
    if !preference.is_empty() {
        return all.into_iter().find(|d| d.device.uid == preference);
    }
    let default_id = default_input_id();
    let mut chosen = all.iter().find(|d| d.id == default_id).or(all.first()).cloned();
    if chosen.as_ref().is_some_and(|c| c.device.is_built_in) && lid_closed() {
        if let Some(other) = all.iter().find(|d| !d.device.is_built_in && !d.device.is_continuity) {
            chosen = Some(other.clone());
        }
    }
    chosen
}

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOServiceMatching(name: *const c_char) -> *mut c_void;
    fn IOServiceGetMatchingService(main_port: u32, matching: *mut c_void) -> u32;
    fn IORegistryEntryCreateCFProperty(entry: u32, key: &CFString, allocator: *const c_void, options: u32) -> *mut CFType;
    fn IOObjectRelease(object: u32) -> i32;
}

/// IOKit IOPMrootDomain AppleClamshellState.
pub fn lid_closed() -> bool {
    const NAME: &CStr = c"IOPMrootDomain";
    unsafe {
        // IOServiceGetMatchingService consumes the matching dictionary.
        let service = IOServiceGetMatchingService(0, IOServiceMatching(NAME.as_ptr()));
        if service == 0 {
            return false;
        }
        let key = CFString::from_static_str("AppleClamshellState");
        let value = IORegistryEntryCreateCFProperty(service, &key, std::ptr::null(), 0);
        IOObjectRelease(service);
        let Some(value) = NonNull::new(value) else { return false };
        let value = CFRetained::from_raw(value);
        value.downcast::<CFBoolean>().map(|b| b.as_bool()).unwrap_or(false)
    }
}

pub struct MacAudioDevices;

impl AudioDevices for MacAudioDevices {
    fn inputs(&self) -> Vec<InputDevice> {
        mac_inputs().into_iter().map(|d| d.device).collect()
    }

    fn resolve(&self, preference: &str) -> Option<InputDevice> {
        resolve_mac(preference).map(|d| d.device)
    }

    fn lid_closed(&self) -> bool {
        lid_closed()
    }

    /// kAudioDevicePropertyDeviceIsRunningSomewhere on the default input device.
    fn microphone_in_use(&self) -> bool {
        let id = default_input_id();
        id != 0 && get_u32(id, kAudioDevicePropertyDeviceIsRunningSomewhere).is_some_and(|v| v != 0)
    }
}
