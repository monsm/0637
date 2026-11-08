use std::env;
use std::fs;
use std::path::Path;

/// Static archives to link, in dependency order (a dependent before the
/// libraries it needs). The MuJoCo build scripts write `libs.txt` next to the
/// archives they produce; `mujoco` alone is the fallback.
fn mujoco_libs(libdir: &str) -> Vec<String> {
    let manifest = Path::new(libdir).join("libs.txt");
    let mut libs: Vec<String> = fs::read_to_string(&manifest)
        .map(|text| {
            text.lines()
                .map(str::trim)
                .filter(|line| !line.is_empty() && !line.starts_with('#'))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    if libs.is_empty() {
        libs.push("mujoco".to_owned());
    }
    libs
}

/// A static archive only pulls members that define referenced symbols, and
/// MuJoCo's mesh decoders (stl/obj) register themselves from constructors —
/// nothing references their objects, so they would be dropped and scene
/// loading fails with "no decoder found". Pull the whole archive.
fn link_mujoco(libdir: &str, apple: bool) {
    // Whole-archive is spelled differently on each linker family.
    if apple {
        println!("cargo:rustc-link-arg=-Wl,-all_load");
    } else {
        println!("cargo:rustc-link-arg=-Wl,--whole-archive");
    }
    for lib in mujoco_libs(libdir) {
        println!("cargo:rustc-link-lib=static={lib}");
    }
    if apple {
        println!("cargo:rustc-link-arg=-Wl,-noall_load");
    } else {
        println!("cargo:rustc-link-arg=-Wl,--no-whole-archive");
    }
}

/// The NDK's per-target sysroot library directory, where `libc++_static.a`
/// lives. rustc links Android binaries with `-nodefaultlibs`, so the C++
/// runtime has to be named explicitly rather than left to the driver.
fn ndk_sysroot_lib_dir() -> Option<String> {
    let ndk = env::var("ANDROID_NDK_HOME")
        .or_else(|_| env::var("ANDROID_NDK_ROOT"))
        .ok()?;
    if ndk.is_empty() {
        return None;
    }
    let host = match env::consts::OS {
        "macos" => "darwin-x86_64",
        "windows" => "windows-x86_64",
        _ => "linux-x86_64",
    };
    let triple = env::var("TARGET").unwrap_or_default();
    Some(format!(
        "{ndk}/toolchains/llvm/prebuilt/{host}/sysroot/usr/lib/{triple}"
    ))
}

fn main() {
    println!("cargo:rerun-if-changed=native/duck_mj_shim.c");
    println!("cargo:rerun-if-changed=native/duck_mj_shim.h");
    println!("cargo:rerun-if-env-changed=MUJOCO_INCLUDE_DIR");
    println!("cargo:rerun-if-env-changed=MUJOCO_LIB_DIR");
    println!("cargo:rerun-if-env-changed=DUCK_LIBDL_STUB_DIR");

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
            link_mujoco(&libdir, false);
            // 16 KB page-size devices (Android 15+ kernels) refuse to load
            // 4 KB-aligned ELF segments.
            println!("cargo:rustc-link-arg=-Wl,-z,max-page-size=16384");
            // Link the API-level libc.so stub first: without it, -lc finds
            // libc.a (bionic's static libc, needed only for static
            // executables) in the per-triple directory, and its scudo malloc
            // divides by zero at startup. The device's libc is dynamic.
            if let Some(dir) = ndk_sysroot_lib_dir() {
                // 29 is the first API level whose libc.so exports the ELF
                // TLS entry points (__tls_get_addr) that libc++_static needs.
                let api = "29"; // matches minSdk and cargo ndk --platform
                println!(
                    "cargo:rustc-link-search=native={dir}/{api}"
                );
            }
            // The NDK has no libdl.so, so rustc's -ldl resolves to libdl.a —
            // bionic's static-executable shim, whose malloc/newlocale/errno/
            // atexit definitions have globals that are never initialised in a
            // shared library. The Android build script rebuilds that archive
            // with only the dl member; put it first on the search path.
            if let Ok(stub) = env::var("DUCK_LIBDL_STUB_DIR") {
                if !stub.is_empty() {
                    println!("cargo:rustc-link-search=native={stub}");
                }
            }
            // Static C++ runtime plus the C++ ABI, self-contained in the .so.
            if let Some(dir) = ndk_sysroot_lib_dir() {
                println!("cargo:rustc-link-search=native={dir}");
                if std::path::Path::new(&dir).join("libc++abi.a").exists() {
                    println!("cargo:rustc-link-lib=static=c++abi");
                }
            }
            println!("cargo:rustc-link-lib=static=c++_static");
            // Without --no-undefined a shared library links happily with
            // unresolved symbols and only fails at dlopen time on the device.
            println!("cargo:rustc-link-arg=-Wl,--no-undefined");
        }
        "ios" | "macos" => {
            link_mujoco(&libdir, true);
            // Apple's libc++ is a system library; the simulator and device
            // slices both have it.
            println!("cargo:rustc-link-lib=dylib=c++");
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
