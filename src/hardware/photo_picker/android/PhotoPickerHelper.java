//NOTE
//if you are changing the .java source file, ensure you delete the stale .class and .dex files prior to recompiling and embedding

//compile with java
// javac --release 8 -cp /absolute/path/to/android.jar PhotoPickerHelper.java

//embed .dex
// /absolute/path/to/Android/sdk/build-tools/d8 --lib /absolute/path/to/android.jar PhotoPickerHelper.class --output .
package com.maverick.photo;

import android.content.Context;
import android.content.Intent;

public class PhotoPickerHelper {

    // Loaded via InMemoryDexClassLoader at runtime, so PhotoPickerActivity (compiled into the
    // app's own build-time dex and declared in the manifest) can't be referenced by class - it's
    // a different classloader. Intent component resolution by name sidesteps that entirely.
    public static void openPhotoPicker(Context context, long callbackPointer) {
        Intent intent = new Intent();
        intent.setClassName(context.getPackageName(), "com.maverick.photo.PhotoPickerActivity");
        intent.putExtra("callback_ptr", callbackPointer);
        try {
            context.startActivity(intent);
        } catch (Exception e) {
            // PhotoPickerActivity not found/declared - nothing to fall back to here since the
            // native callback is now owned by that Activity, not this class.
        }
    }
}
