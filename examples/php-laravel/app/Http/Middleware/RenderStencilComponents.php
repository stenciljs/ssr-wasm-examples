<?php

namespace App\Http\Middleware;

use App\Ssr\StencilRenderer;
use Closure;
use Illuminate\Http\Request;
use Symfony\Component\HttpFoundation\BinaryFileResponse;
use Symfony\Component\HttpFoundation\Response;
use Symfony\Component\HttpFoundation\StreamedResponse;

/**
 * Middleware mode: post-process the whole HTML response through Stencil's renderToString
 * (a full document, default options). Non-HTML, streamed and file responses pass through.
 */
class RenderStencilComponents
{
    public function __construct(private readonly StencilRenderer $renderer) {}

    public function handle(Request $request, Closure $next): Response
    {
        $response = $next($request);

        if ($response instanceof StreamedResponse
            || $response instanceof BinaryFileResponse
            || ! str_starts_with((string) $response->headers->get('Content-Type'), 'text/html')) {
            return $response;
        }

        $content = $response->getContent();
        if ($content !== false && $content !== '') {
            $response->setContent($this->renderer->render($content));
        }

        return $response;
    }
}
