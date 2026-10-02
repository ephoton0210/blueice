// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

using System.Buffers.Binary;
using System.Text.Json;

namespace BlueIce.WinUI.Protocol;

public static class BrowserWire
{
    public const int Version = 2;
    public const int MaxMessageBytes = 8 * 1024 * 1024;

    public static async Task<JsonElement> ReadAsync(Stream stream, CancellationToken cancellation = default)
    {
        byte[] prefix = new byte[4];
        await stream.ReadExactlyAsync(prefix, cancellation);
        uint length = BinaryPrimitives.ReadUInt32LittleEndian(prefix);
        if (length == 0 || length > MaxMessageBytes)
            throw new InvalidDataException("Invalid browser message length.");
        byte[] payload = new byte[length];
        await stream.ReadExactlyAsync(payload, cancellation);
        using JsonDocument document = JsonDocument.Parse(payload);
        return document.RootElement.Clone();
    }

    public static async Task WriteAsync(Stream stream, object message, ulong? tabId, long requestId, CancellationToken cancellation = default)
    {
        byte[] payload = JsonSerializer.SerializeToUtf8Bytes(new { request_id = requestId, tab_id = tabId, message });
        if (payload.Length > MaxMessageBytes) throw new InvalidDataException("Browser message is too large.");
        byte[] prefix = new byte[4];
        BinaryPrimitives.WriteUInt32LittleEndian(prefix, (uint)payload.Length);
        await stream.WriteAsync(prefix, cancellation);
        await stream.WriteAsync(payload, cancellation);
        await stream.FlushAsync(cancellation);
    }

    public static object Command(string name, object fields) => new Dictionary<string, object> { [name] = fields };
}
