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
    let mut wire: serde_json::Value = serde_json::from_slice(&source()).unwrap();
    let attribute = wire["models"]["Book@default"]["attributes"][0]
        .as_object_mut()
        .unwrap();
    let values = attribute.remove("values").unwrap();
    attribute.insert("enum".into(), values);
    let snapshot = lml::load(&serde_json::to_vec(&wire).unwrap(), InputFormat::Json).unwrap();
    let artifacts = Artifacts::generate(&snapshot).unwrap();
    assert_eq!(artifacts.manifest.lml_checksum, snapshot.checksum());
    assert!(snapshot.canonical_json().contains("\"enum\""));
    assert!(serde_json::to_string(&snapshot.bundle().unwrap())
        .unwrap()
        .contains("\"values\""));
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

#[test]
fn manifest_reports_generated_capabilities_instead_of_source_formats() {
    let mut input: serde_json::Value = serde_json::from_slice(&source()).unwrap();
    input["capabilities"] = json!(["json", "xml"]);
    input["models"]["Book@default"]["mappings"]["xml"] =
        json!({"root":{"element":"book"},"namespace":{"mode":"inherit"},"rules":[]});
    let snapshot = lml::load(&serde_json::to_vec(&input).unwrap(), InputFormat::Json).unwrap();
    let artifacts = Artifacts::generate(&snapshot).unwrap();
    assert_eq!(artifacts.manifest.capabilities, vec!["json"]);
    assert_eq!(artifacts.manifest.lml_checksum, snapshot.checksum());
    assert!(!artifacts.files.contains_key("xml_codecs.rs"));
}

fn invoke_corpus(
    command: &str,
    input: &Path,
    output: &Path,
    manifest_pin: Option<&str>,
    index: Option<&Path>,
    corpus_pin: Option<&str>,
) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_lutaml-model"));
    cmd.arg(command)
        .arg("--input")
        .arg(input)
        .arg("--output")
        .arg(output);
    if let Some(pin) = manifest_pin {
        cmd.arg("--manifest-sha256").arg(pin);
    }
    if let Some(index) = index {
        cmd.arg("--corpus-index").arg(index);
    }
    if let Some(pin) = corpus_pin {
        cmd.arg("--corpus-sha256").arg(pin);
    }
    cmd.output().unwrap()
}
#[test]
fn actual_cli_pins_supplied_oracle_index_without_claiming_replay() {
    let temp = Temp::new();
    let schema = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/codecs/oracle/schema.json");
    let index = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/codecs/oracle/index.json");
    let out = temp.0.join("out");
    let bytes = fs::read(&index).unwrap();
    let corpus_pin = digest(&bytes);
    let generated = invoke_corpus(
        "generate",
        &schema,
        &out,
        None,
        Some(&index),
        Some(&corpus_pin),
    );
    assert!(
        generated.status.success(),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );
    let manifest_pin = String::from_utf8(generated.stdout).unwrap();
    let manifest_pin = manifest_pin.trim();
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(out.join("manifest.json")).unwrap()).unwrap();
    let oracle: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        manifest["corpora"],
        json!([{"generation":oracle["generation"],"sha256":corpus_pin}])
    );
    assert!(manifest.get("executed_case_ids").is_none());
    let repeated = invoke_corpus(
        "generate",
        &schema,
        &out,
        None,
        Some(&index),
        Some(&corpus_pin),
    );
    assert!(repeated.status.success());
    assert_eq!(
        String::from_utf8(repeated.stdout).unwrap().trim(),
        manifest_pin
    );
    let spaced_index = temp.0.join("spaced-index.json");
    let mut spaced = bytes.clone();
    spaced.push(b'\n');
    fs::write(&spaced_index, &spaced).unwrap();
    let rejected = invoke_corpus(
        "generate",
        &schema,
        &out,
        None,
        Some(&spaced_index),
        Some(&corpus_pin),
    );
    assert!(!rejected.status.success());
    let unchanged: serde_json::Value =
        serde_json::from_slice(&fs::read(out.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(unchanged, manifest);
    let spaced_out = temp.0.join("spaced");
    let spaced_pin = digest(&spaced);
    assert!(invoke_corpus(
        "generate",
        &schema,
        &spaced_out,
        None,
        Some(&spaced_index),
        Some(&spaced_pin)
    )
    .status
    .success());
    let spaced_manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(spaced_out.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(spaced_manifest["corpora"][0]["sha256"], spaced_pin);
    assert_ne!(spaced_manifest["corpora"], manifest["corpora"]);

    assert!(invoke_corpus(
        "check",
        &schema,
        &out,
        Some(manifest_pin),
        Some(&index),
        Some(&corpus_pin)
    )
    .status
    .success());
    assert!(!invoke("check", &schema, &out, Some(manifest_pin))
        .status
        .success());
    assert!(!invoke_corpus(
        "check",
        &schema,
        &out,
        Some(manifest_pin),
        Some(&index),
        Some("sha256:wrong")
    )
    .status
    .success());
    let plain = temp.0.join("plain");
    assert!(invoke("generate", &schema, &plain, None).status.success());
    let plain_manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(plain.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(plain_manifest["corpora"], json!([]));
    assert_ne!(manifest, plain_manifest);
}
#[test]
fn invalid_corpus_inputs_fail_before_artifact_writes() {
    let temp = Temp::new();
    let schema = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/codecs/oracle/schema.json");
    let original =
        fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/codecs/oracle/index.json"))
            .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&original).unwrap();
    let index = temp.0.join("index.json");
    fs::write(&index, &original).unwrap();
    let out = temp.0.join("out");
    let pin = digest(&original);
    for (index_arg, pin_arg) in [
        (Some(index.as_path()), None),
        (None, Some(pin.as_str())),
        (Some(index.as_path()), Some("sha256:wrong")),
    ] {
        assert!(
            !invoke_corpus("generate", &schema, &out, None, index_arg, pin_arg)
                .status
                .success()
        );
        assert!(!out.exists());
    }
    fs::write(&index, b"corrupted index bytes").unwrap();
    assert!(
        !invoke_corpus("generate", &schema, &out, None, Some(&index), Some(&pin))
            .status
            .success()
    );
    assert!(!out.exists());
    assert!(!invoke_corpus(
        "generate",
        &schema,
        &out,
        None,
        Some(&index),
        Some(&digest(b"corrupted index bytes"))
    )
    .status
    .success());
    assert!(!out.exists());
    let mut variants = Vec::new();
    let mut edited = value.clone();
    edited["corpus"] = json!(2);
    variants.push(edited);
    let mut edited = value.clone();
    edited["generation"] = json!(" ");
    variants.push(edited);
    let mut edited = value.clone();
    edited["cases"] = json!([]);
    variants.push(edited);
    let mut edited = value.clone();
    edited["cases"][0]["id"] = json!("");
    variants.push(edited);
    let mut edited = value.clone();
    edited["cases"][1]["id"] = edited["cases"][0]["id"].clone();
    variants.push(edited);
    let mut edited = value.clone();
    edited["cases"][0]["lml_checksum"] = json!("sha256:other-source");
    variants.push(edited);
    let mut edited = value.clone();
    edited.as_object_mut().unwrap().remove("cases");
    variants.push(edited);
    let mut edited = value.clone();
    edited["cases"][0]
        .as_object_mut()
        .unwrap()
        .remove("lml_checksum");
    variants.push(edited);
    let mut edited = value.clone();
    edited["cases"][0].as_object_mut().unwrap().remove("id");
    variants.push(edited);
    for edited in variants {
        let bytes = serde_json::to_vec(&edited).unwrap();
        fs::write(&index, &bytes).unwrap();
        let result = invoke_corpus(
            "generate",
            &schema,
            &out,
            None,
            Some(&index),
            Some(&digest(&bytes)),
        );
        assert!(!result.status.success(), "invalid index accepted: {edited}");
        assert!(!out.exists());
    }
    let missing = temp.0.join("missing.json");
    assert!(
        !invoke_corpus("generate", &schema, &out, None, Some(&missing), Some(&pin))
            .status
            .success()
    );
    assert!(!out.exists());
    fs::write(&index, &original).unwrap();
    let other_schema = temp.0.join("other.json");
    fs::write(&other_schema, source()).unwrap();
    assert!(!invoke_corpus(
        "generate",
        &other_schema,
        &out,
        None,
        Some(&index),
        Some(&pin)
    )
    .status
    .success());
    assert!(!out.exists());
}
