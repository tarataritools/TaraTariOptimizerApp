using System.Text.Json;
using LibreHardwareMonitor.Hardware;

try
{
    var computer = new Computer
    {
        IsCpuEnabled = true,
        IsGpuEnabled = true,
        IsMemoryEnabled = true,
        IsMotherboardEnabled = true,
        IsStorageEnabled = true,
        IsNetworkEnabled = true,
        IsControllerEnabled = true,
    };

    computer.Open();
    HardwareSnapshot[] hardware;
    try
    {
        hardware = computer.Hardware.Select(ReadHardware).ToArray();
    }
    finally
    {
        computer.Close();
    }

    Console.Out.Write(JsonSerializer.Serialize(
        new { hardware },
        new JsonSerializerOptions { PropertyNamingPolicy = JsonNamingPolicy.CamelCase }));
    return 0;
}
catch (Exception error)
{
    Console.Error.WriteLine(error.Message);
    return 1;
}

static HardwareSnapshot ReadHardware(IHardware hardware)
{
    hardware.Update();
    return new HardwareSnapshot(
        hardware.Name,
        hardware.HardwareType.ToString(),
        hardware.Sensors
            .Where(sensor => sensor.Value is float value && float.IsFinite(value))
            .Select(sensor => new SensorSnapshot(
                sensor.Name,
                sensor.SensorType.ToString(),
                sensor.Value!.Value))
            .ToArray(),
        hardware.SubHardware.Select(ReadHardware).ToArray());
}

record SensorSnapshot(string Name, string Type, float Value);
record HardwareSnapshot(
    string Name,
    string Type,
    SensorSnapshot[] Sensors,
    HardwareSnapshot[] SubHardware);
