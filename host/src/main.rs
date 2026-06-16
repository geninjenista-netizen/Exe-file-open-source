use xcap::Monitor;
use std::time::Instant;
use std::thread::sleep;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::io::AsyncWriteExt;
use common::protocol::FramePacket;
use image::DynamicImage;

#[tokio::main]
async fn main() {
    println!("--- Launching Remote Presenter Host Engine ---");

    let listener = TcpListener::bind("127.0.0.1:8080").await.unwrap();
    println!("Server listening on 127.0.0.1:8080. Awaiting Client connection...");

    let (mut socket, addr) = listener.accept().await.unwrap();
    println!("Client connected successfully from: {}", addr);

    let monitors = Monitor::all().unwrap();
    if monitors.is_empty() {
        eprintln!("Error: No monitors detected!");
        return;
    }
    let primary_monitor = monitors[0].clone();
    println!("Capturing Target: {} ({}x{})", 
        primary_monitor.name(), 
        primary_monitor.width(), 
        primary_monitor.height()
    );

    loop {
        let start_time = Instant::now();

        if let Ok(image_buffer) = primary_monitor.capture_image() {
            
            // BUG FIX: Convert RGBA (transparency) to RGB because JPEGs don't support Alpha!
            let rgb_image = DynamicImage::ImageRgba8(image_buffer).into_rgb8();

            let mut jpeg_bytes: Vec<u8> = Vec::new();
            let mut encoder = image::codecs::jpeg::JpegEncoder::new(&mut jpeg_bytes);
            
            // Explicitly pass the RGB8 image format
            match encoder.encode(
                &rgb_image, 
                rgb_image.width(), 
                rgb_image.height(), 
                image::ColorType::Rgb8.into()
            ) {
                Ok(_) => {
                    let packet = FramePacket::VideoFrame(jpeg_bytes);
                    if let Ok(serialized_data) = bincode::serialize(&packet) {
                        let packet_length = serialized_data.len() as u32;
                        
                        if socket.write_all(&packet_length.to_be_bytes()).await.is_err() { break; }
                        if socket.write_all(&serialized_data).await.is_err() { break; }

                        println!(
                            "Transmitted Frame | Payload Size: {:.2} KB | Total Latency: {:?}",
                            packet_length as f32 / 1024.0,
                            start_time.elapsed()
                        );
                    }
                }
                Err(e) => {
                    // Stop hiding errors!
                    eprintln!("Failed to compress JPEG: {:?}", e);
                }
            }
        } else {
            eprintln!("Warning: Failed to capture frame from monitor.");
        }

        sleep(Duration::from_millis(33));
    }
}