//! Files the operating system asks the app to open: the command line's, and on macOS the Finder's, which arrive
//! as `application:openURLs:` on the application delegate, never on the command line.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use wavedude_core::sync::lock;

/// Paths waiting for the window; the last one wins, the app holds one project.
#[derive(Clone, Default)]
pub struct OpenRequests(Arc<Mutex<Vec<PathBuf>>>);

impl OpenRequests {
	pub fn push(&self, path: PathBuf) {
		lock(&self.0).push(path);
		wavedude_app_state::wake::wake();
	}

	/// The last path asked for since the previous call.
	pub fn take_last(&self) -> Option<PathBuf> {
		std::mem::take(&mut *lock(&self.0)).pop()
	}
}

#[cfg(target_os = "macos")]
pub use macos::install;

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod macos {
	use objc2::rc::Retained;
	use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
	use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
	use objc2_app_kit::{NSApplication, NSApplicationDelegate};
	use objc2_foundation::{NSArray, NSURL};

	use super::OpenRequests;

	define_class!(
		// SAFETY: NSObject has no subclassing requirements; the class adds no Drop and only main-thread methods.
		#[unsafe(super = NSObject)]
		#[thread_kind = MainThreadOnly]
		#[ivars = OpenRequests]
		struct Delegate;

		// SAFETY: NSObjectProtocol has no extra requirements.
		unsafe impl NSObjectProtocol for Delegate {}

		// SAFETY: the method below has the signature AppKit declares for the selector.
		unsafe impl NSApplicationDelegate for Delegate {
			#[unsafe(method(application:openURLs:))]
			fn open_urls(&self, _app: &NSApplication, urls: &NSArray<NSURL>) {
				for url in urls {
					if let Some(path) = url.to_file_path() {
						self.ivars().push(path);
					}
				}
			}
		}
	);

	/// Makes the Finder's open requests land in `requests`. Call after the event loop is built (winit sets up
	/// `NSApplication` there and registers no delegate of its own) and keep the result alive while it runs:
	/// AppKit holds its delegate weakly.
	pub fn install(requests: &OpenRequests) -> Retained<NSObject> {
		let mtm = MainThreadMarker::new().expect("called on the main thread");
		let this = Delegate::alloc(mtm).set_ivars(requests.clone());
		// SAFETY: `init` of NSObject on a freshly allocated instance.
		let delegate: Retained<Delegate> = unsafe { msg_send![super(this), init] };
		NSApplication::sharedApplication(mtm).setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
		Retained::into_super(delegate)
	}
}
