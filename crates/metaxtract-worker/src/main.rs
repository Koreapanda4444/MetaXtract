#![forbid(unsafe_code)]
fn main() {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if arguments.len() != 2 || arguments[0] != "--protocol=1" {
        eprintln!("MetaXtract internal parser worker; launch through the scanner.");
        std::process::exit(2);
    }
    if let Err(error) =
        metaxtract_core::scan::run_bundled_worker_stdio(std::path::Path::new(&arguments[1]))
    {
        eprintln!("Parser worker failed: {error}");
        std::process::exit(1);
    }
}
