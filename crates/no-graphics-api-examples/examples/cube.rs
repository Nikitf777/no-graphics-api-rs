//! Port of `examples/cube/cube.cpp`.

fn main() {
    if let Err(error) = no_graphics_api_examples::cube::run() {
        eprintln!("cube: {error}");
        std::process::exit(1);
    }
}
