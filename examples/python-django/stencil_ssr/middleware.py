from .ssr import REQUEST_ATTR, for_request


class StencilSSRMiddleware:
    """
    Renders every complete ``text/html`` response with ``renderToString`` as a full document.

    Skips responses that already used ``{% stencil %}`` (so components aren't rendered twice),
    and non-HTML, streaming and compressed responses.
    """

    def __init__(self, get_response):
        self.get_response = get_response

    def __call__(self, request):
        response = self.get_response(request)

        helper = getattr(request, REQUEST_ATTR, None)
        if helper is not None and helper.fragments:
            return response  # {% stencil %} already rendered the components
        if (
            response.streaming
            or not response.get("Content-Type", "").startswith("text/html")
            or response.has_header("Content-Encoding")
            or not response.content
        ):
            return response

        charset = response.charset
        html = for_request(request).render(response.content.decode(charset))
        response.content = html.encode(charset)
        if response.has_header("Content-Length"):
            response["Content-Length"] = str(len(response.content))
        return response
