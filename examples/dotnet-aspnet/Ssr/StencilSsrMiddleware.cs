using System.Text;

namespace SsrWasm.Ssr;

/// <summary>
/// Middleware mode: buffers the response and, if it's <c>text/html</c>, passes the whole document
/// through renderToString. Pages that used the <c>&lt;stencil&gt;</c> tag helper are skipped, so
/// their components aren't rendered twice. Anything else is copied through unchanged.
/// </summary>
public sealed class StencilSsrMiddleware(RequestDelegate next)
{
    public async Task InvokeAsync(HttpContext context)
    {
        var originalBody = context.Response.Body;
        using var buffer = new MemoryStream();
        context.Response.Body = buffer;
        try
        {
            await next(context);
        }
        finally
        {
            context.Response.Body = originalBody;
        }

        var ssr = context.RequestServices.GetRequiredService<SsrHandle>();
        if (context.Response.ContentType?.StartsWith("text/html", StringComparison.OrdinalIgnoreCase) == true
            && !ssr.RenderedFragments)
        {
            var html = Encoding.UTF8.GetString(buffer.GetBuffer(), 0, (int)buffer.Length);
            var bytes = Encoding.UTF8.GetBytes(await ssr.RenderAsync(html));
            context.Response.ContentLength = bytes.Length;
            await originalBody.WriteAsync(bytes, context.RequestAborted);
        }
        else
        {
            buffer.Position = 0;
            await buffer.CopyToAsync(originalBody, context.RequestAborted);
        }
    }
}
