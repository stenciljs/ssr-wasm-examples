require "json"
require "connection_pool"
require_relative "stencil_ssr/compiled_plugin"

# Server-side rendering of Stencil components through the `ssr-wasm` Extism plugin.
#
#   * The .wasm is compiled once per process (config/puma.rb does it at boot, after any fork).
#   * Instances are created from it on demand and kept in a ConnectionPool sized to Puma's thread
#     count. A request checks one out on its first render and Middleware checks it back in, so
#     concurrent requests never share one.
module StencilSsr
  PLUGIN = "stencil_ssr.plugin"       # the request's checked-out instance
  FRAGMENTS = "stencil_ssr.fragments" # true once the request rendered a fragment
  BROKEN = "stencil_ssr.broken"       # true if a call raised: drop the instance

  class << self
    def configure(wasm_path:, pool_size:)
      @wasm_path = wasm_path
      @compile_lock = Mutex.new
      # The timeout covers requests that arrive while the plugin is still compiling at boot.
      @pool = ConnectionPool.new(size: pool_size, timeout: 10) { compiled.instantiate }
    end

    # Compile the plugin and create the first instance now, rather than in the first request.
    def warm_up
      @pool.with { |_plugin| }
    end

    # Middleware mode: the whole page, with default options (fullDocument: true).
    def render_document(env, html)
      render(env, html, {})
    end

    # Helper mode: one component. All fragments of a page share one hydration-id sequence in the
    # instance, so it's reset before the request's first fragment only, never between fragments.
    def render_fragment(env, html)
      reset = !env[FRAGMENTS]
      env[FRAGMENTS] = true
      render(env, html, { fullDocument: false }, reset: reset)
    end

    # Return the request's instance to the pool, or drop it if a call raised.
    def release(env)
      return unless env[PLUGIN]

      @pool.discard_current_connection if env[BROKEN]
      @pool.checkin
    end

    private

    def compiled
      @compile_lock.synchronize do
        @compiled ||= CompiledPlugin.new(File.binread(@wasm_path), wasi: true)
      end
    end

    # Returns the rendered HTML, or +html+ unchanged if rendering fails: the components then
    # still render on the client.
    def render(env, html, options, reset: false)
      plugin = (env[PLUGIN] ||= @pool.checkout)
      plugin.call("resetSsrDocData", "") if reset
      out = plugin.call("renderToString", JSON.generate({ html: html, options: options }))
      # The SDK returns a binary (ASCII-8BIT) string.
      result = JSON.parse(out.force_encoding(Encoding::UTF_8))
      return result["html"] unless result["__error"]

      Rails.logger.error("[ssr-wasm] renderToString failed: #{result["__error"]}")
      html
    rescue Extism::Error => e
      # A trap leaves the instance in an unknown state: don't reuse it.
      env[BROKEN] = true
      Rails.logger.error("[ssr-wasm] plugin call failed: #{e.message}")
      html
    end
  end

  # Middleware mode: renders every text/html response as a whole document (unless the template
  # used the helper), then returns the request's instance to the pool.
  class Middleware
    def initialize(app)
      @app = app
    end

    def call(env)
      status, headers, body = @app.call(env)

      if status == 200 && headers["content-type"].to_s.start_with?("text/html") && !env[FRAGMENTS]
        html = +""
        body.each { |chunk| html << chunk }
        body.close if body.respond_to?(:close)
        html = StencilSsr.render_document(env, html.force_encoding(Encoding::UTF_8))
        headers["content-length"] = html.bytesize.to_s
        body = [html]
      end

      [status, headers, body]
    ensure
      StencilSsr.release(env)
    end
  end
end
