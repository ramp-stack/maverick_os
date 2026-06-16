#[cfg(target_os = "ios")]
use objc2::runtime::Bool;
#[cfg(target_os = "ios")]
use objc2::MainThreadMarker;
#[cfg(target_os = "ios")]
use objc2_ui_kit::UIApplication;
#[cfg(target_os = "ios")]
use std::cell::Cell;

#[cfg(target_os = "macos")]
use objc2::runtime::Bool;
#[cfg(target_os = "macos")]
use objc2_foundation::NSBundle;
#[cfg(target_os = "macos")]
use objc2::rc::autoreleasepool;

use objc2_user_notifications::{
    UNAuthorizationOptions, UNUserNotificationCenter, UNNotificationRequest,
    UNMutableNotificationContent, UNTimeIntervalNotificationTrigger,
};
use block2::StackBlock;
use objc2_foundation::{NSString, NSError};

#[derive(Clone)]
pub struct OsNotifications;

impl OsNotifications {
    pub fn new() -> Self {
        Self
    }

    #[cfg(target_os = "ios")]
    pub fn register(&self) {
        use objc2::runtime::AnyObject;
        use objc2::{msg_send, class};
        unsafe {
            // let ui_app = class!(UIApplication);
            // let shared_app: *mut AnyObject = msg_send![ui_app, sharedApplication];

            // let delegate: *mut AnyObject = msg_send![shared_app, delegate];

            let mtm = MainThreadMarker::new().unwrap();
            let app = UIApplication::sharedApplication(mtm);

            println!("app = {:?}", &*app);

            // let delegate = app.delegate();
            // println!("delegate = {:?}", delegate);

            // println!("Delegate: {:?}", delegate);


            if app.delegate().is_none() {
                let delegate = create_app_delegate();
                let _: () = msg_send![&*app, setDelegate: delegate];
                let current: *mut AnyObject = msg_send![&*app, delegate];

                println!("delegate after install = {:?}", current);
            }

            app.registerForRemoteNotifications();
        }

        // unsafe {
        //     let center = UNUserNotificationCenter::currentNotificationCenter();

        //     let options = UNAuthorizationOptions::Alert
        //         | UNAuthorizationOptions::Sound
        //         | UNAuthorizationOptions::Badge;

        //     // Use a Cell to communicate across closure boundary
        //     let granted_cell = Cell::new(false);

        //     if MainThreadMarker::new().is_some() {
        //         let granted_cell_closure = granted_cell.clone();
        //         let block = StackBlock::new(
        //             move |granted: Bool, error: *mut NSError| {
        //                 granted_cell_closure.set(granted.as_bool());

        //                 if granted.as_bool() {
        //                     println!("Push permission granted.");
        //                 } else {
        //                     println!("Push permission denied.");
        //                 }

        //                 if !error.is_null() {
        //                     println!("Authorization error occurred.");
        //                 }
        //             },
        //         ).copy();

        //         center.requestAuthorizationWithOptions_completionHandler(options, &block);

        //         // Delay registerForRemoteNotifications until we have the result
        //         std::thread::spawn(move || {
        //             std::thread::sleep(std::time::Duration::from_secs(1));
        //             if granted_cell.get() {
        //                 if let Some(mtm2) = MainThreadMarker::new() {
        //                     let app = UIApplication::sharedApplication(mtm2);
        //                     app.registerForRemoteNotifications();
        //                 }
        //             }
        //         });
        //     }
        // }
    }

    #[cfg(target_os = "macos")]
    pub fn register(&self) {
        println!("Registering notifications..");
        unsafe {
            let main_bundle = NSBundle::mainBundle();
            let bundle_url = main_bundle.bundleURL();
            let rust_str = autoreleasepool(|pool| {
                let absolute_string = bundle_url.absoluteString().unwrap();
                absolute_string.to_str(pool).to_owned()
            });

            if !rust_str.ends_with(".app/") {
                eprintln!("⚠️ No valid app bundle detected. Skipping notification registration.");
                return;
            }

            let center = UNUserNotificationCenter::currentNotificationCenter();

            let options = UNAuthorizationOptions::Alert 
                | UNAuthorizationOptions::Sound 
                | UNAuthorizationOptions::Badge;
            
            let block = StackBlock::new(
                move |granted: Bool, error: *mut NSError| {
                    if granted.as_bool() {
                        println!("Push permission granted on macOS.");
                    } else {
                        println!("Push permission denied on macOS.");
                    }
                    if !error.is_null() {
                        println!("Authorization error on macOS.");
                    }
                },
            ).copy();

            center.requestAuthorizationWithOptions_completionHandler(options, &block);
        }
    }

    #[cfg(target_os = "ios")]
    pub fn push(&self, title: &str, body: &str) {
        unsafe {
            let content = UNMutableNotificationContent::new();
            content.setTitle(&NSString::from_str(title));
            content.setBody(&NSString::from_str(body));
            content.setSound(Some(&objc2_user_notifications::UNNotificationSound::defaultSound()));

            let trigger = UNTimeIntervalNotificationTrigger::triggerWithTimeInterval_repeats(1.0, false);
            let identifier = NSString::from_str("demo-id");

            let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
                &identifier,
                &content,
                Some(&trigger),
            );

            let center = UNUserNotificationCenter::currentNotificationCenter();
            center.addNotificationRequest_withCompletionHandler(&request, None);

            println!("Notification scheduled!");
        }
    }

    #[cfg(target_os = "macos")]
    pub fn push(&self, title: &str, body: &str) {
        unsafe {
            let main_bundle = NSBundle::mainBundle();
            let bundle_url = main_bundle.bundleURL();
            let rust_str = autoreleasepool(|pool| {
                let absolute_string = bundle_url.absoluteString().unwrap();
                absolute_string.to_str(pool).to_owned()
            });

            if !rust_str.ends_with(".app/") {
                eprintln!("⚠️ No valid app bundle detected. Skipping notification registration.");
                return;
            }

            let content = UNMutableNotificationContent::new();
            content.setTitle(&NSString::from_str(title));
            content.setBody(&NSString::from_str(body));
            content.setSound(Some(&objc2_user_notifications::UNNotificationSound::defaultSound()));

            let trigger = UNTimeIntervalNotificationTrigger::triggerWithTimeInterval_repeats(1.0, false);
            let identifier = NSString::from_str("demo-id");

            let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
                &identifier,
                &content,
                Some(&trigger),
            );

            let center = UNUserNotificationCenter::currentNotificationCenter();
            center.addNotificationRequest_withCompletionHandler(&request, None);

            println!("Notification scheduled on macOS!");
        }
    }
}

use objc2::{sel, class, msg_send};
use std::sync::OnceLock;
use objc2::declare::ClassBuilder;
use objc2::runtime::AnyClass;
use objc2::runtime::AnyObject;
use objc2::runtime::Sel;

static APP_DELEGATE_CLASS: OnceLock<&'static AnyClass> = OnceLock::new();

fn create_app_delegate() -> *mut AnyObject {
    unsafe {
        let cls = APP_DELEGATE_CLASS.get_or_init(|| {
            let mut decl = ClassBuilder::new(c"RustPushDelegate", class!(NSObject)).unwrap();

            extern "C" fn did_register(
                this: &'static AnyObject,
                _cmd: Sel,
                _app: *mut AnyObject,
                token: *mut AnyObject,
            ) {
                println!("APNs did_register called");

                unsafe {
                   use objc2_foundation::NSData;

                    let token: &NSData = &*(token as *const NSData);

                    let length = token.length();

                    let bytes: *const std::ffi::c_void = unsafe {
                        objc2::msg_send![token, bytes]
                    };

                    let slice = unsafe {
                        std::slice::from_raw_parts(
                            bytes as *const u8,
                            length,
                        )
                    };

                    let token_string = slice
                        .iter()
                        .map(|b| format!("{:02x}", b))
                        .collect::<String>();

                    println!("APNs token: {}", token_string);
                }
            }

            extern "C" fn did_fail(
                this: &'static AnyObject,
                _cmd: Sel,
                _app: *mut AnyObject,
                error: *mut AnyObject,
            ) {
                println!("APNs registration failed: {:?}", error);
            }

            decl.add_method(
                sel!(application:didRegisterForRemoteNotificationsWithDeviceToken:),
                did_register as extern "C" fn(&'static AnyObject, Sel, *mut AnyObject, *mut AnyObject),
            );

            decl.add_method(
                sel!(application:didFailToRegisterForRemoteNotificationsWithError:),
                did_fail as extern "C" fn(&'static AnyObject, Sel, *mut AnyObject, *mut AnyObject),
            );

            decl.register()
        });

        let delegate: *mut AnyObject = msg_send![*cls, new];
        delegate
    }
}