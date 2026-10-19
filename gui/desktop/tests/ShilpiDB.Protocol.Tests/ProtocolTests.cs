using System.Buffers.Binary;
using Xunit;

namespace ShilpiDB.Protocol.Tests;

public class ProtocolTests
{
    [Fact]
    public void Requests_round_trip_on_the_wire_bytes()
    {
        ShilpiRequest[] requests =
        [
            new ShilpiRequest.Ping(),
            new ShilpiRequest.Insert(7, 0, 0, 1, 1, [1, 2, 3]),
            new ShilpiRequest.Get(7),
            new ShilpiRequest.Delete(7),
            new ShilpiRequest.QueryBbox(-1, -1, 1, 1),
            new ShilpiRequest.QueryPoint(1, 2, 0.5),
            new ShilpiRequest.List(),
            new ShilpiRequest.Stats(),
            new ShilpiRequest.Save(),
            new ShilpiRequest.ListBoxes(),
            new ShilpiRequest.InsertMany(
            [
                (1, 0, 0, 1, 1, [1, 2]),
                (2, 5, 5, 6, 6, []),
            ]),
        ];

        foreach (var request in requests)
        {
            var bytes = WireFormat.EncodeRequest(request);
            var again = WireFormat.EncodeRequest(WireFormat.DecodeRequest(bytes));
            Assert.Equal(bytes, again);
        }
    }

    [Fact]
    public void Responses_round_trip_on_the_wire_bytes()
    {
        ShilpiResponse[] responses =
        [
            new ShilpiResponse.Pong(),
            new ShilpiResponse.Ok(),
            new ShilpiResponse.Err("hÉllo"),
            new ShilpiResponse.Found(0, 0, 2, 2, [9, 9]),
            new ShilpiResponse.NotFound(),
            new ShilpiResponse.Deleted(true),
            new ShilpiResponse.Deleted(false),
            new ShilpiResponse.Ids([1, 2, 3]),
            new ShilpiResponse.Stats(42, 1.5),
            new ShilpiResponse.Records(
            [
                new RecordBox(1, 0, 0, 1, 1),
                new RecordBox(2, 5, 5, 6, 6),
            ]),
        ];

        foreach (var response in responses)
        {
            var bytes = WireFormat.EncodeResponse(response);
            var again = WireFormat.EncodeResponse(WireFormat.DecodeResponse(bytes));
            Assert.Equal(bytes, again);
        }
    }

    [Fact]
    public void Insert_layout_matches_the_codec()
    {
        var bytes = WireFormat.EncodeRequest(new ShilpiRequest.Insert(1, 0, 0, 1, 1, "hello"u8.ToArray()));
        var expected = new List<byte> { 1 };
        expected.AddRange(new byte[] { 1, 0, 0, 0, 0, 0, 0, 0 });
        expected.AddRange(new byte[16]);
        expected.AddRange(new byte[] { 0, 0, 0, 0, 0, 0, 0xF0, 0x3F });
        expected.AddRange(new byte[] { 0, 0, 0, 0, 0, 0, 0xF0, 0x3F });
        expected.AddRange(new byte[] { 5, 0, 0, 0 });
        expected.AddRange("hello"u8.ToArray());
        Assert.Equal(expected, bytes);
    }

    [Fact]
    public void Framing_round_trips_and_clean_eof_is_null()
    {
        var body = WireFormat.EncodeRequest(new ShilpiRequest.Ping());
        using var buffer = new MemoryStream();
        Frame.Write(buffer, body);
        buffer.Position = 0;
        Assert.Equal(body, Frame.Read(buffer));
        Assert.Null(Frame.Read(buffer));
    }

    [Fact]
    public void Oversized_length_prefix_is_rejected()
    {
        using var buffer = new MemoryStream();
        Span<byte> length = stackalloc byte[4];
        BinaryPrimitives.WriteUInt32LittleEndian(length, WireFormat.MaxMessageLength + 1);
        buffer.Write(length);
        buffer.Position = 0;
        var ex = Assert.Throws<ProtocolException>(() => Frame.Read(buffer));
        Assert.Contains("exceeds max", ex.Message);
    }

    [Fact]
    public void Unknown_opcode_and_empty_message_are_protocol_errors()
    {
        Assert.Contains("unknown opcode 255", Assert.Throws<ProtocolException>(() => WireFormat.DecodeRequest([255])).Message);
        Assert.Contains("empty message", Assert.Throws<ProtocolException>(() => WireFormat.DecodeRequest([])).Message);
        Assert.Throws<ProtocolException>(() => WireFormat.DecodeRequest([1]));
    }

    [Fact]
    public void Endpoint_requires_host_and_port()
    {
        Assert.Equal(("127.0.0.1", 7420), EndpointAddress.Parse("127.0.0.1:7420"));
        Assert.Equal(("::1", 7420), EndpointAddress.Parse("[::1]:7420"));
        Assert.Throws<ProtocolException>(() => EndpointAddress.Parse("localhost"));
    }

    [Fact]
    public async Task Talks_to_a_local_shilpid_when_the_binary_is_present()
    {
        var located = ShilpidLocator.Find();
        if (located is null)
        {
            if (Environment.GetEnvironmentVariable("SHILPID_PATH") is { Length: > 0 } configured)
                throw new InvalidOperationException($"SHILPID_PATH does not point at shilpid and no other binary was found: {configured}");
            return;
        }

        var dataPath = Path.Combine(Path.GetTempPath(), $"shilpidb-csharp-{Guid.NewGuid():N}.vdb");
        using var host = new ShilpidHost();
        try
        {
            var address = await host.StartAsync(dataPath);
            using var client = new ShilpiClient();
            await client.ConnectAsync(address);
            await client.InsertAsync(1, 0, 0, 10, 10, "hello"u8.ToArray());

            var boxes = await client.ListBoxesAsync();
            var box = Assert.Single(boxes);
            Assert.Equal(1ul, box.Id);
            Assert.Equal(10, box.MaxX);

            var hits = await client.QueryBboxAsync(-1, -1, 1, 1);
            Assert.Equal(new ulong[] { 1 }, hits);
            Assert.Empty(await client.QueryBboxAsync(100, 100, 110, 110));

            var detail = await client.GetAsync(1);
            Assert.NotNull(detail);
            Assert.Equal("hello", detail!.PayloadText);
            Assert.Equal("68656c6c6f", detail.PayloadHex);

            var stats = await client.StatsAsync();
            Assert.Equal(1u, stats.Count);
            await client.SaveAsync();
            await client.DisconnectAsync();
            host.Stop();

            address = await host.StartAsync(dataPath);
            await client.ConnectAsync(address);
            var persisted = await client.GetAsync(1);
            Assert.NotNull(persisted);
            Assert.Equal("hello", persisted!.PayloadText);
            Assert.True(await client.DeleteAsync(1));
            await client.SaveAsync();
        }
        finally
        {
            host.Stop();
            try { File.Delete(dataPath); } catch (IOException) { }
        }
    }
}
