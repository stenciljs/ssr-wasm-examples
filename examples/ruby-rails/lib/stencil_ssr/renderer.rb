require "json"

module StencilSsr
  # One instance of the ssr-wasm plugin. Not thread-safe: use it from one request at a time
  # (StencilSsr.pool takes care of that).
  class Renderer
    def initialize(compiled, logger:)
      @logger = logger
      started = clock
      @plugin = compiled.instantiate
      @logger.info { format("[ssr-wasm] plugin instantiated in %.0fms", (clock - started) * 1000) }
    end

    # Restart the hydration ids that fragment renders share (s-id, c-id, <!--r.N-->). At most once
    # per request, before its first fragment - never between fragments of one page.
    def reset!
      @plugin.call("resetSsrDocData", "")
    end

    # Render +html+ with the given render options (e.g. fullDocument: false).
    # Returns the rendered HTML, or +html+ unchanged if the plugin reports an error: the
    # components then still render on the client.
    def render(html, **options)
      html = html.to_str
      started = clock
      out = @plugin.call("renderToString", JSON.generate({ html: html, options: options }))
      # The SDK returns the output buffer as a binary (ASCII-8BIT) string.
      result = JSON.parse(out.force_encoding(Encoding::UTF_8))
      @logger.debug { format("[ssr-wasm] renderToString %p in %.1fms", options, (clock - started) * 1000) }

      if (error = result["__error"])
        @logger.error("[ssr-wasm] renderToString failed, serving un-rendered HTML: #{error}")
        html
      else
        result.fetch("html")
      end
    rescue Extism::Error => e
      # A trap leaves the instance in an unknown state: don't return it to the pool.
      @broken = true
      @logger.error("[ssr-wasm] plugin call failed, serving un-rendered HTML: #{e.message}")
      html
    end

    def broken?
      @broken == true
    end

    private

    def clock
      Process.clock_gettime(Process::CLOCK_MONOTONIC)
    end
  end
end
