using ARMeilleure;
using ARMeilleure.Common;
using ARMeilleure.Translation;
using Ryujinx.Cpu;
using Ryujinx.Cpu.Jit;
using Ryujinx.Memory;
using Context = ARMeilleure.State.ExecutionContext;

if (args.Length > 0 && args[0] == "identity")
{
    bool allMatched = true;
    foreach (string path in args.Skip(1))
    {
        string name = Path.GetFileNameWithoutExtension(path);
        string expected = $"{name}, Version=1.3.3.0, Culture=neutral, PublicKeyToken=null";
        string observed = System.Reflection.AssemblyName.GetAssemblyName(path).FullName;
        bool matched = expected == observed;
        allMatched &= matched;
        Console.WriteLine(System.Text.Json.JsonSerializer.Serialize(new { path, expected, observed, passed = matched }));
    }
    return allMatched && args.Length > 1 ? 0 : 1;
}
bool actual = args.Contains("execute");
uint opcode = args.Length > 1 ? Convert.ToUInt32(args[1], 16) : 0xd5380429u;
ulong size = MemoryBlock.GetPageSize();
ulong code = size;
using var ram = new MemoryBlock(size * 2);
var memory = new MemoryManager(ram, size * 16);
memory.IncrementReferenceCount();
memory.Map(code, 0, size, MemoryMapFlags.Private);
// TBZ W0,#11,+8; MRS X9,<unsupported>; MOV X1,#42; BRK #0.
memory.Write(code, 0x36580040u);
memory.Write(code + 4, opcode);
memory.Write(code + 8, 0xd2800541u);
memory.Write(code + 12, 0xd4200000u);
int breaks = 0, undefined = 0;
ulong faultPc = 0;
int faultWord = 0;
var context = new Context(new JitMemoryAllocator(), new TickSource(19200000),
    breakCallback: (ctx, address, imm) => { breaks++; ctx.StopRunning(); },
    undefinedCallback: (ctx, address, word) => { undefined++; faultPc = address; faultWord = word; ctx.StopRunning(); });
context.SetX(0, actual ? 0x803ul : 3ul);
context.SetX(1, 0xfeed);
context.SetX(9, 0x1234);
Optimizations.AllowLcqInFunctionTable = false;
Optimizations.UseUnmanagedDispatchLoop = args.Contains("unmanaged");
var table = AddressTable<ulong>.CreateForArm(true, memory.Type);
var translator = new Translator(new JitMemoryAllocator(), memory, table);
translator.Execute(context, code);
bool passed = actual ?
    (undefined == 1 && breaks == 0 && faultPc == code + 4 && unchecked((uint)faultWord) == opcode && context.GetX(1) == 0xfeed && context.GetX(9) == 0x1234) :
    (undefined == 0 && breaks == 1 && context.GetX(1) == 42 && context.GetX(9) == 0x1234);
Console.WriteLine(System.Text.Json.JsonSerializer.Serialize(new { mode = actual ? "execute" : "guarded", dispatch = Optimizations.UseUnmanagedDispatchLoop ? "unmanaged" : "managed", opcode = $"0x{opcode:x8}", passed, breaks, undefined, faultPc, faultWord = $"0x{unchecked((uint)faultWord):x8}", x1 = context.GetX(1), x9 = context.GetX(9) }));
context.Dispose();
memory.DecrementReferenceCount();
return passed ? 0 : 1;
