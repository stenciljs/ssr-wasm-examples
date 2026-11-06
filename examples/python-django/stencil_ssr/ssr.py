"""
Thin wrapper around Stencil's ``ssr-wasm`` Extism plugin.

Lifecycle (see CONTRACT.md and the README):

* The ``.wasm`` is compiled **once per process** (``CompiledPlugin``), lazily or from gunicorn's
  ``post_worker_init`` hook. Compiling is the expensive part.
* Each **thread** gets its own plugin instance (``threading.local``), created from the compiled
  module on first use. Instances aren't thread-safe; with gunicorn's ``gthread`` worker a request
  runs start-to-finish on one thread, so a thread's instance is only ever used by one request
  at a time.
* ``for_request(request)`` hands out a per-request handle. It calls ``resetSsrDocData`` once,
  before the request's first fragment, so every fragment rendered during that request shares one
  hydration-ID sequence that starts again at 1 on the next request. (Full-document renders
  always start from 1 by themselves.)
"""

from __future__ import annotations

import json
import logging
import threading
import time
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
                path = Path(settings.SSR_WASM_PATH)
                t0 = time.perf_counter()
                _compiled = CompiledPlugin(path.read_bytes(), wasi=True)
                log.info("compiled %s in %.0f ms", path, (time.perf_counter() - t0) * 1000)
    return _compiled


class StencilRenderer:
    """One plugin instance. Not thread-safe; get one through ``for_request``."""

    def __init__(self) -> None:
        compiled = compiled_plugin()
        t0 = time.perf_counter()
        self._plugin = extism.Plugin(compiled)
        log.info("instantiated plugin in %.1f ms", (time.perf_counter() - t0) * 1000)

    def reset(self) -> None:
        self._plugin.call("resetSsrDocData", b"")

    def render_to_string(self, html: str, **options) -> str:
        """Render ``html``; on an ``__error`` result, log it and return ``html`` unchanged."""
        payload = json.dumps({"html": html, "options": options})
        result = json.loads(self._plugin.call("renderToString", payload))
        if "__error" in result:
            log.error("renderToString failed, serving un-rendered HTML: %s", result["__error"])
            return html
        errors = [d for d in result.get("diagnostics") or [] if d.get("level") == "error"]
        for d in errors:
            log.error("renderToString diagnostic: %s: %s", d.get("header"), d.get("messageText"))
        if not isinstance(result.get("html"), str):
            # e.g. invalid input: Stencil reports it as an error diagnostic with `html: null`
            # rather than `__error`.
            log.error("renderToString returned no HTML, serving un-rendered HTML")
            return html
        return result["html"]


def _thread_renderer() -> StencilRenderer:
    renderer = getattr(_local, "renderer", None)
    if renderer is None:
        renderer = _local.renderer = StencilRenderer()
    return renderer


class RequestRenderer:
    """This thread's plugin, for the current request. Tracks what the request rendered.

    Hydration IDs: full-document renders always number them from 1. Fragments share one ID
    sequence in the plugin (so the fragments of a page never collide), which keeps counting across
    requests unless reset. That's harmless, but resetting once, before the request's first
    fragment and never between fragments, gives the same output for the same page.
    """

    def __init__(self) -> None:
        self._renderer: StencilRenderer | None = _thread_renderer()
        self.fragments = 0
        self.seconds = 0.0

    def render(self, html: str, **options) -> str:
        if self._renderer is None:  # the instance trapped earlier in this request
            return html
        t0 = time.perf_counter()
        try:
            return self._renderer.render_to_string(html, **options)
        except extism.Error:
            # A trap (as opposed to an `__error` result) may leave the instance in a bad state:
            # drop it so this thread instantiates a fresh one on its next request.
            log.exception("plugin call failed, serving un-rendered HTML")
            self._renderer = _local.renderer = None
            return html
        finally:
            self.seconds += time.perf_counter() - t0

    def render_fragment(self, html: str) -> str:
        if self.fragments == 0 and self._renderer is not None:
            try:
                self._renderer.reset()
            except extism.Error:
                log.exception("resetSsrDocData failed, serving un-rendered HTML")
                self._renderer = _local.renderer = None
        self.fragments += 1
        return self.render(html, fullDocument=False)


def for_request(request) -> RequestRenderer:
    """The request's renderer, created on first use and shared by everything the request renders."""
    renderer = getattr(request, REQUEST_ATTR, None)
    if renderer is None:
        renderer = RequestRenderer()
        setattr(request, REQUEST_ATTR, renderer)
    return renderer
