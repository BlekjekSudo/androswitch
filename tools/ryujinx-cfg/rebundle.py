"""Task-local .NET single-file bundle replacement; input files remain unchanged.
Format: dotnet/runtime src/installer/managed/Microsoft.NET.HostModel/Bundle/{Manifest,FileEntry}.cs.
"""
import argparse, hashlib, io, json, pathlib, struct, zlib
SIGNATURE=bytes.fromhex('8b1202b96a612038727b930214d7a03213f5b9e6efae3318ee3b2dce24b36aae')
def readstr(f):
    length=0
    for i in range(5):
        byte=f.read(1)
        if not byte: raise ValueError('Truncated string length')
        v=byte[0]; length|=(v&127)<<(7*i)
        if not v&128: break
    else: raise ValueError('Oversized string length')
    if length>4096: raise ValueError('Oversized manifest string')
    text=f.read(length)
    if len(text)!=length: raise ValueError('Truncated manifest string')
    return text.decode('utf-8')
def writestr(f,s):
    b=s.encode('utf-8'); n=len(b)
    while n>=128: f.write(bytes([(n&127)|128])); n>>=7
    f.write(bytes([n])); f.write(b)
def main():
    ap=argparse.ArgumentParser(); ap.add_argument('original',type=pathlib.Path); ap.add_argument('replacement',type=pathlib.Path); ap.add_argument('output',type=pathlib.Path); ap.add_argument('--expected-sha',required=True); args=ap.parse_args()
    original=args.original.resolve(); output=args.output.resolve()
    if output==original or output.exists(): raise ValueError('Output must be new and distinct')
    data=original.read_bytes(); sha=hashlib.sha256(data).hexdigest()
    if sha!=args.expected_sha: raise ValueError('Original input digest mismatch')
    if data.count(SIGNATURE)!=1: raise ValueError('Bundle signature must be unique')
    pointer=data.index(SIGNATURE)-8; header,=struct.unpack_from('<Q',data,pointer)
    f=io.BytesIO(data); f.seek(header)
    major,minor,count=struct.unpack('<III',f.read(12))
    if (major,minor)!=(6,0) or count>10000: raise ValueError('Unsupported bundle version/count')
    bundleid=readstr(f); locations=f.read(40)
    entries=[]
    for _ in range(count):
        off,size,compressed,kind=struct.unpack('<QQQB',f.read(25)); name=readstr(f)
        if off+(compressed or size)>header or size>512*1024*1024: raise ValueError('Invalid file range')
        entries.append([off,size,compressed,kind,name])
    matches=[e for e in entries if e[4]=='ARMeilleure.dll']
    if len(matches)!=1: raise ValueError('Exactly one ARMeilleure.dll required')
    entry=matches[0]; old=data[entry[0]:entry[0]+(entry[2] or entry[1])]
    if entry[2]: old=zlib.decompress(old,-15)
    if len(old)!=entry[1]: raise ValueError('Original assembly size mismatch')
    new=args.replacement.read_bytes()
    if not new.startswith(b'MZ'): raise ValueError('Replacement must be a PE assembly')
    out=bytearray(data); out.extend(b'\0'*((-len(out))%16)); newoff=len(out); out.extend(new)
    entry[0]=newoff; entry[1]=len(new); entry[2]=0
    newheader=len(out); h=io.BytesIO(); h.write(struct.pack('<III',major,minor,count)); writestr(h,hashlib.sha256(data+new).hexdigest()[:32]); h.write(locations)
    for off,size,compressed,kind,name in entries:
        h.write(struct.pack('<QQQB',off,size,compressed,kind)); writestr(h,name)
    out.extend(h.getvalue()); struct.pack_into('<Q',out,pointer,newheader)
    output.parent.mkdir(parents=True,exist_ok=True); output.write_bytes(out)
    receipt={'original':str(original),'original_sha256':sha,'bundle_version':[major,minor],'bundle_entries':count,'replaced_entry':'ARMeilleure.dll','original_assembly_sha256':hashlib.sha256(old).hexdigest(),'replacement_assembly_sha256':hashlib.sha256(new).hexdigest(),'output':str(output),'output_sha256':hashlib.sha256(out).hexdigest(),'scope':'Original apphost and every other bundled payload preserved; manifest refreshed with unique extraction id; no profile, keys or firmware copied.'}
    print(json.dumps(receipt,indent=2))
if __name__=='__main__': main()
