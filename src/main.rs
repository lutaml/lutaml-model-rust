fn main() -> std::process::ExitCode {
    if std::env::args()
        .skip(1)
        .any(|arg| arg == "--help" || arg == "-h")
    {
        println!("lutaml-model: portable LML Rust contracts (generation CLI integration pending)");
        std::process::ExitCode::SUCCESS
    } else {
        eprintln!("generation CLI is not available in the contract bootstrap; use --help");
        std::process::ExitCode::FAILURE
    }
}
