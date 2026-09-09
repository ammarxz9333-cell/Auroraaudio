fn main() {
    println!("cargo:rerun-if-env-changed=AURORA_LIBMPEGH_LIB_DIR");

    if std::env::var_os("CARGO_FEATURE_NATIVE_MPEGH").is_none() {
        return;
    }

    let lib_dir = std::env::var("AURORA_LIBMPEGH_LIB_DIR").expect(
        "native-mpegh requires AURORA_LIBMPEGH_LIB_DIR pointing to a built libmpegh directory",
    );
    println!("cargo:rustc-link-search=native={lib_dir}");
    println!("cargo:rustc-link-lib=static=ia_mpeghd_lib");
}
