use std::env;
use std::path::Path;

fn main() {
    let dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let root_dir = Path::new(&dir).parent().unwrap().parent().unwrap();
    env::set_current_dir(root_dir).unwrap();
    let mut build = cc::Build::new();

    build
        .file("c/account_lock_lib/secp256k1.c")
        .static_flag(true)
        .flag("-O3")
        .flag("-fno-builtin-printf")
        .flag("-fno-builtin-memcmp")
        .flag("-nostdinc")
        .flag("-nostdlib")
        .flag("-fvisibility=hidden")
        .flag("-Wl,-static")
        .flag("-fdata-sections")
        .flag("-ffunction-sections")
        .flag("-Wl,--gc-sections")
        // secp256k1
        .include("c/deps/ckb-production-scripts/c")
        .include("c/deps/ckb-production-scripts/build")
        .include("c/deps/ckb-production-scripts/deps/secp256k1/src")
        .include("c/deps/ckb-production-scripts/deps/secp256k1")
        // ckb-c-stdlib
        .include("c/deps/ckb-c-stdlib")
        .include("c/deps/ckb-c-stdlib/libc")
        .include("c/deps/molecule")
        .include("c/build")
        .flag("-Wall")
        .flag("-Werror")
        .flag("-Wno-unused-parameter")
        .flag("-Wno-nonnull")
        .flag("-Wno-nonnull-compare")
        .flag("-Wno-unused-function")
        .define("__SHARED_LIBRARY__", None)
        .compile("ckb-secp256k1");
}
