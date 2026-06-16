use tokio::net::TcpStream;
use tokio::io::AsyncReadExt;
use common::protocol::FramePacket;
use slint::{Image, SharedPixelBuffer, Rgba8Pixel};

slint::slint! {
    export component ViewerWindow inherits Window {
        title: "Remote Presenter - Viewer";
        preferred-width: 1280px;
        preferred-height: 720px;
        background: black;
        
        in property <image> video_frame;

        Image {
            source: root.video_frame;
            width: 100%;
            height: 100%;
            image-fit: contain;
        }
    }
}

#[tokio::main]
async fn main() {
    println!("--- Launching Remote Presenter Client UI ---");

    let ui = ViewerWindow::new().unwrap();
    let ui_handle = ui.as_weak();

    tokio::spawn(async move {
        println!("Connecting to Host at 127.0.0.1:8080...");
        let mut socket = match TcpStream::connect("127.0.0.1:8080").await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Failed to connect to host: {}", e);
                return;
            }
        };
        println!("Connected to Host machine pipeline successfully!");

        loop {
            let mut length_buffer = [0u8; 4];
            if socket.read_exact(&mut length_buffer).await.is_err() {
                println!("Host terminated the connection.");
                break;
            }
            let packet_length = u32::from_be_bytes(length_buffer) as usize;

            let mut frame_buffer = vec![0u8; packet_length];
            if socket.read_exact(&mut frame_buffer).await.is_err() {
                println!("Error losing packet data sync.");
                break;
            }

            if let Ok(FramePacket::VideoFrame(jpeg_data)) = bincode::deserialize(&frame_buffer) {
                if let Ok(dynamic_image) = image::load_from_memory(&jpeg_data) {
                    let rgba_image = dynamic_image.into_rgba8();
                    
                    // 1. Create the thread-safe pixel buffer
                    let buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                        rgba_image.as_raw(),
                        rgba_image.width(),
                        rgba_image.height(),
                    );

                    let ui_clone = ui_handle.clone();
                    
                    // 2. Move the raw buffer into the UI thread closure
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_clone.upgrade() {
                            // 3. Create the Slint Image SAFELY on the Main UI thread
                            let slint_image = Image::from_rgba8(buffer);
                            ui.set_video_frame(slint_image);
                        }
                    });
                }
            }
        }
    });

    ui.run().unwrap();
}