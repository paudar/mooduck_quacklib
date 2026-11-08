use mooduck_quacklib::{Audio, Emotion, MICRODUCK_SEED, Options, Synthesizer};
use std::{env, error::Error, path::PathBuf};

const HELP: &str = "mooduck_quacklib — expressive Microduck-inspired sounds

Usage:
  mooduck_quacklib list
  mooduck_quacklib play <emotion> [options]                 Play on macOS with afplay
  mooduck_quacklib render <emotion> --output <file.wav> [options]
  mooduck_quacklib demo [--output-dir <directory>] [options] Export all emotions

Options:
  --intensity <0..1>      Expression / energy (default: 0.7)
  --volume <0..1>         Linear volume (default: 0.6)
  --seed <u32>           Voice identity (default: 4145077059, Microduck reference)
  --variant <u32>        Repeatable delivery variation (default: 0)
  --sample-rate <hz>     8000..192000 (default: 48000)
  -o, --output <path>    WAV destination for render or play
  --output-dir <path>    Demo destination (default: audio)
  -h, --help             Show help

English and German emotion names are accepted, e.g. happy / freude.
Intensity zero is subdued; volume zero is silent. No audio dependencies needed.
";

struct Args {
    command: String,
    emotion: Option<Emotion>,
    options: Options,
    seed: u32,
    output: Option<PathBuf>,
    output_dir: Option<PathBuf>,
}

fn parse(args: &[String]) -> Result<Args, Box<dyn Error>> {
    let command = args[0].clone();
    if !["play", "render", "demo"].contains(&command.as_str()) {
        return Err(format!("unknown command {command:?}; see --help").into());
    }
    let mut parsed = Args {
        command,
        emotion: None,
        options: Options::default(),
        seed: MICRODUCK_SEED,
        output: None,
        output_dir: None,
    };
    let mut iter = args[1..].iter();
    if parsed.command != "demo" {
        parsed.emotion = Some(
            iter.next()
                .ok_or("missing emotion; see mooduck_quacklib list")?
                .parse()?,
        );
    }
    while let Some(flag) = iter.next() {
        if ![
            "--intensity",
            "--volume",
            "--sample-rate",
            "--seed",
            "--variant",
            "--output",
            "-o",
            "--output-dir",
        ]
        .contains(&flag.as_str())
        {
            return Err(format!("unknown option {flag:?}; see --help").into());
        }
        let value = iter
            .next()
            .ok_or_else(|| format!("missing value for {flag}"))?;
        match flag.as_str() {
            "--intensity" => parsed.options.intensity = value.parse()?,
            "--volume" => parsed.options.volume = value.parse()?,
            "--sample-rate" => parsed.options.sample_rate = value.parse()?,
            "--seed" => parsed.seed = value.parse()?,
            "--variant" => parsed.options.variant = value.parse()?,
            "--output" | "-o" => parsed.output = Some(value.into()),
            "--output-dir" => parsed.output_dir = Some(value.into()),
            _ => unreachable!(),
        }
    }
    if parsed.command == "demo" && parsed.output.is_some() {
        return Err("demo uses --output-dir, not --output".into());
    }
    if parsed.command != "demo" && parsed.output_dir.is_some() {
        return Err("--output-dir is only valid for demo".into());
    }
    if parsed.command == "render" && parsed.output.is_none() {
        return Err("render requires --output <file.wav>".into());
    }
    Ok(parsed)
}

fn save(audio: &Audio, path: &std::path::Path) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    audio.save_wav(path)?;
    println!(
        "{} ({:.2} s, {} Hz, mono)",
        path.display(),
        audio.duration_seconds(),
        audio.sample_rate()
    );
    Ok(())
}

#[cfg(target_os = "macos")]
fn play(audio: &Audio, output: Option<PathBuf>) -> Result<(), Box<dyn Error>> {
    // A private temp directory avoids collisions and is removed even if afplay fails.
    struct TemporaryDirectory(PathBuf);
    impl Drop for TemporaryDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let mut temporary = None;
    let path = match output {
        Some(path) => {
            save(audio, &path)?;
            path
        }
        None => {
            use std::os::unix::fs::DirBuilderExt;
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos();
            let dir =
                env::temp_dir().join(format!("mooduck_quacklib-{}-{stamp}", std::process::id()));
            std::fs::DirBuilder::new().mode(0o700).create(&dir)?;
            temporary = Some(TemporaryDirectory(dir.clone()));
            let path = dir.join("voice.wav");
            audio.save_wav(&path)?;
            path
        }
    };
    let status = std::process::Command::new("/usr/bin/afplay")
        .arg(&path)
        .status()?;
    drop(temporary);
    if !status.success() {
        return Err(format!("afplay failed ({status}); try render --output voice.wav").into());
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn play(_audio: &Audio, _output: Option<PathBuf>) -> Result<(), Box<dyn Error>> {
    Err(
        "direct playback currently supports macOS; use render --output voice.wav or the PCM API"
            .into(),
    )
}

fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() || args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{HELP}");
        return Ok(());
    }
    if args[0] == "list" {
        if args.len() != 1 {
            return Err("list takes no arguments".into());
        }
        for emotion in Emotion::ALL {
            println!("{emotion}");
        }
        return Ok(());
    }
    let args = parse(&args)?;
    let voice = Synthesizer::new(args.seed);
    if args.command == "demo" {
        // Validate and render before creating the output directory.
        let clips: Vec<_> = Emotion::ALL
            .into_iter()
            .map(|emotion| {
                voice
                    .render(emotion, args.options)
                    .map(|clip| (emotion, clip))
            })
            .collect::<Result<_, _>>()?;
        let directory = args.output_dir.unwrap_or_else(|| "audio".into());
        for (emotion, clip) in clips {
            save(&clip, &directory.join(format!("{emotion}.wav")))?;
        }
    } else {
        let emotion = args.emotion.unwrap();
        let audio = voice.render(emotion, args.options)?;
        if args.command == "play" {
            println!(
                "{emotion}: intensity {}, volume {}",
                args.options.intensity, args.options.volume
            );
            play(&audio, args.output)?;
        } else {
            save(&audio, &args.output.unwrap())?;
        }
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("mooduck_quacklib: {error}");
        std::process::exit(1);
    }
}
