require "connection_pool"
require_relative "stencil_ssr/compiled_plugin"
require_relative "stencil_ssr/renderer"
require_relative "stencil_ssr/session"
require_relative "stencil_ssr/middleware"

# Server-side rendering of Stencil components through the `ssr-wasm` Extism plugin.
#
# Lifecycle (see README):
#   * the .wasm is compiled once per process (after Puma forks, never in the master);
#   * plugin instances (Renderer) are created from it on demand and live in a ConnectionPool
#     sized to Puma's thread count, so a concurrent request never shares an instance;
#   * a request checks out one instance the first time it renders anything, uses it for every
#     render in that request (resetting fragment hydration ids once, before the first fragment),
#     and returns it when the response is done (StencilSsr::Middleware).
module StencilSsr
  ENV_KEY = "stencil_ssr.session"

  class << self
    attr_reader :logger

    def configure(wasm_path:, pool_size:, logger:, checkout_timeout: 10)
      @wasm_path = wasm_path
      @logger = logger
      @compile_lock = Mutex.new
      @pool = ConnectionPool.new(size: pool_size, timeout: checkout_timeout) do
        Renderer.new(compiled, logger: logger)
      end
    end

    # The compiled plugin for this process. Compiled on first use; a forked child compiles its own.
    def compiled
      @compile_lock.synchronize do
        if @compiled_pid != Process.pid
          started = Process.clock_gettime(Process::CLOCK_MONOTONIC)
          @compiled = CompiledPlugin.new(File.binread(@wasm_path), wasi: true)
          @compiled_pid = Process.pid
          logger.info do
            format("[ssr-wasm] plugin compiled in %.0fms",
                   (Process.clock_gettime(Process::CLOCK_MONOTONIC) - started) * 1000)
          end
        end
        @compiled
      end
    end

    # Compile the plugin and create the first instance now, rather than in the first request.
    # (Further instances are created from the compiled plugin on demand, which is cheap.)
    # Called by config/puma.rb once the serving process is up (after fork, in cluster mode).
    def warm_up
      pool.with { |_renderer| }
    rescue => e
      logger.error("[ssr-wasm] warm-up failed: #{e.class}: #{e.message}")
    end

    def pool
      @pool or raise "StencilSsr.configure hasn't been called"
    end

    # The rendering session for the current request (created on first use).
    def session(env)
      env[ENV_KEY] ||= Session.new(pool)
    end
  end
end
