fn main() {
    if let Err(error) = jig::run() {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}
