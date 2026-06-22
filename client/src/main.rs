#![windows_subsystem = "windows"]

use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use common::protocol::{FramePacket, InputPacket, RelayPacket};
use slint::{Image, SharedPixelBuffer, Rgba8Pixel};
use tokio::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::fs::OpenOptions;
use std::io::Write;

// ==========================================
// 🔴 THE FLIGHT RECORDER (SILENT LOGGING)
// ==========================================
fn log_debug(msg: &str) {
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open("CLIENT_LOG.txt") {
        let _ = writeln!(file, "{}", msg);
    }
}

// ==========================================
// 🎨 SLINT GUI WINDOW DEFINITION
// ==========================================
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

// ==========================================
// 🚀 THE ACTUAL ENGINE (REAL MAIN)
// ==========================================
async fn real_main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = std::fs::remove_file("CLIENT_LOG.txt"); // Clean old log per launch
    log_debug("==================================================");
    log_debug("   🚀 REMOTE CLIENT LAUNCHED (FLIGHT RECORDER)    ");
    log_debug("==================================================");

    let room_id = "555555".to_string();
    log_debug("[CLIENT] Attempting TCP Connection to Linux Host at 10.0.0.20:8081...");

    let mut socket = TcpStream::connect("10.0.0.20:8081").await?;
    log_debug("[CLIENT] Socket Connected! Transmitting Room Join Handshake...");

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
            log_debug(&format!("[CLIENT] ⚡ SUCCESS! Fused to Room '{}'. Spawning Slint GUI...", room_id));
        }
        Ok(RelayPacket::RoomNotFound) => {
            let err = format!("Room ID '{}' does not exist on Relay, or Host is offline.", room_id);
            log_debug(&format!("[CLIENT ERROR] Handshake Rejected: {}", err));
            return Err(err.into());
        }
        _ => {
            let err = "Received invalid packet structure during handshake.";
            log_debug(&format!("[CLIENT ERROR] {}", err));
            return Err(err.into());
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
        log_debug("[CLIENT] 🖱️ UI Left-Click Registered! Pushing MouseClick to wire...");
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
            if let Some((x, y)) = coord {
                let _ = tx_move.send(InputPacket::MouseMove { x_percent: x, y_percent: y });
            }
        }
    });

    let (mut read_half, mut write_half) = socket.into_split();

    tokio::spawn(async move {
        while let Some(packet) = rx.recv().await {
            if let Ok(bytes) = bincode::serialize(&packet) {
                let len = bytes.len() as u32;
                if write_half.write_all(&len.to_be_bytes()).await.is_err() { break; }
                if write_half.write_all(&bytes).await.is_err() { break; }
            }
        }
    });

    tokio::spawn(async move {
        loop {
            let mut len_buf = [0u8; 4];
            if read_half.read_exact(&mut len_buf).await.is_err() {
                log_debug("[CLIENT WARN] Stream severed by Relay / Host.");
                break;
            }
            let len = u32::from_be_bytes(len_buf) as usize;

            let mut frame_buf = vec![0u8; len];
            if read_half.read_exact(&mut frame_buf).await.is_err() {
                log_debug("[CLIENT WARN] Failed to read complete frame payload.");
                break;
            }

            if let Ok(FramePacket::VideoFrame(jpeg_data)) = bincode::deserialize(&frame_buf) {
                if let Ok(dynamic_image) = image::load_from_memory(&jpeg_data) {
                    let rgba_image = dynamic_image.into_rgba8();
                    let buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                        rgba_image.as_raw(), rgba_image.width(), rgba_image.height(),
                    );
                    let ui_clone = ui_handle.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_clone.upgrade() {
                            ui.set_video_frame(Image::from_rgba8(buffer));
                        }
                    });
                }
            }
        }
    });

    log_debug("[CLIENT] Slint Engine running desktop window...");
    ui.run()?;
    log_debug("[CLIENT] UI Window closed cleanly by user.");
    Ok(())
}

// ==========================================
// 🛡️ THE MASTER CRASH TRAP (OUTER MAIN)
// ==========================================
#[tokio::main]
async fn main() {
    // Trap A: Catch standard panics
    std::panic::set_hook(Box::new(|panic_info| {
        let msg = format!("💥 RUST FATAL PANIC:\n{}", panic_info);
        log_debug(&msg);
        let _ = std::fs::write("CRASH_PANIC.txt", msg);
    }));

    // Trap B: Catch network connection rejections
    if let Err(e) = real_main().await {
        let msg = format!("❌ CLIENT TERMINATED WITH ERROR:\n{}", e);
        log_debug(&msg);
        let _ = std::fs::write("CRASH_NETWORK.txt", msg);
    }
}