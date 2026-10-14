// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

using System.Diagnostics;
using System.Text.Json;

namespace BlueIce.WinUI.Protocol;

public sealed class BrowserSession : IAsyncDisposable
{
    private readonly SemaphoreSlim writer = new(1, 1);
    private readonly CancellationTokenSource lifetime = new();
    private Process? core;
    private Task? reader;
    private long requestId;
    private int disposed;
    public string FrameDirectory { get; } = Path.Combine(Path.GetTempPath(), "blueice-winui-" + Guid.NewGuid().ToString("N"));
    public Func<JsonElement, Task>? Received { get; set; }
    public Action<string>? Failed { get; set; }

    public async Task StartAsync(string executable)
    {
        if (!File.Exists(executable)) throw new FileNotFoundException("BlueIce core was not found. Build it and place it beside the frontend.", executable);
        var start = new ProcessStartInfo(Path.GetFullPath(executable)) { UseShellExecute = false, CreateNoWindow = true, RedirectStandardInput = true, RedirectStandardOutput = true, RedirectStandardError = true };
        foreach (string argument in new[] { "--stdio", "--frame-dir", FrameDirectory }) start.ArgumentList.Add(argument);
        core = Process.Start(start) ?? throw new IOException("Could not start BlueIce core.");
        core.ErrorDataReceived += (_, args) => { if (args.Data is not null) Debug.WriteLine(args.Data); };
        core.BeginErrorReadLine();
        using var handshake = CancellationTokenSource.CreateLinkedTokenSource(lifetime.Token);
        handshake.CancelAfter(TimeSpan.FromSeconds(10));
        await BrowserWire.WriteAsync(core.StandardInput.BaseStream, BrowserWire.Command("Hello", new { protocol_version = BrowserWire.Version }), null, 1, handshake.Token);
        JsonElement hello = await BrowserWire.ReadAsync(core.StandardOutput.BaseStream, handshake.Token);
        if (!hello.GetProperty("message").TryGetProperty("Hello", out var fields) || fields.GetProperty("protocol_version").GetInt32() != BrowserWire.Version)
            throw new InvalidDataException("BlueIce core uses an incompatible browser protocol.");
        requestId = 1;
        reader = Task.Run(ReadMessagesAsync);
    }

    public async Task SendAsync(object message, ulong? tabId = null)
    {
        if (core is null || core.HasExited) throw new IOException("BlueIce core is not running.");
        await writer.WaitAsync(lifetime.Token).ConfigureAwait(false);
        try { await BrowserWire.WriteAsync(core.StandardInput.BaseStream, message, tabId, Interlocked.Increment(ref requestId), lifetime.Token).ConfigureAwait(false); }
        finally { writer.Release(); }
    }

    private async Task ReadMessagesAsync()
    {
        try
        {
            while (!lifetime.IsCancellationRequested)
            {
                JsonElement message = await BrowserWire.ReadAsync(core!.StandardOutput.BaseStream, lifetime.Token);
                if (Received is not null) await Received(message).WaitAsync(lifetime.Token);
            }
        }
        catch (OperationCanceledException) when (lifetime.IsCancellationRequested) { }
        catch (Exception error) { if (!lifetime.IsCancellationRequested) Failed?.Invoke("Core connection ended: " + error.Message); }
    }

    public async ValueTask DisposeAsync()
    {
        if (Interlocked.Exchange(ref disposed, 1) != 0) return;
        lifetime.Cancel();
        if (core is not null)
        {
            try
            {
                if (!core.HasExited)
                {
                    using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(2));
                    await writer.WaitAsync(timeout.Token).ConfigureAwait(false);
                    try { await BrowserWire.WriteAsync(core.StandardInput.BaseStream, "Shutdown", null, Interlocked.Increment(ref requestId), timeout.Token).ConfigureAwait(false); }
                    finally { writer.Release(); }
                    await core.WaitForExitAsync(timeout.Token).ConfigureAwait(false);
                }
            }
            catch (Exception error) when (error is IOException or OperationCanceledException or InvalidOperationException)
            { if (!core.HasExited) { core.Kill(true); await core.WaitForExitAsync().ConfigureAwait(false); } }
            finally { core.Dispose(); }
        }
        if (reader is not null) await reader.ConfigureAwait(false);
        try { if (Directory.Exists(FrameDirectory)) Directory.Delete(FrameDirectory, true); }
        catch (IOException) { }
        lifetime.Dispose();
    }
}
