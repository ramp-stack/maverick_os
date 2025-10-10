
#![cfg(any(target_os = "macos", target_os = "ios"))]


use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2::{class, msg_send, sel};
use objc2_foundation::NSError;
use objc2_av_foundation::AVCaptureDevice;
use objc2_av_foundation::AVCaptureTorchMode;

pub fn toggle_flash(on: bool) {
    unsafe {
        let media_type = objc2_av_foundation::AVMediaTypeVideo
            .expect("AVMediaTypeVideo should be available");
        
        let device: Option<Retained<AVCaptureDevice>> =
            AVCaptureDevice::defaultDeviceWithMediaType(media_type);

        if let Some(device) = device {
            if device.hasTorch() {
                let _ = device.lockForConfiguration();

                if device.isTorchModeSupported(objc2_av_foundation::AVCaptureTorchMode::On) {
                    if on {
                        let _ : Bool = msg_send![&*device, setTorchModeOnWithLevel:1.0f32 error:std::ptr::null_mut::<*mut NSError>()];
                    } else {
                        let _ : () = msg_send![&*device, setTorchMode:objc2_av_foundation::AVCaptureTorchMode::Off];
                    }
                }

                device.unlockForConfiguration();
            } else {
                println!("Torch not available on this device");
            }
        } else {
            println!("No camera device available");
        }
    }
}