use xcap::Monitor;
use std::time::Duration;
use std::thread::sleep;
use tokio::net::TcpListener;
use tokio::io::{AsyncWriteExt, AsyncReadExt};
use common::protocol::{FramePacket, InputPacket};
use image::DynamicImage;
use enigo::{Enigo, MouseControllable, MouseButton};

#[tokio::main]
async fn main() {
    println!("--- Launching Remote Presenter Host Engine ---");

   let listener = TcpListener::bind("0.0.0.0:8080").await.unwrap();
    // FIXED: Removed 'mut' since socket is split immediately and doesn't need to be mutable
    let (socket, _) = listener.accept().await.unwrap();
    println!("Client connected!");

    let primary_monitor = Monitor::all().unwrap()[0].clone();
    let host_width = primary_monitor.width() as f32;
    let host_height = primary_monitor.height() as f32;

    let (mut read_half, mut write_half) = socket.into_split();

    // BACKGROUND TASK: Listen for Client Mouse Inputs
    tokio::spawn(async move {
        let mut enigo = Enigo::new();
        loop {
            let mut len_buf = [0u8; 4];
            if read_half.read_exact(&mut len_buf).await.is_err() { break; }
            let len = u32::from_be_bytes(len_buf) as usize;
            
            let mut packet_buf = vec![0u8; len];
            if read_half.read_exact(&mut packet_buf).await.is_err() { break; }

           if let Ok(input) = bincode::deserialize::<InputPacket>(&packet_buf) {
                match input {
                    InputPacket::MouseMove { x_percent, y_percent } => {
                        let target_x = (x_percent * host_width) as i32;
                        let target_y = (y_percent * host_height) as i32;
                        
                        // ADD THIS LINE to prove the network is working
                        println!("HOST RECEIVED: Move to {}x{}", target_x, target_y); 
                        
                        enigo.mouse_move_to(target_x, target_y);
                    }
                    InputPacket::MouseClick { button } => {
                        // ADD THIS LINE
                        println!("HOST RECEIVED: Click!"); 
                        
                        if button == 1 { enigo.mouse_click(MouseButton::Left); }
                    }
                }
            }
        }
    });

    // MAIN LOOP: Stream Video
    loop {
        if let Ok(image_buffer) = primary_monitor.capture_image() {
            let rgb_image = DynamicImage::ImageRgba8(image_buffer).into_rgb8();
            let mut jpeg_bytes = Vec::new();
            let mut encoder = image::codecs::jpeg::JpegEncoder::new(&mut jpeg_bytes);
            
            if encoder.encode(&rgb_image, rgb_image.width(), rgb_image.height(), image::ColorType::Rgb8.into()).is_ok() {
                let packet = FramePacket::VideoFrame(jpeg_bytes);
                if let Ok(serialized_data) = bincode::serialize(&packet) {
                    let len = serialized_data.len() as u32;
                    if write_half.write_all(&len.to_be_bytes()).await.is_err() { break; }
                    if write_half.write_all(&serialized_data).await.is_err() { break; }
                }
            }
        }
        sleep(Duration::from_millis(33));
    }
}