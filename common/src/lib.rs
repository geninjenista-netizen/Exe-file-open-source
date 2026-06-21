pub mod protocol {
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub enum Command {
        // Presentation controls
        NextSlide,
        PrevSlide,
        MovePointer { x: f32, y: f32 },
        
        // Security/Auth
        Authenticate { token: String },
    }

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub enum FramePacket {
        // Holds the compressed image bytes of the screen/window
        VideoFrame(Vec<u8>),
    }

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub enum InputPacket {
        MouseMove { x_percent: f32, y_percent: f32 },
        MouseClick { button: u8 }, // 1 = Left Click
    }
}
