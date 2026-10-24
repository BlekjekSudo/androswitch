# Java source cannot declare two fields with one name and distinct descriptors,
# but JVM class files and DEX permit this. Preserve all bytecode/types/values;
# change only the equal-length UTF8 field name omega -> alpha after javac.
param([Parameter(Mandatory)][string]$Path)
$ErrorActionPreference='Stop'
$bytes=[IO.File]::ReadAllBytes($Path)
if($bytes.Length -lt 10 -or $bytes[0] -ne 0xca -or $bytes[1] -ne 0xfe){throw 'Expected JVM class file'}
$count=([int]$bytes[8] -shl 8) -bor $bytes[9]
$cursor=10;$replaced=0
for($index=1;$index -lt $count;$index++){
  $tag=$bytes[$cursor];$cursor++
  switch($tag){
    1 {
      $length=([int]$bytes[$cursor] -shl 8) -bor $bytes[$cursor+1];$cursor+=2
      if($length -eq 5 -and [Text.Encoding]::UTF8.GetString($bytes,$cursor,$length) -ceq 'omega'){
        [Array]::Copy([Text.Encoding]::UTF8.GetBytes('alpha'),0,$bytes,$cursor,5);$replaced++
      }
      $cursor+=$length
    }
    {$_ -in 3,4,9,10,11,12,17,18} {$cursor+=4}
    {$_ -in 5,6} {$cursor+=8;$index++}
    {$_ -in 7,8,16,19,20} {$cursor+=2}
    15 {$cursor+=3}
    default {throw "Unsupported class constant pool tag $tag"}
  }
}
if($replaced -ne 1){throw "Expected one distinct field name, found $replaced"}
[IO.File]::WriteAllBytes($Path,$bytes)
