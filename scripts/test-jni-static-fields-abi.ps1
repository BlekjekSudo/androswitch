$ErrorActionPreference='Stop'
Push-Location (Split-Path -Parent $PSScriptRoot)
try{
  & python -X utf8 "$PSScriptRoot/test-jni-static-fields-abi.py"
  if($LASTEXITCODE){throw 'JNI static-field ABI oracle failed'}
}finally{Pop-Location}
