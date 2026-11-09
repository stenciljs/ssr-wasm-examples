"""
Wrapper around Stencil's ``ssr-wasm`` Extism plugin.

* The ``.wasm`` is compiled once per process (``compiled_plugin``). Compiling is the slow part.
* Each thread gets its own plugin instance (``threading.local``), because instances aren't
  thread-safe. With gunicorn's ``gthread`` worker a request stays on one thread.
* ``for_request(request)`` returns the request's ``Renderer``. It calls ``resetSsrDocData`` once,
  before the request's first fragment, so the fragments of one page share one hydration-ID
  sequence. (Full-document renders number IDs from 1 by themselves.)
"""

from __future__ import annotations

import json
import logging
import threading
from pathlib import Path

import extism
from django.conf import settings

# extism 1.1.1 doesn't re-export CompiledPlugin from the package root.
from extism.extism import CompiledPlugin

log = logging.getLogger(__name__)

_compile_lock = threading.Lock()
_compiled: CompiledPlugin | None = None
_local = threading.local()

REQUEST_ATTR = "_stencil_ssr"


def compiled_plugin() -> CompiledPlugin:
    """The process-wide compiled module (thread-safe to share; instances are not)."""
    global _compiled
    if _compiled is None:
        with _compile_lock:
            if _compiled is None:
                _compiled = CompiledPlugin(Path(settings.SSR_WASM_PATH).read_bytes(), wasi=True)
    return _compiled


class Renderer:
    """This thread's plugin instance, used by one request."""

    def __init__(self) -> None:
        if getattr(_local, "plugin", None) is None:
            _local.plugin = extism.Plugin(compiled_plugin())
        self.plugin: extism.Plugin | None = _local.plugin
        self.fragments = 0  # fragments rendered so far; the middleware skips the page if > 0

    def render(self, html: str, fragment: bool = False) -> str:
        """Render ``html`` (a whole document, or one fragment). On any failure, return it unchanged
        so the browser renders the components instead."""
        if self.plugin is None:  # the instance failed earlier in this request
            return html
        options = {}
        try:
            if fragment:
                if self.fragments == 0:
                    self.plugin.call("resetSsrDocData", b"")
                self.fragments += 1
                options = {"fullDocument": False}
            payload = json.dumps({"html": html, "options": options})
            result = json.loads(self.plugin.call("renderToString", payload))
        except extism.Error:
            # A trap may leave the instance unusable: drop it so this thread makes a fresh one on
            # its next request.
            log.exception("ssr-wasm call failed, serving un-rendered HTML")
            self.plugin = _local.plugin = None
            return html
        # `__error` for a failed render; invalid input comes back as `html: null` with diagnostics.
        if not isinstance(result.get("html"), str):
            log.error(
                "renderToString failed, serving un-rendered HTML: %s",
                result.get("__error") or result.get("diagnostics"),
            )
            return html
        return result["html"]


def for_request(request) -> Renderer:
    """The request's renderer, created on first use and shared by everything the request renders."""
    renderer = getattr(request, REQUEST_ATTR, None)
    if renderer is None:
        renderer = Renderer()
        setattr(request, REQUEST_ATTR, renderer)
    return renderer
