using System.Text;
using System.Text.RegularExpressions;
using Microsoft.Extensions.Logging.Abstractions;
using SsrWasm.Ssr;

namespace SsrWasm.Tests;

/// <summary>One renderer for the whole class: compiling the module takes seconds without a warm cache.</summary>
public sealed class RendererFixture : IDisposable
{
    public SsrRenderer Renderer { get; } = new(Paths.WasmPath, 2, NullLogger<SsrRenderer>.Instance);
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
        var html = await h.RenderAsync("""<demo-counter start="5" step="2"></demo-counter>""", fragment: true);
        Assert.Contains("""<template shadowrootmode="open">""", html);
        Assert.DoesNotContain("<html", html);
        Assert.True(h.RenderedFragments);
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
            var a = Ids(await h.RenderAsync(badge, fragment: true));
            var b = Ids(await h.RenderAsync(badge, fragment: true));
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
        await a.RenderAsync("<demo-badge>a</demo-badge>", fragment: true); // takes both instances
        await b.RenderAsync("<demo-badge>b</demo-badge>", fragment: true);

        using var cts = new CancellationTokenSource(TimeSpan.FromMilliseconds(50));
        using var c = Renderer.CreateHandle(cts.Token);
        const string input = "<demo-badge>c</demo-badge>";
        Assert.Equal(input, await c.RenderAsync(input, fragment: true)); // unrendered fallback
    }

    [Fact]
    public async Task Disposing_a_handle_returns_its_instance_to_the_pool()
    {
        for (var i = 0; i < 3; i++) // more handles in turn than the pool has instances
        {
            using var h = Renderer.CreateHandle(new CancellationTokenSource(TimeSpan.FromSeconds(10)).Token);
            Assert.Contains("s-id=", await h.RenderAsync("<demo-badge>a</demo-badge>", fragment: true));
        }
    }

    [Theory]
    [InlineData("""{"__error":"ReferenceError: x is not defined"}""")]
    [InlineData("""{"__error":{"message":"x"}}""")]
    [InlineData("""{}""")]
    [InlineData("""[]""")]
    public void ParseResult_rejects_errors_and_malformed_results(string json)
    {
        Assert.ThrowsAny<Exception>(() => SsrRenderer.ParseResult(Encoding.UTF8.GetBytes(json)));
    }

    [Fact]
    public void ParseResult_returns_the_html()
    {
        Assert.Equal("<p>ok</p>", SsrRenderer.ParseResult("""{"html":"<p>ok</p>","hydratedCount":0}"""u8));
    }
}
