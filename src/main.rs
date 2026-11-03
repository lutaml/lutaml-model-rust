use lutaml_model::{artifacts::Artifacts, lml, InputFormat};
use std::{error::Error, path::PathBuf};
const HELP: &str = "lutaml-model <generate|check> --input BUNDLE --output DIR [--format json|yaml] [--manifest-sha256 sha256:HEX] [--corpus-index FILE --corpus-sha256 sha256:HEX]\ncheck requires a caller-pinned --manifest-sha256. generate prints this digest.\nCorpus options must be paired and pin index identity, without claiming replay.\nGenerated mod.rs uses lutaml-model runtime plus pinned serde and serde_json dependencies.";
fn run() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let Some(command) = args.next() else {
        return Err(HELP.into());
    };
    if command == "--help" || command == "-h" {
        println!("{HELP}");
        return Ok(());
    }
    if command != "generate" && command != "check" {
        return Err("expected generate or check".into());
    }
    let (mut input, mut output, mut format, mut expected, mut corpus_index, mut corpus_pin) =
        (None, None, None, None, None, None);
    while let Some(arg) = args.next() {
        let slot = match arg.as_str() {
            "--input" => &mut input,
            "--output" => &mut output,
            "--format" => &mut format,
            "--manifest-sha256" => &mut expected,
            "--corpus-index" => &mut corpus_index,
            "--corpus-sha256" => &mut corpus_pin,
            _ => return Err(format!("unknown option: {arg}").into()),
        };
        if slot.is_some() {
            return Err(format!("duplicate option: {arg}").into());
        }
        *slot = Some(args.next().ok_or("option requires a value")?);
    }
    let corpus = match (corpus_index, corpus_pin) {
        (Some(index), Some(pin)) => Some((std::fs::read(index)?, pin)),
        (None, None) => None,
        _ => return Err("--corpus-index and --corpus-sha256 must be supplied together".into()),
    };
    let input = PathBuf::from(input.ok_or("--input is required")?);
    let output = PathBuf::from(output.ok_or("--output is required")?);
    let format = match format
        .as_deref()
        .or_else(|| input.extension().and_then(|s| s.to_str()))
    {
        Some("json") => InputFormat::Json,
        Some("yaml" | "yml") => InputFormat::Yaml,
        _ => return Err("input format must be json or yaml".into()),
    };
    let snapshot = lml::load(&std::fs::read(input)?, format)?;
    let artifacts = Artifacts::generate(&snapshot)?;
    let artifacts = if let Some((bytes, pin)) = corpus {
        artifacts.with_corpus(&bytes, &pin)?
    } else {
        artifacts
    };
    if command == "check" {
        artifacts.verify(
            &output,
            &expected.ok_or("check requires --manifest-sha256")?,
        )?;
    } else {
        if let Some(expected) = expected {
            if artifacts.manifest_digest()? != expected {
                return Err("expected manifest checksum mismatch".into());
            }
        }
        artifacts.write(&output)?;
    }
    println!("{}", artifacts.manifest_digest()?);
    Ok(())
}
fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
