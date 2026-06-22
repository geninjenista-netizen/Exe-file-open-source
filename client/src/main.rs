#![windows_subsystem = "windows"]
use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use common::protocol::{FramePacket, InputPacket, RelayPacket};
use slint::{Image, SharedPixelBuffer, Rgba8Pixel};
use tokio::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

slint::slint! {
    export component ViewerWindow inherits Window {
        title: "Remote Presenter Client";
        preferred-width: 1280px;
        preferred-height: 720px;
        background: black;
        
        in property <image> video_frame;
        callback mouse-moved(float, float);
        callback mouse-clicked();

        Image {
            source: root.video_frame;
            width: 100%; height: 100%;
            image-fit: contain;
        }

        TouchArea {
            width: 100%; height: 100%;
            pointer-event(event) => {
                if (event.button == PointerEventButton.left && event.kind == PointerEventKind.down) {
                    root.mouse-clicked();
                }
            }
            moved => {
                root.mouse-moved(self.mouse-x / self.width, self.mouse-y / self.height);
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let room_id = "555555".to_string();
    println!("[CLIENT] Connecting to Relay Server at 127.0.0.1:8081...");
    let mut socket = TcpStream::connect("50.50.0.18:8081").await?;

    // 1. Request connection to Room
    let connect_pkt = RelayPacket::ConnectToHost { room_id: room_id.clone() };
    let bytes = bincode::serialize(&connect_pkt)?;
    socket.write_all(&(bytes.len() as u32).to_be_bytes()).await?;
    socket.write_all(&bytes).await?;

    // 2. Wait for SessionReady confirmation from Relay
    let mut len_buf = [0u8; 4];
    socket.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    let mut buf = vec![0u8; len];
    socket.read_exact(&mut buf).await?;

    match bincode::deserialize::<RelayPacket>(&buf) {
        Ok(RelayPacket::SessionReady) => {
            println!("\n[CLIENT] ⚡ SUCCESS! Connected to Host Room '{}'. Handing off stream to UI... ⚡\n", room_id);
        }
        Ok(RelayPacket::RoomNotFound) => {
            eprintln!("[CLIENT Error]: Room ID '{}' does not exist or host is offline.", room_id);
            return Ok(());
        }
        _ => {
            eprintln!("[CLIENT Error]: Invalid relay response.");
            return Ok(());
        }
    }

    // --- UI & STREAMING ENGINE ---
    let ui = ViewerWindow::new()?;
    let ui_handle = ui.as_weak();

    let (tx, mut rx) = mpsc::unbounded_channel::<InputPacket>();
    let latest_mouse_pos: Arc<Mutex<Option<(f32, f32)>>> = Arc::new(Mutex::new(None));
    let mouse_pos_clone = latest_mouse_pos.clone();

    ui.on_mouse_moved(move |x, y| {
        if let Ok(mut pos) = mouse_pos_clone.lock() { *pos = Some((x, y)); }
    });

    let tx_click = tx.clone();
    ui.on_mouse_clicked(move || {
        println!("[CLIENT] 🖱️ UI Click Registered! Pushing to wire...");
        let _ = tx_click.send(InputPacket::MouseClick { button: 1 });
    });

    let tx_move = tx.clone();
    let throttler_pos = latest_mouse_pos.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(16));
        loop {
            interval.tick().await;
            let mut coord = None;
            if let Ok(mut pos) = throttler_pos.lock() { coord = pos.take(); }
            if let Some((x, y)) = coord { let _ = tx_move.send(InputPacket::MouseMove { x_percent: x, y_percent: y }); }
        }
    });

    let (mut read_half, mut write_half) = socket.into_split();

    tokio::spawn(async move {
        while let Some(packet) = rx.recv().await {
            if let Ok(bytes) = bincode::serialize(&packet) {
                let len = bytes.len() as u32;
                let _ = write_half.write_all(&len.to_be_bytes()).await;
                let _ = write_half.write_all(&bytes).await;
            }
        }
    });

    tokio::spawn(async move {
        loop {
            let mut len_buf = [0u8; 4];
            if read_half.read_exact(&mut len_buf).await.is_err() { break; }
            let len = u32::from_be_bytes(len_buf) as usize;

            let mut frame_buf = vec![0u8; len];
            if read_half.read_exact(&mut frame_buf).await.is_err() { break; }

            if let Ok(FramePacket::VideoFrame(jpeg_data)) = bincode::deserialize(&frame_buf) {
                if let Ok(dynamic_image) = image::load_from_memory(&jpeg_data) {
                    let rgba_image = dynamic_image.into_rgba8();
                    let buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                        rgba_image.as_raw(), rgba_image.width(), rgba_image.height(),
                    );
                    let ui_clone = ui_handle.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_clone.upgrade() { ui.set_video_frame(Image::from_rgba8(buffer)); }
                    });
                }
            }
        }
    });

    ui.run()?;
    Ok(())
}