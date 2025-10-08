use super::app::App;
use super::background::BackgroundMessage;

/// Process all pending background messages from the channel
pub fn process_background_messages(
    app: &mut App,
    receiver: &mut tokio::sync::mpsc::UnboundedReceiver<BackgroundMessage>,
) {
    while let Ok(message) = receiver.try_recv() {
        app.process_background_message(message);
    }
}
