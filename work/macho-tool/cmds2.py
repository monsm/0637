import struct, sys
NAMES = {0x19:"LC_SEGMENT_64",0x0d:"LC_ID_DYLIB",0x0c:"LC_LOAD_DYLIB",0x8000001c:"LC_RPATH",2:"LC_SYMTAB",0xb:"LC_DYSYMTAB",
0x1d:"LC_CODE_SIGNATURE",0x32:"LC_BUILD_VERSION",0x24:"LC_VERSION_MIN_MACOSX",0x25:"LC_VERSION_MIN_IPHONEOS",
0x2f:"LC_VERSION_MIN_TVOS",0x30:"LC_VERSION_MIN_WATCHOS",0x1b:"LC_UUID",0x2a:"LC_SOURCE_VERSION",0x26:"LC_FUNCTION_STARTS",
0x29:"LC_DATA_IN_CODE",0x2b:"LC_DYLIB_CODE_SIGN_DRS",0x2e:"LC_LINKER_OPTIMIZATION_HINT",0x2c:"LC_ATOM_INFO?",
0x80000022:"LC_DYLD_INFO_ONLY",0x80000033:"LC_DYLD_EXPORTS_TRIE",0x80000034:"LC_DYLD_CHAINED_FIXUPS",0x80000028:"LC_MAIN"}
def dump(path):
    d=open(path,"rb").read()
    print(f"== {path} ({len(d)}) ==")
    bases=[0]
    if struct.unpack_from(">I",d,0)[0]==0xcafebabe:
        n=struct.unpack_from(">I",d,4)[0]; bases=[]
        for i in range(n):
            cpu,sub,o,sz,al=struct.unpack_from(">IIIII",d,8+i*20); bases.append(o)
            print(f" fat arch cpu={hex(cpu)} off={o} size={sz}")
    for base in bases:
        m=struct.unpack_from("<I",d,base)[0]
        if m!=0xfeedfacf:
            print(f" slice {base}: not macho64 ({d[base:base+8]})"); continue
        ct,cs,ft,nc,sc,fl,r=struct.unpack_from("<iiIIIII",d,base+4)
        print(f" slice@{base}: cputype={hex(ct)} filetype={ft} ncmds={nc} flags={hex(fl)}")
        p=base+32
        for _ in range(nc):
            cmd,sz=struct.unpack_from("<II",d,p); nm=NAMES.get(cmd,hex(cmd)); extra=""
            if cmd==0x32:
                pl,mi,sd,nt=struct.unpack_from("<IIII",d,p+8); f=lambda v:f"{v>>16}.{(v>>8)&0xff}.{v&0xff}"
                extra=f" platform={pl} minos={f(mi)} sdk={f(sd)}"
            elif cmd in (0x24,0x25,0x2f,0x30):
                v,s=struct.unpack_from("<II",d,p+8); f=lambda v:f"{v>>16}.{(v>>8)&0xff}.{v&0xff}"
                extra=f" version={f(v)} sdk={f(s)}"
            elif cmd in (0x0d,0x0c,0x8000001c):
                no=struct.unpack_from("<I",d,p+8)[0]; extra=" "+d[p+no:p+sz].split(b"\0")[0].decode(errors="replace")
            print(f"   {nm}{extra}")
            p+=sz
for a in sys.argv[1:]: dump(a)
