namespace ShilpiDB.Protocol;

public readonly record struct RecordBox(ulong Id, double MinX, double MinY, double MaxX, double MaxY);

public sealed record RecordDetail(
    ulong Id,
    double MinX,
    double MinY,
    double MaxX,
    double MaxY,
    string PayloadText,
    string PayloadHex);

public readonly record struct DatabaseStats(uint Count, double CellSize);

public abstract record ShilpiRequest
{
    public sealed record Ping : ShilpiRequest;
    public sealed record Insert(ulong Id, double MinX, double MinY, double MaxX, double MaxY, byte[] Payload) : ShilpiRequest;
    public sealed record InsertMany(IReadOnlyList<(ulong Id, double MinX, double MinY, double MaxX, double MaxY, byte[] Payload)> Records) : ShilpiRequest;
    public sealed record Get(ulong Id) : ShilpiRequest;
    public sealed record Delete(ulong Id) : ShilpiRequest;
    public sealed record QueryBbox(double MinX, double MinY, double MaxX, double MaxY) : ShilpiRequest;
    public sealed record QueryPoint(double X, double Y, double Tol) : ShilpiRequest;
    public sealed record List : ShilpiRequest;
    public sealed record Stats : ShilpiRequest;
    public sealed record Save : ShilpiRequest;
    public sealed record ListBoxes : ShilpiRequest;
}

public abstract record ShilpiResponse
{
    public sealed record Pong : ShilpiResponse;
    public sealed record Ok : ShilpiResponse;
    public sealed record Err(string Message) : ShilpiResponse;
    public sealed record Found(double MinX, double MinY, double MaxX, double MaxY, byte[] Payload) : ShilpiResponse;
    public sealed record NotFound : ShilpiResponse;
    public sealed record Deleted(bool Existed) : ShilpiResponse;
    public sealed record Ids(IReadOnlyList<ulong> Values) : ShilpiResponse;
    public sealed record Stats(uint Count, double CellSize) : ShilpiResponse;
    public sealed record Records(IReadOnlyList<RecordBox> Boxes) : ShilpiResponse;
}

/// <summary>
/// Opcode layout of <c>shilpi-protocol</c>. Bodies have no length prefix; <see cref="Frame"/> adds that.
/// </summary>
public static class WireFormat
{
    public const uint MaxMessageLength = 64 * 1024 * 1024;

    private const byte OpPing = 0;
    private const byte OpInsert = 1;
    private const byte OpGet = 2;
    private const byte OpDelete = 3;
    private const byte OpQueryBbox = 4;
    private const byte OpQueryPoint = 5;
    private const byte OpList = 6;
    private const byte OpStats = 7;
    private const byte OpSave = 8;
    private const byte OpListBoxes = 9;
    private const byte OpInsertMany = 10;

    private const byte OpPong = 64;
    private const byte OpOk = 65;
    private const byte OpErr = 66;
    private const byte OpFound = 67;
    private const byte OpNotFound = 68;
    private const byte OpDeleted = 69;
    private const byte OpIds = 70;
    private const byte OpStatsReply = 71;
    private const byte OpRecordsReply = 72;

    public static byte[] EncodeRequest(ShilpiRequest request)
    {
        var w = new CodecWriter();
        switch (request)
        {
            case ShilpiRequest.Ping:
                w.U8(OpPing);
                break;
            case ShilpiRequest.Insert insert:
                w.U8(OpInsert);
                w.U64(insert.Id);
                WriteBox(w, insert.MinX, insert.MinY, insert.MaxX, insert.MaxY);
                WritePayload(w, insert.Payload);
                break;
            case ShilpiRequest.InsertMany many:
                w.U8(OpInsertMany);
                w.U32((uint)many.Records.Count);
                foreach (var (id, minX, minY, maxX, maxY, payload) in many.Records)
                {
                    w.U64(id);
                    WriteBox(w, minX, minY, maxX, maxY);
                    WritePayload(w, payload);
                }
                break;
            case ShilpiRequest.Get get:
                w.U8(OpGet);
                w.U64(get.Id);
                break;
            case ShilpiRequest.Delete delete:
                w.U8(OpDelete);
                w.U64(delete.Id);
                break;
            case ShilpiRequest.QueryBbox query:
                w.U8(OpQueryBbox);
                WriteBox(w, query.MinX, query.MinY, query.MaxX, query.MaxY);
                break;
            case ShilpiRequest.QueryPoint point:
                w.U8(OpQueryPoint);
                w.F64(point.X);
                w.F64(point.Y);
                w.F64(point.Tol);
                break;
            case ShilpiRequest.List:
                w.U8(OpList);
                break;
            case ShilpiRequest.Stats:
                w.U8(OpStats);
                break;
            case ShilpiRequest.Save:
                w.U8(OpSave);
                break;
            case ShilpiRequest.ListBoxes:
                w.U8(OpListBoxes);
                break;
            default:
                throw new ProtocolException($"unsupported request {request.GetType().Name}");
        }

        return w.ToArray();
    }

    public static ShilpiRequest DecodeRequest(byte[] bytes)
    {
        if (bytes.Length == 0)
            throw new ProtocolException("shilpi-protocol: empty message");

        var r = new CodecReader(bytes);
        return r.U8() switch
        {
            OpPing => new ShilpiRequest.Ping(),
            OpInsert => new ShilpiRequest.Insert(r.U64(), r.F64(), r.F64(), r.F64(), r.F64(), ReadPayload(r)),
            OpInsertMany => ReadInsertMany(r),
            OpGet => new ShilpiRequest.Get(r.U64()),
            OpDelete => new ShilpiRequest.Delete(r.U64()),
            OpQueryBbox => new ShilpiRequest.QueryBbox(r.F64(), r.F64(), r.F64(), r.F64()),
            OpQueryPoint => new ShilpiRequest.QueryPoint(r.F64(), r.F64(), r.F64()),
            OpList => new ShilpiRequest.List(),
            OpStats => new ShilpiRequest.Stats(),
            OpSave => new ShilpiRequest.Save(),
            OpListBoxes => new ShilpiRequest.ListBoxes(),
            var op => throw new ProtocolException($"shilpi-protocol: unknown opcode {op}"),
        };
    }

    public static byte[] EncodeResponse(ShilpiResponse response)
    {
        var w = new CodecWriter();
        switch (response)
        {
            case ShilpiResponse.Pong:
                w.U8(OpPong);
                break;
            case ShilpiResponse.Ok:
                w.U8(OpOk);
                break;
            case ShilpiResponse.Err err:
                w.U8(OpErr);
                w.Str(err.Message);
                break;
            case ShilpiResponse.Found found:
                w.U8(OpFound);
                WriteBox(w, found.MinX, found.MinY, found.MaxX, found.MaxY);
                WritePayload(w, found.Payload);
                break;
            case ShilpiResponse.NotFound:
                w.U8(OpNotFound);
                break;
            case ShilpiResponse.Deleted deleted:
                w.U8(OpDeleted);
                w.Bool(deleted.Existed);
                break;
            case ShilpiResponse.Ids ids:
                w.U8(OpIds);
                w.U32((uint)ids.Values.Count);
                foreach (var id in ids.Values)
                    w.U64(id);
                break;
            case ShilpiResponse.Stats stats:
                w.U8(OpStatsReply);
                w.U32(stats.Count);
                w.F64(stats.CellSize);
                break;
            case ShilpiResponse.Records records:
                w.U8(OpRecordsReply);
                w.U32((uint)records.Boxes.Count);
                foreach (var box in records.Boxes)
                {
                    w.U64(box.Id);
                    WriteBox(w, box.MinX, box.MinY, box.MaxX, box.MaxY);
                }
                break;
            default:
                throw new ProtocolException($"unsupported response {response.GetType().Name}");
        }

        return w.ToArray();
    }

    public static ShilpiResponse DecodeResponse(byte[] bytes)
    {
        if (bytes.Length == 0)
            throw new ProtocolException("shilpi-protocol: empty message");

        var r = new CodecReader(bytes);
        return r.U8() switch
        {
            OpPong => new ShilpiResponse.Pong(),
            OpOk => new ShilpiResponse.Ok(),
            OpErr => new ShilpiResponse.Err(r.Str()),
            OpFound => new ShilpiResponse.Found(r.F64(), r.F64(), r.F64(), r.F64(), ReadPayload(r)),
            OpNotFound => new ShilpiResponse.NotFound(),
            OpDeleted => new ShilpiResponse.Deleted(r.Bool()),
            OpIds => new ShilpiResponse.Ids(ReadIds(r)),
            OpStatsReply => new ShilpiResponse.Stats(r.U32(), r.F64()),
            OpRecordsReply => new ShilpiResponse.Records(ReadBoxes(r)),
            var op => throw new ProtocolException($"shilpi-protocol: unknown opcode {op}"),
        };
    }

    private static ShilpiRequest.InsertMany ReadInsertMany(CodecReader r)
    {
        var count = Count(r.U32());
        var records = new List<(ulong, double, double, double, double, byte[])>(Math.Min(count, 64));
        for (var i = 0; i < count; i++)
            records.Add((r.U64(), r.F64(), r.F64(), r.F64(), r.F64(), ReadPayload(r)));
        return new ShilpiRequest.InsertMany(records);
    }

    private static List<ulong> ReadIds(CodecReader r)
    {
        var count = Count(r.U32());
        var ids = new List<ulong>(Math.Min(count, 64));
        for (var i = 0; i < count; i++)
            ids.Add(r.U64());
        return ids;
    }

    private static List<RecordBox> ReadBoxes(CodecReader r)
    {
        var count = Count(r.U32());
        var boxes = new List<RecordBox>(Math.Min(count, 64));
        for (var i = 0; i < count; i++)
            boxes.Add(new RecordBox(r.U64(), r.F64(), r.F64(), r.F64(), r.F64()));
        return boxes;
    }

    private static int Count(uint n)
    {
        if (n > MaxMessageLength)
            throw new ProtocolException($"count {n} exceeds max message length");
        return (int)n;
    }

    private static void WriteBox(CodecWriter w, double minX, double minY, double maxX, double maxY)
    {
        w.F64(minX);
        w.F64(minY);
        w.F64(maxX);
        w.F64(maxY);
    }

    private static void WritePayload(CodecWriter w, byte[] payload)
    {
        w.U32((uint)payload.Length);
        w.Raw(payload);
    }

    private static byte[] ReadPayload(CodecReader r) => r.Raw(checked((int)r.U32()));
}
