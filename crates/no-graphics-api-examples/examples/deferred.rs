//! Port of `examples/deferred_renderer/deferred_renderer.cpp`.

fn main() {
    if let Err(error) = no_graphics_api_examples::deferred::run() {
        eprintln!("deferred: {error}");
        std::process::exit(1);
    }
}
