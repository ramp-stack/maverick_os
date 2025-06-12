#[cfg(target_os = "android")]
use image::{Rgba, RgbaImage};
use jni::objects::JClass;
use jni::sys::jobject;
#[cfg(target_os = "android")]
use jni::{
    objects::{GlobalRef, JByteBuffer, JObject, JObjectArray, JString, JValue},
    JNIEnv, JavaVM,
};
use ndk_context;

#[cfg(target_os = "android")]

#[cfg(target_os = "android")]
use std::error::Error;
#[cfg(target_os = "android")]
use std::thread;
#[cfg(target_os = "android")]
use std::time::Duration;
#[cfg(target_os = "android")]
use std::fs;
#[cfg(target_os = "android")]
use std::path::Path;
use std::sync::Arc;

#[cfg(target_os = "android")]
#[derive(Clone, Debug)]
pub struct AndroidCamera {
    java_vm: Arc<JavaVM>,
    app_context: GlobalRef,
    camera_manager: GlobalRef,
    camera_helper_class_loader: Option<GlobalRef>,
    camera_helper_instance: Option<GlobalRef>,
}

#[cfg(target_os = "android")]
impl AndroidCamera {
    pub fn new() -> Result<Self, Box<dyn Error>> {
        let jvm = Arc::new(unsafe { JavaVM::from_raw(ndk_context::android_context().vm().cast())? });

        let (global_context, global_camera_manager) = {
            let mut env = jvm.attach_current_thread()?;

            let ctx_ptr = ndk_context::android_context().context();
            if ctx_ptr.is_null() {
                return Err("Failed to get Android context".into());
            }

            let context_obj = unsafe { JObject::from_raw(ctx_ptr as jobject) };
            let global_context = env.new_global_ref(context_obj)?;
            let global_camera_manager =
                Self::initialize_camera_manager_static(&mut env, &global_context)?;

            (global_context, global_camera_manager)
        };

        Ok(Self {
            java_vm: jvm.clone(),
            app_context: global_context,
            camera_manager: global_camera_manager,
            camera_helper_class_loader: None,
            camera_helper_instance: None,
        })
    }

    fn initialize_camera_manager_static(
        env: &mut JNIEnv,
        context: &GlobalRef,
    ) -> Result<GlobalRef, Box<dyn Error>> {
        let camera_service = env
            .get_static_field("android/content/Context", "CAMERA_SERVICE", "Ljava/lang/String;")?
            .l()?;

        let manager = env
            .call_method(
                context.as_obj(),
                "getSystemService",
                "(Ljava/lang/String;)Ljava/lang/Object;",
                &[JValue::Object(&camera_service)],
            )?
            .l()?;

        Ok(env.new_global_ref(manager)?)
    }

    fn get_embedded_dex_bytes(&self) -> &'static [u8] {
        static DEX_BYTES: &[u8] = include_bytes!(".././classes.dex");
        println!("Using embedded dex bytes: {} bytes", DEX_BYTES.len());
        DEX_BYTES
    }

    // Alternative: Load from runtime file path (if you still need this option)
    fn load_dex_from_file(&self, dex_file_path: &str) -> Result<Vec<u8>, Box<dyn Error>> {
        println!("Loading dex file from: {}", dex_file_path);

        if !Path::new(dex_file_path).exists() {
            return Err(format!("Dex file not found: {}", dex_file_path).into());
        }

        let dex_bytes = fs::read(dex_file_path)?;
        println!("Successfully loaded {} bytes from dex file", dex_bytes.len());

        Ok(dex_bytes)
    }

    unsafe fn dex_loader_from_bytes(&mut self, dex_bytes: &[u8]) -> Result<(), Box<dyn Error>> {
        let mut env = self.java_vm.attach_current_thread()?;
        println!("Starting dex_loader_from_bytes with {} bytes", dex_bytes.len());

        let byte_buffer = env.new_direct_byte_buffer(dex_bytes.as_ptr() as *mut u8, dex_bytes.len())?;
        println!("Created direct ByteBuffer: {:?}", byte_buffer);

        let context_class = env.get_object_class(&self.app_context)?;
        let get_class_loader_method = env.get_method_id(context_class, "getClassLoader", "()Ljava/lang/ClassLoader;")?;
        let parent_class_loader = env.call_method_unchecked(
            &self.app_context,
            get_class_loader_method,
            jni::signature::ReturnType::Object,
            &[],
        )?.l()?;
        println!("Parent class loader obtained: {:?}", parent_class_loader);

        let in_memory_dex_class_loader_class = env.find_class("dalvik/system/InMemoryDexClassLoader")?;
        println!("InMemoryDexClassLoader class found: {:?}", in_memory_dex_class_loader_class);

        let constructor_id = env.get_method_id(
            &in_memory_dex_class_loader_class,
            "<init>",
            "(Ljava/nio/ByteBuffer;Ljava/lang/ClassLoader;)V",
        )?;
        println!("InMemoryDexClassLoader constructor ID retrieved: {:?}", constructor_id);

        let dex_class_loader_obj = env.new_object_unchecked(
            in_memory_dex_class_loader_class,
            constructor_id,
            &[
                JValue::Object(&byte_buffer).as_jni(),
                JValue::Object(&parent_class_loader).as_jni(),
            ],
        )?;
        println!("InMemoryDexClassLoader instantiated: {:?}", dex_class_loader_obj);

        self.camera_helper_class_loader = Some(env.new_global_ref(dex_class_loader_obj)?);

        let thread = env.call_static_method("java/lang/Thread", "currentThread", "()Ljava/lang/Thread;", &[])?.l()?;
        env.call_method(
            thread,
            "setContextClassLoader",
            "(Ljava/lang/ClassLoader;)V",
            &[JValue::Object(self.camera_helper_class_loader.as_ref().unwrap().as_obj())],
        )?;
        println!("Context class loader set.");

        let class_name = env.new_string("com.orangeme.camera.CameraHelper")?;
        let camera_helper_class = env.call_method(
            self.camera_helper_class_loader.as_ref().unwrap().as_obj(),
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[JValue::Object(&class_name)],
        )?.l()?;
        println!("CameraHelper class loaded: {:?}", camera_helper_class);

        let camera_helper_class_jclass = JClass::from(camera_helper_class);
        let camera_helper_constructor = env.get_method_id(
            &camera_helper_class_jclass,
            "<init>",
            "(Landroid/content/Context;)V",
        )?;

        let camera_helper_obj = env.new_object_unchecked(
            camera_helper_class_jclass,
            camera_helper_constructor,
            &[JValue::Object(&self.app_context).as_jni()],
        )?;
        println!("CameraHelper instance created: {:?}", camera_helper_obj);

        self.camera_helper_instance = Some(env.new_global_ref(camera_helper_obj)?);

        Ok(())
    }

    unsafe fn load_embedded_dex(&mut self) -> Result<(), Box<dyn Error>> {
        println!("Loading embedded dex bytes");
        let dex_bytes = self.get_embedded_dex_bytes();
        self.dex_loader_from_bytes(dex_bytes)
    }

    pub fn has_camera_permission(&self) -> Result<bool, Box<dyn Error>> {
        let mut env = self.java_vm.attach_current_thread()?;
        let camera_helper = self.camera_helper_instance.as_ref().ok_or("CameraHelper not initialized")?;

        let has_permission = env.call_method(
            camera_helper.as_obj(),
            "hasCameraPermission",
            "()Z",
            &[],
        )?.z()?;

        Ok(has_permission)
    }

    pub fn request_camera_permission(&self) -> Result<(), Box<dyn Error>> {
        let mut env = self.java_vm.attach_current_thread()?;
        let camera_helper = self.camera_helper_instance.as_ref().ok_or("CameraHelper not initialized")?;

        env.call_method(
            camera_helper.as_obj(),
            "requestCameraPermission",
            "()V",
            &[],
        )?;

        Ok(())
    }

    pub fn is_waiting_for_permission(&self) -> Result<bool, Box<dyn Error>> {
        let mut env = self.java_vm.attach_current_thread()?;
        let camera_helper = self.camera_helper_instance.as_ref().ok_or("CameraHelper not initialized")?;

        let waiting = env.call_method(
            camera_helper.as_obj(),
            "isWaitingForPermission",
            "()Z",
            &[],
        )?.z()?;

        Ok(waiting)
    }

    pub fn wait_for_permission(&self, timeout_seconds: u64) -> Result<bool, Box<dyn Error>> {
        println!("Waiting for camera permission (timeout: {}s)", timeout_seconds);

        let start_time = std::time::Instant::now();
        let timeout = Duration::from_secs(timeout_seconds);

        // First check if we already have permission
        if self.has_camera_permission()? {
            println!("Camera permission already granted");
            return Ok(true);
        }

        // Request permission if we don't have it
        self.request_camera_permission()?;

        // Wait for permission to be granted or denied
        while start_time.elapsed() < timeout {
            if self.has_camera_permission()? {
                println!("Camera permission granted!");
                return Ok(true);
            }

            if !self.is_waiting_for_permission()? {
                println!("No longer waiting for permission - likely denied");
                return Ok(false);
            }

            println!("Still waiting for camera permission...");
            thread::sleep(Duration::from_millis(500));
        }

        println!("Timeout waiting for camera permission");
        Ok(false)
    }

    pub fn open_camera_with_dex_file(&mut self, dex_file_path: &str) -> Result<(), Box<dyn Error>> {
        println!("Opening camera with dex file: {}", dex_file_path);

        let dex_bytes = self.load_dex_from_file(dex_file_path)?;
        unsafe {
            self.dex_loader_from_bytes(&dex_bytes)?;
        }

        self.open_camera_internal()
    }

    pub fn open_camera(&mut self) -> Result<(), Box<dyn Error>> {
        println!("Opening camera with embedded dex");

        unsafe {
            self.load_embedded_dex()?;
        }

        self.open_camera_internal()
    }

    fn open_camera_internal(&mut self) -> Result<(), Box<dyn Error>> {
        println!("Checking camera permission before opening camera");

        if !self.has_camera_permission()? {
            println!("Camera permission not granted, requesting...");
            if !self.wait_for_permission(30)? {
                return Err("Camera permission not granted within timeout".into());
            }
        }

        let mut env = self.java_vm.attach_current_thread()?;
        let camera_helper = self.camera_helper_instance.as_ref().ok_or("CameraHelper not initialized")?;

        let camera_id_list_obj = env.call_method(
            camera_helper.as_obj(),
            "getCameraIdList",
            "()[Ljava/lang/String;",
            &[],
        )?.l()?;

        let camera_id_array = JObjectArray::from(camera_id_list_obj);
        let length = env.get_array_length(&camera_id_array)?;
        if length == 0 {
            return Err("No cameras available".into());
        }

        let first_camera_id = env.get_object_array_element(&camera_id_array, 0)?;
        let camera_id_str: String = env.get_string(&JString::from(first_camera_id))?.into();
        let camera_id_jstr = env.new_string(&camera_id_str)?;

        println!("Opening camera with ID: {}", camera_id_str);

        env.call_method(
            camera_helper.as_obj(),
            "openCamera",
            "(Ljava/lang/String;)V",
            &[JValue::Object(&camera_id_jstr)],
        )?;

        println!("Camera open request sent successfully");
        Ok(())
    }

    pub fn wait_for_camera_ready(&self, timeout_seconds: u64) -> Result<bool, Box<dyn Error>> {
        println!("Waiting for camera session to be ready (timeout: {}s)", timeout_seconds);

        let start_time = std::time::Instant::now();
        let timeout = Duration::from_secs(timeout_seconds);

        while start_time.elapsed() < timeout {
            match self.is_camera_ready() {
                Ok(true) => {
                    println!("Camera session is ready!");
                    return Ok(true);
                }
                Ok(false) => {
                    println!("Camera session not ready yet, waiting...");
                    thread::sleep(Duration::from_millis(100));
                }
                Err(e) => {
                    println!("Error checking camera ready status: {}", e);
                    return Err(e);
                }
            }
        }

        println!("Timeout waiting for camera session to be ready");
        Ok(false)
    }

    fn get_image_dimensions(&self, image: &JObject) -> Result<(i32, i32), Box<dyn Error>> {
        let mut env_guard = self.java_vm.attach_current_thread()?;
        let env = &mut *env_guard;

        let width = env.call_method(image, "getWidth", "()I", &[])?.i()?;
        let height = env.call_method(image, "getHeight", "()I", &[])?.i()?;

        println!("Image dimensions - Width: {}, Height: {}", width, height);
        Ok((width, height))
    }

    fn convert_yuv_to_rgba(
        &self,
        image: &JObject,
        width: i32,
        height: i32,
    ) -> Result<Vec<u8>, Box<dyn Error>> {
        println!("Starting convert_yuv_to_rgba");
        let mut env = self.java_vm.attach_current_thread()?;

        let planes: JObjectArray = env
            .call_method(image, "getPlanes", "()[Landroid/media/Image$Plane;", &[])?
            .l()?
            .into();

        let plane_count = env.get_array_length(&planes)?;
        println!("Number of planes: {}", plane_count);

        if plane_count < 3 {
            return Err("Image does not have the expected YUV planes".into());
        }

        let mut extract = |idx| -> Result<(Vec<u8>, i32, i32), Box<dyn Error>> {
            println!("Extracting plane index: {}", idx);
            let plane = env.get_object_array_element(&planes, idx)?;
            let buffer = env.call_method(&plane, "getBuffer", "()Ljava/nio/ByteBuffer;", &[])?.l()?;
            let byte_buffer = JByteBuffer::from(buffer);

            let len = env.get_direct_buffer_capacity(&byte_buffer)?;
            let ptr = env.get_direct_buffer_address(&byte_buffer)?;
            let data = unsafe { std::slice::from_raw_parts(ptr, len).to_vec() };

            let row_stride = env.call_method(&plane, "getRowStride", "()I", &[])?.i()?;
            let pixel_stride = env.call_method(&plane, "getPixelStride", "()I", &[])?.i()?;

            println!(
                "Plane {}: len = {}, row_stride = {}, pixel_stride = {}",
                idx, len, row_stride, pixel_stride
            );

            Ok((data, row_stride, pixel_stride))
        };

        let (y, y_rs, y_ps) = extract(0)?;
        let (u, u_rs, u_ps) = extract(1)?;
        let (v, v_rs, v_ps) = extract(2)?;

        let mut rgba = Vec::with_capacity((width * height * 4) as usize);

        for row in 0..height {
            for col in 0..width {
                let yi = (row * y_rs + col * y_ps) as usize;
                let ui = ((row / 2) * u_rs + (col / 2) * u_ps) as usize;
                let vi = ((row / 2) * v_rs + (col / 2) * v_ps) as usize;

                let y_val = y.get(yi).copied().unwrap_or(0) as i32;
                let u_val = u.get(ui).copied().unwrap_or(128) as i32;
                let v_val = v.get(vi).copied().unwrap_or(128) as i32;

                let c = y_val - 16;
                let d = u_val - 128;
                let e = v_val - 128;

                let r = ((298 * c + 409 * e + 128) >> 8).clamp(0, 255) as u8;
                let g = ((298 * c - 100 * d - 208 * e + 128) >> 8).clamp(0, 255) as u8;
                let b = ((298 * c + 516 * d + 128) >> 8).clamp(0, 255) as u8;

                rgba.extend_from_slice(&[r, g, b, 255]);
            }
        }

        println!("Finished convert_yuv_to_rgba");
        Ok(rgba)
    }

    pub fn get_latest_frame(&self) -> Result<RgbaImage, Box<dyn Error>> {
        println!("Starting get_latest_frame");

        // Ensure we have permission before trying to get frame
        if !self.has_camera_permission()? {
            return Err("Camera permission not granted".into());
        }

        if !self.wait_for_camera_ready(10)? {
            return Err("Camera session not ready within timeout".into());
        }

        let mut env = self.java_vm.attach_current_thread()?;
        println!("Attached to Java thread.");

        let camera_helper = self.camera_helper_instance.as_ref().ok_or("CameraHelper not initialized")?;
        println!("CameraHelper instance retrieved: {:?}", camera_helper);

        let session_ready = env.call_method(
            camera_helper.as_obj(),
            "isSessionReady",
            "()Z",
            &[],
        )?.z()?;
        println!("Session ready status: {}", session_ready);

        if !session_ready {
            return Err("Camera session still not ready after waiting".into());
        }

        let image_obj = env.call_method(
            camera_helper.as_obj(),
            "acquireLatestImage",
            "()Landroid/media/Image;",
            &[],
        )?.l()?;
        println!("Acquired image object: {:?}", image_obj);

        if image_obj.is_null() {
            println!("No image available.");
            return Err("No image available".into());
        }

        println!("Image acquired successfully: {:?}", image_obj);

        let (w, h) = self.get_image_dimensions(&image_obj)?;
        println!("Image dimensions retrieved: Width = {}, Height = {}", w, h);

        let data = self.convert_yuv_to_rgba(&image_obj, w, h)?;
        println!("YUV to RGBA conversion completed. Data length: {}", data.len());

        let mut img = RgbaImage::new(w as u32, h as u32);
        println!("Created new RgbaImage with dimensions: {}x{}", w, h);

        for (i, px) in data.chunks_exact(4).enumerate() {
            let x = (i % w as usize) as u32;
            let y = (i / w as usize) as u32;
            img.put_pixel(x, y, Rgba([px[0], px[1], px[2], px[3]]));
        }
        println!("Populated RgbaImage with pixel data.");

        env.call_method(&image_obj, "close", "()V", &[])?;
        println!("Closed image to free resources.");

        println!("Successfully converted image to RGBA");
        Ok(img)
    }

    pub fn is_camera_ready(&self) -> Result<bool, Box<dyn Error>> {
        if let Some(camera_helper) = &self.camera_helper_instance {
            let mut env = self.java_vm.attach_current_thread()?;
            let ready = env.call_method(
                camera_helper.as_obj(),
                "isSessionReady",
                "()Z",
                &[],
            )?.z()?;
            Ok(ready)
        } else {
            Ok(false)
        }
    }

    pub fn close_camera(&self) -> Result<(), Box<dyn Error>> {
        if let Some(camera_helper) = &self.camera_helper_instance {
            let mut env = self.java_vm.attach_current_thread()?;
            env.call_method(
                camera_helper.as_obj(),
                "closeCamera",
                "()V",
                &[],
            )?;
            println!("Camera closed successfully");
        }
        Ok(())
    }
}


























































// #[cfg(target_os = "android")]
// use image::{Rgba, RgbaImage};
// use jni::objects::JClass;
// use jni::sys::jobject;
// #[cfg(target_os = "android")]
// use jni::{
//     objects::{GlobalRef, JByteBuffer, JObject, JObjectArray, JString, JValue},
//     JNIEnv, JavaVM,
// };
// use jni_min_helper::*;
//
// #[cfg(target_os = "android")]
// use ndk_context::android_context;
// #[cfg(target_os = "android")]
// use std::error::Error;
// #[cfg(target_os = "android")]
// use std::thread;
// #[cfg(target_os = "android")]
// use std::time::Duration;
//
// #[cfg(target_os = "android")]
// #[derive(Debug)]
// pub struct AndroidCamera {
//     java_vm: JavaVM,
//     app_context: GlobalRef,
//     camera_manager: GlobalRef,
//     camera_helper_class_loader: Option<GlobalRef>,
//     camera_helper_instance: Option<GlobalRef>,
// }
//
// #[cfg(target_os = "android")]
// impl AndroidCamera {
//     pub fn new() -> Result<Self, Box<dyn Error>> {
//         let jvm = unsafe { JavaVM::from_raw(android_context().vm() as *mut _)? };
//
//         let (global_context, global_camera_manager) = {
//             let mut env = jvm.attach_current_thread()?;
//
//             let ctx_ptr = android_context().context();
//             if ctx_ptr.is_null() {
//                 return Err("Failed to get Android context".into());
//             }
//
//             let context_obj = unsafe { JObject::from_raw(ctx_ptr as jobject) };
//             let global_context = env.new_global_ref(context_obj)?;
//             let global_camera_manager =
//                 Self::initialize_camera_manager_static(&mut env, &global_context)?;
//
//             (global_context, global_camera_manager)
//         };
//
//         Ok(Self {
//             java_vm: jvm,
//             app_context: global_context,
//             camera_manager: global_camera_manager,
//             camera_helper_class_loader: None,
//             camera_helper_instance: None,
//         })
//     }
//
//     fn initialize_camera_manager_static(
//         env: &mut JNIEnv,
//         context: &GlobalRef,
//     ) -> Result<GlobalRef, Box<dyn Error>> {
//         let camera_service = env
//             .get_static_field("android/content/Context", "CAMERA_SERVICE", "Ljava/lang/String;")?
//             .l()?;
//
//         let manager = env
//             .call_method(
//                 context.as_obj(),
//                 "getSystemService",
//                 "(Ljava/lang/String;)Ljava/lang/Object;",
//                 &[JValue::Object(&camera_service)],
//             )?
//             .l()?;
//
//         Ok(env.new_global_ref(manager)?)
//     }
//
//     unsafe fn copy_dex(&mut self) -> Result<(), Box<dyn Error>> {
//         {
//             let mut env = self.java_vm.attach_current_thread()?;
//             println!("Starting to copy dex file.");
//
//             let code_cache_dir_obj = env
//                 .call_method(self.app_context.as_obj(), "getCodeCacheDir", "()Ljava/io/File;", &[])?
//                 .l()?;
//             let abs_path_obj = env
//                 .call_method(code_cache_dir_obj, "getAbsolutePath", "()Ljava/lang/String;", &[])?
//                 .l()?;
//             let code_cache_path: String = env.get_string(&JString::from(abs_path_obj))?.into();
//             println!("Code cache path: {}", code_cache_path);
//
//             let secondary_dex_path = format!("{}/secondary-dexes", code_cache_path);
//             let file_class = env.find_class("java/io/File")?;
//             let secondary_dex_jstr = env.new_string(&secondary_dex_path)?;
//             let secondary_dex_file_obj = env.new_object(file_class, "(Ljava/lang/String;)V", &[JValue::Object(&secondary_dex_jstr)])?;
//             env.call_method(secondary_dex_file_obj, "mkdirs", "()Z", &[])?;
//             println!("Ensured secondary-dexes directory exists.");
//
//             let asset_manager_obj = env
//                 .call_method(self.app_context.as_obj(), "getAssets", "()Landroid/content/res/AssetManager;", &[])?
//                 .l()?;
//             let asset_name_jstr = env.new_string("classes.dex")?;
//             let input_stream_obj = env
//                 .call_method(asset_manager_obj, "open", "(Ljava/lang/String;)Ljava/io/InputStream;", &[JValue::Object(&asset_name_jstr)])?
//                 .l()?;
//             println!("Opened input stream for 'classes.dex'.");
//
//             let dest_file_path = format!("{}/classes.dex", secondary_dex_path);
//             let dest_file_jstr = env.new_string(&dest_file_path)?;
//             let fos_class = env.find_class("java/io/FileOutputStream")?;
//             let file_output_stream_obj = env.new_object(fos_class, "(Ljava/lang/String;)V", &[JValue::Object(&dest_file_jstr)])?;
//             println!("Prepared output stream for destination file.");
//
//             let buffer_size = 4096;
//             let buffer = env.new_byte_array(buffer_size)?;
//
//             loop {
//                 let bytes_read = env.call_method(&input_stream_obj, "read", "([B)I", &[JValue::Object(&buffer)])?.i()?;
//                 if bytes_read == -1 {
//                     break;
//                 }
//                 env.call_method(
//                     &file_output_stream_obj,
//                     "write",
//                     "([BII)V",
//                     &[JValue::Object(&buffer), JValue::Int(0), JValue::Int(bytes_read)],
//                 )?;
//             }
//
//             env.call_method(input_stream_obj, "close", "()V", &[])?;
//             env.call_method(file_output_stream_obj, "close", "()V", &[])?;
//             println!("Dex file copied successfully to {}", dest_file_path);
//         }
//
//         self.dex_loader()?;
//         Ok(())
//     }
//
//     unsafe fn dex_loader(&mut self) -> Result<(), Box<dyn Error>> {
//         let mut env = self.java_vm.attach_current_thread()?;
//         println!("Starting dex_loader");
//
//         let dex_path = "/data/user/0/com.orange.pkg/code_cache/secondary-dexes/classes.dex";
//         let optimized_dir = "/data/user/0/com.orange.pkg/code_cache";
//
//         let dex_path_java = env.new_string(dex_path)?;
//         println!("Dex path set: {}", dex_path);
//         let optimized_dir_java = env.new_string(optimized_dir)?;
//         println!("Optimized directory set: {}", optimized_dir);
//         let null_str = JObject::null();
//
//         let context_class = env.get_object_class(&self.app_context)?;
//         println!("Retrieved context class: {:?}", context_class);
//         let get_class_loader_method = env.get_method_id(context_class, "getClassLoader", "()Ljava/lang/ClassLoader;")?;
//         println!("Retrieved getClassLoader method ID: {:?}", get_class_loader_method);
//         let parent_class_loader = env.call_method_unchecked(
//             &self.app_context,
//             get_class_loader_method,
//             jni::signature::ReturnType::Object,
//             &[],
//         )?.l()?;
//         println!("Parent class loader obtained: {:?}", parent_class_loader);
//
//         let dex_class_loader_class = env.find_class("dalvik/system/DexClassLoader")?;
//         println!("DexClassLoader class found: {:?}", dex_class_loader_class);
//         let constructor_id = env.get_method_id(
//             &dex_class_loader_class,
//             "<init>",
//             "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/ClassLoader;)V",
//         )?;
//         println!("DexClassLoader constructor ID retrieved: {:?}", constructor_id);
//
//         let dex_class_loader_obj = env.new_object_unchecked(
//             dex_class_loader_class,
//             constructor_id,
//             &[
//                 JValue::Object(&dex_path_java).as_jni(),
//                 JValue::Object(&optimized_dir_java).as_jni(),
//                 JValue::Object(&null_str).as_jni(),
//                 JValue::Object(&parent_class_loader).as_jni(),
//             ],
//         )?;
//         println!("DexClassLoader instantiated: {:?}", dex_class_loader_obj);
//
//         self.camera_helper_class_loader = Some(env.new_global_ref(dex_class_loader_obj)?);
//
//         let thread = env.call_static_method("java/lang/Thread", "currentThread", "()Ljava/lang/Thread;", &[])?.l()?;
//         println!("Current thread obtained: {:?}", thread);
//         env.call_method(
//             thread,
//             "setContextClassLoader",
//             "(Ljava/lang/ClassLoader;)V",
//             &[JValue::Object(self.camera_helper_class_loader.as_ref().unwrap().as_obj())],
//         )?;
//         println!("Context class loader set.");
//
//         let class_name = env.new_string("com.orangeme.camera.CameraHelper")?;
//         let camera_helper_class = env.call_method(
//             self.camera_helper_class_loader.as_ref().unwrap().as_obj(),
//             "loadClass",
//             "(Ljava/lang/String;)Ljava/lang/Class;",
//             &[JValue::Object(&class_name)],
//         )?.l()?;
//         println!("CameraHelper class loaded: {:?}", camera_helper_class);
//
//         let camera_helper_class_jclass = JClass::from(camera_helper_class);
//         println!("Retrieved CameraHelper JClass: {:?}", camera_helper_class_jclass);
//         let camera_helper_constructor = env.get_method_id(
//             &camera_helper_class_jclass,
//             "<init>",
//             "(Landroid/content/Context;)V",
//         )?;
//         println!("CameraHelper constructor ID retrieved: {:?}", camera_helper_constructor);
//
//         let camera_helper_obj = env.new_object_unchecked(
//             camera_helper_class_jclass,
//             camera_helper_constructor,
//             &[JValue::Object(&self.app_context).as_jni()],
//         )?;
//         println!("CameraHelper instance created: {:?}", camera_helper_obj);
//
//         self.camera_helper_instance = Some(env.new_global_ref(camera_helper_obj)?);
//
//         Ok(())
//     }
//
//     pub fn has_camera_permission(&self) -> Result<bool, Box<dyn Error>> {
//         let mut env = self.java_vm.attach_current_thread()?;
//         let camera_helper = self.camera_helper_instance.as_ref().ok_or("CameraHelper not initialized")?;
//
//         let has_permission = env.call_method(
//             camera_helper.as_obj(),
//             "hasCameraPermission",
//             "()Z",
//             &[],
//         )?.z()?;
//
//         Ok(has_permission)
//     }
//
//     pub fn request_camera_permission(&self) -> Result<(), Box<dyn Error>> {
//         let mut env = self.java_vm.attach_current_thread()?;
//         let camera_helper = self.camera_helper_instance.as_ref().ok_or("CameraHelper not initialized")?;
//
//         env.call_method(
//             camera_helper.as_obj(),
//             "requestCameraPermission",
//             "()V",
//             &[],
//         )?;
//
//         Ok(())
//     }
//
//     pub fn is_waiting_for_permission(&self) -> Result<bool, Box<dyn Error>> {
//         let mut env = self.java_vm.attach_current_thread()?;
//         let camera_helper = self.camera_helper_instance.as_ref().ok_or("CameraHelper not initialized")?;
//
//         let waiting = env.call_method(
//             camera_helper.as_obj(),
//             "isWaitingForPermission",
//             "()Z",
//             &[],
//         )?.z()?;
//
//         Ok(waiting)
//     }
//
//     pub fn wait_for_permission(&self, timeout_seconds: u64) -> Result<bool, Box<dyn Error>> {
//         println!("Waiting for camera permission (timeout: {}s)", timeout_seconds);
//
//         let start_time = std::time::Instant::now();
//         let timeout = Duration::from_secs(timeout_seconds);
//
//         // First check if we already have permission
//         if self.has_camera_permission()? {
//             println!("Camera permission already granted");
//             return Ok(true);
//         }
//
//         // Request permission if we don't have it
//         self.request_camera_permission()?;
//
//         // Wait for permission to be granted or denied
//         while start_time.elapsed() < timeout {
//             if self.has_camera_permission()? {
//                 println!("Camera permission granted!");
//                 return Ok(true);
//             }
//
//             if !self.is_waiting_for_permission()? {
//                 println!("No longer waiting for permission - likely denied");
//                 return Ok(false);
//             }
//
//             println!("Still waiting for camera permission...");
//             thread::sleep(Duration::from_millis(500));
//         }
//
//         println!("Timeout waiting for camera permission");
//         Ok(false)
//     }
//
//     pub fn open_camera(&mut self) -> Result<(), Box<dyn Error>> {
//         // Initialize the camera helper first
//         unsafe {
//             self.copy_dex()?;
//         }
//
//         println!("Checking camera permission before opening camera");
//
//         // Check and wait for permission if needed
//         if !self.has_camera_permission()? {
//             println!("Camera permission not granted, requesting...");
//             if !self.wait_for_permission(30)? {  // Wait up to 30 seconds for permission
//                 return Err("Camera permission not granted within timeout".into());
//             }
//         }
//
//         let mut env = self.java_vm.attach_current_thread()?;
//         let camera_helper = self.camera_helper_instance.as_ref().ok_or("CameraHelper not initialized")?;
//
//         let camera_id_list_obj = env.call_method(
//             camera_helper.as_obj(),
//             "getCameraIdList",
//             "()[Ljava/lang/String;",
//             &[],
//         )?.l()?;
//
//         let camera_id_array = JObjectArray::from(camera_id_list_obj);
//         let length = env.get_array_length(&camera_id_array)?;
//         if length == 0 {
//             return Err("No cameras available".into());
//         }
//
//         let first_camera_id = env.get_object_array_element(&camera_id_array, 0)?;
//         let camera_id_str: String = env.get_string(&JString::from(first_camera_id))?.into();
//         let camera_id_jstr = env.new_string(&camera_id_str)?;
//
//         println!("Opening camera with ID: {}", camera_id_str);
//
//         env.call_method(
//             camera_helper.as_obj(),
//             "openCamera",
//             "(Ljava/lang/String;)V",
//             &[JValue::Object(&camera_id_jstr)],
//         )?;
//
//         println!("Camera open request sent successfully");
//         Ok(())
//     }
//
//     pub fn wait_for_camera_ready(&self, timeout_seconds: u64) -> Result<bool, Box<dyn Error>> {
//         println!("Waiting for camera session to be ready (timeout: {}s)", timeout_seconds);
//
//         let start_time = std::time::Instant::now();
//         let timeout = Duration::from_secs(timeout_seconds);
//
//         while start_time.elapsed() < timeout {
//             match self.is_camera_ready() {
//                 Ok(true) => {
//                     println!("Camera session is ready!");
//                     return Ok(true);
//                 }
//                 Ok(false) => {
//                     println!("Camera session not ready yet, waiting...");
//                     thread::sleep(Duration::from_millis(100));
//                 }
//                 Err(e) => {
//                     println!("Error checking camera ready status: {}", e);
//                     return Err(e);
//                 }
//             }
//         }
//
//         println!("Timeout waiting for camera session to be ready");
//         Ok(false)
//     }
//
//     fn get_image_dimensions(&self, image: &JObject) -> Result<(i32, i32), Box<dyn Error>> {
//         let mut env_guard = self.java_vm.attach_current_thread()?;
//         let env = &mut *env_guard;
//
//         let width = env.call_method(image, "getWidth", "()I", &[])?.i()?;
//         let height = env.call_method(image, "getHeight", "()I", &[])?.i()?;
//
//         println!("Image dimensions - Width: {}, Height: {}", width, height);
//         Ok((width, height))
//     }
//
//     fn convert_yuv_to_rgba(
//         &self,
//         image: &JObject,
//         width: i32,
//         height: i32,
//     ) -> Result<Vec<u8>, Box<dyn Error>> {
//         println!("Starting convert_yuv_to_rgba");
//         let mut env = self.java_vm.attach_current_thread()?;
//
//         let planes: JObjectArray = env
//             .call_method(image, "getPlanes", "()[Landroid/media/Image$Plane;", &[])?
//             .l()?
//             .into();
//
//         let plane_count = env.get_array_length(&planes)?;
//         println!("Number of planes: {}", plane_count);
//
//         if plane_count < 3 {
//             return Err("Image does not have the expected YUV planes".into());
//         }
//
//         let mut extract = |idx| -> Result<(Vec<u8>, i32, i32), Box<dyn Error>> {
//             println!("Extracting plane index: {}", idx);
//             let plane = env.get_object_array_element(&planes, idx)?;
//             let buffer = env.call_method(&plane, "getBuffer", "()Ljava/nio/ByteBuffer;", &[])?.l()?;
//             let byte_buffer = JByteBuffer::from(buffer);
//
//             let len = env.get_direct_buffer_capacity(&byte_buffer)?;
//             let ptr = env.get_direct_buffer_address(&byte_buffer)?;
//             let data = unsafe { std::slice::from_raw_parts(ptr, len).to_vec() };
//
//             let row_stride = env.call_method(&plane, "getRowStride", "()I", &[])?.i()?;
//             let pixel_stride = env.call_method(&plane, "getPixelStride", "()I", &[])?.i()?;
//
//             println!(
//                 "Plane {}: len = {}, row_stride = {}, pixel_stride = {}",
//                 idx, len, row_stride, pixel_stride
//             );
//
//             Ok((data, row_stride, pixel_stride))
//         };
//
//         let (y, y_rs, y_ps) = extract(0)?;
//         let (u, u_rs, u_ps) = extract(1)?;
//         let (v, v_rs, v_ps) = extract(2)?;
//
//         let mut rgba = Vec::with_capacity((width * height * 4) as usize);
//
//         for row in 0..height {
//             for col in 0..width {
//                 let yi = (row * y_rs + col * y_ps) as usize;
//                 let ui = ((row / 2) * u_rs + (col / 2) * u_ps) as usize;
//                 let vi = ((row / 2) * v_rs + (col / 2) * v_ps) as usize;
//
//                 let y_val = y.get(yi).copied().unwrap_or(0) as i32;
//                 let u_val = u.get(ui).copied().unwrap_or(128) as i32;
//                 let v_val = v.get(vi).copied().unwrap_or(128) as i32;
//
//                 let c = y_val - 16;
//                 let d = u_val - 128;
//                 let e = v_val - 128;
//
//                 let r = ((298 * c + 409 * e + 128) >> 8).clamp(0, 255) as u8;
//                 let g = ((298 * c - 100 * d - 208 * e + 128) >> 8).clamp(0, 255) as u8;
//                 let b = ((298 * c + 516 * d + 128) >> 8).clamp(0, 255) as u8;
//
//                 rgba.extend_from_slice(&[r, g, b, 255]);
//             }
//         }
//
//         println!("Finished convert_yuv_to_rgba");
//         Ok(rgba)
//     }
//
//     pub fn get_latest_frame(&self) -> Result<RgbaImage, Box<dyn Error>> {
//         println!("Starting get_latest_frame");
//
//         // Ensure we have permission before trying to get frame
//         if !self.has_camera_permission()? {
//             return Err("Camera permission not granted".into());
//         }
//
//         if !self.wait_for_camera_ready(10)? {
//             return Err("Camera session not ready within timeout".into());
//         }
//
//         let mut env = self.java_vm.attach_current_thread()?;
//         println!("Attached to Java thread.");
//
//         let camera_helper = self.camera_helper_instance.as_ref().ok_or("CameraHelper not initialized")?;
//         println!("CameraHelper instance retrieved: {:?}", camera_helper);
//
//         let session_ready = env.call_method(
//             camera_helper.as_obj(),
//             "isSessionReady",
//             "()Z",
//             &[],
//         )?.z()?;
//         println!("Session ready status: {}", session_ready);
//
//         if !session_ready {
//             return Err("Camera session still not ready after waiting".into());
//         }
//
//         let image_obj = env.call_method(
//             camera_helper.as_obj(),
//             "acquireLatestImage",
//             "()Landroid/media/Image;",
//             &[],
//         )?.l()?;
//         println!("Acquired image object: {:?}", image_obj);
//
//         if image_obj.is_null() {
//             println!("No image available.");
//             return Err("No image available".into());
//         }
//
//         println!("Image acquired successfully: {:?}", image_obj);
//
//         let (w, h) = self.get_image_dimensions(&image_obj)?;
//         println!("Image dimensions retrieved: Width = {}, Height = {}", w, h);
//
//         let data = self.convert_yuv_to_rgba(&image_obj, w, h)?;
//         println!("YUV to RGBA conversion completed. Data length: {}", data.len());
//
//
//         let mut img = RgbaImage::new(w as u32, h as u32);
//         println!("Created new RgbaImage with dimensions: {}x{}", w, h);
//
//         for (i, px) in data.chunks_exact(4).enumerate() {
//             let x = (i % w as usize) as u32;
//             let y = (i / w as usize) as u32;
//             img.put_pixel(x, y, Rgba([px[0], px[1], px[2], px[3]]));
//         }
//         println!("Populated RgbaImage with pixel data.");
//
//         env.call_method(&image_obj, "close", "()V", &[])?;
//         println!("Closed image to free resources.");
//
//         println!("Successfully converted image to RGBA");
//         Ok(img)
//     }
//
//     pub fn is_camera_ready(&self) -> Result<bool, Box<dyn Error>> {
//         if let Some(camera_helper) = &self.camera_helper_instance {
//             let mut env = self.java_vm.attach_current_thread()?;
//             let ready = env.call_method(
//                 camera_helper.as_obj(),
//                 "isSessionReady",
//                 "()Z",
//                 &[],
//             )?.z()?;
//             Ok(ready)
//         } else {
//             Ok(false)
//         }
//     }
//
//     pub fn close_camera(&self) -> Result<(), Box<dyn Error>> {
//         if let Some(camera_helper) = &self.camera_helper_instance {
//             let mut env = self.java_vm.attach_current_thread()?;
//             env.call_method(
//                 camera_helper.as_obj(),
//                 "closeCamera",
//                 "()V",
//                 &[],
//             )?;
//             println!("Camera closed successfully");
//         }
//         Ok(())
//     }
// }
