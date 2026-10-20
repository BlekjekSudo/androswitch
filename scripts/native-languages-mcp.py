#!/usr/bin/env python3
"""Auditable 47-tool end-to-end matrix for genuine C/C++/Rust Android APKs."""
import argparse,base64,hashlib,json,os,selectors,signal,subprocess,time
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--rd',type=Path,required=True);p.add_argument('--apks',type=Path,required=True);p.add_argument('--out',type=Path,required=True);p.add_argument('--abi',default='x86_64');p.add_argument('--only',choices=['c','cpp','rust']);p.add_argument('--smoke',action='store_true');p.add_argument('--jit',choices=['on','off'],default='off');a=p.parse_args();a.rd=a.rd.resolve();a.apks=a.apks.resolve();a.out=a.out.resolve();a.out.mkdir(parents=True,exist_ok=True)
def digest(tag,left,right):
 h=2166136261^tag
 for n in (left,right):
  for i in range(4):h=((h^((n>>(8*i))&255))*16777619)&0xffffffff
 return h&0x7fffffff
class Session:
 def __init__(self,folder):
  self.folder=folder;folder.mkdir(parents=True,exist_ok=True);self.seq=0;self.pending=b'';self.coverage={};self.calls=0;self.schemas={}
  env=dict(os.environ,RD_NATIVE_ABI=a.abi,RD_NATIVE_PYTHON=os.environ.get('RD_NATIVE_PYTHON','python'),RD_GC='on',RD_JIT=a.jit,RD_MCP_SANDBOX=os.pathsep.join([str(a.apks),str(folder)]))
  self.proc=subprocess.Popen([str(a.rd),'mcp'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=(folder/'stderr.log').open('wb'),bufsize=0,env=env,start_new_session=True)
  self.sel=selectors.DefaultSelector();self.sel.register(self.proc.stdout,selectors.EVENT_READ);self.log=(folder/'transcript.jsonl').open('w')
 def rpc(self,method,params=None):
  self.seq+=1;req={'jsonrpc':'2.0','id':self.seq,'method':method,'params':params or {}};start=time.monotonic();self.proc.stdin.write((json.dumps(req)+'\n').encode());limit=time.monotonic()+120
  while True:
   while b'\n' not in self.pending:
    if not self.sel.select(max(0,limit-time.monotonic())):raise RuntimeError('MCP timeout '+method)
    data=os.read(self.proc.stdout.fileno(),65536)
    if not data:raise RuntimeError('MCP exited '+str(self.proc.poll()))
    self.pending+=data
   line,self.pending=self.pending.split(b'\n',1);res=json.loads(line)
   if res.get('id')==self.seq:break
  self.log.write(json.dumps({'id':self.seq,'request':req,'response':res,'elapsed_ms':round((time.monotonic()-start)*1000,3)},ensure_ascii=False)+'\n');self.log.flush();return res
 def notify(self,method,params=None):
  req={"jsonrpc":"2.0","method":method,"params":params or {}};self.proc.stdin.write((json.dumps(req)+"\n").encode());self.log.write(json.dumps({"request":req,"notification":True})+"\n");self.log.flush()
 def call(self,name,args=None,error=None):
  if not error and name in self.schemas:
   schema=self.schemas[name];keys=set(args or {});assert not keys-set(schema.get('properties',{})),(name,'unknown arguments',keys);assert set(schema.get('required',[]))<=keys,(name,'missing required arguments')
  res=self.rpc('tools/call',{'name':name,'arguments':args or {}});self.calls+=1
  assert 'error' not in res,(name,res)
  value=res['result'];texts=[x['text'] for x in value.get('content',[]) if x['type']=='text'];decoded=None
  if texts:
   try:decoded=json.loads(texts[0])
   except (ValueError,TypeError):decoded=texts[0]
  entry=self.coverage.setdefault(name,{'positive':0,'negative':0,'error_codes':[]})
  if error is not None:
   assert value.get('isError'),(name,'expected error',value)
   code=decoded.get('code') if isinstance(decoded,dict) else None
   assert code in error if isinstance(error,(tuple,list,set)) else code==error,(name,error,decoded)
   entry['negative']+=1;entry['error_codes'].append(code)
  else:
   assert not value.get('isError'),(name,decoded)
   assert not any('!! draw pass failed' in t for t in texts),(name,texts)
   if isinstance(decoded,dict): assert decoded.get('ok') is not False,(name,decoded)
   if name=='assert_finished': assert decoded['finished'],decoded
   entry['positive']+=1
  print(self.folder.name,self.calls,name,'ERROR '+str(error) if error else 'OK',flush=True)
  return decoded,value,texts
 def close(self):
  self.log.close()
  try:os.killpg(self.proc.pid,signal.SIGTERM);self.proc.wait(timeout=5)
  except (ProcessLookupError,subprocess.TimeoutExpired):
   try:os.killpg(self.proc.pid,signal.SIGKILL)
   except ProcessLookupError:pass
 def image(self,name,args,path):
  _,res,_=self.call(name,args);image=next(c for c in res['content'] if c['type']=='image');data=base64.b64decode(image['data']);assert data.startswith(b'\x89PNG\r\n\x1a\n');path.write_bytes(data)
all_results=[]
for ident,label,tag in [('c','C',1),('cpp','C++',2),('rust','Rust',3)]:
 if a.only and a.only!=ident:continue
 folder=a.out/(ident+'-'+a.abi);s=Session(folder);summary={'language':label,'id':ident,'abi':a.abi,'jit_requested':a.jit,'success':False,'apk_sha256':hashlib.sha256((a.apks/f'native-{ident}.apk').read_bytes()).hexdigest()};start=time.monotonic();pkg='org.rustydroid.demo.'+ident;cls='Lorg/rustydroid/nativeworkbench/MainActivity;'
 try:
  init=s.rpc('initialize',{'protocolVersion':'2024-11-05','capabilities':{},'clientInfo':{'name':'native-language-gate','version':'1'}});assert 'result' in init
  s.notify('notifications/initialized')
  tools=s.rpc('tools/list')['result']['tools'];s.schemas={t['name']:t['inputSchema'] for t in tools};names={t['name'] for t in tools};assert len(names)==47
  s.call('install',{'apk':str(a.apks/f'native-{ident}.apk')})
  info,_,_=s.call('app_info');assert info['package']==pkg
  assert info['engines']['execution_claimed'] is False
  assert {e['id'] for e in info['engines']['engines'] if e['registry_adapter_status']=='AVAILABLE_BOUNDED'}=={'c-ndk','cpp-ndk','rust-ndk'}
  health,_,_=s.call('runtime_health');assert health['running'] is False
  s.call('permissions_grant',{'permission':'android.permission.POST_NOTIFICATIONS'})
  s.call('launch',{'engine':{'c':'c-ndk','cpp':'cpp-ndk','rust':'rust-ndk'}[ident]})
  if a.jit=='on':
   health,_,_=s.call('runtime_health');assert 'jit' in health,'JIT-on replay requires a JIT-enabled binary'
  native,_,_=s.call('native_state');assert native['selected_engine']=={'c':'c-ndk','cpp':'cpp-ndk','rust':'rust-ndk'}[ident] and native['enabled'] and native['instructions']>0 and not native['missing_imports'],native
  assert native['abi']=={'x86_64':'X86_64','arm64-v8a':'Arm64','armeabi-v7a':'Arm'}[a.abi],native
  initial_instructions=native['instructions'];summary['native_initial']=native
  if not a.smoke:
   views,_,_=s.call('find_views',{'resource_id':pkg+':id/compute','enabled':True});assert views['total']==1 and not views['truncated'],views
   selected,_,_=s.call('tap_view',{'resource_id':pkg+':id/compute','expected_token':views['tree_token']});assert selected['listener_acionado'],selected
   waited,_,_=s.call('wait_for_view',{'resource_id':pkg+':id/result','timeout_ms':0});assert waited['condition_met'] and waited['waited_ms']==0,waited
   s.call('tap_view',{'resource_id':pkg+':id/compute','expected_token':views['tree_token']},error='STALE_VIEW')
   failure,_,_=s.call('last_error');assert failure['error']['tool']=='tap_view' and failure['error']['code']=='STALE_VIEW',failure
   info,_,_=s.call('session_info');assert info['tool_count']==47 and info['running_package']==pkg and info['running'],info
   tail,_,_=s.call('logs_tail',{'max_bytes':31});assert len(base64.b64decode(tail['data_b64']))==tail['bytes']<=31,tail
  s.call('assert_text',{'resource_id':pkg+':id/result','expected':'Result: '+str(digest(tag,7,5))})
  # The result oracle is Python, independent of Java/native implementations.
  for x,y in [(42,9),(-17,3),(2147483647,-2147483648),(0,0)]:
   result,_,_=s.call('invoke',{'class':cls,'method':'digest','sig':'(II)I','args':[x,y]});assert result["value"]==digest(tag,x,y),result
  flavor,_,_=s.call('invoke',{'class':cls,'method':'nativeFlavor','sig':'()I','args':[]});assert flavor['value']==tag,flavor
  if a.smoke:
   after,_,_=s.call('native_state');assert after['instructions']>initial_instructions;summary.update(native_final=after,success=True,mode='native APK launch and four JNI arithmetic oracles')
  else:
   s.call('package_list');s.call('permissions_list');s.call('permissions_audit');s.call('device_profile');s.call('behavior_switches');s.call('compat_gaps');s.call('get_api_usage')
   observed,_,texts=s.call('observe',{'format':'text'});assert label+' native workbench' in '\n'.join(texts)
   s.call('ui_tree',{'format':'xml'})
   s.call('assert_activity',{'activity':'org.rustydroid.nativeworkbench.MainActivity'})
   s.call('assert_view',{'resource_id':pkg+':id/left'})
   found,_,_=s.call('wait_for',{'resource_id':pkg+':id/right','timeout_ms':300});assert found['found'],found
   tree,_,_=s.call('accessibility',{'action':'tree'});editors=[n for n in tree['nodes'] if n.get('editable')];assert len(editors)==2,tree
   left,right=editors[0]['id'],editors[1]['id']
   s.call('ime',{'action':'focus','view':left})
   s.call('type_text',{'text':'42','replace':True});s.call('key',{'key':'DEL'});s.call('type_text',{'text':'42','replace':True})
   s.call('assert_text',{'resource_id':pkg+':id/left','expected':'42'})
   s.call('ime',{'action':'focus','view':right});s.call('ime',{'action':'select','start':0,'end':1});s.call('ime',{'action':'commit','text':'9'})
   changed,_,_=s.call('accessibility',{'action':'perform','node':right,'action_id':2097152,'text':'9'});assert changed['performed']
   s.call('runtime_health')
   s.call('tap_text',{'text':'Compute '+label,'exact':True})
   expected='Result: '+str(digest(tag,42,9));s.call('assert_text',{'resource_id':pkg+':id/result','expected':expected})
   # Pointer input hits the same actual JNI-backed compute listener.
   s.call('tap',{'x':200,'y':168});s.call('assert_text',{'resource_id':pkg+':id/result','expected':expected})
   s.call('swipe',{'x1':700,'y1':1000,'x2':700,'y2':1100,'ms':100});s.call('wait',{'ms':50})
   s.image('screenshot',{},folder/'native-result.png')
   logs,_,texts=s.call('logs');assert 'Native result=' in '\n'.join(texts)
   coverage,_,_=s.call('get_coverage');assert coverage['methods_executed']>=5,coverage
   s.call('component_service',{'class':'org.rustydroid.nativeworkbench.ProofIme','role':'ime'})
   s.call('component_service',{'class':'org.rustydroid.nativeworkbench.ProofAccessibility','role':'accessibility'})
   s.call('component_service',{'class':'org.rustydroid.nativeworkbench.ProofListener','role':'notification_listener'})
   provider,_,_=s.call('component_service',{'class':'org.rustydroid.nativeworkbench.ProofWidget','role':'widget'})
   widget,_,_=s.call('widgets',{'action':'allocate'});wid=widget['id']
   s.call('widgets',{'action':'bind','id':wid,'provider':provider['object']})
   s.image('widget_screenshot',{'id':wid},folder/'native-widget.png')
   s.call('widgets',{'action':'delete','id':wid})
   s.call('tap_text',{'text':'Notify '+label,'exact':True})
   notices,_,_=s.call('notifications');assert notices['permission_granted'] and len(notices['notifications'])==1,notices
   s.call('notifications',{'action':'click','id':17})
   s.call('assert_activity',{'activity':'Lorg/rustydroid/nativeworkbench/DetailsActivity;'})
   s.call('runtime_health',{'gc':True})
   _,_,texts=s.call('observe',{'format':'text'});assert 'Native proof '+label in '\n'.join(texts)
   s.call('key',{'key':'BACK'})
   s.call('notifications',{'action':'cancel','id':17})
   s.call('assert_activity',{'activity':cls});s.call('tap_text',{'text':'Compute '+label,'exact':True});s.call('assert_text',{'resource_id':pkg+':id/result','expected':expected})
   s.call('tap_text',{'text':'Pin shortcut '+label,'exact':True})
   shortcuts,_,_=s.call('shortcuts');assert len(shortcuts['shortcuts'])==1,shortcuts
   s.call('shortcuts',{'action':'launch','id':'native-proof'})
   _,_,texts=s.call('observe',{'format':'text'});assert 'Native proof '+label in '\n'.join(texts)
   s.call('key',{'key':'BACK'})
   s.call('permissions_revoke',{'permission':'android.permission.POST_NOTIFICATIONS'})
   notices,_,_=s.call('notifications');assert notices['permission_granted'] is False
   s.call('permissions_grant',{'permission':'android.permission.POST_NOTIFICATIONS'})
   batch,_,_=s.call('batch',{'calls':[{'name':'assert_text','arguments':{'resource_id':pkg+':id/result','expected':expected}},{'name':'native_state','arguments':{}}]});assert '"ok": false' not in json.dumps(batch) and '"isError": true' not in json.dumps(batch),batch
   # Negative contracts do not count as positive coverage.
   before,_,_=s.call('native_state')
   s.call('invoke',{'class':cls,'method':'missingNativeMethod','sig':'()I','args':[]},error='NOT_FOUND')
   after,_,_=s.call('native_state');assert after['instructions']==before['instructions'],(before,after)
   s.call('launch',{'engine':'flutter'},error='NOT_IMPLEMENTED')
   s.call('launch',{'engine':'not-a-real-engine'},error='UNKNOWN_ENGINE')
   s.call('launch',{'engine':17},error='INVALID_FORMAT')
   s.call('component_service',{'class':'org.rustydroid.nativeworkbench.Missing','role':'ime'},error='INVALID_ARG')
   s.call('ime',{'action':'select','start':-1,'end':0},error='INVALID_ARG')
   s.call('shortcuts',{'action':'launch','id':'absent'},error='JAVA_EXCEPTION')
   s.call('screenshot',{'path':'/etc/native-language-proof.png'},error='PATH_OUTSIDE_SANDBOX')
   s.call('tap_text',{'text':'NOT A REAL BUTTON'},error='VIEW_NOT_FOUND')
   final_native,_,_=s.call('native_state');assert final_native['instructions']>initial_instructions and not final_native['missing_imports'];summary['native_final']=final_native
   s.call('runtime_health');s.call('assert_text',{'resource_id':pkg+':id/result','expected':expected})
   s.call('launch');s.call('assert_text',{'resource_id':pkg+':id/result','expected':expected})
   s.call('tap_text',{'text':'Finish '+label,'exact':True});s.call('assert_finished')
   s.call('package_clear',{'package':pkg});s.call('package_remove',{'package':pkg})
   remaining,_,_=s.call('package_list');assert pkg not in json.dumps(remaining)
   missing=names-{n for n,c in s.coverage.items() if c['positive']>0};assert not missing,('missing positive coverage',missing)
   summary.update(success=True,tools_registered=len(names),positive_tools=len(names),missing_positive_tools=sorted(missing),typed_result=expected,forced_gc_and_warm_restart_persistence=True)
 except Exception as e:
  summary['error']=str(e);print('FAILED',ident,str(e),flush=True)
 finally:
  summary.update(calls=s.calls,coverage=s.coverage,elapsed_seconds=round(time.monotonic()-start,3));(folder/'summary.json').write_text(json.dumps(summary,indent=2)+'\n');s.close();all_results.append(summary)
 if not summary['success']:break
(a.out/'matrix-summary.json').write_text(json.dumps({'success':all(s['success'] for s in all_results) and (len(all_results)==1 if a.only else len(all_results)==3),'abi':a.abi,'runs':all_results},indent=2)+'\n')
raise SystemExit(0 if all(s['success'] for s in all_results) and (len(all_results)==1 if a.only else len(all_results)==3) else 1)
