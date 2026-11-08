use std::process::Command;

#[test]
fn lists_all_emotions_and_accepts_german_names() {
    let output = Command::new(env!("CARGO_BIN_EXE_mooduck_quacklib"))
        .arg("list")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().lines().count(),
        10
    );
    for emotion in mooduck_quacklib::Emotion::ALL {
        assert_eq!(
            emotion.name().parse::<mooduck_quacklib::Emotion>().unwrap(),
            emotion
        );
    }
    assert_eq!(
        "neugier".parse::<mooduck_quacklib::Emotion>().unwrap(),
        mooduck_quacklib::Emotion::Curious
    );
}

#[test]
fn invalid_cli_input_fails_with_useful_errors() {
    for args in [
        vec!["render", "happy"],
        vec!["play", "nonsense"],
        vec!["render", "happy", "-o", "/unused.wav", "--intensity", "NaN"],
        vec!["play", "happy", "--unknown", "1"],
        vec!["play", "happy", "--volume"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_mooduck_quacklib"))
            .args(args)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .starts_with("mooduck_quacklib:")
        );
    }
}
