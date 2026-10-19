using System.Buffers.Binary;
using System.Text;

namespace ShilpiDB.Protocol;

/// <summary>
/// Little-endian codec shared with <c>shilpidb</c>'s <c>Writer</c>/<c>Reader</c>.
/// Integers and floats are fixed width; strings are a <c>u32</c> byte length plus UTF-8.
/// </summary>
public sealed class CodecWriter
{
    private readonly MemoryStream _buf = new();

    public void U8(byte value) => _buf.WriteByte(value);

    public void Bool(bool value) => U8(value ? (byte)1 : (byte)0);

    public void U32(uint value)
    {
        Span<byte> bytes = stackalloc byte[4];
        BinaryPrimitives.WriteUInt32LittleEndian(bytes, value);
        Raw(bytes);
    }

    public void U64(ulong value)
    {
        Span<byte> bytes = stackalloc byte[8];
        BinaryPrimitives.WriteUInt64LittleEndian(bytes, value);
        Raw(bytes);
    }

    public void F64(double value)
    {
        Span<byte> bytes = stackalloc byte[8];
        BinaryPrimitives.WriteDoubleLittleEndian(bytes, value);
        Raw(bytes);
    }

    public void Str(string value)
    {
        var bytes = Encoding.UTF8.GetBytes(value);
        U32((uint)bytes.Length);
        Raw(bytes);
    }

    public void Raw(ReadOnlySpan<byte> bytes) => _buf.Write(bytes);

    public byte[] ToArray() => _buf.ToArray();
}

public sealed class CodecReader
{
    private static readonly Encoding Utf8 = new UTF8Encoding(encoderShouldEmitUTF8Identifier: false, throwOnInvalidBytes: true);
    private readonly byte[] _data;
    private int _pos;

    public CodecReader(byte[] data) => _data = data;

    public byte U8() => Take(1)[0];

    public bool Bool() => U8() != 0;

    public uint U32() => BinaryPrimitives.ReadUInt32LittleEndian(Take(4));

    public ulong U64() => BinaryPrimitives.ReadUInt64LittleEndian(Take(8));

    public double F64() => BinaryPrimitives.ReadDoubleLittleEndian(Take(8));

    public string Str()
    {
        var bytes = Raw(checked((int)U32()));
        try
        {
            return Utf8.GetString(bytes);
        }
        catch (DecoderFallbackException)
        {
            throw new ProtocolException("invalid utf-8 string");
        }
    }

    public byte[] Raw(int count) => Take(count).ToArray();

    private ReadOnlySpan<byte> Take(int count)
    {
        if (count < 0 || _pos > _data.Length - count)
            throw new ProtocolException("unexpected end of data");
        var slice = _data.AsSpan(_pos, count);
        _pos += count;
        return slice;
    }
}

/// <summary>
/// A message on the wire is a <c>u32</c> little-endian length followed by that many bytes.
/// </summary>
public static class Frame
{
    public static void Write(Stream stream, ReadOnlySpan<byte> body)
    {
        if ((uint)body.Length > WireFormat.MaxMessageLength)
        {
            throw new ProtocolException(
                $"message length {body.Length} exceeds max {WireFormat.MaxMessageLength}");
        }

        Span<byte> length = stackalloc byte[4];
        BinaryPrimitives.WriteUInt32LittleEndian(length, (uint)body.Length);
        stream.Write(length);
        stream.Write(body);
        stream.Flush();
    }

    /// <summary>
    /// Returns <c>null</c> on a clean end of stream before a new frame, matching
    /// <c>read_message</c> in <c>shilpi-protocol</c>.
    /// </summary>
    public static byte[]? Read(Stream stream)
    {
        var lengthBytes = new byte[4];
        var read = ReadChunk(stream, lengthBytes, allowEmpty: true);
        if (read == 0)
            return null;

        var length = BinaryPrimitives.ReadUInt32LittleEndian(lengthBytes);
        if (length > WireFormat.MaxMessageLength)
        {
            throw new ProtocolException(
                $"message length {length} exceeds max {WireFormat.MaxMessageLength}");
        }

        var body = new byte[length];
        ReadChunk(stream, body, allowEmpty: false);
        return body;
    }

    private static int ReadChunk(Stream stream, byte[] buffer, bool allowEmpty)
    {
        var offset = 0;
        while (offset < buffer.Length)
        {
            var n = stream.Read(buffer, offset, buffer.Length - offset);
            if (n == 0)
            {
                if (offset == 0 && allowEmpty)
                    return 0;
                throw new EndOfStreamException("connection closed mid-frame");
            }

            offset += n;
        }

        return offset;
    }
}
