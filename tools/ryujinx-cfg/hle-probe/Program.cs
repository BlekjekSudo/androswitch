using System.Reflection;
using System.Runtime.CompilerServices;
using System.Runtime.Loader;
using System.Text.Json;

try {
// Exercise the actual emulator allocator/context implementation without opening
// a window, constructing a GPU, or accessing a profile/firmware/key file.
if (args.Length != 2 || args[1] is not ("fixed" or "baseline"))
    throw new ArgumentException("HleProbe <directory-containing-Ryujinx.HLE.dll> fixed|baseline");
string input = Path.GetFullPath(args[0]);
string assemblyPath = File.Exists(input) ? input : Path.Combine(input, "Ryujinx.HLE.dll");
string root = Path.GetDirectoryName(assemblyPath)!;
AssemblyLoadContext.Default.Resolving += (context, name) => {
    string path = Path.Combine(root, name.Name + ".dll");
    return File.Exists(path) ? context.LoadFromAssemblyPath(path) : null;
};
Assembly hle = AssemblyLoadContext.Default.LoadFromAssemblyPath(assemblyPath);
Type Find(string suffix) => hle.GetType(suffix switch {
    ".NvHostAsGpuDeviceFile" => "Ryujinx.HLE.HOS.Services.Nv.NvDrvServices.NvHostAsGpu.NvHostAsGpuDeviceFile",
    ".NvMemoryAllocator" => "Ryujinx.HLE.HOS.Services.Nv.NvMemoryAllocator",
    ".InfoType" => "Ryujinx.HLE.HOS.Kernel.SupervisorCall.InfoType",
    _ => "Ryujinx.HLE.HOS.Services.Nv.NvDrvServices.NvHostAsGpu.Types" + suffix
}, true)!;
Type fileType = Find(".NvHostAsGpuDeviceFile"), allocatorType = Find(".NvMemoryAllocator");
Type contextType = Find(".AddressSpaceContext"), argumentsType = Find(".AllocSpaceArguments");
const BindingFlags All = BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance;
object file = RuntimeHelpers.GetUninitializedObject(fileType);
object context = Activator.CreateInstance(contextType, All, null, new object[] { null }, null)!;
fileType.GetField("_asContext", All)!.SetValue(file, context);
fileType.GetField("_memoryAllocator", All)!.SetValue(file, Activator.CreateInstance(allocatorType, true));
int checks = 0;
void Check(bool value, string contract) { if (!value) throw new Exception(contract); checks++; }
(int result, ulong offset) Allocate(ulong address, uint pages, bool fixedAddress) {
    object a = Activator.CreateInstance(argumentsType)!;
    argumentsType.GetField("Pages")!.SetValue(a, pages);
    argumentsType.GetField("PageSize")!.SetValue(a, 0x1000u);
    FieldInfo flags = argumentsType.GetField("Flags")!;
    flags.SetValue(a, Enum.ToObject(flags.FieldType, fixedAddress ? 1 : 0));
    argumentsType.GetField("Offset")!.SetValue(a, address);
    object[] parameters = { a };
    int result = Convert.ToInt32(fileType.GetMethod("AllocSpace", All)!.Invoke(file, parameters));
    return (result, (ulong)argumentsType.GetField("Offset")!.GetValue(parameters[0])!);
}
bool Valid(ulong address, ulong size) => (bool)contextType.GetMethod("ValidateFixedBuffer")!.Invoke(context, new object[] {address, size, 0x1000ul})!;
const ulong requested = 0x1ffff80000;
var first = Allocate(requested, 0x80, true);
if (args[1] == "baseline") {
    Check(first.result == 0 && first.offset != requested, "Original fixed-address defect must reproduce");
    Check(!Valid(requested, 0x80000), "Requested range is missing from original reservations");
} else {
    Check(first.result == 0 && first.offset == requested, "Fixed allocation must preserve requested address");
    Check(Valid(requested, 0x80000), "Entire fixed range must be reserved");
    Check(!Valid(requested, 0x81000), "A mapping cannot cross the reservation end");
    Check(!Valid(requested + 1, 0x1000), "Unaligned mapping must fail");
    Check(!Valid(requested, 0), "Zero-sized mapping must fail");
    Check(!Valid(ulong.MaxValue - 0xfff, 0x2000), "Wrapping mapping must fail");
    var overlap = Allocate(requested, 1, true);
    Check(overlap.result != 0, "Overlapping allocation must fail");
    var second = Allocate(0x100000, 2, true);
    Check(second.result == 0 && second.offset == 0x100000 && Valid(second.offset, 0x2000), "Second free gap must work");
    contextType.GetMethod("AddMap")!.Invoke(context, new object[] {requested, 0x1000ul, 0ul, false});
    Check(!Valid(requested, 0x1000), "Existing mapping overlap must fail");
    Check(Valid(requested + 0x1000, 0x1000), "Adjacent nonoverlapping mapping must work");
    var automatic = Allocate(0x1000, 1, false);
    Check(automatic.result == 0 && Valid(automatic.offset, 0x1000), "Automatic allocations must still work");
    Type info = Find(".InfoType");
    Check(Convert.ToUInt32(Enum.Parse(info, "ThreadTickCountDeprecated")) == 0xf0000002u,
        "Legacy kernel thread accounting alias must retain its ABI value");
}
Console.WriteLine(JsonSerializer.Serialize(new { mode = args[1], checks, requested, returned = first.offset,
    assembly = hle.FullName, status = args[1] == "baseline" ? "baseline_defect_reproduced" : "PASS" }));

} catch (Exception error) { Console.Error.WriteLine(error.ToString()); Environment.ExitCode = 1; }
