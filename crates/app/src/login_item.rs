//! Open at login: the app as its own login item, through SMAppService. The system keeps the choice and lists it in
//! System Settings > General > Login Items, where it can be turned off too; this only asks.
#![expect(
    unsafe_code,
    reason = "ServiceManagement is reached through its Objective-C bindings"
)]

use objc2_service_management::{SMAppService, SMAppServiceStatus};

fn status() -> SMAppServiceStatus {
    // SAFETY: SMAppService's class method and property, which take nothing and may be read on any thread.
    unsafe { SMAppService::mainAppService().status() }
}

pub fn is_on() -> bool {
    status() == SMAppServiceStatus::Enabled
}

/// The system wants the person to allow it in System Settings before it takes effect.
pub fn needs_approval() -> bool {
    status() == SMAppServiceStatus::RequiresApproval
}

/// Turns it on or off. Only a signed app in Applications can be a login item, so `just run` gets an error.
pub fn set(on: bool) -> anyhow::Result<()> {
    // SAFETY: as above; registering and unregistering report failure through the NSError.
    let result = unsafe {
        let service = SMAppService::mainAppService();
        if on {
            service.registerAndReturnError()
        } else {
            service.unregisterAndReturnError()
        }
    };
    result.map_err(|error| anyhow::anyhow!("{}", error.localizedDescription()))
}

/// Opens Login Items in System Settings.
pub fn open_system_settings() {
    // SAFETY: a class method with no arguments.
    unsafe { SMAppService::openSystemSettingsLoginItems() }
}
