class PagesController < ApplicationController
  FRAMEWORK = "Rails".freeze

  # Deliberately includes <, >, & and quotes: ERB escapes the JSON into the attribute.
  TASKS = [
    { title: "Write components", done: true },
    { title: 'Render with <wasm> & "friends"', done: false },
    { title: "Hydrate on the client" },
  ].freeze

  before_action { @framework = FRAMEWORK; @tasks = TASKS }

  # GET / - rendered whole by StencilSsr::Middleware.
  def middleware; end

  # GET /helper - each component rendered by StencilHelper#stencil.
  def helper; end
end
