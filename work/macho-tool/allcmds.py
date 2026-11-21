import struct, sys

NAMES = {0x19:"LC_SEGMENT_64", 0x0d:"LC_ID_DYLIB", 0x0c:"LC_LOAD_DYLIB", 0x8000001c:"LC_RPATH",
2:"LC_SYMTAB", 0xb:"LC_DYSYMTAB", 0xe:"LC_LOAD_DYLINKER", 0x80000022:"LC_DYLD_INFO_ONLY",
0x80000034:"LC_DYLD_EXPORTS_TRIE", 0x80000033:"LC_DYLD_CHAINED_FIXUPS", 0x1d:"LC_CODE_SIGNATURE",
0x32:"LC_BUILD_VERSION", 0x24:"LC_VERSION_MIN_IPHONEOS", 0x25:"LC_VERSION_MIN_MACOSX",
0x26:"LC_VERSION_MIN_TVOS", 0x27:"LC_VERSION_MIN_WATCHOS", 0xc:"LC_LOAD_DYLIB", 0x8000001f:"LC_REEXPORT_DYLIB",
0x1b:"LC_UUID", 0x80000028:"LC_MAIN", 0x2a:"LC_SOURCE_VERSION", 0x1e:"LC_SEGMENT_SPLIT_INFO",
0x80000023:"LC_LINKER_OPTIMIZATION_HINT", 0x80000035:"LC_LINKER_OPTIMIZATION_HINT?"}

def dump(path):
    data = open(path,"rb").read()
    print(f"== {path} ({len(data)}) ==")
    magic = struct.unpack_from(">I", data, 0)[0]
    off = 0
    if magic == 0xcafebabe:
        n = struct.unpack_from(">I", data, 4)[0]
        for i in range(n):
            cpu, sub, o, sz, al = struct.unpack_from(">IIIII", data, 8+i*20)
            print(f" fat arch cpu={hex(cpu)} off={o} size={sz}")
            off = o
        slices = None
    else:
        slices = [0]
    offs = slices if slices else [off]
    for base in offs:
        print(f" -- slice at {base} --")
        m = struct.unpack_from("<I", data, base)[0]
        print(" magic", hex(m))
        if m != 0xfeedfacf:
            print(" (not 64-bit mach-o):", data[base:base+16])
            continue
        cputype, cpusub, ft, ncmds, sizeofcmds, flags, res = struct.unpack_from("<iiIIIII", data, base+4)
        print(f" cputype={hex(cputype)} filetype={ft} ncmds={ncmds} flags={hex(flags)}")
        p = base+32
        for _ in range(ncmds):
            cmd, cmdsize = struct.unpack_from("<II", data, p)
            nm = NAMES.get(cmd, hex(cmd))
            extra = ""
            if cmd == 0x32:
                pl, minos, sdk, ntools = struct.unpack_from("<IIII", data, p+8)
                f = lambda v: f"{v>>16}.{(v>>8)&0xff}.{v&0xff}"
                extra = f" platform={pl} minos={f(minos)} sdk={f(sdk)}"
            if cmd == 0x24:
                v, s = struct.unpack_from("<II", data, p+8)
                f = lambda v: f"{v>>16}.{(v>>8)&0xff}.{v&0xff}"
                extra = f" version={f(v)} sdk={f(s)}"
            if cmd in (0x0d,0x0c,0x8000001c,0x8000001f):
                name_off = struct.unpack_from("<I", data, p+8)[0]
                extra = " " + data[p+name_off:p+cmdsize].split(b"\0")[0].decode(errors="replace")
            print(f"   {nm}{extra}")
            p += cmdsize

for a in sys.argv[1:]:
    dump(a)
