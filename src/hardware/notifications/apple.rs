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
        unsafe {
            let center = UNUserNotificationCenter::currentNotificationCenter();

            let options = UNAuthorizationOptions::Alert
                | UNAuthorizationOptions::Sound
                | UNAuthorizationOptions::Badge;

            // Use a Cell to communicate across closure boundary
            let granted_cell = Cell::new(false);

            if MainThreadMarker::new().is_some() {
                let granted_cell_closure = granted_cell.clone();
                let block = StackBlock::new(
                    move |granted: Bool, error: *mut NSError| {
                        granted_cell_closure.set(granted.as_bool());

                        if granted.as_bool() {
                            println!("Push permission granted.");
                        } else {
                            println!("Push permission denied.");
                        }

                        if !error.is_null() {
                            println!("Authorization error occurred.");
                        }
                    },
                ).copy();

                center.requestAuthorizationWithOptions_completionHandler(options, &block);

                // Delay registerForRemoteNotifications until we have the result
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    if granted_cell.get() {
                        if let Some(mtm2) = MainThreadMarker::new() {
                            let app = UIApplication::sharedApplication(mtm2);
                            app.registerForRemoteNotifications();
                        }
                    }
                });
            }
        }
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