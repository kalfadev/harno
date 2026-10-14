fn main() {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let code = analytics::main(std::env::args().skip(1).collect(), cwd, &mut std::io::stdout());
    std::process::exit(code);
}
