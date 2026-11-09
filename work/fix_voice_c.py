import io

p = r'D:\AI\Duck\core\duck-mobile-core\src\ffi.rs'
s = io.open(p, encoding='utf-8').read()
start = s.index('/// The next sound the duck wants played')
end = s.index('/// Render one sound to mono f32 PCM')
new = '''/// The next sound the duck wants played, as a tag the platform looks up.
/// `NULL` when nothing is queued. The tag is a static string; the shell owns
/// the speaker and the PCM.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_next_sound(core: *mut DuckCore) -> *const c_char {
    if core.is_null() {
        return std::ptr::null();
    }
    let core = &mut *core;
    let Some(tag) = core.runtime.take_sound() else {
        return std::ptr::null();
    };
    // `c"..."` is a NUL-terminated static, so the pointer stays valid for as
    // long as the caller holds it and nothing is allocated per sound.
    let tag: &'static std::ffi::CStr = match tag.as_str() {
        "greet" => c"greet",
        "chirp" => c"chirp",
        "coo" => c"coo",
        "peck" => c"peck",
        "inquire" => c"inquire",
        "alarm" => c"alarm",
        _ => return std::ptr::null(),
    };
    tag.as_ptr()
}

'''
s = s[:start] + new + s[end:]
io.open(p, 'w', encoding='utf-8', newline='').write(s)
print('rewritten')
