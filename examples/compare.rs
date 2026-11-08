//! Render the three main expressions at three strengths for listening comparisons.
use mooduck_quacklib::{Emotion, Options, Synthesizer};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all("audio/comparison")?;
    let voice = Synthesizer::default();
    for emotion in [Emotion::Happy, Emotion::Curious, Emotion::Sad] {
        for intensity in [0.2, 0.6, 1.0] {
            let path = format!("audio/comparison/{emotion}-{intensity:.1}.wav");
            voice
                .render(
                    emotion,
                    Options {
                        intensity,
                        ..Options::default()
                    },
                )?
                .save_wav(&path)?;
            println!("{path}");
        }
    }
    Ok(())
}
