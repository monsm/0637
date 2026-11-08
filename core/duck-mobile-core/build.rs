use std::env;

fn main() {
    println!("cargo:rerun-if-changed=native/duck_mj_shim.c");
    println!("cargo:rerun-if-changed=native/duck_mj_shim.h");
    println!("cargo:rerun-if-env-changed=MUJOCO_INCLUDE_DIR");
    println!("cargo:rerun-if-env-changed=MUJOCO_LIB_DIR");

    if env::var_os("CARGO_FEATURE_MUJOCO").is_none() {
        return;
    }

    let include = env::var("MUJOCO_INCLUDE_DIR").unwrap_or_else(|_| {
        r"D:\AI\Duck\work\.venv\Lib\site-packages\mujoco\include\mujoco".to_owned()
    });
    let libdir = env::var("MUJOCO_LIB_DIR")
        .unwrap_or_else(|_| r"D:\AI\Duck\work\mujoco-lib".to_owned());
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

    cc::Build::new()
        .file("native/duck_mj_shim.c")
        .include("native")
        .include(&include)
        .warnings(false)
        .compile("duck_mj_shim");

    println!("cargo:rustc-link-search=native={libdir}");
    match target_os.as_str() {
        "android" => {
            // NDK static MuJoCo; c++_static keeps the APK free of an extra
            // libc++_shared.so dependency.
            println!("cargo:rustc-link-lib=static=mujoco");
            println!("cargo:rustc-link-lib=static=c++_static");
            // rustc doesn't know the NDK sysroot's per-triple lib dir where
            // libc++_static.a lives.
            let ndk = env::var("ANDROID_NDK_HOME")
                .or_else(|_| env::var("ANDROID_NDK_ROOT"))
                .or_else(|_| {
                    env::var("ANDROID_HOME")
                        .map(|home| format!("{home}/ndk/27.2.12479018"))
                })
                .unwrap_or_default();
            if !ndk.is_empty() {
                let host = match env::consts::OS.as_str() {
                    "macos" => "darwin-x86_64",
                    "windows" => "windows-x86_64",
                    _ => "linux-x86_64",
                };
                let triple = env::var("TARGET").unwrap_or_default();
                println!(
                    "cargo:rustc-link-search=native={ndk}/toolchains/llvm/prebuilt/{host}/sysroot/usr/lib/{triple}"
                );
            }
        }
        "ios" | "macos" => {
            // CI supplies a MuJoCo 3.10 static library built for the target.
            println!("cargo:rustc-link-lib=static=mujoco");
            println!("cargo:rustc-link-lib=c++");
            // MuJoCo and ONNX Runtime's Apple slices use these.
            println!("cargo:rustc-link-lib=framework=Foundation");
            println!("cargo:rustc-link-lib=framework=Accelerate");
            println!("cargo:rustc-link-lib=framework=CoreML");
            println!("cargo:rustc-link-lib=framework=CoreFoundation");
            println!("cargo:rustc-link-lib=framework=CoreGraphics");
        }
        _ => {
            println!("cargo:rustc-link-lib=mujoco");
        }
    }
}
