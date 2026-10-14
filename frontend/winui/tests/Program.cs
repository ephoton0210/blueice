// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

using BlueIce.WinUI.Protocol;
using System.Buffers.Binary;
using System.Text.Json;

int passed = 0;
async Task Test(string name, Func<Task> run) { await run(); Console.WriteLine("PASS " + name); passed++; }
void Check(bool value) { if (!value) throw new Exception("Assertion failed."); }
async Task Reject<T>(Func<Task> run) where T : Exception { try { await run(); } catch (T) { return; } throw new Exception("Expected " + typeof(T).Name); }

await Test("canonical Rust envelope and UTF-8 text", async () => {
    using var stream = new MemoryStream();
    await BrowserWire.WriteAsync(stream, BrowserWire.Command("Navigate", new { url = "https://example.org/繁體" }), 7, 42);
    stream.Position = 0;
    var value = await BrowserWire.ReadAsync(stream);
    Check(value.GetProperty("tab_id").GetInt32() == 7 && value.GetProperty("request_id").GetInt32() == 42);
    Check(value.GetProperty("message").GetProperty("Navigate").GetProperty("url").GetString() == "https://example.org/繁體");
});
await Test("fragmented prefix and payload", async () => {
    using var stream = new MemoryStream();
    await BrowserWire.WriteAsync(stream, "GetHistoryState", 3, 9);
    using var chunks = new FragmentedStream(stream.ToArray());
    Check((await BrowserWire.ReadAsync(chunks)).GetProperty("message").GetString() == "GetHistoryState");
});
await Test("oversized and truncated frames", async () => {
    byte[] bytes = new byte[4]; BinaryPrimitives.WriteUInt32LittleEndian(bytes, BrowserWire.MaxMessageBytes + 1);
    await Reject<InvalidDataException>(async () => await BrowserWire.ReadAsync(new MemoryStream(bytes)));
    BinaryPrimitives.WriteUInt32LittleEndian(bytes, 12);
    await Reject<EndOfStreamException>(async () => await BrowserWire.ReadAsync(new MemoryStream(bytes)));
});
await Test("frame pixels, exact byte length and path boundary", async () => {
    string directory = Path.Combine(Path.GetTempPath(), "blueice-frame-test-" + Guid.NewGuid());
    Directory.CreateDirectory(directory);
    try {
        string path = Path.Combine(directory, "frame-1-1.rgba");
        await File.WriteAllBytesAsync(path, new byte[] { 10, 20, 30, 255 });
        Check(FramePixels.ReadBgra(directory, path, 1, 1).SequenceEqual(new byte[] { 30, 20, 10, 255 }));
        await Reject<InvalidDataException>(() => { FramePixels.ReadBgra(directory, path, 2, 1); return Task.CompletedTask; });
        await Reject<InvalidDataException>(() => { FramePixels.ReadBgra(directory, directory + "-other/frame.rgba", 1, 1); return Task.CompletedTask; });
        await Reject<InvalidDataException>(() => { FramePixels.ReadBgra(directory, path, 4097, 1); return Task.CompletedTask; });
    } finally { Directory.Delete(directory, true); }
});
if (args.Length == 1) await Test("native core process renders through the C# adapter", async () => {
    await using var session = new BrowserSession();
    var frame = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
    session.Received = envelope => {
        var message = envelope.GetProperty("message");
        if (message.ValueKind == JsonValueKind.Object && message.TryGetProperty("FrameReady", out var value)) {
            Check(envelope.GetProperty("tab_id").GetInt32() == 1);
            var pixels = FramePixels.ReadBgra(session.FrameDirectory, value.GetProperty("shm_path").GetString()!, value.GetProperty("width").GetInt32(), value.GetProperty("height").GetInt32());
            Check(pixels.Any(pixel => pixel != 255)); frame.TrySetResult();
        }
        return Task.CompletedTask;
    };
    session.Failed = message => frame.TrySetException(new Exception(message));
    await session.StartAsync(args[0]);
    await session.SendAsync(BrowserWire.Command("Navigate", new { url = "about:credits" }), 1);
    await frame.Task.WaitAsync(TimeSpan.FromSeconds(20));
});
Console.WriteLine($"{passed} tests passed");

sealed class FragmentedStream(byte[] bytes) : MemoryStream(bytes)
{
    public override ValueTask<int> ReadAsync(Memory<byte> buffer, CancellationToken cancellation = default) => base.ReadAsync(buffer[..Math.Min(buffer.Length, 1)], cancellation);
}
