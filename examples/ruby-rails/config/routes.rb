Rails.application.routes.draw do
  # Middleware mode: a normal view; StencilSsr::Middleware renders the whole response.
  root "pages#middleware"

  # Helper mode: the view wraps each component in `<%= stencil do %>`.
  get "helper", to: "pages#helper"

  # Plain Rack endpoint: never touches the plugin.
  get "health", to: ->(_env) { [200, { "content-type" => "text/plain" }, ["ok"]] }

  # The Stencil client bundle. Rack::Files guards against path traversal and sets content types.
  mount Rack::Files.new(Rails.configuration.x.stencil_ssr.client_dir,
                        { "cache-control" => "public, max-age=3600" }), at: "/stencil"
end
