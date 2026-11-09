using System.Net;
using Microsoft.AspNetCore.Mvc.Testing;

namespace SsrWasm.Tests;

public sealed class AppFactory : WebApplicationFactory<Program>
{
    public AppFactory()
    {
        _ = Paths.WasmPath; // sets the env vars Program reads
        _ = Paths.ClientDir;
        Environment.SetEnvironmentVariable("SSR_POOL_SIZE", "2");
    }
}

/// <summary>The middleware and the tag helper, end to end (the conformance suite covers the rest).</summary>
public sealed class PagesTests(AppFactory factory) : IClassFixture<AppFactory>
{
    [Fact]
    public async Task Middleware_renders_the_whole_document()
    {
        var res = await factory.CreateClient().GetAsync("/", TestContext.Current.CancellationToken);
        var body = await res.Content.ReadAsByteArrayAsync(TestContext.Current.CancellationToken);
        Assert.Equal(HttpStatusCode.OK, res.StatusCode);
        Assert.Matches("<html[^>]*data-stencil-build", System.Text.Encoding.UTF8.GetString(body));
        Assert.Equal(body.Length, res.Content.Headers.ContentLength); // fixed up for the rendered body
    }

    [Fact]
    public async Task Middleware_leaves_pages_that_used_the_helper_alone()
    {
        var html = await factory.CreateClient().GetStringAsync("/helper", TestContext.Current.CancellationToken);
        Assert.DoesNotMatch("<html[^>]*data-stencil-build", html);
        Assert.Equal(8, System.Text.RegularExpressions.Regex.Count(html, @"\ss-id=""\d+"""));
        Assert.DoesNotContain("<stencil>", html);
    }

    [Fact]
    public async Task Non_html_responses_pass_through()
    {
        var client = factory.CreateClient();
        Assert.Equal("ok", await client.GetStringAsync("/health", TestContext.Current.CancellationToken));
        var js = await client.GetAsync("/stencil/demo-components.esm.js", TestContext.Current.CancellationToken);
        Assert.Equal(HttpStatusCode.OK, js.StatusCode);
    }
}
