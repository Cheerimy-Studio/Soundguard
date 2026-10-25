//! Shared measurement pipeline, with a Windows-only WASAPI backend.
include!("../../client/common/audio_core.rs");
use std::sync::mpsc::Sender;
#[cfg(windows)]
use std::time::Duration;
#[cfg(windows)]
#[path = "windows_audio.rs"]
mod backend;

#[derive(Clone)]
pub struct AudioEngineHandle {
    pub shared: Arc<AudioShared>,
    commands: Arc<Mutex<Option<Sender<backend_command::Command>>>>,
}
#[cfg(windows)]
mod backend_command {
    pub use super::backend::AudioCommand as Command;
}
#[cfg(not(windows))]
mod backend_command {
    pub enum Command {
        Start,
        Pause,
        Retry,
        Stop,
    }
}
impl AudioEngineHandle {
    pub fn start(offset: f32) -> Self {
        let shared = AudioShared::new(offset);
        shared.set_capturing(false);
        shared.set_engine(Box::new(crate::rnnoise_engine::RnnoiseEngine::new(
            DENOISE_OFF,
            false,
        )));
        #[cfg(windows)]
        let commands = Some(backend::spawn_audio_engine(shared.clone()));
        #[cfg(not(windows))]
        let commands = {
            shared.set_error(Some(
                "此平台不支持 WASAPI 采集，请在 Windows 10/11 上运行采集端".into(),
            ));
            None
        };
        Self {
            shared,
            commands: Arc::new(Mutex::new(commands)),
        }
    }
    fn command(&self, command: backend_command::Command) {
        if let Some(tx) = self
            .commands
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
        {
            let _ = tx.send(command);
        }
    }
    pub fn capture(&self, enabled: bool) {
        if self.shared.capturing() == enabled {
            return;
        }
        self.shared.set_capturing(enabled);
        self.command(if enabled {
            backend_command::Command::Start
        } else {
            backend_command::Command::Pause
        });
    }
    pub fn retry(&self) {
        self.command(backend_command::Command::Retry);
    }
    pub fn stop(&self) {
        self.shared.set_capturing(false);
        self.command(backend_command::Command::Stop);
    }
    #[cfg(test)]
    pub fn test_signal(&self) {
        self.shared.ready.store(true, Ordering::SeqCst);
        self.shared.set_capturing(true);
        self.shared.push_block(&[0.1; 480], 1, 48000);
    }
}
