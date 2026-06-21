use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use common::protocol::{FramePacket, InputPacket};
use slint::{Image, SharedPixelBuffer, Rgba8Pixel};
use tokio::sync::mpsc;

slint::slint! {
    export component ViewerWindow inherits Window {
        title: "Remote Presenter";
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

        // Invisible touch layer that sits over the video
        TouchArea {
            width: 100%; height: 100%;
            pointer-event(event) => {
                if (event.button == PointerEventButton.left && event.kind == PointerEventKind.down) {
                    root.mouse-clicked();
                }
            }
            moved => {
                // Calculate percentage (0.0 to 1.0) and send to Rust logic
                root.mouse-moved(self.mouse-x / self.width, self.mouse-y / self.height);
            }
        }
    }
}

#[tokio::main]
async fn main() {
    let ui = ViewerWindow::new().unwrap();
    let ui_handle = ui.as_weak();

    // Create a communication channel between UI and Network
    let (tx, mut rx) = mpsc::unbounded_channel::<InputPacket>();

    let tx_move = tx.clone();
    ui.on_mouse_moved(move |x, y| {
        let _ = tx_move.send(InputPacket::MouseMove { x_percent: x, y_percent: y });
    });

    let tx_click = tx.clone();
    ui.on_mouse_clicked(move || {
        let _ = tx_click.send(InputPacket::MouseClick { button: 1 });
    });

    tokio::spawn(async move {
        let socket = TcpStream::connect("50.50.0.179:8080").await.unwrap();
        let (mut read_half, mut write_half) = socket.into_split();

        // Task to SEND inputs to host
        tokio::spawn(async move {
            while let Some(packet) = rx.recv().await {
                if let Ok(bytes) = bincode::serialize(&packet) {
                    let len = bytes.len() as u32;
                    let _ = write_half.write_all(&len.to_be_bytes()).await;
                    let _ = write_half.write_all(&bytes).await;
                }
            }
        });

        // Task to RECEIVE video from host
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
                        if let Some(ui) = ui_clone.upgrade() {
                            ui.set_video_frame(Image::from_rgba8(buffer));
                        }
                    });
                }
            }
        }
    });

    ui.run().unwrap();
}