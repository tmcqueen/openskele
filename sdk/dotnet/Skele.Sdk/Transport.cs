using System.Text;

namespace Skele.Sdk;

/// <summary>LSP-style framing: <c>Content-Length: N\r\n\r\n</c> followed by N bytes of JSON.</summary>
internal sealed class Transport(Stream input, Stream output)
{
    private readonly BufferedStream _input = new(input);
    private readonly SemaphoreSlim _writing = new(1, 1);

    /// <summary>Reads one frame, or returns null when the stream has ended.</summary>
    public async Task<byte[]?> ReadAsync(CancellationToken cancellation)
    {
        int? length = null;
        while (true)
        {
            var header = ReadHeaderLine();
            if (header is null)
            {
                return null;
            }

            if (header.Length == 0)
            {
                if (length is not { } size)
                {
                    continue;
                }

                var body = new byte[size];
                await _input.ReadExactlyAsync(body, cancellation);
                return body;
            }

            var colon = header.IndexOf(':');
            if (colon > 0
                && header.AsSpan(0, colon).Trim().Equals("content-length", StringComparison.OrdinalIgnoreCase)
                && int.TryParse(header.AsSpan(colon + 1).Trim(), out var parsed))
            {
                length = parsed;
            }
        }
    }

    public async Task WriteAsync(ReadOnlyMemory<byte> body, CancellationToken cancellation)
    {
        // One frame at a time: handlers run concurrently and must not interleave.
        await _writing.WaitAsync(cancellation);
        try
        {
            await output.WriteAsync(Encoding.ASCII.GetBytes($"Content-Length: {body.Length}\r\n\r\n"), cancellation);
            await output.WriteAsync(body, cancellation);
            await output.FlushAsync(cancellation);
        }
        finally
        {
            _writing.Release();
        }
    }

    private string? ReadHeaderLine()
    {
        var line = new StringBuilder();
        while (true)
        {
            var next = _input.ReadByte();
            if (next < 0)
            {
                return null;
            }

            if (next == '\n')
            {
                return line.ToString().TrimEnd('\r');
            }

            line.Append((char)next);
        }
    }
}
