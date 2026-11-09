<?php

namespace App\Http\Middleware;

use App\Ssr\StencilRenderer;
use Closure;
use Illuminate\Http\Request;
use Symfony\Component\HttpFoundation\Response;

/**
 * Middleware mode: render the finished text/html response as a full document.
 *
 * Attach it only to routes whose views don't use `@stencil`, so nothing is rendered twice.
 */
class RenderStencilComponents
{
    public function __construct(private readonly StencilRenderer $renderer) {}

    public function handle(Request $request, Closure $next): Response
    {
        $response = $next($request);

        // Streamed and file responses have no content (false) and pass through untouched.
        $content = $response->getContent();
        if (str_starts_with((string) $response->headers->get('Content-Type'), 'text/html')
            && is_string($content) && $content !== '') {
            $response->setContent($this->renderer->render($content));
        }

        return $response;
    }
}
