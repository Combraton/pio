fn main() -> std::process::ExitCode {
    match std::env::args().nth(1).as_deref() {
        Some("--version") => println!(
            "PIO {} (M1 skeleton; {})",
            env!("CARGO_PKG_VERSION"),
            pio_host::IMPLEMENTATION
        ),
        Some("participant") => println!("{}", pio_core::participant()),
        _ => {
            eprintln!("PIO M1 skeleton: protocol service and fake host are not implemented");
            return std::process::ExitCode::from(2);
        }
    }
    std::process::ExitCode::SUCCESS
}
