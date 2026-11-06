require_relative "boot"

require "rails"
# Only the frameworks this app uses: no Active Record, Action Mailer, Active Job, Active Storage,
# Action Cable or asset pipeline.
require "action_controller/railtie"
require "action_view/railtie"

Bundler.require(*Rails.groups)

require_relative "../lib/stencil_ssr"

module StencilSsrExample
  class Application < Rails::Application
    config.load_defaults 8.1

    # Where the two build artifacts live. Both default to the monorepo checkout so the app runs
    # from a clone without any setup; the Dockerfile points them at copies inside the image.
    repo = File.expand_path("../../..", __dir__)
    config.x.stencil_ssr.wasm_path =
      ENV.fetch("SSR_WASM_PATH") { File.join(repo, "packages/components/dist/ssr-wasm/index.wasm") }
    config.x.stencil_ssr.client_dir =
      ENV.fetch("STENCIL_CLIENT_DIR") { File.join(repo, "packages/components/dist/loader-bundle/demo-components") }
    # One plugin instance per Puma thread (see config/puma.rb and lib/stencil_ssr.rb).
    config.x.stencil_ssr.pool_size = Integer(ENV.fetch("RAILS_MAX_THREADS", 3))

    # Middleware mode: post-process every text/html response through renderToString.
    # `use` appends it to the end of the stack, so it sits inside Rack::ETag / ConditionalGet
    # (they see the rendered body) and after ActionDispatch::Static (static files never reach it).
    config.middleware.use StencilSsr::Middleware

    # The app keeps no sessions or signed cookies, so a per-boot secret is fine for this demo.
    config.secret_key_base = ENV.fetch("SECRET_KEY_BASE") { SecureRandom.hex(64) }
  end
end
