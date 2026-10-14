fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut stdout = std::io::stdout();
    let code = harno::main(args, harno::Env::from_process(), &mut stdout);
    std::process::exit(code);
}
