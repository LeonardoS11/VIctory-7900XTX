//! Local convenience: `VICTORY_VOLUME=<percent>` sets this process's volume
//! in the Windows mixer (its audio session on the default output), so the
//! title's own mix can stay as it is. Windows remembers the level per
//! executable, exactly as if the slider in the volume mixer had been moved.

use windows::Win32::Media::Audio::{
    IAudioSessionManager, IMMDeviceEnumerator, MMDeviceEnumerator, eConsole, eRender,
};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
};

/// Applies `VICTORY_VOLUME` (0-100) if it is set.
pub fn apply() {
    let Some(percent) = std::env::var("VICTORY_VOLUME")
        .ok()
        .and_then(|s| s.trim().parse::<f32>().ok())
    else {
        return;
    };
    let level = (percent / 100.0).clamp(0.0, 1.0);
    let _ = std::thread::Builder::new()
        .name("victory volume".into())
        .spawn(move || {
            // SAFETY: plain COM calls on a thread of our own.
            let result: windows::core::Result<()> = unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                (|| {
                    let devices: IMMDeviceEnumerator =
                        CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
                    let device = devices.GetDefaultAudioEndpoint(eRender, eConsole)?;
                    let sessions: IAudioSessionManager = device.Activate(CLSCTX_ALL, None)?;
                    let volume = sessions.GetSimpleAudioVolume(None, 0)?;
                    volume.SetMasterVolume(level, std::ptr::null())
                })()
            };
            match result {
                Ok(()) => tracing::info!(percent, "process volume set (VICTORY_VOLUME)"),
                Err(error) => tracing::warn!(%error, "VICTORY_VOLUME: could not set the volume"),
            }
        });
}
