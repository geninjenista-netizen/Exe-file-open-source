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
}
