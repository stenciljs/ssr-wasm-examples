module StencilSsr
  # Everything one request renders. Checks a Renderer out of the pool on first use, so every
  # render in the request (the whole document, or each helper fragment) shares one instance.
  # Released by StencilSsr::Middleware.
  #
  # Hydration ids: full-document renders always number them from 1. Fragments share one id
  # sequence in the plugin (so the fragments of a page never collide), which keeps counting
  # across requests unless reset. That's harmless, but resetting once - before the request's
  # first fragment, never between fragments - gives the same output for the same page.
  class Session
    def initialize(pool)
      @pool = pool
      @renderer = nil
      @fragments = false
    end

    # Middleware mode: the whole response, with default options (fullDocument: true).
    def render_document(html)
      renderer.render(html)
    end

    # Helper mode: one component's markup, inlined into a template the plugin never sees.
    def render_fragment(html, **options)
      renderer.reset! unless @fragments
      @fragments = true
      renderer.render(html, fullDocument: false, **options)
    end

    # Did a template render fragments? Then the page must not be post-processed as a whole.
    def fragments?
      @fragments
    end

    def release
      return unless @renderer

      @pool.discard_current_connection if @renderer.broken?
      @pool.checkin
      @renderer = nil
    end

    private

    def renderer
      @renderer ||= @pool.checkout
    end
  end
end
