use std::ffi::CString;
use std::path::PathBuf;
use objc2::{class, msg_send};
use objc2::rc::autoreleasepool;
use objc2::runtime::AnyObject;
use objc2_foundation::NSString;

/// Shows the file at `path` in the system video player, with sound even
/// when the phone is on silent (as other apps play videos).
pub fn play(path: PathBuf) {
    dispatch2::DispatchQueue::main().exec_async(move || {
        autoreleasepool(|_| unsafe {
            let Some(path) = path.to_str().map(NSString::from_str) else { return };
            let url: *mut AnyObject = msg_send![class!(NSURL), fileURLWithPath: &*path];
            if url.is_null() { return; }

            let audio: *mut AnyObject = msg_send![class!(AVAudioSession), sharedInstance];
            if let Some(playback) = av_constant("AVAudioSessionCategoryPlayback") && !audio.is_null() {
                let mut error: *mut AnyObject = std::ptr::null_mut();
                let _: bool = msg_send![audio, setCategory: playback, error: &mut error];
            }

            let player: *mut AnyObject = msg_send![class!(AVPlayer), playerWithURL: url];
            let controller: *mut AnyObject = msg_send![class!(AVPlayerViewController), new];
            if player.is_null() || controller.is_null() { return; }
            let _: () = msg_send![controller, setPlayer: player];

            // On top of whatever's showing.
            let app: *mut AnyObject = msg_send![class!(UIApplication), sharedApplication];
            let window: *mut AnyObject = msg_send![app, keyWindow];
            if window.is_null() { return; }
            let mut presenter: *mut AnyObject = msg_send![window, rootViewController];
            loop {
                if presenter.is_null() { return; }
                let presented: *mut AnyObject = msg_send![presenter, presentedViewController];
                if presented.is_null() { break; }
                presenter = presented;
            }

            let null_block: *mut AnyObject = std::ptr::null_mut();
            let _: () = msg_send![presenter, presentViewController: controller, animated: true, completion: null_block];
            let _: () = msg_send![player, play];
            // `new` gave it to us; presenting it keeps it.
            objc2::ffi::objc_release(controller as *mut _);
        });
    });
}

/// One of AVFoundation's string constants.
unsafe fn av_constant(name: &str) -> Option<*mut AnyObject> {
    let symbol = CString::new(name).ok()?;
    let ptr = unsafe { libc::dlsym(libc::RTLD_DEFAULT, symbol.as_ptr()) } as *const *mut AnyObject;
    if ptr.is_null() { return None; }
    let value = unsafe { *ptr };
    (!value.is_null()).then_some(value)
}
