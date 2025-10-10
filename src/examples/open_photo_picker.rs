let (tx, rx) = channel::<(Vec<u8>, ImageOrientation)>();
hardware_context.open_photo_picker(tx);