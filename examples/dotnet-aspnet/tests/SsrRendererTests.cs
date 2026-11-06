using System.Text;
using System.Text.RegularExpressions;
using Microsoft.AspNetCore.Http;
using Microsoft.Extensions.Logging.Abstractions;
using SsrWasm.Ssr;

namespace SsrWasm.Tests;

/// <summary>One renderer for the whole class: compiling the module takes seconds without a warm cache.</summary>
public sealed class RendererFixture : IDisposable
{
    public SsrRenderer Renderer { get; } = new(new SsrConfig { WasmPath = Paths.WasmPath, PoolSize = 2 }, NullLogger<SsrRenderer>.Instance);
    public void Dispose() => Renderer.Dispose();
}

public sealed partial class SsrRendererTests(RendererFixture fixture) : IClassFixture<RendererFixture>
{
    private SsrRenderer Renderer => fixture.Renderer;

    [GeneratedRegex(@"\ss-id=""(\d+)""")]
    private static partial Regex SId();

    private static string[] Ids(string html) => SId().Matches(html).Select(m => m.Groups[1].Value).ToArray();

    [Fact]
    public async Task Fragment_render_produces_declarative_shadow_dom_without_a_document()
    {
        using var h = Renderer.CreateHandle();
        var html = await h.RenderAsync("""<demo-counter start="5" step="2"></demo-counter>""", SsrOptions.Fragment);
        Assert.Contains("""<template shadowrootmode="open">""", html);
        Assert.DoesNotContain("<html", html);
        Assert.True(h.RenderedFragments);
        Assert.True(h.Elapsed > TimeSpan.Zero);
    }

    // Fragment ids are reset once per handle, before the first fragment: fragments of one request
    // continue the sequence, and the next request starts from 1 again.
    [Fact]
    public async Task Fragment_ids_reset_once_per_handle()
    {
        const string badge = """<demo-badge tone="success">SSR</demo-badge>""";
        for (var i = 0; i < 2; i++)
        {
            using var h = Renderer.CreateHandle();
            var a = Ids(await h.RenderAsync(badge, SsrOptions.Fragment));
            var b = Ids(await h.RenderAsync(badge, SsrOptions.Fragment));
            Assert.Equal(["1"], a);
            Assert.Equal(["2"], b);
        }
    }

    // Full-document renders number their ids from 1 by themselves, without any reset.
    [Fact]
    public async Task Document_ids_start_from_one()
    {
        const string page = "<html><body><demo-badge>a</demo-badge><demo-badge>b</demo-badge></body></html>";
        using var h = Renderer.CreateHandle();
        for (var i = 0; i < 3; i++)
        {
            Assert.Equal(["1", "2"], Ids(await h.RenderAsync(page)));
        }
        Assert.False(h.RenderedFragments);
    }

    [Fact]
    public async Task Waiting_for_an_instance_gives_up_when_the_request_is_aborted()
    {
        using var a = Renderer.CreateHandle();
        using var b = Renderer.CreateHandle();
        await a.RenderAsync("<demo-badge>a</demo-badge>", SsrOptions.Fragment); // takes both instances
        await b.RenderAsync("<demo-badge>b</demo-badge>", SsrOptions.Fragment);

        using var cts = new CancellationTokenSource(TimeSpan.FromMilliseconds(50));
        var context = new DefaultHttpContext { RequestAborted = cts.Token };
        using var c = Renderer.CreateHandle(context);
        const string input = "<demo-badge>c</demo-badge>";
        Assert.Equal(input, await c.RenderAsync(input, SsrOptions.Fragment)); // unrendered fallback
    }

    [Fact]
    public async Task Released_handle_returns_its_input_and_the_instance_is_reused()
    {
        var h = Renderer.CreateHandle();
        await h.RenderAsync("<demo-badge>a</demo-badge>");
        h.Release();
        h.Release(); // idempotent
        Assert.Equal("<demo-badge>x</demo-badge>", await h.RenderAsync("<demo-badge>x</demo-badge>"));

        // Both instances are back in the pool: two handles can render at once.
        using var a = Renderer.CreateHandle();
        using var b = Renderer.CreateHandle();
        Assert.Contains("s-id=", await a.RenderAsync("<demo-badge>a</demo-badge>", SsrOptions.Fragment));
        Assert.Contains("s-id=", await b.RenderAsync("<demo-badge>b</demo-badge>", SsrOptions.Fragment));
    }

    [Theory]
    [InlineData("""{"__error":"ReferenceError: x is not defined"}""")]
    [InlineData("""{"__error":{"message":"x"}}""")]
    [InlineData("""{}""")]
    [InlineData("""[]""")]
    public void ParseResult_rejects_errors_and_malformed_results(string json)
    {
        Assert.Throws<SsrException>(() => SsrRenderer.ParseResult(Encoding.UTF8.GetBytes(json)));
    }

    [Fact]
    public void ParseResult_returns_the_html()
    {
        Assert.Equal("<p>ok</p>", SsrRenderer.ParseResult("""{"html":"<p>ok</p>","hydratedCount":0}"""u8));
    }
}
