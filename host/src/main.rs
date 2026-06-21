use xcap::Monitor;
use std::time::Duration;
use std::thread::sleep;
use tokio::net::TcpStream;
use tokio::io::{AsyncWriteExt, AsyncReadExt};
use common::protocol::{FramePacket, InputPacket, RelayPacket, Command};
use image::DynamicImage;
use std::process::Command as ProcessCommand;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("--- Launching Remote Presenter Host (Relay Mode) ---");

    let room_id = "555555".to_string();
    println!("[HOST] Connecting to Relay Server at 165.245.178.149:6790...");
    let mut socket = TcpStream::connect("165.245.178.149:6790").await?;

    // 1. Send RegisterHost packet to Relay
    let register_pkt = RelayPacket::RegisterHost { room_id: room_id.clone() };
    let bytes = bincode::serialize(&register_pkt)?;
    socket.write_all(&(bytes.len() as u32).to_be_bytes()).await?;
    socket.write_all(&bytes).await?;

    // 2. Wait for confirmation from Relay
    let mut len_buf = [0u8; 4];
    socket.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    let mut buf = vec![0u8; len];
    socket.read_exact(&mut buf).await?;

    if let Ok(RelayPacket::HostRegistered { .. }) = bincode::deserialize(&buf) {
        println!("[HOST] Successfully registered Room '{}'. Waiting for client to join...", room_id);
    } else {
        eprintln!("[HOST] Failed to register room on relay.");
        return Ok(());
    }

    // 3. Wait for ClientConnected signal from Relay
    socket.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    let mut buf = vec![0u8; len];
    socket.read_exact(&mut buf).await?;

    if let Ok(RelayPacket::ClientConnected) = bincode::deserialize(&buf) {
        println!("\n[HOST] ⚡ CLIENT JOINED! TCP Streams are now fused. Starting engine... ⚡\n");
    } else {
        eprintln!("[HOST] Unexpected handshake response.");
        return Ok(());
    }

    // --- ENGINE LOGIC (Exactly same as before) ---
    let primary_monitor = Monitor::all().unwrap()[0].clone();
    let host_width = primary_monitor.width() as f32;
    let host_height = primary_monitor.height() as f32;

    let (mut read_half, mut write_half) = socket.into_split();

    let (input_tx, mut input_rx) = tokio::sync::mpsc::unbounded_channel::<InputPacket>();

    std::thread::spawn(move || {
        while let Some(input) = input_rx.blocking_recv() {
            match input {
                InputPacket::MouseMove { x_percent, y_percent } => {
                    let target_x = (x_percent * host_width) as i32;
                    let target_y = (y_percent * host_height) as i32;
                    let _ = ProcessCommand::new("xdotool").args(&["mousemove", &target_x.to_string(), &target_y.to_string()]).spawn();
                }
                InputPacket::MouseClick { button } => {
                    if button == 1 {
                        let _ = ProcessCommand::new("xdotool").args(&["click", "1"]).spawn();
                    }
                }
            }
        }
    });

    tokio::spawn(async move {
        loop {
            let mut len_buf = [0u8; 4];
            if read_half.read_exact(&mut len_buf).await.is_err() { break; }
            let len = u32::from_be_bytes(len_buf) as usize;
            
            let mut packet_buf = vec![0u8; len];
            if read_half.read_exact(&mut packet_buf).await.is_err() { break; }

            // Try RelayPacket first (commands from Relay)
            if let Ok(pkt) = bincode::deserialize::<RelayPacket>(&packet_buf) {
                match pkt {
                    RelayPacket::RelayCommand(cmd) => {
                        match cmd {
                            Command::Execute { cmd } => {
                                println!("[HOST] Execute command requested: {}", cmd);
                                let _ = ProcessCommand::new("sh").arg("-c").arg(&cmd).spawn();
                            }
                            _ => {
                                println!("[HOST] RelayCommand received: {:?}", cmd);
                            }
                        }
                        continue;
                    }
                    RelayPacket::CommandResponse { message } => {
                        println!("[HOST] Relay response: {}", message);
                        continue;
                    }
                    RelayPacket::Error { message } => {
                        eprintln!("[HOST] Relay error: {}", message);
                        continue;
                    }
                    _ => {
                        // ignore other relay control packets here
                        continue;
                    }
                }
            }

            // Fallback: Input packets (mouse, etc.)
            if let Ok(input) = bincode::deserialize::<InputPacket>(&packet_buf) {
                let _ = input_tx.send(input);
            } else {
                // Unknown packet type
                println!("[HOST] Received unknown packet type ({} bytes)", packet_buf.len());
            }
        }
    });

    let mut last_raw_frame: Vec<u8> = Vec::new();

    loop {
        if let Ok(image_buffer) = primary_monitor.capture_image() {
            let current_raw_slice = image_buffer.as_raw();

            if current_raw_slice == last_raw_frame.as_slice() {
                sleep(Duration::from_millis(33));
                continue; 
            }

            last_raw_frame = current_raw_slice.clone();

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

    Ok(())
}