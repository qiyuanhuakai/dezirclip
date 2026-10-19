use crate::ports::{ClipboardPayload, ClipboardReader, ClipboardWriter, PlatformResult};

pub struct ClipboardService<R, W> {
    reader: R,
    writer: W,
}
impl<R: ClipboardReader, W: ClipboardWriter> ClipboardService<R, W> {
    pub fn new(reader: R, writer: W) -> Self {
        Self { reader, writer }
    }
    pub fn formats(&self) -> PlatformResult<Vec<String>> {
        self.reader.formats()
    }
    pub fn read(&self, mime: &str) -> PlatformResult<Vec<u8>> {
        self.reader.read_representation(mime)
    }
    /// Writes only; automatic paste remains a distinct PasteController port.
    pub fn write(&self, payload: ClipboardPayload) -> PlatformResult<()> {
        self.writer.write(payload)
    }
}
