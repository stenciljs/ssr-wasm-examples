using Microsoft.AspNetCore.Http.Features;
using Microsoft.Net.Http.Headers;

namespace SsrWasm.Ssr;

/// <summary>
/// Middleware mode: server-renders the Stencil components in every <c>text/html</c> response by
/// passing the whole document through renderToString (default options, fullDocument: true).
/// </summary>
/// <remarks>
/// <para>Which responses: every response whose Content-Type is <c>text/html</c>, isn't already
/// encoded and has a body - <b>except</b> responses that rendered fragments with the
/// <c>&lt;stencil&gt;</c> tag helper (helper mode). Rendering those again would render their
/// components twice. Everything else (static files, JSON, /health) streams through untouched,
/// and never touches the plugin.</para>
/// <para>It also releases the request's <see cref="SsrHandle"/> as soon as the endpoint has
/// finished, so a slow client doesn't keep a plugin instance out of the pool.</para>
/// </remarks>
public sealed class StencilSsrMiddleware(RequestDelegate next, ILogger<StencilSsrMiddleware> log)
{
    public async Task InvokeAsync(HttpContext context)
    {
        var originalBody = context.Features.Get<IHttpResponseBodyFeature>()!;
        var capture = new HtmlCaptureStream(context, originalBody.Stream);
        context.Features.Set<IHttpResponseBodyFeature>(new StreamResponseBodyFeature(capture, originalBody));

        string? html = null;
        SsrHandle? handle = null;
        try
        {
            await next(context);

            if (capture.Buffer is { } buffer)
            {
                html = System.Text.Encoding.UTF8.GetString(buffer.GetBuffer(), 0, (int)buffer.Length);
                handle = context.RequestServices.GetRequiredService<SsrHandle>();
                if (handle.RenderedFragments)
                {
                    log.LogDebug("ssr middleware: {Path} used the <stencil> helper, not rendering it again", context.Request.Path);
                }
                else
                {
                    html = await handle.RenderAsync(html);
                }
            }
        }
        finally
        {
            context.Features.Set(originalBody);
            // The endpoint is done with the plugin; give the instance back before writing.
            context.RequestServices.GetService<SsrHandle>()?.Release();
        }

        if (html is not null)
        {
            var bytes = System.Text.Encoding.UTF8.GetBytes(html);
            context.Response.ContentLength = bytes.Length;
            await originalBody.Stream.WriteAsync(bytes, context.RequestAborted);
        }
    }
}

/// <summary>
/// Decides on the first write (or flush), when the handler has set its headers, whether to hold
/// the body back for rendering (an HTML response) or pass it straight through.
/// </summary>
internal sealed class HtmlCaptureStream(HttpContext context, Stream inner) : Stream
{
    private bool _decided;

    /// <summary>The buffered body, if this is an HTML response that's being held back.</summary>
    public MemoryStream? Buffer { get; private set; }

    private Stream Target
    {
        get
        {
            if (!_decided)
            {
                _decided = true;
                if (ShouldBuffer(context.Response))
                {
                    Buffer = new MemoryStream();
                    // The rendered body has a different length.
                    context.Response.ContentLength = null;
                }
            }
            return Buffer ?? inner;
        }
    }

    private static bool ShouldBuffer(HttpResponse response) =>
        MediaTypeHeaderValue.TryParse(response.ContentType, out var type)
        && type.MediaType.Equals("text/html", StringComparison.OrdinalIgnoreCase)
        && (!type.Charset.HasValue || type.Charset.Equals("utf-8", StringComparison.OrdinalIgnoreCase))
        && !response.Headers.ContainsKey(HeaderNames.ContentEncoding)
        && response.StatusCode is not (StatusCodes.Status204NoContent or StatusCodes.Status304NotModified)
        && !response.HasStarted;

    public override void Write(byte[] buffer, int offset, int count) => Target.Write(buffer, offset, count);
    public override void Write(ReadOnlySpan<byte> buffer) => Target.Write(buffer);
    public override Task WriteAsync(byte[] buffer, int offset, int count, CancellationToken ct) => Target.WriteAsync(buffer, offset, count, ct);
    public override ValueTask WriteAsync(ReadOnlyMemory<byte> buffer, CancellationToken ct = default) => Target.WriteAsync(buffer, ct);

    // Flushing is a no-op while buffering: the whole document is needed before it can be rendered.
    public override void Flush() => Target.Flush();
    public override Task FlushAsync(CancellationToken ct) => Target.FlushAsync(ct);

    public override bool CanRead => false;
    public override bool CanSeek => false;
    public override bool CanWrite => true;
    public override long Length => throw new NotSupportedException();
    public override long Position { get => throw new NotSupportedException(); set => throw new NotSupportedException(); }
    public override int Read(byte[] buffer, int offset, int count) => throw new NotSupportedException();
    public override long Seek(long offset, SeekOrigin origin) => throw new NotSupportedException();
    public override void SetLength(long value) => throw new NotSupportedException();
}
