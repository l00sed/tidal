#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioBackendKind {
    Cpal,
    CoreAudio,
    Jack,
}

pub fn available_backends() -> Vec<AudioBackendKind> {
    let mut out = vec![AudioBackendKind::Cpal];

    #[cfg(target_os = "macos")]
    {
        out.push(AudioBackendKind::CoreAudio);
    }

    #[cfg(feature = "jack-backend")]
    {
        out.push(AudioBackendKind::Jack);
    }

    out
}

pub fn default_backend() -> AudioBackendKind {
    std::env::var("TUI_MIXER_AUDIO_BACKEND")
        .ok()
        .as_deref()
        .map(|s| s.to_ascii_lowercase())
        .and_then(|s| match s.as_str() {
            "cpal" => Some(AudioBackendKind::Cpal),
            "coreaudio" => Some(AudioBackendKind::CoreAudio),
            "jack" => Some(AudioBackendKind::Jack),
            _ => None,
        })
        .filter(|requested| available_backends().contains(requested))
        .unwrap_or(AudioBackendKind::Cpal)
}

pub fn backend_label(backend: AudioBackendKind) -> &'static str {
    match backend {
        AudioBackendKind::Cpal => "cpal",
        AudioBackendKind::CoreAudio => "coreaudio",
        AudioBackendKind::Jack => "jack",
    }
}
