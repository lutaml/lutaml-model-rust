#![cfg(feature = "generator")]
use lutaml_model::{
    artifacts::{digest, Artifacts},
    lml, InputFormat,
};
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "lutaml-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn source() -> Vec<u8> {
    serde_json::to_vec(&json!({"lml_bundle":1,"roots":["Book@default"],"models":{"Book@default":{"lml":2,"model":"Book","register":"default","attributes":[{"name":"title","type":{"kind":"builtin","name":"string"},"required":true,"collection":false,"default":{"present":false},"initialize_empty":false,"values":["hello","goodbye"]}],"mappings":{"json":{"rules":[{"key":["label"],"to":"title"}]}}}},"value_types":{},"capabilities":["json"]})).unwrap()
}
fn invoke(command: &str, input: &Path, output: &Path, pin: Option<&str>) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_lutaml-model"));
    cmd.arg(command)
        .arg("--input")
        .arg(input)
        .arg("--output")
        .arg(output);
    if let Some(pin) = pin {
        cmd.arg("--manifest-sha256").arg(pin);
    }
    cmd.output().unwrap()
}
#[test]
fn actual_cli_generates_deterministic_pinned_artifacts_and_checks_corruption() {
    let temp = Temp::new();
    let input = temp.0.join("input.json");
    let out = temp.0.join("out");
    fs::write(&input, source()).unwrap();
    let first = invoke("generate", &input, &out, None);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let pin = String::from_utf8(first.stdout).unwrap();
    let pin = pin.trim();
    let manifest = fs::read(out.join("manifest.json")).unwrap();
    let yaml = temp.0.join("input.yaml");
    fs::write(
        &yaml,
        serde_yaml::to_string(&serde_yaml::from_slice::<serde_yaml::Value>(&source()).unwrap())
            .unwrap(),
    )
    .unwrap();
    let yaml_check = invoke("check", &yaml, &out, Some(pin));
    assert!(
        yaml_check.status.success(),
        "{}",
        String::from_utf8_lossy(&yaml_check.stderr)
    );
    assert_eq!(
        invoke("generate", &input, &out, None).stdout,
        format!("{pin}\n").as_bytes()
    );
    assert_eq!(fs::read(out.join("manifest.json")).unwrap(), manifest);
    assert!(invoke("check", &input, &out, Some(pin)).status.success());
    assert!(!invoke("check", &input, &out, None).status.success());
    assert!(!invoke("check", &input, &out, Some("sha256:wrong"))
        .status
        .success());
    fs::write(out.join("models.rs"), "corrupt").unwrap();
    assert!(!invoke("check", &input, &out, Some(pin)).status.success());
    invoke("generate", &input, &out, None);
    let mut edited: serde_json::Value = serde_json::from_slice(&manifest).unwrap();
    edited["unexpected"] = json!("must be included in manifest identity");
    fs::write(
        out.join("manifest.json"),
        serde_json::to_vec(&edited).unwrap(),
    )
    .unwrap();
    assert!(!invoke("check", &input, &out, Some(pin)).status.success());
    edited.as_object_mut().unwrap().remove("unexpected");
    edited["dependencies"][0]["revision"] = json!("changed");
    fs::write(
        out.join("manifest.json"),
        serde_json::to_vec(&edited).unwrap(),
    )
    .unwrap();
    assert!(!invoke("check", &input, &out, Some(pin)).status.success());
    edited["files"][0]["path"] = json!("../outside.rs");
    let bytes = serde_json::to_vec(&edited).unwrap();
    let forged_pin = digest(&bytes);
    fs::write(out.join("manifest.json"), bytes).unwrap();
    assert!(!invoke("check", &input, &out, Some(&forged_pin))
        .status
        .success());
    assert!(!temp.0.join("outside.rs").exists());
}
#[test]
fn snapshot_wire_checksum_is_authoritative_and_generated_consumer_executes() {
    let temp = Temp::new();
    let snapshot = lml::load(&source(), InputFormat::Json).unwrap();
    let artifacts = Artifacts::generate(&snapshot).unwrap();
    assert_eq!(artifacts.manifest.lml_checksum, snapshot.checksum());
    assert!(snapshot.canonical_json().contains("\"values\""));
    assert!(serde_json::to_string(&snapshot.bundle().unwrap())
        .unwrap()
        .contains("\"enum\""));
    let mut detached = snapshot.bundle().unwrap();
    detached.capabilities.clear();
    assert_eq!(artifacts.manifest.lml_checksum, snapshot.checksum());
    let src = temp.0.join("src");
    fs::create_dir(&src).unwrap();
    artifacts.write(&src.join("generated")).unwrap();
    let path = serde_json::to_string(env!("CARGO_MANIFEST_DIR")).unwrap();
    fs::write(temp.0.join("Cargo.toml"),format!("[package]\nname=\"cli-generated-consumer\"\nversion=\"0.0.0\"\nedition=\"2021\"\n[dependencies]\nlutaml-model={{path={path},default-features=false,features=[\"runtime\"]}}\nserde={{version=\"=1.0.219\",features=[\"derive\"]}}\nserde_json={{version=\"=1.0.140\",features=[\"arbitrary_precision\"]}}\n")).unwrap();
    fs::write(
        src.join("main.rs"),
        r##"mod generated;
fn main() {
 let book=generated::json_codecs::read_Book_json(r#"{"label":"hello"}"#).unwrap();
 assert_eq!(generated::json_codecs::write_Book_json(&book).unwrap(),r#"{"label":"hello"}"#);
 assert!(generated::json_codecs::read_Book_json("{}").is_err());
}
"##,
    )
    .unwrap();
    let result = Command::new(env!("CARGO"))
        .args(["run", "--offline", "--quiet"])
        .current_dir(&temp.0)
        .env(
            "CARGO_TARGET_DIR",
            Path::new(env!("CARGO_MANIFEST_DIR")).join("target/cli-consumer"),
        )
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
#[cfg(unix)]
#[test]
fn symlink_escape_is_rejected_for_generation_and_verification() {
    use std::os::unix::fs::symlink;
    let temp = Temp::new();
    let root = temp.0.join("out");
    fs::create_dir(&root).unwrap();
    let outside = temp.0.join("outside");
    fs::write(&outside, "preserved").unwrap();
    let artifacts = Artifacts::generate(&lml::load(&source(), InputFormat::Json).unwrap()).unwrap();
    artifacts.write(&root).unwrap();
    fs::remove_file(root.join("models.rs")).unwrap();
    symlink(&outside, root.join("models.rs")).unwrap();
    assert!(artifacts
        .verify(&root, &artifacts.manifest_digest().unwrap())
        .is_err());
    assert!(artifacts.write(&root).is_err());
    assert_eq!(fs::read_to_string(&outside).unwrap(), "preserved");
    let link = temp.0.join("linked-root");
    symlink(&root, &link).unwrap();
    assert!(artifacts.write(&link).is_err());
}
#[test]
fn invalid_cli_input_fails_without_artifacts() {
    let temp = Temp::new();
    let input = temp.0.join("bad.json");
    let out = temp.0.join("out");
    fs::write(&input, "{}").unwrap();
    assert!(!invoke("generate", &input, &out, None).status.success());
    assert!(!out.exists());
}
