import struct, sys, plistlib

CPU = {0x0100000c: "arm64", 0x01000007: "x86_64"}
FILETYPE = {2: "MH_EXECUTE", 6: "MH_DYLIB", 8: "MH_BUNDLE"}
PLATFORM = {1: "macOS", 2: "iOS", 6: "iOS-simulator", 7: "tvOS", 8: "tvOS-sim", 11: "visionOS", 12: "visionOS-sim"}

def parse_fat(data, base=0):
    magic = struct.unpack_from(">I", data, base)[0]
    if magic == 0xcafebabe:
        n = struct.unpack_from(">I", data, base+4)[0]
        out = []
        for i in range(n):
            off = base + 8 + i*20
            cpu, sub, offset, size, align = struct.unpack_from(">IIIII", data, off)
            out.append((CPU.get(cpu, hex(cpu)), offset, size))
        return out
    return None

def slices(data):
    fat = parse_fat(data)
    if fat is not None:
        return [(name, off, sz) for (name, off, sz) in fat]
    return [("thin", 0, len(data))]

def dump(path, want_plist=False):
    data = open(path, "rb").read()
    print(f"== {path} ({len(data)} bytes) ==")
    if want_plist:
        try:
            print(plistlib.loads(data))
        except Exception as e:
            print("plist error", e)
        return
    for name, off, size in slices(data):
        magic = struct.unpack_from("<I", data, off)[0]
        if magic != 0xfeedfacf:
            print(f" slice {name} off={off} magic={hex(magic)} (not 64-bit LE mach-o)")
            continue
        cputype, cpusub, filetype, ncmds, sizeofcmds, flags, _ = struct.unpack_from("<iiIIIII", data, off+4)
        print(f" slice {name} off={off} size={size} cputype={CPU.get(cputype,hex(cputype))} filetype={FILETYPE.get(filetype,filetype)} ncmds={ncmds}")
        p = off + 32
        for _ in range(ncmds):
            cmd, cmdsize = struct.unpack_from("<II", data, p)
            if cmd == 0x19:  # LC_SEGMENT_64
                segname = data[p+8:p+24].split(b"\0")[0].decode()
                vmaddr, vmsize, fileoff, filesize = struct.unpack_from("<QQQQ", data, p+24)
                print(f"   LC_SEGMENT_64 {segname} filesize={filesize}")
            elif cmd == 0x0d:  # LC_ID_DYLIB
                name_off = struct.unpack_from("<I", data, p+8)[0]
                nm = data[p+name_off:p+cmdsize].split(b"\0")[0].decode()
                print(f"   LC_ID_DYLIB {nm}")
            elif cmd == 0x0c:  # LC_LOAD_DYLIB
                name_off = struct.unpack_from("<I", data, p+8)[0]
                nm = data[p+name_off:p+cmdsize].split(b"\0")[0].decode()
                print(f"   LC_LOAD_DYLIB {nm}")
            elif cmd == 0x8000001c:  # LC_RPATH
                name_off = struct.unpack_from("<I", data, p+8)[0]
                nm = data[p+name_off:p+cmdsize].split(b"\0")[0].decode()
                print(f"   LC_RPATH {nm}")
            elif cmd == 0x32:  # LC_BUILD_VERSION
                platform, minos, sdk, ntools = struct.unpack_from("<IIII", data, p+8)
                f = lambda v: f"{v>>16}.{(v>>8)&0xff}.{v&0xff}"
                print(f"   LC_BUILD_VERSION platform={PLATFORM.get(platform,platform)} minos={f(minos)} sdk={f(sdk)}")
            elif cmd == 0x24:  # LC_VERSION_MIN_IPHONEOS
                version, sdk = struct.unpack_from("<II", data, p+8)
                f = lambda v: f"{v>>16}.{(v>>8)&0xff}.{v&0xff}"
                print(f"   LC_VERSION_MIN_IPHONEOS minos={f(version)} sdk={f(sdk)}")
            elif cmd == 0x1d:  # LC_CODE_SIGNATURE
                off2, size2 = struct.unpack_from("<II", data, p+8)
                print(f"   LC_CODE_SIGNATURE dataoff={off2} datasize={size2}")
            elif cmd == 0x22:  # LC_DYLD_INFO_ONLY
                pass
            p += cmdsize

for path in sys.argv[2:]:
    dump(path, want_plist=(sys.argv[1] == "plist"))

