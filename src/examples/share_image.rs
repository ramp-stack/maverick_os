let my_image: RgbaImage = ImageReader::open("dog.png")
    .expect("Failed to open image")
    .decode()
    .expect("Failed to decode image")
    .into_rgba8();
    
hardware_context.share_image(my_image);