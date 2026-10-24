$ErrorActionPreference='Stop'
Push-Location (Split-Path -Parent $PSScriptRoot)
try {
  & python -X utf8 "$PSScriptRoot/test-jni-constructor-abi.py"
  if($LASTEXITCODE){throw 'JNI constructor ABI oracle failed'}
}finally{Pop-Location}
