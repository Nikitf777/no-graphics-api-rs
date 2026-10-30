use std::path::{Path, PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let profile = std::env::var("PROFILE").unwrap();
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap();

    let source_dir = match std::env::var_os("NO_GRAPHICS_API_SOURCE") {
        Some(path) => PathBuf::from(path),
        None => manifest_dir.join("vendor/NoGraphicsAPI"),
    };
    let include_dir = source_dir.join("include");
    assert!(
        include_dir
            .join("NoGraphicsAPI/NoGraphicsAPI.hpp")
            .is_file(),
        "NoGraphicsAPI headers not found under {}. Set NO_GRAPHICS_API_SOURCE to a checkout of \
         https://github.com/sebbbi/NoGraphicsAPI",
        source_dir.display()
    );

    println!("cargo:rerun-if-env-changed=NO_GRAPHICS_API_SOURCE");
    println!("cargo:rerun-if-env-changed=VULKAN_SDK");
    println!("cargo:rerun-if-changed=csi/shim.h");
    println!("cargo:rerun-if-changed=csi/shim.cpp");
    println!("cargo:rerun-if-changed={}", include_dir.display());
    println!(
        "cargo:rerun-if-changed={}",
        source_dir.join("src").display()
    );

    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++20")
        .include(&include_dir)
        .include(manifest_dir.join("csi"))
        .warnings(false)
        .flag_if_supported("-fno-exceptions")
        .flag_if_supported("-fno-rtti")
        .file(manifest_dir.join("csi/shim.cpp"));

    if profile == "release" {
        build.define("NDEBUG", None);
    }

    let vulkan_sdk = std::env::var_os("VULKAN_SDK").map(PathBuf::from);
    if let Some(sdk) = &vulkan_sdk {
        for dir in [sdk.join("Include"), sdk.join("include")] {
            if dir.is_dir() {
                build.include(&dir);
            }
        }
    }

    match target_os.as_str() {
        "macos" | "ios" => {
            build
                .file(source_dir.join("src/NoGraphicsAPIMetal.mm"))
                .flag_if_supported("-fno-objc-arc");
        }
        _ => {
            build.file(source_dir.join("src/NoGraphicsAPI.cpp"));
        }
    }

    build.compile("NoGraphicsAPI");

    match target_os.as_str() {
        "macos" | "ios" => {
            for framework in ["Metal", "Foundation", "QuartzCore"] {
                println!("cargo:rustc-link-lib=framework={framework}");
            }
        }
        "windows" => {
            println!("cargo:rustc-link-lib=dylib=vulkan-1");
            if let Some(sdk) = &vulkan_sdk {
                println!(
                    "cargo:rustc-link-search=native={}",
                    sdk.join("Lib").display()
                );
            }
        }
        _ => {
            if std::env::var_os("VULKAN_LIB").is_some() {
                println!(
                    "cargo:rustc-link-search=native={}",
                    std::env::var("VULKAN_LIB").unwrap()
                );
            }
            println!("cargo:rustc-link-lib=dylib=vulkan");
        }
    }

    let mut bindgen = bindgen::Builder::default()
        .header(manifest_dir.join("csi/shim.h").to_str().unwrap())
        .clang_arg("-x")
        .clang_arg("c++")
        .clang_arg("-std=c++20")
        .clang_arg(format!("-I{}", include_dir.display()))
        .allowlist_function("nga_.*")
        .allowlist_type("gpu::.*")
        .allowlist_var("gpu::.*")
        .blocklist_type("gpu::detail::.*")
        .enable_cxx_namespaces()
        .newtype_enum("gpu::.*")
        .bitfield_enum("gpu::(TextureUsage|Stage|Access|RenderingFlags)")
        .layout_tests(true)
        .derive_default(false)
        .derive_partialeq(true)
        .generate_comments(true);

    if let Some(sdk) = &vulkan_sdk {
        for dir in [sdk.join("Include"), sdk.join("include")] {
            if dir.is_dir() {
                bindgen = bindgen.clang_arg(format!("-I{}", dir.display()));
            }
        }
    }

    let bindings = bindgen
        .generate()
        .expect("failed to generate NoGraphicsAPI bindings (libclang is required)");
    bindings
        .write_to_file(out_dir.join("bindings.rs"))
        .expect("failed to write bindings.rs");

    assert!(Path::new(&out_dir).join("bindings.rs").is_file());
}
