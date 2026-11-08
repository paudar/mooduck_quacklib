use mooduck_quacklib::Audio;
use std::{error::Error, path::PathBuf};

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub fn play(audio: &Audio, output: Option<PathBuf>) -> Result<(), Box<dyn Error>> {
    #[cfg(target_os = "macos")]
    let players = ["/usr/bin/afplay"];
    #[cfg(target_os = "linux")]
    let players = ["pw-play", "paplay", "aplay"];

    play_with_players(audio, output, &players)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn play(_audio: &Audio, _output: Option<PathBuf>) -> Result<(), Box<dyn Error>> {
    Err(
        "direct playback supports macOS and Linux; use render --output voice.wav or the PCM API"
            .into(),
    )
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
struct TemporaryDirectory(PathBuf);

#[cfg(any(target_os = "macos", target_os = "linux"))]
impl TemporaryDirectory {
    fn new() -> std::io::Result<Self> {
        use std::os::unix::fs::DirBuilderExt;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(std::io::Error::other)?
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("mooduck_quacklib-{}-{stamp}", std::process::id()));
        std::fs::DirBuilder::new().mode(0o700).create(&path)?;
        Ok(Self(path))
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn play_with_players(
    audio: &Audio,
    output: Option<PathBuf>,
    players: &[&str],
) -> Result<(), Box<dyn Error>> {
    let _temporary;
    let path = match output {
        Some(path) => {
            crate::save(audio, &path)?;
            path
        }
        None => {
            _temporary = TemporaryDirectory::new()?;
            let path = _temporary.0.join("voice.wav");
            audio.save_wav(&path)?;
            path
        }
    };
    // Absolute paths also keep names beginning with '-' from becoming player options.
    let path = std::fs::canonicalize(path)?;
    let mut errors = Vec::new();
    let mut found = false;
    for player in players {
        match std::process::Command::new(player).arg(&path).output() {
            Ok(result) if result.status.success() => return Ok(()),
            Ok(result) => {
                found = true;
                let stderr = String::from_utf8_lossy(&result.stderr);
                errors.push(format!("{player}: {} {}", result.status, stderr.trim()));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                found = true;
                errors.push(format!("{player}: {error}"));
            }
        }
    }
    let hint = if cfg!(target_os = "linux") {
        "Install pw-play (PipeWire), paplay (PulseAudio), or aplay (ALSA), and ensure audio output is available."
    } else {
        "Ensure the system audio player and audio output are available."
    };
    if !found {
        return Err(format!("no audio player found (tried {}); {hint} Alternatively, use render --output voice.wav.", players.join(", ")).into());
    }
    Err(format!(
        "audio playback failed: {}; {hint} Alternatively, use render --output voice.wav.",
        errors.join("; ")
    )
    .into())
}

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod tests {
    use super::*;
    use mooduck_quacklib::{Emotion, Options, Synthesizer};
    use std::{os::unix::fs::PermissionsExt, path::Path};

    fn clip() -> Audio {
        Synthesizer::default()
            .render(Emotion::Yes, Options::default())
            .unwrap()
    }

    fn player(directory: &Path, name: &str, body: &str) -> PathBuf {
        let path = directory.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        path
    }

    #[test]
    fn missing_and_failed_players_fall_back_to_a_working_player() {
        let directory = TemporaryDirectory::new().unwrap();
        let missing = directory.0.join("missing");
        let failed = player(
            &directory.0,
            "failed",
            "printf 'no audio server' >&2; exit 1",
        );
        let success = player(
            &directory.0,
            "success",
            "test \"$#\" -eq 1 && test -s \"$1\"",
        );
        let unused = player(&directory.0, "unused", "exit 1");
        play_with_players(
            &clip(),
            None,
            &[
                missing.to_str().unwrap(),
                failed.to_str().unwrap(),
                success.to_str().unwrap(),
                unused.to_str().unwrap(),
            ],
        )
        .unwrap();
    }

    #[test]
    fn temporary_wav_is_removed_after_success_and_failure() {
        for exit in [0, 1] {
            let directory = TemporaryDirectory::new().unwrap();
            // Record the received WAV path without using a shell to launch the player.
            let script = player(
                &directory.0,
                "player",
                &format!("printf '%s' \"$1\" > \"$0.path\"; test -s \"$1\" || exit 2; exit {exit}"),
            );
            let result = play_with_players(&clip(), None, &[script.to_str().unwrap()]);
            assert_eq!(result.is_ok(), exit == 0);
            let path = std::fs::read_to_string(directory.0.join("player.path")).unwrap();
            let path = Path::new(&path);
            assert!(!path.exists());
            assert!(!path.parent().unwrap().exists());
        }
    }

    #[test]
    fn explicit_output_with_spaces_survives_playback_failure() {
        let directory = TemporaryDirectory::new().unwrap();
        let script = player(
            &directory.0,
            "player",
            "test \"$#\" -eq 1 && test -s \"$1\" || exit 2; printf 'device unavailable' >&2; exit 1",
        );
        let path = directory.0.join("saved sounds/curious duck.wav");
        let error = play_with_players(&clip(), Some(path.clone()), &[script.to_str().unwrap()])
            .unwrap_err()
            .to_string();
        assert!(error.contains("device unavailable"));
        assert!(error.contains("render --output"));
        assert!(std::fs::read(path).unwrap().starts_with(b"RIFF"));
    }

    #[test]
    fn missing_players_produce_an_actionable_error() {
        let directory = TemporaryDirectory::new().unwrap();
        let missing = directory.0.join("missing");
        let error = play_with_players(&clip(), None, &[missing.to_str().unwrap()])
            .unwrap_err()
            .to_string();
        assert!(error.contains("no audio player found"));
        assert!(error.contains("render --output voice.wav"));
    }
}
