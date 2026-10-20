#!/usr/bin/env python3
"""Corrupt actual APK native payloads and prove launch fails closed, in 9 cases.
These deliberately unsigned/invalid files are negative fixtures, not deliverables.
"""
import argparse, hashlib, json, os, subprocess, zipfile
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--rd',required=True,type=Path);p.add_argument('--apks',required=True,type=Path);p.add_argument('--out',required=True,type=Path);a=p.parse_args();a.out.mkdir(parents=True,exist_ok=True)
results=[]
for language,engine in [('c','c-ndk'),('cpp','cpp-ndk'),('rust','rust-ndk')]:
 source=a.apks/f'native-{language}.apk';broken=a.out/f'intentionally-invalid-{language}.apk'
 with zipfile.ZipFile(source) as src,zipfile.ZipFile(broken,'w',compression=zipfile.ZIP_DEFLATED) as dst:
  for info in src.infolist():
   if info.filename.startswith('META-INF/'):continue
   data=src.read(info.filename)
   if info.filename.startswith('lib/') and info.filename.endswith('/librdcalc.so'):data=b'\x7fELF'+bytes(20)
   dst.writestr(info.filename,data)
 for abi in ['x86_64','arm64-v8a','armeabi-v7a']:
  name=f'{language}-{abi}';env=dict(os.environ,RD_NATIVE_ABI=abi)
  result=subprocess.run([str(a.rd.resolve()),'engine','run',engine,str(broken.resolve()),'--json'],env=env,capture_output=True,text=True,timeout=60)
  (a.out/(name+'.stdout')).write_text(result.stdout);(a.out/(name+'.stderr')).write_text(result.stderr)
  errors=[]
  try:
   obj=json.loads(result.stderr)
   if isinstance(obj,dict) and 'code' in obj:errors.append(obj)
  except json.JSONDecodeError:pass
  for line in result.stderr.splitlines():
   try:
    obj=json.loads(line)
    if isinstance(obj,dict) and 'code' in obj:errors.append(obj)
   except json.JSONDecodeError:pass
  assert result.returncode!=0,(name,'corrupt native APK must fail',result.stdout,result.stderr)
  assert not result.stdout.strip(),(name,'no success JSON or fallback app stdout',result.stdout)
  assert errors,(name,'typed error required',result.stderr)
  error=errors[-1];assert error['code'] not in ('NOT_IMPLEMENTED','UNKNOWN_ENGINE','MISSING_ENTRY'),(name,error)
  assert any(x in (error['cause']+' '+error['code']).lower() for x in ('elf','native','librdcalc')),(name,error)
  entry={'language':language,'engine':engine,'abi':abi,'success':True,'negative_fixture_sha256':hashlib.sha256(broken.read_bytes()).hexdigest(),'source_apk_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),'exit_code':result.returncode,'typed_error':error,'success_stdout_absent':True};results.append(entry);print(name,error['code'],'FAIL-CLOSED OK',flush=True)
(a.out/'summary.json').write_text(json.dumps({'success':len(results)==9,'negative_cases':results},indent=2)+'\n')
