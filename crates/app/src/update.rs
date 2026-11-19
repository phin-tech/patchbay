//! Updates with Sparkle. The release build puts Sparkle.framework in `Contents/Frameworks` and SUFeedURL in the
//! Info.plist, and nothing links against the framework: it's loaded here at launch, so a build without it, from
//! `just run` or `just install-local`, simply has no updates. Sparkle keeps its own schedule, windows and settings.
#![expect(
    unsafe_code,
    reason = "Sparkle is reached through the Objective-C runtime"
)]

use std::cell::RefCell;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};

use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyClass, AnyObject, Bool, NSObject, NSObjectProtocol};
use objc2::{ClassType, define_class, msg_send};
use objc2_foundation::{NSBundle, NSSet, NSString};

thread_local! {
    /// Sparkle's controller, kept for as long as the app runs, since letting it go stops the checks.
    static CONTROLLER: RefCell<Option<Retained<AnyObject>>> = const { RefCell::new(None) };
    /// Its delegate, which Sparkle holds weakly.
    static CHANNELS: RefCell<Option<Retained<Channels>>> = const { RefCell::new(None) };
}

/// Whether this copy takes betas, from Settings.
static BETA: AtomicBool = AtomicBool::new(false);

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and Channels doesn't implement Drop.
    #[unsafe(super(NSObject))]
    #[name = "patchbay_SparkleChannels"]
    struct Channels;

    impl Channels {
        /// SPUUpdaterDelegate's question: which channels of the appcast this copy takes, besides the default one.
        #[unsafe(method_id(allowedChannelsForUpdater:))]
        fn allowed_channels(&self, _updater: &AnyObject) -> Retained<NSSet<NSString>> {
            if BETA.load(Ordering::Relaxed) {
                NSSet::from_retained_slice(&[NSString::from_str("beta")])
            } else {
                NSSet::new()
            }
        }
    }

    unsafe impl NSObjectProtocol for Channels {}
);

/// Takes betas from the next check on, or stops: a beta tester moves on to the release that follows, since builds only
/// go up.
pub fn set_beta(on: bool) {
    BETA.store(on, Ordering::Relaxed);
}

/// Starts Sparkle, which then checks on its own schedule and shows its own windows. Runs on the main thread.
pub fn start() {
    if let Some(controller) = load() {
        CONTROLLER.with(|slot| *slot.borrow_mut() = Some(controller));
    }
}

fn load() -> Option<Retained<AnyObject>> {
    let frameworks = NSBundle::mainBundle().privateFrameworksPath()?;
    let path = NSString::from_str(&format!("{frameworks}/Sparkle.framework"));
    let sparkle = NSBundle::bundleWithPath(&path)?;
    // SAFETY: loading runs the framework's initializers; this is the Sparkle the release signed in with the app.
    if !unsafe { sparkle.load() } {
        return None;
    }
    let class = AnyClass::get(c"SPUStandardUpdaterController")?;
    // SAFETY: `new` on an NSObject subclass with no instance variables.
    let channels: Retained<Channels> = unsafe { msg_send![Channels::class(), new] };
    let delegate: *const AnyObject = (&*channels as *const Channels).cast();
    CHANNELS.with(|slot| *slot.borrow_mut() = Some(channels));
    let nothing = ptr::null_mut::<AnyObject>();
    // SAFETY: the initializer Sparkle documents for starting the updater with its standard windows; the delegate is
    // kept alive above for as long as the app runs.
    unsafe {
        let blank: Allocated<AnyObject> = msg_send![class, alloc];
        msg_send![blank, initWithStartingUpdater: Bool::YES, updaterDelegate: delegate, userDriverDelegate: nothing]
    }
}

/// Sparkle's updater, when it's running.
fn updater() -> Option<Retained<AnyObject>> {
    let controller = CONTROLLER.with(|slot| slot.borrow().clone())?;
    // SAFETY: `updater` is the controller's property holding its SPUUpdater.
    unsafe { msg_send![&*controller, updater] }
}

/// Whether there's a Sparkle to ask: only in the released app.
pub fn available() -> bool {
    CONTROLLER.with(|slot| slot.borrow().is_some())
}

/// Asks Sparkle to check now, showing what it finds. False when there's no Sparkle to ask.
pub fn check() -> bool {
    let Some(controller) = CONTROLLER.with(|slot| slot.borrow().clone()) else {
        return false;
    };
    // SAFETY: `checkForUpdates:` is the controller's menu action, and takes the sender, which may be nil.
    let _: () = unsafe { msg_send![&*controller, checkForUpdates: ptr::null_mut::<AnyObject>()] };
    true
}

/// Whether Sparkle checks once a day on its own, which it keeps in the app's defaults.
pub fn automatic() -> bool {
    updater().is_some_and(|updater| {
        // SAFETY: a BOOL property of SPUUpdater.
        let on: Bool = unsafe { msg_send![&*updater, automaticallyChecksForUpdates] };
        on.as_bool()
    })
}

pub fn set_automatic(on: bool) {
    if let Some(updater) = updater() {
        // SAFETY: the setter of the BOOL property above.
        let _: () =
            unsafe { msg_send![&*updater, setAutomaticallyChecksForUpdates: Bool::new(on)] };
    }
}
