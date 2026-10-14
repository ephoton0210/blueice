// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

using System.IO.MemoryMappedFiles;

namespace BlueIce.WinUI.Protocol;

public static class FramePixels
{
    public static byte[] ReadBgra(string directory, string path, int width, int height)
    {
        string root = Path.GetFullPath(directory).TrimEnd(Path.DirectorySeparatorChar) + Path.DirectorySeparatorChar;
        if (!Path.GetFullPath(path).StartsWith(root, StringComparison.OrdinalIgnoreCase))
            throw new InvalidDataException("Frame is outside this browser session.");
        if (width < 1 || height < 1 || width > 4096 || height > 4096)
            throw new InvalidDataException("Unsupported frame dimensions.");
        int size = checked(width * height * 4);
        using var file = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.Read | FileShare.Delete);
        if (file.Length != size) throw new InvalidDataException("Frame dimensions do not match its pixel data.");
        using var map = MemoryMappedFile.CreateFromFile(file, null, 0, MemoryMappedFileAccess.Read, HandleInheritability.None, true);
        using var view = map.CreateViewAccessor(0, size, MemoryMappedFileAccess.Read);
        byte[] pixels = new byte[size];
        view.ReadArray(0, pixels, 0, size);
        for (int i = 0; i < size; i += 4) (pixels[i], pixels[i + 2]) = (pixels[i + 2], pixels[i]);
        return pixels;
    }
}
