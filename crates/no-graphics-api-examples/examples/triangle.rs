//! Port of `examples/triangle/triangle.cpp`.

fn main() {
    if let Err(error) = no_graphics_api_examples::triangle::run() {
        eprintln!("triangle: {error}");
        std::process::exit(1);
    }
}
