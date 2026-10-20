#!/usr/bin/env python3
"""Build three real, debug-signed Android APKs with C/C++/Rust native cores.
SDK/NDK/D8 inputs are SHA-256 pinned. Generated keys/binaries stay outside Git.
"""
from pathlib import Path
import os,subprocess,sys,json,hashlib,zipfile,shutil,urllib.request,argparse
ROOT=Path(__file__).resolve().parents[2];SRC=Path(__file__).resolve().parent
p=argparse.ArgumentParser();p.add_argument('--out',type=Path,default=ROOT/'target/native-languages/apks');p.add_argument('--cache',type=Path,default=Path(os.environ.get('RD_ANDROID_BUILD_CACHE',Path.home()/'.cache/native-language-sdk')));a=p.parse_args()
CACHE=a.cache.resolve();OUT=a.out.resolve();WORK=ROOT/'target/native-languages/build';CACHE.mkdir(parents=True,exist_ok=True);OUT.mkdir(parents=True,exist_ok=True);WORK.mkdir(parents=True,exist_ok=True)
PINS={
 'platform26.zip':('https://dl.google.com/android/repository/platform-26_r02.zip','2aafa7d19c5e9c4b643ee6ade3d85ef89dc2f79e8383efdb9baf7fddad74b52a'),
 'ndk-r27c.zip':('https://dl.google.com/android/repository/android-ndk-r27c-linux.zip','59c2f6dc96743b5daf5d1626684640b20a6bd2b1d85b13156b90333741bad5cc'),
 'r8.jar':('https://dl.google.com/android/maven2/com/android/tools/r8/8.5.35/r8-8.5.35.jar','4733945987ee0a840fafc34080b135259e01678412e07212b23f706334290294')}
def sha(p):
 h=hashlib.sha256()
 with p.open("rb") as f:
  for chunk in iter(lambda:f.read(1024*1024),b""):h.update(chunk)
 return h.hexdigest()
def run(*args,**kw):print('+',' '.join(map(str,args)),flush=True);subprocess.run(list(map(str,args)),check=True,**kw)
for name,(url,digest) in PINS.items():
 f=CACHE/name
 if not f.exists() or sha(f)!=digest:
  tmp=f.with_suffix('.download');urllib.request.urlretrieve(url,tmp)
  if sha(tmp)!=digest:raise RuntimeError(name+" download digest mismatch")
  tmp.replace(f)
 if sha(f)!=digest:raise RuntimeError(name+" cached digest mismatch")
android=CACHE/'platform26/android-8.0.0/android.jar'
if not android.exists():
 with zipfile.ZipFile(CACHE/'platform26.zip') as z:z.extractall(CACHE/'platform26')
ndk=CACHE/'android-ndk-r27c/toolchains/llvm/prebuilt/linux-x86_64'
if not (ndk/'bin/clang').exists():
 # zipfile doesn't preserve Unix executable/symlink attributes; use unzip.
 run('unzip','-q',CACHE/'ndk-r27c.zip','android-ndk-r27c/toolchains/llvm/prebuilt/linux-x86_64/*','android-ndk-r27c/source.properties','-d',CACHE)
key=WORK/'debug-only.jks'
if not key.exists():
 run('keytool','-genkeypair','-keystore',key,'-storepass','android','-keypass','android','-alias','androiddebugkey','-keyalg','RSA','-keysize','2048','-validity','3650','-dname','CN=RustyDroid Native Demo,O=Test Only,C=BR')
abis={'x86_64':('x86_64-linux-android26','x86_64-linux-android'),'arm64-v8a':('aarch64-linux-android26','aarch64-linux-android'),'armeabi-v7a':('armv7a-linux-androideabi26','armv7-linux-androideabi')}
manifest={'schema_version':1,'kind':'project-owned real native Android demonstration APKs, not third-party applications','source_commit':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),'source_dirty':bool(subprocess.check_output(['git','status','--porcelain'],cwd=ROOT,text=True).strip()),'build_inputs':{n:{'url':u,'sha256':h} for n,(u,h) in PINS.items()},'rustc':subprocess.check_output(['rustc','-Vv'],text=True).strip(),'ndk':'27.2.12479018','apks':[],'source_sha256':{str(x.relative_to(SRC)):sha(x) for x in SRC.rglob('*') if x.is_file() and x.suffix in ('.java','.c','.cpp','.rs','.xml','.py')}}
manifest["compilers"]={"clang":subprocess.check_output([str(ndk/"bin/clang"),"--version"],text=True).strip(),"clang++":subprocess.check_output([str(ndk/"bin/clang++"),"--version"],text=True).strip(),"javac":subprocess.check_output(["javac","-version"],text=True,stderr=subprocess.STDOUT).strip(),"aapt":subprocess.check_output(["aapt","version"],text=True).strip(),"apksigner":subprocess.check_output(["apksigner","version"],text=True).strip(),"python":sys.version}
for ident,label,tag in [('c','C',1),('cpp','C++',2),('rust','Rust',3)]:
 w=WORK/ident
 if w.exists():shutil.rmtree(w)
 for n in ('src/org/rustydroid/nativeworkbench','generated','classes','dex','lib'): (w/n).mkdir(parents=True,exist_ok=True)
 for src in (SRC/'java').glob('*.java'):(w/'src/org/rustydroid/nativeworkbench'/src.name).write_text(src.read_text().replace('@LANG@',label))
 (w/'AndroidManifest.xml').write_text((SRC/'AndroidManifest.xml').read_text().replace('@ID@',ident).replace('@LANG@',label))
 run('aapt','package','-f','-M',w/'AndroidManifest.xml','-S',SRC/'res','-I',android,'-F',w/'base.apk','-J',w/'generated','--custom-package','org.rustydroid.nativeworkbench')
 java=list((w/'src').rglob('*.java'))+list((w/'generated').rglob('*.java'))
 run('javac','--release','8','-cp',android,'-d',w/'classes',*java)
 run('java','-cp',CACHE/'r8.jar','com.android.tools.r8.D8','--lib',android,'--min-api','26','--output',w/'dex',*list((w/'classes').rglob('*.class')))
 libs={}
 for abi,(triple,target) in abis.items():
  lib=w/'lib'/abi/'librdcalc.so';lib.parent.mkdir(parents=True,exist_ok=True)
  cc=ndk/'bin'/(triple+'-clang')
  if ident=='rust':
   run('rustc',SRC/'native/rust/calc.rs','--edition=2021','--crate-name','rdcalc','--crate-type','cdylib','--target',target,'-C','opt-level=2','-C','panic=abort','-C',f'linker={cc}','-C','link-arg=-Wl,--build-id=none','-o',lib)
  else:
   compiler=ndk/'bin'/(triple+('-clang++' if ident=='cpp' else '-clang'))
   opts=['-std=c++20','-fno-exceptions','-fno-rtti','-nostdlib++'] if ident=='cpp' else ['-std=c11']
   run(compiler,'-shared','-fPIC','-O2','-Wl,--build-id=none','-Wl,-soname,librdcalc.so',*opts,SRC/f'native/{ident}/calc.{"cpp" if ident=="cpp" else "c"}','-o',lib)
  libs[abi]={'sha256':sha(lib),'bytes':lib.stat().st_size,'elf_machine':int.from_bytes(lib.read_bytes()[18:20],'little')}
 unsigned=w/'unsigned.apk';shutil.copy2(w/'base.apk',unsigned)
 with zipfile.ZipFile(unsigned,'a',compression=zipfile.ZIP_DEFLATED) as z:
  z.write(w/'dex/classes.dex','classes.dex')
  for abi in abis:z.write(w/'lib'/abi/'librdcalc.so',f'lib/{abi}/librdcalc.so')
 run('zipalign','-f','4',unsigned,w/'aligned.apk')
 apk=OUT/f'native-{ident}.apk'
 run('apksigner','sign','--ks',key,'--ks-pass','pass:android','--key-pass','pass:android','--out',apk,w/'aligned.apk')
 run('apksigner','verify','--verbose',apk)
 manifest['apks'].append({'language':label,'id':ident,'engine':{'c':'c-ndk','cpp':'cpp-ndk','rust':'rust-ndk'}[ident],'tag':tag,'package':f'org.rustydroid.demo.{ident}','apk':apk.name,'sha256':sha(apk),'libraries':libs,'signing':'debug-only generated certificate; never use for production'})
(OUT/'provenance.json').write_text(json.dumps(manifest,indent=2)+'\n')
print('Three signed APKs and compiler/source/library provenance:',OUT)
